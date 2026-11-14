"""Generates the icons of mouseless: a keycap with a mouse pointer.

Usage: python assets/make_icons.py   (requires Pillow)
"""
from pathlib import Path
from PIL import Image, ImageDraw

SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
HERE = Path(__file__).parent

VARIANTS = {
    # name: (keycap side, keycap top, arrow fill, arrow outline)
    "mouseless": ("#1f2937", "#374151", "#ffffff", "#111827"),
    "mouseless-active": ("#1d4ed8", "#3b82f6", "#ffffff", "#1e3a8a"),
    "mouseless-paused": ("#6b7280", "#9ca3af", "#e5e7eb", "#4b5563"),
}

# mouse pointer arrow, in units of the arrow box (0..1)
ARROW = [(0.0, 0.0), (0.0, 0.80), (0.21, 0.62), (0.35, 0.93), (0.49, 0.87), (0.36, 0.57), (0.62, 0.57)]


def render(size, side, top, fill, outline):
    s = 4  # supersampling
    n = size * s
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pad = round(n * 0.03)
    r = round(n * 0.2)
    # keycap: darker body with a lighter top face
    d.rounded_rectangle([pad, pad, n - pad - 1, n - pad - 1], radius=r, fill=side)
    inset = round(n * 0.09)
    d.rounded_rectangle([pad + inset * 0.6, pad + inset * 0.4, n - pad - inset * 0.6, n - pad - inset * 1.3],
                        radius=round(r * 0.75), fill=top)
    # arrow, slightly bigger at small sizes to stay readable
    box = n * (0.78 if size <= 16 else 0.68 if size <= 24 else 0.56)
    ox = (n - box * 0.62) / 2
    oy = (n - box * 0.93) / 2 - n * 0.03
    pts = [(ox + x * box, oy + y * box) for x, y in ARROW]
    width = max(s, round(n * 0.035))
    d.polygon(pts, fill=fill, outline=outline, width=width)
    return img.resize((size, size), Image.LANCZOS)


for name, colors in VARIANTS.items():
    images = [render(sz, *colors) for sz in SIZES]
    images[-1].save(HERE / f"{name}.ico", sizes=[(sz, sz) for sz in SIZES], append_images=images[:-1])
    if name == "mouseless":
        images[-1].save(HERE / f"{name}.png")
print("icons written to", HERE)
