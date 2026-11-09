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
describes `main` at commit `195d517` plus this documentation refresh (tested tree: `195d517` + this
branch). Nothing has been tagged, published or deployed.

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
  ledger, schedules (cron, interval), the L1 supervisor tick (including the single-writer merge-queue
  step), L2 worker prompts, L3 verify, L4 planner, L5 keeper, L6 automatic limit failover/probe-back,
  L7 inbox answers, L8 watchdog, guarded promotion and rollback.
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
60 s (reconcile, dispatch, gates, and step 7 of the tick — the single-writer merge queue: rebase,
repo-wide checks, secret scan, fast-forward), L2 worker start/continue/fix, L3 independent verify with a
bounded fix loop, L4 planner, L5 keeper (covers planning when the planner has no available rung), L6
automatic limit failover and probe-back, L7 inbox answer files, L8 watchdog and L9 schedules. The merge
queue is part of the L1 tick, not an L3 step.

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

- **L4–L8 are merged but not spec-complete.** Planner (L4), keeper (L5), automatic limit
  failover/probe-back (L6), inbox answer files (L7) and the watchdog (L8) were introduced in n08d at
  `356e185`, merged as `ad163b0` and fixed through `0f78c1d`. Known gaps: keeper takeover/hand-back
  capsules are partial (covering turns are not compacted into a fresh planner capsule); the keeper
  digest tails the last 60 indexed events, not the pretty log; a failed switch during quota failover
  does not append its ledger failure; a second rate event does not extend the backoff; and
  `provider_of_job` can pick the first rung for a job that never ran. The full ranked list is in the
  release notes' defect table.
- **Dogfood passed; six blocking defects remain open.** n14 merged as `195d517` (final run on
  `8f65b81`: both phases, 22.24 s; 5/5 consecutive default runs on `0f78c1d`), and its full evidence is
  in `docs/dogfood/RESULTS.md`. `vowp up --home`, the planner/keeper/inbox windows, the SIGKILL crash
  window, the job brief in the worktree, `down` vs `status --check`/watchdog and `up --no-tmux` remain
  open (D1–D6); a seventh defect (D7) is minor.
- **Remote-node probe on switch is skipped.** `session switch` to another node does not run
  `vowp --node <n> doctor --json` (sessions.md §5 step 3); only the local adapter registry is probed
  (`TODO` in `crates/vowp-core/src/session/switch.rs`).
- `session watch --once` imports Claude Code transcripts; continuous `--follow` is not available in
  this build (a TTY invocation exits 2 asking for `--once`), and the optional adapter transcript
  importer (`vowp_adapters::claude_code`) is not shipped.
- The first turn after a `session switch` passes no caller-chosen native session id
  (`new_native_id: None` in `session/switch.rs`); a fresh session gets a generated uuid-v4.
- Interval schedules align to the Unix epoch, not to the schedule's creation time.
- Time zones: UTC or a fixed offset only; IANA zones are deferred to v0.2.
- `[env.*]` has no `build` key; the promotion build guard runs `[merge].check_cmds`, else
  `[project].check_cmds`, and passes with a recorded note if neither is set.
- `vowp doctor` warns about missing provider API-key variables even when Claude Code uses its own login.
- Docs drift remains on a few pages: the `vowp 0.1.0-dev` version string is still shown in
  `README.md:80`, `CONTRIBUTING.md:21`, `docs/guides/quickstart.md:23` and `docs/guides/install.md:47`;
  the design pages still disagree on the skills path and on which command creates loop rows. See "Docs
  follow-ups" in the release notes.
- Fresh-user test: PASS (`4a5dfd2`; 31 documented commands, 37 s to first success, 188 s total). Its
  D1–D8 deviations were documentation-only and are fixed on `main`, except the harness polish listed in
  the release notes.
- Verified only on Linux x86_64 in a container. The macOS and aarch64 release targets are untested.
- Nothing is published: no tag, no crates.io release, no npm wrapper (none exists in the repository), no
  deployed site. `cargo publish` is not prepared either: `vowp-core` and `vowp-protocol` have no version
  key in `crates/vowp-cli/Cargo.toml`, and no `cargo package` dry run was done.

[Unreleased]: https://github.com/syver777/vowp/compare/v0.1.0-rc.1...HEAD
[0.1.0-rc.1]: https://github.com/syver777/vowp/releases/tag/v0.1.0-rc.1
