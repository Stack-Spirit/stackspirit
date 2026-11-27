# Documentation index

This directory holds StackSpirit's product, desktop, contract and release documents. Start with
the root [`README.md`](../README.md) for install and development, then use the sections below. The
released runtime — engine, `stackspirit` CLI, daemons and MCP — has its own docs under
[`runtime/`](../runtime/README.md); this page only indexes the entry points.

The sections below cover the current entry points and keep historical material separate.
[`PLAN.md`](PLAN.md) and the runtime `docs/design/` tree are historical records, not current
contracts.

## Start here

- [`README.md`](../README.md) — install, surfaces, repository layout, gates and status limits.
- [`STRATEGY.md`](STRATEGY.md) — locked product, architecture and roadmap decisions (2026-11-25).
- [`DEV-SETUP.md`](DEV-SETUP.md) — non-root Linux Docker container for the desktop host and UI build
  gates.

## Desktop & UI

- [`HANDOFF-DESKTOP-UI.md`](HANDOFF-DESKTOP-UI.md) — design source of truth for the desktop UI and
  brand: owner decisions, real versus preview, next steps.
- [`HARNESSES.md`](HARNESSES.md) — Universal Harness Registry: presets, protocols, auth modes and
  one-click connect.
- [`ORCHESTRATOR-CHAT.md`](ORCHESTRATOR-CHAT.md) — orchestrator chat contract: bundled OpenCode
  server, VOWP endpoint, non-streaming caveat and Server/Local gates.
- [`WEB.md`](WEB.md) — Web/PWA contract and route rules; the W1 pre-build contract is historical,
  with the current 0.6.1 state in §11.
- [`CLUSTER.md`](CLUSTER.md) — pooled compute and Home promotion: typed cluster model, VOWP
  commands and desktop UI.
- [`SERVER-CHAT-OPTIONS.md`](SERVER-CHAT-OPTIONS.md) — decision note on the Server chat transport;
  options and recommendation, not an implementation.

## Runtime & contracts

- [`MCP.md`](MCP.md) — `stackspirit mcp` stdio server, fleet tools, dashboard widget, local proxy
  and safety.
- [`VOWP-CONTRACT.md`](VOWP-CONTRACT.md) — frozen read contract for the VOWP commands, JSON
  documents and view-model mapping the desktop depends on.
- [`D6-READINESS.md`](D6-READINESS.md) — D6 local-orchestrator readiness and gate evidence; live
  execution remains owner-gated.
- [`runtime/README.md`](../runtime/README.md) — runtime workspace: engine, CLI, daemons, MCP and
  packaging.
- [`runtime/docs/roadmap.md`](../runtime/docs/roadmap.md) — runtime roadmap: shipped releases,
  implemented-but-unreleased slices and open candidates.
- [`runtime/docs/reference/`](../runtime/docs/reference/) — generated CLI and JSON Schema reference
  for the runtime binary.
- [`runtime/docs/troubleshooting.md`](../runtime/docs/troubleshooting.md) — diagnosing stuck jobs,
  capacity, provider limits and home problems from recorded evidence.
- [`runtime/docs/security.md`](../runtime/docs/security.md) — runtime security model: the five
  limits, secret injection and what it does not protect.

## Release & verification

- [`PUBLIC-READINESS.md`](PUBLIC-READINESS.md) — public-readiness audit, history secret scan and
  release state.
- [`RELEASE-DESKTOP-MVP.md`](RELEASE-DESKTOP-MVP.md) — D1–D4 desktop MVP, W1 read-only Web shell and
  C1 `spirit` CLI closeout note.
- [`RELEASE-D5-ASIF.md`](RELEASE-D5-ASIF.md) — D5 ASIF export/import closeout note; a review draft
  with declared losses, not a conformance claim.
- [`RELEASE-D6.md`](RELEASE-D6.md) — D6 local-orchestrator closeout note, covering the gated paths
  only.
- [`OWNER-VERIFICATION.md`](OWNER-VERIFICATION.md) — owner-run macOS/Linux/Windows checklist,
  evidence template and sign-off rule.
- [`runtime/docs/release/0.6.1.md`](../runtime/docs/release/0.6.1.md) — 0.6.1 patch release note:
  PyPI metadata, package URLs, desktop UI and readiness changes.

## History

- [`PLAN.md`](PLAN.md) — historical D1–D4 engineering baseline (2026-11-14); superseded for current
  decisions by `STRATEGY.md` and `HANDOFF-DESKTOP-UI.md`.
- [`runtime/docs/design/`](../runtime/docs/design/) — design records for the VOWP engine
  (architecture, schema, types and decisions), not current contracts.
