---
job: ss-116b-real-orch-openrouter-v2-fix-2
status: done
branch: job/ss-116b-real-orch-openrouter-v2
commits: ["3473d317ff1470ac0cbb10d41f6c3c5a7fde6c79"]
tests: "scripts/cargo-gate.sh test -p spirit-core --test setup_sandbox (4 pass)"
---

# Fix round 2

setup_sandbox.rs is now in owns; re-applied the test updates from 2ce2f69 (no global AGENTS.md, setup prompt asserted via SETUP_SYSTEM_PROMPT, live-engine test asserts the setup prompt is not loaded globally). The setup_sandbox test target passes.
