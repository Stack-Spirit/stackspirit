# StackSpirit Worker Instructions

1. **Desktop First:** Build the Rust core (`crates/spirit-core`) and the Tauri 2 + React/TypeScript
   desktop application (`apps/spirit-desktop`) before standalone Web or CLI surfaces. Keep all UI
   calls behind the `SpiritBridge` interface so Web can reuse the UI later, and keep all host/SSH/
   VOWP logic in `crates/spirit-core` so the CLI can reuse it later.
2. **Cross-Project VOWP Reliance:** `/home/user/vowp` is the companion VOWP framework repository.
   - The design source of truth is [`docs/HANDOFF-DESKTOP-UI.md`](docs/HANDOFF-DESKTOP-UI.md) for the
     desktop UI and brand, and [`design/brand/README.md`](design/brand/README.md) for the logo,
     palette and icon builders. There is no older design note to consult.
   - You may read `/home/user/vowp/docs/design/v0.4/local-orchestrator.md`,
     `/home/user/vowp/docs/design/v0.4/asif-integration.md`, `/home/user/vowp/docs/design/cli.md`, and
     `/home/user/vowp/crates/vowp-protocol/src/` (read-only) to match exact VOWP JSON types.
   - Never edit `/home/user/vowp` from a `stackspirit` job.
   - VOWP 0.4.0 shipped the Phase 3 local-orchestrator surface and the ASIF 0.4 interchange, so
     those VOWP-side gates are open. Keep the explicit `LocalOrchestratorGate` and `AsifGate` in
     code and UI until the StackSpirit wiring in `docs/HANDOFF-DESKTOP-UI.md` §5 lands; do not
     claim a feature is live before then.
3. **No Secret Leaks:** Never read, print, log, or commit any `.env` file, API key, or token.
   Provider API keys belong in the local OS keychain trait or on the user's own server through
   VOWP's `vowp secret` vault (`vowp secret set <NAME>` over verified SSH stdin, `ls --json`,
   `rm --yes`; never invent a second custom server secret store — see
   `docs/HANDOFF-DESKTOP-UI.md` §3 and §5). The AI only ever sees secret names/metadata, never
   secret values.
4. **Never Push:** Commit on your job branch (`job/<id>`) and write the VOWP report at
   `.vowp/jobs/<id>/report.md` with required front-matter (`job`, `status`, `branch`, `commits`,
   `tests`). The supervisor merge queue merges onto local `main`.
5. **Desktop UI and brand:** Before changing the desktop UI, the web app's look, the logo, or
   anything in `design/brand`, read `docs/HANDOFF-DESKTOP-UI.md`. It records the owner's
   design, naming, session and security decisions, what is real versus preview, and the
   planned next steps.
6. **Fetch the engine before workspace Cargo:** `apps/spirit-desktop/src-tauri/binaries/` is
   gitignored, so a fresh job worktree lacks the Tauri sidecar and `spirit-desktop`'s build
   script fails before clippy reads any Rust code. Run `node scripts/fetch-engine.mjs` (or let
   `scripts/cargo-gate.sh` do it) before any workspace `cargo` command, or create the job with
   `--gates rust-check` / `rust-core-check`.
