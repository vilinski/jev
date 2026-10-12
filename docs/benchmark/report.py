"""Build report.html (the benchmark report), og.png and data.json from results/. See AGENTS.md for the workflow.

    python3 report.py          # URL of the published report: $REPORT_URL, default https://ai.vilinski.dev/jev-benchmark/
"""
import datetime, json, os, statistics as st

from charts import bars, strip
from models import CHAT, MODELS
from scoring import bootstrap_cis, load_all, wrong_items
from site_lib import AUTHOR, PROFILE, REPO, SITE, esc, meta, og_frame

now = datetime.date.today().isoformat()
URL = os.environ.get("REPORT_URL", f"{SITE}/jev-benchmark/").strip()
DM_URL = f"{SITE}/decision-models/"
R, H = load_all()                    # own-set and public-sample analysis by model key
names = [m.key for m in MODELS]
M = {m.key: m for m in MODELS}
pc = lambda v: f"{v:.0%}"
ms = lambda v: f"{v*1000:.0f} ms"
d = lambda f: {n: f(R[n]) for n in names}
B = lambda *a, **k: bars(MODELS, *a, **k)

# --- cost: list price per 1M input tokens x measured input tokens per call (+ output tokens where they are charged)
OUTTOK = {m.key: R[m.key]["tok"][1] if m.price_out else 0 for m in MODELS}
COST = {m.key: R[m.key]["tok"][0] * m.price_in + OUTTOK[m.key] * m.price_out for m in MODELS}    # USD per 1M decisions, own inputs
HCOST = {m.key: H[m.key]["tok"] * m.price_in + OUTTOK[m.key] * m.price_out for m in MODELS}      # same on the public-benchmark mix
dedicated = [n for n in names if n != CHAT.key]
charts_cost = [B("Cost per 1,000 decisions", "list price × tokens measured in this benchmark, one question per call", {n: COST[n] / 1000 for n in names}, lambda v: f"${v:.4f}", "low"),
               B("List price per 1M input tokens", "output free except OpenAI ($1.60 per 1M)", {m.key: m.price_in for m in MODELS}, lambda v: f"${v:.3f}", "low"),
               B("Input tokens per call", "each vendor counts with its own tokenizer, so this is billing size, not content size", d(lambda o: o["tok"][0]), lambda v: f"{v:.0f}", None)]
cost_rows = "".join(f'<tr><td style="color:{m.color}"><b>{esc(m.label)}</b></td><td>${m.price_in:.3f}</td><td>{"free" if m.price_out == 0 else "$%.2f" % m.price_out}</td><td>{R[m.key]["tok"][0]:.0f}</td><td>${COST[m.key]:.2f}</td><td>${COST[m.key]/1000:.4f}</td></tr>' for m in MODELS)
cost_tbl = f'<div class="tw"><table><tr><th>Model</th><th>Input / 1M tokens</th><th>Output / 1M tokens</th><th>Input tokens per call</th><th>Per 1M decisions</th><th>Per 1,000 decisions</th></tr>{cost_rows}</table></div>'

# --- public sample
th = "".join(f'<th style="color:{m.color}">{esc(m.label)}</th>' for m in MODELS)
CI = bootstrap_cis({n: H[n]["accs"] for n in names})
NPUB = H[names[0]]["n"]; PER = NPUB // 22   # items in the public sample and per dataset
HW = st.mean((CI[n][1] - CI[n][0]) / 2 for n in dedicated) * 100   # typical 95% half-width, in points
pub_charts = [B("jev-bench accuracy", f"{NPUB} items: {PER} random test items from each of the 22 datasets", {n: H[n]["acc"] for n in names}, pc, "high", 1),
              B("jev-bench Brier score", "sum over options of (probability − label)²; lower is better", {n: H[n]["brier"] for n in names}, lambda v: f"{v:.3f}", "low"),
              B("Latency p50 on real inputs", "inputs from 30 to 2,800 characters", {n: H[n]["p50"] for n in names}, ms, "low"),
              B("Cost per 1,000 decisions on this mix", f"list price × tokens measured on the {NPUB} items", {n: HCOST[n] / 1000 for n in names}, lambda v: f"${v:.4f}", "low")]
def shade(v, lo=0.2, hi=1.0):
    t = max(0, min(1, (v - lo) / (hi - lo))); return f"background:rgba(18,163,107,{0.08+0.5*t:.2f})"
