# v0.5 — scope and accepted StackSpirit foundation plan

Status: **accepted scope (2026-11-18).** v0.4 shipped as **0.4.0** on 2026-11-18; v0.5 active
development is approved for the five slices in [Accepted v0.5 scope](#accepted-v05-scope) below.
No public tag or date is promised until every slice and its gates pass.

## Status

v0.4 is shipped in **0.4.0** and its evidence is recorded: phase 4 is implemented, the security
review checklist, the owner live Mac smoke test and the rollback drill are in
[`docs/releases/v0.4-hardening.md`](../../releases/v0.4-hardening.md), and
[`docs/roadmap.md`](../../roadmap.md) carries the same status. The owner live Mac smoke test
(§8.3: pair or start one local orchestrator, complete one no-op turn on the same VPS session log,
then stop and clean up) **passed on 2026-11-18**.

## Accepted v0.5 scope

On 2026-11-18 the owner approved one focused **v0.5** cycle so VOWP provides every engine
primitive needed by desktop/UI surfaces (and VOWP's own web window) without UI clients reinventing
transport, session movement, project bootstrap or secret storage. Execute these five slices in
clean, disjoint-ownership jobs:

### 1. Live SSE streams for the web window (`vowp-daemon`)

- **1a. Jobs list SSE and keeper-action live refresh:** add a bounded snapshot/stream route for
  `/api/v1/jobs` and `/api/v1/jobs/{id}` following the board-stream heartbeat pattern (with
  service-worker `/stream` bypass and 5-second polling fallback), and refresh the `/jobs` and
  `/jobs/{id}` pages automatically after keeper actions.
- **1b. Live updates for Cost, Inbox and Nodes pages:** apply the same snapshot-stream pattern and
  polling fallback to `/cost`, `/inbox` and `/nodes` so no window page relies solely on 5-second
  polling.

### 2. Moving an orchestrator between home and an agent node while keeping its session

- Today `vowp orchestrator new --force` deletes the existing orchestrator directory and creates a
  **brand-new `SessionId`**. Desktop UIs need a one-click **Server ↔ Local** switch that moves
  where an orchestrator runs while keeping the **exact same `SessionId` and single session log on
  the home**.
- Add `vowp orchestrator move` *(planned)* (e.g. `vowp orchestrator move <name> --node <node>` and
  `vowp orchestrator move <name> --home` *(planned)*):
  1. Validate the target (`--home` or an active enrolled agent node; if the bound project is set
     and moving to an agent node, check its path-map policy as `orchestrator new` does).
  2. Stop the current orchestrator host (home host or remote agent host via `OrchestratorCommand`)
     and release its active surface lease cleanly.
  3. Write a handoff capsule on the existing `SessionId` so the new host resumes with current
     context.
  4. Re-materialize and start the orchestrator on the target (`--home` or `--node <node>`) bound to
     the **existing `SessionId`** (never minting a new session id), issuing/pairing the surface as
     appropriate and updating the orchestrator card's `node` and `state`.
  5. Export any updated protocol/schema types and regenerate `docs/reference/`.

### 3. Official orchestrator endpoint discovery for UI clients

- Desktop clients connect their chat UI directly to the orchestrator's paired `opencode serve`
  process (either locally on the agent machine or over an SSH loopback forward to the home) while
  VOWP's bridge mirrors turns to the home session log.
- Add `vowp orchestrator endpoint` *(planned)* (home-side, e.g.
  `vowp orchestrator endpoint <name> --json` *(planned)*) and `vowp agent orchestrator` *(planned)*
  local inspection (e.g. `vowp agent orchestrator endpoint <name> --json` *(planned)*) returning a
  typed JSON document (`name`, `app`, `session_id`, `location`, `node`, `state`, loopback `url`,
  native app session id when known, and auth header/username metadata needed by a local client on
  that same user account).
- Security rule: never include credentials in `snapshot.json`, `fleet status`, `orchestrator ls`,
  logs or error strings; any local auth material is returned only by the explicit local owner
  command reading the mode-`0600` host file on that machine.

### 4. One-step project bootstrap for UI `Add project` flows

- Today adding a repository from a UI takes four separate steps (`git clone`, `vowp init`,
  `vowp project add`, `vowp orchestrator new`).
- Extend `vowp project add` *(planned flags)* (or add `vowp project create` *(planned)*) with
  `--git <url>` *(planned)*, `--init` *(planned)* and `--orchestrator <app>` *(planned)*:
  - when `--git <url>` is given, clone into a validated directory under `<home-parent>/<name>` (or
    an explicit target path) using argv-only `git clone` with no credentials in argv or logs;
  - when `--init` is given and `vowp.toml` is absent, initialize the checkout cleanly;
  - register the project in the global store and, when `--orchestrator <app>` is given, create its
    project orchestrator and return a single machine-readable JSON summary (`project`, `path`,
    `initialized`, `orchestrator`).

### 5. Encrypted-at-rest option for the `vowp secret` vault

- VOWP already provides `vowp secret set`, `vowp secret ls`, `vowp secret rm`, `vowp secret
  upgrade` (isolated `vowp-vault` Linux user) and automatic `[REDACTED:<NAME>]` scrubbing, so UI
  clients will call `vowp secret` over verified SSH stdin rather than inventing a second secret
  store.
- Add an encrypted-at-rest backend option in `vowp-core::vault` and `vowp secret` (using `age`
  keyfile / `systemd-creds` when available on Linux, falling back cleanly to existing `0600` /
  `vowp-vault` modes, plus non-interactive stdin support on `vowp secret set <NAME>` verified end
  to end over SSH pipes without a TTY) and expose the storage mode (`basic`, `vault_user`,
  `encrypted`) in `vowp secret ls --json` so UI clients can display honest security status without
  ever seeing a secret value.

---

## Original candidate evaluation (reference)

The table below preserves the broader v0.5 candidate survey evaluated before the owner selected the
five slices above.

| Candidate | User value | Size | Depends on | Design note | Main risk | Owner decision first? |
|---|---|---|---|---|---|---|
| ACP streaming chunk events and permission flow | A long ACP turn shows progress as it streams, and a permission request can be answered instead of ending the turn. | medium | `crates/vowp-adapters/src/acp/`; an event shape for chunks and its schema page; for permission, a pause/resume path plus an inbox or UI answer. | [`v0.2/acp-adapter.md`](../v0.2/acp-adapter.md) — chunks and the permission flow are **planned**; approval needs pause/resume. | One event per delta can flood the log and every surface; an auto-approval or keystroke path would break the no-dialog rule. | Yes for the permission flow (inbox approval or explicit refusal); no for chunks alone. |
| Live updates for the cost, inbox and nodes pages | The three pages stop polling every five seconds; a new inbox item, node change or cost row appears at once. | small | The daemon snapshot-stream pattern, the three existing GET handlers and the embedded pages; the service worker already bypasses `/stream` URLs. | [`v0.3/web-window.md`](../v0.3/web-window.md), [`v0.3/http-api.md`](../v0.3/http-api.md) — shape and polling fallback exist; no per-page stream contract. | Three more long-lived streams per open tab; the polling fallback and the no-cache rule must stay. | Accepted (Slice 1b). |
| Node add, remove and probe writes through the HTTP API | Add or remove an SSH node and re-run its probe from the window, without a terminal. | medium | `crates/vowp-cli/src/cmd/node.rs` semantics; the request queue and supervisor apply; token-gated routes; `/nodes` controls; a fence rule for a node that owns surfaces or runs. | [`v0.3/http-api.md`](../v0.3/http-api.md), [`v0.3/web-window.md`](../v0.3/web-window.md) — read-only by design, no placeholder; only the direction is recorded. | `node add` runs an SSH probe, so the daemon either runs SSH or queues the work; removing a connected node must fence its work first. | Deferred past the 5 accepted slices. |
| SSE for the jobs list and keeper job actions | The jobs list and detail page update as jobs move; a keeper action shows its result without a manual refresh. | small | The `/api/v1/jobs` and `/api/v1/jobs/{id}` reads, the board-stream snapshot pattern and the seven existing job routes; the two job pages. | [`v0.3/http-api.md`](../v0.3/http-api.md), [`v0.3/web-window.md`](../v0.3/web-window.md) — marked later work; the stream contract is not written. | Job rows change often; a per-row frame stream can flood a phone. Send bounded snapshots and keep the polling fallback. | Accepted (Slice 1a). |
| Browser chat turn | Continue a session from the browser instead of the terminal. | large | A resident run queue in the daemon, the core `run_turn` cancel token, a streaming path, and lease/actor rules; a new browser write surface. | [`v0.3/web-window.md`](../v0.3/web-window.md) — a first-slice non-goal; needs a run queue, cancellation and a streaming path. No contract. | A browser turn and a CLI turn can become two writers on one session; a turn can outlive the tab; the write token gains a new power. | Deferred. |
| Full-directory `session push` and bidirectional `session sync` | Copy a whole session directory in the push direction, and keep two offline homes in step with a visible divergence report. | medium for `push --full`; large for `sync` | The existing `session dir-export`, full-pull staging and manifest checks; a remote directory import; the divergence head report; for sync, a policy for two advanced heads, the offline queue and the source lease. | [`v0.2/cross-node-sync.md`](../v0.2/cross-node-sync.md) — push is the fifth implemented slice; `--full`, remote dir import and sync are explicitly out of scope, with open questions. | A bidirectional command without a merge rule is a data-loss trap; a compacted source is refused today, and enforcement against a live source lease is still open. | Deferred. |
| VPS metrics alerts | Warn the owner about low memory, load or disk pressure before a job fails. | medium | Capacity history (shipped, schema v7 `capacity_rollups`), a threshold policy and a delivery path; no alert channel exists today. | [`v0.4/hardening.md`](../v0.4/hardening.md) §7 — history shipped; alerts are explicitly out of scope. No alert note. | Alert noise; a delivery channel adds an integration and a secret surface; an alert must report, not park or kill work. | Deferred. |
| Multi-user accounts, quotas and roles | More than one operator on one home, with per-user limits and an audit trail. | large | An identity and session model, per-user token and role storage, authorization on every read and write, audit rows, and schema/backup/restore changes across the store and every surface. | None. [`plan.md`](../plan.md) §14 lists it as later scope; the [0.3.0 known limitations](../../release/0.3.0.md#known-limitations) keep one operator per home. | Reads are unauthenticated on loopback today; accounts without a real authorization model would look safer than they are. | Deferred. |
| Parallel fan-out with a bounded budget | Try several executors or approaches for one job in parallel and keep the best, within a fixed spend. | large | Graph admission and slot accounting, per-job cost data, a budget policy, a winner-selection rule and result verification. | None. [`plan.md`](../plan.md) principle 7 says cost is reported, not capped; a budget is a product decision first. | It changes the cost promise, duplicates side effects across attempts and can starve other jobs of slots. | Deferred. |
| Hosted relay without SSH | Reach the home from a browser or phone without an SSH tunnel. | large | A public ingress, relay identity and auth, TLS, abuse controls and hosting; today every listener is loopback-only and SSH (or the owner's TLS proxy) is the only remote path. | None. [`v0.4/hardening.md`](../v0.4/hardening.md) §1 says the WSS ingress is not a hosted relay; the 0.3.0 limitations list no public listener. | A new public trust boundary, hosting cost, key custody and abuse; it changes the security story rather than extending it. | Deferred. |
| Shell completions for `vowp` | Tab completion for commands and flags in the common shells; fewer typos and better discovery. | small | The CLI command tree only; no store, protocol or daemon change. | None; not in [`docs/design/cli.md`](../cli.md). | Generated files drift from the parser; each shell needs one checked example. No data risk. | Optional filler behind Slices 1–5. |
| IANA time zones for schedules and limit resets | A 09:00 schedule keeps its local wall-clock meaning across DST, and `resets 11pm` is read in the owner's zone. | medium | `ScheduleConfig.tz` and `vowp-core::schedules::spec` (UTC or fixed ±HH:MM today), `[node].tz_offset_minutes`, the adapters' reset-clock parser, config validation and local display; a zone-data source. | None. The [schedules guide](../../guides/schedules.md) and the roadmap record the fixed-offset limit. | DST gaps and repeated hours, an offset that changes inside one cron window, and a zone-data source the tree does not carry today. | Deferred. |
