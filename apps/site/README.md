# StackSpirit marketing site

The production static site for <https://stackspirit.io>. It is dependency-free at
runtime: plain HTML, CSS and ES modules, with no build step, no external fonts, no
third-party scripts and no trackers.

The site explains the free MIT software, shows the quickstart for npm, PyPI, MCP and
the desktop app, lists the harnesses StackSpirit works with, and publishes the
managed Always-On Homes catalog.

## Structure

| Path | Purpose |
|---|---|
| `index.html` | The landing page, including the static pricing fallback. |
| `styles.css` | Meadow design tokens, layout and responsive breakpoints. |
| `app.mjs` | Quickstart tabs, copy buttons, catalog fetch and backup toggle. |
| `pricing-data.mjs` | Public plan URL, strict response validation, formatting and the offline fallback. |
| `functions/_middleware.js` | Cloudflare Pages middleware: permanent 301 from the old hosts to the apex. |
| `_headers` | CSP, security headers and cache policy. |
| `404.html` | Static not-found page. |
| `robots.txt`, `sitemap.xml` | Search-engine basics. |
| `assets/` | Local mark, favicons, touch icon and Open Graph image. |
| `tools/build-assets.py` | Rebuilds `assets/` from `design/brand/mark` (Pillow). |
| `scripts/serve.mjs` | Zero-dependency local preview server. |
| `scripts/verify.mjs` | Static contract, middleware, tracker, header and server smoke checks. |
| `scripts/verify-layout.py` | Optional Playwright pass at 390 px and 1280 px. |

## Preview locally

```sh
node apps/site/scripts/serve.mjs
# http://127.0.0.1:4173
```

The server serves `apps/site` as the web root, uses the right MIME type for `.mjs`, and
returns `404.html` with a 404 status for unknown paths.

## Verify

The main gate is dependency-free:

```sh
node apps/site/scripts/verify.mjs
```

It checks the hero promise, all four quickstart tabs and commands, both primary CTAs,
every harness name, the published fallback prices, the off-by-default backup switch,
strict catalog parsing, the host redirects, accessible markup basics, the absence of
tracker code, the CSP and cache headers, image dimensions, internal links, unique
ids, and a real HTTP smoke test through `scripts/serve.mjs`.

The optional browser pass checks responsive layout and interaction. It needs Python
3.10+, Playwright and Chromium:

```sh
python3 -m pip install playwright
python3 -m playwright install chromium
python3 apps/site/scripts/verify-layout.py
```

It loads the site in Chromium at **390 × 844** and **1280 × 800**, asserts there is no
horizontal overflow, checks roles and names, exercises the keyboard tabs, copy
buttons and backup switch, verifies both the offline fallback and a simulated live
catalog, and fails if the page requests anything other than its own origin or
`https://api.stackspirit.io`. Screenshots are written to the system temporary
directory for inspection.

## Managed Always-On Homes

`app.mjs` fetches `https://api.stackspirit.io/v1/plans` with `credentials: "omit"`,
a five-second timeout and no cache. A response is used only after
`parseCatalog()` has validated every field; unknown or malformed responses fall back
to the committed prices from `docs/STRATEGY.md` §6:

- Home 8: €6.60/mo + €7.80 one-time setup
- Home 12: €9/mo
- Home 24: €17/mo
- Home 48: €30/mo
- Home 64: €44/mo
- Off-Site Encrypted R2 Backups: €3.50/mo, optional and **off by default**

The server cannot turn backups on through the catalog. The markup renders the fallback
cards statically, so prices are visible before JavaScript loads and when it is
disabled. There is no free trial copy and no trial UI.

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

## Assets and brand

The mark comes from `design/brand/mark` (see `design/brand/README.md`). To regenerate
the local sizes after a brand change (Python 3 with Pillow):

```sh
python3 apps/site/tools/build-assets.py
```

The script copies the favicons, downsamples the master mark with nearest-neighbour
resampling, builds the 180 px touch icon and renders the 1200 × 630 Open Graph image.

## Privacy and accessibility

- No analytics, advertising, tag managers, session recorders or external requests
  other than the optional public plans fetch.
- The site code writes no cookies, uses no `localStorage`, and never touches
  `document.cookie`.
- One `h1`, landmark structure, a skip link, labelled tab and switch controls, visible
  focus, reduced-motion support and WCAG AA Meadow colour tokens.
- Harness names belong to their respective owners; the site says so.

## License

MIT, like the rest of StackSpirit. See the repository `LICENSE`.
