# VOWP v0.1 implementation spec — index

This directory turns the approved design (`plan.md`) into a spec precise enough to build each crate without inventing architecture.
`plan.md` is the product decision record and is **not** edited; every deviation or clarification is in `decisions.md`.

## How to use it

**If you are a build worker** (an AI agent with one job): your brief names the spec sections you implement. Read, in this order:

1. `AGENTS.md` at the repo root (rules), then `conventions.md` (Rust rules, dependency list, test rules).
2. `types.md` for every type you touch — types are defined **once**, in `vowp-protocol`; never redefine them.
3. Your crate's file in `crates/` — module layout, the complete public API with exact signatures, behaviour rules, and the **named tests** you must implement.
4. The behavioural document your crate is part of (`adapters.md`, `loops.md`, `sessions.md`, `cli.md`) and `schema.sql` if you touch the database.
5. Your job's entry in `job-map.md` (what you own, what you need, acceptance tests).

Names in the spec are binding: same type, table, column, command and test names. If the spec is ambiguous or wrong, write the question under
`questions_for_owner` in your report (status `blocked` or `partial`); do not invent a different design.

**If you are a human contributor**: start with `architecture.md`, then the crate file for the area you want to change; update the spec in the same PR as the code.

## Files

| File | What it defines |
|---|---|
| `plan.md` | The approved product plan (read-only here). |
| `decisions.md` | Deviations/clarifications relative to the plan, each with its reason; open owner questions. |
| `architecture.md` | Components and data flow, threading model, on-disk layout of a node, the full `vowp.toml` schema with an annotated example, error/logging conventions, security model. |
| `schema.sql` | The complete SQLite schema (19 tables; plus SQLite's internal `sqlite_sequence`) with constraints, indexes and comments; WAL; migration by `PRAGMA user_version`. Loads in plain SQLite. |
| `types.md` | Every protocol type as Rust with serde attributes and JSON examples: ids, timestamps, events + payloads, capsule, job/edge/state, gates/evidence, brief & report front-matter, executor/profile, limit, cost, run, session, inbox, loop, schedule, release, config, adapter and service traits. |
| `conventions.md` | Rust conventions, per-crate dependency list, test and fixture rules, naming. |
| `crates/vowp-protocol.md` | Crate API beyond `types.md`: modules, helper traits (`Log`, `FakeClock`), tests. |
| `crates/vowp-store.md` | SQLite access, session logs, blobs, reindex, backup. |
| `crates/vowp-graph.md` | DAG, state machine, ownership overlap, dispatch planning, rendering. |
| `crates/vowp-tmux.md` | Safe tmux window manager and its in-memory fake. |
| `crates/vowp-adapters.md` | Claude Code, opencode and generic cli adapters; classification; process helpers. |
| `crates/vowp-core.md` | `session`, `loops`, `gates`, `ledger`, `schedules`, `promote` modules. |
| `crates/vowp-cli.md` | The `vowp` binary: module layout, context, output, exit codes, remote execution. |
| `adapters.md` | Adapter behaviour: claude-code and opencode mappings, cli mapping-file format, limit/credit/auth/context classification, cost receipts, fixtures. |
| `loops.md` | L1–L9, the job state-machine transition table, gates, merge queue, failover, inbox, watchdog, schedules, drills. |
| `sessions.md` | Canonical event log, reindex, capsule writer, switch protocol, export/import bundle. |
| `cli.md` | Every v0.1 command: arguments, flags, human and `--json` output, exit codes, `--node` over ssh. |
| `job-map.md` | The build plan: concrete jobs (id, title, owns, needs, spec slice, acceptance tests), ordered must-haves first. |

## Vocabulary (one meaning everywhere)

*Node* (a machine) · *project* (a repo homed on a node) · *job* (graph node) · *edge* (`needs|verifies|fixes|escalates|blocks`) · *gate* (checkable condition + evidence) ·
*run* (one adapter process) · *session* (universal conversation, event log) · *event* (log line) · *capsule* (handoff) · *switch* (executor change) · *executor* (`harness:model@effort#variant`) ·
*profile* (role's ladder of executors) · *limit* (`quota|credits|rate|context|auth`) · *loop* (L1–L9) · *schedule* (L9 row) · *ledger* (append-only record) · *inbox* (human questions) · *release* (promotion).

## Status of the spec

v0.1 only. Everything marked **v0.2** (HTTP+SSE daemon, web window, ACP adapter, cross-node sync, IANA timezones) is deliberately not specified beyond the hooks noted.
