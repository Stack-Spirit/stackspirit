# Dogfood results — `vowp up` on a throwaway project

This is the result of job `n14-dogfood`: run the real `vowp` binary through
`init → doctor → job add → up → worker → gates → merge → status/board → down`, using fake
model adapters only, and record every breakage as a fixable defect.

- **Harness:** `scripts/dogfood/run.sh` (fake harness in `scripts/dogfood/fake-harness/`)
- **Final run:** `bash scripts/dogfood/run.sh` on branch `job/n14-dogfood` rebased on
  `origin/main` commit `1b14daa` (release: qualify the native-session-id limitation as
  switch-specific). n08d planner/keeper/failover was **not** in that commit (the n08d
  branch was still in review); the script was re-run after the rebase and passed.
- **Result:** PASS (exit 0) in **45 s** wall time including the rebuild (`vowp 0.1.0-rc.1`)
- **Environment:** Linux, tmux 3.4, Python 3.12, stable Rust from `rust-toolchain.toml`;
  no network, no real `claude`/`opencode`, no real `HOME`.

## What the harness does

Both phases create a fresh temporary directory with a private `HOME`, `TMPDIR`, VOWP
home, and a private tmux server. A `tmux` wrapper on `PATH` turns every tmux call into
`tmux -L vowp-dogfood-<pid>-<phase>`, so the caller's tmux server is never contacted.
Fake `claude` and `opencode` binaries answer only `--version` (the `doctor` probes).
The worker/verifier is a shell script that edits one file, commits it, and writes a valid
report; the supervisor, git worktrees, gates, merge queue, ledger, board, tmux windows and
heartbeat are the real implementation.

**Phase 1** (`dogfood-one`) creates a 3-job graph:

```
alpha ──> gamma          (one dependency edge)
beta
```

It runs `vowp init --no-framework`, installs the fake-adapter `vowp.toml`, runs
`vowp doctor --json`, adds the jobs, starts `vowp up --detach`, waits for all three
`done`, then runs `vowp down --kill-windows`. Assertions: exactly the initial commit plus
three job commits on the local bare `origin/main`; the board shows 3 done with no running
jobs/slots; each job has state ledger rows and exactly one merge ledger row; three worker
start runs; no job worktrees or `job/*` branches remain; no tmux windows remain on the
private socket; `heartbeat` is gone and `heartbeat.stopped` exists; `status --json` reports
stopped.

**Phase 2** (`dogfood-two`, ids prefixed `crash-`) repeats the graph and kills the
detached supervisor with `SIGKILL` after every currently `running` job has an in-flight
run row with a live pid. It restarts `vowp up --detach` and waits again. Assertions: the
restart records orphaned runs as `crash`, writes `RunEndedNoReport` continue rows, the fake
harness sees `continue` turns, all three jobs reach `done`, each job has exactly one merge
ledger row, origin still has exactly three job commits (no duplicated merge), no job is
stuck `running` without an open run, and the same worktree/tmux/heartbeat cleanup holds.

Run it with:

```sh
bash scripts/dogfood/run.sh
# optional:
KEEP=1 bash scripts/dogfood/run.sh
VOWP=/path/to/vowp bash scripts/dogfood/run.sh
CRASH_EARLY=1 bash scripts/dogfood/run.sh   # fast reproducer for D3, expected to fail
```

## What passed

| Area | Evidence |
|---|---|
| `vowp init` / config / project registration | `initialised project dogfood-one at …`, then `vowp doctor` exit 0 |
| `vowp doctor` with fake harnesses | `ok` for `home`, `db`, `git`, `config`, `profiles`, `harness:claude-code`, `harness:opencode` |
| `vowp job add` + dependency edge | `vowp graph` prints `gamma <- alpha` (phase 1) and `crash-gamma <- crash-alpha` (phase 2) |
| `vowp up` / supervisor | detached supervisor pid, fresh heartbeat, board/status running |
| worker → report gate → cmd gate → owns gate | all three jobs `done` |
| merge queue | exactly 3 job commits on `origin/main`, local `main` fast-forwarded and clean |
| board / status | `done: 3`, no running jobs, `slots_busy: 0`, `heartbeat_age_secs: null` after down |
| ledger | each job has ≥3 `state` rows and exactly 1 `merge` row; 3 worker `start` runs |
| `vowp down --kill-windows` | lock removed, heartbeat removed, stopped marker written, no windows/worktrees |
| SIGKILL + restart (default window) | 2 crash runs, 2 `RunEndedNoReport` continue rows, 2 `continue` harness turns, all 3 jobs done, 1 merge row each |
| loops smoke check (n08d-tolerant) | `l4`–`l9` rows present via `vowp loops list --json` when that subcommand exists, otherwise via `vowp loops --json` |
| fake verifier config | the `verifier` profile resolves and probes; the fake harness also implements verifier reports (no `review` gate is in this graph, so no verify job was created) |

