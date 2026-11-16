"""Make the app icon files from the 1024 px master assets/icon/app-1024.png
(drawn by tools/glass_icon.py): assets/icon/app.ico (the Windows .exe icon),
assets/icon/app.png and assets/icon/app.rgba (the window icon, raw
256 x 256 RGBA).

    python tools/build_app_icon.py
"""
from PIL import Image

master = Image.open('assets/icon/app-1024.png').convert('RGBA')
big = master.resize((256, 256), Image.LANCZOS)
sizes = [16, 24, 32, 48, 64, 128, 256]
big.save('assets/icon/app.ico', sizes=[(n, n) for n in sizes],
         append_images=[master.resize((n, n), Image.LANCZOS) for n in sizes[:-1]])
big.save('assets/icon/app.png')
open('assets/icon/app.rgba', 'wb').write(big.tobytes())
print('wrote assets/icon/app.ico, app.png, app.rgba')
