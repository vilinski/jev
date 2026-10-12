//! End-to-end checks: run the real `jev` binary against a one-shot local HTTP
//! server and assert what it put on the wire and what it printed.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::thread::{self, JoinHandle};

/// Starts a server that answers one request with `response_body` and returns
/// its base URL plus a handle yielding the raw request it received.
fn mock(response_body: &str) -> (String, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    let payload = response_body.to_owned();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut request = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let read = stream.read(&mut chunk).expect("read");
            request.extend_from_slice(&chunk[..read]);
            let text = String::from_utf8_lossy(&request);
            if let Some(split) = text.find("\r\n\r\n") {
                let length = text[..split]
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|value| value.trim().parse::<usize>().expect("length"))
                    })
                    .unwrap_or(0);
                if request.len() >= split + 4 + length {
                    break;
                }
            }
            if read == 0 {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}",
            payload.len()
        );
        stream.write_all(response.as_bytes()).expect("write");
        String::from_utf8(request).expect("utf8")
    });
    (url, handle)
}

/// A `jev` command with every variable that could change its behaviour cleared.
fn jev() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jev"));
    for var in [
        "JEV_PROVIDER",
        "JEV_MODEL",
        "JEV_URL",
        "JEV_KEEP_ALIVE",
        "JEV_TELEMETRY",
        "JEV_TELEMETRY_CONTENT",
        "TYPESAFE_API_URL",
        "TYPESAFE_API_KEY",
        "CLOUDFLARE_API_TOKEN",
        "CLOUDFLARE_ACCOUNT_ID",
        "OPENAI_API_KEY",
        "CELERIS_API_KEY",
    ] {
        command.env_remove(var);
    }
    command
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1");
    command
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Lower-cased request text, so header assertions ignore header-name casing.
fn header_text(request: &str) -> String {
    request.to_ascii_lowercase()
}

