---
job: ss-99-project-add-bridge-fix-1
status: done
branch: job/ss-99-project-add-bridge
commits: see git log on job/ss-99-project-add-bridge (fix(ss-99) commits)
tests: scripts/cargo-gate.sh fmt --all -- --check pass; scripts/cargo-gate.sh clippy --workspace --all-targets -- -D warnings pass; scripts/cargo-gate.sh test -p spirit-core 433 passed, 0 failed; npm run typecheck pass; npm run lint pass; npm test 393 UI + 10 desktop + 73 web passed, 0 failed
---

1. `SshVowpClient::project_add` now maps every failure from `check_output` through
   `scrub_project_add_error`. It replaces absolute (`/...`) and `~`/`~/...` path tokens in the
   message and hint with `[remote-path]`. This covers the reproduced VOWP 0.5.1 conflict
   document (`clone target /tmp/opencode/ss99-verify/demo-app already exists`, exit 4) and the
   stderr fallback (`check_command_output` only keeps the first stderr line). The success path
   still drops `path` in `parse_project_bootstrap`.
   Added `vowp_project_add_errors_do_not_leak_the_clone_target` to the owned
   `crates/spirit-core/tests/projects.rs`: it drives `SshVowpClient::project_add` with the exact
   reproduced error document and asserts the code is `conflict`, the message/hint contain no
   clone-target path, the message keeps `[remote-path] already exists`, and the serialized
   `SpiritError` contains no path. The temporary inline helper test from the first attempt was
   folded into this end-to-end test.
2. Added the required MockBridge test in
   `packages/spirit-ui/src/bridge/localOrchestrator.test.ts` (the only TS test file touched;
   `owns` has no TS test path, as the review noted). It calls `bridge.projects.add({...})`,
   asserts `recordedProjectAdds()` equals the input, and asserts the serialized outcome has no
   `path` key. This raises the UI suite from 392 to 393 tests.

Also fixed the rustfmt failure in the first fix attempt (`cargo fmt --all -- --check` had flagged
the wrapped `map_err` chain), so the full gate set now passes as listed above.
