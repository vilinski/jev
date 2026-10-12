"""SVG charts for the report: horizontal bar charts per metric and the latency strip plot."""
import math
from site_lib import esc

def bars(models, title, sub, vals, fmt, better, maxv=None, pairs=None):
    """One horizontal bar chart. vals: {key: value}; pairs: {key: (a, b)} draws two bars per model."""
    W = 480; lab = 130; right = 64; rowh = 26 if not pairs else 40
    mv = maxv or max((max(v) if pairs else v) for v in (pairs or vals).values()) * 1.08 or 1
    rows = ""
    for i, m in enumerate(models):
        y = 8 + i * (rowh + 10)
        rows += f'<text x="0" y="{y+rowh/2+4}" class="lbl">{esc(m.label)}</text>'
        vs = pairs[m.key] if pairs else (vals[m.key],)
        for j, v in enumerate(vs):
            h = (rowh - 4) / len(vs); w = max((W - lab - right) * v / mv, 1)
            op = 1 if j == 0 else .38
            rows += f'<rect x="{lab}" y="{y+j*(h+2):.1f}" width="{w:.1f}" height="{h:.1f}" rx="3" fill="{m.color}" opacity="{op}"/><text x="{lab+w+5:.1f}" y="{y+j*(h+2)+h/2+4:.1f}" class="val">{fmt(v)}</text>'
    H = 8 + len(models) * (rowh + 10)
    hint = {"low": "lower is better", "high": "higher is better", None: ""}[better]
    legend = '<span class="lg">solid = right, faded = wrong</span>' if pairs else ""
    return f'<section class="card"><h3>{esc(title)}</h3><p class="sub">{sub} <span class="dir">{hint}</span> {legend}</p><svg viewBox="0 0 {W} {H}" width="100%" role="img" aria-label="{esc(title)}">{rows}</svg></section>'

def strip(models, own, ncalls=180):
    """Every call of every model as a dot on a log time axis, with p50 and p95 marked."""
    W = 900; lab = 130; top = 24; rowh = 70
    lo, hi = 0.3, 10.0; X = lambda v: lab + (W - lab - 20) * (math.log(v) - math.log(lo)) / (math.log(hi) - math.log(lo))
    g = ""
    for t in (0.3, 0.5, 1, 2, 5, 10):
        g += f'<line x1="{X(t):.1f}" x2="{X(t):.1f}" y1="{top-6}" y2="{top+rowh*len(models)}" class="grid"/><text x="{X(t):.1f}" y="{top+rowh*len(models)+16}" class="tick" text-anchor="middle">{t:g} s</text>'
    for i, m in enumerate(models):
        cy = top + i * rowh + rowh / 2; o = own[m.key]["lat"]
        g += f'<text x="0" y="{cy+4}" class="lbl">{esc(m.label)}</text>'
        for k, v in enumerate(o["all"]):
            jy = ((k * 37) % 21 - 10) * 1.4   # deterministic vertical jitter
            g += f'<circle cx="{X(v):.1f}" cy="{cy+jy:.1f}" r="3" fill="{m.color}" opacity=".45"/>'
        for key in ("p50", "p95"):
            x = X(o[key]); g += f'<line x1="{x:.1f}" x2="{x:.1f}" y1="{cy-24}" y2="{cy+24}" stroke="{m.color}" stroke-width="2.5"/><text x="{x+4:.1f}" y="{cy-22}" class="mk" fill="{m.color}">{key} {o[key]*1000:.0f} ms</text>'
        if o["mx"] > 1.2:
            x = X(o["mx"]); g += f'<text x="{x:.1f}" y="{cy+34}" class="mk" text-anchor="middle" fill="{m.color}">max {o["mx"]:.1f} s</text>'
    H = top + rowh * len(models) + 26
    return f'<section class="card wide"><h3>Latency of every call</h3><p class="sub">Each dot is one of the {ncalls} calls per model (log axis, so the fast cluster is spread out and slow outliers stay visible). Bars mark p50 and p95. <span class="dir">further left is better</span></p><svg viewBox="0 0 {W} {H}" width="100%" role="img" aria-label="Latency strip plot">{g}</svg></section>'
