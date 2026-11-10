# S1 — capacity measurement spike

Job: `v2-s1-capacity-spike` (VOWP-V2-PLAN §13.3, Wave 0)
Date: 2026-11-10
Host: the VPS build node, as the `dev` build user
Tools: `scripts/spikes/capacity/**`
Status: complete; the measurements below are real runs on this node.

## Decision

1. **Use a cgroup v2 user scope per job.** `systemd-run --user --scope` works on
   this stock Ubuntu 24.04 VPS as the build user. A transient scope exposes
   `memory.peak` and `cpu.stat` per job, and `MemoryHigh` is writable and
   enforced. This is the preferred E1 method.
2. **Keep the `/proc` tree walk only as a fallback.** It needs no privileges and
   follows `AdapterRun::pid()`, but it overcounts shared pages by about 31 % in
   a warm `cargo test`, undercounts CPU by 15–36 % because short-lived
   processes exit between samples, and cannot see a build that runs in a
   Docker container. It is good enough to keep a node limping, not good enough
   for precise estimates.
3. **Treat disk as a first-class capacity dimension.** The 290 GB disk was
   98–100 % full during the spike. A cold `cargo test` build needs a 1.7 GB
   target directory (with debug info off) and its page cache; when the disk
   filled, tests failed with `No space left on device` / `sqlite: disk I/O
   error`. The 5 GB / 10 % reserve in §6.2 is necessary, not cosmetic.
4. **The build, not the harness, dominates a cold job.** A 192 MiB harness
   stand-in plus a cold `cargo test` peaked at **2.45 GiB** and used **300 s
   CPU**; the build was 2.25 GiB and 99.6 % of the CPU. A warm build peaked at
   **0.37 GiB** and used **56–60 s CPU**, where the harness is about half the
   RAM but under 2 % of the CPU. The plan's "1.5 GB / 1 CPU until five
   samples" default is plausible for warm gate runs and too low for a cold
   Rust build.

E1 should therefore store both the cgroup peak and the `anon` figure, label
cold/incremental runs separately, and let E2 refuse new work when disk or RAM
would cross the reserve. It should not use the `/proc` RSS sum as the primary
number.

## Environment and probe

```text
$ python3 scripts/spikes/capacity/probe.py --json /tmp/capacity-probe.json
capacity probe
  host:        Ubuntu 24.04.4 LTS / 6.8.0-147-generic / x86_64
  user:        dev (uid 1001), 8 CPUs
  memory:      24031 MiB total, 20448 MiB available
  cgroup:      v2 mounted at /sys/fs/cgroup; own path /user.slice/user-1001.slice/session-8524.scope
  systemd-run: systemd 255 (255.4-1ubuntu8.18)

  ok   cgroup_v2
  ok   transient_user_scope
  ok   memory_peak
  ok   cpu_stat
  ok   memory_high_property
  ok   memory_high_enforced
  ok   proc_tree_walk
result: ok
exit 0
```

Facts relevant to the design:

| Fact | Observed |
|---|---|
| OS / kernel | Ubuntu 24.04.4 LTS, Linux 6.8.0-147-generic, x86_64 |
| cgroup | v2 mounted at `/sys/fs/cgroup`; controllers `cpu memory pids` |
| Delegation | `user@1001.service` has `cpu memory pids` and `+cpu +memory +pids`; `Linger=yes` |
| CPU | 8 CPUs, `cpu.max = max 100000` (no quota), load average 5–16 during the runs |
| RAM / swap | 25,199,173,632 B (24,031 MiB) / 4,294,963,200 B (4 GiB) |
| Disk | 290 GB, 98–100 % used; under 15 MiB free at the worst point |
| `memory.peak` | readable; 38,531,072 B for a 33,554,432 B allocation in a scope |
| `cpu.stat` | readable; `usage_usec` 27,126 → 199,113 for a short CPU burn |
| `memory.high` / `MemoryHigh=` | available and writable; `MemoryHigh=64M` accepted |
| `MemoryHigh` enforcement | 96 MiB allocation under 64 MiB high: 486 `high` events, process stayed alive and exited 0, `oom_kill 0` |
| `memory.peak` reset | **not permitted** for the user on this kernel (`Permission denied`) |
| `/proc` | `/proc/<pid>/stat`, `/proc/<pid>/status`, `/proc/pressure/{cpu,memory}` all present |

`MemoryHigh` is a soft throttle: the kernel slowed the allocation but did not
kill it. That is the behaviour the plan wants for a per-job soft limit. A
single scope's `memory.peak` is cumulative and cannot be reset by the user, so
per-phase peaks need child cgroups (as the spike tool does) or separate scopes.

## Method

