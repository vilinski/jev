//! Adapter that answers System One questions with `OpenAI` chat completions.
//!
//! Used by `openai-chat`: each question becomes one chat completion whose
//! reply is a single token (`yes`/`no`, an option letter or a
//! level digit). The probabilities come from the token log-probabilities of
//! that one position, which gives the same distribution-shaped answer the
//! System One providers return. Needs a model that supports `logprobs`
//! (for example `gpt-4.1-mini`); reasoning models reject the parameter.

use std::fmt::Write as _;

use serde_json::{Map, Value, json};

use crate::Error;
use crate::util::round4;

/// How many `top_logprobs` to request; the API allows up to 20.
const TOP_LOGPROBS: u32 = 20;

/// Most options or levels a single answer token can name.
const MAX_CHOICES: usize = 26;

/// What kind of answer a prepared request expects.
#[derive(Debug)]
pub enum Kind {
    /// `yes` or `no`.
    Noul,
    /// One option; the names are in the order of the letters `A`, `B`, ...
    Choice(Vec<String>),
    /// One level; the descriptions are in ascending order, labelled `0`, `1`, ...
    Score(Vec<String>),
}

/// One question translated into a chat completion request.
#[derive(Debug)]
pub struct Prepared {
    /// Body for `POST /v1/chat/completions`.
    pub body: Value,
    /// How to read the answer.
    pub kind: Kind,
}

/// Letter that labels the option at `index` (`A`, `B`, ...).
fn letter(index: usize) -> char {
    char::from(b'A' + u8::try_from(index).unwrap_or(0))
}

/// Translates one System One question into a chat completion request.
pub fn prepare(model: &str, state: &str, question: &Value) -> Result<Prepared, Error> {
    let instructions = question
        .get("instructions")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Usage("a question needs an `instructions` string".to_owned()))?;
    let criteria = question.get("criteria");
    let (kind, answer_rule, options) = match question.get("type").and_then(Value::as_str) {
        Some("noul") => (
            Kind::Noul,
            "Reply with exactly one word: yes or no.".to_owned(),
            String::new(),
        ),
        Some("choice") => {
            let map = criteria.and_then(Value::as_object).ok_or_else(|| {
                Error::Usage("a choice question needs `criteria` as an object".to_owned())
            })?;
            if map.len() < 2 || map.len() > MAX_CHOICES {
                return Err(Error::Usage(format!(
                    "a choice question needs 2 to {MAX_CHOICES} options with this provider"
                )));
            }
            let mut listing = String::from("\n\nOptions:\n");
            let mut names = Vec::new();
            for (index, (name, description)) in map.iter().enumerate() {
                let _ = write!(listing, "{}) {name}", letter(index));
                if let Some(text) = description.as_str() {
                    let _ = write!(listing, ": {text}");
                }
                listing.push('\n');
                names.push(name.clone());
            }
            (
                Kind::Choice(names),
                "Reply with exactly one character: the letter of the best option.".to_owned(),
                listing,
            )
        }
        Some("score") => {
            let levels: Vec<String> = criteria
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    Error::Usage("a score question needs `criteria` as an array".to_owned())
                })?
                .iter()
                .map(|level| level.as_str().unwrap_or_default().to_owned())
                .collect();
            if levels.len() < 2 || levels.len() > 10 {
                return Err(Error::Usage(
                    "a score question needs 2 to 10 levels with this provider".to_owned(),
                ));
            }
            let mut listing = String::from("\n\nLevels, in ascending order:\n");
            for (index, level) in levels.iter().enumerate() {
                let _ = writeln!(listing, "{index}) {level}");
            }
            (
                Kind::Score(levels),
                "Reply with exactly one digit: the number of the best level.".to_owned(),
                listing,
            )
        }
        other => {
            return Err(Error::Usage(format!(
                "unsupported question type {other:?}; use noul, choice or score"
            )));
        }
    };

    let prompt = format!("State:\n{state}\n\nQuestion: {instructions}{options}\n\n{answer_rule}");
    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "You judge the given state. Answer only as instructed, with no explanation."},
            {"role": "user", "content": prompt},
        ],
        "max_completion_tokens": 1,
        "temperature": 0,
        "logprobs": true,
        "top_logprobs": TOP_LOGPROBS,
    });
    Ok(Prepared { body, kind })
}

/// Reads the probability mass the reply's first token puts on each label.
///
/// `slot` maps a token to the index of the label it names, or `None` for a
/// token that names none. The result is normalised over the labels.
fn distribution(
    response: &Value,
    slot: impl Fn(&str) -> Option<usize>,
    n: usize,
) -> Result<Vec<f64>, Error> {
    let top = response
        .pointer("/choices/0/logprobs/content/0/top_logprobs")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Error::Usage(
                "the model returned no logprobs; pick a model that supports them, e.g. gpt-4.1-mini"
                    .to_owned(),
            )
        })?;
    let mut mass = vec![0.0_f64; n];
    for entry in top {
        let token = entry
            .get("token")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let logprob = entry
            .get("logprob")
            .and_then(Value::as_f64)
            .unwrap_or(f64::NEG_INFINITY);
        if let Some(index) = slot(token.trim()) {
            mass[index] += logprob.exp();
        }
    }
    let total: f64 = mass.iter().sum();
    if total <= 0.0 {
        return Err(Error::Usage(
            "the model's first token named none of the answers".to_owned(),
        ));
    }
    Ok(mass.into_iter().map(|m| m / total).collect())
}

