//! Adapts generative Celeris models to typed decisions with JSON output.
//!
//! These probabilities are model-reported judgments, unlike the distributions
//! produced by dedicated decision heads or token logprobs. Benchmarks must label
//! the adapter and include its generated output in cost and latency.

use serde_json::{Value, json};

use crate::Error;
use crate::openai::{Kind, Prepared, concentration};
use crate::util::round4;

/// Builds a JSON-schema completion for one typed question without an option-letter limit.
pub fn prepare(model: &str, state: &str, question: &Value) -> Result<Prepared, Error> {
    let criteria = question.get("criteria");
    let kind = match question.get("type").and_then(Value::as_str) {
        Some("noul") => Kind::Noul,
        Some("choice") => Kind::Choice(
            criteria
                .and_then(Value::as_object)
                .ok_or_else(|| Error::Usage("a choice needs a criteria object".to_owned()))?
                .keys()
                .cloned()
                .collect(),
        ),
        Some("score") => Kind::Score(
            criteria
                .and_then(Value::as_array)
                .ok_or_else(|| Error::Usage("a score needs criteria levels".to_owned()))?
                .iter()
                .map(|v| v.as_str().unwrap_or_default().to_owned())
                .collect(),
        ),
        _ => return Err(Error::Usage("use noul, choice or score".to_owned())),
    };
    let number = json!({"type": "number", "minimum": 0, "maximum": 1});
    let (properties, required, count) = match &kind {
        Kind::Noul => (json!({"noul": number}), json!(["noul"]), 1),
        Kind::Choice(names) | Kind::Score(names) => {
            if names.len() < 2 || names.len() > 512 {
                return Err(Error::Usage("expected 2 to 512 allowed answers".to_owned()));
            }
            let keys: Vec<String> = match &kind {
                Kind::Score(_) => (0..names.len()).map(|i| i.to_string()).collect(),
                _ => names.clone(),
            };
            let probs: serde_json::Map<String, Value> = keys
                .iter()
                .map(|key| (key.clone(), number.clone()))
                .collect();
            (
                json!({"probabilities": {"type":"object", "properties":probs,"required":keys,"additionalProperties":false}}),
                json!(["probabilities"]),
                names.len(),
            )
        }
    };
    let body = json!({
        "model": model,
        "messages": [
            {"role":"system","content":"Evaluate the question against the state. Return only the requested JSON. For noul, estimate the probability that the statement is true. For choice or score, estimate a probability for EVERY allowed option; probabilities must sum to one. Use the question's exact option keys, or zero-based level indices for score. Do not include an explanation."},
            {"role":"user","content":json!({"state":state,"question":question}).to_string()}
        ],
        "temperature": 0,
        "seed": 7,
        "chat_template_kwargs": {"enable_thinking":false},
        "max_tokens": count.saturating_mul(20).saturating_add(128).clamp(512, 8192),
        "response_format": {"type":"json_schema","json_schema":{"name":"decision","strict":true,"schema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}}
    });
    Ok(Prepared { body, kind })
}

/// Validates model-reported probabilities and returns the common typed answer shape.
pub fn interpret(prepared: &Prepared, response: &Value) -> Result<Value, Error> {
    let content = response
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Usage("the chat model returned no answer content".to_owned()))?;
    let answer: Value = serde_json::from_str(content)
        .map_err(|_| Error::Usage("the chat model returned invalid decision JSON".to_owned()))?;
    if matches!(prepared.kind, Kind::Noul) {
        let p = probability(answer.get("noul"))?;
        return Ok(json!({"type":"noul","noul":round4(p)}));
    }
    let names: Vec<String> = match &prepared.kind {
        Kind::Choice(names) => names.clone(),
        Kind::Score(levels) => (0..levels.len()).map(|i| i.to_string()).collect(),
        Kind::Noul => unreachable!(),
    };
    let source = answer
        .get("probabilities")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Usage("missing decision probabilities".to_owned()))?;
    let mut values = names
        .iter()
        .map(|name| probability(source.get(name)))
        .collect::<Result<Vec<_>, _>>()?;
    let sum: f64 = values.iter().sum();
    if !(0.99..=1.01).contains(&sum) {
        return Err(Error::Usage(
            "decision probabilities must sum to one".to_owned(),
        ));
    }
    for value in &mut values {
        *value /= sum;
    }
    let distribution: serde_json::Map<String, Value> = names
        .iter()
        .zip(&values)
        .map(|(name, p)| (name.clone(), json!(p)))
        .collect();
    let confidence = round4(concentration(&values));
    match &prepared.kind {
        Kind::Choice(_) => {
            let best = values
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .ok_or_else(|| Error::Usage("empty decision probabilities".to_owned()))?
                .0;
            Ok(
                json!({"type":"choice","choice":names[best],"probabilities":distribution,"confidence":confidence}),
            )
        }
        Kind::Score(levels) => {
            let score: f64 = values
                .iter()
                .enumerate()
                .map(|(i, p)| f64::from(u32::try_from(i).unwrap_or(u32::MAX)) * p)
                .sum();
            let legend: serde_json::Map<String, Value> = names
                .iter()
                .zip(levels)
                .map(|(name, level)| (name.clone(), json!(level)))
                .collect();
            Ok(
                json!({"type":"score","score":round4(score),"probabilities":distribution,"confidence":confidence,"legend":legend}),
            )
        }
        Kind::Noul => unreachable!(),
    }
}

/// Reads a finite probability in the closed unit interval.
fn probability(value: Option<&Value>) -> Result<f64, Error> {
    value
        .and_then(Value::as_f64)
        .filter(|p| p.is_finite() && (0.0..=1.0).contains(p))
        .ok_or_else(|| Error::Usage("missing or invalid decision probability".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_keeps_the_level_legend_and_returns_the_expected_value() {
        let prepared = prepare(
            "celeris-1",
            "Production is down",
            &json!({"type":"score","instructions":"How severe?","criteria":["minor","major"]}),
        )
        .expect("prepare");
        let response =
            json!({"choices":[{"message":{"content":r#"{"probabilities":{"0":0.1,"1":0.9}}"#}}]});
        let answer = interpret(&prepared, &response).expect("interpret");
        assert_eq!(answer["type"], "score");
        assert_eq!(answer["score"], 0.9);
        assert_eq!(answer["probabilities"], json!({"0":0.1,"1":0.9}));
        assert_eq!(answer["legend"], json!({"0":"minor","1":"major"}));
    }
}
