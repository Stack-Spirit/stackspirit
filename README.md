# StackSpirit

**StackSpirit** is the free, MIT-licensed home for your AI coding agents: bring your own VPS, spare
laptop or home server, and work keeps running after your laptop closes. The software is free to
run; you pay only for the machine you choose, and [`docs/STRATEGY.md`](docs/STRATEGY.md) records the
vision, brand rules and roadmap.

## Install

StackSpirit 0.6.1 is released on GitHub, npm and PyPI for Linux and macOS on x86_64 and arm64.
Windows and musl/Alpine are not shipped targets; use WSL 2 on Windows, or build from source.

### Run from npm or PyPI

The npm launcher needs Node.js 18 or newer; the wheels need Python 3.9 or newer. Both commands run
the released `stackspirit` CLI without a global install:

```sh
npx -y stackspirit@0.6.1 --version
uvx stackspirit==0.6.1 --version
```

For a permanent install, use `npm install -g stackspirit@0.6.1` or
`pip install stackspirit==0.6.1`. Both put the `stackspirit` CLI, the preserved `vowp` alias,
`stackspirit-daemon` and `vowp-daemon` on `PATH`. If you previously installed the frozen `vowp`
npm package, run `npm uninstall -g vowp` first; both packages provide a global `vowp` command.

### Prebuilt archives

