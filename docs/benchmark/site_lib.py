"""Shared helpers for the benchmark report and the ai.vilinski.dev pages: SEO tags, social preview images, scatter plot."""
import html, json, math, os, shutil, subprocess, sys

SITE = "https://ai.vilinski.dev"
PROFILE = "https://andreas.vilinski.dev/"
REPO = "https://github.com/vilinski/jev"
AUTHOR = {"@type": "Person", "name": "Andreas Vilinski", "url": PROFILE}

def esc(s):
    return html.escape(str(s))

# ---------------------------------------------------------------- SEO

def meta(url, title, desc, image_alt, *, kind="website", ld=None, social_title=None, social_desc=None, modified=None, verify=None, extra=()):
    """Head tags for one page: description, canonical, Open Graph, Twitter card (large image <url>og.png), JSON-LD.

    `ld` is merged over a schema.org WebPage; `modified` adds dateModified; `verify` is a search-console token;
    `extra` holds more raw tags. Social title and description stay short because LinkedIn, X and Bluesky cut long text.
    """
    img = f"{url}og.png"
    st, sd = social_title or title, social_desc or desc
    doc = {"@context": "https://schema.org", "@type": "WebPage", "name": title, "description": desc, "url": url, "image": img, "author": AUTHOR, "inLanguage": "en"}
    if modified:
        doc["dateModified"] = modified
    doc.update(ld or {})
    tags = ([f'<meta name="google-site-verification" content="{verify}">'] if verify else []) + [
        f'<meta name="description" content="{esc(desc)}">', '<meta name="robots" content="index,follow">', '<meta name="author" content="Andreas Vilinski">',
        f'<link rel="canonical" href="{url}">', f'<meta property="og:type" content="{kind}">', f'<meta property="og:url" content="{url}">',
        '<meta property="og:site_name" content="ai.vilinski.dev">', f'<meta property="og:title" content="{esc(st)}">', f'<meta property="og:description" content="{esc(sd)}">',
        f'<meta property="og:image" content="{img}">', '<meta property="og:image:type" content="image/png">', '<meta property="og:image:width" content="1200">',
        '<meta property="og:image:height" content="630">', f'<meta property="og:image:alt" content="{esc(image_alt)}">',
        '<meta name="twitter:card" content="summary_large_image">', f'<meta name="twitter:title" content="{esc(st)}">', f'<meta name="twitter:description" content="{esc(sd)}">',
        f'<meta name="twitter:image" content="{img}">', f'<meta name="twitter:image:alt" content="{esc(image_alt)}">',
        *extra, '<script type="application/ld+json">' + json.dumps(doc) + '</script>']
    return "\n".join(tags)

# ---------------------------------------------------------------- social preview images

def raster(svg, base):
    """Write <base>.svg and rasterize it to <base>.png (1200x630) with macOS sips; elsewhere an existing png is reused."""
    open(base + ".svg", "w").write(svg)
    if sys.platform == "darwin" and shutil.which("sips"):
        subprocess.run(["sips", "-s", "format", "png", base + ".svg", "--out", base + ".png"], capture_output=True, timeout=60)
    return os.path.exists(base + ".png")

def og_frame(base, kicker, title, sub, accent, body="", footer=None, footer_y=612):
    """A 1200x630 dark card: accent bar, kicker, title, subtitle, free SVG body and a footer line; written to <base>.png."""
    svg = (f'<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="630" viewBox="0 0 1200 630" font-family="Helvetica, Arial, sans-serif">'
           f'<rect width="1200" height="630" fill="#16171a"/><rect width="1200" height="8" fill="{accent}"/>'
           f'<text x="64" y="84" font-size="26" font-weight="700" letter-spacing="2" fill="{accent}">{esc(kicker)}</text>'
           f'<text x="64" y="150" font-size="56" font-weight="800" fill="#fff">{esc(title)}</text><text x="64" y="196" font-size="26" fill="#a1a1aa">{esc(sub)}</text>'
           f'{body}' + (f'<text x="64" y="{footer_y}" font-size="22" fill="#71717a">{esc(footer)}</text>' if footer else "") + '</svg>')
    return raster(svg, base)

# ---------------------------------------------------------------- scatter plot

def pareto(points, xk="x", yk="y"):
    """Points that no other point beats on both axes (lower x and higher y are better), in x order."""
    best, front = -math.inf, []
    for p in sorted(points, key=lambda q: q[xk]):
        if p[yk] > best:
            best = p[yk]
            front.append(p)
    return front

def nice_ticks(lo, hi, n):
    step0 = (hi - lo) / n
    mag = 10 ** math.floor(math.log10(step0))
    r = step0 / mag
    step = (1 if r < 1.5 else 2 if r < 3.5 else 5 if r < 7.5 else 10) * mag
    out, v = [], math.ceil(lo / step) * step
    while v <= hi + 1e-9:
        out.append(round(v, 10))
        v += step
    return out

