"""Trace a generated pixel-art image into an exact palette grid.

Usage:
  python3 -I trace_mark.py IMAGE OUT_DIR [--report]

1. Finds the jittered pixel grid: cut lines are chosen by dynamic programming on
   the edge profile of each axis, with spacing between MIN_STEP and MAX_STEP.
2. Samples each cell with the median of its interior (edges are excluded, so
   anti-aliasing between blocks does not leak in).
3. Snaps every cell to the Meadow palette by nearest colour. Near-black cells
   connected to the image border become transparent; enclosed near-black
   cells stay as the eye colour.
4. Writes grid.json (palette + rows of one-character codes) and a preview PNG.
"""

import json
import sys
from collections import Counter, deque
from pathlib import Path

import numpy as np
from PIL import Image

MIN_STEP, MAX_STEP, TARGET = 6, 11, 8.0

# Codes are single characters so the grid stays readable as text.
PALETTE = {
    "o": ("outline", "#123321"),
    "d": ("deep", "#1E5B36"),
    "s": ("shadow", "#2E7F48"),
    "m": ("main", "#43A85A"),
    "l": ("light", "#7CCB6B"),
    "p": ("pale", "#CDEDB0"),
    "e": ("eye", "#0A0E0B"),
    "w": ("white", "#FFFFFF"),
}
BACKGROUND = "."


def hex_rgb(value: str) -> np.ndarray:
    return np.array([int(value[i : i + 2], 16) for i in (1, 3, 5)], dtype=np.float64)


def cuts(profile: np.ndarray) -> list[int]:
    """Cut positions (block starts) maximising edge strength, spacing-limited."""
    n = len(profile) + 1
    score = np.full(n, -np.inf)
    back = np.zeros(n, dtype=int)
    score[0] = 0.0
    for i in range(1, n):
        for step in range(MIN_STEP, MAX_STEP + 1):
            j = i - step
            if j < 0 or score[j] == -np.inf:
                continue
            # Reward strong edges, lightly penalise drifting from the target size.
            edge = profile[i - 1] if i - 1 < len(profile) else 0.0
            value = score[j] + edge - 40.0 * abs(step - TARGET)
            if value > score[i]:
                score[i] = value
                back[i] = j
    # Allow the last block to end anywhere in the last MAX_STEP pixels.
    end = max(range(n - MAX_STEP, n), key=lambda k: score[k])
    out = [end]
    while out[-1] > 0:
        out.append(back[out[-1]])
    return sorted(out)


