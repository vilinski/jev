"""Build the ai.vilinski.dev tree into ./site from data.json (written by report.py) and the current coding-agents page.

    python3 report.py
    python3 site.py <coding-agents-page.html> [<updater-template.html>]

Pages: / (landing, three cards), /decision-models/ (interactive scatter), /jev-benchmark/ (report),
/coding-agents/ (the Artificial Analysis chart the hourly updater renders; its meta is added here).
Every page gets its own og.png. Upload only after review (see AGENTS.md).
"""
import json, math, os, re, shutil, sys

from site_lib import PROFILE, SITE, esc, latency_radius, meta, og_frame, scatter

OUT = "site"
VERIFY = "K_8TwgTxZWlUlupLEKquy_fQknMYJndY3SvZoJqeFNk"   # Google Search Console token already on the live site; keep it on /
CODING_MARK = 'property="og:title"'                      # present on a coding-agents page that already carries the generated meta
PAGES = ["/", "/coding-agents/", "/decision-models/", "/jev-benchmark/"]
D = json.load(open("data.json"))
PTS, NOW, NPUB, PER = D["points"], D["date"], D["npub"], D["per"]
CODING = open(sys.argv[1]).read()
TEMPLATE = sys.argv[2] if len(sys.argv) > 2 else None
for p in PAGES[1:]:
    os.makedirs(OUT + p, exist_ok=True)

DARK = {"mut": "#9a9aa3", "grid": "#2b2d33", "axis": "#52555d", "halo": "#16171a"}

# ---------------------------------------------------------------- decision-models: preview image + page
dm_points = [dict(x=p["cost"], y=p["acc"], r=latency_radius(p["p50"], 1.15), color=p["color"], name=p["name"], lo=p["lo"], hi=p["hi"]) for p in PTS]
body = f'<g transform="translate(40 214)">{scatter(dm_points, W=1120, H=390, theme=DARK, font=1.15, front=False, xticks=(0.01, 0.02, 0.03, 0.05, 0.1, 0.2))}</g>'
og_frame(f"{OUT}/decision-models/og", "DECISION MODELS", "Accuracy vs. cost per decision", f"Five hosted models on {NPUB} public items, bubble = median latency", "#c0289c", body,
         footer=f"ai.vilinski.dev/decision-models · measured {NOW}")
page_css = re.search(r"<style>(.*?)</style>", CODING, re.S).group(1)   # same look as the coding-agents page
section = open("section.html").read().replace("__PTS__", json.dumps(PTS)).replace("{NOW}", NOW).replace("{NPUB}", str(NPUB)).replace("{PER}", str(PER))
dm_title = "Decision models compared: accuracy vs. cost and speed"
dm_desc = (f"Interactive scatter plot of five hosted decision models (Jev, Celeris-1, Clef, OpenAI Decisions, OpenAI chat) on {NPUB} items of the public jev-bench: "
           f"accuracy against cost per 1,000 decisions or latency, with 95% intervals and a Pareto frontier. Measured {NOW}.")
dm_meta = meta(f"{SITE}/decision-models/", dm_title, dm_desc, "Scatter plot: accuracy against cost per 1,000 decisions for five decision models", kind="article", modified=NOW,
               social_title="Decision models: accuracy vs. cost", social_desc="Five hosted decision models on public data: who is accurate, cheap and fast? Interactive chart.")
open(f"{OUT}/decision-models/index.html", "w").write(f'''<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{esc(dm_title)}</title>
{dm_meta}
<style>{page_css}
  .app + .app {{ margin-top:24px; }}
  .app code {{ background:#f0f2f6; padding:1px 5px; border-radius:4px; font-size:12px; }}
  .back {{ display:inline-block; margin:0 0 12px 4px; color:#5a58c8; text-decoration:none; font-size:13px; }}
</style>
</head>
<body>
<a class="back" href="/">&larr; All charts</a>
{section}
</body>
</html>
''')

# ---------------------------------------------------------------- jev-benchmark: the report
shutil.copy("report.html", f"{OUT}/jev-benchmark/index.html")
shutil.copy("og.png", f"{OUT}/jev-benchmark/og.png")