heat = "".join("<tr><td>" + esc(sx) + "</td>" + "".join(f'<td style="{shade(H[n]["per"].get(sx, 0))}">{H[n]["per"].get(sx, 0):.0%}</td>' for n in names) + "</tr>" for sx in sorted(H[names[0]]["per"]))
pub_tbl = "<div class=\"tw\"><table><tr><th>Model</th><th>Accuracy (95% CI)</th><th>Brier</th><th>p50 / p95</th><th>Failed or refused</th></tr>" + "".join(f'<tr><td style="color:{m.color}"><b>{esc(m.label)}</b></td><td>{H[m.key]["acc"]:.1%} <small>({CI[m.key][0]:.0%}–{CI[m.key][1]:.0%})</small></td><td>{H[m.key]["brier"]:.3f}</td><td>{H[m.key]["p50"]*1000:.0f} / {H[m.key]["p95"]*1000:.0f} ms</td><td>{H[m.key]["fail"]} of {H[m.key]["n"]}</td></tr>' for m in MODELS) + "</table></div>"
pub_heat = '<div class="tw"><table class="heat"><tr><th>Dataset (' + str(PER) + ' items each)</th>' + th + '</tr>' + heat + '</table></div>'

# --- own set
charts = [strip(MODELS, R),
          B("Latency p50", "median single call, incl. process start + TLS", d(lambda o: o["lat"]["p50"]), ms, "low"),
          B("Latency p95", "slowest 5% of calls", d(lambda o: o["lat"]["p95"]), ms, "low"),
          B("10 questions, one request", "all questions about one state; OpenAI runs them as 10 parallel calls", d(lambda o: o["batch"]), ms, "low"),
          B("10 questions, 10 calls in sequence", "what you pay if you do not batch", d(lambda o: o["sep"]), lambda v: f"{v:.1f} s", "low"),
          B("Accuracy: yes/no", "24 items × 3 runs, vague items excluded", d(lambda o: o["acc"]["noul"][0]), pc, "high", 1),
          B("Accuracy: routing", "20 items × 3 runs, 6 options incl. “unclear”", d(lambda o: o["acc"]["choice"][0]), pc, "high", 1),
          B("Accuracy: severity (exact level)", "16 items × 3 runs, 4 levels", d(lambda o: o["acc"]["score_exact"][0]), pc, "high", 1),
          B("Severity error", "mean distance to the labelled level, in levels", d(lambda o: o["mae"]), lambda v: f"{v:.2f}", "low"),
          B("Calibration: yes/no (Brier)", "squared error of the probability", d(lambda o: o["brier"]["noul"]), lambda v: f"{v:.3f}", "low"),
          B("Calibration: routing (Brier)", "squared error over all options", d(lambda o: o["brier"]["choice"]), lambda v: f"{v:.3f}", "low"),
          B("Confidence when right vs wrong", "a useful confidence is clearly lower when wrong", "", lambda v: f"{v:.2f}", "high", 1, pairs={n: (R[n]["conf_ok"], R[n]["conf_bad"]) for n in names}),
          B("Accuracy when confidence ≥ 0.8", "how far you can trust a confident answer", d(lambda o: o["hi"][0]), pc, "high", 1),
          B("Accuracy when confidence < 0.8", "a good model is worse here, so these can go to a human", d(lambda o: o["lo"][0]), pc, "low", 1),
          B("Says “can’t tell” on vague input", "share of 4 vague yes/no items answered 0.25–0.75", d(lambda o: o["amb_abstain"]), pc, "high", 1),
          B("Decision flips between runs", "items whose decision changed across 3 runs", d(lambda o: o["flip"]), pc, "low", None),
          B("Input tokens per call", "as reported by the provider; sizes are not comparable across tokenizers", d(lambda o: o["tok"][0]), lambda v: f"{v:.0f}", None)]
def tr(label, f, hint=""):
    return f'<tr><td>{label}<div class="hint">{hint}</div></td>' + "".join(f'<td>{f(R[n])}</td>' for n in names) + '</tr>'
tbl = (tr("Failed calls", lambda o: f'{o["err"]} / {o["n"]}') + tr("Overall accuracy", lambda o: f'{o["acc_all"]:.1%}')
       + tr("Severity within ±¾ level", lambda o: pc(o["acc"]["score_pm1"][0])) + tr("Latency max", lambda o: ms(o["lat"]["mx"]))
       + tr("Run-to-run std dev", lambda o: f'{o["sd"]:.3f}') + tr("Output tokens per call", lambda o: f'{o["tok"][1]:.0f}')
       + tr("Items answered with conf ≥ 0.8", lambda o: f'{o["hi"][1]} of {o["hi"][1]+o["lo"][1]}'))
