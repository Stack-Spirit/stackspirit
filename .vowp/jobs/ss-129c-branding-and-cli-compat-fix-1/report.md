---
job: ss-129c-branding-and-cli-compat-fix-1
status: done
branch: job/ss-129c-branding-and-cli-compat
commits: ["c16b514"]
tests: "cargo test -p spirit-core: all suites pass; cargo clippy -p spirit-core --all-targets: clean; cargo fmt applied"
---

# Fix round 1

## Problem
`run_remote` in `engine/gate.rs` sent `stackspirit` unconditionally, so legacy `vowp`-only hosts exited 127 for every gate probe and the capability gates stayed disabled.

## Fix
`run_remote` now rewrites the engine argv[0] to `engine_binary_for(destination)`. If the preferred `stackspirit` exits 127, it retries with `vowp` and records it via `set_engine_binary_for`, matching `SshRemote::run_engine_aware`. No new unit test was added for the fallback path.
