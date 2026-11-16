"""Draw the shuttlecock icon (regular and filled) into the app's icon font,
in place of Fluent's generic "sport" glyph (a basketball and a football).

    python tools/shuttle_glyph.py assets/fonts/FluentIcons-DropIn.ttf

Run after tools/build_icons.py. Designed on Fluent's 24-unit grid.
"""
import math, sys
from fontTools.ttLib import TTFont
from fontTools.pens.ttGlyphPen import TTGlyphPen

REGULAR, FILLED = 0xEB67, 0x100003

def circle(cx, cy, r, n=28):
    return [(cx + r * math.cos(2 * math.pi * k / n), cy + r * math.sin(2 * math.pi * k / n)) for k in range(n)]

def lerp(a, b, t):
    return (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)

def quad(a, b, c, d):
    return [a, b, c, d]

def bar(p, q, w):
    """A straight stroke of width w from p to q."""
    dx, dy = q[0] - p[0], q[1] - p[1]
    l = math.hypot(dx, dy)
    nx, ny = -dy / l * w / 2, dx / l * w / 2
    return [(p[0] + nx, p[1] + ny), (q[0] + nx, q[1] + ny), (q[0] - nx, q[1] - ny), (p[0] - nx, p[1] - ny)]

# Upright shuttle (y down): skirt flaring up from the cork.
TOP_L, TOP_R = (4.6, 2.6), (19.4, 2.6)
BASE_L, BASE_R = (9.3, 15.2), (14.7, 15.2)
CORK = (12.0, 17.9, 3.6)  # centre x, y, radius

def filled():
    solid = [quad(TOP_L, TOP_R, BASE_R, BASE_L), circle(*CORK)]
    # Feathers: two slits running up the skirt, broken by the band that ties
    # them together (holes must not overlap, or they fill back in).
    holes = []
    band = 0.5
    for t in (0.33, 0.67):
        lo = lerp(BASE_L, BASE_R, t)
        hi = lerp(TOP_L, TOP_R, t)
        holes.append(bar(lerp(lo, hi, 0.12), lerp(lo, hi, band - 0.09), 1.2))
        holes.append(bar(lerp(lo, hi, band + 0.09), lerp(lo, hi, 0.9), 1.2))
    left, right = lerp(BASE_L, TOP_L, band), lerp(BASE_R, TOP_R, band)
    holes.append(bar(lerp(left, right, 0.08), lerp(left, right, 0.92), 1.2))
    return solid, holes

def regular():
    w = 1.5
    outer = quad(TOP_L, TOP_R, BASE_R, BASE_L)
    # The skirt as a ring (outer outline minus an inset copy), with a rib up
    # the middle and the band across it.
    inset = quad((TOP_L[0] + 1.9, TOP_L[1] + w), (TOP_R[0] - 1.9, TOP_R[1] + w), (BASE_R[0] - 1.1, BASE_R[1] - w), (BASE_L[0] + 1.1, BASE_L[1] - w))
    left, right = lerp(BASE_L, TOP_L, 0.5), lerp(BASE_R, TOP_R, 0.5)
    ribs = [bar(lerp(BASE_L, BASE_R, 0.5), lerp(TOP_L, TOP_R, 0.5), w), bar(left, right, w)]
    cx, cy, r = CORK
    return [outer] + ribs + [circle(cx, cy, r)], [inset, circle(cx, cy + 0.4, r - w)], outer

def place(contours, holes):
    """Tilt 45° (skirt to the top right), fit to the 2..22 box, to font units."""
    a = math.radians(45)
    rot = lambda p: ((p[0] - 12) * math.cos(a) - (p[1] - 12) * math.sin(a), (p[0] - 12) * math.sin(a) + (p[1] - 12) * math.cos(a))
    allc = [[rot(p) for p in c] for c in contours + holes]
    xs = [p[0] for c in allc for p in c]
    ys = [p[1] for c in allc for p in c]
    s = 19.0 / max(max(xs) - min(xs), max(ys) - min(ys))
    ox, oy = (min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2
    unit = 500 / 24
    to_font = lambda p: (round((12 + (p[0] - ox) * s) * unit), round((24 - (12 + (p[1] - oy) * s)) * unit))
    out = []
    for k, c in enumerate(allc):
        pts = [to_font(p) for p in c]
        area = sum(pts[i][0] * pts[(i + 1) % len(pts)][1] - pts[(i + 1) % len(pts)][0] * pts[i][1] for i in range(len(pts)))
        is_hole = k >= len(contours)
        # TrueType: filled contours clockwise, holes counter-clockwise.
        if (area > 0) != is_hole:
            pts.reverse()
        out.append(pts)
    return out

def glyph(contours):
    pen = TTGlyphPen(None)
    for c in contours:
        pen.moveTo(c[0])
        for p in c[1:]:
            pen.lineTo(p)
        pen.closePath()
    return pen.glyph()

path = sys.argv[1]
font = TTFont(path)
cmap = font.getBestCmap()
solid, holes = filled()
strokes, rholes, _ = regular()
for cp, (c, h) in ((FILLED, (solid, holes)), (REGULAR, (strokes, rholes))):
    name = cmap[cp]
    g = glyph(place(c, h))
    font['glyf'][name] = g
    font['hmtx'][name] = (500, g.xMin if hasattr(g, 'xMin') else 0)
font.save(path)
print('shuttle glyphs written to', path)
