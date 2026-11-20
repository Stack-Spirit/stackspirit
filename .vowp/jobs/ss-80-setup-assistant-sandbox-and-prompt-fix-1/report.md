---
job: ss-80-setup-assistant-sandbox-and-prompt-fix-1
status: done
branch: job/ss-80-setup-assistant-sandbox-and-prompt
commits: [00cb2e3]
tests: cargo test -p spirit-core --test setup_sandbox --test chat (pass); cargo clippy -p spirit-core --all-targets -D warnings (pass)
---
1. With OPENCODE_DISABLE_PROJECT_CONFIG=1 the engine only reads $OPENCODE_CONFIG_DIR/AGENTS.md, so PrivateEngineTree::prepare now writes AGENTS.md (0600, atomic) to engine/config/opencode. The workspace copy is gone. docs/ORCHESTRATOR-CHAT.md and the chat.rs/setup_sandbox.rs path checks are updated.
2. New test live_engine_sends_the_setup_prompt_to_the_model runs the bundled sidecar's `opencode run` against a loopback mock provider and asserts the model request contains the StackSpirit, "+ Connect a new server" and "Form a cluster" text. It skips if the sidecar isn't fetched. I confirmed it fails (60 s timeout, prompt missing) when the file is put back in the workspace.