## Timings (final run)

| Step | Time |
|---|---|
| `cargo build -q -p vowp-cli` (rebuild after the rebase) | included in the total below |
| Phase 1 (init → down) | 9 s on both the post-rebase and warm runs |
| Phase 2 (init → SIGKILL → restart → down) | 11 s |
| Total wall time (`/usr/bin/time`) | 44.65 s including the rebuild; 23.89 s on the post-commit warm rerun |

The normal run completes in well under a minute; the default timeouts are 600 s for phase 1
and 300 s for phase 2.

## Defect list

Each defect has a reproduction, the observed and expected behaviour, the specification it
violates, and the module that should own the fix. Nothing in `crates/**` was changed by
this job.

### D1 — `vowp up --home` creates tmux windows without `--home`; the layout dies

**Severity: blocking for v0.1** (tests always pass `--home`; a non-default home cannot run
`up` at all, and a default home can make the windows attach to the wrong node database).

Reproduction (private `HOME`, no `VOWP_HOME`):

```sh
WORK=$(mktemp -d)
HOME="$WORK/empty-home" mkdir -p "$WORK/empty-home"
git init -q -b main "$WORK/repo"
cd "$WORK/repo"
vowp --home "$WORK/vowp-home" init --name d1 --no-framework
unset VOWP_HOME
vowp --home "$WORK/vowp-home" up --detach; echo "exit=$?"
```

Observed (from the dogfood run before the harness set `VOWP_HOME`):

```
vowp: tmux ["set-option", "-w", "-t", "@0", "@vowp", "1"] failed with Some(1):
      no server running on .../tmux-<uid>/default
dogfood: FAIL: [phase1] vowp up --detach failed
```

The `board` window runs the bare binary `vowp __window board`; it resolves the node home
from `$VOWP_HOME`/`~/.vowp`, not from the `--home` passed to `up`. Without a database there,
the window exits, tmux destroys the window/session, and the `@vowp` tag write fails. If the
caller's real `~/.vowp` exists, the windows instead attach to the *wrong* home.

Expected: `up --home <dir>` starts a supervisor whose windows use that same home.

Spec: `docs/design/cli.md` §1 (`--home` is global) and §2 (`up` ensures the layout);
`docs/design/crates/vowp-tmux.md` §4.

Suggested owner: `vowp-cli/src/cmd/up.rs` (`ensure_project_windows` builds the argv; add
`--home`/`--project`) and `vowp-core/src/loops/reconcile.rs` (`ensure_windows` has the same
problem and additionally uses the bare name `vowp`, not the current executable — see D7).

Harness workaround: `scripts/dogfood/run.sh` exports `VOWP_HOME` before `up`, so the
windows resolve the private home. This does not fix the CLI defect.

### D2 — planner, keeper and inbox tmux windows exit immediately

**Severity: blocking for v0.1** (the documented five-window layout does not exist after
`up`; only `board` and `supervisor` survive).

Reproduction: start a supervisor with a private tmux socket and list the windows:

```sh
# inside a run of the dogfood harness, or with the same tmux wrapper:
vowp up --detach
tmux -L <private-socket> list-windows -a -F '#{session_name}:#{window_name}'
```

Observed (dogfood output, repeated in both phases):

```
dogfood: [phase1] tmux: board+supervisor present; one-shot windows exited: planner keeper inbox (defect D2)
```

`vowp __window planner`/`keeper` print `no <role> session yet` and return when there is no
role session; `vowp __window inbox` lists the inbox once and returns. tmux removes a window
when its command exits, so those three windows vanish immediately.

Expected: the five fixed windows `board supervisor planner keeper inbox` remain (or their
panes stay in a dead-but-visible state until the session exists).

