//! Wire-level checks for export, content capture, retries and CLI exit handling.

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::Value;

/// A request as received by the mock model server or OTLP receiver.
struct Request {
    headers: String,
    body: Vec<u8>,
}

/// Serves a finite sequence of responses, with deadlines so failures cannot hang tests.
fn server(responses: Vec<(u16, &'static str)>) -> (String, JoinHandle<Vec<Request>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
    listener.set_nonblocking(true).expect("nonblocking");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let handle = thread::spawn(move || {
        let mut captured = Vec::new();
        for (status, body) in responses {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "mock did not receive a request");
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            };
            // Accepted sockets inherit the listener's non-blocking mode on BSD and macOS.
            stream.set_nonblocking(false).expect("blocking stream");
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .expect("timeout");
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            let (split, length) = loop {
                let count = stream.read(&mut chunk).expect("read request");
                assert!(count > 0, "incomplete request");
                request.extend_from_slice(&chunk[..count]);
                if let Some(split) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..split]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().expect("content length"))
                        })
                        .unwrap_or(0);
                    if request.len() >= split + 4 + length {
                        break (split, length);
                    }
                }
            };
            captured.push(Request {
                headers: String::from_utf8_lossy(&request[..split]).into_owned(),
                body: request[split + 4..split + 4 + length].to_vec(),
            });
            write!(stream, "HTTP/1.1 {status} Mock\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).expect("respond");
        }
        captured
    });
    (url, handle)
}

/// Clears external configuration and uses only the mock model's credential.
fn jev() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_jev"));
    for (name, _) in std::env::vars_os() {
        let text = name.to_string_lossy();
        if ["OTEL_", "LANGFUSE_", "JEV_", "TYPESAFE_", "CLOUDFLARE_"]
            .iter()
            .any(|p| text.starts_with(p))
        {
            cmd.env_remove(name);
        }
    }
    cmd.env("TYPESAFE_API_KEY", "ts_test_secret")
        .env_remove("TRACEPARENT")
        .env_remove("TRACESTATE")
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1");
    cmd
}

/// Runs a request with a local OTLP receiver and returns the exported spans.
fn run(responses: Vec<(u16, &'static str)>, content: bool) -> (Output, Vec<Value>, Request) {
    let (api, model_requests) = server(responses);
    let (collector, export) = server(vec![(200, "{}")]);
    let mut cmd = jev();
    cmd.env(
        "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
        format!("{collector}/v1/traces"),
    )
    .env("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", "http/json")
    .env("OTEL_EXPORTER_OTLP_TRACES_HEADERS", "x-test=collector")
    .env("LANGFUSE_PUBLIC_KEY", "pk-not-for-collector")
    .env("LANGFUSE_SECRET_KEY", "sk-not-for-collector")
    .env("JEV_SESSION_ID", "workshop-session")
    .env(
        "TRACEPARENT",
        "00-11111111111111111111111111111111-2222222222222222-01",
    )
    .args([
        "--telemetry",
        "--url",
        &api,
        "noul",
        "Is this a test?",
        "-s",
        "private state ts_test_secret",
    ]);
    if content {
        cmd.arg("--telemetry-content");
    }
    let output = cmd.output().expect("run jev");
    model_requests.join().expect("model server");
    let mut requests = export.join().expect("OTLP receiver");
    let request = requests.remove(0);
    let json: Value = serde_json::from_slice(&request.body).expect("OTLP JSON");
    let spans = json["resourceSpans"]
        .as_array()
        .expect("resources")
        .iter()
        .flat_map(|r| r["scopeSpans"].as_array().expect("scopes"))
        .flat_map(|s| s["spans"].as_array().expect("spans"))
        .cloned()
        .collect();
    (output, spans, request)
}

/// Locates a named observation.
fn span<'a>(spans: &'a [Value], name: &str) -> &'a Value {
    spans
        .iter()
        .find(|s| s["name"] == name)
        .expect("span present")
}

/// Returns the OTLP `AnyValue` for an attribute, or null if absent.
fn attribute(span: &Value, key: &str) -> Value {
    span["attributes"]
        .as_array()
        .expect("attributes")
        .iter()
        .find(|a| a["key"] == key)
        .map_or(Value::Null, |a| a["value"].clone())
}

const ANSWER: &str = r#"{"model":"jev-resolved","answers":{"answer":{"noul":0.9}},"usage":{"input_tokens":12,"output_tokens":3}}"#;

#[test]
fn exports_a_complete_trace_before_exit_without_payloads_or_credentials() {
    let (output, spans, request) = run(vec![(200, ANSWER)], false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        serde_json::from_slice::<Value>(&output.stdout).is_ok(),
        "stdout stays JSON"
    );
    assert_eq!(spans.len(), 3);
    let root = span(&spans, "jev.command");
    let generation = span(&spans, "jev.system_one");
    let attempt = span(&spans, "http.attempt");
    assert_eq!(generation["parentSpanId"], root["spanId"]);
    assert_eq!(attempt["parentSpanId"], generation["spanId"]);
    assert_eq!(root["traceId"], "11111111111111111111111111111111");
    assert_eq!(root["parentSpanId"], "2222222222222222");
    for s in &spans {
        assert_eq!(
            attribute(s, "langfuse.session.id")["stringValue"],
            "workshop-session"
        );
    }
    assert_eq!(
        attribute(generation, "langfuse.observation.type")["stringValue"],
        "generation"
    );
    let usage = attribute(generation, "langfuse.observation.usage_details");
    let usage: Value =
        serde_json::from_str(usage["stringValue"].as_str().expect("usage")).expect("token JSON");
    assert_eq!(usage, serde_json::json!({"input":12,"output":3}));
    let wire = String::from_utf8_lossy(&request.body);
    for secret in [
        "private state",
        "Is this a test?",
        "ts_test_secret",
        "sk-not-for-collector",
    ] {
        assert!(
            !wire.contains(secret),
            "payload/credential leaked: {secret}"
        );
    }
    assert!(
        request
            .headers
            .to_ascii_lowercase()
            .contains("x-test: collector")
    );
    assert!(
        !request
            .headers
            .to_ascii_lowercase()
            .contains("authorization:")
    );
}

