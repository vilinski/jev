exec(open("an.py").read())
import math, html, datetime
names=list(R); short={"Jev":"Jev","Clef":"Clef","OpenAI gpt-4.1-mini":"OpenAI"}
def esc(s): return html.escape(str(s))
def bars(title,sub,vals,fmt,better,maxv=None,pairs=None):
    """vals: {name: value}; pairs: {name:(a,b)} for two-bar groups. Horizontal bars, one SVG."""
    W=420; lab=64; right=64; rowh=26 if not pairs else 40
    mv=maxv or max((max(v) if pairs else v) for v in (pairs or vals).values())*1.08 or 1
    rows=""
    for i,n in enumerate(names):
        y=8+i*(rowh+10)
        rows+=f'<text x="0" y="{y+rowh/2+4}" class="lbl">{esc(short[n])}</text>'
        vs=pairs[n] if pairs else (vals[n],)
        for j,v in enumerate(vs):
            h=(rowh-4)/len(vs); w=max((W-lab-right)*v/mv,1)
            op=1 if j==0 else .38
            rows+=f'<rect x="{lab}" y="{y+j*(h+2):.1f}" width="{w:.1f}" height="{h:.1f}" rx="3" fill="{COL[n]}" opacity="{op}"/><text x="{lab+w+5:.1f}" y="{y+j*(h+2)+h/2+4:.1f}" class="val">{fmt(v)}</text>'
    H=8+len(names)*(rowh+10)
    hint={"low":"lower is better","high":"higher is better",None:""}[better]
    legend='<span class="lg">solid = right, faded = wrong</span>' if pairs else ""
    return f'<section class="card"><h3>{esc(title)}</h3><p class="sub">{sub} <span class="dir">{hint}</span> {legend}</p><svg viewBox="0 0 {W} {H}" width="100%" role="img" aria-label="{esc(title)}">{rows}</svg></section>'
def strip():
    W=900; lab=70; top=24; rowh=70
    lo,hi=0.3,5.0; X=lambda v: lab+(W-lab-20)*(math.log(v)-math.log(lo))/(math.log(hi)-math.log(lo))
    g=""
    for t in (0.3,0.5,1,2,3,5):
        g+=f'<line x1="{X(t):.1f}" x2="{X(t):.1f}" y1="{top-6}" y2="{top+rowh*len(names)}" class="grid"/><text x="{X(t):.1f}" y="{top+rowh*len(names)+16}" class="tick" text-anchor="middle">{t:g} s</text>'
    for i,n in enumerate(names):
        cy=top+i*rowh+rowh/2; o=R[n]["lat"]
        g+=f'<text x="0" y="{cy+4}" class="lbl">{esc(short[n])}</text>'
        for k,v in enumerate(o["all"]):
            jy=((k*37)%21-10)*1.4   # deterministic vertical jitter
            g+=f'<circle cx="{X(v):.1f}" cy="{cy+jy:.1f}" r="3" fill="{COL[n]}" opacity=".45"/>'
        for key,lbl,dy in (("p50","p50",-26),("p95","p95",-26)):
            x=X(o[key]); g+=f'<line x1="{x:.1f}" x2="{x:.1f}" y1="{cy-24}" y2="{cy+24}" stroke="{COL[n]}" stroke-width="2.5"/><text x="{x+4:.1f}" y="{cy+dy+4}" class="mk" fill="{COL[n]}">{lbl} {o[key]*1000:.0f} ms</text>'
        if o["mx"]>1.2:
            x=X(o["mx"]); g+=f'<text x="{x:.1f}" y="{cy+34}" class="mk" text-anchor="middle" fill="{COL[n]}">max {o["mx"]:.1f} s</text>'
    H=top+rowh*len(names)+26
    return f'<section class="card wide"><h3>Latency of every call</h3><p class="sub">Each dot is one of the 180 calls per model (log axis, so the fast cluster is spread out and slow outliers stay visible). Bars mark p50 and p95. <span class="dir">further left is better</span></p><svg viewBox="0 0 {W} {H}" width="100%" role="img" aria-label="Latency strip plot">{g}</svg></section>'
