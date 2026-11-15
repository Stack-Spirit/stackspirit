# StackSpirit

**StackSpirit** is the desktop-first visual control plane for **VOWP** — add a VPS once, click
**Connect** or **Disconnect**, and see and control your entire agent fleet: worker slots, job
graphs, VPS disk/storage limits, recommended worker capacity, active harnesses/providers, and
sessions.

Under the hood, StackSpirit bundles and supervises `opencode` as the **StackSpirit Engine**, so
users can run immediately with included models or their own provider API keys (stored in the
local OS keychain) without needing to install or configure a separate harness—while still
supporting existing `opencode` or `claude-code` installations for power users.

## Surface Order

1. **Desktop App First (`apps/spirit-desktop` + `crates/spirit-core`)**: Tauri 2 (Rust backend) +
   React/TypeScript visual cockpit.
2. **Web App Second (`apps/spirit-web`)**: A read-only Web cockpit over the VOWP daemon HTTP/SSE
   API. It reuses `@stackspirit/ui` and shows unavailable capacity, host, and engine data
   explicitly instead of inventing it.
3. **CLI Third (`crates/spirit-cli`)**: Thin Rust `spirit` CLI over `crates/spirit-core`.

## Quick start

Node 22 and Rust 1.90 are the supported toolchains. The desktop needs the pinned engine sidecar
before `tauri dev` or a Tauri bundle; see [Fetching the sidecar](#fetching-the-sidecar).

```sh
npm ci
node scripts/fetch-engine.mjs               # pinned, checksum-verified sidecar
(cd apps/spirit-desktop && npx tauri dev)   # desktop app
cargo run -p spirit-cli -- --help           # spirit CLI
npm run dev:web                             # read-only Web shell
```

The Web shell is read-only and has no browser token. Set `VITE_SPIRIT_MOCK=1` for the shared
`MockBridge` preview, or set the loopback proxy target and browser origin for a live daemon; see
[`docs/WEB.md`](docs/WEB.md). The CLI quickstart, fixture mode, and exit codes are in
[`crates/spirit-cli/README.md`](crates/spirit-cli/README.md).

## Repository layout

| Path | Role |
|---|---|
| `crates/spirit-core` | Rust core: host profiles, outbound SSH, VOWP JSON parsing, view models, and engine supervision. |
| `apps/spirit-desktop` | Tauri 2 shell. `src/main.tsx` creates a `SpiritBridge` and mounts the shared shell; `src/bridge/TauriBridge.ts` is the only renderer file allowed to import `@tauri-apps/api`. |
| `packages/spirit-ui` | `@stackspirit/ui`: the reusable React cockpit (design-system primitives, views, store), the `SpiritBridge` contract, and `MockBridge`. It is exported from TypeScript source, so Vite and Vitest consume it without a separate build step. |
| `apps/spirit-web` | `@stackspirit/web`: read-only Web cockpit over the VOWP daemon HTTP/SSE API. It reuses `@stackspirit/ui`, sends no browser token, and shows unavailable daemon data explicitly. |
| `crates/spirit-cli` | `spirit` CLI over `spirit-core`: host profiles, fleet, capacity, gates, engine status, JSON output, and fixture mode. |

The root `npm` workspaces cover `apps/*` and `packages/*`. `npm test` runs the shared UI suite
and then the desktop and Web suites; `npm run test:e2e` runs the desktop and Web Playwright flows.

## Documentation

- [`docs/RELEASE-DESKTOP-MVP.md`](docs/RELEASE-DESKTOP-MVP.md) — Desktop MVP release and
  closeout note: scope, evidence, limitations, and owner sign-off status.
- [`docs/DEV-SETUP.md`](docs/DEV-SETUP.md) — non-root Linux Docker build container for the Tauri
  host and desktop UI gates.
- [`docs/OWNER-VERIFICATION.md`](docs/OWNER-VERIFICATION.md) — owner-run macOS/Linux/Windows
  checklist, evidence template, and sign-off rule.
- [`docs/WEB.md`](docs/WEB.md) — W1 Web contract, implementation status, and daemon route gaps.
- [`crates/spirit-cli/README.md`](crates/spirit-cli/README.md) — CLI commands, storage, fixture
  mode, and exit codes.

## StackSpirit Engine (Phase D4)

The desktop ships a pinned `opencode` sidecar as the **StackSpirit Engine** and never asks the user
to install or configure `opencode` in the normal onboarding path.

- **Bundled and verified.** `engine/manifest.json` pins the release, archive SHA-256, source and
  license. `scripts/fetch-engine.mjs` downloads the archive for a target triple, verifies the
  checksum, extracts the sidecar to
  `apps/spirit-desktop/src-tauri/binaries/opencode-<target-triple>`, and records the extracted
  binary digest in a generated lock file next to it. The binary is never committed.
- **Supervised locally.** `spirit-core::engine::ProcessEngineRuntime` starts the sidecar with
  `serve --hostname 127.0.0.1 --port <ephemeral>`, probes `/global/health`, restarts with capped
  backoff, keeps the last 200 redacted log lines, and kills the child on shutdown and app exit.
  The bundled choice is resolved only from Tauri's binary/resource directories, never from `PATH`.
- **Included models are verified.** `engine/models.json` lists the free models generated for the
  pinned build. The view labels a model **Included · no key** only after the running engine reports
  its id. An engine build with no included models shows the BYOK empty state instead.
- **BYOK stays local.** Provider keys go to the OS keychain through the `Keychain`/`SecretStore`
  boundary. Status exposes provider names and states only; keys are zeroized, never logged, never
  written to fixtures or reports, and never sent to the VPS. Verification exercises the key on an
  isolated loopback engine with a minimal model call. Stored keys reach the sidecar only through
  its ephemeral `OPENCODE_AUTH_CONTENT` environment; StackSpirit never writes them into the
  engine's auth file.
- **External harnesses are opt-in.** **Scan PATH** checks only the exact names `opencode`, `claude`
  and `claude-code`, probes `--version`, and requires an explicit **Use this installation**. It
  never edits the external configuration and never falls back silently.
- **First-run is local only.** The selection (bundled/external plus model id) is saved in the app's
  local `engine.json`. It does not write to VOWP. Provider/model names cross the SSH boundary only
  later, when a VOWP profile or local orchestrator is configured.

### Fetching the sidecar

```sh
node scripts/fetch-engine.mjs --target aarch64-apple-darwin
bash scripts/verify-engine-checksum.sh
bash scripts/verify-engine-checksum.sh --self-test
```

`--all` fetches every pinned target; `--force` replaces an existing binary. The fetch host needs
`tar` for `.tar.gz` assets and `unzip` for `.zip` assets. CI should run the fetch and checksum
script before packaging, because Tauri's `bundle.externalBin` expects the sidecar to exist.

### Capability gates

The Engine view ends with two honest VOWP gate cards:

- **Local orchestrator** — gated on VOWP Phase 2 (`v4-local-2e/2f/2g`) and Phase 3
  (`v4-local-3*`: local app host, bridge, MCP proxy, orchestrator materialization, backfill,
  local cache).
- **.asif.zip sessions** — gated on `v4-asif-core` and `v4-asif-cli`.

The probes read VOWP command surfaces and exit codes; a version string alone never makes a gate
Ready. D4 keeps `features.asif_actions` and `features.local_orchestrator_actions` false, so even a
Ready gate shows **not implemented in this build** with the evidence and exact missing milestones
in a Details drawer.

## Cross-Project Relationship with VOWP (`/home/dev/vowp`)

StackSpirit is a separate repository and VOWP project (`stackspirit`), but relies on VOWP wire
contracts and milestones in `/home/dev/vowp`:

- **Available now (Phases D0–D4):** Outbound SSH connection, VOWP snapshot & capacity/storage
  telemetry (`vowp fleet status --json`, `vowp capacity report --json`), visual worker/job lanes,
  recommended worker advisor, job/capacity actions, and local StackSpirit Engine (`opencode`)
  configuration.
- **Gated on VOWP ASIF integration (`v4-asif-*` in `/home/dev/vowp`):** Live `.asif.zip` session
  download and import actions (Phase D5).
- **Gated on VOWP Phase 3 (`v4-local-3*` in `/home/dev/vowp`):** Running global and project
  orchestrators locally on the user's machine with live VOWP session lease/backfill (Phase D6).

## Development gates

```sh
cargo fmt --all -- --check
cargo test -p spirit-core --features fixtures
cargo clippy -p spirit-core --all-targets -- -D warnings
cargo test -p spirit-cli
npm ci
npm run typecheck
npm run lint
npm test
npm run build
npm run test:e2e
bash scripts/verify-engine-checksum.sh --self-test
```

The macOS desktop build (`npx tauri build --debug`), a signed/notarized bundle, and the real OS
keychain round trip remain owner verification. On Linux, `scripts/desktop-build-container.sh
check` compiles and tests the Tauri host in the pinned container without host packages; add
`tauri-build` for a `--no-bundle` host build. See [`docs/DEV-SETUP.md`](docs/DEV-SETUP.md).

See [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) for the bundled engine license.