wrong_rows = "".join(f'<tr><td>{esc(text)}</td><td>{esc(gold)}</td>' + "".join(f'<td class="{"bad" if w else "good"}">{esc(desc)}</td>' for _, desc, w in cells) + '</tr>' for text, gold, cells in wrong_items(R, names))

# --- data for the interactive page (site.py) and the social image
PTS = [dict(id=m.key, name=m.label, sub=m.sub, color=m.color, acc=round(H[m.key]["acc"], 4), lo=round(CI[m.key][0], 4), hi=round(CI[m.key][1], 4), brier=round(H[m.key]["brier"], 4),
            p50=round(H[m.key]["p50"] * 1000), p95=round(H[m.key]["p95"] * 1000), cost=round(HCOST[m.key] / 1000, 5), fail=H[m.key]["fail"], n=H[m.key]["n"], own=round(R[m.key]["acc_all"], 4)) for m in MODELS]
json.dump({"date": now, "npub": NPUB, "per": PER, "points": PTS}, open("data.json", "w"), indent=1)

J, Ce, Cl, OD, OC = (R[n] for n in ("Jev", "Celeris-1", "Clef", "OpenAI Decisions", CHAT.key))
notes = f'''<p><b>Public benchmark.</b> On a {NPUB}-item sample of jev-bench, Celeris is clearly the most accurate ({H["Celeris-1"]["acc"]:.0%}, 95% CI {CI["Celeris-1"][0]:.0%}&ndash;{CI["Celeris-1"][1]:.0%}) and best calibrated (Brier {H["Celeris-1"]["brier"]:.3f}). Clef ({H["Clef"]["acc"]:.0%}) and Jev ({H["Jev"]["acc"]:.0%}) follow, then OpenAI Decisions ({H["OpenAI Decisions"]["acc"]:.0%}); the DIY chat adapter is far behind because it cannot handle questions with more than 26 options (five datasets). The 95% intervals are about &plusmn;{HW:.1f} points wide, so Celeris leads OpenAI Decisions clearly, while Clef and Jev are statistically tied.</p>
<p><b>Speed.</b> Jev ({J["lat"]["p50"]*1000:.0f} ms) and OpenAI Decisions ({OD["lat"]["p50"]*1000:.0f} ms) have the lowest median on my own set, Celeris is steady at {Ce["lat"]["p50"]*1000:.0f} ms, Clef is slower with a long tail (p95 {Cl["lat"]["p95"]*1000:.0f} ms, slowest call {Cl["lat"]["mx"]:.1f} s). Celeris advertises 67 ms from AWS us-east-1; from here, through the CLI with a new TLS handshake per call, every number is dominated by the network path. The first call of each run is left out as a cold start.</p>
<p><b>My own 60-item set</b> is easy: all five models are within a few points of each other ({J["acc_all"]:.0%} to {Cl["acc_all"]:.0%}), and one answer is worth almost two points. OpenAI Decisions has the best calibration here (Brier {OD["brier"]["noul"]:.4f} yes/no, {OD["brier"]["choice"]:.4f} routing), Jev is the only model that answers every vague input with &ldquo;can&rsquo;t tell&rdquo; ({J["amb_abstain"]:.0%}), and the DIY chat adapter is nearly as confident when wrong ({OC["conf_bad"]:.2f}) as when right ({OC["conf_ok"]:.2f}). On the harder public data the ranking changes, which is the reason to trust the larger set more.</p>
<p><b>Cost.</b> All four dedicated decision APIs cost between ${min(COST[n] for n in dedicated)/1000:.4f} and ${max(COST[n] for n in dedicated)/1000:.4f} per 1,000 single-question decisions on my short inputs, and ${min(HCOST[n] for n in dedicated)/1000:.4f} to ${max(HCOST[n] for n in dedicated)/1000:.4f} on the longer public-benchmark inputs, so a million decisions costs roughly $6 to $42. OpenAI Decisions is priced at $0.10 per million input tokens with no output charge; the DIY chat route costs about {COST[CHAT.key]/COST["Celeris-1"]:.0f}x Celeris on short inputs. Cost is a minor factor at small scale; accuracy and calibration matter more. Tokens are counted by each vendor&rsquo;s own tokenizer.</p>
<p><b>Caveats.</b> The public sample is {PER} items per dataset, one run, so differences under about {HW:.0f} points are inside the error margin. My own set is synthetic, small and easy, with 3 repeats. Latency comes from the <code>jev</code> CLI on one machine. Severity labels on my set are my own judgement. Scoring on jev-bench counts a failed or refused request as a uniform answer. jev-bench is published by a third party and has a mixed licence; I used only a small sample of its test splits. The OpenAI Decisions API is in public beta. No sponsorship or affiliation with any of these vendors.</p>'''