#[test]
fn content_capture_includes_redacted_input_and_structured_output() {
    let (output, spans, _) = run(vec![(200, ANSWER)], true);
    assert!(output.status.success());
    let generation = span(&spans, "jev.system_one");
    let input = attribute(generation, "langfuse.observation.input");
    let input: Value =
        serde_json::from_str(input["stringValue"].as_str().expect("input")).expect("input JSON");
    assert_eq!(input["state"], "private state <redacted>");
    let output = attribute(generation, "langfuse.observation.output");
    let output: Value =
        serde_json::from_str(output["stringValue"].as_str().expect("output")).expect("output JSON");
    assert_eq!(output["answers"]["answer"]["noul"], 0.9);
}

#[test]
fn retry_attempts_and_backoff_are_separate_children_of_the_model_call() {
    let (output, spans, _) = run(vec![(429, "rate limited"), (200, ANSWER)], false);
    assert!(output.status.success());
    let generation = span(&spans, "jev.system_one");
    let attempts: Vec<_> = spans
        .iter()
        .filter(|s| s["name"] == "http.attempt")
        .collect();
    assert_eq!(attempts.len(), 2);
    let failed = attempts
        .iter()
        .find(|s| attribute(s, "http.response.status_code")["intValue"] == "429")
        .unwrap_or_else(|| panic!("failed attempt missing: {attempts:#?}"));
    assert_eq!(failed["status"]["code"], 2);
    assert_eq!(
        span(&spans, "retry.backoff")["parentSpanId"],
        generation["spanId"]
    );
    assert_ne!(generation["status"]["code"], 2, "retry recovered");
}

#[test]
fn failed_commands_flush_their_trace_and_keep_the_failure_exit_status() {
    let (output, spans, request) = run(vec![(401, "upstream echoed ts_test_secret")], false);
    assert!(!output.status.success());
    assert_eq!(span(&spans, "jev.command")["status"]["code"], 2);
    assert_eq!(span(&spans, "jev.system_one")["status"]["code"], 2);
    assert!(!String::from_utf8_lossy(&request.body).contains("ts_test_secret"));
    assert!(!String::from_utf8_lossy(&request.body).contains("upstream echoed"));
}

#[test]
fn an_unreachable_exporter_does_not_change_a_successful_model_result() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("unused port");
    let endpoint = format!(
        "http://{}/v1/traces",
        listener.local_addr().expect("address")
    );
    drop(listener);
    let (api, model) = server(vec![(200, ANSWER)]);
    let output = jev()
        .env("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT", endpoint)
        .args(["--telemetry", "--url", &api, "noul", "q", "-s", "x"])
        .output()
        .expect("run");
    model.join().expect("model server");
    assert!(
        output.status.success(),
        "export failure must not replace the result"
    );
    assert!(serde_json::from_slice::<Value>(&output.stdout).is_ok());
}

#[test]
fn langfuse_configuration_sets_basic_auth_and_v4_ingestion() {
    let (api, model) = server(vec![(200, ANSWER)]);
    let (langfuse, export) = server(vec![(200, "{}")]);
    let output = jev()
        .env("LANGFUSE_BASE_URL", langfuse)
        .env("LANGFUSE_PUBLIC_KEY", "pk-test-lf")
        .env("LANGFUSE_SECRET_KEY", "sk-test-lf")
        .env("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", "http/json")
        .args(["--telemetry", "--url", &api, "noul", "q", "-s", "x"])
        .output()
        .expect("run");
    model.join().expect("model");
    let requests = export.join().expect("export");
    assert!(output.status.success());
    let headers = requests[0].headers.to_ascii_lowercase();
    assert!(headers.starts_with("post /api/public/otel/v1/traces "));
    assert!(headers.contains("x-langfuse-ingestion-version: 4"));
    assert!(headers.contains("authorization: basic cgstdgvzdc1szjpzay10zxn0lwxm"));
    let spans = String::from_utf8_lossy(&requests[0].body);
    assert!(!spans.contains("sk-test-lf"));
    assert!(!spans.contains("ts_test_secret"));
}

#[test]
fn configured_credentials_do_not_enable_telemetry_implicitly() {
    let collector = TcpListener::bind("127.0.0.1:0").expect("receiver");
    collector.set_nonblocking(true).expect("nonblocking");
    let endpoint = format!("http://{}", collector.local_addr().expect("address"));
    let (api, model) = server(vec![(200, ANSWER)]);
    let output = jev()
        .env("LANGFUSE_BASE_URL", endpoint)
        .env("LANGFUSE_PUBLIC_KEY", "pk-test-lf")
        .env("LANGFUSE_SECRET_KEY", "sk-test-lf")
        .args(["--url", &api, "noul", "q", "-s", "x"])
        .output()
        .expect("run");
    model.join().expect("model");
    assert!(output.status.success());
    assert!(
        matches!(collector.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
}