pc=lambda v:f"{v:.0%}"
d=lambda f:{n:f(R[n]) for n in names}
ms=lambda v:f"{v*1000:.0f} ms"
charts=[]
charts.append(strip())
charts.append(bars("Latency p50","median single call, incl. process start + TLS",d(lambda o:o["lat"]["p50"]),ms,"low"))
charts.append(bars("Latency p95","slowest 5% of calls",d(lambda o:o["lat"]["p95"]),ms,"low"))
charts.append(bars("10 questions, one request","all questions about one state; OpenAI runs them as 10 parallel calls",d(lambda o:o["batch"]),ms,"low"))
charts.append(bars("10 questions, 10 calls in sequence","what you pay if you do not batch",d(lambda o:o["sep"]),lambda v:f"{v:.1f} s","low"))
charts.append(bars("Accuracy: yes/no","24 items × 3 runs, vague items excluded",d(lambda o:o["acc"]["noul"][0]),pc,"high",1))
charts.append(bars("Accuracy: routing","20 items × 3 runs, 6 options incl. “unclear”",d(lambda o:o["acc"]["choice"][0]),pc,"high",1))
charts.append(bars("Accuracy: severity (exact level)","16 items × 3 runs, 4 levels",d(lambda o:o["acc"]["score_exact"][0]),pc,"high",1))
charts.append(bars("Severity error","mean distance to the labelled level, in levels",d(lambda o:o["mae"]),lambda v:f"{v:.2f}","low"))
charts.append(bars("Calibration: yes/no (Brier)","squared error of the probability",d(lambda o:o["brier"]["noul"]),lambda v:f"{v:.3f}","low"))
charts.append(bars("Calibration: routing (Brier)","squared error over all options",d(lambda o:o["brier"]["choice"]),lambda v:f"{v:.3f}","low"))
charts.append(bars("Confidence when right vs wrong","a useful confidence is clearly lower when wrong","",lambda v:f"{v:.2f}","high",1,pairs={n:(R[n]["conf_ok"],R[n]["conf_bad"]) for n in names}))
charts.append(bars("Accuracy when confidence ≥ 0.8","how far you can trust a confident answer",d(lambda o:o["hi"][0]),pc,"high",1))
charts.append(bars("Accuracy when confidence < 0.8","a good model is worse here, so these can go to a human",d(lambda o:o["lo"][0]),pc,"low",1))
charts.append(bars("Says “can’t tell” on vague input","share of 4 vague yes/no items answered 0.25–0.75",d(lambda o:o["amb_abstain"]),pc,"high",1))
charts.append(bars("Decision flips between runs","items whose decision changed across 3 runs",d(lambda o:o["flip"]),pc,"low",None))
charts.append(bars("Input tokens per call","as reported by the provider; sizes are not comparable across tokenizers",d(lambda o:o["tok"][0]),lambda v:f"{v:.0f}",None))
# tables
def tr(l,f,hint=""): return f'<tr><td>{l}<div class="hint">{hint}</div></td>'+"".join(f'<td>{f(R[n])}</td>' for n in names)+'</tr>'
th="".join(f'<th style="color:{COL[n]}">{esc(short[n])}</th>' for n in names)
tbl=(tr("Failed calls",lambda o:f'{o["err"]} / {o["n"]}')+tr("Overall accuracy",lambda o:f'{o["acc_all"]:.1%}')
 +tr("Severity within ±¾ level",lambda o:pc(o["acc"]["score_pm1"][0]))+tr("Latency max",lambda o:ms(o["lat"]["mx"]))
 +tr("Run-to-run std dev",lambda o:f'{o["sd"]:.3f}')+tr("Output tokens per call",lambda o:f'{o["tok"][1]:.0f}')
 +tr("Items answered with conf ≥ 0.8",lambda o:f'{o["hi"][1]} of {o["hi"][1]+o["lo"][1]}'))
def label(i): return {"n":N,"c":C,"s":S}[i[0]][int(i[1:])]
from collections import Counter
def pred(a,kind):
    return ("yes" if a["noul"]>.5 else "no")+f" ({a['noul']:.2f})" if kind=="noul" else (f"{a['choice']} ({a['confidence']:.2f})" if kind=="choice" else f"L{max(a['probabilities'],key=a['probabilities'].get)} · {a['score']:.2f}")