# --- social preview and SEO
top = max(p["acc"] for p in PTS)
rows = ""
for i, p in enumerate(sorted(PTS, key=lambda q: -q["acc"])):
    y = 250 + i * 60; w = 420 * p["acc"] / top
    rows += (f'<text x="64" y="{y+26}" font-size="28" font-weight="700" fill="#f4f4f5">{esc(p["name"])}</text><rect x="330" y="{y}" width="{w:.0f}" height="34" rx="6" fill="{p["color"]}"/>'
             f'<text x="{330+w+14:.0f}" y="{y+26}" font-size="26" fill="#d4d4d8">{p["acc"]:.0%} · ${p["cost"]:.3f} per 1,000</text>')
og_frame("og", "DECISION API BENCHMARK", "Five decision models, tested", f"{NPUB} public items + 180 own calls per model: accuracy, speed, cost", "#4f46e5", rows,
         footer=f"Jev · Celeris · Clef · OpenAI · measured with the jev CLI · {now}")
TITLE = "Decision API benchmark: Jev, Celeris, Clef and OpenAI (accuracy, cost, latency)"
DESC = (f"Measured comparison of TypeSafe Jev, Celeris-1, Cloudflare Clef and OpenAI's Decisions API as typed decision models: accuracy on a {NPUB}-item sample of the public jev-bench, "
        f"cost per decision, latency and calibration, plus 180 calls per model on a 60-item set, run through the jev CLI on {now}.")
SEO = meta(URL, TITLE, DESC, "Bar chart: accuracy and cost per 1,000 decisions of Jev, Celeris, Clef and two OpenAI variants", kind="article", modified=now,
           social_title="Decision APIs compared: Jev, Celeris, Clef, OpenAI", social_desc="Accuracy, cost and speed of five typed decision models, measured on public data with the jev CLI.",
           ld={"@type": "TechArticle", "headline": TITLE, "datePublished": now, "author": {**AUTHOR, "sameAs": ["https://www.linkedin.com/in/andreas-vilinski/", "https://github.com/vilinski"]},
               "about": ["decision models", "LLM evaluation", "latency", "calibration"], "mainEntityOfPage": URL, "citation": REPO,
               "isBasedOn": {"@type": "SoftwareSourceCode", "name": "jev", "codeRepository": REPO, "programmingLanguage": "Rust", "license": "https://spdx.org/licenses/MIT.html"}},
           extra=('<meta name="color-scheme" content="light dark">',
                  '<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns=%27http://www.w3.org/2000/svg%27 viewBox=%270 0 32 32%27%3E%3Crect width=%2732%27 height=%2732%27 rx=%278%27 fill=%27%234f46e5%27/%3E%3Ctext x=%2716%27 y=%2722%27 font-family=%27system-ui,sans-serif%27 font-size=%2713%27 font-weight=%27900%27 fill=%27white%27 text-anchor=%27middle%27%3Ejev%3C/text%3E%3C/svg%3E">'))

