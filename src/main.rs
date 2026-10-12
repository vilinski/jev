//! `jev` - a CLI for System One decision models.
//!
//! Asks a decision model (`TypeSafe` Jev, Cloudflare Clef, or Clef on a local
//! Ollama) typed questions about a piece of state and prints the typed answers
//! as JSON. Two reasons this exists as a tool rather than a `curl` call in every
//! repository:
//!
//! * The credential lives in one place instead of being scattered across projects.
//! * A coding agent can call `jev` without ever reading the credential: it is
//!   loaded inside this process, marked sensitive in the request headers, and
//!   redacted from everything printed.
//!
//! See <https://docs.typesafe.ai> for the question types and response shapes.

mod api;
mod decisions;
mod key;
mod openai;
mod provider;
mod telemetry;
mod util;

use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use tracing::Instrument as _;

use provider::Provider;

/// Everything that can go wrong, with the key never part of the message.
#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(
        "no {what} stored. Run `jev login --provider {provider}` to save it in the OS keychain, or set {env}. {help}"
    )]
    Missing {
        what: &'static str,
        provider: String,
        env: &'static str,
        help: &'static str,
    },
    #[error("keychain error: {0}")]
    Keyring(#[source] keyring::Error),
    #[error("the API key contains characters that are not valid in an HTTP header")]
    InvalidKey,
    #[error("API returned HTTP {status}{request_suffix}: {body}")]
    Api {
        status: u16,
        request_suffix: String,
        body: String,
    },
    #[error("request failed: {0}")]
    Http(#[source] reqwest::Error),
    #[error("invalid JSON: {0}")]
    Json(#[source] serde_json::Error),
    #[error("{0}")]
    Io(#[source] std::io::Error),
    #[error("{0}")]
    Usage(String),
}

/// Ask a decision model (Jev, Clef) typed questions about some state.
#[derive(Debug, Parser)]
#[command(name = "jev", version, about, long_about = None)]
struct Cli {
    /// Print the whole response instead of just the answers.
    #[arg(long, global = true)]
    raw: bool,

    /// Backend to send requests to.
    #[arg(
        short = 'p',
        long,
        global = true,
        value_enum,
        default_value = "typesafe",
        env = "JEV_PROVIDER"
    )]
    provider: Provider,

    /// Model to send the request to (default depends on the provider).
    #[arg(long, global = true, env = "JEV_MODEL")]
    model: Option<String>,

    /// State as literal text.
    #[arg(short = 's', long, global = true, conflicts_with = "file")]
    state: Option<String>,

    /// State from a file; JSON is sent structured, anything else as text.
    #[arg(short = 'f', long, global = true)]
    file: Option<PathBuf>,

    /// API root, for a proxy or a mock server (default depends on the provider).
    #[arg(long, global = true, env = "JEV_URL")]
    url: Option<String>,

    /// How long Ollama keeps the model loaded, e.g. `30m` or `-1` (Ollama only).
    #[arg(long, global = true, env = "JEV_KEEP_ALIVE")]
    keep_alive: Option<String>,

    /// Request timeout in seconds.
    #[arg(long, global = true, env = "JEV_TIMEOUT", default_value_t = 120)]
    timeout: u64,

    /// Retries for timeouts, rate limits and server errors (0 disables).
    #[arg(long, global = true, env = "JEV_RETRIES", default_value_t = 2)]
    retries: u32,

    /// Export traces to Langfuse or a configured OTLP/HTTP collector.
    #[arg(long, global = true, env = "JEV_TELEMETRY")]
    telemetry: bool,

    /// Include request and response JSON in traces (also enables telemetry).
    #[arg(long, global = true, env = "JEV_TELEMETRY_CONTENT")]
    telemetry_content: bool,

    #[command(subcommand)]
    command: Command,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Store the provider's credential in the OS keychain (input is not echoed).
    Login,
    /// Remove the provider's stored credential from the OS keychain.
    Logout,
    /// Report where the credential was found and whether the API answers.
    Check,
    /// List the models this account may send in the `model` field.
    Models,
    /// Send a complete request body verbatim (`-` reads stdin).
    Run {
        /// Path to a JSON request body, or `-` for stdin.
        path: String,
    },
    /// Yes/no question; returns the probability of yes.
    Noul {
        /// The question to evaluate.
        question: String,
    },
    /// Pick one of several options; returns the pick and the distribution.
    Choice {
        /// The question to evaluate.
        question: String,
        /// Options as `name` or `name=description`; at least two.
        #[arg(required = true, num_args = 2..)]
        options: Vec<String>,
    },
    /// Rate against ordered levels; returns the level and the distribution.
    Score {
        /// The question to evaluate.
        question: String,
        /// Level descriptions in ascending order; at least two.
        #[arg(required = true, num_args = 2..)]
        levels: Vec<String>,
    },
}

/// Executes a command and flushes telemetry before returning its exit status.
fn main() -> ExitCode {
    let cli = Cli::parse();
    let enabled = (cli.telemetry || cli.telemetry_content)
        && !matches!(cli.command, Command::Login | Command::Logout);
    let telemetry = telemetry::Telemetry::init(enabled);
    let span = telemetry.command_span(cli.command.name(), cli.provider.name());
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::Io)
        .and_then(|runtime| runtime.block_on(run(cli).instrument(span.clone())));
    if result.is_err() {
        telemetry::failed(&span, "command failed");
    }
    drop(span);
    telemetry.shutdown();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("jev: {error}");
            ExitCode::FAILURE
        }
    }
}

