"""Outlines a word in a variable font at one weight, for the wordmark.

    python3 assets/brand/src/glyphs.py FONT TEXT WGHT > assets/brand/src/contour-glyphs.json

Needs fonttools, brotli (woff2) and uharfbuzz. Text is shaped by HarfBuzz so
the font's own kerning (GPOS) applies; then a light -2% em tracking. Output
is one SVG path in font units (y down, baseline at 0) plus the ink extents
build.py uses to place and size the wordmark.
"""
import io, json, sys
import uharfbuzz as hb
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.transformPen import TransformPen

path, text, wght = sys.argv[1], sys.argv[2], float(sys.argv[3])
font = TTFont(path)
font.flavor = None
buf = io.BytesIO(); font.save(buf)
upm = font['head'].unitsPerEm

hb_font = hb.Font(hb.Face(hb.Blob(buf.getvalue())))
hb_font.set_variations({"wght": wght})
hb_buf = hb.Buffer(); hb_buf.add_str(text); hb_buf.guess_segment_properties()
hb.shape(hb_font, hb_buf, {"kern": True, "liga": False})

inst = instantiateVariableFont(font, {"wght": wght})
gs = inst.getGlyphSet(); order = inst.getGlyphOrder()
track = -0.02 * upm
x = 0.0; out = []; bounds = BoundsPen(gs)
for info, pos in zip(hb_buf.glyph_infos, hb_buf.glyph_positions):
    name = order[info.codepoint]
    m = (1, 0, 0, -1, x + pos.x_offset, -pos.y_offset)
    pen = SVGPathPen(gs)
    gs[name].draw(TransformPen(pen, m))
    gs[name].draw(TransformPen(bounds, m))
    out.append(pen.getCommands())
    x += pos.x_advance + track
xmin, ymin, xmax, ymax = bounds.bounds
os2 = inst['OS/2']
print(json.dumps({"text": text, "wght": wght, "d": " ".join(out), "advance": x - track,
                  "xmin": xmin, "xmax": xmax, "upm": upm, "cap": os2.sCapHeight,
                  "asc": inst['hhea'].ascent, "desc": inst['hhea'].descent}))
