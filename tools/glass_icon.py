"""Draw the Liquid Glass app icon (a glass shuttlecock on a blue-to-cyan
tile) as assets/icon/app.svg and the 1024 px master assets/icon/app-1024.png.
Builds the SVG here and renders it with headless Chrome. Afterwards run
tools/build_app_icon.py to make the .ico, .png and window icon from it.

    python3 tools/glass_icon.py
"""
import math, os, subprocess, tempfile
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
S = 1024
TILT = 38            # degrees clockwise; skirt points to the top right
SCALE = 1.2

# ---------- geometry (upright shuttle, y down, then tilted into screen space)
TOP_Y, TOP_HW = -265, 262      # feather tips
BASE_Y, BASE_HW = 118, 86      # where the skirt meets the cork
CORK_HW, CORK_Y1 = 104, 158    # cork: short collar, then a round dome
N_FEATHERS = 4

def raw_points():
    pts = [(-TOP_HW, TOP_Y - 40), (TOP_HW, TOP_Y - 40), (0, CORK_Y1 + CORK_HW)]
    return pts

def make_xf():
    a = math.radians(TILT)
    ca, sa = math.cos(a), math.sin(a)
    def rot(p):
        return (SCALE * (p[0] * ca - p[1] * sa), SCALE * (p[0] * sa + p[1] * ca))
    # centre the whole silhouette's bounding box on the tile
    sil = [(-TOP_HW, TOP_Y), (TOP_HW, TOP_Y), (0, TOP_Y - 30)]
    sil += [(CORK_HW * math.cos(t), CORK_Y1 + CORK_HW * math.sin(t)) for t in [i * math.pi / 12 for i in range(13)]]
    r = [rot(p) for p in sil]
    cx = (min(p[0] for p in r) + max(p[0] for p in r)) / 2
    cy = (min(p[1] for p in r) + max(p[1] for p in r)) / 2
    return lambda p: (rot(p)[0] - cx + S / 2 + 4, rot(p)[1] - cy + S / 2 - 2)

XF = make_xf()
f = lambda p: '%.1f %.1f' % XF(p)

def skirt_path():
    d = 'M' + f((-BASE_HW, BASE_Y)) + ' L' + f((-TOP_HW, TOP_Y))
    w = 2 * TOP_HW / N_FEATHERS
    r = w * 0.62 * SCALE   # scallop radius: gently rounded feather tips
    for i in range(N_FEATHERS):
        x1 = -TOP_HW + (i + 1) * w
        d += ' A%.1f %.1f 0 0 1 %s' % (r, r, f((x1, TOP_Y)))
    d += ' L' + f((BASE_HW, BASE_Y)) + ' Z'
    return d

def cork_path():
    r = CORK_HW * SCALE
    top = BASE_Y - 6
    d = 'M' + f((-CORK_HW + 14, top)) + ' L' + f((CORK_HW - 14, top))
    d += ' Q' + f((CORK_HW, top)) + ' ' + f((CORK_HW, top + 16))
    d += ' L' + f((CORK_HW, CORK_Y1))
    d += ' A%.1f %.1f 0 0 1 %s' % (r, r, f((-CORK_HW, CORK_Y1)))
    d += ' L' + f((-CORK_HW, top + 16))
    d += ' Q' + f((-CORK_HW, top)) + ' ' + f((-CORK_HW + 14, top)) + ' Z'
    return d

def band_path():
    y0, y1 = BASE_Y + 22, BASE_Y + 50
    return 'M%s L%s L%s L%s Z' % (f((-CORK_HW, y0)), f((CORK_HW, y0)), f((CORK_HW, y1)), f((-CORK_HW, y1)))

def lerp(a, b, t):
    return (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)

def vanes():
    out = []
    for i in range(1, N_FEATHERS):
        t = i / N_FEATHERS
        lo = lerp((-BASE_HW, BASE_Y), (BASE_HW, BASE_Y), t)
        hi = lerp((-TOP_HW, TOP_Y), (TOP_HW, TOP_Y), t)
        out.append('M%s L%s' % (f(lerp(lo, hi, 0.06)), f(hi)))
    return ' '.join(out)

def thread(t):
    l = lerp((-BASE_HW, BASE_Y), (-TOP_HW, TOP_Y), t)
    r = lerp((BASE_HW, BASE_Y), (TOP_HW, TOP_Y), t)
    return 'M%s L%s' % (f(l), f(r))

def squircle(pad, n=5.0, steps=400):
    c, r = S / 2, S / 2 - pad
    pts = []
    for k in range(steps):
        t = 2 * math.pi * k / steps
        ct, st = math.cos(t), math.sin(t)
        x = c + r * math.copysign(abs(ct) ** (2 / n), ct)
        y = c + r * math.copysign(abs(st) ** (2 / n), st)
        pts.append('%.1f %.1f' % (x, y))
    return 'M' + ' L'.join(pts) + ' Z'

