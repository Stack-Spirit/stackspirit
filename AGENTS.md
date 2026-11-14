# StackSpirit Worker Instructions

1. **Desktop First:** Build the Rust core (`crates/spirit-core`) and the Tauri 2 + React/TypeScript
   desktop application (`apps/spirit-desktop`) before standalone Web or CLI surfaces. Keep all UI
   calls behind the `SpiritBridge` interface so Web can reuse the UI later, and keep all host/SSH/
   VOWP logic in `crates/spirit-core` so the CLI can reuse it later.
2. **Cross-Project VOWP Reliance:** `/home/dev/vowp` is the companion VOWP framework repository.
   - You may read `/home/dev/vowp/docs/design/stackspirit.md`,
     `/home/dev/vowp/docs/design/v0.4/local-orchestrator.md`,
     `/home/dev/vowp/docs/design/v0.4/asif-integration.md`, and
     `/home/dev/vowp/crates/vowp-protocol/src/` (read-only) to match exact VOWP JSON types.
   - Never edit `/home/dev/vowp` from a `stackspirit` job.
   - Respect VOWP milestone gates: Phases D0–D4 (core, desktop shell, host connect/disconnect,
     visual worker/capacity/storage dashboard, and local StackSpirit Engine / `opencode` sidecar
     setup) can be built and tested now using VOWP's current snapshot/CLI JSON contracts and
     deterministic fixtures. Live local-orchestrator execution is gated on VOWP Phase 3, and live
     `.asif.zip` export/import is gated on VOWP `v4-asif-*`; represent those gates explicitly in
     code and UI (`LocalOrchestratorGate`, `AsifGate`) rather than faking completion.
3. **No Secret Leaks:** Never read, print, log, or commit any `.env` file, API key, or token.
   Provider API keys belong only in the local OS keychain trait; the VPS only ever receives
   provider/model names.
4. **Never Push:** Commit on your job branch (`job/<id>`) and write the VOWP report at
   `.vowp/jobs/<id>/report.md` with required front-matter (`job`, `status`, `branch`, `commits`,
   `tests`). The supervisor merge queue merges onto local `main`.