One job is shaped like a VOWP worker run: a harness process holds a resident
set while `cargo test --workspace` runs underneath it. The harness is
`fake_harness.py`, a deterministic stand-in that allocates 192 MiB, burns about
0.4 s CPU, and idles until the build finishes. It never calls a model, the
network or a real harness. For reference, the live opencode workers on the
same node measured 93.1 MiB RSS / 96.1 MiB `VmHWM` each at the time, and the
shared `opencode serve` process 662.9 MiB RSS; 192 MiB is a conservative
per-worker harness figure. The real harnesses were not invoked: that would
spend model tokens and break the common rule against touching real harnesses
in tests.

Each measurement run creates a `systemd-run --user --scope` and splits it into
child cgroups:

```text
vowp-cap-<label>.scope
├── main       observer, /proc sampler (excluded from job CPU)
├── harness    fake_harness.py
└── build      cargo/rustc/test binaries, or the cargo shim
```

`memory.peak` and `cpu.stat` are read for the scope and every child. The `/proc`
walk follows `/proc/<pid>/task/<pid>/children`, sums `VmRSS`, and accumulates
`utime`/`stime` deltas. The observer's cgroup is subtracted from the job CPU
and RSS so the two methods compare the same thing.

The three required runs use **host cargo** (`NIGHT_CARGO_HOST=1`), which is the
stock-VPS case: `cargo test` and every rustc/test process are descendants of the
job. The normal night-shift shim runs cargo in a Docker container in
`system.slice`, outside the job scope; that case is measured separately below.

Because the host disk was nearly full, every build used:

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
```

This keeps the target at about 1.7 GB instead of the 9–10 GB default-debug
targets observed on this host. It lowers the absolute build peak and build
time, but does not change the cgroup-vs-`/proc` conclusion. The target was
cleaned before run 1 so the first run is a cold build; runs 2 and 3 are warm.

## Results — three host-local jobs

Commands (run 2026-11-10 11:50 UTC):

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  ./scripts/spikes/capacity/run_three.sh --mode host --runs 3 \
  --out /tmp/opencode/cap-results/20261110T115027Z-host-coldwarm
# exit 0
```

| Run | Build | Duration | Job peak (cgroup) | Job parts | Anon peak | Harness peak | Build peak | `/proc` peak | `/proc`/cgroup | Job CPU | Harness CPU | Build CPU | Avg cores |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| host-1 | cold | 114.1 s | 2452.5 MiB | 2410.6 MiB | 1030.4 MiB | 197.7 MiB | 2246.0 MiB | 2389.2 MiB | 0.974 | 299.77 s | 1.30 s | 298.42 s | 2.63 |
| host-2 | warm | 64.5 s | 369.0 MiB | 355.1 MiB | 344.0 MiB | 197.7 MiB | 162.6 MiB | 465.9 MiB | 1.262 | 60.21 s | 0.95 s | 59.18 s | 0.93 |
| host-3 | warm | 63.8 s | 365.9 MiB | 348.5 MiB | 340.3 MiB | 197.8 MiB | 159.5 MiB | 462.1 MiB | 1.263 | 56.17 s | 0.91 s | 55.21 s | 0.88 |

All three exited 0. "Job peak (cgroup)" is the scope `memory.peak` (includes
the ~12 MiB observer). "Job parts" is the sampled sum of the harness + build
child cgroups, which is the cleaner job figure. "Anon peak" excludes
reclaimable page cache. The build used 91.6 % of cold peak RAM and 99.6 % of
cold CPU; on warm runs it used 43.6–44.1 % of peak RAM and 98.0–98.3 % of CPU.

The `/proc` comparison is informative:

- **Cold build:** `/proc` 2389.2 MiB vs cgroup 2452.5 MiB (0.974). The 100 ms
  sampler happened to be close because the build processes were large and
  mostly private.
- **Warm build:** `/proc` 462–466 MiB vs cgroup 366–369 MiB (1.262–1.263). The
  tree walk sums shared pages once per process, so test binaries and shared
  libraries inflate RSS by about 31 %.
- **CPU:** `/proc` 255.8 s vs cgroup 299.8 s on the cold run (0.853), and
  35.9–38.3 s vs 56.2–60.2 s warm (0.636–0.639). The delta comes from
  processes that exit between samples; cgroup `cpu.stat` is exact.

## Results — the night-shift shim (Docker)

The normal shim is not what a stock VPS runs, but it is what this build host
runs, and it exposes a real blind spot. Two cold attempts were measured; both
completed the build and were measured, but their test suites failed under the
shared load/disk pressure:

| Run | Duration | Scope peak (host side) | Container peak | Combined peak | Harness peak | Build CPU | `/proc` host only | `/proc` + container | Test exit |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| shim-1 | 120.9 s | 228.8 MiB | 2128.1 MiB | 2323.2 MiB | 197.9 MiB | 243.93 s | 239.2 MiB | 2467.5 MiB | 101 (flaky `drill_failover_and_probe_back`) |
| shim-2 | 82.6 s | 229.4 MiB | 2165.6 MiB | 2363.5 MiB | 197.9 MiB | 205.03 s | 239.8 MiB | 2593.0 MiB | 101 (`No space left on device` in `session.rs`) |