def main() -> None:
    src, out_dir = Path(sys.argv[1]), Path(sys.argv[2])
    out_dir.mkdir(parents=True, exist_ok=True)
    img = np.asarray(Image.open(src).convert("RGB")).astype(np.int32)
    h, w, _ = img.shape

    prof_x = np.abs(np.diff(img, axis=1)).sum(axis=2).sum(axis=0).astype(np.float64)
    prof_y = np.abs(np.diff(img, axis=0)).sum(axis=2).sum(axis=1).astype(np.float64)
    prof_x /= prof_x.mean()
    prof_y /= prof_y.mean()
    xs, ys = cuts(prof_x), cuts(prof_y)
    xs = [0] + [c for c in xs if 0 < c < w] + [w]
    ys = [0] + [c for c in ys if 0 < c < h] + [h]
    cols, rows = len(xs) - 1, len(ys) - 1

    cells = np.zeros((rows, cols, 3))
    for r in range(rows):
        for c in range(cols):
            y0, y1, x0, x1 = ys[r], ys[r + 1], xs[c], xs[c + 1]
            inner = img[y0 + 2 : max(y0 + 3, y1 - 2), x0 + 2 : max(x0 + 3, x1 - 2)]
            cells[r, c] = np.median(inner.reshape(-1, 3), axis=0)

    # Snap to the Meadow palette first, then measure the colour the image really
    # uses for each code (median of its cells) and re-snap to those, a few
    # rounds. The result keeps the same 8 roles but with the image's exact
    # shades, so the code version matches the image instead of the brand hexes.
    codes = list(PALETTE)
    colours = np.stack([hex_rgb(PALETTE[k][1]) for k in codes])
    for _ in range(4):
        dist = ((cells[:, :, None, :] - colours[None, None]) ** 2).sum(axis=3)
        assign = dist.argmin(axis=2)
        for k in range(len(codes)):
            members = cells[assign == k]
            if len(members):
                colours[k] = np.median(members, axis=0)
    dist = ((cells[:, :, None, :] - colours[None, None]) ** 2).sum(axis=3)
    measured = {k: "#%02X%02X%02X" % tuple(int(round(v)) for v in colours[i]) for i, k in enumerate(codes)}

    # Final pick per cell: snap every pixel in the cell to the measured palette
    # and take the majority, which maximises the share of matching pixels. Where
    # the image drifted half a block, a cell is split between two colours;
    # averaging would give an in-between shade that snaps to a third colour,
    # while the majority keeps the colour that covers most of the cell.
    grid = np.empty((rows, cols), dtype=object)
    for r in range(rows):
        for c in range(cols):
            y0, y1, x0, x1 = ys[r], ys[r + 1], xs[c], xs[c + 1]
            px = img[y0:y1, x0:x1].reshape(-1, 3)
            snapped = ((px[:, None, :] - colours[None]) ** 2).sum(axis=2).argmin(axis=1)
            grid[r, c] = codes[np.bincount(snapped, minlength=len(codes)).argmax()]

    # Cells matching the corner background colour and reachable from the border are
    # background. The outline and the eyes are dark too, so darkness alone is not
    # enough: match the sampled background colour closely instead.
    corners = np.stack([cells[0, 0], cells[0, -1], cells[-1, 0], cells[-1, -1]])
    bg = np.median(corners, axis=0)
    dark = np.sqrt(((cells - bg) ** 2).sum(axis=2)) < 14
    seen = np.zeros_like(dark)
    queue = deque((r, c) for r in range(rows) for c in range(cols)
                  if (r in (0, rows - 1) or c in (0, cols - 1)) and dark[r, c])
    for r, c in queue:
        seen[r, c] = True
    while queue:
        r, c = queue.popleft()
        grid[r, c] = BACKGROUND
        for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            rr, cc = r + dr, c + dc
            if 0 <= rr < rows and 0 <= cc < cols and dark[rr, cc] and not seen[rr, cc]:
                seen[rr, cc] = True
                queue.append((rr, cc))

    # Trim empty margins.
    filled = grid != BACKGROUND
    rs, cs = np.where(filled.any(axis=1))[0], np.where(filled.any(axis=0))[0]
    grid = grid[rs[0] : rs[-1] + 1, cs[0] : cs[-1] + 1]

    lines = ["".join(row) for row in grid]
    payload = {
        "source": src.name,
        "grid": {"rows": len(lines), "cols": len(lines[0])},
        # Where the trimmed grid sits in the source: cell offset and pixel cuts,
        # so a viewer can lay the trace exactly over the source image.
        "source_offset": {"row": int(rs[0]), "col": int(cs[0])},
        "source_cuts": {"x": [int(v) for v in xs], "y": [int(v) for v in ys]},
        # hex is measured from the image; brand_hex is the Meadow token it maps to.
        "palette": {k: {"role": v[0], "hex": measured[k], "brand_hex": v[1]} for k, v in PALETTE.items()},
        "background": BACKGROUND,
        "rows": lines,
    }
    (out_dir / "grid.json").write_text(json.dumps(payload, indent=1) + "\n")

    scale = 8
    preview = Image.new("RGBA", (len(lines[0]) * scale, len(lines) * scale), (0, 0, 0, 0))
    px = preview.load()
    for r, line in enumerate(lines):
        for c, ch in enumerate(line):
            if ch == BACKGROUND:
                continue
            rgb = tuple(int(v) for v in hex_rgb(measured[ch]))
            for y in range(r * scale, (r + 1) * scale):
                for x in range(c * scale, (c + 1) * scale):
                    px[x, y] = (*rgb, 255)
    preview.save(out_dir / "trace-preview.png")

    print(f"grid {cols}x{rows} -> trimmed {len(lines[0])}x{len(lines)}")
    if "--report" in sys.argv:
        print("steps x", Counter(np.diff(xs)).most_common(5))
        print("steps y", Counter(np.diff(ys)).most_common(5))
        print("codes", Counter("".join(lines)).most_common())
        print("measured", {k: f"{measured[k]} (brand {PALETTE[k][1]})" for k in codes})
        err = np.sqrt(dist.min(axis=2)).mean()
        print(f"mean snap error {err:.1f} (RGB distance)")


if __name__ == "__main__":
    main()
