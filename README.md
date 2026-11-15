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
- [`docs/RELEASE-D5-ASIF.md`](docs/RELEASE-D5-ASIF.md) — D5 ASIF release and closeout note:
  shipped core/bridge/Sessions UI, capability-probe evidence, the local CLI round trip, the
  pending owner-run live smoke, and the `ss-13-asif-core`, `ss-14-asif-bridge`, and
  `ss-15-asif-ui` evidence status.
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
- **.asif.zip sessions** — shipped in D5 for the desktop. The read-only probe must prove
  export and import independently; a partial Ready gate enables only the proven direction.

The probes read VOWP command surfaces and exit codes; a version string alone never makes a gate
Ready. The D5 desktop build enables `features.asif_actions` (the `d5` Cargo feature is on by
default), while `features.local_orchestrator_actions` stays false until D6. A Ready
local-orchestrator gate therefore still shows **not implemented in this build** with the evidence
and exact missing milestones in a Details drawer.

## Sessions and ASIF (Phase D5)

The desktop ships a read-only **Sessions** drawer from the last VOWP snapshot and two actions
enabled per direction only when the live `AsifGate` proves them:

- **Export .asif.zip** writes a private package on the VOWP host, copies it over SSH after a
  size and byte-count check, removes the remote temp directory, and shows the declared losses,
  unsupported operations, and local file name. The remote path never reaches the renderer.
- **Import .asif.zip** streams a local package to
  `vowp --project <project> --json session import - --format asif` and shows the new session
  id, `idle` state, losses, reindex warnings, and unsupported operations. The imported session
  is never run automatically; there is no force switch or `--allow-unsupported`.

Strict and allow-declared-loss are advanced, mutually exclusive choices; allow-loss is the
default and no result is called lossless. Packages may contain secrets, so an export is not
secret-free by default. See [`docs/RELEASE-D5-ASIF.md`](docs/RELEASE-D5-ASIF.md) for the
capability-probe evidence, the local disposable CLI round trip, the D5 report status, and the
owner-run live smoke that is still pending.

## Cross-Project Relationship with VOWP (`/home/dev/vowp`)

StackSpirit is a separate repository and VOWP project (`stackspirit`), but relies on VOWP wire
contracts and milestones in `/home/dev/vowp`:

- **Available now (Phases D0–D5):** Outbound SSH connection, VOWP snapshot & capacity/storage
  telemetry (`vowp fleet status --json`, `vowp capacity report --json`), visual worker/job lanes,
  recommended worker advisor, job/capacity actions, local StackSpirit Engine (`opencode`)
  configuration, and desktop `.asif.zip` session export/import behind the live ASIF capability
  probe.
- **Shipped in D5 but not owner-smoke-tested:** ASIF 0.4 remains a review draft. Export and
  import are enabled independently only after the read-only probe proves them; see
  [`docs/RELEASE-D5-ASIF.md`](docs/RELEASE-D5-ASIF.md). No live host/desktop smoke has been
  recorded by the owner.
- **Gated on VOWP Phase 2 (`v4-local-2e/2f/2g`) and Phase 3 (`v4-local-3*` in
  `/home/dev/vowp`):** Running global and project orchestrators locally on the user's machine
  with live VOWP session lease/backfill (Phase D6).

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