impl Command {
    /// Stable command name for telemetry; contains no user-supplied arguments.
    const fn name(&self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Logout => "logout",
            Self::Check => "check",
            Self::Models => "models",
            Self::Run { .. } => "run",
            Self::Noul { .. } => "noul",
            Self::Choice { .. } => "choice",
            Self::Score { .. } => "score",
        }
    }
}

/// Provider, model and URL after applying defaults and the legacy variable.
struct Target {
    provider: Provider,
    model: String,
    url: String,
}

impl Target {
    /// Applies the provider's defaults to whatever the user left unset.
    fn resolve(cli: &Cli) -> Self {
        let provider = cli.provider;
        let url = cli
            .url
            .clone()
            // Older setups point `jev` at a gateway through a provider variable.
            .or_else(|| provider.url_env().and_then(key::env_nonempty))
            .unwrap_or_else(|| provider.default_url().to_owned());
        Self {
            provider,
            model: cli
                .model
                .clone()
                .unwrap_or_else(|| provider.default_model().to_owned()),
            url,
        }
    }
}

/// Runs the requested subcommand.
async fn run(cli: Cli) -> Result<(), Error> {
    let target = Target::resolve(&cli);
    match &cli.command {
        Command::Login => return login(target.provider),
        Command::Logout => return logout(target.provider),
        _ => {}
    }

    let name = target.provider.name();
    let secret = target
        .provider
        .secret_slot()
        .map(|slot| key::load(slot, name))
        .transpose()?;
    let account = target
        .provider
        .account_slot()
        .map(|slot| key::load(slot, name))
        .transpose()?
        .map(|account| account.expose().to_owned());
    let client = api::Client::new(
        target.provider,
        &target.url,
        secret,
        account.clone(),
        cli.timeout,
        cli.retries,
    )?
    .with_telemetry_content(cli.telemetry_content);

    match &cli.command {
        Command::Login | Command::Logout => unreachable!("handled above"),
        Command::Check => {
            println!("provider: {name}");
            println!("key:      {}", client.key_source());
            if let Some(account) = &account {
                println!("account:  {account}");
            }
            println!(
                "api:      {} ({})",
                client.probe(&target.model).await?,
                target.url
            );
            println!("model:    {}", target.model);
            Ok(())
        }
        Command::Models => {
            print_json(&client.models().await?.body);
            Ok(())
        }
        Command::Run { path } => {
            let mut body: Value = serde_json::from_str(&read_source(path)?).map_err(Error::Json)?;
            let model = body
                .get("model")
                .and_then(Value::as_str)
                .map_or_else(|| target.model.clone(), str::to_owned);
            if let Some(object) = body.as_object_mut() {
                object
                    .entry("model")
                    .or_insert_with(|| Value::String(model.clone()));
            }
            target
                .provider
                .apply_keep_alive(&mut body, cli.keep_alive.as_deref());
            emit(&client.system_one(&body, &model).await?, cli.raw);
            Ok(())
        }
        Command::Noul { question } => {
            let questions = json!({ "answer": { "type": "noul", "instructions": question } });
            ask(&client, &cli, &target, questions).await
        }
        Command::Choice { question, options } => {
            let criteria: serde_json::Map<String, Value> = options
                .iter()
                .map(|option| {
                    option
                        .split_once('=')
                        .map_or((option.as_str(), Value::Null), |(name, text)| {
                            (name, json!(text))
                        })
                })
                .map(|(name, description)| (name.to_owned(), description))
                .collect();
            let questions = json!({
                "answer": { "type": "choice", "instructions": question, "criteria": criteria }
            });
            ask(&client, &cli, &target, questions).await
        }
        Command::Score { question, levels } => {
            let questions = json!({
                "answer": { "type": "score", "instructions": question, "criteria": levels }
            });
            ask(&client, &cli, &target, questions).await
        }
    }
}