def maj(v,kind):
    dec=[(a["noul"]>.5) if kind=="noul" else a["choice"] if kind=="choice" else int(max(a["probabilities"],key=a["probabilities"].get)) for x,a in v]
    return Counter(dec).most_common(1)[0][0]
trs=[]
for i in R[names[0]]["byid"]:
    kind={"n":"noul","c":"choice","s":"score"}[i[0]]; it=label(i); gold=it[-1]
    if gold is None: continue
    ms_=[maj(R[n]["byid"][i],kind) for n in names]; wrong=[m!=gold for m in ms_]
    if any(wrong):
        cells="".join(f'<td class="{"bad" if w else "good"}">{esc(pred(R[n]["byid"][i][0][1],kind))}</td>' for n,w in zip(names,wrong))
        trs.append(f'<tr><td>{esc(it[0])}</td><td>{esc(gold)}</td>{cells}</tr>')
J,Cl,O=(R[n] for n in names)
now=datetime.date.today().isoformat()
notes=f'''<p><b>Speed.</b> Jev is the fastest and steadiest (p50 {J["lat"]["p50"]*1000:.0f} ms, p95 {J["lat"]["p95"]*1000:.0f} ms). Clef has similar typical speed but a long tail (p95 {Cl["lat"]["p95"]*1000:.0f} ms, one call {Cl["lat"]["mx"]:.1f} s). OpenAI is the slowest typically ({O["lat"]["p50"]*1000:.0f} ms) but has no tail beyond {O["lat"]["mx"]:.1f} s.</p>
<p><b>Accuracy.</b> All three are close (overall {J["acc_all"]:.0%} Jev, {Cl["acc_all"]:.0%} Clef, {O["acc_all"]:.0%} OpenAI) and the differences come from a handful of items, see the list below. Jev is flawless on yes/no and routing but weakest on severity; Clef is best on severity; OpenAI got one routing answer wrong in one of three runs, the only decision flip of the whole benchmark.</p>
<p><b>Calibration is where they differ.</b> OpenAI’s log-probabilities are sharp: it is the best on yes/no (Brier {O["brier"]["noul"]:.4f}) but its confidence is {O["conf_bad"]:.2f} when wrong versus {O["conf_ok"]:.2f} when right, so confidence does not flag its mistakes. Jev is lower when wrong ({J["conf_bad"]:.2f} vs {J["conf_ok"]:.2f}) and flags every vague input as uncertain, which OpenAI and Clef do on only {O["amb_abstain"]:.0%} of them. Clef has the weakest Brier scores.</p>
<p><b>Cost.</b> Not measured in money. Jev and Clef answer all questions of a request in one call. OpenAI makes one request per question, so a batch of N questions costs N requests (fast in parallel, not cheaper).</p>
<p><b>Caveats.</b> Synthetic, small (60 items) and easy: yes/no and routing sit near the ceiling, and a single item changes a percentage by 2 points. Only 3 repeats. Latency is from the <code>jev</code> CLI with a fresh TLS connection per call, with the three providers run back to back in one session, each as one block of 180 calls rather than interleaved, so time-of-day effects can still differ between them. The OpenAI result depends on the prompt that <code>jev</code> writes for it and on the model (<code>gpt-4.1-mini</code>); other models would score differently. Severity labels are the author's own judgement.</p>'''

