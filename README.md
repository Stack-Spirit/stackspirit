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
2. **Web App Second (`apps/spirit-web`)**: Reuses the `spirit-ui` frontend over an HTTP/SSE bridge.
3. **CLI Third (`crates/spirit-cli`)**: Thin Rust `spirit` CLI over `crates/spirit-core`.

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
npm ci
npm run typecheck
npm run lint
npm test
npm run build
npm run test:e2e
bash scripts/verify-engine-checksum.sh --self-test
```

The macOS desktop build (`npx tauri build --debug`) and the real OS keychain round trip remain
owner verification. The Linux CI worker has no `libgtk-3-dev`, `libglib2.0-dev` or
`libwebkit2gtk-4.1-dev`, so it cannot compile the Tauri host; it still runs every core and
renderer gate.

See [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) for the bundled engine license.
