# StackSpirit

**StackSpirit** is the desktop-first visual control plane for **VOWP** — add a VPS once, click
**Connect** or **Disconnect**, and see and control your entire agent fleet: worker slots, job
graphs, VPS disk/storage limits, recommended worker capacity, active harnesses/providers, and
sessions.

Under the hood, StackSpirit bundles and supervises `opencode` as the **StackSpirit Engine**, so
users can run immediately with included/free models or their own provider API keys (stored in the
local OS keychain) without needing to install or configure a separate harness—while still supporting
existing `opencode` or `claude-code` installations for power users.

## Surface Order

1. **Desktop App First (`apps/spirit-desktop` + `crates/spirit-core`)**: Tauri 2 (Rust backend) +
   React/TypeScript visual cockpit.
2. **Web App Second (`apps/spirit-web`)**: Reuses the `spirit-ui` frontend over an HTTP/SSE bridge.
3. **CLI Third (`crates/spirit-cli`)**: Thin Rust `spirit` CLI over `crates/spirit-core`.

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
