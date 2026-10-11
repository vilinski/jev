//! Optional OpenTelemetry export to Langfuse or an OTLP/HTTP collector.
//!
//! Only this application's spans are exported. Configuration and export errors
//! never replace the command's result, and shutdown is bounded for a short CLI.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use base64::Engine as _;
use opentelemetry::propagation::TextMapPropagator as _;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{Protocol, WithExportConfig as _, WithHttpConfig as _};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

use crate::key::env_nonempty;

/// Owns the exporter until all command spans have ended and been flushed.
pub struct Telemetry {
    provider: Option<SdkTracerProvider>,
}

impl Telemetry {
    /// Initializes export when requested, falling back to normal CLI operation.
    /// Must run outside Tokio: the HTTP exporter uses a blocking client.
    pub fn init(enabled: bool) -> Self {
        if !enabled
            || env_nonempty("OTEL_SDK_DISABLED").is_some_and(|v| v.eq_ignore_ascii_case("true"))
        {
            return Self { provider: None };
        }
        match Self::build() {
            Ok(provider) => Self {
                provider: Some(provider),
            },
            Err(reason) => {
                eprintln!("jev: telemetry disabled: {reason}");
                Self { provider: None }
            }
        }
    }

    /// Configures the standard OTLP exporter, or derives Langfuse configuration.
    fn build() -> Result<SdkTracerProvider, &'static str> {
        let protocol = match env_nonempty("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL")
            .or_else(|| env_nonempty("OTEL_EXPORTER_OTLP_PROTOCOL"))
            .as_deref()
        {
            None | Some("http/protobuf") => Protocol::HttpBinary,
            Some("http/json") => Protocol::HttpJson,
            Some(_) => return Err("use OTLP http/protobuf or http/json"),
        };
        let mut builder = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_protocol(protocol)
            .with_timeout(Duration::from_secs(2));

        // Explicit OTLP endpoints keep their own authentication. Langfuse keys
        // are only used for the Langfuse fallback, never sent to a collector.
        if env_nonempty("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT").is_none()
            && env_nonempty("OTEL_EXPORTER_OTLP_ENDPOINT").is_none()
        {
            let public = env_nonempty("LANGFUSE_PUBLIC_KEY")
                .ok_or("set LANGFUSE_PUBLIC_KEY and LANGFUSE_SECRET_KEY, or an OTLP endpoint")?;
            let secret = env_nonempty("LANGFUSE_SECRET_KEY").ok_or("set LANGFUSE_SECRET_KEY")?;
            let base = env_nonempty("LANGFUSE_BASE_URL")
                .or_else(|| env_nonempty("LANGFUSE_HOST"))
                .unwrap_or_else(|| "https://cloud.langfuse.com".to_owned());
            let auth =
                base64::engine::general_purpose::STANDARD.encode(format!("{public}:{secret}"));
            builder = builder
                .with_endpoint(format!(
                    "{}/api/public/otel/v1/traces",
                    base.trim_end_matches('/')
                ))
                .with_headers(HashMap::from([
                    ("Authorization".to_owned(), format!("Basic {auth}")),
                    ("x-langfuse-ingestion-version".to_owned(), "4".to_owned()),
                ]));
        }
        // Avoid printing exporter error details, which may contain configuration.
        let exporter = builder
            .build()
            .map_err(|_| "could not configure the HTTP exporter")?;
        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(
                Resource::builder()
                    .with_service_name("jev")
                    .with_attribute(opentelemetry::KeyValue::new(
                        "service.version",
                        env!("CARGO_PKG_VERSION"),
                    ))
                    .build(),
            )
            .build();
        let layer = tracing_opentelemetry::layer()
            .with_tracer(provider.tracer("jev"))
            .with_filter(Targets::new().with_target("jev", tracing::Level::TRACE));
        if tracing_subscriber::registry()
            .with(layer)
            .try_init()
            .is_err()
        {
            let _ = provider.shutdown_with_timeout(Duration::from_secs(3));
            return Err("could not install the tracing subscriber");
        }
        Ok(provider)
    }

    /// Instruments one command, optionally joining a caller's W3C trace context.
    pub fn command_span(&self, command: &str, provider: &str) -> tracing::Span {
        let span = tracing::info_span!(
            "jev.command",
            command,
            "langfuse.observation.type" = "tool",
            "langfuse.observation.metadata.provider" = provider,
            "langfuse.session.id" = tracing::field::Empty,
            "otel.status_code" = tracing::field::Empty,
            "otel.status_description" = tracing::field::Empty,
        );
        if self.provider.is_some() {
            if let Some(parent) =
                env_nonempty("JEV_TRACEPARENT").or_else(|| env_nonempty("TRACEPARENT"))
            {
                let mut carrier = HashMap::from([("traceparent".to_owned(), parent)]);
                if let Some(state) =
                    env_nonempty("JEV_TRACESTATE").or_else(|| env_nonempty("TRACESTATE"))
                {
                    carrier.insert("tracestate".to_owned(), state);
                }
                let context = TraceContextPropagator::new().extract(&carrier);
                let _ = span.set_parent(context);
            }
            record_session(&span);
        }
        span
    }

    /// Sends remaining spans on both success and failure before the process exits.
    pub fn shutdown(self) {
        if let Some(provider) = self.provider
            && provider
                .shutdown_with_timeout(Duration::from_secs(3))
                .is_err()
        {
            eprintln!("jev: telemetry export did not complete; command result is unchanged");
        }
    }
}

/// Marks an operation failed using a fixed message, without recording error bodies.
pub fn failed(span: &tracing::Span, message: &'static str) {
    span.record("otel.status_code", "ERROR");
    span.record("otel.status_description", message);
}

/// The caller's session id, read from the environment once per process.
static SESSION: OnceLock<Option<String>> = OnceLock::new();

/// Propagates a caller's session id to each observation for Langfuse v4 filtering.
pub fn record_session(span: &tracing::Span) {
    if span.is_disabled() {
        return;
    }
    if let Some(session) = SESSION.get_or_init(|| env_nonempty("JEV_SESSION_ID")) {
        span.record("langfuse.session.id", session.as_str());
    }
}
