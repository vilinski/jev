//! Thin HTTP client for System One endpoints (`TypeSafe` Jev, Cloudflare Clef,
//! a local Ollama).
//!
//! Retries transient failures with exponential backoff and jitter, honouring a
//! `retry-after` header when the server sends one. The API's own docs call for
//! this: rate limits adjust dynamically and a burst can return `429` or `529`
//! at any time, so a client without backoff loses work that would have
//! succeeded a second later.
//!
//! See <https://docs.typesafe.ai/api> for the request and response shapes.
//! Cloudflare wraps the same answers in a `result` envelope, which
//! [`unwrap_envelope`] removes so callers see one shape.

use std::borrow::Cow;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Method;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Value, json};

use crate::Error;
use crate::decisions;
use crate::key::Secret;
use crate::openai;
use crate::provider::{Provider, Wire};
use crate::telemetry;
use crate::util;

/// First backoff delay; doubles per attempt.
const BACKOFF_INITIAL: Duration = Duration::from_millis(500);

/// Ceiling for a single backoff delay, before jitter.
const BACKOFF_MAX: Duration = Duration::from_secs(5);

/// Ceiling for a server-supplied `retry-after`, so a long one cannot hang a
/// shell pipeline indefinitely.
const RETRY_AFTER_MAX: u64 = 60;

/// A successful response plus the request id that produced it.
pub struct Response {
    /// The decoded JSON body.
    pub body: Value,
    /// Value of the provider's request id header, when the server sent one.
    pub request_id: Option<String>,
}

/// One HTTP attempt, including the response body read and its retry metadata.
struct HttpResponse {
    status: u16,
    request_id: Option<String>,
    retry_after: Option<u64>,
    text: String,
}

/// A configured client. Owns the secret so callers never handle it directly.
pub struct Client {
    http: reqwest::Client,
    provider: Provider,
    base: String,
    account: Option<String>,
    secret: Option<Secret>,
    max_retries: u32,
    telemetry_content: bool,
}

