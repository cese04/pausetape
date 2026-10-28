"""Draw the app icon (1024 px PNG) in the app's own style: warm paper, an ink serif initial,
and a small loudness sparkline ending in the red playhead dot.

    python3 scripts/make-icon.py [letter]      ->  assets/icon/icon-1024.png

The letter is a placeholder until the app has its final name.
"""

import math
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

SIZE = 1024
# macOS icon grid: the body is ~80% of the canvas, with a soft corner radius.
BODY = 824
RADIUS = 185
PAPER = (255, 255, 248, 255)
INK = (17, 17, 17, 255)
FAINT = (205, 205, 196, 255)
ACCENT = (160, 0, 0, 255)
FONTS = ["/System/Library/Fonts/Palatino.ttc", "/System/Library/Fonts/Supplemental/Georgia.ttf"]

letter = sys.argv[1] if len(sys.argv) > 1 else "N"
out = Path(__file__).resolve().parent.parent / "assets" / "icon" / "icon-1024.png"
out.parent.mkdir(parents=True, exist_ok=True)

# Draw at 2x and downsample for smooth edges.
S = 2
img = Image.new("RGBA", (SIZE * S, SIZE * S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
left = (SIZE - BODY) // 2 * S
top = left
box = (left, top, left + BODY * S, top + BODY * S)

# A soft shadow, then the paper.
shadow = Image.new("RGBA", img.size, (0, 0, 0, 0))
ImageDraw.Draw(shadow).rounded_rectangle((box[0], box[1] + 12 * S, box[2], box[3] + 12 * S), RADIUS * S, fill=(0, 0, 0, 60))
img = Image.alpha_composite(img, shadow)
d = ImageDraw.Draw(img)
d.rounded_rectangle(box, RADIUS * S, fill=PAPER, outline=FAINT, width=2 * S)

font = next((ImageFont.truetype(f, 560 * S, index=0) for f in FONTS if Path(f).exists()), ImageFont.load_default())
cx = SIZE * S // 2
d.text((cx, 455 * S), letter, font=font, fill=INK, anchor="mm")

# Sparkline: a loudness envelope across the lower part, played portion in ink.
y0, x0, x1 = 760 * S, 245 * S, 779 * S
points = []
# Few, thick bars so it still reads at 32 px.
BARS = 34
for i in range(BARS):
    t = i / (BARS - 1)
    amp = 0.25 + 0.75 * abs(math.sin(t * 7.1 + 0.3) * math.cos(t * 3.3)) ** 0.8
    points.append((x0 + (x1 - x0) * t, amp))
head = 0.7
for x, amp in points:
    h = amp * 60 * S
    played = (x - x0) / (x1 - x0) <= head
    d.line((x, y0 - h, x, y0 + h), fill=INK if played else FAINT, width=8 * S)
hx = x0 + (x1 - x0) * head
r = 13 * S
d.ellipse((hx - r, y0 - r, hx + r, y0 + r), fill=ACCENT)

img.resize((SIZE, SIZE), Image.LANCZOS).save(out)
print(out)
