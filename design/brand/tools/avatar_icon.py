"""Build the full-bleed square profile avatar from the master mark.

Usage: python3 -I avatar_icon.py MARK_DIR

Produces MARK_DIR/avatar-1024.png: an opaque 1024x1024 PNG filled edge-to-edge
with the Meadow light gradient (matching the desktop app icon) and the master
mark optically centered inside the circle-crop safe zone for Gmail, X, GitHub,
and other profile accounts.
"""

import sys
from pathlib import Path

from PIL import Image, ImageDraw

CANVAS = 1024
MARK_HEIGHT = 760
Y_OFFSET = 8
TOP, BOTTOM = (247, 249, 242), (214, 236, 196)  # Meadow light: paper to soft sprout


def build_avatar(master: Image.Image) -> Image.Image:
    canvas = Image.new("RGBA", (CANVAS, CANVAS))
    draw = ImageDraw.Draw(canvas)
    for y in range(CANVAS):
        t = y / (CANVAS - 1)
        colour = tuple(round(TOP[i] + (BOTTOM[i] - TOP[i]) * t) for i in range(3))
        draw.line([(0, y), (CANVAS - 1, y)], fill=(*colour, 255))

    mark = master.crop(master.getbbox())
    width = round(mark.width * MARK_HEIGHT / mark.height)
    mark = mark.resize((width, MARK_HEIGHT), Image.LANCZOS)
    canvas.alpha_composite(mark, ((CANVAS - width) // 2, (CANVAS - MARK_HEIGHT) // 2 + Y_OFFSET))
    return canvas.convert("RGB")


def main() -> None:
    mark_dir = Path(sys.argv[1])
    master = Image.open(mark_dir / "stackspirit-mark-master.png").convert("RGBA")
    out = mark_dir / "avatar-1024.png"
    build_avatar(master).save(out, optimize=True)
    print("avatar written:", out)


if __name__ == "__main__":
    main()
