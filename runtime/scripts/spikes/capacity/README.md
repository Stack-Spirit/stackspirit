# scripts/spikes/capacity — S1 capacity measurement spike

Internal spike tooling for the VOWP v0.2 capacity manager. It answers the
questions in `VOWP-V2-PLAN.md` §6.1 and the S1 card:

- can a job run in a `systemd-run --user --scope` cgroup on this node?
- are `memory.peak`, `cpu.stat` and `MemoryHigh` available and usable?
- how do per-job cgroup numbers compare with the `/proc` tree-walk fallback?
- how much of a worker job is the harness and how much is the build?

The tools are deliberately separate from the product crates. Nothing here is
compiled into `vowp`; E1 should implement the chosen method in Rust.

## Files

| File | Purpose |
|---|---|
| `probe.py` | Read-only cgroup/`/proc` capability probe. Prints a table and optional JSON. |
| `job_measure.py` | Runs one harness+build job and measures it with both methods. |
| `fake_harness.py` | Deterministic harness stand-in (fixed RSS/CPU, no network, no real harness). |
| `run_one.sh` | Runs a command in a transient systemd user scope when available, otherwise directly. |
| `run_three.sh` | Runs N jobs in `host` or `shim` mode with a private `TMUX_TMPDIR` and temporary `VOWP_HOME`. |
| `summarize.py` | Turns measurement JSON into `summary.md` / `summary.csv`. |
| `tests/run.sh` | Hermetic self-test: probe, cgroup job, `/proc`-only fallback. |

## Safety

- No network, no real `claude`/`opencode`, no model tokens.
- No default tmux server. `run_three.sh` creates a private `TMUX_TMPDIR`,
  sets a temporary `VOWP_HOME`, and unsets `TMUX`/`TMUX_PANE`.
- `probe.py` only creates short-lived `--collect` user scopes.
- Nothing touches `~/.vowp`, and no measured process is killed except a
  harness the script itself started.

## Usage

Probe the node:

```sh
python3 scripts/spikes/capacity/probe.py --json /tmp/capacity-probe.json
```

Self-test the tooling:

```sh
sh scripts/spikes/capacity/tests/run.sh
```

Measure three host-local jobs (the stock-VPS case; `cargo test` sees the whole
build tree):

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  ./scripts/spikes/capacity/run_three.sh --mode host --runs 3 \
  --out /tmp/capacity-host
```

Measure the night-shift shim (the build runs in a Docker container):

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  ./scripts/spikes/capacity/run_three.sh --mode shim --runs 3 \
  --out /tmp/capacity-shim
```

The measurements in `docs/maintainers/v0.2-spikes/capacity.md` used the
`CARGO_PROFILE_*_DEBUG=0` and `CARGO_INCREMENTAL=0` overrides because the host
disk was 98–100 % full. Debug-info generation does not change the conclusion,
but it does change the absolute build peak; default-debug builds are larger.

## What the two methods do

`host` mode runs `cargo test --workspace` directly on the host. The top-level
scope is split into child cgroups:

```text
vowp-cap-<label>.scope
├── main       the observer and /proc sampler (excluded from job CPU)
├── harness    fake_harness.py
└── build      cargo, rustc, test binaries, shim
```

`memory.peak` and `cpu.stat` are read per child and for the scope. The `/proc`
walk follows `/proc/<pid>/task/<pid>/children`, sums `VmRSS`, and accumulates
`utime`/`stime` deltas. The observer is excluded from job RSS/CPU so the two
methods are comparable.

`shim` mode uses the normal night-shift `cargo` shim. Docker puts the build in
`system.slice/docker-<id>.scope`, outside the user scope, so the tool follows
that container cgroup by its `vowp.shimpid=<pid>` label. The `/proc` walk is
reported both for the host tree alone and with the container tree, because a
real `/proc` fallback cannot discover the container by itself.

## Reading the output

- `summary.job_peak_bytes` — conservative cgroup peak. For `host` it is the
  scope `memory.peak` (includes the ~12 MiB observer); for `shim` it is the
  sampled scope+container current peak.
- `summary.job_peak_excluding_observer_bytes` — sampled peak of the job child
  cgroups (`harness` + `build` + container), the cleanest job figure.
- `summary.job_peak_anon_bytes` — sampled peak of process anon memory, without
  reclaimable page cache.
- `summary.harness_peak_bytes`, `summary.build_peak_bytes` — exact child
  cgroup `memory.peak` (build = container `memory.peak` in shim mode).
- `summary.job_cpu_usec` — `cpu.stat` minus the observer cgroup; exact.
- `summary.proc_job_peak_rss_bytes` — `/proc` tree walk; compare with the
  cgroup figure, not with the harness allocation alone.
