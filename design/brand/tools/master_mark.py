"""Build the exact master artwork from the generated image.

Usage: python3 -I master_mark.py IMAGE MARK_DIR

The clean 8 px grid (grid.json) cannot match the image exactly: the generated
image has soft block edges and a few blocks drawn half a block off its own
grid (the mouth sits between two rows). This keeps the image's own geometry
and only cleans the colours: every pixel is snapped to the 8 colours measured
in grid.json, and the background (connected to the border) becomes transparent.

Writes MARK_DIR/stackspirit-mark-master.png: 1024x1024, 8 colours plus
transparency, saved as an indexed PNG so the colours stay exact.
"""

import json
import sys
from collections import deque
from pathlib import Path

import numpy as np
from PIL import Image


def main() -> None:
    src, out = Path(sys.argv[1]), Path(sys.argv[2])
    grid = json.loads((out / "grid.json").read_text())
    img = np.asarray(Image.open(src).convert("RGB")).astype(np.int32)
    h, w, _ = img.shape

    codes = list(grid["palette"])
    pal = np.array([[int(grid["palette"][k]["hex"][i : i + 2], 16) for i in (1, 3, 5)] for k in codes])
    corners = np.stack([img[4, 4], img[4, w - 5], img[h - 5, 4], img[h - 5, w - 5]])
    bg = np.median(corners, axis=0).astype(np.int32)
    allpal = np.vstack([pal, bg])
    bg_index = len(codes)
    eye_index = codes.index("e")

    snap = np.empty((h, w), dtype=np.int32)
    for y in range(0, h, 64):  # chunked to keep memory small
        block = img[y : y + 64]
        snap[y : y + 64] = ((block[:, :, None, :] - allpal[None, None]) ** 2).sum(axis=3).argmin(axis=2)

    # Background = dark pixels connected to the border. The eyes are the same
    # near-black but enclosed by green, so connectivity keeps them.
    dark = (snap == bg_index) | (snap == eye_index)
    transparent = np.zeros((h, w), dtype=bool)
    queue = deque()
    for x in range(w):
        for y in (0, h - 1):
            if dark[y, x] and not transparent[y, x]:
                transparent[y, x] = True
                queue.append((y, x))
    for y in range(h):
        for x in (0, w - 1):
            if dark[y, x] and not transparent[y, x]:
                transparent[y, x] = True
                queue.append((y, x))
    while queue:
        y, x = queue.popleft()
        for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            yy, xx = y + dy, x + dx
            if 0 <= yy < h and 0 <= xx < w and dark[yy, xx] and not transparent[yy, xx]:
                transparent[yy, xx] = True
                queue.append((yy, xx))
    # Any enclosed background-coloured pixel is part of the eye.
    snap[(snap == bg_index) & ~transparent] = eye_index

    # Indexed PNG: palette entries 0..7 are the mark colours, 8 is transparent.
    indexed = np.where(transparent, bg_index, snap).astype(np.uint8)
    im = Image.fromarray(indexed, mode="P")
    flat = [int(v) for colour in allpal for v in colour]
    im.putpalette(flat + [0, 0, 0] * (256 - len(allpal)))
    im.info["transparency"] = bg_index
    im.save(out / "stackspirit-mark-master.png", transparency=bg_index, optimize=True)

    used = np.bincount(indexed[~transparent].ravel(), minlength=len(codes))
    print("master written:", out / "stackspirit-mark-master.png")
    print("pixels per colour:", {codes[i]: int(used[i]) for i in range(len(codes))})
    print("transparent pixels:", int(transparent.sum()))


if __name__ == "__main__":
    main()