/// `1 - H/ln(n)`: 1 for a certain answer, 0 for a uniform one.
pub fn concentration(probabilities: &[f64]) -> f64 {
    let entropy: f64 = probabilities
        .iter()
        .filter(|p| **p > 0.0)
        .map(|p| -p * p.ln())
        .sum();
    // The counts are tiny, so the conversion to f64 is exact.
    #[allow(clippy::cast_precision_loss)]
    let max = (probabilities.len() as f64).ln();
    if max > 0.0 {
        (1.0 - entropy / max).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Turns a chat completion into the answer a System One provider would give.
pub fn interpret(prepared: &Prepared, response: &Value) -> Result<Value, Error> {
    match &prepared.kind {
        Kind::Noul => {
            let p = distribution(
                response,
                |t| match t.to_ascii_lowercase().as_str() {
                    "yes" => Some(0),
                    "no" => Some(1),
                    _ => None,
                },
                2,
            )?;
            Ok(json!({ "type": "noul", "noul": round4(p[0]) }))
        }
        Kind::Choice(names) => {
            let p = distribution(
                response,
                |t| {
                    let mut chars = t.chars();
                    let (first, rest) = (chars.next()?.to_ascii_uppercase(), chars.next());
                    let index = (first as usize).checked_sub('A' as usize)?;
                    (rest.is_none() && index < names.len()).then_some(index)
                },
                names.len(),
            )?;
            let best = p
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map_or(0, |(index, _)| index);
            let probabilities: Map<String, Value> = names
                .iter()
                .zip(&p)
                .map(|(name, prob)| (name.clone(), json!(round4(*prob))))
                .collect();
            Ok(json!({
                "type": "choice",
                "choice": names[best],
                "confidence": round4(concentration(&p)),
                "probabilities": probabilities,
            }))
        }
        Kind::Score(levels) => {
            let p = distribution(
                response,
                |t| {
                    let digit = t.parse::<usize>().ok()?;
                    (t.len() == 1 && digit < levels.len()).then_some(digit)
                },
                levels.len(),
            )?;
            let mut score = 0.0;
            for (index, prob) in p.iter().enumerate() {
                // At most ten levels, so the conversion is exact.
                #[allow(clippy::cast_precision_loss)]
                {
                    score += index as f64 * prob;
                }
            }
            let probabilities: Map<String, Value> = p
                .iter()
                .enumerate()
                .map(|(index, prob)| (index.to_string(), json!(round4(*prob))))
                .collect();
            let legend: Map<String, Value> = levels
                .iter()
                .enumerate()
                .map(|(index, level)| (index.to_string(), json!(level)))
                .collect();
            Ok(json!({
                "type": "score",
                "score": round4(score),
                "confidence": round4(concentration(&p)),
                "probabilities": probabilities,
                "legend": legend,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(tokens: &[(&str, f64)]) -> Value {
        let top: Vec<Value> = tokens
            .iter()
            .map(|(token, p)| json!({"token": token, "logprob": p.ln()}))
            .collect();
        json!({"choices": [{"logprobs": {"content": [{"top_logprobs": top}]}}]})
    }

    #[test]
    fn noul_merges_case_and_space_variants() {
        let prepared =
            prepare("m", "x", &json!({"type": "noul", "instructions": "Is it?"})).expect("prepare");
        let answer = interpret(
            &prepared,
            &reply(&[("Yes", 0.6), (" yes", 0.2), ("No", 0.1), ("maybe", 0.1)]),
        )
        .expect("interpret");
        // 0.8 of 0.9 usable mass.
        assert!((answer["noul"].as_f64().expect("number") - 0.8889).abs() < 1e-4);
    }

    #[test]
    fn choice_maps_letters_to_names() {
        let prepared = prepare(
            "m",
            "x",
            &json!({"type": "choice", "instructions": "Which?", "criteria": {"billing": "Money", "auth": null}}),
        )
        .expect("prepare");
        // Criteria are sorted by name: auth = A, billing = B.
        let answer = interpret(&prepared, &reply(&[("B", 0.7), ("A", 0.3)])).expect("interpret");
        assert_eq!(answer["choice"], "billing");
        assert_eq!(answer["probabilities"]["auth"], 0.3);
    }

    #[test]
    fn score_is_the_expected_level_and_keeps_the_legend() {
        let prepared = prepare(
            "m",
            "x",
            &json!({"type": "score", "instructions": "How bad?", "criteria": ["low", "mid", "high"]}),
        )
        .expect("prepare");
        let answer = interpret(&prepared, &reply(&[("1", 0.5), ("2", 0.5)])).expect("interpret");
        assert_eq!(answer["score"], 1.5);
        assert_eq!(answer["legend"]["2"], "high");
    }

    #[test]
    fn missing_logprobs_explain_how_to_fix_it() {
        let prepared =
            prepare("m", "x", &json!({"type": "noul", "instructions": "Is it?"})).expect("prepare");
        let error = interpret(&prepared, &json!({"choices": [{}]})).expect_err("no logprobs");
        assert!(error.to_string().contains("logprobs"));
    }

    #[test]
    fn rejects_unknown_question_types() {
        let result = prepare("m", "x", &json!({"type": "rank", "instructions": "q"}));
        assert!(result.is_err());
    }

    #[test]
    fn certain_answers_have_full_concentration() {
        assert!((concentration(&[1.0, 0.0]) - 1.0).abs() < 1e-9);
        assert!(concentration(&[0.5, 0.5]).abs() < 1e-9);
    }
}
