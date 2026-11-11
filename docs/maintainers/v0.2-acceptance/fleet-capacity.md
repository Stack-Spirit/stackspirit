# v0.2 acceptance — fleet and capacity (T2)

This is the acceptance record for §13.5 tests **2 (two projects on one home, and a
broken project)** and **3 (capacity: brake, owner override and advice)**. It is the T2
card `v2-t2-fleet-capacity`.

**Result on the tested build: test 2 passes. Test 3 is partial: the brake and the owner
override pass; the advisor sub-check cannot run because `v2-e3-advisor` is not merged.**
The run also found two defects that stop a project from running at all: the default
worktree root is outside the session allow-list, and `project add` from outside a
checkout builds its adapter registry in the wrong directory. Both are recorded in
[Open items](#open-items) with exact reproductions.

## Environment

- Date: 2026-11-10 (UTC). The run finished late on 2026-11-10 UTC / early 2026-11-11
  local time.
- Worktree: `job/v2-t2-fleet-capacity`, based on `ed3e9c6` (`origin/main` at the time).
- Binary under test: built with `cargo build -p vowp-cli --bin vowp`; it reports
  `vowp 0.1.0` (the package version during the v0.2 pre-release), SHA-256
  `8a2f62b252718178ed9c242eeb27f2393975f7c4968c88f886b7517bfe7ad32e`.
- Rig: `bash scripts/acceptance/rig.sh up --name t2 --variant sudo --no-build`.
  Home container `vowp-acceptance-t2-home`, image `vowp-acceptance/vps:sudo`
  (`sha256:3c19867d4d633df0ca5f0dda7fa06300e69816a3857c1488be05184c915011ab`),
  Ubuntu 24.04 with sshd, git, tmux, sqlite3 and the rig fixtures.
- Private tmux: `TMUX_TMPDIR=/home/vowp/.acceptance/tmux`, socket
  `vowp-acceptance-t2`. The host home, host `~/.vowp` and host tmux server were never
  touched.
- Home under test: `/home/vowp/acceptance/t2/home`, a temporary `VOWP_HOME`.
- No real harness, model, network or secret was used. Every worker was the rig's fake
  harness. The only injected host data is the two forced memory-pressure samples
  described under test 3; the broken project's needs cycle is fixture data.
- The rig was removed after the evidence was collected with
  `bash scripts/acceptance/rig.sh down --name t2` (exit 0; container and generated
  keys removed).

The commands below use these shell variables inside the home container:

```sh
V=/home/vowp/bin/vowp
H=/home/vowp/acceptance/t2/home
E=/home/vowp/evidence
```

### Test configuration

The fixture generator does not set `project.worktree_root`. This build's session
allow-list contains the project path and the configured worktree root, but not the
sibling `<project>-wt` directory that the worktree code chooses when the setting is
missing. With the fixture as generated, every worker run was refused before the harness
started:

```text
ERROR l2 run for alpha-1 failed to start: refused: working directory /home/vowp/projects/alpha-wt/alpha-1 is outside the allow-list
```

The acceptance run therefore sets one explicit worktree root per project and commits it
in the fixture checkout (a clean main checkout is a merge-queue requirement):

```toml
[project]
worktree_root = "/home/vowp/acceptance/t2/worktrees/<name>"
```

The home's node config `<home>/config.toml` sets `tick_secs = 1` so the test does not
wait a minute per supervisor tick; the fixture project configs already set
`[node] max_workers = 4` and `tick_secs = 1`. The default worktree gap is open item 1.

## Test 2 — two projects and a broken project: PASS

### Fixture

A fresh home was created from a throwaway bootstrap checkout with `vowp init`, the
bootstrap project was removed from the registry, and three fixture projects were
generated:

```sh
bash scripts/acceptance/rig.sh fixture --name t2 hello /home/vowp/projects/alpha
bash scripts/acceptance/rig.sh fixture --name t2 memory-hog /home/vowp/projects/bravo \
    --hog-mb 256 --hog-hold 6
bash scripts/acceptance/rig.sh fixture --name t2 hello /home/vowp/projects/broken
```

`alpha` had three build jobs (`alpha-1`…`alpha-3`), `bravo` had three build jobs
(`bravo-1`…`bravo-3`) using the 256 MiB / 6 s memory-hog worker, and `broken` had two
jobs (`broken-1`, `broken-2`). The two broken jobs were made a dependency cycle with a
direct project-database insert, because `job add --needs` refuses to create a cycle:

```sh
NOW=$(date -u +%Y-%m-%dT%H:%M:%S.000Z)
sqlite3 "$H/projects/broken/project.db" \
  "INSERT INTO edges(src,dst,kind,created_at) VALUES
     ('broken-1','broken-2','needs','$NOW'),
     ('broken-2','broken-1','needs','$NOW');"
```

`vowp project add` was run for each checkout. When run from the home (not from inside
the checkout) it exited 1 with `vowp: io: No such file or directory` for the projects
with a custom harness mapping, although the registry row and project database were
created. Running it again from inside the checkout succeeded; that defect is open item
2. The registry before the run was:

```text
NAME    STATE   WEIGHT  PATH
alpha   active  1       /home/vowp/projects/alpha
bravo   active  1       /home/vowp/projects/bravo
broken  active  1       /home/vowp/projects/broken
```

The supervisor was started with `vowp --home "$H" up --detach`. It sampled the host and
chose the measured auto target **6** workers (the CPU fit,
`floor(8 × 0.85 / 1.0)`, was the binding limit for the default estimate); the project
worker limit is a preference, not a cap.

### Both projects ran at the same time

The supervisor status at 23:38:24 (three seconds into the run) shows work in flight in
both projects at once:

```text
alpha  active  running 3            running: alpha-1, alpha-2, alpha-3
bravo  active  running 2, ready 1   running: bravo-1, bravo-2
broken active  pending 2
```

At 23:38:25.8 `alpha` had moved its three jobs to `gating` and `bravo` had all three
workers running:

```text
alpha  active  gating 3    running: alpha-1, alpha-2, alpha-3
bravo  active  running 3   running: bravo-1, bravo-2, bravo-3
broken active  pending 2
```

The `alpha` fake workers are sub-second, while `bravo`'s memory-hog workers hold their
256 MiB for 6 s; the board's `running` counts are job states, so the two projects'
worker process lifetimes are close but do not fully overlap. The point of the check is
that the one supervisor admitted and advanced both projects in the same passes and
neither waited for the other; both finished all their jobs. The wait loop observed zero
non-terminal jobs at t=18 s; the run's end marker was 23:38:40, 22 s after start. The
worker run history (from each project database) is:

```text
alpha|alpha-1|worker|2026-11-10T23:38:21.455Z|2026-11-10T23:38:21.692Z|ok
alpha|alpha-2|worker|2026-11-10T23:38:21.835Z|2026-11-10T23:38:21.982Z|ok
alpha|alpha-3|worker|2026-11-10T23:38:22.315Z|2026-11-10T23:38:22.555Z|ok
bravo|bravo-1|worker|2026-11-10T23:38:23.041Z|2026-11-10T23:38:29.934Z|ok
bravo|bravo-2|worker|2026-11-10T23:38:23.984Z|2026-11-10T23:38:30.641Z|ok
bravo|bravo-3|worker|2026-11-10T23:38:24.403Z|2026-11-10T23:38:30.952Z|ok
```

The final status was `alpha done 3`, `bravo done 3`; neither project waited for the
other and neither was starved.

### The broken project parked alone

`broken` failed its `states` and `dispatch` steps on every tick with the dependency
cycle. After three failed ticks the node supervisor paused only that project:

```text
broken  paused  reason: broken: 3 failed ticks: states: dependency cycle:
                [JobId("broken-1"), JobId("broken-2"), JobId("broken-1")];
                dispatch: dependency cycle: ...
```

The node inbox received one notice, `project broken paused` (kind `notice`, state
`open`). `alpha` and `bravo` remained `active` and both completed their jobs after the
park, so the break was isolated.

**Test 2: PASS.**

## Test 3 — capacity

A fourth fixture project `hog` was added for this test, using
`memory-hog --hog-mb 512 --hog-hold 20`. This is a real worker that allocates and
touches 512 MiB, holds it for 20 s, then commits its owned file and writes its report.

### 3a — owner override above the advice: PASS

The report before the test was an `auto` target of **6** with `advised 6`. A target two
above the advice was refused without `--override`, exit **5**:

```sh
$V --home "$H" --json capacity set --target 8 --reason "acceptance override probe"
```

```json
{"error":{"hint":null,"kind":"refused","message":"refused: target 8 is above the advised 6; pass --override to accept it"}}
```

The same target with `--override` was accepted:

```json
{"mode":"fixed","target_workers":8,"advised_workers":6,"is_override":true,"set_by":"acceptance","set_at":"2026-11-10T23:39:05.695Z","reason":"acceptance owner override"}
```

`capacity show` printed the override line:

```text
target: 8 (fixed) advised 6
owner override: 8 (advised 6)
```

A second owner target, 9, was also accepted with `--override` and shown as
`owner override: 9 (advised 6)`. The target history is one row per change:

```text
2026-11-10T23:38:21.159Z  auto   6  6  0  supervisor  auto target from measurements
2026-11-10T23:39:05.695Z  fixed  8  6  1  acceptance  acceptance owner override
2026-11-10T23:39:05.875Z  fixed  9  6  1  acceptance  acceptance override again
2026-11-10T23:39:20.409Z  fixed  6  6  0  owner       acceptance brake test
```

**3a: PASS.**

### 3b — brake stops admissions and nothing is killed: PASS

The `hog` project's first job, `hog-1`, started at 23:39:18; the owner target was set
back to `6` at 23:39:20. At 23:39:36 the fake worker was holding its 512 MiB (process
1987, 18 s elapsed), and the test paused the project so a second job could be added
without racing the next tick:

```sh
$V --home "$H" project pause hog
printf '# Goal\n\nCreate src/hog-2.txt; this job must wait for the brake.\n' |
  $V --home "$H" job add --id hog-2 --title "hog job 2" --kind build \
     --owns src/hog-2.txt --gates report,owns_respected --brief -
```

The rig host is shared with other workers and its real `/proc/pressure/memory` sampled
at `avg10 = 3.55` at 23:39:35, below the brake threshold. The brake had nevertheless
already been persisted active at 23:39:33.875, so the test inserted two samples with the
exact shape the brake reads, timestamps 20 s ahead and `pressure_mem_avg10` 14.2 and
15.0, to keep the measured condition in the window while the supervisor's clean samples
continued to arrive. This is a deterministic brake-enforcement check with a live
memory-heavy worker, not a claim that the 512 MiB hog alone drove host pressure over
10:

```sh
AT=$(date -u -d '+20 seconds' +%Y-%m-%dT%H:%M:%S.000Z)
for PRESSURE in 14.2 15.0; do
  sqlite3 "$H/vowp.db" "
    INSERT INTO host_samples (
      at, interval_secs, mem_total_bytes, mem_available_bytes, swap_used_bytes,
      load1, load5, load15, cpu_count, cpu_quota_cores, disk_free_home_bytes,
      disk_free_worktrees_bytes, pressure_cpu_avg10, pressure_mem_avg10,
      pressure_io_avg10, source
    )
    SELECT '$AT', 5, mem_total_bytes, mem_available_bytes, swap_used_bytes,
      load1, load5, load15, cpu_count, cpu_quota_cores, disk_free_home_bytes,
      disk_free_worktrees_bytes, pressure_cpu_avg10, $PRESSURE,
      pressure_io_avg10, 'proc'
    FROM host_samples ORDER BY id DESC LIMIT 1;"
done
```

`capacity show` then reported the brake, and `vowp project resume hog` let the next tick
try to dispatch `hog-2`:

```json
{"brakes":{"active":true,
          "reasons":[{"code":"memory_pressure","detail":"memory pressure (avg10 15.0)"}],
          "since":"2026-11-10T23:39:33.875Z"},
 "target":{"mode":"fixed","target_workers":6,"advised_workers":6,"is_override":false,
           "set_by":"owner","set_at":"2026-11-10T23:39:20.409Z",
           "reason":"acceptance brake test"}}
```

`fleet queue` and `jobs.note` showed the admission stopped at the brake, not at the
target:

```text
project  job     state  waiting_reason
hog      hog-2   ready  waiting: memory pressure (avg10 15.0)
```

`hog-2` stayed `ready` for the whole brake. `hog-1` was not signalled: it finished on
its own at 23:39:39.250 with outcome `ok`, and its `run_usage` row recorded
`oom_killed = 0`, `exit_code = 0`, peak RSS 552,357,888 bytes. The process was observed
alive (`ps`, PID 1987, 18 s elapsed) while the brake was active.

The brake flag then cleared. `hog-2` kept the old waiting note in `ready` until the
next dispatch, then started and completed:

```text
brake cleared at ~23:39:49; hog-2 still ready with the old note
hog-2 worker: 2026-11-10T23:39:59.585Z .. 2026-11-10T23:40:21.437Z, outcome ok
run_usage:    project=hog job=hog-2 peak_rss=552,296,448 oom_killed=0 exit_code=0
```

The final hog jobs were both `done`, and neither hog job was OOM-killed:

```text
project  job_id  role    peak_rss_bytes  cpu_usec  source     oom_killed  exit_code
hog      hog-1   worker  552357888       580000    proc_tree  0           0
hog      hog-2   worker  552296448       670000    proc_tree  0           0
```

On the same shared host, real memory pressure later crossed the threshold on its own
(`avg10 = 21.0` at 23:41:11, with load 57.87); the brake tripped and cleared with no
jobs running. That is not part of the controlled result, but it shows the same code path
on a real condition.

**3b: PASS.**

### 3c — an advisor proposal arrives as an inbox item and is applied only after a yes: FAIL

This build does not implement the advisor. `v2-e3-advisor` is not merged;
`CapacityManager::advise()` is still the C0 stub returning `Ok(None)`, and no supervisor
step calls it. After the whole run:

```text
capacity_advice rows                    0
node_inbox rows                         one, the broken-project notice
node_inbox titles LIKE 'Capacity:%'     0
capacity_targets set_by values          supervisor, acceptance, owner only
```

There was no advisor proposal to accept or reject, and therefore no proof that a
proposal is applied only after a yes. The rest of test 3 passes, but the advisor clause
is a release-candidate blocker and must be re-run after E3 merges.

**3c: FAIL (not implemented).**

## Open items

1. **Fix job: default worktree root is outside the session allow-list.**
   `worktree_root()` falls back to the sibling `<project>-wt` directory when
   `project.worktree_root` is unset, but `SessionService::check_cwd()` and the switch
   path only allow the project path, the configured worktree root and
   `[node].cwd_allow`. A fixture with the default config therefore refuses every run
   with `refused: working directory ... is outside the allow-list`. The acceptance run
   worked around it by setting and committing `worktree_root` per project.
   Suggested owner paths: `crates/vowp-core/src/session/service.rs`,
   `crates/vowp-core/src/session/switch.rs`, and/or
   `crates/vowp-cli/src/cmd/project.rs` (persist the resolved default when a project is
   registered).
2. **Fix job: `project add` from outside a checkout fails to prepare the project.**
   With a fixture whose harness mapping is a relative path, running
   `vowp --home "$H" project add /home/vowp/projects/io-check` from the home exits 1
   with `vowp: io: No such file or directory (os error 2)`. The registry row and
   project database are created anyway; running the same command from inside the
   checkout exits 0 (`already registered`). The cause is that `register_checkout()`
   loads the project config with `project.path = None` and
   `AdapterRegistry::from_config()` resolves the relative mapping against the current
   directory. Suggested owner paths: `crates/vowp-cli/src/cmd/project.rs` (set
   `config.project.path` before `prepare`) or `crates/vowp-core/src/deps.rs`
   (`mapping_path` should resolve against the config file's directory).
3. **Advisor acceptance is not complete until E3 merges.** `v2-e3-advisor` is a separate
   Wave 4 card; the T2 `needs` list in the plan names E2, C2 and T0, not E3. Re-run
   3c after E3, or add E3 to T2's needs in a future revision. The expected shape is the
   §6.4 advice row and a `decision` inbox item titled `Capacity: …` (the spec's §16
   deviation from the plan's "capacity" kind), applied with
   `vowp capacity set --target N` only after the owner answers yes.
4. **Observation, not a product failure: the fake worker log is never written.** The
   fixture's `[harnesses.acceptance-hello]` lists `env_pass =
   ["VOWP_ACCEPTANCE_WORKER_LOG"]`, but `env_pass` is resolved through the vault as
   secret names, not from the supervisor's process environment. Every turn logged
   `vault secret VOWP_ACCEPTANCE_WORKER_LOG is not set; not injected` and
   `/home/vowp/evidence/t2-worker.log` stayed empty. The acceptance run used the run
   timestamps and `run_usage` rows from the databases instead. This is a T0 fixture
   documentation mismatch; the rig's README should not promise the log without storing
   the value as a secret.

## How to reproduce

On a fresh rig (`bash scripts/acceptance/rig.sh up --name t2 --variant sudo --no-build`),
copy the built `vowp` into the home container, then inside the home container:

```sh
V=/home/vowp/bin/vowp
H=/home/vowp/acceptance/t2/home
E=/home/vowp/evidence
```

1. Create a bootstrap checkout, `vowp --home "$H" init`, `vowp --home "$H" project rm
   bootstrap --yes`. Write `<home>/config.toml` with `[node] tick_secs = 1` and
   `max_workers = 4`.
2. Generate `alpha` (`hello`), `bravo` (`memory-hog --hog-mb 256 --hog-hold 6`) and
   `broken` (`hello`) with the host command
   `bash scripts/acceptance/rig.sh fixture --name t2 <kind> <path>` (inside the home
   container the same script is `/opt/vowp-acceptance/fixtures/project.sh`). Add and
   commit the explicit
   `worktree_root` shown above in each project, then run `vowp --home "$H" project add .`
   from inside each checkout. Seed three jobs in `alpha` and `bravo`, two in `broken`,
   and insert the two `needs` edges shown in the test 2 fixture.
3. `vowp --home "$H" up --detach`. Observe `vowp --home "$H" fleet status` while both
   projects have running jobs, then wait for `alpha` and `bravo` to be `done`; check
   `vowp --home "$H" project ls` and `fleet inbox` for the broken project's pause.
4. For the override, run the `capacity set` commands in 3a. For the brake, add the
   `hog` project, run `hog-1`, pause the project, add `hog-2`, insert the two
   `pressure_mem_avg10` samples at `now + 20 s`, run `capacity show`, resume the project
   and read `fleet queue`. Check `run_usage` for `oom_killed = 0` after both hog jobs
   finish.

The evidence files from this run were kept outside the repository in the container's
`/home/vowp/evidence` (now removed with the rig); the key documents are
`t2-status-during-3s.json`, `t2-status-during-6s.json`, `t2-status-final.json`,
`t2-worker-runs.txt`, `t3-capacity-before.json`, `t3-set-refused.json`,
`t3-set-override.json`, `t3-capacity-brake.json`, `t3-queue-brake.json`,
`t3-hog-run-usage.txt`, `t3-hog-runs-final.txt` and `t3-advisor-*`.
