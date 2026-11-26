# VOWP static site

A small, dependency-free static site for VOWP v0.1. Every page is generated at build time from the
Markdown already in this repository; there is no copied documentation and no network access in the
build.

Sources:

- `README.md` becomes the landing page.
- `docs/concepts/*.md` and `docs/guides/*.md` become one page each.
- Reference pages linked from those docs (security, troubleshooting, design, contributing, and so on)
  are generated too, so every internal link works.

The output is `site/dist/`. It is generated, ignored by git, and never deployed by this repository's
normal checks.

## Build

From the repository root:

```sh
bash site/build.sh
```

Or run the generator directly:

```sh
python3 site/build.py
```

The build needs Python 3 (standard library only) and writes `site/dist/index.html` plus the concept,
guide and reference pages. `python3 site/build.py --out /tmp/vowp-site` writes somewhere else.

## Preview

Open `site/dist/index.html` directly, or serve it locally:

```sh
python3 -m http.server -d site/dist 8000
```

Then visit <http://localhost:8000/>. The site uses only relative links, so it also works from `file://`.

## Test

```sh
python3 -m unittest discover -s site/tests
```

The test builds into a temporary directory and checks that every source page produced HTML, that internal
links resolve, that no external asset URLs are emitted, and that the landing page contains the project
name and the quickstart block.

## Deploying

Deploying is a manual owner step and is deliberately out of scope for this directory. Nothing here
publishes, uploads or contacts a hosting provider. If the owner decides to deploy, build `site/dist/` and
publish that directory with whatever static host they choose.
