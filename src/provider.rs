//! The System One backends `jev` can talk to, and the ways they differ.
//!
//! All of them take the same request body (`model`, `state`, `questions`) and
//! return the same `answers`. What differs is data: URL, credential, request
//! id header, model restrictions and the wire protocol. Each provider is one
//! [`Spec`]; adding a provider means adding one entry here.

use clap::ValueEnum;
use serde_json::{Value, json};

use crate::Error;
use crate::key::{self, Slot};

/// Where decision requests are sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Provider {
    /// `TypeSafe`'s hosted Jev (`api.typesafe.ai`).
    Typesafe,
    /// Cloudflare Workers AI, serving `clef`, `clef-flash`, and `clef-omni`.
    #[value(aliases = ["cf", "clef"])]
    Cloudflare,
    /// A local Ollama server running `clef` or `clef-flash`; needs no credential.
    #[value(alias = "ol")]
    Ollama,
    /// `OpenAI`'s Decisions API (`/v1/decisions`, model `gpt-6-luna`).
    #[value(alias = "oai")]
    Openai,
    /// A `OpenAI` chat model made to answer through token log-probabilities.
    OpenaiChat,
    /// Celeris-1 Decision, a hosted diffusion model with a System One endpoint.
    #[value(alias = "cel")]
    Celeris,
    /// `OpenRouter`'s System One gateway for hosted decision models.
    #[value(alias = "or")]
    Openrouter,
}

/// How a provider's HTTP API relates to the System One request `jev` builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wire {
    /// The request body is sent as is and the reply already has `answers`
    /// (Cloudflare wraps it in a `result` envelope, which the client removes).
    SystemOne,
    /// `OpenAI`'s Decisions API: translated by [`crate::decisions`].
    Decisions,
    /// `OpenAI` chat completions, one per question: translated by [`crate::openai`].
    ChatLogprobs,
    /// A generative model prompted for JSON probabilities, rather than token logprobs.
    ChatStructured,
}

/// Everything that differs between providers, as data.
struct Spec {
    /// Name as spelled on the command line; also the human-readable name in errors.
    name: &'static str,
    /// API root used when `--url` is not given.
    url: &'static str,
    /// Model used when `--model` is not given.
    model: &'static str,
    /// Legacy environment variable that overrides the base URL.
    url_env: Option<&'static str>,
    /// Response header holding the id to quote in a bug report.
    request_id: Option<&'static str>,
    /// Keychain slot of the secret; `None` when no credential is needed.
    secret: Option<Slot>,
    /// Keychain slot of the account id, for providers that put it in the URL.
    account: Option<Slot>,
    /// Path of the decision request below the base, with `<account>` and `<model>` placeholders.
    path: &'static str,
    /// Path of the model listing below the base, where the provider has one.
    models_path: Option<&'static str>,
    /// The only models the provider serves, for providers that restrict them.
    fixed_models: Option<&'static [&'static str]>,
    /// Wire protocol.
    wire: Wire,
}

/// Models Cloudflare accepts in the `model` field and URL path.
const CLOUDFLARE_MODELS: [&str; 3] = ["clef", "clef-flash", "clef-omni"];

/// Celeris models, with chat models adapted separately from the decision endpoint.
const CELERIS_MODELS: [&str; 3] = ["celeris-1-decision", "celeris-1", "celeris-1-magnus"];

/// A `Spec` with the common defaults: no credential, no restrictions, plain System One.
const BASE: Spec = Spec {
    name: "",
    url: "",
    model: "",
    url_env: None,
    request_id: None,
    secret: None,
    account: None,
    path: "/v1/systemone",
    models_path: None,
    fixed_models: None,
    wire: Wire::SystemOne,
};

const TYPESAFE: Spec = Spec {
    name: "typesafe",
    url: "https://api.typesafe.ai",
    model: "jev-latest",
    url_env: Some("TYPESAFE_API_URL"),
    request_id: Some("x-typesafe-request-id"),
    secret: Some(key::TYPESAFE_KEY),
    models_path: Some("/v1/models"),
    ..BASE
};

const CLOUDFLARE: Spec = Spec {
    name: "cloudflare",
    url: "https://api.cloudflare.com/client/v4",
    model: "clef-flash",
    request_id: Some("cf-ray"),
    secret: Some(key::CLOUDFLARE_TOKEN),
    account: Some(key::CLOUDFLARE_ACCOUNT),
    path: "/accounts/<account>/ai/run/@cf/cloudflare/<model>",
    fixed_models: Some(&CLOUDFLARE_MODELS),
    ..BASE
};

const OLLAMA: Spec = Spec {
    name: "ollama",
    url: "http://localhost:11434",
    model: "clef-flash",
    models_path: Some("/api/tags"),
    ..BASE
};

const OPENAI: Spec = Spec {
    name: "openai",
    url: "https://api.openai.com",
    model: "gpt-6-luna",
    request_id: Some("x-request-id"),
    secret: Some(key::OPENAI_KEY),
    path: "/v1/decisions",
    models_path: Some("/v1/models"),
    wire: Wire::Decisions,
    ..BASE
};

const OPENAI_CHAT: Spec = Spec {
    name: "openai-chat",
    model: "gpt-4.1-mini",
    path: "/v1/chat/completions",
    wire: Wire::ChatLogprobs,
    ..OPENAI
};

