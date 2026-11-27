#!/usr/bin/env python3
"""Build the marketing site's image assets from the committed brand sources.

The site is dependency-free at runtime; this script is only needed when the
brand mark or mascots change. Run it from anywhere:

    python3 apps/site/tools/build-assets.py

Sources of truth: design/brand/mark/ and design/brand/gen/ (see
design/brand/README.md). The generated files are committed under apps/site/assets/.
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

SITE = Path(__file__).resolve().parents[1]
REPO = SITE.parents[1]
BRAND = REPO / "design" / "brand" / "mark"
GEN = REPO / "design" / "brand" / "gen"
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


PAGE = (245, 247, 240)
TINT = (237, 243, 230)
INK = (13, 18, 15)
MUTED = (76, 90, 80)
DEEP = (30, 91, 54)
ACCENT = (37, 107, 60)
SPROUT = (205, 237, 176)
GREEN = (67, 168, 90)


def load_master() -> Image.Image:
    return Image.open(BRAND / "stackspirit-mark-master.png").convert("RGBA")


def resized(mark: Image.Image, size: int) -> Image.Image:
    # The source is pixel art on a power-of-two grid, so NEAREST keeps it crisp.
    return mark.resize((size, size), Image.Resampling.NEAREST)


def cutout_mascot(name: str, width: int) -> Image.Image:
    """Remove the flat black generated background and trim to the character.

    The generated mascots are RGB images with a near-black backdrop. A
    corner flood-fill keeps the dark outline around the character while
    clearing the background, then the alpha bbox trims the empty canvas. A
    small transparent padding keeps the sprite from touching the edge.
    """
    image = Image.open(GEN / name).convert("RGBA")
    for corner in [(0, 0), (image.width - 1, 0), (0, image.height - 1), (image.width - 1, image.height - 1)]:
        ImageDraw.floodfill(image, corner, (0, 0, 0, 0), thresh=24)
    bbox = image.getbbox()
    if bbox is None:
        raise SystemExit(f"{name}: no visible pixels after background removal")
    cut = image.crop(bbox)
    pad = 16
    padded = Image.new("RGBA", (cut.width + pad * 2, cut.height + pad * 2), (0, 0, 0, 0))
    padded.alpha_composite(cut, (pad, pad))
    height = round(width * padded.height / padded.width)
    return padded.resize((width, height), Image.Resampling.NEAREST)


def build() -> None:
    ASSETS.mkdir(parents=True, exist_ok=True)
    mark = load_master()

    # The marketing mark comes from the exact master, not the cropped preview.
    resized(mark, 256).save(ASSETS / "stackspirit-mark-256.png", optimize=True)
    # Kept for the sticky header and favicon lockup.
    resized(mark, 128).save(ASSETS / "stackspirit-mark-128.png", optimize=True)

    # Pixel mascots at web sizes. The walk sprite is deliberately smaller so
    # the scroll path does not ship a large image.
    cutout_mascot("r3-thick.png", 256).save(ASSETS / "mascot-thick-256.png", optimize=True)
    cutout_mascot("r3-thick.png", 128).save(
        ASSETS / "mascot-thick-walk-128.png", optimize=True
    )
    cutout_mascot("r3-gazer.png", 256).save(ASSETS / "mascot-gazer-256.png", optimize=True)

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
    draw.text((x, 142), "STACKSPIRIT  ·  MIT OPEN SOURCE  ·  LOCAL FIRST", font=eyebrow_font, fill=ACCENT)
    draw.text((x, 206), "Manage the always-on", font=title_font, fill=INK)
    draw.text((x, 272), "computers your coding", font=title_font, fill=INK)
    draw.text((x, 338), "agents work on.", font=title_font, fill=DEEP)
    draw.text((x, 446), "stackspirit.io", font=body_font, fill=MUTED)
    draw.text(
        (x, 486),
        "OpenCode included · BYO VPS · Managed Homes",
        font=body_font,
        fill=MUTED,
    )

    image.save(ASSETS / "og-image.png", optimize=True)


if __name__ == "__main__":
    build()
    print(f"assets written to {ASSETS}")
