# StackSpirit brand sheet

Design exploration for the StackSpirit logo and colours (started 2026-11-17).
This folder is committed. The sibling `design/baseline/` (frozen MockBridge
builds of the desktop UI) is regenerated locally and excluded from git.

## Open it

The pages fetch local files, so serve the folder instead of opening it as a file:

```sh
python3 -m http.server 4174 --bind 127.0.0.1 --directory design/brand
```

- <http://localhost:4174/spirit-logos.html>: the full sheet. It has the image-vs-code
  comparison for the chosen mark (wipe, blink, difference map, match numbers),
  both palettes, the six code-drawn spirits, and every generated image.
- <http://localhost:4174/mark.html>: the chosen mark. It shows source vs. master vs. grid,
  the lockup with the wordmark, and the small icons at real size.

In Claude Code the same servers are defined in `.claude/launch.json` (`brand`,
`baseline`, `desktop-mock`).

## Decisions so far

- **Palette:** A · Meadow. Primary `#43A85A`, deep `#1E5B36`, sprout `#CDEDB0`,
  light `#F5F7F0`, dark `#0D120F`. The mark itself uses the shades measured
  from the generated image (`mark/grid.json` → `palette.*.hex`). The Meadow token
  each one maps to is kept as `brand_hex`.
- **Mark:** the deadpan sprout face, `gen/r2-sprout.png`. It was generated with
  `gpt-image-2.6-sunburst` from three references: the deadpan figure for the
  expression, a pixel-art cartoon for the technique, and a frog painting for
  the eyes.
- **Logo file:** `mark/stackspirit-mark-master.png`. This is the image itself with
  every pixel snapped to the 8 mark colours and a transparent background. It
  matches the image to 0.01% of pixels. The clean 8 px grid version
  (`grid.json`, `stackspirit-mark.svg`) is 98% the same but cannot reproduce
  the image's half-block drift, for example in the mouth.
- **Small icons:** `mark/icon-32.*` and `mark/icon-16.*` are hand-drawn. Shrinking
  the big mark does not survive at those sizes.
- **Mascots, not logos:** the thicker full body (`gen/r3-thick.png`) and the
  deadpan Gazer (`gen/r3-gazer.png`).

## Rebuild

```sh
cd design/brand
python3 -I tools/trace_mark.py gen/r2-sprout.png mark    # grid.json + measured palette
python3 -I tools/export_mark.py mark                     # grid SVG/PNG
python3 -I tools/icon_mark.py mark                       # hand-drawn 32/16 px icons
python3 -I tools/master_mark.py gen/r2-sprout.png mark   # exact master PNG
```

Run them in this order; `export_mark.py` writes pooled icons that `icon_mark.py`
then replaces.

Desktop app icons (macOS tile, `.icns`, `.ico` and the PNG sizes Tauri lists):

```sh
python3 -I design/brand/tools/app_icon.py design/brand/mark apps/spirit-desktop/src-tauri/icons
```

Profile / social avatar (full-bleed 1024×1024 Meadow light gradient, circle-crop safe for Gmail, X, GitHub, etc.):

```sh
python3 -I design/brand/tools/avatar_icon.py design/brand/mark
```

The desktop favicons are copies of `mark/icon-32.png` and `mark/icon-16.png` in
`apps/spirit-desktop/public/`. The UI uses `mark/stackspirit-mark-master.png`, copied to
`packages/spirit-ui/src/next/assets/spirit-mark.png`.

## Folder

| Path | What |
|---|---|
| `spirit-logos.html`, `mark.html` | The two pages |
| `gen/` | Generated images: round 1 `a*`/`b*` (no references), round 2 `r2-*`, round 3 `r3-*` |
| `mark/` | Chosen mark: master PNG, grid JSON/SVG/PNG, icons, `avatar-1024.png` (profile/social) |
| `tools/` | Trace, export, icon, master and avatar scripts (Python 3, Pillow, NumPy) |

Image generation cost so far: 11 images at 200,000 North Noir tokens each
(about $4.40).
