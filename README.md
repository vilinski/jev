# jev

[![CI](https://github.com/vilinski/jev/actions/workflows/ci.yml/badge.svg)](https://github.com/vilinski/jev/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![Rust 2024](https://img.shields.io/badge/rust-2024%20edition-orange?logo=rust)](https://doc.rust-lang.org/edition-guide/rust-2024/)
[![clippy: pedantic + nursery](https://img.shields.io/badge/clippy-pedantic%20%2B%20nursery-success?logo=rust)](Cargo.toml)
[![unsafe: denied](https://img.shields.io/badge/unsafe-denied-brightgreen)](Cargo.toml)
[![Providers: TypeSafe | Cloudflare | Ollama | OpenAI](https://img.shields.io/badge/providers-TypeSafe%20%7C%20Cloudflare%20%7C%20Ollama%20%7C%20OpenAI-8A2BE2)](#providers)
[![Platforms: macOS | Linux | Windows](https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey)](#install)
[![OpenTelemetry](https://img.shields.io/badge/telemetry-OpenTelemetry%20%2B%20Langfuse-425cc7?logo=opentelemetry)](#telemetry)

CLI for **System One decision models**: [TypeSafe](https://docs.typesafe.ai) Jev,
Cloudflare Clef, Clef on a local Ollama, or OpenAI chat models read through their log-probabilities. It sends a piece of *state* plus typed
*questions* and prints the typed answers as JSON.

These models are not chat models. They do not generate text, write code or hold a
conversation. They return calibrated probabilities that ordinary code can branch
on — which makes them useful for the things we otherwise solve with brittle parsing
or a hand-written rule per case.

## Providers

All four speak the same request (`model`, `state`, `questions`) and return the
same `answers`; `jev` hides the differences.

| `--provider` | Where it runs | Credential | Default model |
|---|---|---|---|
| `typesafe` (default) | `api.typesafe.ai` | API key, keychain or `TYPESAFE_API_KEY` | `jev-latest` |
| `cloudflare` | Workers AI | API token + account id, keychain or `CLOUDFLARE_API_TOKEN` / `CLOUDFLARE_ACCOUNT_ID` | `clef-flash` |
| `ollama` | `localhost:11434` | none | `clef-flash` |
| `openai` (`oai`) | `api.openai.com/v1/decisions` (public beta) | API key, keychain or `OPENAI_API_KEY` | `gpt-6-luna` |
| `openai-chat` | `api.openai.com` chat completions | same `OPENAI_API_KEY` | `gpt-4.1-mini` |
| `celeris` (`cel`) | `inference.celeris.ai` | API key from <https://console.celeris.ai>, keychain or `CELERIS_API_KEY` | `celeris-1-decision` |

Pick one with `--provider` (`-p`; `cf`, `ol` and `oai` work for `cloudflare`, `ollama` and `openai`), or once with `JEV_PROVIDER=cloudflare`. `--model` and
`JEV_MODEL` override the default; `--url` / `JEV_URL` point at a gateway, a
different port or a mock.

Cloudflare serves `clef` (27B) and `clef-flash` (9B), and `jev` rejects any other
model name before sending anything. Its REST API puts the answer under a `result`
key; `jev` removes that envelope so every provider prints the same shape.
`openai` calls OpenAI's own Decisions API (`POST /v1/decisions`, only `gpt-6-luna` so far, $0.10 per 1M input tokens, no output charge); `jev` translates the System One request into its `predicate`/`choice`/`score` array format and back. `openai-chat` is a do-it-yourself alternative for chat models: `jev` asks each question as one chat
completion that must answer with a single token (`yes`/`no`, an option letter or
a level digit) and reads the probabilities from that token's log-probabilities.
The questions of a request run concurrently and come back in the usual shape;
`confidence` is `1 - entropy/ln(n)` of the distribution. The model must support
`logprobs` (`gpt-4.1-mini`, `gpt-4.1`, `gpt-4o`; reasoning models reject it),
and choices are limited to 26 options and scores to 10 levels. Because the
prompt is written by `jev`, an OpenAI result measures that prompt as much as
the model. Cost is one request per question, so batching saves latency, not money.
Ollama needs a model pulled first (`ollama pull clef-flash`, about 11 GB) and
`--keep-alive 30m` (or `JEV_KEEP_ALIVE`) keeps it loaded between calls.

## Why a tool and not `curl`

* **The credential lives in the OS keychain, once.** Not in a `.env`, not in
  `mise.toml`, not copied into every repository that wants a decision call.
* **Nothing is read from the working directory.** A tool that scans project
  files for a secret eventually picks up the wrong one, and it pushes every
  project back into keeping its own copy.
* **Coding agents can call it without seeing the credential.** It is read inside
  this process, marked sensitive on the request header, and redacted from every
  byte printed — so `jev` can be allow-listed for an agent while the key itself
  stays out of the agent's context and its transcript.

## Install

```sh
cargo install --git https://github.com/vilinski/jev   # or, from a checkout: cargo install --path . --root ~/.local
jev login                                   # paste the key; input is not echoed
jev check
# provider: typesafe
# key:      found (keychain jev/TYPESAFE_API_KEY)
# api:      reachable (https://api.typesafe.ai)
# model:    jev-latest
```

Create a TypeSafe key at <https://console.typesafe.ai/keys>. For Cloudflare run
`jev login --provider cloudflare`, which asks for the token (hidden) and the
account id; create the token at
<https://dash.cloudflare.com/profile/api-tokens> with the **Workers AI**
permission only, and give it an expiry. For OpenAI run
`jev login --provider openai` and paste a key created at
<https://platform.openai.com/api-keys> (Create new secret key; restrict it to a
project with only the models you need, and add a few dollars under Billing
first, otherwise requests fail with `insufficient_quota`). `jev logout --provider <name>` removes
the stored values again.

Entries are stored under service `jev` — Keychain on macOS, Secret Service on
Linux, Credential Manager on Windows:

| Provider | Keychain account(s) |
|---|---|
| typesafe | `TYPESAFE_API_KEY` |
| cloudflare | `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID` |
| openai | `OPENAI_API_KEY` |

### Overriding the stored credential

The environment variable wins over the keychain. That is the escape hatch for CI,
where there is no keychain to unlock; everywhere else, leave it unset and let the
keychain answer.

## Usage

```
jev login                         store the provider's credential in the OS keychain
jev logout                        remove it again
jev check                         report credential source and API reachability
jev models                        list the models this account may use
jev run <file.json|->             send a full request body verbatim
jev noul   <question>             yes/no -> probability
jev choice <question> <opt>...    one of N   (opt, or opt=description)
jev score  <question> <level>...  ordered rubric levels
```

State comes from `-s "<text>"`, `-f <file>` or stdin. A file or stdin holding
JSON is sent as structured state; anything else is sent as a string.

`--raw` prints the whole response instead of just `.answers`. The request id and
token usage go to stderr, so they stay out of a piped JSON stream — quote the
request id when reporting a problem (`x-typesafe-request-id` for TypeSafe, the
`cf-ray` id for Cloudflare).

`jev check` against Cloudflare sends one tiny test request, because Workers AI has
no endpoint that lists decision models; `jev models` prints the documented pair.

Timeouts, rate limits (`429`) and server errors (`5xx`, including `529
Overloaded`) are retried twice by default with exponential backoff and jitter,
honouring a `retry-after` header when the server sends one. `--retries <n>`
changes the count; `--retries 0` disables it.

### Latency

Process startup is about 7 ms and the keychain read about another 7 ms. The rest
of a call is network, and every invocation pays a fresh connection (DNS, TCP, TLS)
because the process is new each time. The practical consequence: batch questions
into one request rather than looping `jev` over items in a shell. One request with
ten questions costs one handshake; ten `jev` calls cost ten. For scoring hundreds
of items, use one of the [SDKs](https://docs.typesafe.ai/sdk) with a pooled, async
client instead — or a local Ollama, where there is no TLS handshake at all.

### Examples

A yes/no judgment on a log line:

```sh
jev noul "Does this indicate the queue is backing up rather than a transient error?" \
  -f /var/log/app/queue.log
```

Routing an issue report to a component, with Clef instead of Jev:

```sh
jev -p cf choice "Which component is at fault?" \
  database=Relational store, outbox or migrations \
  ingestion=Device connectivity and data intake \
  unclear=Not determinable from this report \
  -f issue.md
```

Severity on a rubric:

```sh
jev score "How severe is this for a production customer?" \
  "Cosmetic, no functional impact" \
  "Degraded but usable, workaround exists" \
  "Feature unavailable, no workaround" \
  "Data loss or total outage" \
  -s "$(git log -1 --format=%B)"
```

Several questions at once — this is the form you usually want, since the model
ingests the state once and evaluates every question against it in parallel:

```sh
jev run - <<'EOF'
{
  "state": {"report": "Data stops arriving after ~6h, restart fixes it."},
  "questions": {
    "component":  {"type": "choice", "instructions": "Which component is at fault?",
                   "criteria": {"database": null, "ingestion": null}},
    "is_leak":    {"type": "noul", "instructions": "Does this describe a resource leak?"},
    "needs_repro": {"type": "noul", "instructions": "Is the report missing steps to reproduce?"}
  }
}
EOF
```

## Telemetry

`--telemetry` (or `JEV_TELEMETRY=true`) exports OpenTelemetry traces over HTTP.
It works with every provider (TypeSafe, Cloudflare, Ollama and OpenAI); an OpenAI request records one model call with one attempt span per question. Telemetry is off by default;
`login` and `logout` never export traces.

For Langfuse, provide `LANGFUSE_PUBLIC_KEY`, `LANGFUSE_SECRET_KEY` and optionally
`LANGFUSE_BASE_URL` (defaults to `https://cloud.langfuse.com`; `LANGFUSE_HOST` is
also accepted). The exporter derives the OTLP endpoint, Basic Auth and the
Langfuse v4 ingestion header. These variables are read from the process environment;
`jev` does not read `.env` files or project configuration itself.

```sh
# Run where your environment manager supplies the Langfuse variables.
jev --telemetry noul "Does this describe a billing problem?" \
  -s "My invoice was charged twice."

# Also capture the state, questions and returned answers.
jev --telemetry-content -p cf choice "Which team should handle this?" \
  billing technical unclear -s "My invoice was charged twice."
```

Find `jev.command` traces in Langfuse. Each inference has a `jev.system_one`
generation containing the provider, model, available token counts and request id.
`http.attempt` child spans include the attempt number and HTTP status; a separate
`retry.backoff` span shows each retry delay. Failed attempts are marked as errors,
while a recovered inference remains successful.

`--telemetry-content` (or `JEV_TELEMETRY_CONTENT=true`) also enables telemetry and
adds request/response JSON. Without it, state, questions, answers and upstream
error bodies are excluded. The current provider's API credential is redacted from
captured content; other sensitive input is not automatically masked. Authorization
headers and request URLs are never recorded in spans. Stdout remains the normal JSON.

For a generic OTLP/HTTP collector, set `OTEL_EXPORTER_OTLP_ENDPOINT` (the exporter
appends `/v1/traces`) or `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` (the complete trace
URL). These take precedence over Langfuse configuration and use standard
`OTEL_EXPORTER_OTLP_HEADERS` / `OTEL_EXPORTER_OTLP_TRACES_HEADERS` instead of
Langfuse credentials. The default protocol is `http/protobuf`; `http/json` is also
supported via the corresponding OTLP protocol variable. For Langfuse behind a
collector, configure its Basic Auth and `x-langfuse-ingestion-version: 4` header
on the collector's export to Langfuse.

Calling agents can pass `TRACEPARENT` and optional `TRACESTATE` in W3C format to
attach the command to an existing trace. `JEV_TRACEPARENT` / `JEV_TRACESTATE`
override those values when provided. Claude Code's beta tracing supplies
`TRACEPARENT` to Bash subprocesses, so an enabled `jev` export joins its tool span;
configure Jev's export destination separately (for example, with the Langfuse
variables above), since Claude Code does not forward `OTEL_*` variables.
See [Claude Code monitoring](https://code.claude.com/docs/en/monitoring-usage).
`JEV_SESSION_ID` (read once per process) groups runs
and is recorded on every inference/attempt/backoff observation. These settings
are optional; otherwise each invocation creates its own trace.

The exporter uses a 2-second HTTP timeout and flushes at process exit on both
success and failure, with a 3-second shutdown deadline. Export/configuration
failures do not change the model result or exit status. `OTEL_SDK_DISABLED=true` (case-insensitive)
disables export even when a telemetry flag is set. Cost calculation depends on
the model pricing available in your Langfuse instance; token counts alone do not
define a model's price.

See [Langfuse's OpenTelemetry guide](https://langfuse.com/integrations/native/opentelemetry).

## Notes

* Batch questions into one request wherever they are independent. TypeSafe's docs
  measure a 13-question briefing as 12.2x cheaper and 10x faster batched, with no
  change in the answers.
* Keep the questions and any thresholds in one place in the calling code. They
  are the part a human needs to review; scattered through a codebase they are
  effectively unreviewable.
* Typed output guarantees the interface, not the truth. Validate against your own
  data before trusting a threshold, and compare providers on the same cases before
  switching: the formats match, the calibration is not guaranteed to.

## Findings

A measured comparison of Jev, Clef and OpenAI (latency, accuracy, calibration) is published at
<https://ai.vilinski.dev/jev-benchmark/>; its scripts and raw results are in
[`docs/benchmark`](docs/benchmark). The notes below are from everyday use against the three providers; numbers are one machine's.

* **Cloudflare validates question ids.** Ids must match `^[A-Za-z0-9_.-]{1,100}$`.
  An id such as `gate::urgent` is accepted by TypeSafe but rejected by Cloudflare with
  an opaque `HTTP 422 ... Request body failed validation`. Option names in a `choice`
  are not validated, so `sc:recommend` is fine there. Use plain ids when a request may
  run against `-p cf`.
* **From a laptop the REST route has a floor of about 0.3 s.** TLS is up after about
  50 ms, but even a request that involves no model took 270-340 ms to answer, and a
  one-question `jev -p cf` call took 0.3-0.7 s (a request to the real skill list,
  two calls in a row, 1.0-1.6 s). The first call after a pause was a cold start of
  7-9 s. The vendors' millisecond figures are model time; do not expect them end to end.
* **So keep it out of hot paths.** An attempt to rank every prompt against a skill
  list through `jev` (replacing the model's own skill choice) added 0.9-1.2 s to each
  prompt and sent the prompts to a hosted service. Its picks were acceptable on 26 of
  32 test runs, but no comparison with the model choosing for itself was measured, so
  there was no evidence of a gain to set against the cost. It was removed. Where `jev` earns its place is work that
  runs asynchronously or in batches, where a second per call does not matter and a
  judgment replaces brittle parsing.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same checks on Linux, macOS and Windows. See [AGENTS.md](AGENTS.md)
for the code layout and conventions.

`tests/providers.rs` runs the real binary against a local one-shot HTTP server and
checks URL, headers, model, envelope handling and that secrets stay out of output.

## Agent skill

`skill/SKILL.md` teaches coding agents to use this tool: call `jev` rather than the
API, batch independent questions, read the probability distribution rather than the
headline value, and treat low confidence as disagreement. Install it by symlinking
the directory, so it stays in sync with the tool:

```sh
ln -sfn "$PWD/skill" ~/.claude/skills/jev          # Claude Code
```

For other agents, point their skills directory at the same folder. Question design
itself is left to the vendor `typesafe-ai` skill and the docs.

See the [primitives](https://docs.typesafe.ai/primitives),
[patterns](https://docs.typesafe.ai/patterns) and
[cookbooks](https://docs.typesafe.ai/cookbooks) for how to shape questions.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
