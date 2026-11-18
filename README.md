# StackSpirit

**StackSpirit** is the desktop-first visual control plane for **VOWP** — add a VPS once, click
**Connect** or **Disconnect**, and see and control your entire agent fleet: worker slots, job
graphs, VPS disk/storage limits, recommended worker capacity, active harnesses/providers, and
sessions.

Under the hood, StackSpirit bundles and supervises `opencode` as the **StackSpirit Engine**, so
users can run immediately with included models or their own provider API keys (in the local OS
keychain, or optionally in VOWP's vault on the connected server) without needing to install or
configure a separate harness—while still supporting existing `opencode` or `claude-code`
installations for power users.

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

The desktop opens the Meadow shell (`packages/spirit-ui/src/next`): a chat-first layout with
the sprout-face mark, light and dark themes that follow the system, and the server, workers
and details views. The classic cockpit is still available with `VITE_SPIRIT_UI=classic` or
`?ui=classic`, and its e2e specs pin it. In preview mode (`VITE_SPIRIT_MOCK=1`), `?scenario=`
switches the MockBridge data without a restart. Brand sources and the icon builder live in
`design/brand`.

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

- [`docs/HANDOFF-DESKTOP-UI.md`](docs/HANDOFF-DESKTOP-UI.md) — start here for the desktop UI
  redesign: owner decisions (brand, layout, naming, sessions, SSH and secrets), real versus
  preview features, and next steps.

- [`docs/ORCHESTRATOR-CHAT.md`](docs/ORCHESTRATOR-CHAT.md) — the orchestrator chat
  contract: the bundled-engine setup assistant, the honest non-streaming
  (`streaming_supported: false`) caveat, the Server chat and Local/Server switch gates, and
  the owner checklist. The static check and checklist live in
  [`scripts/verify-orchestrator-chat.mjs`](scripts/verify-orchestrator-chat.mjs).

- [`SECURITY.md`](SECURITY.md) — the implemented security model: SSH host-key pinning, the
  app-owned SSH key, one-time-password onboarding, server secrets in VOWP's vault, and the
  honest limits (VOWP `0.4.0` does not encrypt the vault at rest).

- [`docs/RELEASE-DESKTOP-MVP.md`](docs/RELEASE-DESKTOP-MVP.md) — Desktop MVP release and
  closeout note: scope, evidence, limitations, and owner sign-off status.
- [`docs/RELEASE-D5-ASIF.md`](docs/RELEASE-D5-ASIF.md) — D5 ASIF release and closeout note:
  shipped core/bridge/Sessions UI, capability-probe evidence, the local CLI round trip, the
  pending owner-run live smoke, and the `ss-13-asif-core`, `ss-14-asif-bridge`, and
  `ss-15-asif-ui` evidence status.
- [`docs/RELEASE-D6.md`](docs/RELEASE-D6.md) — D6 local-orchestrator release and closeout note:
  shipped core/bridge/Engine UI, the `LocalOrchestratorGate` capability gate, the D6 core,
  bridge, and panel test evidence, and the pending owner-run live setup and smoke.
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
- **BYOK is local by default.** Provider keys go to the OS keychain through the
  `Keychain`/`SecretStore` boundary. Status exposes provider names and states only; keys are
  zeroized, never logged, never written to fixtures or reports, and local keys are never sent
  to the VPS. Verification exercises the key on an isolated loopback engine with a minimal
  model call. Stored keys reach the sidecar only through its ephemeral
  `OPENCODE_AUTH_CONTENT` environment; StackSpirit never writes them into the engine's auth
  file. Provider keys may instead be stored on the connected server in VOWP's own vault
  through Settings → Advanced → **Server secrets** (`ss-35`, `506d9d2`); the value travels
  only over verified SSH stdin after VOWP's hidden prompt, and the AI sees names/metadata
  only. See [`SECURITY.md`](SECURITY.md).
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

- **Local orchestrator** — shipped in D6 for the desktop behind the live
  `LocalOrchestratorGate`; the probe covers the VOWP Phase 2 offload read and the Phase 3/4
  local-node CLI surface.
- **.asif.zip sessions** — shipped in D5 for the desktop. The read-only probe must prove
  export and import independently; a partial Ready gate enables only the proven direction.

The probes read VOWP command surfaces and exit codes; a version string alone never makes a gate
Ready. The desktop build enables `features.asif_actions` (the `d5` Cargo feature is on by
default). D6 is implemented in the core, bridge, and Engine UI: actions enable only when the
published `features.local_orchestrator_actions` flag is on, `LocalOrchestratorGate` is `Ready`,
and the local agent is `connected`, with the node, protocol, and path-map prerequisites
re-checked by the backend. The probes now match the installed `vowp 0.4.0` surface (`ss-32`,
`cd6a975`; live home read `ss-37`, `39d8538`). On this host the live gate remains **Waiting**:
the live home reports an online `macbook` agent card with protocol `vowp-agent/1`, but the
Mac-side `vowp agent status` (`connection: connected`) has not been verified from this host and
the live agent reports `agent_version: 0.3.0`. Live execution remains owner-gated, D6 actions
are not enabled, and no live desktop smoke has been recorded.

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
- **Shipped in D6 but still gated:** VOWP `0.4.0` is installed and ships the Phase 3/4
  local-orchestrator CLI surface; StackSpirit's gate probes and live-home evidence were
  reconciled with it (`ss-32`, `cd6a975`; `ss-37`, `39d8538`). The live `macbook` agent card is
  online with protocol `vowp-agent/1`, while the home node is offline and non-agent. The
  Mac-side `vowp agent status` is still an owner check, so D6 actions remain disabled and the
  owner-run smoke is pending. See [`docs/RELEASE-D6.md`](docs/RELEASE-D6.md) and
  [`docs/D6-READINESS.md`](docs/D6-READINESS.md).

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

See [`SECURITY.md`](SECURITY.md) for the security model and supported versions, and
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) for the bundled engine license.
