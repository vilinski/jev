//! Translation between System One requests and `OpenAI`'s Decisions API.
//!
//! `POST /v1/decisions` takes shared evidence in `input` and an array of
//! `questions` (`predicate`, `choice`, `score`), and returns an `answers`
//! array. This module maps a System One request (`state`, `questions` object)
//! onto that shape and the reply back onto the `answers` object every other
//! provider returns.

use serde_json::{Map, Value, json};

use crate::Error;
use crate::util::{render_state, round4};

/// Builds the `/v1/decisions` request body for a System One request.
pub fn request(body: &Value, model: &str) -> Result<Value, Error> {
    let questions = body
        .get("questions")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Usage("the request needs a `questions` object".to_owned()))?;
    let mut translated = Vec::with_capacity(questions.len());
    for (id, question) in questions {
        translated.push(question_to_decisions(id, question)?);
    }
    Ok(json!({
        "model": model,
        "input": render_state(body.get("state").unwrap_or(&Value::Null)),
        "questions": translated,
    }))
}

/// Maps one System One question onto a Decisions API question.
fn question_to_decisions(id: &str, question: &Value) -> Result<Value, Error> {
    let instructions = question
        .get("instructions")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Usage(format!("question `{id}` needs an `instructions` string")))?;
    let criteria = question.get("criteria");
    match question.get("type").and_then(Value::as_str) {
        Some("noul") => Ok(json!({"type": "predicate", "name": id, "instructions": instructions})),
        Some("choice") => {
            let options = criteria.and_then(Value::as_object).ok_or_else(|| {
                Error::Usage(format!(
                    "choice question `{id}` needs `criteria` as an object"
                ))
            })?;
            let choices: Vec<Value> = options
                .iter()
                .map(|(name, description)| {
                    json!({"value": name, "description": description.as_str().unwrap_or(name)})
                })
                .collect();
            Ok(
                json!({"type": "choice", "name": id, "instructions": instructions, "choices": choices}),
            )
        }
        Some("score") => {
            let levels: Vec<Value> = criteria
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    Error::Usage(format!("score question `{id}` needs `criteria` as an array"))
                })?
                .iter()
                .enumerate()
                .map(|(index, text)| {
                    json!({"label": index.to_string(), "description": text.as_str().unwrap_or_default()})
                })
                .collect();
            Ok(json!({"type": "score", "name": id, "instructions": instructions, "levels": levels}))
        }
        other => Err(Error::Usage(format!(
            "unsupported question type {other:?} for `{id}`; use noul, choice or score"
        ))),
    }
}

/// Turns a Decisions API reply into the System One response shape.
pub fn response(reply: &Value, body: &Value) -> Result<Value, Error> {
    let answers = reply
        .get("answers")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Usage("the Decisions API reply has no `answers` array".to_owned()))?;
    let mut out = Map::new();
    for answer in answers {
        let name = answer
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Usage("an answer has no `name`".to_owned()))?;
        let converted = match answer.get("type").and_then(Value::as_str) {
            Some("predicate") => json!({
                "type": "noul",
                "noul": answer.get("probability").and_then(Value::as_f64).map(round4),
            }),
            Some("choice") => {
                let probabilities: Map<String, Value> = prob_entries(answer)
                    .filter_map(|entry| {
                        let value = entry.get("value")?.as_str()?;
                        let p = entry.get("probability")?.as_f64()?;
                        Some((value.to_owned(), json!(round4(p))))
                    })
                    .collect();
                json!({
                    "type": "choice",
                    "choice": answer.get("choice"),
                    "confidence": answer.get("confidence"),
                    "probabilities": probabilities,
                })
            }
            Some("score") => {
                let levels = body
                    .get("questions")
                    .and_then(|questions| questions.get(name))
                    .and_then(|question| question.get("criteria"))
                    .and_then(Value::as_array);
                let probabilities: Map<String, Value> = prob_entries(answer)
                    .enumerate()
                    .map(|(index, entry)| {
                        let p = entry
                            .get("probability")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0);
                        (index.to_string(), json!(round4(p)))
                    })
                    .collect();
                let legend: Map<String, Value> = levels
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .map(|(index, text)| (index.to_string(), text.clone()))
                    .collect();
                json!({
                    "type": "score",
                    "score": answer.get("score"),
                    "confidence": answer.get("confidence"),
                    "probabilities": probabilities,
                    "legend": legend,
                })
            }
            Some("refusal") => json!({"type": "refusal"}),
            other => {
                return Err(Error::Usage(format!(
                    "unexpected answer type {other:?} for `{name}`"
                )));
            }
        };
        out.insert(name.to_owned(), converted);
    }
    let mut result = Map::from_iter([("answers".to_owned(), Value::Object(out))]);
    for key in ["usage", "model"] {
        if let Some(value) = reply.get(key) {
            result.insert(key.to_owned(), value.clone());
        }
    }
    Ok(Value::Object(result))
}

/// The entries of an answer's `probabilities` array, empty when it has none.
fn prob_entries(answer: &Value) -> impl Iterator<Item = &Value> {
    answer
        .get("probabilities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_the_three_question_types() {
        let body = json!({
            "state": "I was charged twice.",
            "questions": {
                "dept": {"type": "choice", "instructions": "Which?", "criteria": {"billing": "Money", "other": null}},
                "urgent": {"type": "noul", "instructions": "Urgent?"},
                "sev": {"type": "score", "instructions": "How bad?", "criteria": ["low", "high"]},
            }
        });
        let out = request(&body, "gpt-6-luna").expect("request");
        assert_eq!(out["model"], "gpt-6-luna");
        assert_eq!(out["input"], "I was charged twice.");
        let questions = out["questions"].as_array().expect("array");
        let by_name = |name: &str| {
            questions
                .iter()
                .find(|q| q["name"] == name)
                .expect("question present")
        };
        assert_eq!(by_name("urgent")["type"], "predicate");
        assert_eq!(by_name("dept")["choices"][1]["description"], "other");
        assert_eq!(by_name("sev")["levels"][1]["description"], "high");
    }

    #[test]
    fn converts_answers_back_to_the_system_one_shape() {
        let body = json!({"questions": {"sev": {"type": "score", "instructions": "x", "criteria": ["low", "high"]}}});
        let reply = json!({"answers": [
            {"name": "urgent", "type": "predicate", "probability": 0.91234},
            {"name": "dept", "type": "choice", "choice": "billing", "confidence": 0.8,
             "probabilities": [{"value": "billing", "probability": 0.9}, {"value": "other", "probability": 0.1}]},
            {"name": "sev", "type": "score", "score": 0.7, "confidence": 0.6,
             "probabilities": [{"value": 0, "label": "0", "probability": 0.3}, {"value": 1, "label": "1", "probability": 0.7}]},
            {"name": "skipped", "type": "refusal"},
        ]});
        let out = response(&reply, &body).expect("response");
        assert_eq!(out["answers"]["urgent"]["noul"], 0.9123);
        assert_eq!(out["answers"]["dept"]["probabilities"]["other"], 0.1);
        assert_eq!(out["answers"]["sev"]["probabilities"]["1"], 0.7);
        assert_eq!(out["answers"]["sev"]["legend"]["1"], "high");
        assert_eq!(out["answers"]["skipped"]["type"], "refusal");
    }
}
