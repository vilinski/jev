"""The compared models: one entry each. Everything else (runners, scoring, report, site) reads this table.

To add a provider: add `jev` support (src/provider.rs), one entry here, then `mise run bench`.
"""
from dataclasses import dataclass

@dataclass(frozen=True)
class Model:
    key: str          # internal id and legend label in charts
    label: str        # short display name
    sub: str          # vendor and model id, for tooltips
    color: str
    provider: str     # value of `jev --provider`
    stem: str         # results/<stem>.json (own set) and results/hf_<stem>.json (public sample)
    price_in: float   # USD per 1M input tokens (list price, checked 2026-10-12)
    price_out: float  # USD per 1M output tokens

MODELS = [
    Model("Jev", "Jev", "TypeSafe · jev-latest", "#2f6fed", "typesafe", "jev", 0.042, 0.0),
    Model("Celeris-1", "Celeris", "Celeris · celeris-1-decision", "#c0289c", "celeris", "celeris", 0.04, 0.0),
    Model("Clef", "Clef", "Cloudflare · clef-flash", "#e8710a", "cloudflare", "clef", 0.038, 0.0),
    Model("OpenAI Decisions", "OpenAI Decisions", "OpenAI · gpt-6-luna, /v1/decisions (beta)", "#12a36b", "openai", "openai", 0.10, 0.0),
    Model("OpenAI chat DIY", "OpenAI chat (DIY)", "OpenAI · gpt-4.1-mini + logprobs", "#7d8593", "openai-chat", "openaichat", 0.40, 1.60),
]
BY_KEY = {m.key: m for m in MODELS}
CHAT = BY_KEY["OpenAI chat DIY"]   # the do-it-yourself chat route: not a dedicated decision API
RESULTS = "results"