impl Client {
    /// Builds a client with the `Authorization` header baked into the defaults,
    /// so the secret is never assembled at a call site. Without a secret (a
    /// local server) no `Authorization` header is sent.
    pub fn new(
        provider: Provider,
        base: &str,
        secret: Option<Secret>,
        account: Option<String>,
        timeout_secs: u64,
        max_retries: u32,
    ) -> Result<Self, Error> {
        let mut headers = HeaderMap::new();
        if let Some(secret) = &secret {
            let mut auth = HeaderValue::from_str(&format!("Bearer {}", secret.expose()))
                .map_err(|_ignored| Error::InvalidKey)?;
            auth.set_sensitive(true);
            headers.insert(AUTHORIZATION, auth);
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(Error::Http)?;

        Ok(Self {
            http,
            provider,
            base: base.trim_end_matches('/').to_owned(),
            account,
            secret,
            max_retries,
            telemetry_content: false,
        })
    }

    /// Enables explicit content capture; metadata is recorded independently.
    pub const fn with_telemetry_content(mut self, enabled: bool) -> Self {
        self.telemetry_content = enabled;
        self
    }

    /// Where the credential came from, safe to print.
    pub fn key_source(&self) -> &str {
        self.secret
            .as_ref()
            .map_or("none needed", |secret| secret.source.as_str())
    }

    /// Lists the models, from the provider's listing endpoint or, for
    /// Cloudflare which has none, from the documented set.
    #[tracing::instrument(name = "jev.models", skip_all)]
    pub async fn models(&self) -> Result<Response, Error> {
        if let Some(models) = self.provider.static_models() {
            return Ok(Response {
                body: json!({ "models": models, "note": "static list; this provider has no listing endpoint" }),
                request_id: None,
            });
        }
        let url = self
            .provider
            .models_url(&self.base)
            .ok_or_else(|| Error::Usage("this provider cannot list models".to_owned()))?;
        self.execute(&Method::GET, &url, None).await
    }

    /// Sends a System One request for `model`.
    #[tracing::instrument(
        name = "jev.system_one", skip_all,
        fields(
            langfuse.observation.type = "generation",
            langfuse.observation.model.name = model,
            langfuse.observation.metadata.provider = self.provider.name(),
            langfuse.observation.metadata.request_id = tracing::field::Empty,
            langfuse.observation.input = tracing::field::Empty,
            langfuse.observation.output = tracing::field::Empty,
            langfuse.observation.usage_details = tracing::field::Empty,
            langfuse.session.id = tracing::field::Empty,
            otel.status_code = tracing::field::Empty,
            otel.status_description = tracing::field::Empty,
        )
    )]
    pub async fn system_one(&self, body: &Value, model: &str) -> Result<Response, Error> {
        let span = tracing::Span::current();
        telemetry::record_session(&span);
        self.provider.check_model(model)?;
        self.record_content(&span, "langfuse.observation.input", body);
        let url = self
            .provider
            .system_one_url(&self.base, self.account.as_deref(), model);
        let result = match self.provider.wire(model) {
            Wire::SystemOne => self.execute(&Method::POST, &url, Some(body)).await,
            Wire::Decisions => self.decisions_system_one(&url, body, model).await,
            Wire::ChatLogprobs => self.openai_system_one(&url, body, model, false).await,
            Wire::ChatStructured => self.openai_system_one(&url, body, model, true).await,
        };
        match &result {
            Ok(response) => {
                if let Some(resolved) = response.body.get("model").and_then(Value::as_str) {
                    span.record("langfuse.observation.model.name", resolved);
                }
                self.record_content(&span, "langfuse.observation.output", &response.body);
                if !span.is_disabled() {
                    if let Some(id) = &response.request_id {
                        span.record(
                            "langfuse.observation.metadata.request_id",
                            self.redact(id).as_ref(),
                        );
                    }
                    if let Some(usage) = token_usage(&response.body) {
                        span.record("langfuse.observation.usage_details", usage.to_string());
                    }
                }
            }
            Err(_) => telemetry::failed(&span, "model request failed"),
        }
        result
    }

    /// Sends a System One request to `OpenAI`'s Decisions API and maps the
    /// reply back onto the System One shape.
    async fn decisions_system_one(
        &self,
        url: &str,
        body: &Value,
        model: &str,
    ) -> Result<Response, Error> {
        let translated = decisions::request(body, model)?;
        let reply = self.execute(&Method::POST, url, Some(&translated)).await?;
        Ok(Response {
            body: decisions::response(&reply.body, body)?,
            request_id: reply.request_id,
        })
    }

    /// Answers a System One request with one chat completion per question,
    /// sent concurrently, and assembles the same `answers` shape the other
    /// providers return.
    async fn openai_system_one(
        &self,
        url: &str,
        body: &Value,
        model: &str,
        structured: bool,
    ) -> Result<Response, Error> {
        let questions = body
            .get("questions")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::Usage("the request needs a `questions` object".to_owned()))?;
        let state = util::render_state(body.get("state").unwrap_or(&Value::Null));
        let prepared = questions
            .iter()
            .map(|(id, question)| {
                let item = if structured {
                    crate::structured::prepare(model, &state, question)?
                } else {
                    openai::prepare(model, &state, question)?
                };
                Ok((id, item))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let replies = futures_util::future::join_all(
            prepared
                .iter()
                .map(|(_, item)| self.execute(&Method::POST, url, Some(&item.body))),
        )
        .await;

        let mut answers = serde_json::Map::new();
        let mut provider_responses = serde_json::Map::new();
        let (mut input, mut output, mut cached) = (0_u64, 0_u64, 0_u64);
        let mut request_id = None;
        for ((id, item), reply) in prepared.iter().zip(replies) {
            let reply = reply?;
            let (reply_in, reply_out) = token_totals(&reply.body);
            input += reply_in;
            output += reply_out;
            cached += cached_tokens(&reply.body);
            request_id = request_id.or(reply.request_id);
            if structured {
                // A malformed self-reported answer refuses this question, not the request.
                let answer = crate::structured::interpret(item, &reply.body)
                    .unwrap_or_else(|error| json!({"type":"refusal", "reason":error.to_string()}));
                answers.insert((*id).clone(), answer);
                provider_responses.insert((*id).clone(), reply.body);
            } else {
                answers.insert((*id).clone(), openai::interpret(item, &reply.body)?);
            }
        }
        Ok(Response {
            body: json!({
                "model": model,
                "answers": answers,
                "provider_responses": provider_responses,
                "usage": { "input_tokens": input, "output_tokens": output, "input_tokens_details": { "cached_tokens": cached } },
                "adapter": if structured { "self-reported-json-probabilities;thinking-disabled" } else { "token-logprobs" },
            }),
            request_id,
        })
    }

    /// Sends a request, retrying transient failures until `max_retries` is spent.
    async fn execute(
        &self,
        method: &Method,
        url: &str,
        body: Option<&Value>,
    ) -> Result<Response, Error> {
        let mut attempt = 0_u32;

        loop {
            match self.send_once(method, url, body, attempt).await {
                Ok(response) => {
                    let HttpResponse {
                        status,
                        request_id,
                        retry_after,
                        text: body_text,
                    } = response;

                    if (200..300).contains(&status) {
                        let parsed: Value =
                            serde_json::from_str(&body_text).map_err(Error::Json)?;
                        if parsed.get("success").and_then(Value::as_bool) == Some(false) {
                            return Err(Error::Api {
                                status,
                                request_suffix: suffix(request_id.as_deref()),
                                body: parsed
                                    .get("errors")
                                    .map_or_else(|| body_text.clone(), ToString::to_string),
                            });
                        }
                        return Ok(Response {
                            body: unwrap_envelope(parsed),
                            request_id,
                        });
                    }

                    if !retryable(status) || attempt >= self.max_retries {
                        return Err(Error::Api {
                            status,
                            request_suffix: suffix(request_id.as_deref()),
                            body: body_text,
                        });
                    }
                    wait(attempt, retry_after, &format!("HTTP {status}")).await;
                }
                Err(Error::Http(error)) => {
                    let transient = error.is_timeout() || error.is_connect();
                    if !transient || attempt >= self.max_retries {
                        return Err(Error::Http(error));
                    }
                    wait(attempt, None, "connection failure").await;
                }
                Err(error) => return Err(error),
            }

            attempt += 1;
        }
    }

    /// Records an attempt through body consumption, without capturing its URL or headers.
    #[tracing::instrument(
        name = "http.attempt", skip_all,
        fields(
            http.request.method = %method,
            http.response.status_code = tracing::field::Empty,
            langfuse.observation.metadata.attempt = i64::from(attempt) + 1,
            langfuse.observation.metadata.request_id = tracing::field::Empty,
            langfuse.session.id = tracing::field::Empty,
            otel.status_code = tracing::field::Empty,
            otel.status_description = tracing::field::Empty,
        )
    )]
    async fn send_once(
        &self,
        method: &Method,
        url: &str,
        body: Option<&Value>,
        attempt: u32,
    ) -> Result<HttpResponse, Error> {
        let span = tracing::Span::current();
        telemetry::record_session(&span);
        let mut request = self.http.request(method.clone(), url);
        if let Some(json) = body {
            request = request.json(json);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                telemetry::failed(&span, "HTTP transport failed");
                return Err(Error::Http(error));
            }
        };
        let status = response.status().as_u16();
        span.record("http.response.status_code", i64::from(status));
        if !(200..300).contains(&status) {
            telemetry::failed(&span, "HTTP response was unsuccessful");
        }
        let request_id = self
            .provider
            .request_id_header()
            .and_then(|name| header(response.headers(), name));
        if let Some(id) = &request_id {
            span.record(
                "langfuse.observation.metadata.request_id",
                self.redact(id).as_ref(),
            );
        }
        let retry_after =
            header(response.headers(), "retry-after").and_then(|value| value.parse::<u64>().ok());
        let text = match response.text().await {
            Ok(text) => {
                let redacted = match self.redact(&text) {
                    Cow::Owned(redacted) => Some(redacted),
                    Cow::Borrowed(_) => None,
                };
                redacted.unwrap_or(text)
            }
            Err(error) => {
                telemetry::failed(&span, "HTTP response body read failed");
                return Err(Error::Http(error));
            }
        };
        Ok(HttpResponse {
            status,
            request_id,
            retry_after,
            text,
        })
    }

    /// Proves the credential, account and model work, returning what was done.
    /// Providers without a model listing get one tiny test request instead.
    pub async fn probe(&self, model: &str) -> Result<&'static str, Error> {
        if self.provider.probe_via_models() {
            self.models().await?;
            return Ok("reachable");
        }
        let questions = json!({ "ping": { "type": "noul", "instructions": "Is this a test?" } });
        let body = request(json!("ping"), model, questions);
        self.system_one(&body, model).await?;
        Ok("answered a test request")
    }

    /// Redacts the provider's credential from explicitly captured payloads.
    fn redact<'a>(&self, text: &'a str) -> Cow<'a, str> {
        self.secret
            .as_ref()
            .map_or(Cow::Borrowed(text), |secret| secret.redact(text))
    }

    /// Records a captured payload on `span` when content capture is on.
    fn record_content(&self, span: &tracing::Span, field: &str, payload: &Value) {
        if self.telemetry_content && !span.is_disabled() {
            span.record(field, self.redact(&payload.to_string()).as_ref());
        }
    }
}

