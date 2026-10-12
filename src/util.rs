//! Small helpers shared by the translators for providers that do not speak System One natively.

use serde_json::Value;

/// Renders the state as prompt text: text as is, structures as pretty JSON.
pub fn render_state(state: &Value) -> String {
    match state {
        Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_ignored| other.to_string()),
    }
}

/// Rounds to four decimals, like the other providers' output.
pub fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}
