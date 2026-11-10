# S1 measurement results

These CSV files are the compact evidence behind
`docs/maintainers/v0.2-spikes/capacity.md`. They were generated on 2026-11-10
on the VPS build node. The raw JSON and build logs live outside the repository
because they contain local temporary/scope paths; the CSVs contain only the
measured numbers.

| File | What it is |
|---|---|
| `2026-11-10-host-coldwarm.csv` | Three host-local jobs: one cold `cargo test --workspace` build and two warm runs, each with the cgroup and `/proc` methods. |
| `2026-11-10-shim-coldwarm.csv` | Two cold attempts through the normal night-shift cargo shim (Docker). Both builds completed and were measured; their test suites exited 101 (one flaky test, one `No space left on device`). |

Columns:

- `job_peak_mib` / `job_parts_mib` — conservative scope peak and the sampled
  harness+build component peak.
- `job_anon_mib` — sampled process anon peak, without page cache.
- `harness_peak_mib` / `build_peak_mib` — exact child cgroup `memory.peak`
  (build = the Docker container in the shim file).
- `proc_peak_mib` — `/proc` tree-walk peak; compare with `proc_over_cgroup`.
- `job_cpu_s` — exact `cpu.stat` minus the observer cgroup.
- For the shim files, `proc_host_mib` is the host tree alone and
  `proc_container_mib` is the container tree; a plain `/proc` fallback can only
  see the former.

Reproduce with `scripts/spikes/capacity/run_three.sh`; see the README in the
parent directory and the spike document for the exact commands and the
`CARGO_PROFILE_*_DEBUG=0` / `CARGO_INCREMENTAL=0` overrides used because the
host disk was nearly full.