HTML = f'''<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{esc(TITLE)}</title>
{SEO}
<style>
:root{{--bg:#fafaf7;--fg:#1b1b1b;--mut:#666;--card:#fff;--line:#e3e1da;--bad:#fbe3e0;--good:#e3f4e6}}
@media(prefers-color-scheme:dark){{:root{{--bg:#16171a;--fg:#ececec;--mut:#9a9a9a;--card:#1f2024;--line:#33353a;--bad:#4a2522;--good:#1f3a25}}}}
body{{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif}} main{{max-width:1040px;margin:0 auto;padding:24px 16px 64px}}
h1{{font-size:28px;margin:0}} h2{{margin-top:36px;font-size:20px}} h3{{margin:0 0 2px;font-size:15px}}
.sub,.hint,small{{color:var(--mut);font-size:12.5px;font-weight:400}} .sub{{margin:0 0 8px}} .dir{{font-weight:600}} .lg{{display:block}}
.grid2{{display:grid;grid-template-columns:repeat(auto-fit,minmax(320px,1fr));gap:12px}} .wide{{grid-column:1/-1}}
.card{{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:14px 16px}}
svg text{{fill:var(--fg)}} .lbl{{font-size:12.5px;font-weight:600}} .val{{font-size:12px;fill:var(--mut)}} .tick{{font-size:11px;fill:var(--mut)}} .mk{{font-size:11px;font-weight:600}} svg .mk{{fill:currentColor}}
.grid{{stroke:var(--line);stroke-width:1}}
.key{{display:flex;flex-wrap:wrap;gap:16px;margin:10px 0;font-weight:600}} .key i{{display:inline-block;width:12px;height:12px;border-radius:3px;margin-right:6px}}
table{{width:100%;border-collapse:collapse;background:var(--card);border:1px solid var(--line)}} th,td{{padding:8px 10px;border-bottom:1px solid var(--line);text-align:left;vertical-align:top;font-size:14px}}
a{{color:inherit}} td.bad{{background:var(--bad)}} td.good{{background:var(--good)}} .tw{{overflow-x:auto}} .note{{background:var(--card);border-left:4px solid #999;padding:4px 16px;border-radius:4px}}
table.heat td{{text-align:center}}
</style></head><body><main>
<h1>Decision API benchmark: Jev, Celeris, Clef and OpenAI</h1>
<p class="sub">{now} · by <a rel="author" href="{PROFILE}">Andreas Vilinski</a> · two test sets, run via the <a href="{REPO}"><code>jev</code> CLI</a> with one question per call: {NPUB} items from the public jev-bench ({PER} per dataset) and my own 60 labelled items × 3 runs. OpenAI Decisions = <code>gpt-6-luna</code> on <code>/v1/decisions</code> (public beta); OpenAI chat (DIY) = <code>gpt-4.1-mini</code> read through token log-probabilities</p>
<p class="sub"><a href="/">&larr; All charts</a> · <a href="{DM_URL}">Interactive scatter plot: accuracy against cost and speed</a></p>
<div class="key">{"".join(f'<span><i style="background:{m.color}"></i>{esc(m.key)}</span>' for m in MODELS)}</div>
<h2>Speed</h2><p class="sub">Every call on my 60-item set, then median, tail and batching. First call of each run left out (cold start).</p><div class="grid2">{"".join(charts[:5])}</div>
<h2>Accuracy on public data: jev-bench sample</h2><p class="sub">Real, public data in the System One format: <a href="https://huggingface.co/datasets/Praveenrajus/jev-bench">Praveenrajus/jev-bench</a> (22 datasets such as MNLI, BoolQ, Banking77, MMLU, SST-5), {PER} test items per dataset. Same questions to every model, scored by accuracy of the top option and the Brier score.</p>{pub_tbl}<h3 style="margin-top:18px">Accuracy per dataset</h3>{pub_heat}
<h2>Public benchmark in charts</h2><div class="grid2">{"".join(pub_charts)}</div>
<h2>Cost</h2><div class="grid2">{"".join(charts_cost)}</div>{cost_tbl}
<p class="sub">Prices are list prices checked on 2026-10-12: <a href="https://celeris.ai/celeris-1-decision">Celeris</a> ($0.04), <a href="https://eesel.ai/blog/typesafe-jev-pricing">Jev</a> ($0.042, no official pricing page found; taken from third-party write-ups of the launch), <a href="https://developers.cloudflare.com/workers-ai/platform/pricing/">Cloudflare clef-flash</a> ($0.038, i.e. 3,455 neurons per M at $0.011 per 1,000; 10,000 neurons per day are free), <a href="https://developers.openai.com/api/docs/guides/decisions">OpenAI Decisions</a> ($0.10, no output charge), <a href="https://developers.openai.com/api/docs/models/gpt-4.1-mini">OpenAI gpt-4.1-mini</a> ($0.40 in, $1.60 out). Cost per decision = input tokens measured here × price. The four decision APIs read the state once for N questions in a request; the DIY chat route sends the state again for every question.</p>
<h2>Accuracy (my 60-item set)</h2><div class="grid2">{"".join(charts[5:9])}</div>
<h2>Calibration and trust</h2><div class="grid2">{"".join(charts[9:])}</div>
<h2>More numbers</h2><div class="tw"><table><tr><th>Metric</th>{th}</tr>{tbl}</table></div>
<h2>Items where a model was wrong (my set, majority of 3 runs)</h2><div class="tw"><table><tr><th>Text</th><th>Gold</th>{th}</tr>{wrong_rows}</table></div>
<h2>Reading the results</h2><div class="note">{notes}</div>
<footer class="sub" style="margin-top:32px">Measured with <a href="{REPO}">jev</a>, a Rust CLI for typed decision models (TypeSafe Jev, Celeris, Cloudflare Clef, Ollama, OpenAI). Source, benchmark scripts and raw results: <a href="{REPO}/tree/main/docs/benchmark">{REPO.replace("https://", "")}</a>. Generated {now}.</footer>
</main></body></html>'''
open("report.html", "w").write(HTML)
