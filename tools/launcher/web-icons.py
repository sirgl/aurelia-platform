#!/usr/bin/env python3
"""Make the page icons (PWA manifest, apple-touch-icon, favicon) from packaging/aurelia.png
(the application icon): LAUNCHER/web/static/icons/*.png. Run by hand; the PNGs are committed."""
import os
import sys
from PIL import Image

# the launcher folder: python3 web-icons.py lands/launcher
root = os.path.abspath(sys.argv[1])
src = Image.open(os.path.join(root, "packaging/aurelia.png")).convert("RGBA")
out = os.path.join(root, "web/static/icons")
os.makedirs(out, exist_ok=True)
PAPER = (232, 216, 176, 255)  # the icon's parchment

def rounded(size):
    return src.resize((size, size), Image.LANCZOS)

def full_bleed(size, inner):
    """Opaque square (iOS rounds it itself; Android masks the maskable one): the compass
    stays inside the central `inner` share, the safe zone of a maskable icon."""
    bg = Image.new("RGBA", (size, size), PAPER)
    box = src.crop((int(512 * 0.11), int(512 * 0.11), int(512 * 0.89), int(512 * 0.89)))
    box = box.resize((round(size * inner), round(size * inner)), Image.LANCZOS)
    bg.alpha_composite(box, ((size - box.width) // 2, (size - box.height) // 2))
    return bg

rounded(192).save(os.path.join(out, "icon-192.png"), optimize=True)
rounded(512).save(os.path.join(out, "icon-512.png"), optimize=True)
full_bleed(512, 0.66).save(os.path.join(out, "icon-maskable-512.png"), optimize=True)
full_bleed(180, 0.80).convert("RGB").save(os.path.join(out, "apple-touch-icon.png"), optimize=True)
rounded(48).save(os.path.join(out, "favicon.png"), optimize=True)
print("icons in", out)