# Social preview (og:image): drawn from the same numbers, so it is regenerated with every report.
import os, subprocess, shutil
def og_image():
    W,H=1200,630; rows=""
    for i,n in enumerate(names):
        y=300+i*96; col=COL[n]; p50=R[n]["lat"]["p50"]*1000; acc=R[n]["acc_all"]
        rows+=(f'<text x="64" y="{y+34}" font-size="34" font-weight="700" fill="#f4f4f5">{esc(short[n])}</text>'
               f'<rect x="250" y="{y}" width="{520*p50/max(R[m]["lat"]["p50"]*1000 for m in names)*0.86:.0f}" height="30" rx="6" fill="{col}"/>'
               f'<text x="{250+520*p50/max(R[m]["lat"]["p50"]*1000 for m in names)*0.86+14:.0f}" y="{y+25}" font-size="26" fill="#d4d4d8">{p50:.0f} ms median</text>'
               f'<rect x="250" y="{y+38}" width="{520*acc*0.86:.0f}" height="14" rx="4" fill="{col}" opacity=".45"/>'
               f'<text x="{250+520*acc*0.86+14:.0f}" y="{y+52}" font-size="22" fill="#a1a1aa">{acc:.0%} accurate</text>')
    svg=(f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" font-family="Helvetica, Arial, sans-serif">'
         f'<rect width="{W}" height="{H}" fill="#16171a"/><rect width="{W}" height="8" fill="#4f46e5"/>'
         f'<text x="64" y="104" font-size="30" fill="#818cf8" font-weight="700" letter-spacing="2">DECISION API BENCHMARK</text>'
         f'<text x="64" y="176" font-size="64" font-weight="800" fill="#ffffff">Jev vs Clef vs OpenAI</text>'
         f'<text x="64" y="232" font-size="30" fill="#a1a1aa">{R[names[0]]['n']*len(names)} measured calls: speed, accuracy, calibration</text>'
         f'{rows}'
         f'<text x="64" y="598" font-size="26" fill="#71717a">measured with the jev CLI · {now}</text></svg>')
    open("og.svg","w").write(svg)
    # macOS: sips rasterizes SVG at its declared size. Elsewhere an existing og.png is reused, if any.
    if sys.platform=="darwin" and shutil.which("sips"):
        subprocess.run(["sips","-s","format","png","og.svg","--out","og.png"],capture_output=True,timeout=60)
    return os.path.exists("og.png")
    return os.path.exists("og.png")

# SEO: regenerated with every report. Set REPORT_URL (e.g. https://example.com/jev-benchmark/) to emit the
# canonical/og:url tags; add REPORT_SITE_FILES=1 to also write robots.txt/sitemap.xml (only if the report is the site root;
# on a sub-path add the URL to the host's own sitemap instead).
import os
TITLE="Jev vs Clef vs OpenAI: decision API benchmark (latency, accuracy, calibration)"
NCALLS=R[names[0]]["n"]
DESC=(f"Measured comparison of TypeSafe Jev, Cloudflare Clef and OpenAI gpt-4.1-mini as typed decision models: "
      f"latency, accuracy and calibration over {NCALLS} calls per model, run through the jev CLI on {now}.")
# Cards on LinkedIn/X/Bluesky cut long text: keep og/twitter text short, the page <title>/description long for search.
SOCIAL_TITLE="Jev vs Clef vs OpenAI: decision API benchmark"
SOCIAL_DESC=f"{R[names[0]]['n']*len(names)} calls, three decision models: speed, accuracy and calibration, measured with the jev CLI."
URL=os.environ.get("REPORT_URL","").strip()
REPO="https://github.com/vilinski/jev"
PROFILE="https://andreas.vilinski.dev/"
seo=[f'<meta name="description" content="{esc(DESC)}">','<meta name="robots" content="index,follow">',
     '<meta property="og:type" content="article">',f'<meta property="og:title" content="{esc(SOCIAL_TITLE)}">',
     f'<meta property="og:description" content="{esc(SOCIAL_DESC)}">','<meta name="twitter:card" content="summary">',
     f'<meta name="twitter:title" content="{esc(SOCIAL_TITLE)}">',f'<meta name="twitter:description" content="{esc(SOCIAL_DESC)}">',
     '<meta name="color-scheme" content="light dark">','<meta name="author" content="Andreas Vilinski">',
     '<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns=%27http://www.w3.org/2000/svg%27 viewBox=%270 0 32 32%27%3E%3Crect width=%2732%27 height=%2732%27 rx=%278%27 fill=%27%234f46e5%27/%3E%3Ctext x=%2716%27 y=%2722%27 font-family=%27system-ui,sans-serif%27 font-size=%2713%27 font-weight=%27900%27 fill=%27white%27 text-anchor=%27middle%27%3Ejev%3C/text%3E%3C/svg%3E">']
ld={"@context":"https://schema.org","@type":"TechArticle","headline":TITLE,"description":DESC,
    "datePublished":now,"dateModified":now,"inLanguage":"en","author":{"@type":"Person","name":"Andreas Vilinski","url":PROFILE,"sameAs":["https://www.linkedin.com/in/andreas-vilinski/","https://github.com/vilinski"]},
    "about":["decision models","LLM evaluation","latency","calibration"],
    "isBasedOn":{"@type":"SoftwareSourceCode","name":"jev","codeRepository":REPO,"programmingLanguage":"Rust","license":"https://spdx.org/licenses/MIT.html"},
    "citation":REPO}
