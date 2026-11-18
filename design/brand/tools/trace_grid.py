"""Find the pixel-art grid in a generated image.

Usage: python3 -I trace_grid.py IMAGE

Prints the best block size and offset for each axis. The generated image is
"fake" pixel art: blocks are roughly square but not perfectly aligned, so the
period is searched in fractional steps.
"""

import sys

import numpy as np
from PIL import Image


def edge_profile(img: np.ndarray, axis: int) -> np.ndarray:
    diff = np.abs(np.diff(img.astype(np.int32), axis=axis)).sum(axis=2)
    return diff.sum(axis=1 - axis).astype(np.float64)


def best_grid(profile: np.ndarray) -> tuple[float, float, float]:
    n = len(profile)
    best = (0.0, 0.0, -1.0)
    for period in np.arange(5.0, 24.0, 0.02):
        for offset in np.arange(0.0, period, 0.25):
            pos = np.arange(offset, n - 1, period)
            score = profile[np.round(pos).astype(int)].mean()
            if score > best[2]:
                best = (float(period), float(offset), float(score))
    return best


def main() -> None:
    img = np.asarray(Image.open(sys.argv[1]).convert("RGB"))
    for axis, name in ((1, "x"), (0, "y")):
        period, offset, score = best_grid(edge_profile(img, axis))
        print(f"{name}: period={period:.2f} offset={offset:.2f} score={score:.0f} cells={1024 / period:.1f}")


if __name__ == "__main__":
    main()
