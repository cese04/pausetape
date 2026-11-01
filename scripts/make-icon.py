#!/usr/bin/env python3
"""The app icon: a cassette seen from the front, its label written by hand, on the app's paper.

    python3 scripts/make-icon.py [handwriting.ttf]   ->  assets/icon/icon-1024.png

The handwriting is Caveat Bold (SIL Open Font License, from Google Fonts), as with a pen on a
tape's label; it's downloaded into target/ the first time, and only the drawing is kept, the
font isn't shipped. Needs Pillow.
"""

import math
import sys
import urllib.request
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
FONT_URL = "https://github.com/google/fonts/raw/main/ofl/caveat/Caveat%5Bwght%5D.ttf"
OUT = ROOT / "assets/icon/icon-1024.png"

# The app's light palette (src/ui/theme.rs).
PAPER = (250, 248, 242)
FAINT = (224, 219, 207)
INK = (51, 48, 43)
BRICK = (165, 64, 47)
LABEL = (243, 236, 217)
WINDOW = (30, 28, 25)
TAPE = (92, 74, 58)
SHELL_LOW = (66, 62, 56)

S = 4  # drawn at 4x, then scaled down: smooth edges without an antialiasing renderer


def font_path() -> Path:
    if len(sys.argv) > 1:
        return Path(sys.argv[1])
    path = ROOT / "target/Caveat.ttf"
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        urllib.request.urlretrieve(FONT_URL, path)
    return path


def box(x0, y0, x1, y1):
    return [x0 * S, y0 * S, x1 * S, y1 * S]


def reel(d: ImageDraw.ImageDraw, cx: float, cy: float, pack: float, turn: float):
    """A reel in the window: the tape wound on it (`pack`, its radius), then the hub and its teeth."""
    d.ellipse(box(cx - pack, cy - pack, cx + pack, cy + pack), fill=TAPE)
    hub = 22
    d.ellipse(box(cx - hub, cy - hub, cx + hub, cy + hub), fill=LABEL)
    d.ellipse(box(cx - 11, cy - 11, cx + 11, cy + 11), fill=WINDOW)
    for k in range(6):
        a = turn + k * math.pi / 3
        x, y = cx + 15 * math.cos(a), cy + 15 * math.sin(a)
        d.ellipse(box(x - 4, y - 4, x + 4, y + 4), fill=WINDOW)


def main():
    img = Image.new("RGBA", (1024 * S, 1024 * S), (0, 0, 0, 0))

    # The paper square, as on macOS: a soft shadow under it, a hairline round it.
    shadow = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle(box(100, 112, 924, 936), radius=185 * S, fill=(0, 0, 0, 70))
    img.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(14 * S)))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle(box(100, 100, 924, 924), radius=185 * S, fill=PAPER, outline=FAINT, width=3 * S)

    # The shell, a little above the middle (the eye sits a cassette lower than it is).
    x0, y0, x1, y1 = 152, 268, 872, 724
    d.rounded_rectangle(box(x0, y0, x1, y1), radius=40 * S, fill=INK)
    # The lower part, where the tape runs past the heads: a step down, a shade lighter.
    d.polygon([(300 * S, y1 * S), (338 * S, 640 * S), (686 * S, 640 * S), (724 * S, y1 * S)], fill=SHELL_LOW)
    for cx in (412, 612):
        d.ellipse(box(cx - 13, 669, cx + 13, 695), fill=INK)
    # Screws in the corners.
    for cx, cy in ((182, 298), (842, 298), (182, 694), (842, 694)):
        d.ellipse(box(cx - 9, cy - 9, cx + 9, cy + 9), fill=SHELL_LOW)

    # The label, written by hand, over a band of the accent.
    lx0, ly0, lx1, ly1 = 196, 304, 828, 612
    d.rounded_rectangle(box(lx0, ly0, lx1, ly1), radius=18 * S, fill=LABEL)
    d.rectangle(box(lx0, 446, lx1, 462), fill=BRICK)
    font = ImageFont.truetype(str(font_path()), 150 * S)
    font.set_variation_by_name("Bold")
    text = "Pausetape"
    l, t, r, b = d.textbbox((0, 0), text, font=font)
    # A little slanted, as written quickly across the label.
    word = Image.new("RGBA", (r - l + 40 * S, b - t + 40 * S), (0, 0, 0, 0))
    ImageDraw.Draw(word).text((20 * S - l, 20 * S - t), text, font=font, fill=INK)
    word = word.rotate(3, resample=Image.BICUBIC, expand=True)
    cx, cy = (lx0 + lx1) / 2 * S, 378 * S
    img.alpha_composite(word, (int(cx - word.width / 2), int(cy - word.height / 2)))

    # The window: the left reel full (side A just begun), the right one nearly bare.
    d = ImageDraw.Draw(img)
    d.rounded_rectangle(box(318, 486, 706, 588), radius=22 * S, fill=WINDOW)
    reel(d, 398, 537, 47, 0.2)
    reel(d, 626, 537, 29, 0.9)
    # The tape between them, along the bottom of the window.
    d.line(box(398, 584, 626, 584), fill=TAPE, width=4 * S)

    img.resize((1024, 1024), Image.LANCZOS).save(OUT)
    print(f"wrote {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