SITE_FILES=os.environ.get("REPORT_SITE_FILES")=="1"   # only when the report is the site root
HAS_OG=og_image()
if URL and HAS_OG:
    IMG=URL.rstrip("/")+"/og.png"; ALT="Bar chart: median latency and accuracy of Jev, Clef and OpenAI"
    seo=[t for t in seo if 'twitter:card' not in t]
    seo+=['<meta name="twitter:card" content="summary_large_image">',f'<meta property="og:image" content="{esc(IMG)}">',
          '<meta property="og:image:type" content="image/png">','<meta property="og:image:width" content="1200">',
          '<meta property="og:image:height" content="630">',f'<meta property="og:image:alt" content="{esc(ALT)}">',
          f'<meta name="twitter:image" content="{esc(IMG)}">',f'<meta name="twitter:image:alt" content="{esc(ALT)}">']
    ld["image"]=IMG
if SITE_FILES:
    open("robots.txt","w").write("User-agent: *\nAllow: /\n"+(f"Sitemap: {URL.rstrip('/')}/sitemap.xml\n" if URL else ""))
if URL:
    seo+= [f'<link rel="canonical" href="{esc(URL)}">',f'<meta property="og:url" content="{esc(URL)}">']
    ld["mainEntityOfPage"]=URL
    if SITE_FILES: open("sitemap.xml","w").write(f'<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><url><loc>{esc(URL)}</loc><lastmod>{now}</lastmod></url></urlset>\n')
seo.append('<script type="application/ld+json">'+json.dumps(ld)+'</script>')
SEO="\n".join(seo)
HTML=f'''<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{esc(TITLE)}</title>
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
.key{{display:flex;gap:16px;margin:10px 0;font-weight:600}} .key i{{display:inline-block;width:12px;height:12px;border-radius:3px;margin-right:6px}}
table{{width:100%;border-collapse:collapse;background:var(--card);border:1px solid var(--line)}} th,td{{padding:8px 10px;border-bottom:1px solid var(--line);text-align:left;vertical-align:top;font-size:14px}}
a{{color:inherit}} td.bad{{background:var(--bad)}} td.good{{background:var(--good)}} .tw{{overflow-x:auto}} .note{{background:var(--card);border-left:4px solid #999;padding:4px 16px;border-radius:4px}}
</style></head><body><main>
<h1>Decision API benchmark: Jev vs Clef vs OpenAI</h1>
<p class="sub">{now} · by <a rel="author" href="{PROFILE}">Andreas Vilinski</a> · 180 calls per model (60 synthetic labelled items × 3 runs) · via the <a href="{REPO}"><code>jev</code> CLI</a>, one question per call · OpenAI = gpt-4.1-mini read through token log-probabilities</p>
<div class="key">{"".join(f'<span><i style="background:{COL[n]}"></i>{esc(n)}</span>' for n in names)}</div>
<h2>Speed</h2><div class="grid2">{"".join(charts[:5])}</div>
<h2>Accuracy</h2><div class="grid2">{"".join(charts[5:9])}</div>
<h2>Calibration and trust</h2><div class="grid2">{"".join(charts[9:])}</div>
<h2>More numbers</h2><div class="tw"><table><tr><th>Metric</th>{th}</tr>{tbl}</table></div>
<h2>Items where a model was wrong (majority of 3 runs)</h2><div class="tw"><table><tr><th>Text</th><th>Gold</th>{th}</tr>{"".join(trs)}</table></div>
<h2>Reading the results</h2><div class="note">{notes}</div>
<footer class="sub" style="margin-top:32px">Measured with <a href="{REPO}">jev</a> — a Rust CLI for typed decision models (TypeSafe Jev, Cloudflare Clef, Ollama, OpenAI). Source, benchmark scripts and raw results: <a href="{REPO}/tree/main/docs/benchmark">{REPO.replace("https://","")}</a>. Generated {now}.</footer>
</main></body></html>'''
open("report.html","w").write(HTML)
