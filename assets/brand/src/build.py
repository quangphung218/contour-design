"""Writes the Nét brand SVGs (assets/brand/*.svg) from one mark path.

    python3 assets/brand/src/build.py [glyphs.json]

The wordmark outlines come from Geist SemiBold (SIL OFL 1.1, vendored in
crates/od-ui/vendor/fonts), extracted once into src/net-glyphs.json by
src/glyphs.py (needs fonttools + brotli). PNGs and the app icon set are
rendered from these SVGs by src/render.mjs.
"""
import json, math, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.dirname(HERE)

# One continuous stroke on a 48 grid: the left and top edges of a frame,
# down its right edge, then folded inward into a pointer whose tip faces
# the frame's corner, closing back on the edge it left.
MARK = "M10 32V14a4 4 0 0 1 4-4h20a4 4 0 0 1 4 4v8L21 18l4 18 4-8 9-6"
STROKE = 3.6
# Small sizes (16-32 px) get a heavier stroke so it stays above 1px.
STROKE_SMALL = 4.6

ACCENT = "#2a7560"       # --color-accent (light)
ACCENT_DARK = "#4fae8c"  # --color-accent (dark)
INK = "#1c1b18"          # --color-text (light)
INK_DARK = "#edebe5"     # --color-text (dark)

def path(color, width=STROKE, extra=""):
    return (f'<path d="{MARK}" fill="none" stroke="{color}" stroke-width="{width}" '
            f'stroke-linecap="round" stroke-linejoin="round"{extra}/>')

def svg(w, h, body, title):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w:g} {h:g}" width="{w:g}" height="{h:g}" '
            f'role="img" aria-label="{title}"><title>{title}</title>{body}</svg>\n')

def write(name, text):
    with open(os.path.join(OUT, name), "w") as f:
        f.write(text)

def mark_files():
    write("net-mark.svg", svg(48, 48, path(ACCENT), "Nét"))
    write("net-mark-dark.svg", svg(48, 48, path(ACCENT_DARK), "Nét"))
    write("net-mark-mono.svg", svg(48, 48, path("currentColor"), "Nét"))
    write("net-mark-small.svg", svg(48, 48, path(ACCENT, STROKE_SMALL), "Nét"))

def wordmark_files(glyphs):
    # Cap height = 24 grid units, baseline on the pointer's lowest point.
    scale = 24 / glyphs["cap"]
    x0, base = 58 - 80 * scale, 36
    width = math.ceil(58 + glyphs["advance"] * scale - 80 * scale + 2)
    text = (lambda c: f'<path transform="translate({x0:.2f} {base}) scale({scale:.5f})" '
            f'fill="{c}" d="{glyphs["d"]}"/>')
    for name, mark, ink in [("net-wordmark.svg", ACCENT, INK),
                            ("net-wordmark-dark.svg", ACCENT_DARK, INK_DARK),
                            ("net-wordmark-mono.svg", "currentColor", "currentColor")]:
        write(name, svg(width, 48, path(mark) + text(ink), "Nét"))

def squircle(x, y, size, n=5.0, steps=256):
    """macOS-style superellipse |x|^n + |y|^n = 1, as a closed path."""
    r = size / 2
    cx, cy = x + r, y + r
    pts = []
    for i in range(steps):
        t = 2 * math.pi * i / steps
        c, s = math.cos(t), math.sin(t)
        px = cx + r * math.copysign(abs(c) ** (2 / n), c)
        py = cy + r * math.copysign(abs(s) ** (2 / n), s)
        pts.append(f"{px:.2f} {py:.2f}")
    return "M" + "L".join(pts) + "Z"

def app_icon(width):
    # Apple's 1024 grid: body 824x824 at 100,100 leaves the safe margin and
    # room for the shadow. Mark centred, ~52% of the body.
    body = squircle(100, 100, 824)
    s = 14.5
    tx, ty = 512 - 24 * s, 512 - 23 * s
    return svg(1024, 1024, f"""<defs>
<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#358a71"/><stop offset="1" stop-color="#1f5f4d"/></linearGradient>
<linearGradient id="hi" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".16"/><stop offset=".5" stop-color="#fff" stop-opacity="0"/></linearGradient>
<filter id="sh" x="-10%" y="-10%" width="120%" height="125%"><feDropShadow dx="0" dy="10" stdDeviation="12" flood-color="#1a140a" flood-opacity=".28"/></filter>
</defs>
<path d="{body}" fill="url(#bg)" filter="url(#sh)"/>
<path d="{body}" fill="url(#hi)"/>
<path d="{body}" fill="none" stroke="#fff" stroke-opacity=".10" stroke-width="2"/>
<g transform="translate({tx:.1f} {ty:.1f}) scale({s})">{path("#ffffff", width)}</g>""", "Nét")

def favicon():
    # Browser tabs: rounded tile so it reads on light and dark tab strips.
    return svg(32, 32, f"""<rect width="32" height="32" rx="7" fill="{ACCENT}"/>
<g transform="translate(16 15.6) scale(0.5) translate(-24 -23)">{path("#ffffff", 5.2)}</g>""", "Nét")

if __name__ == "__main__":
    glyphs = json.load(open(sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "net-glyphs.json")))
    mark_files()
    wordmark_files(glyphs)
    write("net-app-icon.svg", app_icon(STROKE))
    write("net-app-icon-small.svg", app_icon(STROKE_SMALL * 1.15))
    write("net-favicon.svg", favicon())
    # od-ui sidebar/onboarding mark: decorative (the link carries the name),
    # so no <title> tooltip.
    with open(os.path.join(OUT, "../../crates/od-ui/assets/net-mark.svg"), "w") as f:
        f.write(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48" width="48" height="48">{path("currentColor")}</svg>\n')