[The v0.6.1 release](https://github.com/Stack-Spirit/stackspirit/releases/tag/v0.6.1) publishes
`stackspirit-0.6.1-<target>.tar.gz` for four native targets, matching `.sha256` files and one
combined `SHA256SUMS`. The legacy `vowp-0.6.1-<target>.tar.gz` archives are published beside them.

| Platform | Architecture | Archive |
|---|---|---|
| Linux (glibc 2.35 or newer) | x86_64 | `stackspirit-0.6.1-x86_64-unknown-linux-gnu.tar.gz` |
| Linux (glibc 2.35 or newer) | arm64 | `stackspirit-0.6.1-aarch64-unknown-linux-gnu.tar.gz` |
| macOS | Intel | `stackspirit-0.6.1-x86_64-apple-darwin.tar.gz` |
| macOS | Apple silicon | `stackspirit-0.6.1-aarch64-apple-darwin.tar.gz` |

Each `stackspirit` archive contains `stackspirit`, `vowp`, `stackspirit-daemon`,
`vowp-daemon`, `LICENSE`, `README.md` and `THIRD-PARTY-LICENSES.md`. Download, verify and check
both CLI names:

```sh
VERSION=0.6.1
TARGET=x86_64-unknown-linux-gnu   # or aarch64-unknown-linux-gnu, x86_64-apple-darwin, aarch64-apple-darwin
BASE=https://github.com/Stack-Spirit/stackspirit/releases/download/v$VERSION

curl -fLO "$BASE/stackspirit-$VERSION-$TARGET.tar.gz"
curl -fLO "$BASE/stackspirit-$VERSION-$TARGET.tar.gz.sha256"
curl -fLO "$BASE/SHA256SUMS"

sha256sum -c "stackspirit-$VERSION-$TARGET.tar.gz.sha256"   # macOS: shasum -a 256 -c ...
tar -xzf "stackspirit-$VERSION-$TARGET.tar.gz"
"./stackspirit-$VERSION-$TARGET/stackspirit" --version       # stackspirit 0.6.1
"./stackspirit-$VERSION-$TARGET/vowp" --version              # vowp 0.6.1
```

Both commands must print `stackspirit 0.6.1` and `vowp 0.6.1`. The macOS archives are not
code-signed or notarized, so Gatekeeper may ask you to confirm the first run.

> **Homebrew is not published yet.** The release workflow renders a formula artifact, but this
> repository does not create or push a tap. There is no supported `brew install` path; use npm,
> PyPI, an archive or a source build.

### Desktop app

0.6.1 does not ship a signed or notarized desktop installer; the desktop is not part of the CLI
archives, npm package or wheels. Build the Tauri 2 app from source with Node 22 and Rust 1.90:

```sh
git clone https://github.com/Stack-Spirit/stackspirit.git
cd stackspirit
npm ci
node scripts/fetch-engine.mjs
(cd apps/spirit-desktop && npx tauri build)   # release bundle
(cd apps/spirit-desktop && npx tauri dev)     # development window
```

`node scripts/fetch-engine.mjs` downloads and checksum-verifies the pinned `opencode` sidecar that
the app bundles and supervises as the StackSpirit Engine. The build is unsigned (no Developer ID
and no notarization), so Gatekeeper may require an explicit first-launch approval if the app
carries a quarantine flag. Linux host packages and the container build are in
[`docs/DEV-SETUP.md`](docs/DEV-SETUP.md).

Read the [0.6.1 release note](runtime/docs/release/0.6.1.md) for the patch changes and
[`SECURITY.md`](SECURITY.md) for the security model and vulnerability-reporting route.

## Surfaces

| Surface | Path | What it is |
|---|---|---|
| Desktop | `apps/spirit-desktop` | Tauri 2 + React/TypeScript cockpit: add a server once, connect over pinned SSH, and see workers, job graphs, capacity, sessions and orchestrator chat. |
| Web (read-only PWA) | `apps/spirit-web` | Mobile-first, read-only cockpit over the daemon HTTP/SSE API. It reuses the shared UI and ships `manifest.webmanifest` and `sw.js`, so it installs as a PWA. |
| Shared UI | `packages/spirit-ui` | `@stackspirit/ui`: design system, Meadow views, store, `SpiritBridge` contract and `MockBridge`. |
| Released CLI, daemons and MCP | `runtime/` | The `stackspirit` CLI built from the separate `runtime/` Cargo workspace, with the `vowp` alias preserved, plus `stackspirit-daemon`, `vowp-daemon` and `stackspirit mcp`. This is the released product CLI. |
| Desktop core | `crates/spirit-core` | Rust host, SSH, VOWP JSON and engine-supervision logic used by the desktop. |
| Developer helper | `crates/spirit-cli` | The thin `spirit` CLI over `crates/spirit-core`: host profiles, fleet, capacity, gates, engine status and fixtures. A developer/testing helper, not the released `stackspirit` CLI. |

The desktop bundles and supervises `opencode` as the StackSpirit Engine, so it can run with
included models or your own provider keys while existing `opencode` and `claude-code` installs stay
supported. Provider keys stay local (a mode-`0600` file) or in the connected server's `vowp secret`
vault; they never reach argv, logs or the assistant's context. See [`SECURITY.md`](SECURITY.md).

The Universal Harness Registry in Settings → Advanced → Harnesses adds one-click connect for the
presets in [`docs/HARNESSES.md`](docs/HARNESSES.md). The orchestrator chat contract, including the
non-streaming setup caveat and the Server/Local gates, is in
[`docs/ORCHESTRATOR-CHAT.md`](docs/ORCHESTRATOR-CHAT.md).

## Develop

Node 22 and Rust 1.90 are the supported toolchains. The desktop needs the pinned engine sidecar
before `tauri dev` or a Tauri bundle.

```sh
npm ci
node scripts/fetch-engine.mjs               # pinned, checksum-verified sidecar
(cd apps/spirit-desktop && npx tauri dev)   # desktop app
cargo run -p spirit-cli -- --help           # developer helper CLI
npm run dev:web                             # read-only Web/PWA shell
```

The desktop runs Meadow (`packages/spirit-ui/src/next`), the only desktop shell: a chat-first
layout with the sprout-face mark, light and dark themes that follow the system, and the server,
workers and details views. In preview mode (`VITE_SPIRIT_MOCK=1`), `?scenario=` switches the
MockBridge data without a restart. Brand sources and the icon builder live in `design/brand`.

The Web shell is read-only and has no browser token. Set `VITE_SPIRIT_MOCK=1` for the shared
`MockBridge` preview, or set the loopback proxy target and browser origin for a live daemon; see
[`docs/WEB.md`](docs/WEB.md). The `spirit` helper's commands, fixture mode and exit codes are in
[`crates/spirit-cli/README.md`](crates/spirit-cli/README.md).

The released runtime is a separate Cargo workspace with its own toolchain, tests and release flow:

```sh
cd runtime
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

See [`runtime/README.md`](runtime/README.md). `engine/manifest.json` pins the sidecar release and
checksum, and `engine/models.json` lists the included models for the pinned build.

### Gates

```sh
cargo fmt --all -- --check
cargo test -p spirit-core --features fixtures
cargo clippy -p spirit-core --all-targets -- -D warnings
cargo test -p spirit-cli
npm run typecheck
npm run lint
npm test
npm run build
npm run test:e2e
bash scripts/verify-engine-checksum.sh --self-test
```

CI (`.github/workflows/ci.yml`) runs the Node UI gates, the root Rust workspace, the `runtime/`
Rust workspace on Linux and macOS, and the runtime npm/PyPI launcher tests. Project gates for the
supervisor live in `stackspirit.toml` (`rust-check`, `rust-core-check`); `vowp.toml` stays for
backwards compatibility until cutover.

## Repository layout

| Path | Role |
|---|---|
| `crates/spirit-core` | Rust core: host profiles, outbound SSH, VOWP JSON parsing, view models and engine supervision. |
| `apps/spirit-desktop` | Tauri 2 shell. `src/main.tsx` creates a `SpiritBridge` and mounts the shared shell; `src/bridge/TauriBridge.ts` is the only renderer file allowed to import `@tauri-apps/api`. |
| `packages/spirit-ui` | `@stackspirit/ui`: the reusable React cockpit (design system, views, store), the `SpiritBridge` contract and `MockBridge`. Exported from TypeScript source, so Vite and Vitest consume it without a separate build step. |
| `apps/spirit-web` | `@stackspirit/web`: read-only Web cockpit over the daemon HTTP/SSE API. It reuses `@stackspirit/ui`, sends no browser token, and shows unavailable daemon data explicitly. |
| `crates/spirit-cli` | Developer helper `spirit` CLI over `spirit-core`: host profiles, fleet, capacity, gates, engine status, JSON output and fixture mode. Not the released `stackspirit` CLI. |
| `runtime/` | Separate Cargo workspace: `stackspirit`/`vowp` CLI, daemons, MCP server and the npm/PyPI packaging. Build and test it with `cd runtime && cargo test --workspace`. |
| `engine/` | Pinned sidecar manifest (`manifest.json`) and included model list (`models.json`). |
| `design/brand` | Logo, palette and icon builders. |
| `scripts/` | Engine fetch and checksum, desktop build container, public-readiness scan and verification scripts. |

The root `npm` workspaces cover `apps/*` and `packages/*`. `npm test` runs the shared UI suite
and then the desktop and Web suites; `npm run test:e2e` runs the desktop and Web Playwright flows.

## Status and limits

The 0.6.1 release ships the desktop cockpit source, the read-only Web PWA source, the
`stackspirit`/`vowp` CLI and daemons, and `stackspirit mcp`. The honest limits that matter:

- **Desktop builds are unsigned.** There is no signed or notarized installer; macOS Gatekeeper may
  ask you to allow the first launch.
- **Server chat remains gated.** Desktop Server `global`/`project` chat returns the typed
  `chat_gated` gate until the owner chooses a transport. The options and recommendation are in
  [`docs/SERVER-CHAT-OPTIONS.md`](docs/SERVER-CHAT-OPTIONS.md).
- **The Mac D6 live check is owner-run.** The local-orchestrator gate stays `Waiting` until the
  owner's Mac agent reports the required version and the owner records the run. D6 actions stay
  disabled until that check passes, and no live desktop smoke is claimed; the checklist is
  [`docs/OWNER-VERIFICATION.md`](docs/OWNER-VERIFICATION.md).
- **Setup chat is non-streaming.** The production transport reports `streaming_supported: false`;
  the final assistant message is real but arrives as one complete message. See
  [`docs/ORCHESTRATOR-CHAT.md`](docs/ORCHESTRATOR-CHAT.md).
- **ASIF 0.4 is a review draft.** Export/import reports declared losses and does not claim ASIF
  conformance, a lossless round trip or portable continuation. See
  [`docs/RELEASE-D5-ASIF.md`](docs/RELEASE-D5-ASIF.md).
- **Windows and musl/Alpine are not shipped targets.** Use WSL 2 on Windows, or build from source.

## Documentation

- [`docs/STRATEGY.md`](docs/STRATEGY.md) — vision, brand rules and roadmap.
- [`docs/HARNESSES.md`](docs/HARNESSES.md) — Universal Harness Registry: presets, protocols,
  auth modes and one-click connect.
- [`docs/ORCHESTRATOR-CHAT.md`](docs/ORCHESTRATOR-CHAT.md) — orchestrator chat contract, the
  non-streaming caveat and the Server/Local gates.
- [`docs/HANDOFF-DESKTOP-UI.md`](docs/HANDOFF-DESKTOP-UI.md) — desktop UI source of truth: owner
  decisions, real versus preview features and next steps.
- [`SECURITY.md`](SECURITY.md) — security model and vulnerability reporting.
- [`runtime/docs/release/0.6.1.md`](runtime/docs/release/0.6.1.md) — the 0.6.1 release note.
- [`docs/PUBLIC-READINESS.md`](docs/PUBLIC-READINESS.md) — public-readiness audit, secret scan
  and release state.
- [`docs/OWNER-VERIFICATION.md`](docs/OWNER-VERIFICATION.md) — owner-run macOS/Linux/Windows
  checklist, evidence template and sign-off rule.
- [`docs/DEV-SETUP.md`](docs/DEV-SETUP.md) — non-root Linux Docker build container for the Tauri
  host and desktop UI gates.
- [`docs/WEB.md`](docs/WEB.md) — Web contract, implementation status and daemon route gaps.
- [`docs/SERVER-CHAT-OPTIONS.md`](docs/SERVER-CHAT-OPTIONS.md) — owner decision note for the
  Server chat transport.
- [`docs/RELEASE-DESKTOP-MVP.md`](docs/RELEASE-DESKTOP-MVP.md),
  [`docs/RELEASE-D5-ASIF.md`](docs/RELEASE-D5-ASIF.md) and
  [`docs/RELEASE-D6.md`](docs/RELEASE-D6.md) — desktop MVP, ASIF and local-orchestrator closeout
  notes.
- [`runtime/README.md`](runtime/README.md) — runtime CLI, daemon, MCP and packaging.
- [`crates/spirit-cli/README.md`](crates/spirit-cli/README.md) — developer helper `spirit`
  commands, storage, fixture mode and exit codes.

## Contributing

Contributions are welcome through GitHub issues and pull requests. Run the gates in
[Develop](#develop) before opening a pull request. Start with [`AGENTS.md`](AGENTS.md) for the
repository's worker contract and [`docs/DEV-SETUP.md`](docs/DEV-SETUP.md) for build environments.

Never include a secret, a real hostname or IP address, or a personal path in an issue, commit,
fixture or report. Refer to secrets by name only and use placeholders such as `/home/user`. Report
suspected vulnerabilities privately as described in [`SECURITY.md`](SECURITY.md).

## License

StackSpirit is released under the [MIT License](LICENSE). The bundled StackSpirit Engine and other
third-party components are covered by [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
