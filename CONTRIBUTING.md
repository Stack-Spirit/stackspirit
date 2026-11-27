# Contributing to StackSpirit

StackSpirit is a free, MIT-licensed home for AI coding agents. Contributions are
welcome: bug reports, documentation fixes, tests and focused code changes. This guide
covers what lives where, how to build and test it, and what a pull request should
contain.

By contributing, you agree that your contribution is licensed under the
[MIT License](LICENSE). There is no CLA.

## What belongs where

This public repository is the main StackSpirit monorepo. Keep a change in the surface
that owns it:

| Area | Path | What it is |
|---|---|---|
| Desktop app | `apps/spirit-desktop` | Tauri 2 host (`src-tauri`) and the React/TypeScript cockpit (`src`). |
| Web app | `apps/spirit-web` | Read-only, mobile-first Web/PWA cockpit over the daemon HTTP/SSE API. |
| Shared UI | `packages/spirit-ui` | `@stackspirit/ui`: design system, views, store, `SpiritBridge` contract and `MockBridge`. |
| Desktop core | `crates/spirit-core` | Host profiles, SSH, VOWP JSON, engine supervision and view models. |
| Developer helper CLI | `crates/spirit-cli` | The thin `spirit` helper over `spirit-core`; not the released CLI. |
| Released CLI, daemons and MCP | `runtime/` | Separate Cargo workspace: `stackspirit`/`vowp`, daemons, `stackspirit mcp` and npm/PyPI packaging. |
| Marketing site | `apps/site` | The dependency-free static site for <https://stackspirit.io>. |
| Documentation and design | `docs/`, `design/brand` | Product, architecture, release and brand sources. |

The desktop bundles and supervises `opencode` as the StackSpirit Engine. The pinned
sidecar manifest is `engine/manifest.json`; generated sidecars are never committed.
If you are unsure which surface owns a change, open an issue first.

## Development setup

You need:

- **Node.js 22 LTS** and npm 10 or newer (the supported local toolchain).
- **Rust 1.90.0** (or a newer 1.9x) with `rustfmt` and `clippy`. The pinned
  `Cargo.lock` rejects older compilers.
- `git`.
- On Linux, the Tauri system packages below, or Docker for the pinned build container.
- Playwright browsers if you want to run the browser end-to-end tests.

Clone your fork, then install the workspace dependencies and fetch the pinned engine
sidecar:

```sh
npm ci
node scripts/fetch-engine.mjs
```

`node scripts/fetch-engine.mjs` downloads and checksum-verifies the sidecar into the
gitignored `apps/spirit-desktop/src-tauri/binaries/` directory. Run it **before** any
workspace Cargo command or Tauri build: the Tauri build script validates the sidecar
while compiling the host. `scripts/cargo-gate.sh` runs the fetch for you.

### Linux desktop packages

On Debian/Ubuntu, install the Tauri host packages:

```sh
sudo apt-get update
sudo apt-get install -y build-essential curl wget file pkg-config libssl-dev \
  libgtk-3-dev libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev \
  libsoup-3.0-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev patchelf
```

If you cannot install host packages, use the pinned Docker build instead:

```sh
scripts/desktop-build-container.sh check
```

[`docs/DEV-SETUP.md`](docs/DEV-SETUP.md) records the container workflow, its caches and
what it does not prove (a signed bundle, a real OS keychain or a visible window).

### UI, Web and browser gates

From the repository root:

```sh
npm run typecheck
npm run lint
npm test
npm run build
npm run test:e2e
```

`npm run test:e2e` runs the desktop and Web Playwright flows. Install the browser once
with `npx playwright install chromium` if you have not already. Browser mode uses the
`MockBridge`, so it is a regression check, not a real host or keychain test.

### Rust gates

Fetch the sidecar once, then test the desktop core:

```sh
node scripts/fetch-engine.mjs
cargo test -p spirit-core
```

CI runs the same test with the `fixtures` feature and also checks formatting and clippy:

