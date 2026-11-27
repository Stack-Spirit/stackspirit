# StackSpirit marketing site

The production static site for <https://stackspirit.io>. It is dependency-free at
runtime: plain HTML, CSS and ES modules, with no build step, no external fonts, no
third-party scripts and no CDNs.

The site leads with the free, MIT-licensed path — local mode, your own VPS, the npm
and PyPI launchers, and the desktop app — then offers optional Managed Always-On
Homes underneath. It also carries the privacy policy and terms of service.

## Structure

| Path | Purpose |
|---|---|
| `index.html` | The landing page: adoption-first hero, cockpit preview, harness matrix, static pricing fallback. |
| `styles.css` | Meadow tokens, handmade layout details, the cockpit, legal pages and responsive breakpoints. |
| `app.mjs` | Quickstart tabs, copy buttons, catalog fetch, backup toggle, and the craft initialiser. |
| `craft.mjs` | Pixel-shimmer canvases and the scroll-driven SVG mascot path. |
| `pricing-data.mjs` | Public plan URL, strict response validation, formatting and the offline fallback. |
| `privacy.html` | Privacy policy, served at `/privacy`. |
| `terms.html` | Terms of service, served at `/terms`. |
| `functions/_middleware.js` | Cloudflare Pages middleware: permanent 301 from the old hosts to the apex. |
| `_headers` | CSP, security headers and cache policy. |
| `404.html` | Static not-found page. |
| `robots.txt`, `sitemap.xml` | Search-engine basics, including both legal pages. |
| `assets/` | Local mark, pixel mascots, favicons, touch icon and Open Graph image. |
| `tools/build-assets.py` | Rebuilds `assets/` from `design/brand/` (Pillow). |
| `scripts/serve.mjs` | Zero-dependency local preview server with `/privacy` and `/terms` routes. |
| `scripts/verify.mjs` | Static contract, middleware, craft, legal-page, tracker, header and server smoke checks. |
| `scripts/verify-layout.py` | Optional Playwright pass at 390 px and 1280 px. |

## Design

The palette is the owner-approved **Meadow** set:

| Token | Hex |
|---|---|
| Primary | `#43A85A` |
| Deep | `#1E5B36` |
| Sprout | `#CDEDB0` |
| Light | `#F5F7F0` |
| Dark | `#0D120F` |

The mark and both mascots come from `design/brand/`:

- `assets/stackspirit-mark-256.png` — the exact master mark at a web size.
- `assets/mascot-gazer-256.png` — the deadpan Gazer, used as the peeking character.
- `assets/mascot-thick-256.png` and `assets/mascot-thick-walk-128.png` — the thicker
  full-body mascot; the smaller cutout travels along the scroll path.

`craft.mjs` adds two progressive enhancements:

1. **Pixel shimmer.** A low-resolution canvas draws flat Meadow-colour squares behind
   the hero and each feature card. The hero has a slow ambient field; cards sparkle
   under the pointer. Without JavaScript the canvases stay empty and invisible.
2. **Scroll path.** A curved SVG vine runs down the right side of the page and the
   pixel mascot travels along its geometry as you scroll, flipping to face its
   direction. It is hidden below 1280 px so it never crowds the reading column.

Both honour `prefers-reduced-motion: reduce`: the canvases paint one still field and
the mascot sits at a fixed point with no bob or scroll animation.

The cockpit preview and harness matrix use a dense monospace, hairline-grid treatment
inspired by modern terminal cockpits. It is an illustration, labelled as a rendered
preview — not a live connection. OpenCode is the bundled default engine; other
harnesses are bring-your-own CLI tools running on the Home, and the page says so.

## Preview locally

```sh
node apps/site/scripts/serve.mjs
# http://127.0.0.1:4173
```

The server serves `apps/site` as the web root, maps `/privacy` to `privacy.html` and
`/terms` to `terms.html` the way Cloudflare Pages does, uses the right MIME type for
`.mjs`, and returns `404.html` with a 404 status for unknown paths.

## Verify

The main gate is dependency-free:

```sh
node apps/site/scripts/verify.mjs
```

It checks the exact hero promise and subline, the free install path appearing before
Managed Homes, all four quickstart tabs and commands, the seven pixel-shimmer
canvases, the scroll-path SVG and mascot, every harness in the matrix, the privacy and
terms pages, the footer and sitemap links, the published fallback prices, the
off-by-default backup switch, strict catalog parsing, the host redirects, accessible
markup basics, the absence of tracker code, the CSP and cache headers, image
dimensions, and a real HTTP smoke test through `scripts/serve.mjs`.

