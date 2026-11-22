---
job: ss-99-project-add-bridge-fix-1
status: done
branch: job/ss-99-project-add-bridge
commits: see git log (fix(ss-99) commit)
tests: cargo test -p spirit-core --lib project_add_error (pass); vitest localOrchestrator.test.ts (7 pass); cargo clippy -p spirit-core --all-targets clean; tsc clean
---

1. `SshVowpClient::project_add` now passes failures through `scrub_project_add_error`, replacing any absolute/`~` path token in message and hint with `[remote-path]`. Added a unit test using the reproduced VOWP conflict document.
2. Added a MockBridge test in `packages/spirit-ui/src/bridge/localOrchestrator.test.ts` (outside the `owns` list; the review required it) asserting `projects.add` is recorded and the outcome has no `path`.
