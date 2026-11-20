---
job: ss-87-searchable-model-picker-and-manage-models
status: blocked
branch: job/ss-87-searchable-model-picker-and-manage-models
commits:
  - 3c74388
tests:
  typecheck: pass
  lint: pass
  unit (npm test): pass (ui 364, desktop 10, web 73)
  e2e (npm run test:e2e -w @stackspirit/desktop): 37 passed, 2 failed (meadow-chat-retry.spec.ts, outside `owns`)
---

# ss-87 searchable model picker and Manage models

## What landed
- `ComposerBar.tsx`: the picker is now a `role="dialog"` popover (`model-picker-popover`) with an auto-focused `type="search"` field (`composer-model-search`), a bounded `listbox` of `option` rows, and a sticky footer. Search is case-insensitive and multi-word across label, id, provider id and provider label. `ArrowUp`/`ArrowDown` wrap, `Enter` selects, `Escape` closes and refocuses `composer-model`. The empty state reads `No models match “<query>”.`.
- `groupChatModels` now returns OpenCode Free first (`Free` badge), then one section per provider (popular-first via a TS copy of `POPULAR_PROVIDER_IDS`, then alphabetical). Each section has a `Connected` badge and a count. Rows have a `✓` on the active model, a clean label with no `BYOK ·` prefix, a secondary id, and a badge.
- **Manage models** (`composer-manage-models`) and **Connect another provider…** (`composer-connect-provider`) both close the picker and open the in-place `Engine & Models` modal. `onOpenEngineSettings` is not used. A unit test asserts this.
- `theme.css`: Meadow-token styling for the popover, search field (border-step focus), scroll area, sections, rows, badges, empty state and sticky footer. Tokens are the existing light/dark ones.
- `MockBridge.ts`: connecting OpenRouter, Anthropic or OpenAI in preview mode unlocks realistic multi-model catalogs.
- Tests: composer, providerSettings, retryRecovery, meadow-provider-connect, next-shell. Axe WCAG 2.2 AA passes in light and dark for the open picker, a filtered list and the empty state.
- Docs: `HANDOFF-DESKTOP-UI.md` and `ORCHESTRATOR-CHAT.md` updated.

## Blocker
`apps/spirit-desktop/e2e/meadow-chat-retry.spec.ts` is not in `owns` but still uses the old selectors: `getByRole("menu", { name: "Choose a model" })`, `menuitemradio`, `BYOK · DeepSeek V4.1 Flash`, and focus on the trigger. Its two tests (light and dark) fail the e2e gate. The brief's requirements (no `BYOK ·` prefix, dialog/listbox semantics, auto-focused search) make the old selectors impossible to keep. I did not edit it.

Suggested fix (mirrors `retryRecovery.test.tsx`): `getByTestId("model-picker-popover")`, `getByRole("option", { name: /Big Pickle/ })` and `/DeepSeek V4\.1 Flash(?! Free)/`, and expect `composer-model-search` to be focused.

## questions_for_owner
- May `meadow-chat-retry.spec.ts` be edited as above, either in this job (add it to `owns`) or in a small follow-up?

## Notes
- The `review`, `owns_respected` and `secret_scan` gates were not run by me. All changed paths are in `owns`.
- `npm ci` was needed in the worktree.
