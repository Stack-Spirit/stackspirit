---
job: v6-4-stackspirit-mcp-namespace
status: done
branch: job/v6-4-stackspirit-mcp-namespace
commits:
  - 2a4ccd2
tests: "fmt --check, clippy -D warnings, mcp/orchestrator(cli,agent)/acp tests and cargo test --workspace --no-fail-fast all pass (199 suites ok)"
---

# Summary

- `initialize` and `server/discover` report `serverInfo.name = "stackspirit-engine"` (v6-2 had left `"vowp"`, so changed in `mcp/mod.rs`); version unchanged.
- New orchestrators write `mcp.stackspirit` (opencode, core and agent) / `mcpServers.stackspirit` (Claude). An existing `vowp` key in the file is preserved on resume/refresh; no dual registration, no auto re-key. Generated command stays `vowp mcp`.
- Tool names and schemas untouched (15 names asserted).
- Proxy `clientInfo` -> `stackspirit-control`; ACP client name -> `stackspirit`.
- Re-keying an old orchestrator is left to the owner (recreate it or edit the app config, then re-approve permissions).

# Tests added/changed

- `mcp.rs`: serverInfo asserted `stackspirit-engine`; tool count == 15; new `stackspirit_mcp_serves_the_same_identity_tools_and_backend_as_vowp_mcp` (both program names give identical initialize/tools/discover and a successful `fleet_status`; exact sorted 15 names).
- `cli/tests/orchestrator.rs`: new test for new key on create, resume keeping legacy `vowp`, no dual keys, no token in `.mcp.json`; existing assertions moved to the new key.
- `agent/tests/orchestrator.rs`, `adapters/tests/acp.rs`: updated to the new key / client name.

# Commands

- `cargo fmt --all -- --check` — exit 0
- `cargo clippy --workspace --all-targets -- -D warnings` — exit 0 (after moving a helper above the test module)
- `cargo test -p vowp-cli --test mcp` / `--test orchestrator`, `-p vowp-agent --test orchestrator`, `-p vowp-adapters --test acp` — all ok
- `cargo test --workspace --no-fail-fast` — exit 0

# Notes

An earlier plain `cargo test --workspace` failed once on `vowp-agent --test socket cached_read_answers_a_tool_timeout` (timing under load, file not touched by this job); it passed 3/3 alone and in the full rerun.
