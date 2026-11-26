#!/usr/bin/env python3
"""Build the marketing site's image assets from the committed brand sources.

The site is dependency-free at runtime; this script is only needed when the
brand mark changes. Run it from anywhere:

    python3 apps/site/tools/build-assets.py

Source of truth: design/brand/mark/ (see design/brand/README.md).
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

SITE = Path(__file__).resolve().parents[1]
REPO = SITE.parents[1]
BRAND = REPO / "design" / "brand" / "mark"
ASSETS = SITE / "assets"

BOLD_FONTS = [
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "/Library/Fonts/Arial Bold.ttf",
    "C:/Windows/Fonts/arialbd.ttf",
]
REGULAR_FONTS = [
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "C:/Windows/Fonts/arial.ttf",
]


def font(size: int, *, bold: bool) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    for candidate in BOLD_FONTS if bold else REGULAR_FONTS:
        if Path(candidate).is_file():
            return ImageFont.truetype(candidate, size)
    print("warning: no Arial/Liberation/DejaVu font found; using Pillow's default font")
    try:
        return ImageFont.load_default(size=size)
    except TypeError:
        return ImageFont.load_default()

PAGE = (251, 252, 248)
TINT = (241, 244, 236)
INK = (13, 18, 15)
MUTED = (79, 92, 83)
DEEP = (30, 91, 54)
ACCENT = (37, 107, 60)
SPROUT = (205, 237, 176)
GREEN = (67, 168, 90)


def load_mark() -> Image.Image:
    return Image.open(BRAND / "stackspirit-mark-1024.png").convert("RGBA")


def resized(mark: Image.Image, size: int) -> Image.Image:
    # The source is pixel art on a power-of-two grid, so NEAREST keeps it crisp.
    return mark.resize((size, size), Image.Resampling.NEAREST)


def build() -> None:
    ASSETS.mkdir(parents=True, exist_ok=True)
    mark = load_mark()

    resized(mark, 128).save(ASSETS / "stackspirit-mark-128.png", optimize=True)

    # Favicons and touch icon.
    for name in ("icon-16.png", "icon-32.png", "icon-32.svg"):
        (ASSETS / name).write_bytes((BRAND / name).read_bytes())

    touch = Image.new("RGB", (180, 180), TINT)
    touch_mark = resized(mark, 140)
    touch.paste(touch_mark, (20, 20), touch_mark)
    touch.save(ASSETS / "apple-touch-icon.png", optimize=True)

    icon_32 = Image.open(BRAND / "icon-32.png").convert("RGBA")
    icon_16 = Image.open(BRAND / "icon-16.png").convert("RGBA")
    icon_32.save(SITE / "favicon.ico", sizes=[(32, 32), (16, 16)], append_images=[icon_16])

    build_og(mark)


def build_og(mark: Image.Image) -> None:
    width, height = 1200, 630
    image = Image.new("RGB", (width, height), PAGE)
    draw = ImageDraw.Draw(image, "RGBA")

    # Soft Meadow glows; drawn at large radii and clipped by the canvas.
    draw.ellipse((760, -260, 1460, 440), fill=(205, 237, 176, 120))
    draw.ellipse((-240, 400, 360, 1000), fill=(67, 168, 90, 28))
    draw.rectangle((0, height - 12, width, height), fill=DEEP)

    og_mark = resized(mark, 232)
    image.paste(og_mark, (78, 196), og_mark)

    title_font = font(58, bold=True)
    eyebrow_font = font(22, bold=True)
    body_font = font(27, bold=False)

    x = 372
    draw.text((x, 142), "STACKSPIRIT  ·  FREE & MIT OPEN SOURCE", font=eyebrow_font, fill=ACCENT)
    draw.text((x, 206), "An always-on machine", font=title_font, fill=INK)
    draw.text((x, 272), "as a home for your", font=title_font, fill=INK)
    draw.text((x, 338), "AI coding agents", font=title_font, fill=DEEP)
    draw.text((x, 446), "stackspirit.io", font=body_font, fill=MUTED)
    draw.text((x, 486), "Turnkey Always-On Homes · Desktop · Web · CLI · MCP", font=body_font, fill=MUTED)

    image.save(ASSETS / "og-image.png", optimize=True)


if __name__ == "__main__":
    build()
    print(f"assets written to {ASSETS}")