const CELERIS: Spec = Spec {
    name: "celeris",
    url: "https://inference.celeris.ai",
    model: "celeris-1-decision",
    secret: Some(key::CELERIS_KEY),
    path: "/celeris-1-decision/v1/systemone",
    fixed_models: Some(&CELERIS_MODELS),
    ..BASE
};

const OPENROUTER: Spec = Spec {
    name: "openrouter",
    url: "https://openrouter.ai/api",
    model: "typesafe/jev-1.13",
    secret: Some(key::OPENROUTER_KEY),
    models_path: Some("/v1/models?output_modalities=decisions"),
    ..BASE
};

impl Provider {
    /// The provider's table entry.
    const fn spec(self) -> &'static Spec {
        match self {
            Self::Typesafe => &TYPESAFE,
            Self::Cloudflare => &CLOUDFLARE,
            Self::Ollama => &OLLAMA,
            Self::Openai => &OPENAI,
            Self::OpenaiChat => &OPENAI_CHAT,
            Self::Celeris => &CELERIS,
            Self::Openrouter => &OPENROUTER,
        }
    }

    /// Name as it is spelled on the command line.
    pub const fn name(self) -> &'static str {
        self.spec().name
    }

    /// API root used when `--url` is not given.
    pub const fn default_url(self) -> &'static str {
        self.spec().url
    }

    /// Model used when `--model` is not given.
    pub const fn default_model(self) -> &'static str {
        self.spec().model
    }

    /// Legacy environment variable that overrides the base URL, if any.
    pub const fn url_env(self) -> Option<&'static str> {
        self.spec().url_env
    }

    /// Wire protocol the client has to speak.
    pub fn wire(self, model: &str) -> Wire {
        if self == Self::Celeris && matches!(model.trim(), "celeris-1" | "celeris-1-magnus") {
            Wire::ChatStructured
        } else {
            self.spec().wire
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
        self.spec().request_id
    }

    /// Keychain slot of the secret, or `None` when no credential is needed.
    pub const fn secret_slot(self) -> Option<Slot> {
        self.spec().secret
    }

    /// Keychain slot of the account id, for providers that put it in the URL.
    pub const fn account_slot(self) -> Option<Slot> {
        self.spec().account
    }

    /// URL that receives a System One request for `model`.
    pub fn system_one_url(self, base: &str, account: Option<&str>, model: &str) -> String {
        let template = if self.wire(model) == Wire::ChatStructured {
            "/<model>/v1/chat/completions"
        } else {
            self.spec().path
        };
        let path = template
            .replace("<account>", account.unwrap_or_default())
            .replace("<model>", model.trim());
        format!("{base}{path}")
    }

    /// URL listing the available models, where the provider has one.
    pub fn models_url(self, base: &str) -> Option<String> {
        self.spec().models_path.map(|path| format!("{base}{path}"))
    }

    /// Rejects a model outside a provider's fixed protocol-specific set.
    pub fn check_model(self, model: &str) -> Result<(), Error> {
        let spec = self.spec();
        match spec.fixed_models {
            Some(models) if !models.contains(&model.trim()) => Err(Error::Usage(format!(
                "{} serves {}; got `{model}`. Pass --model {}.",
                spec.name,
                models.join(" and "),
                spec.model
            ))),
            _ => Ok(()),
        }
    }

    /// Models a provider with a fixed set serves, for `jev models` when there is no listing endpoint to call.
    pub const fn static_models(self) -> Option<&'static [&'static str]> {
        let spec = self.spec();
        if spec.models_path.is_none() {
            spec.fixed_models
        } else {
            None
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
        assert!(Provider::Cloudflare.check_model("clef-omni").is_ok());
        assert!(Provider::Cloudflare.check_model(" clef-flash ").is_ok());
        assert!(Provider::Typesafe.check_model("jev-latest").is_ok());
    }

    #[test]
    fn openai_uses_the_decisions_endpoint_and_openai_chat_uses_completions() {
        assert_eq!(
            Provider::Openai.system_one_url("https://api.openai.com", None, "gpt-6-luna"),
            "https://api.openai.com/v1/decisions"
        );
        assert_eq!(
            Provider::OpenaiChat.system_one_url("https://api.openai.com", None, "gpt-4.1-mini"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            Provider::Openai.secret_slot().map(|s| s.env),
            Some("OPENAI_API_KEY")
        );
        assert!(Provider::Openai.account_slot().is_none());
    }

    #[test]
    fn celeris_has_its_own_systemone_path_and_key() {
        assert_eq!(
            Provider::Celeris.system_one_url(
                "https://inference.celeris.ai",
                None,
                "celeris-1-decision"
            ),
            "https://inference.celeris.ai/celeris-1-decision/v1/systemone"
        );
        assert_eq!(
            Provider::Celeris.secret_slot().map(|s| s.env),
            Some("CELERIS_API_KEY")
        );
    }

    #[test]
    fn only_cloudflare_needs_an_account() {
        assert!(Provider::Cloudflare.account_slot().is_some());
        assert!(Provider::Typesafe.account_slot().is_none());
        assert!(Provider::Ollama.secret_slot().is_none());
    }
}