# ---------------------------------------------------------------- coding-agents: preview image from the live data, meta added to the page and the template
rows = json.loads(re.search(r"const data = (\[.*?\]);", CODING, re.S).group(1))
rows = [r for r in rows if r.get("cost") and r.get("index") is not None]
provs = sorted({r["provider"] for r in rows})
palette = ["#5e5bd8", "#e8710a", "#12a36b", "#c0289c", "#2f6fed", "#d9a400", "#0e9fb0", "#8a8f98", "#d6455d", "#6b7a8f"]
color = {p: palette[i % len(palette)] for i, p in enumerate(provs)}
tmax = max(r.get("time") or 0 for r in rows) or 1
ca_points = [dict(x=r["cost"], y=r["index"], r=5 + 12 * math.sqrt((r.get("time") or 0) / tmax), color=color[r["provider"]]) for r in rows]
legend = "".join(f'<circle cx="{70 + i * 150}" cy="0" r="6" fill="{color[p]}"/><text x="{82 + i * 150}" y="5" font-size="16" fill="#a1a1aa">{esc(p[:16])}</text>' for i, p in enumerate(provs[:7]))
body = (f'<g transform="translate(40 218)">{scatter(ca_points, W=1120, H=372, theme=DARK, ypad=4, labels=False, ci=False, opacity=.8)}</g>'
        f'<g transform="translate(40 604)">{legend}</g>')
og_frame(f"{OUT}/coding-agents/og", "CODING AGENTS", "Performance vs. cost per task", f"{len(rows)} agent and effort variants, Artificial Analysis Coding Agent Index", "#5e5bd8", body)
ca_title = "Coding agent performance vs. cost, time and tokens per task"
ca_desc = "Interactive scatter plot of coding agents and their effort settings from the Artificial Analysis Coding Agent Index: performance against cost, time or tokens per task, with a Pareto frontier. Refreshed hourly."

def patch_coding(text):
    """Add title, meta and a back link to the generated coding-agents page (or the template it is rendered from). Idempotent."""
    if CODING_MARK in text:
        return text
    text = re.sub(r'\s*<p class="sub">Also measured here:.*?</p>', "", text, flags=re.S)
    tags = meta(f"{SITE}/coding-agents/", ca_title, ca_desc, "Scatter plot of coding agent performance against cost per task",
                social_title="Coding agents: performance vs. cost", social_desc="Which coding agent gives the most for the money? Live chart from the Artificial Analysis index, refreshed hourly.", verify=None)
    text = text.replace("<title>Coding Agent Effort Variants</title>", f"<title>{esc(ca_title)}</title>\n{tags}", 1)
    return text.replace('<main class="app">', '<a href="/" style="display:inline-block;margin:0 0 12px 4px;color:#5a58c8;text-decoration:none;font-size:13px">&larr; All charts</a>\n<main class="app">', 1)

open(f"{OUT}/coding-agents/index.html", "w").write(patch_coding(CODING))
if TEMPLATE:   # what the hourly updater renders from: the same patch, placeholders must survive
    tpl = patch_coding(open(TEMPLATE).read())
    assert "const data = [" in tpl and "Data refreshed: STATIC SNAPSHOT" in tpl, "the template lost a placeholder the updater needs"
    open(f"{OUT}/_template.html", "w").write(tpl)

# ---------------------------------------------------------------- landing page
cards = [("/coding-agents/", "Coding agents: performance vs. cost", "Which coding agent gives the most for the money? Index scores against cost, time and tokens per task, from Artificial Analysis. Refreshed hourly.", "Live chart"),
         ("/decision-models/", "Decision models: accuracy vs. cost", f"Five hosted decision models (Jev, Celeris, Clef, OpenAI) on {NPUB} public items: accuracy against cost or speed, with intervals.", f"Measured {NOW}"),
         ("/jev-benchmark/", "Decision API benchmark: the full report", "Accuracy per dataset, calibration, latency, cost and the 60-item hand-labelled test, with scripts and raw results.", f"Report, {NOW}")]
tiles = ""
for i, (col, name) in enumerate([("#5e5bd8", "Coding agents"), ("#c0289c", "Decision models"), ("#2f6fed", "Benchmark report")]):
    x = 64 + i * 370
    tiles += f'<rect x="{x}" y="290" width="340" height="250" rx="18" fill="#1f2024" stroke="#33353a"/><text x="{x + 24}" y="332" font-size="26" font-weight="700" fill="#f4f4f5">{name}</text>'
    tiles += "".join(f'<circle cx="{x + 50 + (j * 41) % 230 + i * 12}" cy="{480 - (j * 29 + i * 13) % 130}" r="{8 + (j * 5) % 14}" fill="{col}" fill-opacity=".8"/>' for j in range(7))
