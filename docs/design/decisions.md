# decisions.md — deviations and clarifications relative to `plan.md`

`plan.md` stands. This file records every place where the spec adds to, tightens or deliberately differs from it, with the reason.
Anything not listed here follows the plan.

| # | Plan says | Spec does | Reason |
|---|---|---|---|
| D1 | §7.3 states: pending, ready, running, stalled, reviewing, fixing, merging, done, failed, replanned, parked | Adds **`gating`** (command gates running between `running` and `reviewing`) and **`cancelled`** (terminal, human cancel) | The plan's diagram shows gates as a transition label; as a state it makes the loop idempotent across crashes (don't re-run 30-minute gates twice) and lets the board show it. `night.py` already uses it. |
| D2 | §2 "one writer: the supervisor" | Loops are single-threaded writers; **short CLI mutations** (`job add`, `inbox answer`, `schedule add`, …) also write directly in one transaction under WAL with `busy_timeout=5000` | Requiring every CLI call to go through a daemon would need the v0.2 HTTP API. State-machine transitions are only ever performed by the supervisor (CLI `retry`/`park` enqueue a *request row*: `kv` key `req:<ulid>` handled on next tick). |
| D3 | §5.2 opencode "against the background service … `opencode serve` is an option" | v0.1 uses `opencode run --format json` only | Matches the brief; one process per run; `--session` resumes. `serve` is v0.2 (ACP/HTTP). |
| D4 | §9 gate table (report, cmd, owns_respected, pushed, review, script, deploy) | Adds **`secret_scan`** gate kind | §14/AGENTS.md require a secret-scan before every push; making it a gate gives it evidence and a place in the merge queue. |
| D5 | §7.1 `fixes` edge "bounded by `max_attempts`" | Fix jobs run **in the original job's branch/worktree**, have no verify job of their own; the original job goes `fixing → reviewing` again | Keeps one diff to verify; avoids merge conflicts between fix and original. |
| D6 | §8 "L2 … 2 automatic continues" | Continues counted per **attempt** (reset when a fix round starts); a hung-before-first-output run is restarted up to 3 times (`hang_restarts`), separate from continues | From `night.py` experience: opencode sometimes prints nothing for 8 minutes. |
| D7 | §8 stall "20 min with no new events and no new commits" | 20 min default; **8 min if the run has produced no output at all**; both configurable (`[loops.l1-supervisor] stall_minutes`) | Same night.py observation. |
| D8 | §13 `vowp-adapters` has `claude-code, opencode, cli` | The adapter **trait and its data types live in `vowp-protocol`** (types.md §12); `vowp-adapters` holds the implementations and shared classification helpers | The module skeleton (n01) gives each job disjoint files; core must depend on the trait, not on a specific adapter file. |
| D9 | §13 "tokio only where streams need it" | No tokio at all in v0.1 | Every stream is a child-process pipe; threads suffice. |
| D10 | §13 stack lists `cron`, `globset`, `ulid`, `time` | Adds `serde_yaml_ng` (YAML front-matter and graph export), `regex`, `which`-free PATH lookup, `sha2`, `schemars`, `toml` | Report/brief front-matter is YAML; gates use regexes. |
| D11 | §8.1 table list | Adds two auxiliary tables: `kv` (small state, flags, request rows) and `session_natives` (index of harness-side session ids; mirrors `adapters.json`) | Needed for `paused` flag / CLI→supervisor requests, and for fast `--resume` lookups. Both rebuildable or trivial. |
| D12 | §13 CLI `vowp job add|update|ls|show|retry|park` | Adds `job cancel`, `vowp schema`, `vowp session tail`, `vowp __window` (hidden) | `session tail` is what a tmux job window runs (read-only pretty log follower); `schema` exports JSON Schema (§13 "JSON Schema export"). |
| D13 | §5.1 `seq` "gaps are rejected" | Append **rejects** a gap or duplicate seq (`StoreError::SeqGap`); on opening a log with a torn (non-newline-terminated, unparsable) final line the store truncates it and records the fact | Crash safety: a crash mid-write must not poison the log. |
| D14 | §11 `on_quota = "next_then_probe_back"` only | `OnQuota` enum: `next_then_probe_back` (default), `next`, `park` | Needed so non-Claude profiles can say what quota means. |
| D15 | §5.2 limit detection "unknown repeated errors treated as unavailable for 15 min" | Exactly: 3 consecutive runs of the same provider ending `error`/`crash` with no limit signal ⇒ `limits.kind='unavailable'`, `resets_at = now + 15 min` | Gives the vague statement a testable threshold (from `night.py` `claude_fail_streak`). |
| D16 | §16.3 job map is a seed | `job-map.md` splits it into concrete jobs (n02a–d, n06a1…, etc.) with disjoint ownership; ids keep the plan's `nNN` prefix | The brief for n00 asks for this. |
| D17 | §7.3: fix round returns the job to `reviewing` | After a fix job finishes the original goes `fixing → gating` (command gates re-run first), then `reviewing`; a finished fix of a `merge` job returns to `merging`; a failed fix job ⇒ original `failed` (loops.md §2 T22) | Cheap command gates run before another verifier is spent. |
| D18 | §13 CLI list | Adds keeper commands `vowp job nudge\|restart\|replan\|fail <id>` (write request rows), `vowp up --foreground\|--detach`, `vowp status --check`, files `~/.vowp/heartbeat` and `~/.vowp/board.json` | Needed for L5/L8 (loops.md §7, §10). |

## Open questions recorded for the owner (none blocking)

1. `serde_yaml_ng` vs `serde_norway` availability is checked by n02a with `cargo add`; either is acceptable (conventions.md §3).
2. `vowp.toml` timezone support: v0.1 accepts `UTC` and fixed offsets (`+02:00`) only; IANA names such as `Europe/Oslo` need a tz database
   crate (`jiff` or `chrono-tz`) — deferred to v0.2 unless the owner wants it earlier. The Claude limit text `resets 11pm (Europe/Oslo)` is parsed with a
   configurable `[node] tz_offset_minutes` fallback and the harness's own `resetsAt` when available (adapters.md §3.4).