The **host-only `/proc` walk sees 239 MiB and misses > 90 % of the build**,
because Docker placed the container in `system.slice/docker-<id>.scope`. The
cgroup method only recovers it because the spike explicitly follows the shim's
`vowp.shimpid=<pid>` label and reads the container cgroup. Plain
`systemd-run --scope` around `cargo` would not see it either. This is a
build-environment artifact, not a stock-VPS requirement, but it is a warning
for E1: if command gates ever run in a container, the container cgroup must be
part of the job scope (or the job must use `--cgroup-parent`).

The second failure is also the disk evidence: the build had completed, then
three `vowp-cli` session tests failed with
`No space left on device (os error 28)` and `sqlite: disk I/O error`. A0 hit the
same condition in its merge check. Capacity admission that ignores disk will
let the node reach this state.

## What this means for E1 / E2

- **Sampler:** sample host `MemAvailable`, swap, load, CPU quota, disk free and
  pressure every 5 s. For per-job usage, start the adapter run and its command
  gates in one transient user scope; read `memory.peak` and `cpu.stat` when the
  scope ends. No 5 s host sample can reconstruct a per-job peak.
- **User-manager environment:** `systemd-run --user` needs the user manager's
  `XDG_RUNTIME_DIR` and `DBUS_SESSION_BUS_ADDRESS`. The probe ran with
  `XDG_RUNTIME_DIR=/run/user/1001` and
  `DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1001/bus` from the login
  session, and `Linger=yes` keeps the manager alive. VOWP's supervisor is a
  systemd user unit and has the same environment; a bare cron/SSH path must set
  them (or go through `systemctl --user`).
- **Store** per run: peak RAM (`memory.peak`), `anon`/`file`/`kernel` at peak
  if sampled, CPU usec (`usage_usec`), duration, exit code, and a
  cold/warm/incremental label if it can be inferred. Store the scope method
  (`cgroup` or `proc`) so estimates do not mix exact and fallback data.
- **Fallback:** if user scopes are unavailable, walk `AdapterRun::pid()` and
  its process group descendants at 50–100 ms, sum `VmRSS`, accumulate CPU
  deltas of exiting pids, exclude the observer, and mark the number
  `confidence: low`. Do not let a container silently drop the build.
- **Estimates:** group by project, job kind, harness, model and build mode.
  Warm Rust gate runs are around 0.35–0.40 GiB and ~1 core; a cold build is
  around 2.4 GiB and ~2.6 cores average. The plan's 1.5 GiB / 1 CPU default is
  a reasonable first-day value but will not cover a cold build; p90 from real
  runs should replace it quickly.
- **Admission:** keep the 5 GB / 10 % disk reserve and the RAM reserve. Add a
  per-job target-dir budget/cleanup (the supervisor already removes a job's
  target after merge), because five 1.7–10 GB targets can fill a 290 GB disk
  before RAM becomes the limit.
- **`MemoryHigh`:** available and safe as a soft limit; it throttled a 96 MiB
  allocation under 64 MiB high without killing it. Do not use it as a hard
  cap on builds until estimates exist. `MemoryMax` is also writable, but the
  spike did not use it because killing a build is worse than throttling it.
- **macOS / no `/proc`:** keep the fixed-target degradation from the plan.
  Nothing here is needed on macOS; the scripts are Linux-only and are not
  compiled into the product.

## Reproduction

```sh
# capability probe
python3 scripts/spikes/capacity/probe.py --json /tmp/capacity-probe.json

# hermetic tooling test
sh scripts/spikes/capacity/tests/run.sh

# three host jobs (cold then warm)
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  ./scripts/spikes/capacity/run_three.sh --mode host --runs 3 \
  --out /tmp/opencode/cap-results/host

# three shim jobs, if the disk has room for the Docker build
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  ./scripts/spikes/capacity/run_three.sh --mode shim --runs 3 \
  --out /tmp/opencode/cap-results/shim
```

The scripts create a private `TMUX_TMPDIR`, a temporary `VOWP_HOME`, and unset
`TMUX`/`TMUX_PANE`. They never start tmux, a real harness, or a network client,
and they never touch `~/.vowp`.

## Caveats

- The node was busy with five concurrent jobs (load average 5–16 on 8 CPUs), so
  the absolute durations and page-cache peaks are not a quiet benchmark. The
  relative comparison and the capability results are stable across runs.
- The harness is a deterministic stand-in; the real opencode worker reference
  above is 93 MiB RSS, so 192 MiB is conservative. A real Claude Code process
  can differ; E1 will collect real per-run data.
- Debug info was disabled for the measured builds to fit the disk. A default
  debug build is larger and somewhat slower. The plan's estimates should be
  based on the product's real profile.
- `memory.peak` includes page cache. On the cold container build the cgroup
  reported 1.66 GB of file pages. If E2's admission is too conservative with
  `memory.peak`, use `anon + kernel` (sampled) for the estimate and keep
  `memory.peak` as the safety ceiling.