Spec: `docs/design/cli.md` §2 (`up` creates/ensures the five windows);
`docs/design/crates/vowp-tmux.md` §2 and §8.

Suggested owner: `vowp-cli/src/cmd/window.rs` (make inbox/planner/keeper follow or refresh
instead of returning) or `vowp-tmux` (set `remain-on-exit` for one-shot window commands).

### D3 — SIGKILL in the `Dispatched`-but-no-run-yet window leaves a job stuck `running`

**Severity: blocking for v0.1** (a crash at the wrong instant loses a job permanently; no
later tick ever starts work or reports an error).

Reproduction:

```sh
CRASH_EARLY=1 bash scripts/dogfood/run.sh
```

The script kills the supervisor as soon as `job ls` reports a job `running` (the narrow
window between the `Dispatched` transition and the run-row insert). Observed after the
restart:

```
crash-alpha: state=running but no open run row
dogfood: FAIL: [phase2] a job is running with no open run after restart (defect D3)
```

The work directory is kept. The same run's database shows `jobs.state='running'`, a
`ready -> running (Dispatched)` ledger row, and no row in `runs` or `sessions` for that job.
`reconcile::startup` only finalises existing open run rows; `step_harvest` needs a run row;
`step_states` only restarts jobs with a `delay:` key. Nothing recovers the job.

Expected: startup reconciliation or the dispatch path detects a `running` job with no open
run and either starts a fresh turn (after marking the missing attempt) or fails/park it;
the graph must not stall forever.