def svg():
    tile = squircle(36)
    sk, ck, bd = skirt_path(), cork_path(), band_path()
    scx, scy = XF((0, -60))
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{S}" height="{S}" viewBox="0 0 {S} {S}">
<defs>
  <clipPath id="tileClip"><path d="{tile}"/></clipPath>
  <clipPath id="skClip"><path d="{sk}"/></clipPath>
  <clipPath id="ckClip"><path d="{ck}"/></clipPath>
  <clipPath id="shClip"><path d="{sk}"/><path d="{ck}"/></clipPath>

  <linearGradient id="bg" x1="0.1" y1="1" x2="0.9" y2="0">
    <stop offset="0" stop-color="#0A3FA8"/>
    <stop offset="0.45" stop-color="#0F6CBD"/>
    <stop offset="1" stop-color="#35C9EE"/>
  </linearGradient>
  <radialGradient id="glowA" cx="0.78" cy="0.2" r="0.6">
    <stop offset="0" stop-color="#9FF3FF" stop-opacity="0.75"/>
    <stop offset="1" stop-color="#9FF3FF" stop-opacity="0"/>
  </radialGradient>
  <radialGradient id="glowB" cx="0.18" cy="0.9" r="0.6">
    <stop offset="0" stop-color="#062A86" stop-opacity="0.8"/>
    <stop offset="1" stop-color="#062A86" stop-opacity="0"/>
  </radialGradient>
  <radialGradient id="glowC" cx="0.5" cy="0.52" r="0.42">
    <stop offset="0" stop-color="#4FB6F5" stop-opacity="0.55"/>
    <stop offset="1" stop-color="#4FB6F5" stop-opacity="0"/>
  </radialGradient>

  <linearGradient id="rimTL" x1="0" y1="0" x2="1" y2="1">
    <stop offset="0" stop-color="#fff" stop-opacity="0.95"/>
    <stop offset="0.3" stop-color="#fff" stop-opacity="0.25"/>
    <stop offset="0.62" stop-color="#fff" stop-opacity="0.02"/>
    <stop offset="1" stop-color="#C9F6FF" stop-opacity="0.6"/>
  </linearGradient>

  <linearGradient id="glassFill" gradientUnits="userSpaceOnUse" x1="300" y1="180" x2="760" y2="800">
    <stop offset="0" stop-color="#fff" stop-opacity="0.8"/>
    <stop offset="0.5" stop-color="#F1FAFF" stop-opacity="0.5"/>
    <stop offset="1" stop-color="#E2F5FF" stop-opacity="0.64"/>
  </linearGradient>
  <linearGradient id="glassRim" gradientUnits="userSpaceOnUse" x1="280" y1="160" x2="780" y2="820">
    <stop offset="0" stop-color="#fff" stop-opacity="1"/>
    <stop offset="0.45" stop-color="#fff" stop-opacity="0.45"/>
    <stop offset="0.7" stop-color="#fff" stop-opacity="0.25"/>
    <stop offset="1" stop-color="#fff" stop-opacity="0.95"/>
  </linearGradient>
  <linearGradient id="corkFill" gradientUnits="userSpaceOnUse" x1="170" y1="600" x2="420" y2="860">
    <stop offset="0" stop-color="#fff" stop-opacity="0.98"/>
    <stop offset="0.6" stop-color="#F2FBFF" stop-opacity="0.9"/>
    <stop offset="1" stop-color="#BFE6FA" stop-opacity="0.88"/>
  </linearGradient>
  <linearGradient id="bandFill" gradientUnits="userSpaceOnUse" x1="250" y1="560" x2="460" y2="760">
    <stop offset="0" stop-color="#27B7EA"/>
    <stop offset="1" stop-color="#0B59B8"/>
  </linearGradient>

  <filter id="b6" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="6"/></filter>
  <filter id="b14" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="14"/></filter>
  <filter id="b26" x="-40%" y="-40%" width="180%" height="180%"><feGaussianBlur stdDeviation="26"/></filter>
  <filter id="b40" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="40"/></filter>
  <filter id="frost" x="-20%" y="-20%" width="140%" height="140%">
    <feGaussianBlur stdDeviation="16"/>
    <feColorMatrix type="saturate" values="1.1"/>
  </filter>

  <g id="backdrop">
    <rect width="{S}" height="{S}" fill="url(#bg)"/>
    <rect width="{S}" height="{S}" fill="url(#glowB)"/>
    <rect width="{S}" height="{S}" fill="url(#glowA)"/>
    <rect width="{S}" height="{S}" fill="url(#glowC)"/>
    <!-- a soft flight arc: something for the glass to bend -->
    <path d="M-40 760 Q 420 250 1080 560" fill="none" stroke="#BDF4FF" stroke-opacity="0.34" stroke-width="54" filter="url(#b14)"/>
    <path d="M-40 930 Q 520 520 1080 800" fill="none" stroke="#0A3A9E" stroke-opacity="0.3" stroke-width="80" filter="url(#b26)"/>
  </g>
</defs>