```sh
scripts/cargo-gate.sh fmt --all -- --check
scripts/cargo-gate.sh clippy --workspace --all-targets -- -D warnings
scripts/cargo-gate.sh test -p spirit-core --features fixtures
```

The released runtime is a separate Cargo workspace with its own checks:

```sh
cd runtime
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
```

### Marketing site

The site has no build step. Its static contract is one command:

```sh
node apps/site/scripts/verify.mjs
```

Preview it locally with `node apps/site/scripts/serve.mjs` and open
<http://127.0.0.1:4173>.

### Desktop app

For development, after the sidecar fetch:

```sh
(cd apps/spirit-desktop && npx tauri dev)
```

For a release bundle:

```sh
(cd apps/spirit-desktop && npx tauri build)
```

Desktop bundles are unsigned and not notarized. macOS Gatekeeper may require an
explicit first-launch approval.

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) is the source of truth for the
checks that must pass: the Node UI suite, the `spirit-core` workspace, the runtime on
Linux and macOS, the runtime npm/PyPI packaging tests, and the site contract.

## Pull requests

- Work on a topic branch. Do not push directly to `main` and do not force-push `main`.
- Keep the change focused on one problem. Read anything you need, but change only the
  files the change needs.
- Add a test for a behaviour change: it should fail without the change and pass with
  it. If a test is not practical, explain why in the pull request.
- Update the documentation, `--help` text or screenshots when behaviour or setup
  changes.
- Use conventional commit subjects, as in the existing log: `type(scope): summary`.
  Examples are `docs(readme): ...`, `feat(harnesses): ...` and `fix(chat): ...`. Keep
  each commit focused, and use the body to explain why when the reason is not obvious.
- Do not add secrets or `.env` files. Use placeholders such as `/home/user`, `<host>`
  and `<token>`; never put a real token, key, password, hostname, IP address or
  personal path in code, logs, fixtures or screenshots.
- Make sure the gates above pass. A red build is not ready for review.
- For UI changes, include a before/after screenshot or a short recording, redacted the
  same way as logs.
- Maintainers may rebase, edit or merge the branch while reviewing. You do not need to
  keep it in sync with `main` yourself.
- Do not hand-edit `Cargo.lock` or `package-lock.json`; let Cargo and npm update them
  when a dependency change requires it.

## Security

Do **not** open a public issue or pull request for a vulnerability. Follow
[`SECURITY.md`](SECURITY.md): use the repository's private security-advisory channel
(for example GitHub Security Advisories), or ask the owner for a private channel before
sharing details.

Include the StackSpirit version or commit, the operating system, the VOWP version, the
steps to reproduce and the impact you expect. Refer to secrets by **name** only, and
redact values, tokens, hostnames and paths from logs and screenshots. Reports are
handled on a best-effort basis; there is no bug-bounty programme and no response-time
SLA.

## Community

All project spaces follow [`runtime/CODE_OF_CONDUCT.md`](runtime/CODE_OF_CONDUCT.md).
Report unacceptable behaviour privately through GitHub rather than in a public thread.

## Licence

StackSpirit is released under the [MIT License](LICENSE), and contributions are
accepted under the same terms. There is no CLA. Third-party components and their
licences are listed in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).

## Where to read more

- [`docs/DEV-SETUP.md`](docs/DEV-SETUP.md) — Linux build container and the workspace
  Cargo gates.
- [`docs/HARNESSES.md`](docs/HARNESSES.md) — Universal Harness Registry: presets,
  protocols, auth modes and one-click connect.
- [`docs/ORCHESTRATOR-CHAT.md`](docs/ORCHESTRATOR-CHAT.md) — orchestrator chat contract,
  the non-streaming caveat and the Server/Local gates.
- [`docs/OWNER-VERIFICATION.md`](docs/OWNER-VERIFICATION.md) — owner-run macOS, Linux
  and Windows checklist, evidence template and sign-off rule.
- [`docs/STRATEGY.md`](docs/STRATEGY.md) — company vision, brand rules and roadmap.
