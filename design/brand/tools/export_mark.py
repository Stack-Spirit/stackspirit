"""Export the traced mark grid as SVG and PNG files.

Usage: python3 -I export_mark.py MARK_DIR

Reads MARK_DIR/grid.json and writes:
  stackspirit-mark.svg       full mark, one <rect> per horizontal colour run
  stackspirit-mark-1024.png  square, transparent, nearest-neighbour scaled
  icon-32.json / icon-16.json and matching .svg/.png: reduced grids made by
  pooling cells, where the outline and eye colours win ties so the features
  survive the reduction. These are a starting point for a hand-tuned icon.
"""

import json
import sys
from collections import Counter
from pathlib import Path

from PIL import Image

PRIORITY = ["e", "w", "o"]  # features that must survive pooling


def load(path: Path) -> dict:
    return json.loads(path.read_text())


def square(rows: list[str], bg: str, size: int) -> list[str]:
    """Centre the grid on a size x size canvas."""
    h, w = len(rows), len(rows[0])
    top, left = (size - h) // 2, (size - w) // 2
    out = [bg * size for _ in range(size)]
    for r, line in enumerate(rows):
        out[top + r] = bg * left + line + bg * (size - w - left)
    return out


def pool(rows: list[str], bg: str, factor: int) -> list[str]:
    n = len(rows) // factor
    out = []
    for r in range(n):
        line = []
        for c in range(n):
            block = [rows[r * factor + y][c * factor + x] for y in range(factor) for x in range(factor)]
            counts = Counter(block)
            filled = factor * factor - counts.get(bg, 0)
            if filled < factor * factor * 0.45:
                line.append(bg)
                continue
            pick = None
            for code in PRIORITY:
                # The eye shine needs only a small share; outlines need more.
                need = 0.12 if code == "w" else 0.3
                if counts.get(code, 0) >= factor * factor * need:
                    pick = code
                    break
            if pick is None:
                pick = max((k for k in counts if k != bg), key=lambda k: counts[k])
            line.append(pick)
        out.append("".join(line))
    return out


def svg(rows: list[str], palette: dict, bg: str) -> str:
    h, w = len(rows), len(rows[0])
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" '
        f'width="{w * 8}" height="{h * 8}" shape-rendering="crispEdges">'
    ]
    for r, line in enumerate(rows):
        c = 0
        while c < w:
            ch = line[c]
            run = 1
            while c + run < w and line[c + run] == ch:
                run += 1
            if ch != bg:
                parts.append(f'<rect x="{c}" y="{r}" width="{run}" height="1" fill="{palette[ch]["hex"]}"/>')
            c += run
    parts.append("</svg>")
    return "\n".join(parts) + "\n"


def png(rows: list[str], palette: dict, bg: str, size: int) -> Image.Image:
    h, w = len(rows), len(rows[0])
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    px = img.load()
    for r, line in enumerate(rows):
        for c, ch in enumerate(line):
            if ch != bg:
                hx = palette[ch]["hex"]
                px[c, r] = (int(hx[1:3], 16), int(hx[3:5], 16), int(hx[5:7], 16), 255)
    return img.resize((size, size), Image.NEAREST)


def main() -> None:
    out = Path(sys.argv[1])
    data = load(out / "grid.json")
    rows, palette, bg = data["rows"], data["palette"], data["background"]

    (out / "stackspirit-mark.svg").write_text(svg(rows, palette, bg))
    sq = square(rows, bg, 128)
    png(sq, palette, bg, 1024).save(out / "stackspirit-mark-1024.png")

    for size, factor in ((32, 4), (16, 8)):
        small = pool(sq, bg, factor)
        (out / f"icon-{size}.json").write_text(json.dumps({**data, "rows": small}, indent=1) + "\n")
        (out / f"icon-{size}.svg").write_text(svg(small, palette, bg))
        png(small, palette, bg, size).save(out / f"icon-{size}.png")
        print(f"icon {size}x{size}:")
        print("\n".join(small))


if __name__ == "__main__":
    main()