<g clip-path="url(#tileClip)">
  <use href="#backdrop"/>

  <!-- shadow the glass casts on the backdrop -->
  <g transform="translate(22 40)" filter="url(#b40)" opacity="0.5">
    <path d="{sk}" fill="#04257A"/><path d="{ck}" fill="#04257A"/>
  </g>
  <g transform="translate(8 14)" filter="url(#b14)" opacity="0.32">
    <path d="{sk}" fill="#031E66"/><path d="{ck}" fill="#031E66"/>
  </g>

  <!-- skirt: refracted (magnified, shifted, frosted) backdrop, then the glass body -->
  <g clip-path="url(#skClip)">
    <g filter="url(#frost)">
      <g transform="translate({scx:.1f} {scy:.1f}) scale(1.28) translate({-scx - 26:.1f} {-scy - 34:.1f})"><use href="#backdrop"/></g>
    </g>
    <path d="{sk}" fill="url(#glassFill)"/>
    <!-- feather vanes, engraved -->
    <path d="{vanes()}" fill="none" stroke="#0A4CB0" stroke-opacity="0.42" stroke-width="15" stroke-linecap="round"/>
    <path d="{vanes()}" fill="none" stroke="#fff" stroke-opacity="0.7" stroke-width="4" stroke-linecap="round" transform="translate(-6 -5)"/>
    <!-- binding thread -->
    <path d="{thread(0.36)}" fill="none" stroke="#0A4CB0" stroke-opacity="0.3" stroke-width="12"/>
    <path d="{thread(0.36)}" fill="none" stroke="#fff" stroke-opacity="0.65" stroke-width="4" transform="translate(-4 -6)"/>
    <!-- thickness: inner glow at the lit edge, inner shade at the far edge -->
    <path d="{sk}" fill="none" stroke="#fff" stroke-opacity="0.7" stroke-width="44" filter="url(#b14)" transform="translate(10 12)"/>
    <path d="{sk}" fill="none" stroke="#0B4BB5" stroke-opacity="0.38" stroke-width="30" filter="url(#b14)" transform="translate(-14 -18)"/>
    <!-- broad sheen across the top-left -->
    <ellipse cx="{XF((-150, -150))[0]:.1f}" cy="{XF((-150, -150))[1]:.1f}" rx="150" ry="70" transform="rotate({TILT - 62} {XF((-150, -150))[0]:.1f} {XF((-150, -150))[1]:.1f})" fill="#fff" opacity="0.4" filter="url(#b26)"/>
    <!-- crisp specular rim -->
    <path d="{sk}" fill="none" stroke="url(#glassRim)" stroke-width="13" stroke-linejoin="round"/>
  </g>

  <!-- cork: brighter, denser glass -->
  <g transform="translate(6 10)" filter="url(#b6)" opacity="0.3"><path d="{ck}" fill="#052C8A"/></g>
  <g clip-path="url(#ckClip)">
    <path d="{ck}" fill="url(#corkFill)"/>
    <path d="{bd}" fill="url(#bandFill)" opacity="0.9"/>
    <path d="{ck}" fill="none" stroke="#2E8FD8" stroke-opacity="0.55" stroke-width="40" filter="url(#b14)" transform="translate(-16 -20)"/>
    <ellipse cx="{XF((-42, 170))[0]:.1f}" cy="{XF((-42, 170))[1]:.1f}" rx="46" ry="26" transform="rotate(-38 {XF((-42, 170))[0]:.1f} {XF((-42, 170))[1]:.1f})" fill="#fff" opacity="0.95" filter="url(#b6)"/>
    <path d="{ck}" fill="none" stroke="#fff" stroke-opacity="0.95" stroke-width="12"/>
  </g>

  <!-- tile glass: top sheen, inner glow, specular rim -->
  <path d="{tile}" fill="none" stroke="#fff" stroke-opacity="0.22" stroke-width="40" filter="url(#b14)"/>
  <path d="{tile}" fill="none" stroke="url(#rimTL)" stroke-width="16"/>
  <path d="{tile}" fill="none" stroke="#fff" stroke-opacity="0.35" stroke-width="3"/>
</g>
</svg>'''

def render(svg_text, out_png, px):
    with tempfile.TemporaryDirectory() as tmp:
        html = os.path.join(tmp, 'icon.html')
        with open(html, 'w') as fh:
            fh.write('<!doctype html><html><body style="margin:0;background:transparent;overflow:hidden">' + svg_text + '</body></html>')
        subprocess.run(['google-chrome', '--headless=new', '--no-sandbox', '--disable-gpu', '--hide-scrollbars',
                        '--default-background-color=00000000', f'--force-device-scale-factor={px / S}',
                        f'--window-size={S},{S}', f'--user-data-dir={tmp}/prof', f'--screenshot={out_png}', 'file://' + html],
                       check=True, capture_output=True)

if __name__ == '__main__':
    out = os.path.join(HERE, '..', 'assets', 'icon')
    text = svg()
    open(os.path.join(out, 'app.svg'), 'w').write(text)
    with tempfile.TemporaryDirectory() as tmp:
        master = os.path.join(tmp, 'icon-2048.png')
        render(text, master, 2048)
        Image.open(master).convert('RGBA').resize((1024, 1024), Image.LANCZOS).save(os.path.join(out, 'app-1024.png'))
    print('wrote assets/icon/app.svg, app-1024.png')
