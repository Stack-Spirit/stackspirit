"""Build the desktop app icons from the master mark.

Usage: python3 -I app_icon.py MARK_DIR ICONS_DIR

Large sizes: the master mark on a macOS-style rounded tile (824px tile on a
1024px canvas, Meadow light gradient, soft shadow). 16 and 32 px use the
hand-drawn pixel icons, which stay crisp where a scaled tile would blur.

Writes exactly the files tauri.conf.json lists, plus icon.png and 512x512.png:
32x32.png, 128x128.png, 128x128@2x.png, 512x512.png, icon.png, icon.icns,
icon.ico. icon.icns is built with macOS `iconutil`.
"""

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

CANVAS, TILE, RADIUS = 1024, 824, 186
TOP, BOTTOM = (247, 249, 242), (214, 236, 196)  # Meadow light: paper to soft sprout
EDGE = (203, 212, 193)


def tile_icon(master: Image.Image) -> Image.Image:
    canvas = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    offset = (CANVAS - TILE) // 2

    mask = Image.new("L", (TILE, TILE), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, TILE - 1, TILE - 1), RADIUS, fill=255)

    # Soft drop shadow, as macOS app icons carry one.
    shadow = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    shadow_mask = Image.new("L", (CANVAS, CANVAS), 0)
    shadow_mask.paste(mask, (offset, offset + 12))
    shadow.putalpha(shadow_mask.filter(ImageFilter.GaussianBlur(18)).point(lambda v: v * 0.22))
    canvas = Image.alpha_composite(canvas, shadow)

    gradient = Image.new("RGBA", (TILE, TILE))
    for y in range(TILE):
        t = y / (TILE - 1)
        colour = tuple(round(TOP[i] + (BOTTOM[i] - TOP[i]) * t) for i in range(3))
        ImageDraw.Draw(gradient).line([(0, y), (TILE, y)], fill=(*colour, 255))
    tile = Image.new("RGBA", (TILE, TILE), (0, 0, 0, 0))
    tile.paste(gradient, (0, 0), mask)
    ImageDraw.Draw(tile).rounded_rectangle((0, 0, TILE - 1, TILE - 1), RADIUS, outline=(*EDGE, 255), width=3)

    mark = master.crop(master.getbbox())
    height = 600
    mark = mark.resize((round(mark.width * height / mark.height), height), Image.LANCZOS)
    tile.alpha_composite(mark, ((TILE - mark.width) // 2, (TILE - mark.height) // 2 + 10))

    canvas.alpha_composite(tile, (offset, offset))
    return canvas


def main() -> None:
    mark_dir, icons = Path(sys.argv[1]), Path(sys.argv[2])
    master = Image.open(mark_dir / "stackspirit-mark-master.png").convert("RGBA")
    pixel16 = Image.open(mark_dir / "icon-16.png").convert("RGBA")
    pixel32 = Image.open(mark_dir / "icon-32.png").convert("RGBA")
    big = tile_icon(master)

    def scaled(size: int) -> Image.Image:
        if size == 16:
            return pixel16
        if size == 32:
            return pixel32
        return big.resize((size, size), Image.LANCZOS)

    big.save(icons / "icon.png")
    scaled(512).save(icons / "512x512.png")
    scaled(256).save(icons / "128x128@2x.png")
    scaled(128).save(icons / "128x128.png")
    scaled(32).save(icons / "32x32.png")

    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / "icon.iconset"
        iconset.mkdir()
        for size in (16, 32, 128, 256, 512):
            scaled(size).save(iconset / f"icon_{size}x{size}.png")
            scaled(size * 2).save(iconset / f"icon_{size}x{size}@2x.png")
        if shutil.which("iconutil"):
            subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(icons / "icon.icns")], check=True)
        else:
            print("iconutil not found (macOS only); icon.icns left unchanged")

    # Pillow writes each requested size from the largest image; keep the pixel
    # icons for 16 and 32 by passing them as extra frames.
    scaled(256).save(
        icons / "icon.ico",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
        append_images=[pixel16, pixel32],
    )
    print("icons written to", icons)


if __name__ == "__main__":
    main()
