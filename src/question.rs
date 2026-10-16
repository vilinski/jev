//! Question and answer types shared by the adapters that turn a generative
//! model into a System One provider ([`crate::openai`], [`crate::structured`]).

use serde_json::{Map, Value, json};

use crate::Error;
use crate::util::round4;

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

impl Kind {
    /// Keys of the possible answers: option names, or level indices for a score.
    pub fn labels(&self) -> Vec<String> {
        match self {
            Self::Noul => Vec::new(),
            Self::Choice(names) => names.clone(),
            Self::Score(levels) => (0..levels.len()).map(|index| index.to_string()).collect(),
        }
    }
}

/// One question translated into a chat completion request.
#[derive(Debug)]
pub struct Prepared {
    /// Body for `POST /v1/chat/completions`.
    pub body: Value,
    /// How to read the answer.
    pub kind: Kind,
}

/// Parses a question's `type` and `criteria` into a [`Kind`].
///
/// A choice may offer 2 to `max_choices` options and a score 2 to `max_levels`
/// levels, because adapters differ in how many answers they can express.
pub fn parse_kind(question: &Value, max_choices: usize, max_levels: usize) -> Result<Kind, Error> {
    let criteria = question.get("criteria");
    match question.get("type").and_then(Value::as_str) {
        Some("noul") => Ok(Kind::Noul),
        Some("choice") => {
            let map = criteria.and_then(Value::as_object).ok_or_else(|| {
                Error::Usage("a choice question needs `criteria` as an object".to_owned())
            })?;
            if !(2..=max_choices).contains(&map.len()) {
                return Err(Error::Usage(format!(
                    "a choice question needs 2 to {max_choices} options with this provider"
                )));
            }
            Ok(Kind::Choice(map.keys().cloned().collect()))
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
            if !(2..=max_levels).contains(&levels.len()) {
                return Err(Error::Usage(format!(
                    "a score question needs 2 to {max_levels} levels with this provider"
                )));
            }
            Ok(Kind::Score(levels))
        }
        other => Err(Error::Usage(format!(
            "unsupported question type {other:?}; use noul, choice or score"
        ))),
    }
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

/// Shapes a probability per answer (one entry, `P(true)`, for a noul) into a typed answer.
pub fn answer(kind: &Kind, p: &[f64]) -> Value {
    let probabilities = |labels: Vec<String>| -> Map<String, Value> {
        labels
            .into_iter()
            .zip(p)
            .map(|(label, prob)| (label, json!(round4(*prob))))
            .collect()
    };
    match kind {
        Kind::Noul => json!({ "type": "noul", "noul": round4(p[0]) }),
        Kind::Choice(names) => {
            let best = p
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map_or(0, |(index, _)| index);
            json!({
                "type": "choice",
                "choice": names[best],
                "confidence": round4(concentration(p)),
                "probabilities": probabilities(kind.labels()),
            })
        }
        Kind::Score(levels) => {
            let mut score = 0.0;
            for (index, prob) in p.iter().enumerate() {
                // At most 512 levels, so the conversion is exact.
                #[allow(clippy::cast_precision_loss)]
                {
                    score += index as f64 * prob;
                }
            }
            let legend: Map<String, Value> = kind
                .labels()
                .into_iter()
                .zip(levels)
                .map(|(label, level)| (label, json!(level)))
                .collect();
            json!({
                "type": "score",
                "score": round4(score),
                "confidence": round4(concentration(p)),
                "probabilities": probabilities(kind.labels()),
                "legend": legend,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certain_answers_have_full_concentration() {
        assert!((concentration(&[1.0, 0.0]) - 1.0).abs() < 1e-9);
        assert!(concentration(&[0.5, 0.5]).abs() < 1e-9);
    }
}