Spec: `docs/design/loops.md` §1 step 1/2 and §11 ("On restart … orphans → crash → L2
continues"); `crates/vowp-core/src/loops/reconcile.rs`, `supervisor.rs` `step_states`.

Suggested owner: `vowp-core/src/loops/reconcile.rs` (add a running-without-run pass) and/or
`vowp-core/src/loops/supervisor.rs` (persist the run row before applying `Dispatched`).
The default dogfood phase 2 waits for a complete in-flight run set before killing; it
therefore exercises the documented crash/continue path and passes. `CRASH_EARLY=1` is the
loud reproducer for this defect.

### D4 — `vowp job add --brief` content is not copied into the job worktree; workers read a generic fallback

**Severity: blocking for v0.1** (an agent worker cannot see its goal, deliverables or
scope; every real job would run the generic one-line brief instead).

Reproduction:

```sh
bash scripts/dogfood/run.sh            # the run prints to stderr/stdout:
# dogfood: [phase1] note: workers saw the dispatch fallback brief, not the job add brief (defect D4)
grep 'brief=' "$WORK"/phase1/fake.log
# 2026-… brief=generic alpha
# 2026-… brief=generic beta
# 2026-… brief=generic gamma
```

Manual shape:

```sh
vowp job add --id demo --brief - <<'EOF'
# Goal
Create src/demo.txt.
EOF
vowp job show demo --brief      # the real body above
# then start the supervisor; the worker's start prompt points at
# <worktree>/.vowp/jobs/demo/brief.md, which contains:
# "Do job demo. Read AGENTS.md first, change only the paths in `owns`, …"
```

`vowp job add` writes `.vowp/jobs/<id>/brief.md` in the main checkout, but `.vowp/` is
git-ignored and is not part of a new git worktree. `dispatch::write_brief` finds no file in
the worktree and writes its generic fallback, so the worker never sees the brief the CLI
stored. (`vowp job show --brief` still shows the real one, which makes the divergence easy
to miss.)

Expected: the worktree brief is the same brief written by `vowp job add` (or the prompt
points at the main checkout's copy).

Spec: `docs/design/cli.md` §3 (`job add` writes the brief) and `docs/design/loops.md` §3.1
(the start prompt tells the worker to read that path).

Suggested owner: `vowp-core/src/loops/dispatch.rs` (`write_brief` should copy the main
checkout's brief before falling back) or `vowp-cli/src/cmd/job.rs` (write the brief into a
path shared by worktrees).

### D5 — a clean `vowp down` still makes `status --check` fail, and the watchdog/systemd restart the supervisor

**Severity: blocking for v0.1 operations** (an installed node cannot be left stopped; the
five-minute watchdog or `Restart=always` brings it back).

Reproduction:

```sh
# start a supervisor, then:
vowp down
vowp status --check; echo "check_exit=$?"       # check_exit=1
VOWP_HOME=<the stopped home> scripts/vowp-watchdog.sh --dry-run
# dry-run: heartbeat is missing; would restart the supervisor
```

Observed: `down` removes `heartbeat` and writes `heartbeat.stopped`; `status --check`
treats the stopped marker as stale and exits 1; `scripts/vowp-watchdog.sh` checks only the
missing heartbeat and would run `vowp up --detach`. The systemd unit is
`Restart=always`, so a clean `--foreground` exit is also restarted.

Expected: a deliberate `down` stays stopped until an operator starts it again. Either
`status --check` should be healthy when the supervisor is deliberately stopped, or the
watchdog/systemd unit should honor `heartbeat.stopped`/`stop`, or `down` should disable the
watchdog.

Spec: `docs/guides/operating.md` ("down writes `~/.vowp/stop`"; `status --check` exit 1
means stale); `docs/design/loops.md` §10; `scripts/vowp.service` (`Restart=always`).

Suggested owner: `vowp-cli/src/cmd/status.rs` and `vowp-cli/src/signals.rs` (make a clean
stop distinguishable from a stale heartbeat), plus `scripts/vowp-watchdog.sh` /
`scripts/vowp.service` (outside this job's owned paths; reported, not edited).

### D6 — `vowp up --no-tmux` still creates a tmux layout on supervisor startup

**Severity: blocking for v0.1** (the flag is documented and used by tests/CI; it can create
windows in a real tmux server the caller did not want touched).

Reproduction (with the dogfood private-socket wrapper on `PATH`, or any private server):

```sh
vowp --home "$H" up --no-tmux --detach
tmux -L <private-socket> list-windows -a -F '#{session_name}:#{window_name}'
```

Observed:

```
vowp-dogfood-one:board
vowp-dogfood-one:supervisor
```

The parent `up` skips `ensure_project_windows` for `--no-tmux`, but the detached supervisor
runs `Supervisor::startup` → `reconcile::ensure_windows`, which always tries to create the
layout. The supervisor also starts a tmux session even when the caller explicitly asked for
no window manager.

Expected: `up --no-tmux` starts the supervisor without touching tmux at all.

Spec: `docs/design/cli.md` §2 (`--no-tmux`: "no window manager"); `docs/guides/quickstart.md`
uses `up --no-tmux` for headless/tests.

Suggested owner: `vowp-core/src/loops/reconcile.rs` (gate `ensure_windows` on a
no-tmux/no-window-manager setting) and `vowp-cli/src/cmd/up.rs` (propagate the flag).

### D7 — startup window recreation uses the bare name `vowp`, not the running executable

**Severity: minor** (only hits a supervisor that needs to recreate the layout while running
an uninstalled build, or with a different `PATH`/`VOWP_HOME`; the normal `up` path uses the
absolute executable).

Reproduction sketch: build the CLI somewhere not on `PATH`, run `vowp up --no-tmux` with a
reachable tmux server, then let the supervisor start. `reconcile::ensure_windows` builds
`["vowp", "__window", …]`; tmux cannot find that command (or finds a different `vowp`), the
window dies with the session, and the supervisor logs a warn.

Spec: `docs/design/crates/vowp-tmux.md` §4 and `docs/design/cli.md` §2.

Suggested owner: `vowp-core/src/loops/reconcile.rs` (use the current executable and the same
home/project arguments as `up`). This is the same root cause as D1's second half and is
listed separately because it can affect a default-home install too.

## Notes and non-defects

- `vowp doctor` unconditionally probes the built-in `claude-code` and `opencode` adapters,
  even when `vowp.toml` only uses the generic `cli` adapter. This is documented in
  `docs/design/cli.md` §2; the harness pins both probes to version-only fake binaries so no
  real harness runs. Not filed as a defect.
- The graph in this run has no `review` gate, so no verify job is created. The verifier
  profile is configured, probed by `doctor`, and the fake harness implements verifier
  reports, but L3 was not exercised end-to-end. That is a coverage limit of this graph, not
  a defect.
- `repo-wt/` (the empty worktree root) remains after a run by design; `git worktree list`
  shows only the main checkout and no `job/*` branches remain.