#[test]
fn typesafe_posts_to_v1_systemone_with_bearer_key() {
    let (url, request) = mock(r#"{"answers":{"answer":{"probability":0.9}}}"#);
    let output = jev()
        .env("TYPESAFE_API_KEY", "ts_test_key")
        .args(["--url", &url, "noul", "Is it?", "-s", "hello"])
        .output()
        .expect("run");
    assert!(output.status.success(), "{}", stderr(&output));

    let request = request.join().expect("mock");
    assert!(request.starts_with("POST /v1/systemone "), "{request}");
    assert!(header_text(&request).contains("authorization: bearer ts_test_key"));
    assert!(request.contains(r#""model":"jev-latest""#), "{request}");
    assert!(stdout(&output).contains("0.9"));
}

#[test]
fn ollama_sends_no_credential_and_defaults_to_clef_flash() {
    let (url, request) = mock(r#"{"answers":{"answer":{"probability":0.2}}}"#);
    let output = jev()
        .args([
            "--provider",
            "ollama",
            "--url",
            &url,
            "--keep-alive",
            "30m",
            "noul",
            "Is it?",
            "-s",
            "hello",
        ])
        .output()
        .expect("run");
    assert!(output.status.success(), "{}", stderr(&output));

    let request = request.join().expect("mock");
    assert!(request.starts_with("POST /v1/systemone "), "{request}");
    assert!(
        !header_text(&request).contains("authorization"),
        "{request}"
    );
    assert!(request.contains(r#""model":"clef-flash""#), "{request}");
    assert!(request.contains(r#""keep_alive":"30m""#), "{request}");
}

#[test]
fn cloudflare_uses_account_path_bearer_token_and_unwraps_the_envelope() {
    let (url, request) =
        mock(r#"{"result":{"answers":{"answer":{"probability":0.7}}},"success":true}"#);
    let output = jev()
        .env("CLOUDFLARE_API_TOKEN", "cf_test_token")
        .env("CLOUDFLARE_ACCOUNT_ID", "acct123")
        .args(["--provider", "cloudflare", "--url", &url])
        .args(["noul", "Is it?", "-s", "hello"])
        .output()
        .expect("run");
    assert!(output.status.success(), "{}", stderr(&output));

    let request = request.join().expect("mock");
    assert!(
        request.starts_with("POST /accounts/acct123/ai/run/@cf/cloudflare/clef-flash "),
        "{request}"
    );
    assert!(header_text(&request).contains("authorization: bearer cf_test_token"));
    assert!(request.contains(r#""model":"clef-flash""#), "{request}");

    let printed = stdout(&output);
    assert!(printed.contains("0.7"), "{printed}");
    assert!(!printed.contains("result"), "envelope leaked: {printed}");
}

#[test]
fn cloudflare_run_takes_the_path_model_from_the_request_body() {
    let (url, request) = mock(r#"{"result":{"answers":{}},"success":true}"#);
    let body = r#"{"model":"clef","state":"x","questions":{}}"#;
    let mut child = jev()
        .env("CLOUDFLARE_API_TOKEN", "cf_dummy_token_value")
        .env("CLOUDFLARE_ACCOUNT_ID", "acct456")
        .args(["--provider", "cloudflare", "--url", &url, "run", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(body.as_bytes())
        .expect("write body");
    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success(), "{}", stderr(&output));

    let request = request.join().expect("mock");
    assert!(
        request.starts_with("POST /accounts/acct456/ai/run/@cf/cloudflare/clef "),
        "{request}"
    );
}

#[test]
fn cloudflare_rejects_a_jev_model_before_any_request() {
    let output = jev()
        .env("CLOUDFLARE_API_TOKEN", "cf_dummy_token_value")
        .env("CLOUDFLARE_ACCOUNT_ID", "acct456")
        .args(["--provider", "cloudflare", "--model", "jev-latest"])
        .args(["--url", "http://127.0.0.1:9", "noul", "q", "-s", "x"])
        .output()
        .expect("run");
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("clef-flash"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn cloudflare_error_flagged_in_a_200_body_is_reported() {
    let (url, _request) =
        mock(r#"{"success":false,"errors":[{"code":10000,"message":"Authentication error"}]}"#);
    let output = jev()
        .env("CLOUDFLARE_API_TOKEN", "cf_dummy_token_value")
        .env("CLOUDFLARE_ACCOUNT_ID", "acct456")
        .args([
            "--provider",
            "cloudflare",
            "--url",
            &url,
            "noul",
            "q",
            "-s",
            "x",
        ])
        .output()
        .expect("run");
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("Authentication error"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_secret_never_appears_in_error_output() {
    let (url, _request) = mock(r#"{"success":false,"errors":["bad token cf_leaky_token"]}"#);
    let output = jev()
        .env("CLOUDFLARE_API_TOKEN", "cf_leaky_token")
        .env("CLOUDFLARE_ACCOUNT_ID", "acct456")
        .args([
            "--provider",
            "cloudflare",
            "--url",
            &url,
            "noul",
            "q",
            "-s",
            "x",
        ])
        .output()
        .expect("run");
    assert!(!output.status.success());
    let everything = format!("{}{}", stdout(&output), stderr(&output));
    assert!(!everything.contains("cf_leaky_token"), "{everything}");
}

#[test]
fn ollama_login_is_refused_because_it_needs_no_credential() {
    let output = jev()
        .args(["--provider", "ollama", "login"])
        .output()
        .expect("run");
    assert!(!output.status.success());
    assert!(stderr(&output).contains("needs no credential"));
}

#[test]
fn short_flag_and_aliases_select_the_same_providers() {
    let (url, request) = mock(r#"{"result":{"answers":{}},"success":true}"#);
    let output = jev()
        .env("CLOUDFLARE_API_TOKEN", "cf_dummy_token_value")
        .env("CLOUDFLARE_ACCOUNT_ID", "acct456")
        .args(["-p", "cf", "--url", &url, "noul", "q", "-s", "x"])
        .output()
        .expect("run");
    assert!(output.status.success(), "{}", stderr(&output));
    let request = request.join().expect("mock");
    assert!(request.starts_with("POST /accounts/acct456/ai/run/@cf/cloudflare/clef-flash "));

    let (url, request) = mock(r#"{"answers":{}}"#);
    let output = jev()
        .args(["-p", "ol", "--url", &url, "noul", "q", "-s", "x"])
        .output()
        .expect("run");
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        request
            .join()
            .expect("mock")
            .starts_with("POST /v1/systemone ")
    );
}

#[test]
fn openai_asks_chat_completions_and_turns_logprobs_into_probabilities() {
    let (url, request) = mock(
        r#"{"choices":[{"logprobs":{"content":[{"top_logprobs":[
            {"token":"yes","logprob":-0.2231435513},{"token":"no","logprob":-1.6094379124}]}]}}],
            "usage":{"prompt_tokens":42,"completion_tokens":1}}"#,
    );
    let output = jev()
        .env("OPENAI_API_KEY", "sk_test_openai")
        .args([
            "-p",
            "openai-chat",
            "--url",
            &url,
            "noul",
            "Is it?",
            "-s",
            "hello",
        ])
        .output()
        .expect("run");
    assert!(output.status.success(), "{}", stderr(&output));

    let request = request.join().expect("mock");
    assert!(
        request.starts_with("POST /v1/chat/completions "),
        "{request}"
    );
    assert!(header_text(&request).contains("authorization: bearer sk_test_openai"));
    assert!(request.contains(r#""model":"gpt-4.1-mini""#), "{request}");
    assert!(request.contains(r#""logprobs":true"#), "{request}");

    let printed = stdout(&output);
    assert!(printed.contains(r#""noul": 0.8"#), "{printed}");
    assert!(
        stderr(&output).contains("usage: 42 in / 1 out"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn openai_login_stores_nothing_extra_and_missing_key_points_to_the_key_page() {
    let output = jev()
        .env_remove("OPENAI_API_KEY")
        .env("HOME", "/nonexistent")
        .args([
            "-p",
            "oai",
            "--url",
            "http://127.0.0.1:9",
            "noul",
            "q",
            "-s",
            "x",
        ])
        .output()
        .expect("run");
    assert!(!output.status.success());
    let text = stderr(&output);
    assert!(
        text.contains("platform.openai.com/api-keys") || text.contains("keychain error"),
        "{text}"
    );
}

#[test]
fn celeris_posts_to_its_model_path_with_bearer_key() {
    let (url, request) = mock(
        r#"{"answers":{"answer":{"type":"noul","noul":0.9}},"usage":{"input_tokens":5,"output_tokens":0}}"#,
    );
    let output = jev()
        .env("CELERIS_API_KEY", "cel_test_key")
        .args(["-p", "cel", "--url", &url, "noul", "Is it?", "-s", "hello"])
        .output()
        .expect("run");
    assert!(output.status.success(), "{}", stderr(&output));
    let request = request.join().expect("mock");
    assert!(
        request.starts_with("POST /celeris-1-decision/v1/systemone "),
        "{request}"
    );
    assert!(header_text(&request).contains("authorization: bearer cel_test_key"));
    assert!(
        request.contains(r#""model":"celeris-1-decision""#),
        "{request}"
    );
}

#[test]
fn openai_posts_to_v1_decisions_and_maps_the_answers() {
    let (url, request) =
        mock(r#"{"answers":[{"name":"answer","type":"predicate","probability":0.75}]}"#);
    let output = jev()
        .env("OPENAI_API_KEY", "sk_test_openai")
        .args([
            "-p", "openai", "--url", &url, "noul", "Is it?", "-s", "hello",
        ])
        .output()
        .expect("run");
    assert!(output.status.success(), "{}", stderr(&output));

    let request = request.join().expect("mock");
    assert!(request.starts_with("POST /v1/decisions "), "{request}");
    assert!(request.contains(r#""model":"gpt-6-luna""#), "{request}");
    assert!(request.contains(r#""type":"predicate""#), "{request}");
    assert!(request.contains(r#""input":"hello""#), "{request}");
    assert!(
        stdout(&output).contains(r#""noul": 0.75"#),
        "{}",
        stdout(&output)
    );
}
