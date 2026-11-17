"""Hand-drawn small StackSpirit icons at 32x32 and 16x16.

Usage: python3 -I icon_mark.py MARK_DIR

The traced mark (grid.json, 101x124 cells) is too detailed to shrink: pooling
breaks the outline and loses the mouth and sprout, and drawing from shapes blurs
the leaves and pushes the eyes into the outline. So both icons are drawn by hand,
one cell at a time, with proportions measured from the traced grid
(col = 3.4 + x * 0.25, row = 0.5 + y * 0.25 for 32 px):

- two-leaf sprout on a short stem, lit top half, mid-green lower half
- eye bumps with a light top rim; square-ish glossy eyes; one white shine cell
  up and to the right in each eye (same light direction as the trace)
- a flat deadpan dash mouth, 4 cells at 32 px and 2 cells at 16 px
- light from the top left: a light left rim and a small pale glint; shade bands
  to the bottom right (main -> shadow -> deep), outline everywhere

Codes: o outline, d deep, s shadow, m main, l light, p pale, e eye, w white,
"." transparent. Writes icon-32.* and icon-16.* (json, svg, png) into MARK_DIR.
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from export_mark import png, svg  # noqa: E402


def row(margin: int, inner: str, right: int | None = None) -> str:
    """One row: margin dots, outline, inner cells, outline, dots to 32."""
    line = "." * margin + "o" + inner + "o"
    return line + "." * ((right if right is not None else 32) - len(line))


def head_row(margin: int, main: int, shadow: int, deep: int = 0, rim: bool = True, pale: str = "") -> str:
    inner = ("l" if rim else "m") + pale + "m" * (main - 1 - len(pale)) + "s" * shadow + "d" * deep
    return row(margin, inner)


ICON_32 = [
    "." * 32,
    ".........oooo......oooo.........",
    "........ollllo....ollllo........",
    "........ommmmmollommmmmo........",
    ".........oooooolloooooo.........",
    "..............ollo..............",
    ".........oooo.ollo.oooo.........",
    "........olllloolloollllo........",
    ".......oleeeelmllmleeeelo.......",
    ".......omeewemmmmmmeewemo.......",
    ".......omeeeemmmmmmeeeemo.......",
    "......ommeeeemmmmmmeeeemso......",
    head_row(6, 16, 2),
    head_row(5, 17, 3),
    head_row(5, 17, 3, pale="pp"),
    head_row(4, 18, 4, pale="p"),
    row(4, "l" + "m" * 8 + "oooo" + "mmmm" + "sssss"),
    head_row(3, 18, 6),
    head_row(3, 17, 5, 2),
    head_row(3, 16, 6, 2),
    head_row(3, 15, 7, 2),
    head_row(3, 14, 8, 2),
    head_row(4, 12, 7, 3),
    head_row(4, 11, 8, 3),
    head_row(5, 9, 8, 3),
    head_row(5, 8, 9, 3, rim=False),
    head_row(6, 6, 9, 3, rim=False),
    head_row(7, 3, 10, 3, rim=False),
    row(8, "s" * 10 + "dddd"),
    "........." + "oo" + "ssssssdddd" + "oo" + ".........",
    "..........." + "o" * 10 + "...........",
    "." * 32,
]

ICON_16 = [
    "....oo....oo....",
    "...ollo..ollo...",
    "....ooollooo....",
    "...oooolloooo...",
    "..olllmllmlllo..",
    ".oleewmmmmeewlo.",
    ".omeeemmmmeeemo.",
    "olmeeemmmmeeesso",
    "olmmmmmmmmmmmsso",
    "olmmmmmoommmmsso",
    "olmmmmmmmmmmssso",
    "ommmmmmmmmmsssdo",
    ".ommmmmmmmsssdo.",
    "..ommmmmsssddo..",
    "...oossssddoo...",
    ".....oooooo.....",
]


def reshade(rows: list[str], first: int, last: int, light=(12.5, 13.5)) -> list[str]:
    """Recolour the head body as a rounded crescent, like the trace.

    Hand-picked shade counts per row give a hard diagonal. This keeps every
    outline, rim, glint and mouth cell and recomputes only main/shadow/deep from
    the distance to a light point up and to the left, with a one-cell checker
    dither on each band edge.
    """
    out = [list(r) for r in rows]
    lx, ly = light
    for r in range(first, last + 1):
        for c, ch in enumerate(out[r]):
            if ch not in "msd":
                continue
            d = ((c + 0.5 - lx) ** 2 + (r + 0.5 - ly) ** 2) ** 0.5
            checker = (r + c) % 2 == 0
            if d < 11.0 or (d < 11.8 and checker):
                out[r][c] = "m"
            elif d < 15.2 or (d < 16.0 and checker):
                out[r][c] = "s"
            else:
                out[r][c] = "d"
    return ["".join(r) for r in out]


ICON_32 = reshade(ICON_32, 12, 29)


def check(name: str, rows: list[str], n: int) -> None:
    bad = [(i, len(r)) for i, r in enumerate(rows) if len(r) != n]
    if len(rows) != n or bad:
        raise SystemExit(f"{name}: expected {n}x{n}, got {len(rows)} rows; bad rows {bad}")


def main() -> None:
    out = Path(sys.argv[1])
    base = json.loads((out / "grid.json").read_text())
    palette, bg = base["palette"], base["background"]
    for n, rows in ((32, ICON_32), (16, ICON_16)):
        check(f"icon-{n}", rows, n)
        payload = {**base, "source": "hand-drawn", "grid": {"rows": n, "cols": n}, "rows": rows}
        (out / f"icon-{n}.json").write_text(json.dumps(payload, indent=1) + "\n")
        (out / f"icon-{n}.svg").write_text(svg(rows, palette, bg))
        png(rows, palette, bg, n).save(out / f"icon-{n}.png")
        print(f"icon {n}x{n}:")
        print("\n".join(rows))


if __name__ == "__main__":
    main()
