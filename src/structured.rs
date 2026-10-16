//! Adapts generative Celeris models to typed decisions with JSON output.
//!
//! These probabilities are model-reported judgments, unlike the distributions
//! produced by dedicated decision heads or token logprobs. Benchmarks must label
//! the adapter and include its generated output in cost and latency.

use serde_json::{Value, json};

use crate::Error;
use crate::question::{Kind, Prepared, answer, parse_kind};

/// Most options or levels one question may offer.
const MAX_OPTIONS: usize = 512;

/// Builds a JSON-schema completion for one typed question without an option-letter limit.
pub fn prepare(model: &str, state: &str, question: &Value) -> Result<Prepared, Error> {
    let kind = parse_kind(question, MAX_OPTIONS, MAX_OPTIONS)?;
    let number = json!({"type": "number", "minimum": 0, "maximum": 1});
    let labels = kind.labels();
    let (properties, required) = if labels.is_empty() {
        (json!({"noul": number}), json!(["noul"]))
    } else {
        let probs: serde_json::Map<String, Value> = labels
            .iter()
            .map(|label| (label.clone(), number.clone()))
            .collect();
        (
            json!({"probabilities": {"type":"object", "properties":probs,"required":labels,"additionalProperties":false}}),
            json!(["probabilities"]),
        )
    };
    let count = labels.len().max(1);
    // `Value`'s Display writes JSON, so the question is serialised without being cloned.
    let payload = format!(
        r#"{{"state":{},"question":{question}}}"#,
        Value::from(state)
    );
    let body = json!({
        "model": model,
        "messages": [
            {"role":"system","content":"Evaluate the question against the state. Return only the requested JSON. For noul, estimate the probability that the statement is true. For choice or score, estimate a probability for EVERY allowed option; probabilities must sum to one. Use the question's exact option keys, or zero-based level indices for score. Do not include an explanation."},
            {"role":"user","content":payload}
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
    let parsed: Value = serde_json::from_str(content)
        .map_err(|_| Error::Usage("the chat model returned invalid decision JSON".to_owned()))?;
    let values = if matches!(prepared.kind, Kind::Noul) {
        vec![probability(parsed.get("noul"))?]
    } else {
        let source = parsed
            .get("probabilities")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::Usage("missing decision probabilities".to_owned()))?;
        let mut values = prepared
            .kind
            .labels()
            .iter()
            .map(|label| probability(source.get(label)))
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
        values
    };
    Ok(answer(&prepared.kind, &values))
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
