# v0.5 — scope brief

Status: planning note. v0.5 is not started; no release, tag, date or deploy is promised.

## Status

v0.4 is code-complete and its evidence is recorded: phase 4 is implemented in the tree, the
security review checklist and the rollback drill are in
[`docs/releases/v0.4-hardening.md`](../../releases/v0.4-hardening.md), and
[`docs/roadmap.md`](../../roadmap.md) carries the same status. The owner live Mac smoke test
(§8.3: pair or start one local orchestrator, complete one no-op turn on the same VPS session log,
then stop and clean up) is **pending**; it has not been run and is not claimed as passed. v0.5 has
no design, no code and no accepted scope. The rows below are candidates from the roadmap's
[Open follow-ups](../../roadmap.md#open-follow-ups), not commitments. This page gives the owner
one decision: which slice, if any, to queue next.

## Candidates

The roadmap's `/jobs/{id}` row is shipped and is not repeated here. The other twelve rows are
listed below. Size is a rough build-job estimate, not a duration: **small** is
one crate and its tests; **medium** crosses a crate boundary or adds a route or command;
**large** changes identity, schema or infrastructure.

| Candidate | User value | Size | Depends on | Design note | Main risk | Owner decision first? |
|---|---|---|---|---|---|---|
| ACP streaming chunk events and permission flow | A long ACP turn shows progress as it streams, and a permission request can be answered instead of ending the turn. | medium | `crates/vowp-adapters/src/acp/`; an event shape for chunks and its schema page; for permission, a pause/resume path plus an inbox or UI answer. | [`v0.2/acp-adapter.md`](../v0.2/acp-adapter.md) — chunks and the permission flow are **planned**; approval needs pause/resume. | One event per delta can flood the log and every surface; an auto-approval or keystroke path would break the no-dialog rule. | Yes for the permission flow (inbox approval or explicit refusal); no for chunks alone. |
| Live updates for the cost, inbox and nodes pages | The three pages stop polling every five seconds; a new inbox item, node change or cost row appears at once. | small | The daemon snapshot-stream pattern, the three existing GET handlers and the embedded pages; the service worker already bypasses `/stream` URLs. | [`v0.3/web-window.md`](../v0.3/web-window.md), [`v0.3/http-api.md`](../v0.3/http-api.md) — shape and polling fallback exist; no per-page stream contract. | Three more long-lived streams per open tab; the polling fallback and the no-cache rule must stay. | No. |
| Node add, remove and probe writes through the HTTP API | Add or remove an SSH node and re-run its probe from the window, without a terminal. | medium | `crates/vowp-cli/src/cmd/node.rs` semantics; the request queue and supervisor apply; token-gated routes; `/nodes` controls; a fence rule for a node that owns surfaces or runs. | [`v0.3/http-api.md`](../v0.3/http-api.md), [`v0.3/web-window.md`](../v0.3/web-window.md) — read-only by design, no placeholder; only the direction is recorded. | `node add` runs an SSH probe, so the daemon either runs SSH or queues the work; removing a connected node must fence its work first. | No for SSH nodes; agent enrollment or revocation would need one. |
| SSE for the jobs list and keeper job actions | The jobs list and detail page update as jobs move; a keeper action shows its result without a manual refresh. | small | The `/api/v1/jobs` and `/api/v1/jobs/{id}` reads, the board-stream snapshot pattern and the seven existing job routes; the two job pages. | [`v0.3/http-api.md`](../v0.3/http-api.md), [`v0.3/web-window.md`](../v0.3/web-window.md) — marked later work; the stream contract is not written. | Job rows change often; a per-row frame stream can flood a phone. Send bounded snapshots and keep the polling fallback. | No. |
| Browser chat turn | Continue a session from the browser instead of the terminal. | large | A resident run queue in the daemon, the core `run_turn` cancel token, a streaming path, and lease/actor rules; a new browser write surface. | [`v0.3/web-window.md`](../v0.3/web-window.md) — a first-slice non-goal; needs a run queue, cancellation and a streaming path. No contract. | A browser turn and a CLI turn can become two writers on one session; a turn can outlive the tab; the write token gains a new power. | Yes: whether the browser may write turns at all, and under which lease and actor. |
| Full-directory `session push` and bidirectional `session sync` | Copy a whole session directory in the push direction, and keep two offline homes in step with a visible divergence report. | medium for `push --full`; large for `sync` | The existing `session dir-export`, full-pull staging and manifest checks; a remote directory import; the divergence head report; for sync, a policy for two advanced heads, the offline queue and the source lease. | [`v0.2/cross-node-sync.md`](../v0.2/cross-node-sync.md) — push is the fifth implemented slice; `--full`, remote dir import and sync are explicitly out of scope, with open questions. | A bidirectional command without a merge rule is a data-loss trap; a compacted source is refused today, and enforcement against a live source lease is still open. | Yes for `sync` (what happens when both sides advanced); no for `push --full`. |
| VPS metrics alerts | Warn the owner about low memory, load or disk pressure before a job fails. | medium | Capacity history (shipped, schema v7 `capacity_rollups`), a threshold policy and a delivery path; no alert channel exists today. | [`v0.4/hardening.md`](../v0.4/hardening.md) §7 — history shipped; alerts are explicitly out of scope. No alert note. | Alert noise; a delivery channel adds an integration and a secret surface; an alert must report, not park or kill work. | Yes: the delivery channel (inbox only, mail or webhook) and whether alerts only report. |
| Multi-user accounts, quotas and roles | More than one operator on one home, with per-user limits and an audit trail. | large | An identity and session model, per-user token and role storage, authorization on every read and write, audit rows, and schema/backup/restore changes across the store and every surface. | None. [`plan.md`](../plan.md) §14 lists it as later scope; the [0.3.0 known limitations](../../release/0.3.0.md#known-limitations) keep one operator per home. | Reads are unauthenticated on loopback today; accounts without a real authorization model would look safer than they are. | Yes. |
| Parallel fan-out with a bounded budget | Try several executors or approaches for one job in parallel and keep the best, within a fixed spend. | large | Graph admission and slot accounting, per-job cost data, a budget policy, a winner-selection rule and result verification. | None. [`plan.md`](../plan.md) principle 7 says cost is reported, not capped; a budget is a product decision first. | It changes the cost promise, duplicates side effects across attempts and can starve other jobs of slots. | Yes. |
| Hosted relay without SSH | Reach the home from a browser or phone without an SSH tunnel. | large | A public ingress, relay identity and auth, TLS, abuse controls and hosting; today every listener is loopback-only and SSH (or the owner's TLS proxy) is the only remote path. | None. [`v0.4/hardening.md`](../v0.4/hardening.md) §1 says the WSS ingress is not a hosted relay; the 0.3.0 limitations list no public listener. | A new public trust boundary, hosting cost, key custody and abuse; it changes the security story rather than extending it. | Yes. |
| Shell completions for `vowp` | Tab completion for commands and flags in the common shells; fewer typos and better discovery. | small | The CLI command tree only; no store, protocol or daemon change. | None; not in [`docs/design/cli.md`](../cli.md). | Generated files drift from the parser; each shell needs one checked example. No data risk. | No. |
| IANA time zones for schedules and limit resets | A 09:00 schedule keeps its local wall-clock meaning across DST, and `resets 11pm` is read in the owner's zone. | medium | `ScheduleConfig.tz` and `vowp-core::schedules::spec` (UTC or fixed ±HH:MM today), `[node].tz_offset_minutes`, the adapters' reset-clock parser, config validation and local display; a zone-data source. | None. The [schedules guide](../../guides/schedules.md) and the roadmap record the fixed-offset limit. | DST gaps and repeated hours, an offset that changes inside one cron window, and a zone-data source the tree does not carry today. | Yes: where zone data comes from, and whether fixed offsets remain supported. |

## Recommendation

**1. SSE for the jobs list and keeper job actions.**
**2. Live updates for the cost, inbox and nodes pages.**

Both are read-only window work. They reuse the board stream's snapshot-and-heartbeat pattern, the
polling fallback and the service worker's existing stream bypass, and they need no protocol frame,
store schema, identity or owner policy. Both are unblocked and independently shippable, and each
carries its own tests in `vowp-daemon`; the first is the smaller of the two. Together they finish
the last pages that still poll, so the window is live wherever it can be.

**Not first:**

- **Browser chat turn.** It is the first browser write into a live session. It needs a resident
  run queue, cancellation, lease and actor rules, and a streaming path; that is the largest design
  gap in the list and the one most likely to produce two writers on one log.
- **Multi-user accounts, quotas and roles.** It needs identity, authorization on every read and
  write, audit, and schema/backup/restore work; it changes the "one operator per home" shape and is
  a project, not a slice.

Shell completions is smaller than either recommendation, but it is a one-time convenience and does
not improve the daily loop; it is a good filler slice behind the window work.

## What would change this recommendation

- **The owner smoke-test result.** If the live Mac run fails or finds a blocker, v0.4 repair comes
  first and no v0.5 slice starts.
- **Release timing.** If the owner chooses to spend the next jobs on v0.4 release work, that work
  comes first. This page sets no date and does not imply a release.
- **A different owner priority.** Remote or mobile use points at `session push --full` / `sync` or
  the relay; spend control points at metrics alerts or the fan-out budget decision; ACP users point
  at streaming chunks; browser-first use points at the chat turn after its design question.
- **A scope surprise.** If inspection shows either stream needs a protocol or store change, it is
  no longer a small slice; swap it for `session push --full`, which needs no new identity or policy.

## Questions for owner

1. Should the next job start v0.5 scope work, or stay on v0.4 release work (the smoke-test record
   and release preparation)?
2. If v0.5: is the recommended order right — jobs SSE first, then cost, inbox and nodes live
   updates — or is another candidate higher priority?
3. Should the first slice (jobs list SSE and keeper-action refresh) be queued now?