/// Prompts for the provider's credential and stores it in the keychain.
fn login(provider: Provider) -> Result<(), Error> {
    let Some(secret_slot) = provider.secret_slot() else {
        return Err(Error::Usage(format!(
            "{} needs no credential; nothing to store",
            provider.name()
        )));
    };
    eprint!("{} (not echoed): ", secret_slot.label);
    std::io::Write::flush(&mut std::io::stderr()).map_err(Error::Io)?;
    let secret = rpassword::read_password().map_err(Error::Io)?;
    key::store(secret_slot, &secret)?;
    drop(secret);
    eprintln!("stored in keychain {}", key::location(secret_slot));

    if let Some(account_slot) = provider.account_slot() {
        eprint!("{}: ", account_slot.label);
        std::io::Write::flush(&mut std::io::stderr()).map_err(Error::Io)?;
        let mut account = String::new();
        std::io::stdin()
            .read_line(&mut account)
            .map_err(Error::Io)?;
        key::store(account_slot, &account)?;
        eprintln!("stored in keychain {}", key::location(account_slot));
    }
    Ok(())
}

/// Removes the provider's credential (and account id) from the keychain.
fn logout(provider: Provider) -> Result<(), Error> {
    let slots = [provider.secret_slot(), provider.account_slot()];
    if slots.iter().all(Option::is_none) {
        return Err(Error::Usage(format!(
            "{} needs no credential; nothing to remove",
            provider.name()
        )));
    }
    for slot in slots.into_iter().flatten() {
        if key::forget(slot)? {
            eprintln!("removed from keychain {}", key::location(slot));
        } else {
            eprintln!("nothing stored in keychain {}", key::location(slot));
        }
    }
    Ok(())
}

/// Builds a single-question request around the resolved state and sends it.
async fn ask(
    client: &api::Client,
    cli: &Cli,
    target: &Target,
    questions: Value,
) -> Result<(), Error> {
    let state = resolve_state(cli)?;
    let mut body = api::request(state, &target.model, questions);
    target
        .provider
        .apply_keep_alive(&mut body, cli.keep_alive.as_deref());
    emit(&client.system_one(&body, &target.model).await?, cli.raw);
    Ok(())
}

/// Reads the state from `-s`, `-f` or stdin, keeping JSON structured.
fn resolve_state(cli: &Cli) -> Result<Value, Error> {
    let text = if let Some(state) = &cli.state {
        state.clone()
    } else if let Some(path) = &cli.file {
        std::fs::read_to_string(path).map_err(Error::Io)?
    } else if std::io::stdin().is_terminal() {
        return Err(Error::Usage(
            "no state given. Use -s \"<text>\", -f <file>, or pipe it on stdin.".to_owned(),
        ));
    } else {
        read_stdin()?
    };

    Ok(match serde_json::from_str::<Value>(&text) {
        Ok(value) if value.is_object() || value.is_array() => value,
        _ => Value::String(text),
    })
}

/// Reads a path, or stdin when the path is `-`.
fn read_source(path: &str) -> Result<String, Error> {
    if path == "-" {
        read_stdin()
    } else {
        std::fs::read_to_string(path).map_err(Error::Io)
    }
}

/// Reads all of stdin as UTF-8.
fn read_stdin() -> Result<String, Error> {
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .map_err(Error::Io)?;
    Ok(buffer)
}

/// Prints the answers, or the whole response with `--raw`, plus token usage on stderr.
fn emit(response: &api::Response, raw: bool) {
    if let Some(id) = &response.request_id {
        eprintln!("request: {id}");
    }
    let response = &response.body;
    if api::token_usage(response).is_some() {
        let (input, output) = api::token_totals(response);
        eprintln!("usage: {input} in / {output} out");
    }
    if raw {
        print_json(response);
    } else {
        print_json(response.get("answers").unwrap_or(response));
    }
}

/// Pretty-prints a JSON value, falling back to its compact form.
fn print_json(value: &Value) {
    serde_json::to_string_pretty(value).map_or_else(
        |_ignored| println!("{value}"),
        |pretty| println!("{pretty}"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory as _;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn structured_state_stays_structured() {
        let cli = Cli::parse_from(["jev", "-s", r#"{"a":1}"#, "noul", "q"]);
        let state = resolve_state(&cli).expect("state resolves");
        assert!(state.is_object());
    }

    #[test]
    fn plain_state_becomes_a_string() {
        let cli = Cli::parse_from(["jev", "-s", "just text", "noul", "q"]);
        let state = resolve_state(&cli).expect("state resolves");
        assert_eq!(state, json!("just text"));
    }

    #[test]
    fn bare_number_is_sent_as_text() {
        let cli = Cli::parse_from(["jev", "-s", "42", "noul", "q"]);
        let state = resolve_state(&cli).expect("state resolves");
        assert_eq!(state, json!("42"));
    }

    #[test]
    fn choice_requires_two_options() {
        assert!(Cli::try_parse_from(["jev", "choice", "q", "only-one"]).is_err());
    }
}