The optional browser pass checks responsive layout and interaction. It needs Python
3.10+, Playwright and Chromium:

```sh
python3 -m pip install playwright
python3 -m playwright install chromium
python3 apps/site/scripts/verify-layout.py
```

It loads the site in Chromium at **390 × 844** and **1280 × 800**, asserts there is no
horizontal overflow, checks roles and names, exercises the keyboard tabs, copy buttons
and backup switch, verifies both the offline fallback and a simulated live catalog,
checks the scroll mascot and the legal routes, and fails if the page requests anything
other than its own origin or `https://api.stackspirit.io`. Screenshots are written to
the system temporary directory for inspection.

## Managed Always-On Homes

`app.mjs` fetches `https://api.stackspirit.io/v1/plans` with `credentials: "omit"`, a
five-second timeout and no cache. A response is used only after `parseCatalog()` has
validated every field; unknown or malformed responses fall back to the committed prices
from `docs/STRATEGY.md` §6:

- Home 8: €6.60/mo + €7.80 one-time setup
- Home 12: €9/mo
- Home 24: €17/mo
- Home 48: €30/mo
- Home 64: €44/mo
- Off-Site Encrypted R2 Backups: €3.50/mo, optional and **off by default**

The server cannot turn backups on through the catalog. The markup renders the fallback
cards statically, so prices are visible before JavaScript loads and when it is disabled.
There is no free trial copy and no trial UI.

## Legal pages

`privacy.html` and `terms.html` are plain static documents with no script. They cover
the local-first default, the fact that StackSpirit Cloud does not store SSH private
keys or provider OAuth tokens, the Cloudflare/Stripe/Resend/Contabo subprocessors, the
MIT license, optional Managed Home billing, and the `syver@stackspirit.io` contact.
Cloudflare Pages serves them at `/privacy` and `/terms`; `sitemap.xml` and the landing
footer link to both.

## Deploy to Cloudflare Pages

Nothing in this folder deploys or changes a live Cloudflare resource on its own.
When you deploy, use a Pages project whose root directory is this folder:

| Setting | Value |
|---|---|
| Project | `stackspirit-site` (existing) |
| Root directory | `apps/site` |
| Framework preset | None |
| Build command | *(empty)* |
| Build output directory | `.` |
| Functions directory | `functions` (auto-detected at the project root) |

Using `apps/site` as the Pages project root is what makes `functions/_middleware.js`
available to the deployment. Do not set the output directory to `apps/site` while
leaving the project root at the repository root, or Pages will not find the
middleware. Prefer Cloudflare's Git integration so Pages builds the Functions
directory alongside the static assets.

The middleware keeps the existing redirect contract:

- `https://www.stackspirit.io/*` → `https://stackspirit.io/*` (301, path and query preserved)
- `https://stackspirit-site.pages.dev/*` → `https://stackspirit.io/*` (301, path and query preserved)
- Preview hosts such as `<hash>.stackspirit-site.pages.dev` are left alone so previews work.
- `app.`, `api.`, `relay.` and `mcp.stackspirit.io` are not redirected.
- Project and preview `*.pages.dev` hosts carry `X-Robots-Tag: noindex` so they never
  compete with the canonical domain in search results.

## Assets and brand

The mark comes from `design/brand/mark` (see `design/brand/README.md`), and the mascots
come from `design/brand/gen/r3-thick.png` and `r3-gazer.png`. To regenerate the local
sizes after a brand change (Python 3 with Pillow):

```sh
python3 apps/site/tools/build-assets.py
```

The script copies the favicons, downsamples the master mark with nearest-neighbour
resampling, removes the flat background from both mascots, builds the 180 px touch
icon and renders the 1200 × 630 Open Graph image with the current headline.

## Privacy and accessibility

- No analytics, advertising, tag managers, session recorders or external requests
  other than the optional public plans fetch.
- The site code writes no cookies, uses no `localStorage`, and never touches
  `document.cookie`.
- One `h1`, landmark structure, a skip link, labelled tab and switch controls, visible
  focus, reduced-motion support and WCAG AA Meadow colour tokens.
- The decorative canvases are `aria-hidden`; the mascot path is not focusable and does
  not add to the reading order.
- Harness names belong to their respective owners; the site says so.

## License

MIT, like the rest of StackSpirit. See the repository `LICENSE`.