/// Maps only numeric token counters, rather than forwarding arbitrary usage metadata.
pub fn token_usage(body: &Value) -> Option<Value> {
    let usage = body.get("usage")?;
    let mut tokens = serde_json::Map::new();
    for (target, names) in [
        ("input", ["input_tokens", "prompt_tokens"]),
        ("output", ["output_tokens", "completion_tokens"]),
    ] {
        if let Some(value) = names
            .iter()
            .find_map(|name| usage.get(name).and_then(Value::as_u64))
        {
            tokens.insert(target.to_owned(), json!(value));
        }
    }
    (!tokens.is_empty()).then_some(Value::Object(tokens))
}

/// Input and output token counts of a response, zero where absent.
pub fn token_totals(body: &Value) -> (u64, u64) {
    let usage = token_usage(body);
    let count = |key: &str| {
        usage
            .as_ref()
            .and_then(|tokens| tokens.get(key))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    (count("input"), count("output"))
}

/// Input tokens served from the provider's prompt cache, zero where absent.
fn cached_tokens(body: &Value) -> u64 {
    body.pointer("/usage/prompt_tokens_details/cached_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// Reads a header as a string, ignoring values that are not valid UTF-8.
fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

/// Renders a request id for an error message, or nothing when absent.
fn suffix(request_id: Option<&str>) -> String {
    request_id.map_or_else(String::new, |id| format!(" (request {id})"))
}

/// Statuses worth retrying: request timeout, rate limit, and any server error
/// (which covers the `529 Overloaded` status).
const fn retryable(status: u16) -> bool {
    status == 408 || status == 429 || status >= 500
}

/// Sleeps before the next attempt, preferring the server's `retry-after`.
#[tracing::instrument(
    name = "retry.backoff", skip_all,
    fields(
        langfuse.observation.metadata.attempt = i64::from(attempt) + 1,
        langfuse.observation.metadata.reason = reason,
        langfuse.observation.metadata.delay_ms = tracing::field::Empty,
        langfuse.session.id = tracing::field::Empty,
    )
)]
async fn wait(attempt: u32, retry_after: Option<u64>, reason: &str) {
    let delay = retry_after.map_or_else(
        || backoff(attempt),
        |secs| Duration::from_secs(secs.min(RETRY_AFTER_MAX)),
    );
    let span = tracing::Span::current();
    telemetry::record_session(&span);
    span.record(
        "langfuse.observation.metadata.delay_ms",
        i64::try_from(delay.as_millis()).unwrap_or(i64::MAX),
    );
    eprintln!("jev: {reason}, retrying in {:.1}s", delay.as_secs_f64());
    tokio::time::sleep(delay).await;
}

/// Exponential backoff with up to 25% jitter, so parallel callers spread out.
fn backoff(attempt: u32) -> Duration {
    let base = BACKOFF_INITIAL
        .saturating_mul(2_u32.saturating_pow(attempt.min(16)))
        .min(BACKOFF_MAX);
    base + jitter(base)
}

/// Pseudo-random jitter of up to a quarter of `base`, seeded from the clock.
/// Spreading retries matters more here than the quality of the randomness.
fn jitter(base: Duration) -> Duration {
    let span = u64::try_from(base.as_millis()).unwrap_or(u64::MAX) / 4;
    if span == 0 {
        return Duration::ZERO;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| u64::from(since.subsec_nanos()));
    Duration::from_millis(nanos % span)
}

/// Removes Cloudflare's `result` envelope, leaving the System One response.
///
/// A body that already carries `answers` at the top level (`TypeSafe`, Ollama) is
/// returned unchanged.
pub fn unwrap_envelope(value: Value) -> Value {
    match value {
        Value::Object(mut map) if !map.contains_key("answers") => match map.remove("result") {
            Some(inner @ Value::Object(_)) => inner,
            Some(other) => {
                map.insert("result".to_owned(), other);
                Value::Object(map)
            }
            None => Value::Object(map),
        },
        other => other,
    }
}

/// Builds the request body around a state and a set of questions.
pub fn request(state: Value, model: &str, questions: Value) -> Value {
    Value::Object(serde_json::Map::from_iter([
        ("state".to_owned(), state),
        ("model".to_owned(), Value::String(model.to_owned())),
        ("questions".to_owned(), questions),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_transient_statuses_only() {
        for status in [408, 429, 500, 502, 503, 529] {
            assert!(retryable(status), "{status} should be retryable");
        }
        for status in [400, 401, 403, 404, 422] {
            assert!(!retryable(status), "{status} should not be retryable");
        }
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        let first = backoff(0);
        let later = backoff(5);
        assert!(first >= BACKOFF_INITIAL);
        assert!(first < BACKOFF_INITIAL * 2);
        // Capped at BACKOFF_MAX plus at most 25% jitter.
        assert!(later <= BACKOFF_MAX + BACKOFF_MAX / 4);
        assert!(later > first);
    }

    #[test]
    fn backoff_does_not_overflow_on_large_attempts() {
        let huge = backoff(u32::MAX);
        assert!(huge <= BACKOFF_MAX + BACKOFF_MAX / 4);
    }

    #[test]
    fn unwraps_the_cloudflare_envelope() {
        let wrapped = json!({"result": {"answers": {"a": 1}}, "success": true});
        assert_eq!(unwrap_envelope(wrapped), json!({"answers": {"a": 1}}));
    }

    #[test]
    fn leaves_a_bare_response_alone() {
        let bare = json!({"answers": {"a": 1}, "usage": {}});
        assert_eq!(unwrap_envelope(bare.clone()), bare);
    }

    #[test]
    fn keeps_a_non_object_result_untouched() {
        let odd = json!({"result": [1, 2]});
        assert_eq!(unwrap_envelope(odd.clone()), odd);
    }

    #[test]
    fn suffix_is_empty_without_a_request_id() {
        assert_eq!(suffix(None), "");
        assert_eq!(suffix(Some("abc")), " (request abc)");
    }
}
