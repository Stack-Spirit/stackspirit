# Changelog

All notable changes to VOWP are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows
[Semantic Versioning](https://semver.org/) (pre-release identifiers until 0.1.0).

Release notes with the verification status of each claim are in
[`docs/release/0.1.0-rc.1.md`](docs/release/0.1.0-rc.1.md).

## [Unreleased]

Nothing yet.

## [0.1.0-rc.1] - 2026-11-09 (prepared, not tagged)

First release candidate of VOWP v0.1: a local-first supervisor that runs stock AI coding harnesses (Claude
Code, opencode and generic CLIs) as a team of job workers, with deterministic loops, evidence-based gates,
universal sessions that can move between harnesses, and guarded Work → Production promotion. This entry
describes `main` at commit `006925c` plus the version bump (tested tree: `006925c` + this branch). Nothing has been tagged, published or deployed.

### Added

**Crates** (Rust edition 2024, MIT, workspace version `0.1.0-rc.1`)

- `vowp-protocol`: shared types, events, capsules, config, gate validation (including the gate
  anti-pattern rules), adapter/service traits, `Clock`, JSON Schema export.
- `vowp-store`: SQLite schema and migrations, JSONL session logs, blobs, reindex, backup.
- `vowp-graph`: job DAG, state machine, ownership-overlap checks, dispatch planning, ASCII and Mermaid
  rendering, YAML import/export.
- `vowp-tmux`: safe tmux window manager (no `respawn`, no `send-keys`) and an in-memory fake.
- `vowp-adapters`: Claude Code, opencode and generic CLI adapters over the harnesses' headless/JSON
  interfaces; shared process runner, limit/error classification, secret redaction.
- `vowp-core`: session service, capsule writer and seeding, session switch with bounded drain and
  recovery, session bundle export/import, gates (command, report, review, deploy, secret-scan), cost
  ledger, schedules (cron, interval), the L1 supervisor tick, L2 worker prompts, L3 verify and the merge
  queue, guarded promotion and rollback.
- `vowp-cli`: the `vowp` binary.

**CLI commands** (full `--help` tree: [`docs/reference/cli.md`](docs/reference/cli.md))

- Project and node: `init`, `doctor`, `up` (foreground or `--detach`, stop file, SIGTERM/SIGINT
  cancellation), `down`, `status`, `board`, `attach`, `plan`, `node add|ls|rm`, global `--node` over ssh.
- Jobs and graph: `job add|update|ls|show|retry|park|nudge|restart|replan|fail|cancel`, `graph`
  (ASCII, Mermaid, export, import in dependency order).
- Sessions: `session ls|show|new|chat|switch|capsule|export|import|watch|tail` and the `chat` alias;
  cross-node sessions and reindex recovery.
- Operations: `inbox ls|show|ask|answer|dismiss`, `cost`, `loops` (list), `loops pause|resume`,
  `schedule add loop|command|job-template`, `schedule ls|pause|resume|rm`, `reindex`, `backup`,
  `schema`, `version`.
- Work → Production: `promote` (guards, serving snapshot, candidate/serving diff, approval, deploy,
  probe, recorded rollback command, `--dry-run`), `rollback`, `releases`.

**Adapters**: Claude Code (`claude -p`, stream-json, `rate_limit_event` limit parsing), opencode
(`opencode run --format json`) and a generic CLI adapter driven by a TOML mapping file.

**Loops** (see [`docs/concepts/graph-and-loops.md`](docs/concepts/graph-and-loops.md)): L1 tick every
60 s (reconcile, dispatch, gates, merge), L2 worker start/continue/fix, L3 independent verify with a
bounded fix loop, the single-writer merge queue (rebase, repo-wide checks, secret scan, fast-forward),
L9 schedules.

**Sessions**: append-only event log per session, handoff capsules, `session switch` between harness,
model or node with a bounded drain, seeded first turn and crash recovery; export/import bundles.

**Install and operations**: `scripts/install.sh`, `scripts/uninstall.sh` (both `--dry-run`), a systemd
user unit, a cron-driven heartbeat watchdog, and a hermetic test suite for them.

**Framework**: templates, four role skills (`vowp-worker`, `vowp-planner`, `vowp-verifier`,
`vowp-keeper`) and agent presets installed by `vowp init` under `.claude/skills/`.

**Docs and example**: concepts, guides, security model, troubleshooting, a generated CLI and JSON Schema
reference (`docs/reference/`), the design spec (`docs/design/`), a docs command/link checker
(`docs/tests/`), a static site generator (`site/`), and the runnable `examples/hello-vowp` fake-adapter
demo.

**CI**: `.github/workflows/ci.yml` (test on Linux and macOS) and `release.yml` (a `v*` tag builds four
targets and creates a *draft* GitHub release). Neither workflow has run yet.

### Changed

- Workspace version `0.1.0-dev` → `0.1.0-rc.1`; `vowp --version` prints `vowp 0.1.0-rc.1`.

### Known limitations

- **Failover loops (L4–L8) were not on `main` at `006925c`.** Planner, keeper, automatic limit
  failover/probe-back (L6), inbox answer-file handling (L7) and the watchdog loop (L8) are job
  `n08d-loops-planner-failover`, which had a done report but was still in verification when this entry was
  written; the supervisor's probe step is a placeholder (`L6 probes owned by n08d`). Manual
  `session switch` is shipped; automatic failover is not on `main` until n08d merges.
- **Remote-node probe on switch is skipped.** `session switch` to another node does not run
  `vowp --node <n> doctor --json` (sessions.md §5 step 3); only the local adapter registry is probed
  (`TODO` in `crates/vowp-core/src/session/switch.rs`).
- `session watch`: transcripts are imported on the first scan; `--follow` on a TTY asks for `--once`.
  The Claude Code transcript importer used by the adapter (`claude_code::import_transcript`) is not
  implemented.
- The first turn after a `session switch` passes no caller-chosen native session id
  (`new_native_id: None` in `session/switch.rs`); a fresh session gets a generated uuid-v4.
- Interval schedules align to the Unix epoch, not to the schedule's creation time.
- Time zones: UTC or a fixed offset only; IANA zones are deferred to v0.2.
- `[env.*]` has no `build` key; the promotion build guard runs `[merge].check_cmds`, else
  `[project].check_cmds`, and passes with a recorded note if neither is set.
- `vowp doctor` warns about missing provider API-key variables even when Claude Code uses its own login.
- Several user docs still carry *(planned)* markers for commands that now work; see "Docs follow-ups" in
  the release notes.
- Dogfood (`n14`) and fresh-user (`n15`) runs had not completed when this entry was written; their results
  are not reflected here.
- Verified only on Linux x86_64 in a container. The macOS and aarch64 release targets are untested.
- Nothing is published: no tag, no crates.io release, no npm wrapper (none exists in the repository), no
  deployed site.

[Unreleased]: https://github.com/syver777/vowp/compare/v0.1.0-rc.1...HEAD
[0.1.0-rc.1]: https://github.com/syver777/vowp/releases/tag/v0.1.0-rc.1