def scatter(points, *, W=1080, H=520, theme=None, font=1.0, ypad=.05, xticks=(), fmt_x=str, fmt_y=str, xlabel=None, ylabel=None,
            labels=True, ci=True, front=True, opacity=.85):
    """Bubble scatter with a log x axis (inner SVG markup).

    points: dicts with x, y, r (bubble radius in px), color and optionally name, lo, hi (interval on y).
    Axis ticks and labels are drawn only when xlabel/ylabel are given. theme: fg, mut, grid, axis, halo colours.
    """
    th = {"mut": "#8a8f98", "grid": "#e3e1da", "axis": "#9aa0a8", "halo": "#fff"}
    th.update(theme or {})
    axes = bool(xlabel or ylabel)
    m = {"l": 74 * font, "r": 36 * font, "t": 26 * font, "b": 62 * font if axes else 24}
    xs = [p["x"] for p in points]
    xlo, xhi = math.log10(min(xs)) - .12, math.log10(max(xs)) + .12
    ys = [p["y"] for p in points] + [p[k] for p in points for k in ("lo", "hi") if k in p]
    ylo, yhi = min(ys) - ypad, max(ys) + ypad
    X = lambda v: m["l"] + (math.log10(v) - xlo) / (xhi - xlo) * (W - m["l"] - m["r"])
    Y = lambda v: H - m["b"] - (v - ylo) / (yhi - ylo) * (H - m["t"] - m["b"])
    fs = lambda n: f'font-size="{n * font:.1f}"'
    g = '<defs><linearGradient id="zg" x1="1" y1="1" x2="0" y2="0"><stop offset="0" stop-color="#2caa72" stop-opacity="0"/><stop offset="1" stop-color="#2caa72" stop-opacity=".16"/></linearGradient></defs>'
    g += f'<rect x="{m["l"]}" y="{m["t"]}" width="{W - m["l"] - m["r"]}" height="{H - m["t"] - m["b"]}" fill="url(#zg)"/>'
    for v in xticks:
        if xlo < math.log10(v) < xhi:
            g += f'<line x1="{X(v):.1f}" x2="{X(v):.1f}" y1="{m["t"]}" y2="{H - m["b"]}" stroke="{th["grid"]}"/>'
            if axes:
                g += f'<text x="{X(v):.1f}" y="{H - m["b"] + 18 * font:.1f}" text-anchor="middle" {fs(11)} fill="{th["mut"]}">{fmt_x(v)}</text>'
    for v in nice_ticks(ylo, yhi, 6):
        g += f'<line x1="{m["l"]}" x2="{W - m["r"]}" y1="{Y(v):.1f}" y2="{Y(v):.1f}" stroke="{th["grid"]}"/>'
        if axes:
            g += f'<text x="{m["l"] - 8}" y="{Y(v) + 4:.1f}" text-anchor="end" {fs(11)} fill="{th["mut"]}">{fmt_y(v)}</text>'
    g += f'<line x1="{m["l"]}" x2="{W - m["r"]}" y1="{H - m["b"]}" y2="{H - m["b"]}" stroke="{th["axis"]}"/><line x1="{m["l"]}" x2="{m["l"]}" y1="{m["t"]}" y2="{H - m["b"]}" stroke="{th["axis"]}"/>'
    if axes:
        g += f'<text x="{(m["l"] + W - m["r"]) / 2:.0f}" y="{H - 14}" text-anchor="middle" {fs(12)} fill="{th["mut"]}">{esc(xlabel)}</text>'
        g += f'<text transform="translate(18 {(m["t"] + H - m["b"]) / 2:.0f}) rotate(-90)" text-anchor="middle" {fs(12)} fill="{th["mut"]}">{esc(ylabel)}</text>'
        g += f'<text x="{m["l"] + 10}" y="{m["t"] + 16}" {fs(10)} font-weight="700" letter-spacing="1" fill="#34785b">BETTER</text>'
    if front:
        fr = pareto(points)
        if len(fr) > 1:
            g += f'<polyline points="{" ".join(f"{X(p["x"]):.1f},{Y(p["y"]):.1f}" for p in fr)}" fill="none" stroke="{th["mut"]}" stroke-width="1.7" stroke-dasharray="5 5" opacity=".7"/>'
    if ci:
        for p in points:
            if "lo" in p:
                x = X(p["x"])
                g += (f'<g stroke="{p["color"]}" stroke-width="{2 * font:.1f}" opacity=".5"><line x1="{x:.1f}" x2="{x:.1f}" y1="{Y(p["lo"]):.1f}" y2="{Y(p["hi"]):.1f}"/>'
                      f'<line x1="{x - 5:.1f}" x2="{x + 5:.1f}" y1="{Y(p["lo"]):.1f}" y2="{Y(p["lo"]):.1f}"/><line x1="{x - 5:.1f}" x2="{x + 5:.1f}" y1="{Y(p["hi"]):.1f}" y2="{Y(p["hi"]):.1f}"/></g>')
    for p in sorted(points, key=lambda q: -q["r"]):
        cx, cy = X(p["x"]), Y(p["y"])
        g += f'<circle cx="{cx:.1f}" cy="{cy:.1f}" r="{p["r"]:.1f}" fill="{p["color"]}" fill-opacity="{opacity}" stroke="{th["halo"]}" stroke-width="2"/>'
        if labels and "name" in p:
            right = cx < W - 200 * font
            tx = cx + (1 if right else -1) * (p["r"] + 6)
            common = f'x="{tx:.1f}" y="{cy + 4:.1f}" text-anchor="{"start" if right else "end"}" {fs(12)} font-weight="700"'
            g += f'<text {common} fill="none" stroke="{th["halo"]}" stroke-width="3.5" stroke-linejoin="round">{esc(p["name"])}</text>'
            g += f'<text {common} fill="{p["color"]}">{esc(p["name"])}</text>'
    return g

def latency_radius(p50_ms, font=1.0):
    """Bubble radius for a median latency: area proportional to it, 1,500 ms = 30 px (so fast models stay small)."""
    return max(4, 30 * font * math.sqrt(p50_ms / 1500))