og_frame(f"{OUT}/og", "AI.VILINSKI.DEV", "Charts on AI tooling", "Coding agents and decision models, measured and compared", "#5e5bd8", tiles, footer="by Andreas Vilinski", footer_y=590)
card_html = "".join(f'<a class="tile" href="{h}"><img src="{h}og.png" width="1200" height="630" alt="" loading="lazy"><div class="tb"><h2>{esc(t)}</h2><p>{esc(d)}</p><span>{esc(u)}</span></div></a>' for h, t, d, u in cards)
l_title = "AI charts by Andreas Vilinski: coding agents and decision models"
l_desc = "Interactive charts and measurements on AI tooling: coding agent performance vs. cost, and a benchmark of hosted decision models (Jev, Celeris, Clef, OpenAI)."
l_meta = meta(f"{SITE}/", l_title, l_desc, "Three chart cards: coding agents, decision models and the benchmark report", social_title="Charts on AI tooling", social_desc="Coding agents and decision models, measured and compared.", verify=VERIFY)
open(f"{OUT}/index.html", "w").write(f'''<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{esc(l_title)}</title>
{l_meta}
<style>
:root{{color-scheme:light dark;--ink:#18212f;--muted:#657184;--line:#e4e9ef;--panel:#fff;--bg:#f5f7fa;--accent:#5e5bd8}}
@media(prefers-color-scheme:dark){{:root{{--ink:#ececec;--muted:#9aa3b2;--line:#33353a;--panel:#1f2024;--bg:#16171a;--accent:#8e8bf0}}}}
*{{box-sizing:border-box}} body{{margin:0;padding:24px;background:var(--bg);color:var(--ink);font:15px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}}
main{{max-width:1120px;margin:0 auto}} h1{{margin:24px 0 6px;font-size:30px;letter-spacing:-.025em}} .lead{{margin:0 0 28px;color:var(--muted);max-width:60ch}}
.tiles{{display:grid;grid-template-columns:repeat(auto-fit,minmax(300px,1fr));gap:18px}}
.tile{{display:block;background:var(--panel);border:1px solid var(--line);border-radius:16px;overflow:hidden;color:inherit;text-decoration:none;box-shadow:0 12px 40px #18212f12;transition:transform .12s,border-color .12s}}
.tile:hover,.tile:focus-visible{{transform:translateY(-3px);border-color:var(--accent);outline:none}}
.tile img{{display:block;width:100%;height:auto;background:#16171a}} .tb{{padding:16px 18px 18px}} .tb h2{{margin:0 0 6px;font-size:17px}} .tb p{{margin:0 0 10px;color:var(--muted);font-size:14px}} .tb span{{font-size:12px;color:var(--accent);font-weight:650}}
footer{{margin-top:32px;color:var(--muted);font-size:13px}} footer a{{color:var(--accent)}}
</style></head><body><main>
<h1>Charts on AI tooling</h1>
<p class="lead">Measurements and comparisons I keep for my own work, shared as they are: which coding agent is worth its cost, and which hosted decision model to use for typed yes/no, choice and score questions.</p>
<div class="tiles">{card_html}</div>
<footer>By <a href="{PROFILE}">Andreas Vilinski</a> · decision benchmark code and raw results on <a href="https://github.com/vilinski/jev">GitHub</a> · coding agent data from <a href="https://artificialanalysis.ai/agents/coding-agents">Artificial Analysis</a></footer>
</main></body></html>
''')

# ---------------------------------------------------------------- crawler files
open(f"{OUT}/sitemap.xml", "w").write('<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n' + "".join(f"  <url>\n    <loc>{SITE}{p}</loc>\n    <lastmod>{NOW}</lastmod>\n  </url>\n" for p in PAGES) + "</urlset>\n")
open(f"{OUT}/robots.txt", "w").write(f"User-agent: *\nAllow: /\n\nSitemap: {SITE}/sitemap.xml\n")
print("built", sorted(os.path.relpath(os.path.join(d, f), OUT) for d, _, fs in os.walk(OUT) for f in fs if not f.endswith(".svg")))
