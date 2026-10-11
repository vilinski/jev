//! Storing and retrieving credentials in the OS keychain.
//!
//! A credential is a per-user secret, not a per-project setting, so it lives in
//! the OS keychain (Keychain on macOS, Secret Service on Linux, Credential
//! Manager on Windows). Nothing is read from the working directory: a tool that
//! scans project files for a secret will eventually pick up the wrong one, and
//! it forces every repository that wants a decision call to keep its own copy.
//!
//! [`Secret`] deliberately has no `Display` and a `Debug` that prints a
//! placeholder, so it cannot leak through a log line or an error chain.

use std::borrow::Cow;
use std::env;
use std::fmt;

use crate::Error;

/// Keychain service name; entries show up under this in Keychain Access.
const SERVICE: &str = "jev";

/// Where one credential lives: an environment variable that overrides the
/// keychain, and the keychain account within [`SERVICE`].
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// Environment variable that wins over the keychain, for CI and one-offs.
    pub env: &'static str,
    /// Keychain account name.
    pub account: &'static str,
    /// What the credential is, for prompts and messages.
    pub label: &'static str,
    /// Where to get one.
    pub help: &'static str,
}

/// The `TypeSafe` API key. The account name predates the other providers and is
/// kept so an already stored key keeps working.
pub const TYPESAFE_KEY: Slot = Slot {
    env: "TYPESAFE_API_KEY",
    account: "TYPESAFE_API_KEY",
    label: "TypeSafe API key",
    help: "Create one at https://console.typesafe.ai/keys",
};

/// The Cloudflare API token. Scope it to Workers AI only.
pub const CLOUDFLARE_TOKEN: Slot = Slot {
    env: "CLOUDFLARE_API_TOKEN",
    account: "CLOUDFLARE_API_TOKEN",
    label: "Cloudflare API token",
    help: "Create one at https://dash.cloudflare.com/profile/api-tokens with the Workers AI permission only",
};

/// The Cloudflare account id. Not secret, but it travels with the token.
pub const CLOUDFLARE_ACCOUNT: Slot = Slot {
    env: "CLOUDFLARE_ACCOUNT_ID",
    account: "CLOUDFLARE_ACCOUNT_ID",
    label: "Cloudflare account id",
    help: "Find it in the Cloudflare dashboard URL or under Workers & Pages",
};

/// The `OpenAI` API key. Use a project key with only the model access it needs.
pub const OPENAI_KEY: Slot = Slot {
    env: "OPENAI_API_KEY",
    account: "OPENAI_API_KEY",
    label: "OpenAI API key",
    help: "Create one at https://platform.openai.com/api-keys (needs a billing balance)",
};

/// A credential plus a human-readable note about where it came from.
pub struct Secret {
    value: String,
    /// Where the credential was found, safe to print.
    pub source: String,
}

impl Secret {
    /// The raw value. Only the HTTP layer should call this.
    pub fn expose(&self) -> &str {
        &self.value
    }

    /// Replaces every literal occurrence of the value in `text` with a placeholder.
    ///
    /// Belt and braces: the value is never intentionally printed, but an upstream
    /// error body echoing the `Authorization` header would be caught here.
    pub fn redact<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if text.contains(&self.value) {
            Cow::Owned(text.replace(&self.value, "<redacted>"))
        } else {
            Cow::Borrowed(text)
        }
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secret")
            .field("value", &"<redacted>")
            .field("source", &self.source)
            .finish()
    }
}

/// Reads an environment variable, trimmed, treating blank as unset.
pub fn env_nonempty(name: &str) -> Option<String> {
    let value = env::var(name).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// Opens the keychain entry for `slot`.
fn entry(slot: Slot) -> Result<keyring::Entry, Error> {
    keyring::Entry::new(SERVICE, slot.account).map_err(Error::Keyring)
}

/// Finds the credential: the environment variable wins, otherwise the keychain.
pub fn load(slot: Slot, provider: &str) -> Result<Secret, Error> {
    if let Some(value) = env_nonempty(slot.env) {
        return Ok(Secret {
            value,
            source: format!("env {}", slot.env),
        });
    }

    match entry(slot)?.get_password() {
        Ok(value) => Ok(Secret {
            value,
            source: format!("keychain {SERVICE}/{}", slot.account),
        }),
        Err(keyring::Error::NoEntry) => Err(Error::Missing {
            what: slot.label,
            provider: provider.to_owned(),
            env: slot.env,
            help: slot.help,
        }),
        Err(error) => Err(Error::Keyring(error)),
    }
}

/// Stores `secret` in the keychain, replacing any existing entry.
pub fn store(slot: Slot, secret: &str) -> Result<(), Error> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(Error::Usage("empty value, nothing stored".to_owned()));
    }
    entry(slot)?.set_password(secret).map_err(Error::Keyring)
}

/// Removes the keychain entry. Reports whether there was one to remove.
pub fn forget(slot: Slot) -> Result<bool, Error> {
    match entry(slot)?.delete_credential() {
        Ok(()) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(error) => Err(Error::Keyring(error)),
    }
}

/// Where a stored value for `slot` would live, for messages.
pub fn location(slot: Slot) -> String {
    format!("{SERVICE}/{}", slot.account)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret() -> Secret {
        Secret {
            value: "ts_secret".to_owned(),
            source: "test".to_owned(),
        }
    }

    #[test]
    fn redacts_the_secret() {
        assert_eq!(secret().redact("Bearer ts_secret"), "Bearer <redacted>");
    }

    #[test]
    fn debug_never_shows_the_secret() {
        let rendered = format!("{:?}", secret());
        assert!(!rendered.contains("ts_secret"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn rejects_an_empty_value() {
        assert!(matches!(store(TYPESAFE_KEY, "   "), Err(Error::Usage(_))));
    }

    #[test]
    fn typesafe_keeps_its_original_keychain_account() {
        assert_eq!(location(TYPESAFE_KEY), "jev/TYPESAFE_API_KEY");
    }
}
