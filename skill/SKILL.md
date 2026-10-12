---
name: jev
description: Use when a task needs a typed, calibrated judgment about text or application state rather than generated prose - routing a request to a handler, classifying or ranking items, scoring on a rubric, checking whether a claim holds, screening input, or verifying an extraction. Use whenever you were about to ask an LLM to "return JSON" for a decision, or to call the TypeSafe API directly. Triggers on jev, TypeSafe, System One, noul, calibrated probability, classify this, route this, score this, is this X, triage, rerank, guardrail.
---

# jev

`jev` is a CLI that asks **Jev**, TypeSafe's System One model, typed questions about
a piece of state and prints typed answers as JSON. Jev does not generate text; it
returns calibrated probabilities that code can branch on.

## Rules

**1. Call `jev`. Never call the API directly.** Do not `curl api.typesafe.ai` or
the Cloudflare Workers AI endpoint, do not read or echo `TYPESAFE_API_KEY` or
`CLOUDFLARE_API_TOKEN` or `OPENAI_API_KEY`, do not write your own HTTP client for them. The
credential lives in the OS keychain and `jev` reads it inside its own process, so
it never enters your context or the session transcript. Calling the API yourself
puts a credential in the transcript and will be blocked anyway.

**Providers.** `--provider typesafe` (default, model Jev), `-p cf` / `--provider cloudflare`
(Clef, `clef` or `clef-flash`), `-p oai` / `--provider openai` (OpenAI Decisions API, `gpt-6-luna`), `--provider openai-chat` (chat model via logprobs, default `gpt-4.1-mini`), `-p cel` / `--provider celeris` (`celeris-1-decision`)
and `-p ol` / `--provider ollama` (local Clef, no credential)
take the same questions and print the same shape. Use the provider the user or
project names; do not switch on your own, because calibration differs between
models and a threshold tuned on one is not valid on another.

**2. Batch independent questions into one request.** This is the single most common
mistake. Jev ingests the state once and evaluates every question against it in
parallel. TypeSafe measures a 13-question briefing as **12x cheaper and 10x faster**
batched. A second request is only warranted when an answer is needed to fetch
evidence or decide the next options.

**3. Read `probabilities`, not just `choice` or `score`.** The headline value throws
away the finding. See *Reading answers* below.

**4. Never invent an answer.** If a call fails, say so. Do not guess what Jev would
have said.

## Commands

```
jev check                         credential source and API reachability
jev models                        available models
jev run <file.json|->             full request body (use this for batches)
jev noul   <question>             yes/no -> probability
jev choice <question> <opt>...    one of N   (opt, or opt=description)
jev score  <question> <level>...  ordered rubric levels
```

State comes from `-s "<text>"`, `-f <file>` or stdin; JSON is sent structured,
anything else as a string. Answers go to stdout; request id and token usage go to
stderr, so `jev run x.json | jq` stays clean. `--raw` prints the whole response.

## The batched form

```sh
jev run - <<'EOF'
{
  "state": {"report": "Data stops arriving after ~6h; restarting the pod fixes it."},
  "questions": {
    "component":  {"type": "choice", "instructions": "Which component is at fault?",
                   "criteria": {"ingestion": "Device connectivity and intake",
                                "postgres": "Relational store or outbox",
                                "unclear": "Not determinable from this report"}},
    "is_leak":    {"type": "noul", "instructions": "Does this describe a resource leak?"},
    "needs_repro": {"type": "noul", "instructions": "Is the report missing steps to reproduce?"},
    "severity":   {"type": "score", "instructions": "How severe for a production customer?",
                   "criteria": ["Cosmetic", "Degraded, workaround exists",
                                "No workaround", "Data loss or outage"]}
  }
}
EOF
```

Run verbatim, that returns `severity` **2.13 with confidence 0.13** — and a
distribution of 0.43 on *"Degraded, workaround exists"*, 0.56 on *"Data loss or
outage"*, and 0.01 between them. The score 2.13 names a level where almost no
probability sits. Branching on `2.13` gets you the wrong answer twice; the finding
is that Jev is split between "annoying" and "serious", which is a ticket for a
human. This is why rule 3 exists.

## Reading answers

**Noul** returns the probability of yes and has no separate confidence. **0.5 means
yes and no are equally likely — not "medium intensity".** A noul asking "is this
urgent" at 0.5 means Jev cannot tell, not that it is moderately urgent.

**Choice** returns the pick, a probability per option, and a confidence that
summarises how concentrated the distribution is. Several acceptable options can
split the mass; low confidence on a harmless preference is not a problem.

**Score** returns a probability-weighted position on the levels, so the number can
land where no level sits. A score of 1.6 with 0.71 on level 1 and 0.28 on level 3
is not "level 1.6" — it is Jev torn between two specific readings, which is a
finding worth surfacing, not averaging away. Always inspect `probabilities` before
acting on a score.

**Confidence is not correctness.** It describes the distribution, not whether the
workflow is right or whether acting is safe. Typed output guarantees the interface,
not the truth.

**Jev is not deterministic.** The same request can return 0.60 one call and 0.70 the
next. Never tune a threshold on a single sample; resample before fixing a cutoff,
and treat a difference smaller than that spread as noise.

## Latency

Process startup is ~7ms, but each invocation pays a fresh TLS handshake — measured
at roughly 700ms to the API. A trivial `jev check` and a real five-question
inference take the same wall-clock time, so **the connection is the cost, not the
model**. Ten questions in one `jev run` cost one handshake; ten `jev` calls cost ten.

For scoring hundreds of items, do not loop the CLI — write code against the
[Python or JS SDK](https://docs.typesafe.ai/sdk), which pools connections and runs
async. `jev` is for the shell and for agent-initiated judgments.

## Failures

Timeouts, `429` and `5xx` (including `529 Overloaded`) are retried twice by default
with backoff and jitter, honouring `retry-after`. `--retries 0` disables it. A `422`
means the request body is malformed — check the question shape, not the state. Every
call prints a request id to stderr; quote it when reporting a problem to TypeSafe.

## Writing the questions

Question design — choosing a primitive, structuring state, splitting a broad
judgment into atomic ones, handling uncertainty — is covered by the vendor
`typesafe:typesafe-ai` skill and the [live docs](https://docs.typesafe.ai). Use
those; this skill does not restate them.

Two things that matter when you write questions into a codebase:

- **Put every question and threshold in one file.** They are the part a human must
  review, and scattered through a codebase they are effectively unreviewable.
- **Give each question enough state to answer it.** Identities, relationships,
  policies, current facts. Reference nested fields with backticks: `` `ticket.messages[0].text` ``.
