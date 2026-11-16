"""Draw the app icon (a brand-blue tile with the white shuttlecock) as
assets/icon/app.ico (Windows .exe icon) and assets/icon/app.rgba (the
window icon, raw 256 x 256 RGBA).

    python tools/build_app_icon.py
"""
from PIL import Image, ImageDraw, ImageFont

FONT = 'assets/fonts/FluentIcons-DropIn.ttf'
SHUTTLE_FILLED = chr(0x100003)
BLUE = (15, 108, 189, 255)

def tile(size):
    s = size * 4  # draw large, then scale down for clean edges
    img = Image.new('RGBA', (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pad = round(s * 0.04)
    d.rounded_rectangle([pad, pad, s - pad, s - pad], radius=round(s * 0.22), fill=BLUE)
    font = ImageFont.truetype(FONT, round(s * 0.74))
    box = d.textbbox((0, 0), SHUTTLE_FILLED, font=font)
    w, h = box[2] - box[0], box[3] - box[1]
    d.text(((s - w) / 2 - box[0], (s - h) / 2 - box[1]), SHUTTLE_FILLED, font=font, fill=(255, 255, 255, 255))
    return img.resize((size, size), Image.LANCZOS)

big = tile(256)
big.save('assets/icon/app.ico', sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
big.save('assets/icon/app.png')
open('assets/icon/app.rgba', 'wb').write(big.tobytes())
print('wrote assets/icon/app.ico, app.png, app.rgba')
