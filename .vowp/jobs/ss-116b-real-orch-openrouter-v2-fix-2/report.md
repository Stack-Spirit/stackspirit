---
job: ss-116b-real-orch-openrouter-v2-fix-2
status: blocked
branch: job/ss-116b-real-orch-openrouter-v2
commits: ["52d70e8780edbaed941921bd06cf6df0afdcefad"]
tests: "owns gate fixed; scripts/cargo-gate.sh test -p spirit-core --test setup_sandbox: 2 pass, 2 FAIL (setup_agents_md_describes_real_flows_without_secrets_or_paths, live_engine_sends_the_setup_prompt_to_the_model)"
---

# Fix round 2

Reverted crates/spirit-core/tests/setup_sandbox.rs to main so the diff stays inside owns.

**Conflict needing a decision:** the round-1 fix (setup prompt no longer written to the global AGENTS.md, now SETUP_SYSTEM_PROMPT per Setup session) makes the two old tests in setup_sandbox.rs fail, since they assert the global AGENTS.md exists. Those tests live outside owns. Either add setup_sandbox.rs to owns (the round-1 test edits are in commit 2ce2f69 and can be re-applied), or revert the process.rs behavior change, which would bring back the prompt leak into project sessions.
