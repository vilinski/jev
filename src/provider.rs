//! The System One backends `jev` can talk to, and the ways they differ.
//!
//! All of them take the same request body (`model`, `state`, `questions`) and
//! return the same `answers`. What changes is the URL, the credential, the
//! header that carries a request id, and whether the answer arrives wrapped in
//! a Cloudflare-style envelope.

use clap::ValueEnum;
use serde_json::{Value, json};

use crate::Error;
use crate::key::{self, Slot};

/// Where decision requests are sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Provider {
    /// `TypeSafe`'s hosted Jev (`api.typesafe.ai`).
    Typesafe,
    /// Cloudflare Workers AI, serving `clef` and `clef-flash`.
    #[value(alias = "cf")]
    Cloudflare,
    /// A local Ollama server running `clef` or `clef-flash`; needs no credential.
    #[value(alias = "ol")]
    Ollama,
    /// `OpenAI` chat completions, answered through token log-probabilities.
    #[value(alias = "oai")]
    Openai,
}

/// Models Cloudflare accepts in the `model` field and URL path.
const CLOUDFLARE_MODELS: [&str; 2] = ["clef", "clef-flash"];

impl Provider {
    /// Name as it is spelled on the command line.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Typesafe => "typesafe",
            Self::Cloudflare => "cloudflare",
            Self::Ollama => "ollama",
            Self::Openai => "openai",
        }
    }

    /// API root used when `--url` is not given.
    pub const fn default_url(self) -> &'static str {
        match self {
            Self::Typesafe => "https://api.typesafe.ai",
            Self::Cloudflare => "https://api.cloudflare.com/client/v4",
            Self::Ollama => "http://localhost:11434",
            Self::Openai => "https://api.openai.com",
        }
    }

    /// Model used when `--model` is not given.
    pub const fn default_model(self) -> &'static str {
        match self {
            Self::Typesafe => "jev-latest",
            Self::Cloudflare | Self::Ollama => "clef-flash",
            Self::Openai => "gpt-4.1-mini",
        }
    }

    /// Legacy environment variable that overrides the base URL, if any.
    pub const fn url_env(self) -> Option<&'static str> {
        match self {
            Self::Typesafe => Some("TYPESAFE_API_URL"),
            Self::Cloudflare | Self::Ollama | Self::Openai => None,
        }
    }

    /// Adds Ollama's `keep_alive` to a request body when one was asked for.
    pub fn apply_keep_alive(self, body: &mut Value, keep_alive: Option<&str>) {
        let (Self::Ollama, Some(keep), Some(object)) = (self, keep_alive, body.as_object_mut())
        else {
            return;
        };
        let value = keep
            .parse::<i64>()
            .map_or_else(|_ignored| json!(keep), |n| json!(n));
        object.insert("keep_alive".to_owned(), value);
    }

    /// Response header holding the id to quote in a bug report.
    pub const fn request_id_header(self) -> Option<&'static str> {
        match self {
            Self::Typesafe => Some("x-typesafe-request-id"),
            Self::Cloudflare => Some("cf-ray"),
            Self::Ollama => None,
            Self::Openai => Some("x-request-id"),
        }
    }

    /// Keychain slot of the secret, or `None` when no credential is needed.
    pub const fn secret_slot(self) -> Option<Slot> {
        match self {
            Self::Typesafe => Some(key::TYPESAFE_KEY),
            Self::Cloudflare => Some(key::CLOUDFLARE_TOKEN),
            Self::Openai => Some(key::OPENAI_KEY),
            Self::Ollama => None,
        }
    }

    /// Keychain slot of the account id, for providers that put it in the URL.
    pub const fn account_slot(self) -> Option<Slot> {
        match self {
            Self::Cloudflare => Some(key::CLOUDFLARE_ACCOUNT),
            Self::Typesafe | Self::Ollama | Self::Openai => None,
        }
    }

    /// URL that receives a System One request for `model`.
    pub fn system_one_url(self, base: &str, account: Option<&str>, model: &str) -> String {
        match self {
            Self::Typesafe | Self::Ollama => format!("{base}/v1/systemone"),
            Self::Openai => format!("{base}/v1/chat/completions"),
            Self::Cloudflare => format!(
                "{base}/accounts/{}/ai/run/@cf/cloudflare/{}",
                account.unwrap_or_default(),
                model.trim()
            ),
        }
    }

    /// URL listing the available models, where the provider has one.
    pub fn models_url(self, base: &str) -> Option<String> {
        match self {
            Self::Typesafe | Self::Openai => Some(format!("{base}/v1/models")),
            Self::Ollama => Some(format!("{base}/api/tags")),
            Self::Cloudflare => None,
        }
    }

    /// Rejects a model name the provider cannot serve, before any request.
    pub fn check_model(self, model: &str) -> Result<(), Error> {
        if self == Self::Cloudflare && !CLOUDFLARE_MODELS.contains(&model.trim()) {
            return Err(Error::Usage(format!(
                "Cloudflare serves {}; got `{model}`. Pass --model clef-flash.",
                CLOUDFLARE_MODELS.join(" and ")
            )));
        }
        Ok(())
    }

    /// Models Cloudflare serves, for `jev models`, which has no listing endpoint to call.
    pub const fn static_models(self) -> Option<&'static [&'static str]> {
        match self {
            Self::Cloudflare => Some(&CLOUDFLARE_MODELS),
            Self::Typesafe | Self::Ollama | Self::Openai => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typesafe_and_ollama_share_the_systemone_path() {
        for provider in [Provider::Typesafe, Provider::Ollama] {
            assert_eq!(
                provider.system_one_url("http://h", None, "m"),
                "http://h/v1/systemone"
            );
        }
    }

    #[test]
    fn cloudflare_puts_account_and_model_in_the_path() {
        assert_eq!(
            Provider::Cloudflare.system_one_url("https://cf/v4", Some("acct"), "clef-flash"),
            "https://cf/v4/accounts/acct/ai/run/@cf/cloudflare/clef-flash"
        );
    }

    #[test]
    fn cloudflare_rejects_jev_model_names() {
        assert!(Provider::Cloudflare.check_model("jev-latest").is_err());
        assert!(Provider::Cloudflare.check_model("clef").is_ok());
        assert!(Provider::Cloudflare.check_model(" clef-flash ").is_ok());
        assert!(Provider::Typesafe.check_model("jev-latest").is_ok());
    }

    #[test]
    fn openai_uses_chat_completions_and_its_own_key() {
        assert_eq!(
            Provider::Openai.system_one_url("https://api.openai.com", None, "gpt-4.1-mini"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            Provider::Openai.secret_slot().map(|s| s.env),
            Some("OPENAI_API_KEY")
        );
        assert!(Provider::Openai.account_slot().is_none());
    }

    #[test]
    fn only_cloudflare_needs_an_account() {
        assert!(Provider::Cloudflare.account_slot().is_some());
        assert!(Provider::Typesafe.account_slot().is_none());
        assert!(Provider::Ollama.secret_slot().is_none());
    }
}
