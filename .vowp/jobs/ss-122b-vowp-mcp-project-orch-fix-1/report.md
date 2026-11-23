---
job: ss-122b-vowp-mcp-project-orch-fix-1
status: done
branch: job/ss-122b-vowp-mcp-project-orch
commits: ["1061d8d"]
tests: "cargo-gate.sh test -p spirit-core: all suites ok (chat 29/29); chat suite run 25x with no failures"
---

# Fix round 1

## Problem
`fallback_registers_the_vowp_mcp_and_carries_the_session_in_every_prompt` failed intermittently at `chat.rs:1961` ("a message request"). It was a test race, not a product bug. `LocalChatProvider::send` returns before the `/message` POST is issued, so the `send_and_read_system` helper could read the recorded requests before the POST was logged.

## Fix
In `send_and_read_system` (tests/chat.rs only), call `wait_until` for the message request count to exceed the previous count before reading the body. No production code changed.

## Verification
Before the fix the chat suite failed 2 of 15 runs. After it, 25 of 25 runs passed, and the full `spirit-core` test gate passes.
