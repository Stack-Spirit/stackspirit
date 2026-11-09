# VOWP — Virtual Open Work-to-Production

VOWP is an open-source (MIT), local-first supervisor for AI coding agents. It runs the harnesses you
already use — Claude Code, opencode and other stock command-line agents — as a small engineering team on
your own machines: a laptop, a VPS, or both. Work is planned as a graph of jobs, deterministic loops keep
it moving, every job runs in a tmux window you can watch, sessions move between harnesses, models and
machines without losing context, and finished work reaches production only through guarded, reversible
promotions.

VOWP does not fork or spoof a harness. It drives the official headless/JSON interfaces, keeps the
conversation in its own append-only event log, and stores no credentials. Model choices are settings in a
profile ladder, not assumptions baked into a job.

> **Status: current (pre-release).** The v0.1 CLI is complete: every command in the user
> documentation is implemented. The build-from-source commands below work today. For a runnable
> project, see the [hello-vowp example](examples/hello-vowp/README.md); for an unattended
> per-user install with the systemd unit and watchdog, see the
> [install scripts](scripts/README.md). The design lives in [`docs/design/`](docs/design/README.md).

## Why VOWP

- **Stock harnesses, no forks.** Claude Code and opencode are driven through their documented headless
  modes. VOWP never scrapes a TUI, never sends keystrokes to answer a dialog, and never reads or forwards
  login tokens.
- **The database is the truth for decisions; files are the truth for content.** Jobs, gates, runs and
  costs live in one SQLite database per node. Briefs, reports, session logs and blobs stay in plain files.
  Kill everything, restart, and the supervisor continues where it was.
- **Deterministic loops, LLM judgment.** A plain program ticks every 60 seconds and never "decides to
  stop". Models are used for judgment: planning, diagnosing a stall, reviewing a report, writing a
  handoff.
- **Any model, any machine.** The same job can run on Claude Code on a laptop or on DeepSeek on a VPS;
  a profile ladder decides the order.
- **Evidence over claims.** A job is done when its gates pass with recorded evidence — commands, exit
  codes, diffs, verifier findings — not when an agent says so.
- **Guarded, reversible Work → Production.** Promotions snapshot what is live, ship only the intended
  change, record a rollback command and require a human yes by default.
- **Humans are never a bottleneck for unrelated work.** A question parks one job and lands in an inbox;
  everything else keeps running.

## How it works

```
you ──► vowp CLI ──► SQLite + session logs (~/.vowp)
                         ▲
                 supervisor (vowp up)
                   ├─ L1 tick every 60 s: reconcile, dispatch, gates, merge
                   ├─ planner / keeper sessions for judgment
                   ├─ failover, inbox, schedules, watchdog
                   └─ tmux windows: board, supervisor, planner, keeper, job-*, inbox
                         │
                         ▼
              harness child processes: claude -p … / opencode run … / generic cli
```

1. **Plan the graph.** Jobs are added with `vowp job add` (or by the planner from `vowp plan`).
   Each job has a brief, owned paths, dependencies and gates.
2. **Dispatch.** L1 picks ready jobs by priority and ownership locks, creates a git worktree and branch
   `job/<id>`, opens a tmux window and starts the worker session.
3. **Work and report.** The worker changes only its owned paths, commits, runs its gates and writes a
   report with structured front-matter.
4. **Gate and verify.** Command gates record evidence; an independent verifier reads the brief, the diff
   and the evidence. Failures create bounded fix jobs.
5. **Merge.** The single merge queue rebases the job branch, runs the repository-wide checks, scans the
   diff for secrets and fast-forwards `main`.
6. **Promote.** A verified change reaches `work` or `prod` through guards, a serving snapshot, a diff of
   what is actually live, a human approval (by default), deploy and probe — with a recorded rollback.

## Quickstart (from source)

Prerequisites: the pinned Rust toolchain from [`rust-toolchain.toml`](rust-toolchain.toml), `git`,
`tmux` for the window manager, and a harness such as Claude Code or opencode, installed and signed in.
Install `tmux` with your operating system's package manager; the
[install guide](docs/guides/install.md#prerequisites) covers harness setup and every other requirement.

```sh
cargo build --workspace
cargo run -p vowp-cli --bin vowp -- --version
```

The last command prints `vowp 0.1.0-dev`. To put the binary on your `PATH`:

```sh
cargo install --path crates/vowp-cli
vowp --version
```

The first project flow starts with:

```sh
vowp init --name hello-vowp
vowp doctor

# start the supervisor and check it
vowp up
vowp status
```

See the [quickstart guide](docs/guides/quickstart.md) for the full walkthrough, the
[hello-vowp example](examples/hello-vowp/README.md) for a runnable three-job graph with fake adapters,
and the [install guide](docs/guides/install.md) for harness, node and script prerequisites.

## Documentation

Both sections have an index: [concepts](docs/concepts/README.md) and [guides](docs/guides/README.md).

| Start here | What it covers |
|---|---|
| [Concepts overview](docs/concepts/overview.md) | The vocabulary, the moving parts and where state lives. |
| [Graph and loops](docs/concepts/graph-and-loops.md) | Jobs, edges, states, gates, the L1–L9 loops and the merge queue. |
| [Sessions and switching](docs/concepts/sessions-and-switching.md) | Event logs, capsules, executors, switch and export/import. |
| [Providers and failover](docs/concepts/providers-and-failover.md) | Profiles, limits, failover, probe-back and cost receipts. |
| [Work → Production](docs/concepts/work-to-production.md) | Environments, guards, approval, deploy, probe and rollback. |

| Guides | What it covers |
|---|---|
| [Install](docs/guides/install.md) | Build from source, harness prerequisites, `PATH` install. |
| [Quickstart](docs/guides/quickstart.md) | Init → doctor → first job → supervisor → inbox → down. |
| [Writing jobs](docs/guides/jobs.md) | Briefs, reports, gates, ownership and the verify/fix loop. |
| [Operating a node](docs/guides/operating.md) | `up`/`down`, status, logs, loops, backups, reindex, watchdog. |
| [Sessions](docs/guides/sessions.md) | Chat, switch, capsule, export/import and `session watch`. |
| [Multiple nodes](docs/guides/multi-node.md) | Node registration, `--node`, SSH and cross-node sessions. |
| [Schedules](docs/guides/schedules.md) | Cron and interval schedules, missed runs, pause/resume. |

Also: [security model](docs/security.md), [troubleshooting](docs/troubleshooting.md), the
[design spec](docs/design/README.md), [contributing](CONTRIBUTING.md) and the
[security policy](SECURITY.md).

## Repository layout

| Path | Contents |
|---|---|
| `crates/vowp-protocol` | Shared types, adapter/service traits, config, `Clock`, JSON Schema export. |
| `crates/vowp-store` | SQLite schema and migrations, JSONL session logs, blobs, reindex, backup. |
| `crates/vowp-graph` | DAG, state machine, ownership overlap, dispatch planning, rendering. |
| `crates/vowp-tmux` | Safe tmux window manager and its in-memory fake. |
| `crates/vowp-adapters` | Claude Code, opencode and generic CLI adapters; classification. |
| `crates/vowp-core` | Sessions, loops, gates, ledger, schedules, promotion. |
| `crates/vowp-cli` | The `vowp` binary. |
| `framework/` | Templates, skills and agent presets installed by `vowp init`. |
| `examples/` | Runnable examples, including the three-job `hello-vowp` graph. |
| `scripts/` | Per-user install, systemd unit, watchdog, uninstall and their tests. |
| `docs/design/` | The v0.1 implementation spec. |

## License

MIT. See [`LICENSE`](LICENSE).
