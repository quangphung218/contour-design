import sys, json
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
f = TTFont(sys.argv[1])
f = instantiateVariableFont(f, {"wght": float(sys.argv[3])})
gs = f.getGlyphSet(); cmap = f.getBestCmap(); upm = f['head'].unitsPerEm
text = sys.argv[2]
x = 0; out = []
hmtx = f['hmtx']
track = -0.02 * upm
for ch in text:
    name = cmap[ord(ch)]
    pen = SVGPathPen(gs)
    tp = TransformPen(pen, (1, 0, 0, -1, x, 0))
    gs[name].draw(tp)
    out.append(pen.getCommands())
    x += hmtx[name][0] + track
os2 = f['OS/2']
print(json.dumps({"d": " ".join(out), "advance": x - track, "upm": upm, "cap": os2.sCapHeight, "asc": f['hhea'].ascent, "desc": f['hhea'].descent}))
