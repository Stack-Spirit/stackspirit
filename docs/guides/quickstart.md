# Quickstart

This walkthrough takes one git repository from "nothing installed" to a supervisor that has a job
running, then stops it cleanly. It uses one small documentation job so you can watch the full lifecycle
without changing code.

> **Status: mixed.** `vowp init`, `vowp doctor`, `vowp job` and `vowp graph` work today. The supervisor,
> board, attach, plan, inbox and down commands are marked *(planned)*. The build-from-source steps in
> [install](install.md) work today. The example project `examples/hello-vowp` *(planned)* will offer a
> runnable three-job graph with fake adapters.

## 0. Build and install

Follow [install](install.md). For the short version:

```sh
cargo build --workspace
cargo install --path crates/vowp-cli
vowp --version
```

The last command must print `vowp 0.1.0-dev`. If it does not, stop here and read the install guide.

## 1. Choose a repository

Use a git repository with a remote and a default branch. VOWP creates job worktrees outside the
repository and the merge queue pushes branches; a repository with no remote can run gates but cannot
complete the merge path.

```sh
cd /path/to/my-project
git status
git remote -v
```

The working tree should be clean, and the current branch should be the one you want VOWP to merge into.

## 2. Initialise the project

```sh
vowp init --name my-project
```

On a fresh repository this prints `initialised project my-project at /path (11 files written, 0 skipped)`.
It writes `vowp.toml` with the four default profiles and `schedules = []` (so `init` installs no
schedules), installs the framework files (`AGENTS.md`, `CLAUDE.md`, four skills and `opencode.jsonc`),
creates `.vowp/`, registers the local node and project, and seeds the nine default loop rows as
**active**. Existing framework files are skipped and listed; an already initialised project exits 4
unless you pass `--force`. The exact `vowp.toml` defaults and file list are in the
[install guide](install.md#prepare-a-project-repository). `vowp up` *(planned)* will also ensure the loop
rows on startup.

Read `vowp.toml` before continuing. In particular:

- `[project].check_cmds` is what the merge queue runs on the rebased tree.
- `[profiles.*].ladder` is the model order for each role. Replace the examples with executors your
  machine can actually run.
- `[providers.*].max_concurrent` prevents one provider from being hammered.

## 3. Check the node

```sh
vowp doctor
```

Fix every `fail` before starting the supervisor:

| Check | What to do |
|---|---|
| `home` | Make `~/.vowp` writable, or pass `--home <dir>` consistently. |
| `db` | Move an unsupported or corrupt database aside; `vowp reindex` *(planned)* rebuilds session indexes, not decisions. |
| `git` | Install git and make sure it is on `PATH`. |
| `tmux` | Install tmux, or accept the warning and run `vowp up --no-tmux` *(planned)*. |
| `config` | Fix `vowp.toml`; the error names the key path. |
| `schedules` | Fix the named `[[schedules]]` entry; its cron, interval or timezone did not parse. |
| `harness:claude-code` / `harness:opencode` | Install the harness, or change the profile ladder to one that is installed. |
| `providers` | Set the named environment variable in the shell that starts the supervisor. The value is never printed. |

A missing harness is a `fail`, not a warning: a profile that cannot resolve would only fail later.

## 4. Add a first job

The CLI can create a job directly. The body is the brief; the front-matter (`job`, `owns`, `gates`, …) is
generated from the arguments.

```sh
vowp job add \
  --id hello-docs \
  --title "Write the hello page" \
  --kind docs \
  --owns "docs/hello.md,README.md" \
  --gates "report,owns_respected,secret_scan,review" \
  --brief - <<'BRIEF'
# Goal

Create `docs/hello.md`, a one-page introduction to this project. It must explain what the project does,
how to run it, and where the tests live.

# Read first

- `README.md` and the existing `docs/` pages.
- The project's `AGENTS.md`.

# Deliverables

- `docs/hello.md`, linked from `README.md`.
- A report at `.vowp/jobs/hello-docs/report.md` with the standard front-matter.

# Tests required

- `git diff --check` passes.
- Every relative link in the new page resolves to a file in the repository.

# Out of scope

- Any file outside `docs/hello.md` and `README.md`.
- Code changes.

# Done when

The page exists, the links resolve, and the report has `status: done`.
BRIEF
```

The gate list includes `review`, so the job will be reviewed independently before it can merge. The
worker still runs the brief's checks even though they are not separate `cmd` gates.

## 5. Inspect the graph

```sh
vowp graph
vowp job show hello-docs --gates --runs
```

`vowp graph` prints one line per job with its layer, state and dependencies. `vowp job show` prints the
job and its edges; `--gates --runs` adds its gates and (once it has run) its runs and gate evidence.

If you would rather let an agent propose the graph first:

```sh
# (planned)
vowp plan "Add a hello page that explains the project" --dry-run
```

`--dry-run` prints the planner's proposed YAML and stores nothing. Review it, then run the same command
without `--dry-run` to let the planner create jobs through the CLI.

## 6. Start the supervisor

```sh
# (planned)
vowp up
```

By default this ensures the tmux session `vowp-my-project` with the `board`, `supervisor`, `planner`,
`keeper` and `inbox` windows, then detaches the supervisor. Use `vowp up --foreground` to run the loop in
the current terminal, or `vowp up --once` to run exactly one tick (useful for tests and cron).

Check it:

```sh
# (planned)
vowp status
vowp board
```

`vowp status` shows the supervisor pid, heartbeat age, jobs by state, slots, limits, cost today and the
open inbox count. `vowp board` is the refreshing view. With tmux installed:

```sh
# (planned)
vowp attach hello-docs
```

attaches to the job window; `vowp attach` with no job opens the board window. The job window is a
read-only follower of the session log, so it keeps working across a model switch.

## 7. Watch the lifecycle

A healthy first job moves through:

```
pending → ready → running → gating → reviewing → merging → done
```

- If the worker stops before writing a report, L2 continues it up to twice.
- If a gate fails, a fix job runs in the same worktree; the original re-runs its gates.
- If the verifier fails, a fix job gets the concrete findings and the original is re-gated.
- If a merge conflict or repository-wide check fails, a `merge` or `fix` job handles it.
- If the worker needs a human, the job is parked and an inbox item appears.

Check the inbox at any time:

```sh
# (planned)
vowp inbox ls
vowp inbox answer <id> "Use the existing title style"
```

Answering unparks only the asking job. Everything else keeps running.

## 8. Stop cleanly

```sh
# (planned)
vowp down
```

`down` writes a stop request and waits up to 30 seconds for the supervisor lock to disappear. Running
runs are interrupted and re-queued as `stalled`, not failed. The tmux windows are left for inspection;
add `--kill-windows` to close them, or `--force` to send SIGTERM after the wait.

Restarting later with `vowp up` reconciles from the database and session logs; nothing needs to be
replayed by hand.

## Where to go next

- [Writing jobs](jobs.md) — briefs, gates, reports and the verify/fix loop.
- [Operating a node](operating.md) — logs, loops, backups, watchdog.
- [Sessions](sessions.md) — switch model or harness mid-job.
- [Troubleshooting](../troubleshooting.md) — when a job does not move.
