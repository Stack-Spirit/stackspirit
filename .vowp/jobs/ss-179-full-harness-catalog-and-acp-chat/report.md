---
job: ss-179-full-harness-catalog-and-acp-chat
status: done
branch: job/ss-179-full-harness-catalog-and-acp-chat
commits: ["4ccd53f", "afeb4db"]
tests: "cargo fmt --all -- --check; cargo clippy -p spirit-core --all-targets -- -D warnings; cargo test -p spirit-core (all suites pass, including 14 harnesses integration tests and 5 ACP mock-process tests); npm run typecheck -w @stackspirit/ui; npm run lint -w @stackspirit/ui; npm test -w @stackspirit/ui (63 files, 584 tests); bash scripts/check-public-readiness.sh; node scripts/check-doc-links.mjs"
questions_for_owner: []
---

# Full harness catalog and ACP stdio client

## What landed

### Explicit surface split (`crates/spirit-core/src/harnesses/manifest.rs`)

- New `HarnessSurface` enum with `OrchestratorAndWorkers` and
  `WorkersOnlyTerminal`, wire values `orchestrator_and_workers` and
  `workers_only_terminal`.
- `HarnessManifest.surface` is authoritative. `supports_embedded_chat` remains the
  legacy mirror and validation rejects a manifest where the two disagree.
- `cli-pty` is rejected for an orchestrator surface, so a terminal protocol can never
  claim the embedded chat.
- `HarnessManifest::embedded_chat()` reads the surface.
- `HarnessProbe` now carries the surface alongside the protocol.

### Built-in catalog (25 presets)

OpenCode stays first and default (`DEFAULT_HARNESS_ID`, `OrchestratorAndWorkers`, BYOK).

**Orchestrator + Workers (ACP / Embedded Chat)** — 19:
`opencode`, `cursor-cli`, `github-copilot`, `auggie`, `kiro`, `goose`, `codex`, `cline`,
`kilo`, `qwen-code`, `mistral-vibe`, `kimi-code`, `minimax-code`, `glm-agent`, `pi-acp`,
`devin-cli`, `factory-droid`, `grok-build`, `junie`.

**Workers Only (Stock CLI Terminal View)** — 6:
`claude-code`, `gemini-cli`, `antigravity`, `aider`, `crush`, `amp`.

Claude Code and Gemini CLI keep their structured worker protocols but are deliberately
not embedded-chat surfaces. Their consumer OAuth sign-in stays in their own CLI.

### ACP client (`crates/spirit-core/src/harnesses/acp.rs`)

- Newline-delimited JSON-RPC 2.0 over any `AsyncRead`/`AsyncWrite` pair; `spawn` owns a
  real child with piped stdin/stdout and `kill_on_drop`, `from_io` is the hermetic seam.
- `initialize` (protocol version 1, no `fs/*` or `terminal/*` client capabilities),
  `session/new`, `session/prompt`, streaming `session/update` notifications,
  `session/request_permission` with `allow_once` / `allow_always` / `reject_once` /
  `reject_always` / cancel mapping, and `session/cancel`.
- Typed prompt/update/permission DTOs; unknown update kinds stay `Unknown` instead of
  being guessed as text. Missing permission options cancel rather than auto-approve.
- Bounded 1 MiB lines, 30 s request/read deadline and 10-minute prompt deadline. A
  timed-out prompt reaps the child instead of reusing a desynchronized stream. Raw
  payloads are not echoed in errors.
- `initialize` keeps only safe auth-method ids; no credential file is read and no
  provider OAuth token is extracted, stored or logged.
- Five deterministic mock-process tests drive the full handshake, a streamed turn with
  a permission request, JSON-RPC errors, malformed lines, cancel framing and the
  missing-option cancellation path. `crates/spirit-core/tests/harnesses.rs` also drives
  the public handshake end to end.

### TOML and store

- `surface` is written by `manifest_to_toml` and accepted by the strict parser.
- Older custom files that omit `surface` derive it from `supports_embedded_chat`
  instead of breaking; conflicting fields and an unknown surface are rejected.

### UI (`packages/spirit-ui/src/next/HarnessesPanel.tsx`)

- OpenCode remains pinned at the top with its collapsible fixed-height scrollable
  Providers dropdown.
- Remaining built-ins and custom manifests are grouped under
  **Orchestrator + Workers (ACP / Embedded Chat)** and
  **Workers Only (Stock CLI Terminal View)**, with surface, capability, protocol and
  built-in/custom badges on every card.
- The `+` form picks the surface first (`acp` for Orchestrator + Workers, `cli-pty` for
  Workers Only) and keeps the protocol select in sync, so an invalid combination cannot
  be submitted.
- `wire.ts` is outside `owns`, so the panel uses a local optional widening of the wire
  manifest and falls back to `supports_embedded_chat` when an older bridge omits
  `surface`.

### Docs and tests

- `docs/HARNESSES.md` now records the surface contract, both catalog tables, the
  `surface` TOML field and its compatibility rule, and the ACP client contract.
- `crates/spirit-core/tests/harnesses.rs` covers the exact 25-id catalog and surfaces,
  surface validation, TOML round-trip, and the public ACP handshake/permission path.
- `packages/spirit-ui/src/test/next/harnessesPanel.test.tsx` covers the pinned OpenCode
  card, both sections, badges, and surface/protocol sync in the custom form.

## Gates run

- `cargo fmt --all -- --check` — clean.
- `cargo clippy -p spirit-core --all-targets -- -D warnings` — clean.
- `cargo test -p spirit-core` — all suites pass (harnesses: 14 tests; ACP mock-process:
  5 tests).
- `npm run typecheck -w @stackspirit/ui` — clean.
- `npm run lint -w @stackspirit/ui` — clean.
- `npm test -w @stackspirit/ui` — 63 files, 584 tests passed.
- `bash scripts/check-public-readiness.sh` — PASS, no secret, public IP, personal home
  path, tracked `.env` or personal e-mail found.
- `node scripts/check-doc-links.mjs` — PASS, 347 files, all relative links resolve.
- `owns_respected` — every changed path is inside `owns` (8 modified files plus the new
  `acp.rs`, 9 files total).
- `secret_scan` — covered by the public-readiness run above.
- `review` — self-reviewed; no unrelated behavior, UI copy or wire DTO outside `owns`
  was changed.

## Honest limits

- The ACP client is core plumbing only. It is **not** wired into the embedded chat, the
  chat-transport selector or the permission cards; the UI and docs say the embedded chat
  still runs on OpenCode. `docs/HARNESS-CHAT.md` remains the design for that wiring.
- The preview `MockBridge` is outside `owns` and still returns its eight historical
  presets. The panel falls back to `supports_embedded_chat` for those, so preview keeps
  working; the full 25-preset catalog is the real desktop host path.
- Install commands and several sign-in commands are display-only hints for the new
  catalog entries. Empty `auth_cmd` values open the harness documentation instead of
  inventing a CLI flag, as before.
- No Playwright/e2e run was part of this brief's gates. The existing preview e2e badge
  selectors still find `Embedded Chat` / `Worker-Only` because both capability badges are
  retained beside the new surface badges.
