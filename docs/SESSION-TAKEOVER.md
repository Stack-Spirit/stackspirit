# Explicit session takeover for orchestrator chat

Status: design only, 2026-11-28. This document changes no code, bridge, UI or
runtime. **Takeover is not live.** The read-only half (`ss-152`/`ss-153`) remains
the only shipped ownership behavior; nothing here overrides the current contracts.

Audience: the core, bridge, UI and verification jobs that implement takeover, the
VOWP owner, and the desktop reviewer.

Scope: how StackSpirit can become the writer of a VOWP orchestrator chat session
with `vowp orchestrator attach`, what the app must do with the child process and
the one-time grant, what the owner sees before and after, and what stays honestly
read-only. The OpenCode HTTP/SSE contract, the durable-log rule and the renderer
boundary in [`ORCHESTRATOR-CHAT.md`](ORCHESTRATOR-CHAT.md) do not change.

Non-goals: no implementation, no bridge or UI change, no loopback engine proxy,
no hard fencing of a direct-engine writer, no Server chat transport, no SSH
attach, no token persistence, and no claim that the current live Local path can
take a session over.

## 0. Short version

- The current tree is **read-only by report and by the composer, not by core
  enforcement**. The chat header shows a chip when VOWP reports an owner, and
  `access: "read_only"` disables the composer. `spirit-core` never checks `access`
  before `send`, holds no lease token and has no writer state. The UI is the only
  gate.
- VOWP 0.5.1's `vowp orchestrator attach <NAME> --holder … [--takeover]` is the
  only durable writer path for a desktop UI shipped today. It creates a
  `DesktopUi` surface, prints one grant, holds the lease while its stdin stays
  open, heartbeats every 5 s, and releases on EOF. `--takeover` replaces a live
  lease after a 5 s drain when the session has an executor. Ownership is
  **cooperative**: it changes VOWP's lease row and notifies paired surfaces; it
  does not stop a UI that writes to the engine's HTTP API directly.
- **The topology is the hard limit.** `orchestrator attach` is a **home-side**
  command and refuses a node-hosted orchestrator. The live Local chat on the
  owner's Mac uses `vowp agent orchestrator endpoint`, which reports a node-hosted
  orchestrator and always reports `owner: null` / `access: "read_only"`. VOWP
  0.5.1 has no `vowp agent orchestrator attach`. Therefore **the current live
  Local chat cannot use `orchestrator attach`**, and the app must not fake a
  takeover with `vowp session takeover` or by pretending `access` is owner.
- The design therefore has two honest deployment shapes:
  1. **Co-located home-hosted orchestrator** (the app and the VOWP home are on the
     same machine). `attach` holds the lease and the app can reach the loopback
     engine. This is the first implementable target.
  2. **Node-hosted orchestrator** (today's Mac). No VOWP node-side attach exists;
     the app stays read-only, shows why, and waits for a VOWP node-side attach
     (recommended). The deferred engine proxy only enforces an owner; it does not
     create one, so it is not a substitute.
- The app side is the same wherever a backend exists: one attach child per app
  process per chat session, no token in the renderer or on disk, a lease watcher
  that flips the window read-only when another surface takes over, and a release
  path that never leaves a stale lease on purpose.
- Recommended next step: build **Phase 1** (core manager + hermetic fake CLI)
  now. Do not ship the Phase 3 action on today's node-hosted path until VOWP
  provides a node-level attach, or until the owner chooses the co-located home
  deployment. A button that always returns `node_hosted` is worse than the current
  honest read-only notice.

## 1. Current state

### 1.1 The read-only half

The ownership chip and read-only notice are real UI, backed by display-only VOWP
metadata:

| Piece | What it does today |
|---|---|
| `ChatOwnershipView` (`crates/spirit-core/src/engine/chat/model.rs`) | Safe display fields: `kind`, optional `label`, `holder`, `node`, `mode`, `state`, `activity`. No surface id, session id, epoch, lease timestamp or token. |
| `ChatAccess` (`model.rs`) | `"owner"` or `"read_only"`. The doc comment already says it is cooperative, not enforced. |
| `ChatSessionView.ownership` / `.access` | Optional owner and optional access on the TypeScript wire (Rust serializes a default `read_only`). An older bridge may omit both; the UI then keeps its old behavior. |
| Endpoint resolver (`crates/spirit-core/src/engine/chat/endpoint.rs`) | Parses VOWP's `owner` and `access`, keeps only the display subset, drops surface/log identity, epoch and lease timestamps, and treats an absent or unknown `access` as `read_only`. |
| `LocalChatProvider` (`crates/spirit-core/src/engine/chat/local.rs`) | Copies the owner and access into the session record and the view. It also keeps the endpoint URL, port, Basic credential and native session id core-only. |
| `ChatView` (`packages/spirit-ui/src/next/ChatView.tsx`) | Shows the header chip when `ownership` is present. When `access === "read_only"`, shows a calm notice naming the writer, disables the message field and Send, keeps the model picker locked, and changes the placeholder to the read-only reason. The notice deliberately has no button. |

The notice's current copy is:

> **<Holder>** is writing in this session. This window is read-only.
> Close the other window or end the CLI session to release it. This window
> becomes writable again when the session is free.

That is accurate for a surface that can release the lease. It is not a takeover
flow.

### 1.2 The separate worker-session takeover

The Meadow shell also has an ownership banner (`OwnershipBanner` in `ChatView.tsx`)
backed by `FleetView.session_owners`. Its **Take over** action calls
`bridge.sessions.takeoverSession`, which runs:

```text
vowp --json session takeover --kind chat --holder stackspirit --node <NODE> <SESSION>
```

That command builds a transient `chat` surface, calls VOWP's `takeover_inner`, and
prints a report. It returns **no token and starts no heartbeat**. The lease row is
written with a token that the caller never receives; it goes stale after 60 s.
This is a useful one-shot "stop the other surface writing" signal, but it is **not
a durable writer lease**. It must not be offered as the chat takeover action and
must not be used to pretend the composer is now writable. Do not use `vowp chat`
or `session lease --acquire` as a keeper for a desktop UI either: they create a
different surface and do not give this app a writer grant it can heartbeat.

The banner and the chat header can both be visible for a `global`/`project` chat.
Phase 3 must keep their semantics separate; see §4.7.

### 1.3 What the read-only state does not enforce

| Gap | Evidence | Consequence |
|---|---|---|
| Core does not check `access` before a turn | `LocalChatProvider::send_turn` validates text, attachment, session and state, but not `record.access` | Any caller that reaches the Tauri command or the bridge directly can still send while the UI says read-only. |
| No writer lease is held | The app parses an endpoint and writes with the engine credential; it never attaches as a VOWP writer | The app cannot prove it is the owner, cannot release the lease, and cannot tell the difference between "no owner" and "another owner" beyond the display fields. |
| The endpoint is not re-checked during a turn | `open_orchestrator` resolves once; the event pump reconnects, not the ownership view | A takeover in another surface may go unnoticed until the chat re-attaches or the next endpoint read. |
| The direct-engine path bypasses VOWP | The app sends OpenCode HTTP requests with the endpoint credential | VOWP's lease is recorded, not enforced. The deferred loopback engine proxy is the eventual enforcement point. |
| The fleet takeover is one-shot | `session takeover` returns no token and starts no keeper | It cannot make this app the durable writer; the lease expires in 60 s. |
| The agent endpoint never reports an owner | `vowp agent orchestrator endpoint` cannot see the home DB and sets `owner: null`, `access: ReadOnly` | On today's live Mac path the chip has no holder and the composer is disabled even when nobody holds the session. |

### 1.4 The live Local path is node-hosted

`VowpEndpointResolver` runs exactly one command:

```text
vowp agent orchestrator endpoint <NAME> --json
```

The agent command reads the node's own `~/.vowp-agent/orchestrators/<NAME>/` state.
Its `endpoint()` builds `OrchestratorEndpoint` with `location: "node"`, then:

- `owner: None` — the agent cannot see the home database;
- `access: EndpointAccess::ReadOnly` — no caller surface identity;
- `url`/`auth` from the app's private `service.json`, when the host is live.

So a node-hosted Local session is **always** `read_only` in this tree. That is
the live owner's Mac setup verified on 2026-11-19. The current UI therefore
disables the composer on that path; the read-only half is honest about the lack
of ownership but does not make the session writable. With `owner: None` and
`access: read_only`, the notice has no holder and falls back to "Another window";
that is a safe default, not evidence that a window exists.

VOWP 0.5.1's `orchestrator attach` refuses the same record:

```text
orchestrator <NAME> runs on a node; the home cannot heartbeat a node-local
engine, so it cannot be attached from here
```

There is no `vowp agent orchestrator attach`; `vowp agent orchestrator` has only
`endpoint`, and that subcommand has no `--surface` option either. A future
node-side attach would also need a way to report this caller's access. This is the
gap the rest of this document is careful not to hide.

## 2. Takeover mechanics

### 2.1 The command and the grant

The writer command is:

```text
vowp [--home <HOME>] --json orchestrator attach <NAME> \
  --holder <HOLDER> [--label <LABEL>] [--takeover]
```

- `--holder` is a non-empty name shown to other surfaces. StackSpirit uses the
  fixed `stackspirit`; the renderer never supplies it.
- `--label` is an optional friendly name such as `StackSpirit · <NODE>`. Core
  builds it from safe metadata; the renderer never supplies it.
- `--takeover` is passed only after the owner has confirmed replacing a live
  holder. A free or stale session attaches without it.
- `--node` is **not** an SSH redirect for `attach`; the CLI consumes it and the
  command works only on a home-hosted orchestrator. Do not pass it.
- `--json` keeps stdout machine-readable. The grant is printed once on stdout.
  The hold notice and the release warning go to stderr:

  ```text
  attach: holding ownership; close stdin to release
  warning: closing stdin releases the session; the UI loses write access
  ```

The grant is an `OrchestratorAttach`:

| Field | Meaning | StackSpirit handling |
|---|---|---|
| `surface_id` | The new `DesktopUi` surface id | Core-only; used for the lease watch and endpoint re-resolution. Never in a view, log or error. |
| `token` | One-time lease token | Secret. Parsed, then zeroized/dropped; the child holds the live copy. Never stored, logged or sent anywhere. |
| `owner` | `SessionOwner` with session id, surface id, kind, label, holder, node, mode, state, activity, epoch and timestamps | Only the display subset becomes `ChatOwnershipView`; the rest stays core-only. |
| `endpoint` | `OrchestratorEndpoint` with `access: "owner"`, loopback `url`, engine `auth`, native session id | Core-only transport data; validated and used for the chat's HTTP/SSE connection. |
| `expires_at` | When the lease goes stale if the child stops heartbeating | Diagnostic only; not shown as a countdown and not a token. |

A grant whose `endpoint.access` is not `owner`, whose `owner.session_id` is not
the expected VOWP session, or whose `endpoint.native_session_id` is not the chat's
native session is rejected and released before any write.

### 2.2 What the lease means

`vowp orchestrator attach` builds a `DesktopUi` surface and calls VOWP's
`LeaseGuard::acquire` with the optional takeover flag. The lease is the
one-row-per-session `leases` record:

- a heartbeat every 5 s;
- `STALE_AFTER` 60 s — a lease with no recent heartbeat is free;
- a monotonically increasing `owner_epoch` on every acquire or takeover;
- one live owner at a time.

With a live owner and `--takeover`, `takeover_inner`:

1. atomically replaces the lease and increments the epoch;
2. invalidates the old token immediately;
3. drains a running turn for up to `TAKEOVER_DRAIN_SECS = 5` when the session has
   an executor, then interrupts it and writes a capsule and a `switch` with
   `reason = takeover`;
4. appends one takeover note with the old holder, the new holder and the
   `ownership_conflict` stop notice;
5. leaves the old surface able to read (`session tail`, `session resume` without
   takeover, fleet reads and engine GETs);
6. sends `LeaseRevoked`/`LeaseLost` to a paired agent surface. A `vowp chat`
   ingest writer sees `IngestAck::LeaseRevoked` on its next line/heartbeat.

What it does **not** do:

- it does not rotate the engine's Basic password;
- it does not close a UI's direct HTTP connection;
- it does not stop an OpenCode window that talks to the engine directly.

A `DesktopUi` attach child does not receive a `LeaseRevoked` frame. Its heartbeat
thread simply stops when the stored token no longer matches, while the CLI keeps
waiting for stdin. **The app must detect revocation itself** (§2.5); waiting for
the child to report it would leave the composer enabled against a lease it no
longer holds.

A job owner is handled separately: an expired job lease, or a live one with
`--takeover`, is fenced through `dispatch::fence_job_owner`, the run is closed,
the job stays `ready`, and the takeover note is recorded. A takeover of a job is
explicit, never silent.

### 2.3 Where it runs

| Orchestrator location | Engine endpoint source | VOWP 0.5.1 attach | StackSpirit takeover |
|---|---|---|---|
| Home-hosted, app co-located with the home | `vowp orchestrator endpoint <NAME> --json`, loopback URL/auth | **Yes** | **Yes** — this design |
| Home-hosted, app remote from the home | Home endpoint + an SSH loopback forward (not approved) or the deferred engine proxy | Yes, but the app cannot reach the loopback engine | **No**; do not hold an unusable lease |
| Node-hosted on this Mac (today's live Local chat) | `vowp agent orchestrator endpoint <NAME> --json` | **No**; attach refuses; no node-side command | **No**; stay read-only |
| Node-hosted on another node | Agent endpoint over a transport that is not part of this design | No | No |
| Bundled setup engine or engine-backed project fallback | App-owned `EngineSupervisor` | Not applicable; the app is the writer | No; `access` is already `owner` |

The co-located home case needs one transport prerequisite the current tree does
not have: `VowpEndpointResolver` is agent-only and cannot resolve a home-hosted
record. Phase 1 adds a narrow home fallback — try the agent command first, and
only when it reports `not_found`/no local record run the home `vowp orchestrator
endpoint <NAME> --json` with the same bounded, secret-free environment. It accepts
the document only when `location` is `home`, `url`/`auth` are live loopback
values, and the reported native session id is present. A home document with
`location: "node"` is metadata-only (URL and auth null) and must never become a
chat transport.

Before shipping that fallback, prove it is a read-only probe on a machine that
has no local home. If the command would initialize or mutate a home directory,
require an explicit local-home configuration instead; opening a chat must not
create a home database as a side effect.

Do not build an SSH attach in this design. Running `vowp orchestrator attach`
over SSH to the home would hold a lease that the app cannot use without the
deferred proxy or forward, and it would create a long-lived SSH control channel
the owner has not approved. If a later Server transport lands, the same manager
can use it behind a new endpoint source; that is a separate decision.

### 2.4 The app's attach child

The child is owned by `spirit-core`, never by the renderer, and is spawned with
argv only (no shell):

```text
vowp --json orchestrator attach <NAME> --holder stackspirit [--label <LABEL>] [--takeover]
```

The process contract:

| Aspect | Rule |
|---|---|
| Binary | The same local VOWP resolver as the endpoint read (`resolve_local_engine_binary`/the app's configured local VOWP path). Never a renderer-supplied path and never a different binary per action. |
| Environment | The same minimal allow-list as the endpoint resolver: `PATH` plus the VOWP home/agent variables the command needs. No provider keys, `OPENCODE_AUTH_CONTENT`, tokens or `.env` values. |
| stdin | Piped and **held open**. Never written to. EOF is the release signal. |
| stdout | Piped. Read **one complete JSON document** (VOWP pretty-prints `--json`), up to 64 KiB, with the attach timeout; parse exactly one value and ignore trailing whitespace. Do not read until EOF: the child keeps stdout open while it waits on stdin. The grant is never logged. |
| Failure output | For a runtime failure after argv parses, a failed `--json` run prints an `{"error": {…}}` document on stdout and exits nonzero; an unknown command or usage error is stderr plus exit 2. Read only the exit class and `error.kind` to classify; never render `message` or `hint` verbatim (they can contain a path or host). A `conflict` maps to `chat_takeover_conflict`; an unknown/missing command maps to `chat_takeover_unsupported`; everything else maps to `chat_takeover_failed` or `chat_takeover_unavailable`. |
| stderr | Piped to a bounded, redacted buffer (200 lines, 1,000 characters). The hold/release notices are safe; raw failures are mapped to typed errors and never shown verbatim. |
| Child lifetime | Exactly the attach. No `--detach`, no keeper, no second process. |
| Release | Drop stdin → wait up to 3 s → SIGTERM → wait 1 s → SIGKILL. The child's `AttachGuard`/`LeaseGuard` drop also releases the row. |
| Unexpected exit | Clear the writer state, mark the view `lost`, and do not auto-retry. The lease may remain until the 60 s stale window. |
| Timeouts | Probe 3 s; attach 20 s (5 s drain plus margin); release 3 s; lease watch 5 s per check. |
| Concurrency | One `WriterLease` per app process per `ChatSessionId`, behind an async mutex. A second `take_over` returns the in-flight state or the same held view; it never spawns a second child. |

The parent parses the grant into a core-only `WriterGrant` and then **does not
keep the token**: the child is the holder and needs it for heartbeats/release.
The raw grant buffer and the parsed token are dropped/zeroized immediately. If a
review asks for a parent-side copy, it must be `Zeroizing<String>` with no
`Debug`, `Serialize` or `Clone` leak; the first implementation does not need one.

Use a dedicated spawn/parse path for attach. Do not route it through a generic
VOWP command runner that captures stdout for logs, errors or diagnostics; the
grant line is the one command output that must never enter a shared buffer.

StackSpirit chooses **one writer surface per app process per chat session** for
the first release, not one per window. VOWP's design leaves window granularity
open (its Q9); a second StackSpirit window shares the process writer or is
read-only, and a second app process must take over explicitly. This avoids two
`DesktopUi` surfaces fighting over one session inside one app.

### 2.5 The lease watcher

The attach child cannot tell the app that another surface replaced the lease.
The app must poll the authoritative endpoint document:

```text
vowp [--home <HOME>] --json orchestrator endpoint <NAME> --surface <SURFACE_ID>
```

The rules:

1. Run every 15 s while the session is attached and the writer state is `held`.
   Also run immediately on window focus, app resume from sleep, before every
   `send`, and after an engine reconnect.
2. `access == "owner"` means this surface is still the live owner. Anything else
   (`read_only`, a missing owner, a different surface) is **terminal for this
   grant**: release the child, set `lost`, disable the composer, and keep the
   session readable.
3. A transient command/transport failure is retried once after 2 s. Two
   consecutive failures are also terminal; the app must not keep a writer state
   it cannot verify.
4. If the orchestrator is stopped or the endpoint disappears, release the child
   and mark the state unavailable. Do not keep a lease on a stopped host.
5. If the check finds the lease stale but still ours (for example after a long
   sleep), the same rule applies: release and require an explicit new takeover.
   A reissue is a new attach with a new token; never extend the old grant.

Polling is not enforcement. It makes **this app** stop writing when it no longer
owns the session. A direct-engine writer that never reads the endpoint can keep
writing until the deferred proxy lands.

### 2.6 Lifecycle

| Event | What happens |
|---|---|
| Chat view opens | Resolve the endpoint and probe takeover eligibility. Do not attach automatically. |
| Owner confirms Take over | Spawn the child, wait for the grant, validate it, switch the chat endpoint if needed, re-read the log, set `held`, enable the composer. |
| Chat view closes / user switches session | Release the lease (drop stdin, wait), then clear the writer state. The session stays attached for read-only use only if another view still shows it. |
| Window closes | Same as the chat view closing. If another window shows the same session, it becomes read-only and may explicitly take over later. |
| App quits | Release every writer child in `AppState` shutdown, wait up to 3 s total, then force. Tauri's exit hook and the manager `Drop` both do this. |
| App crashes / is force-quit | The OS closes the stdin pipe; the child reads EOF and releases. If the child is killed too, the lease goes stale within 60 s. No keeper is left behind. |
| Sleep | Timers and heartbeats freeze. On resume/focus the watcher runs before the next send. A lease that went stale is treated as lost and requires an explicit new takeover. |
| Engine restart / new port | Re-resolve the home endpoint with the surface id; if still owner, update the loopback URL/auth and re-establish the SSE pump; if not, go read-only. The lease belongs to the session, not the port. |
| Home restart / DB unavailable | The watcher fails; two consecutive failures release the child and show read-only. The lease row persists and ages naturally. |
| Another surface takes over | The watcher sees `read_only`; release the child, mark `lost`, disable the composer, and show the new owner. A running turn may have been drained/interrupted; do not claim it was stopped. |
| Duplicate Take over (same app) | The mutex returns the in-flight/held state; no second child, no second surface. |
| Duplicate Take over (second app) | VOWP's `--takeover` replaces the lease; the first app's watcher flips it read-only on its next check. The second app is the new writer. |
| Release while a turn is running | Release is not Stop. The lease is freed; the turn may continue on the engine. Copy must say so. Use Stop first to abort the turn. |

The manager's `Drop` is a safety net, not the primary path. It must not block the
async runtime; use a blocking reaper or `spawn_blocking`.

The 3 s release budget applies to a child that is already `Held`. A child still in
`Attaching` may be inside the 5 s takeover drain and has not reached the stdin
read yet. Close stdin, wait up to 6 s for the grant/release, then force. Closing
stdout or exiting the app feeds the child an EPIPE, and its guard drop still
releases; a forced kill during the drain can leave the new lease recorded until
the 60 s stale window, so say that rather than promising a clean release.

### 2.7 Writer state machine

The core state for one chat session:

```text
Unavailable { reason } --probe--> Available
Available --take_over--> Attaching --grant--> Held
Attaching --conflict or retryable failure--> Available (the action stays)
Attaching --permanent failure--> Unavailable { reason }
Held --release--> Releasing --> Unavailable
Held --watch/child exit--> Lost { reason } --probe--> Available
```

Safe reasons (never raw VOWP text):

```text
node_hosted
vowp_too_old
not_home
orchestrator_stopped
attach_refused
attach_failed
lease_lost
session_mismatch
```

`Available` means the probe found a co-located home-hosted orchestrator, a live
loopback endpoint, a matching native session and an installed `attach` command.
It does not mean a lease is held. A conflict or a retryable attach failure returns
to `Available` with a one-time error, so the owner can confirm again; a permanent
failure (unsupported, node-hosted, not home, stopped) is `Unavailable` with a
reason. `Held` means this app has a validated grant and a live child. `Lost` means
the app no longer trusts the grant; the old token is discarded and a new takeover
is a new token.

### 2.8 Core and bridge surface (proposed)

The existing `ChatSessionView` stays the only session view. Add one optional,
display-safe field:

```ts
export type ChatWriterState =
  | "unavailable"
  | "available"
  | "attaching"
  | "held"
  | "releasing"
  | "lost";

export type ChatWriterReason =
  | "node_hosted"
  | "vowp_too_old"
  | "not_home"
  | "orchestrator_stopped"
  | "attach_refused"
  | "attach_failed"
  | "lease_lost"
  | "session_mismatch";

export interface ChatWriterView {
  state: ChatWriterState;
  can_take_over: boolean;
  can_release: boolean;
  reason?: ChatWriterReason;
}
```

- `writer` is optional on the wire. An older bridge omits it; the UI keeps the
  exact read-only behavior it has today.
- `ownership` continues to say **who** holds the session. `writer` says whether
  **this app** holds it. The UI needs both; `access` alone is not enough after a
  restart or a stale endpoint read.
- The view carries no surface id, token, epoch, lease timestamp, URL, port,
  native session id or command line.
- Do not widen `ChatOwnershipView` to carry a surface id or epoch. Keep those in
  the core-only grant/watch state; the display view stays the safe subset it is
  today.

Bridge additions, both optional so the Web and older bridges keep compiling:

```ts
takeOver?(session: SessionId): Promise<ChatSessionView>;
releaseWriter?(session: SessionId): Promise<ChatSessionView>;
```

Tauri commands:

```text
chat_take_over(session)        -> ChatSessionView
chat_release_writer(session)   -> ChatSessionView
```

The renderer sends only the `SessionId`; core resolves the orchestrator, holder,
label, home and grant. The commands reject `setup` and engine-backed project
sessions (the app is already the writer) and any server-gated session.

Proposed typed errors, all safe to show through the existing error card:

| Code | When |
|---|---|
| `chat_takeover_unsupported` | The installed VOWP has no `orchestrator attach` command. |
| `chat_takeover_unavailable` | Node-hosted, not co-located, orchestrator stopped, or no live endpoint. |
| `chat_takeover_conflict` | A live owner appeared between the probe and the attach and `--takeover` was not confirmed. |
| `chat_takeover_failed` | Spawn, grant parse, validation or timeout failed. |
| `chat_writer_lost` | The lease was replaced or disappeared while held. |
| `chat_read_only` | A send was attempted while this app does not hold the writer lease. |

## 3. Token handling

There are two different secrets in play. They must never be conflated.

| Secret | Where it lives | Exposure |
|---|---|---|
| Lease token (`OrchestratorAttach.token`) | The attach child's memory; a one-time grant on stdout | Parsed by core, then zeroized/dropped. Never written to disk, never sent to the engine, never in a view, log, error, event, snapshot, crash report or renderer message. |
| Engine Basic credential (`OrchestratorEndpoint.auth`) | Core-only `LoopbackAuth`/`ChatEndpoint`, zeroized on drop | Used only as the HTTP `Authorization` header to the loopback engine. Never renderer, log or error. The attach grant carries the same kind of value; it replaces the endpoint auth only after validation. |

Rules:

1. **One-time grant, one holder.** The child prints the grant once. Core reads a
   bounded JSON document (it is pretty-printed, not one line), parses it, and
   immediately drops and zeroizes the raw buffer and the token. The child keeps
   the only live copy and uses it for heartbeats and release. The parent never
   needs the token after the spawn. Core-owned memory includes the attach child
   the app spawns; the token never leaves the app's core-owned processes.
2. **No token at rest.** It is not in `chat/sessions.json`, settings, a snapshot,
   the app-data tree, a temp file, argv or the environment. The grant is not
   written to a file.
3. **No token in observability.** No `Debug`, `Serialize`, `Clone` leak, log
   line, error string, panic message, Tauri event or DOM node. The inner grant
   type gets a custom `Debug` that prints `"[redacted]"`; the public view has no
   field for it.
4. **No token in the engine request.** The lease token is VOWP's, not OpenCode's.
   Sending it as a bearer/Basic credential would be wrong and would leak it to the
   engine. Only the endpoint `auth` goes on HTTP requests.
5. **Reissue is a new token.** Release, crash, sleep past the stale window or a
   second takeover end the old grant. The next takeover runs `attach` again and
   gets a new surface id and token. Never cache or reuse the old token, never
   "reconnect" the old child after it exited. An engine restart may keep the same
   grant when the watcher still verifies this surface as the live owner; that is
   not a reissue.
6. **Parse failure discards the buffer.** If the grant is not one valid JSON
   object with the required fields, close stdin, reap the child, return
   `chat_takeover_failed`, and never echo the raw stdout/stderr.
7. **Validation before use.** Check the owner session id, surface id shape,
   endpoint `access == owner`, loopback URL, present auth, and native session id.
   A mismatch releases immediately and never writes.
8. **Surface id is identity, not a secret.** It stays core-only because it is
   not display data, but it is not a credential. It is used for the lease watch
   and endpoint re-resolution, never rendered.
9. **Expiry is not a countdown.** `expires_at` is when the lease would go stale;
   the child's heartbeat is what keeps it alive. Do not show a timer or imply the
   user must act before it.

## 4. UX flow

### 4.1 When Take over appears

The action is available only when all of these hold:

| Condition | Why |
|---|---|
| `session` is `global` or `project:<name>` | `setup` is app-owned; takeover is not a concept. |
| The session is not `engine_backed` | The project fallback runs on the bundled engine; the app is the writer. |
| The endpoint is a VOWP local orchestrator, not the bundled engine or a server-gated transport | There is no lease to take on the bundled engine, and Server chat is out of scope. |
| The chat is attached and has a native session id | Takeover never creates or guesses a native session. |
| `writer.state === "available"` | The probe found a co-located home-hosted orchestrator and the `attach` command. |
| `writer.state` is not `attaching`/`releasing` | No duplicate action while a transition is in flight. |

The action is **absent** when the writer state is unavailable. The read-only
notice stays, with one extra safe reason line and no dead button. This is the
honest fallback for a node-hosted session, an older VOWP, a stopped orchestrator,
or an app that is not on the VOWP home.

If `access` says `"owner"` but `writer.state !== "held"` (a stale endpoint, a
restart, or a takeover from another app), the UI still treats the session as
read-only and offers Take over. The lease, not the string, is the authority. If
`writer.state === "held"`, show Release instead of Take over.

### 4.2 The confirmation

The user action is a single, explicit confirmation. It names the current holder
and the consequence:

```text
Take over this session?

<Holder> is writing in this session. (<Activity>)
Taking over makes the other app read-only. A running turn may be interrupted
after a few seconds. A window that connects to the orchestrator directly may
keep working until it notices.

[ Cancel ]  [ Take over ]
```

Rules:

- Build the holder name from `ownership.label`, then `holder`, then the human
  kind (`OpenCode`, `Claude Code`, `CLI`, `StackSpirit`, `Agent`). Append the node
  only when it adds information; never show a surface id, session id or epoch.
- If no owner is reported, say "No one is writing in this session." and keep the
  same primary action. This is an attach, not a replacement, but the user
  intent is the same.
- If `ownership.state` is `stale`/`expired`, say the previous writer left and this
  will replace a stale lease. Attach without `--takeover`; a live owner is the
  only case that needs the flag.
- If `ownership.activity === "running"`, add: "A turn is running. It may be
  interrupted after a few seconds." The existing fleet banner uses a two-step
  confirmation for `running`; the chat flow uses one dialog that states it
  explicitly. Do not claim the turn is definitely stopped: a direct-engine turn
  may continue.
- The dialog is the last point where the user can cancel. Once the attach child
  is spawned, the command is bounded and the UI shows `Attaching`; do not offer a
  Cancel that kills a child mid-takeover and leaves a recorded lease.
- The primary button is not focused by default; focus Cancel. Escape cancels.
  The dialog uses `role="alertdialog"`, `aria-labelledby`, `aria-describedby`
  and a focus trap. The result is announced politely.
- Do not use the word "fence" in the UI. It is not true for a direct-engine
  writer until the proxy lands. "Makes the other app read-only" is.

### 4.3 Resulting state

| State | Header chip | Composer | Notice |
|---|---|---|---|
| `held` | `StackSpirit · <NODE>` (or `StackSpirit`) with a "you are writing" title | Enabled | A one-time calm notice: "You are writing here. Close this chat or release the session to let another app write." |
| `available`, other owner | The owner's chip | Disabled | The existing read-only notice plus a Take over action in the header/ownership area. |
| `available`, no owner | A calm "No writer" chip | Disabled | "No one is writing in this session." plus Take over. |
| `attaching` | "Taking over…" with a spinner | Disabled | "Taking over the session…" |
| `releasing` | The previous chip, disabled | Disabled | "Releasing the session…" |
| `lost` | The new owner's chip, if reported | Disabled | "Another writer took this session over. This window is read-only." The previous turn, if any, may have been stopped. |
| `unavailable` | The owner's chip if reported | Disabled | The existing read-only notice plus the safe reason line from §4.1. |

The chip stays in the header. The Take over control is a compact button next to
the chip or in the ownership row; it is **not** inside the calm read-only notice,
so the existing notice copy and its "no action" contract stay intact. The action
is reachable by keyboard and has an accessible name.

After a successful takeover:

- The read-only notice disappears; the composer becomes usable only when the
  existing `state === "ready"` and no model gate applies.
- The model picker remains locked for an orchestrator session, as today.
- If the previous owner was `running`, show a non-blocking note: "The previous
  turn may have been interrupted." Do not show a fake completion or a fake error.
- The chat log is re-read from `GET /session/{id}/message` before the first send,
  because the endpoint may have changed and a drained turn may have written a
  final message. The VOWP takeover note lives in the VOWP session log, not the
  OpenCode message log.

### 4.4 What the previous writer sees

| Previous writer | What it sees |
|---|---|
| Another StackSpirit window/process | Its lease watcher polls the endpoint, sees `read_only`, releases its child, disables the composer, and shows the new owner plus "Another surface took this session over." If it was streaming, it marks the turn `ownership_lost`, keeps the partial text and ignores a late response for that turn. |
| A paired VOWP agent surface (OpenCode/Claude Code/CLI through the gateway) | VOWP sends `LeaseRevoked`/`LeaseLost`; its ingest writer refuses further lines and receives the `ownership_conflict` stop notice. The app does not see these frames. |
| A `vowp chat` live guard | Its next ingest/heartbeat sees the token mismatch (`IngestAck::LeaseRevoked`) and the guard becomes read-only. |
| An OpenCode window or any raw HTTP client | **May keep writing.** The lease row changed, but the engine credential and HTTP connection did not. The proxy is deferred. The app must not claim it stopped this writer. |

The takeover note is appended to the VOWP session log. It is visible through
`vowp session tail`/`session show`; it is not automatically a chat message. The
app may show its own local notice, but it must not invent a log entry.

### 4.5 Releasing

The writer has an explicit **Release** action in the ownership area while
`writer.state === "held"`. It is not the same as Stop.

- Release drops stdin, waits for the child to exit, releases the VOWP lease and
  returns the view to `available`/read-only. The composer disables.
- If a turn is streaming, the turn keeps running on the engine. The confirmation
  for Release says: "The current turn keeps running on the orchestrator. Release
  this session?" with Release/Cancel. If no turn is streaming, release is a
  single click with no dialog.
- Closing the chat view, switching sessions, closing the window or quitting the
  app releases the writer. The app does not keep the lease while no chat is open.
- If the child exits unexpectedly, the app marks `lost` and does not
  automatically re-attach. The user can Take over again; that is a new grant.
- A release is best-effort on crash. The worst case is a stale lease that clears
  in 60 s; the UI must not promise an immediate release after a force-quit.

### 4.6 Copy and accessibility rules

- Use "Take over", "Release", "read-only", and the holder's name. Avoid "lease",
  "token", "surface", "epoch", "attach" and "proxy" in user-facing copy.
- A holder with no label uses the kind plus a node only when the node is not
  already in the label.
- The confirmation and the lost notice are polite live regions, not assertive
  alerts unless the lease was lost mid-turn.
- Axe/WCAG 2.2 AA in light and dark for the header action, dialog and notices.
- No URL, port, native id, surface id, token, epoch or path in the DOM, a
  tooltip, an aria-label or a test id.

### 4.7 What the UI must not do

- Do not reuse `OwnershipBanner` or `bridge.sessions.takeoverSession` for chat
  takeover. That is the one-shot `vowp session takeover` path and does not hold a
  lease. If both controls would be visible on a `global`/`project` chat, hide the
  fleet banner's action there or label it as a different operation; do not show
  two "Take over" buttons with different meanings.
- Do not enable the composer because `access === "owner"` alone. Require
  `writer.state === "held"` for a VOWP orchestrator session.
- Do not auto-retry a failed takeover. A conflict means someone else won; show
  the new holder and let the owner decide again.
- Do not auto-attach on view open. The owner's explicit click is the consent.
- Do not claim a raw OpenCode window or any direct-engine writer was stopped,
  fenced or made read-only. It was not, until the proxy lands.
- Do not show a Take over button on a node-hosted session where VOWP cannot
  attach; show the reason and keep the composer disabled.

## 5. Failure modes

| Failure | Detection | What the user sees | Recovery |
|---|---|---|---|
| Attach refused: a live owner exists and no `--takeover` was confirmed | The child exits nonzero; VOWP's typed conflict names the holder | "Someone started writing while you were deciding." The new holder is named; no write. The action remains available. | Refresh the view, confirm again if the user still wants it. No retry loop. |
| Attach refused: VOWP has no `orchestrator attach` | Probe of the command fails; the child never spawns | `writer.state = unavailable`, reason `vowp_too_old`; read-only notice. | Upgrade VOWP to 0.5.1+; no fake. |
| Node-hosted orchestrator | The endpoint reports `location: "node"` or the agent endpoint is the only source | `writer.state = unavailable`, reason `node_hosted`; read-only notice explains VOWP 0.5.1 can only attach a home-hosted orchestrator. | Wait for a VOWP node-side attach (recommended) or use the co-located home deployment. Do **not** call `session takeover` as a workaround. |
| App not on the VOWP home / remote home | The home endpoint probe fails or returns no live loopback endpoint | `unavailable`, reason `not_home`; read-only. | Run the app on the home; remote home chat is the deferred proxy/forward, not this design. |
| Orchestrator stopped | Endpoint `state` is stopped/degraded or `url`/`auth` are null | `unavailable`, reason `orchestrator_stopped`; offer the existing Resume action if available. | Resume the orchestrator, refresh, then Take over. |
| Node unreachable (another node) | The agent endpoint command fails | A typed `chat_not_configured`/`chat_gated` from the existing transport; no takeover control. | Restore the node/agent; this design does not add a remote transport. |
| Attach timeout / hung child | 20 s without a grant | `chat_takeover_failed`; the child is reaped best-effort. | Retry once. If VOWP recorded a lease, it clears within 60 s; say so, do not promise immediate freedom. |
| Child exits before/after the grant | Wait task observes the exit | `lost` if a grant was held; `failed` if not. The composer disables. | Take over again; a new grant/token. |
| Lease revoked mid-turn | 15 s watcher, focus, resume or pre-send check sees `read_only` | `lost`; the current turn is marked "may have been stopped"; the new owner is named. | Stay read-only; the new writer controls the session. Do not abort the new owner's turn. |
| Another window takes over while idle | Same watcher | `lost`; read-only notice. | Explicitly take over again if desired. |
| App quits while holding | Shutdown releases each child; `Drop` is a safety net | No UI. | Next launch sees no owner or a stale row; a new attach is explicit. |
| App crashes / force-quit | OS closes stdin; child releases; worst case 60 s stale | On next launch: maybe a stale StackSpirit owner. | The stale row can be replaced without `--takeover`; if it still looks live, confirm a takeover. Never reuse the old token. |
| Sleep past the stale window | Resume/focus watcher | `lost`; read-only. | Take over again; the new grant is a new token. |
| Engine restart / new port | Endpoint re-resolve after SSE reconnect | A short connecting state; no fake error. | If still owner, update the endpoint and resume; otherwise read-only. |
| Home restart / DB busy | Two consecutive watcher failures | `lost`; read-only. | Reconnect; take over again when the home is healthy. |
| Duplicate Take over (same app) | Mutex/state machine | `attaching` or the held view; no second child. | Nothing; the first operation finishes. |
| Duplicate Take over (second app) | The second attach with `--takeover` succeeds; the first watcher sees it | First app: `lost`; second app: `held`. | The first app stays read-only; the second is the writer. |
| A direct-engine writer keeps writing | The app cannot detect it through the lease | The app still shows itself as owner; the other window may also write. | This is the known cooperative limit. The proxy is the fix; do not claim otherwise. |
| Session id mismatch between the grant and the chat | Grant validation | `chat_takeover_failed` or `chat_not_mirrored`; no write. | Release; re-resolve the orchestrator and native session; do not attach another log. |
| Stale owner row from a dead StackSpirit | `ownership.state` is stale, or attach without `--takeover` replaces it | "The previous writer left; this will replace a stale lease." | Attach without `--takeover` (or with it if the row still looks live); a new token. |
| Older bridge / VOWP without the fields | `writer` absent, `access` absent | Today's exact behavior: no chip action, composer as before. | Upgrade; no fake. |
| Token leak attempt | A test plants a canary in the fake grant | The test fails if the canary appears in a view, log, error, event, file or DOM. | Fix the leak; never ship. |

The honest read-only fallback is the same in every unavailable case: keep the
session readable, show the owner if VOWP reports one, disable the composer, and
say why takeover is not available. No fallback ever sets `access` to owner or
fabricates a writer.

## 6. Phased delivery

Each phase is a separate job or two small jobs. A phase must not claim a live
writer until its acceptance test and the owner-run check pass.

| Phase | Scope | One-line acceptance | Suggested `owns` |
|---|---|---|---|
| **1** | Core attach manager, capability probe, home endpoint fallback, fake CLI | "A fake `vowp` child prints one grant and holds stdin; the manager reports `held` with a redacted view, rejects duplicate attach, releases on drop, and no fake token appears in any Debug, view, log, error or file." | Core chat/endpoint/writer files + fixtures/tests (see below) |
| **2** | Tauri commands + bridge + wire | "`chat_take_over`/`chat_release_writer` return a safe `ChatSessionView` and never a token, surface id or endpoint; the bridge methods are optional and Mock/Web behavior is explicit." | Tauri commands/state + bridge/wire + tests |
| **3** | UI action, confirmation, chip/state, read-only fallback | "On a supported co-located home-hosted session a read-only window shows Take over, the dialog names the holder, a successful attach enables the composer and shows the writer chip; on a node-hosted or older VOWP session the action is absent with the honest reason." | `ChatView.tsx` + ownership CSS + UI tests/e2e |
| **4** | Release, quit, sleep, reconnect, previous-writer loss | "Closing the chat/window, quitting, sleeping past the stale window or losing the lease releases or reaps the child, flips the view read-only within one watch interval, and never claims a turn was stopped." | Lifecycle wiring + watch/restart tests + e2e |

### Phase 1 — core attach manager with a fake CLI

Deliver:

- `ChatWriter`/`WriterLease` state machine with `probe`, `take_over`, `release`,
  `watcher` and `Drop`.
- The exact attach child contract from §2.4: argv-only spawn, held stdin, bounded
  stdout/stderr, grant validation, token drop, graceful release, typed errors.
- The capability probe: `vowp orchestrator attach` command presence, co-located
  home endpoint, live loopback `url`/`auth`, matching native session, matching
  VOWP session id. The probe must not spawn an attach.
- The narrow home endpoint fallback in `endpoint.rs`: agent command first, home
  `vowp orchestrator endpoint` only when the agent has no local record; accept
  `location: home` with loopback URL/auth and a native id. Reject `location: node`
  metadata as a transport.
- The core `send_turn` rule: a VOWP orchestrator session may send only when
  `writer.state === "held"` and `access === "owner"`. Setup and engine-backed
  sessions keep their app-owned path. This closes the current bypass.
- The `ChatWriterView` (or the field on `ChatSessionView`) with only the safe
  state/reason booleans.
- A hermetic fake `vowp` child and the state-machine/token tests in §7.

Suggested `owns`:

```text
crates/spirit-core/src/engine/chat/writer.rs              (new)
crates/spirit-core/src/engine/chat/mod.rs
crates/spirit-core/src/engine/chat/model.rs               (safe view only)
crates/spirit-core/src/engine/chat/endpoint.rs            (home fallback + probe)
crates/spirit-core/src/engine/chat/local.rs               (send gate + wiring)
crates/spirit-core/tests/chat_writer.rs                   (new)
crates/spirit-core/tests/fixtures/fake-vowp/              (new)
```

Do not add a Tauri command or a UI control in Phase 1. The fake CLI is the only
process; no test touches a real home, keychain, network or engine.

### Phase 2 — Tauri command + bridge

Deliver:

- `chat_take_over` and `chat_release_writer` in
  `apps/spirit-desktop/src-tauri/src/commands/chat.rs`, behind the existing
  `AppState` chat service. Validate the session, reject setup/engine-backed and
  server-gated sessions, and return only `ChatSessionView`.
- `SpiritBridge.chat.takeOver?`/`releaseWriter?`, `TauriBridge` wiring, wire types
  for `ChatWriterView`, and `MockBridge` behavior. The Mock may return a typed
  `preview_unavailable` or a deterministic preview state; it must not claim a
  real lease. The Web bridge omits the methods.
- Tests that the serialized command result contains no token, surface id, URL,
  port, native id, epoch or path.

Suggested `owns`:

```text
apps/spirit-desktop/src-tauri/src/commands/chat.rs
apps/spirit-desktop/src-tauri/src/state.rs                  (shutdown hook)
apps/spirit-desktop/src/bridge/TauriBridge.ts
packages/spirit-ui/src/bridge/SpiritBridge.ts
packages/spirit-ui/src/bridge/wire.ts
packages/spirit-ui/src/bridge/MockBridge.ts
packages/spirit-ui/src/test/next/chatWriterBridge.test.tsx  (new)
```

### Phase 3 — UI action

Deliver:

- The header/ownership action, the confirmation dialog, the writer chip/state,
  the release action and the safe reason copy from §4.
- The existing read-only notice stays unchanged and action-free; the Take over
  button is separate. The fleet `OwnershipBanner` action is not reused; if both
  would appear on a chat, hide or separate the fleet action.
- Accessibility and copy tests: holder naming, running extra line, keyboard focus,
  Escape, light/dark axe, no token/surface id in the DOM.
- The action is absent when `writer.state` is unavailable. A node-hosted session
  shows the reason and remains read-only. Do not ship a button that always fails.

Suggested `owns`:

```text
packages/spirit-ui/src/next/ChatView.tsx
packages/spirit-ui/src/next/theme.css                        (ownership action only)
packages/spirit-ui/src/test/next/chatTakeover.test.tsx       (new)
apps/spirit-desktop/e2e/next-shell.spec.ts                   (writer flow, mock-safe)
```

### Phase 4 — release, quit and reconnection

Deliver:

- App/window/chat lifecycle release: close chat, switch session, close window,
  quit, `RunEvent::ExitRequested`, and the manager `Drop` safety net.
- The lease watcher running on a 15 s interval plus focus/resume/pre-send checks;
  a terminal non-owner result releases the child and flips the view.
- Previous-writer detection: `lost`, read-only notice, turn fencing, late-response
  ignore, log backfill.
- Engine restart/endpoint re-resolution and sleep/wake tests.
- The owner-run checklist in §7.5.

Suggested `owns`:

```text
crates/spirit-core/src/engine/chat/writer.rs               (watch/lifecycle)
crates/spirit-core/src/engine/chat/local.rs                (fenced turn)
apps/spirit-desktop/src-tauri/src/state.rs                 (exit hook)
apps/spirit-desktop/src-tauri/src/lib.rs                   (window events)
packages/spirit-ui/src/next/ChatView.tsx                   (unmount release only)
crates/spirit-core/tests/chat_writer_lifecycle.rs          (new)
apps/spirit-desktop/e2e/chat-takeover.spec.ts              (new, fake bridge)
```

## 7. Test strategy

### 7.1 The hermetic fake `vowp`

`crates/spirit-core/tests/fixtures/fake-vowp/` holds one small executable (or a
shell/Rust helper) that is invoked through a temp `PATH`. It must never call the
real `vowp`, the network, `~/.vowp`, `~/.vowp-agent`, the keychain or an engine.

Modes, selected by an environment variable or argv:

| Mode | Behavior |
|---|---|
| `attach-grant` | Print a valid, pretty-printed grant with a canary token and a loopback URL/auth; write a "holding" stderr line; read stdin until EOF; write a release marker; exit 0. |
| `attach-conflict` | Exit nonzero with a VOWP-shaped conflict naming the holder; no lease. |
| `attach-node` | Exit nonzero with the home-only refusal. |
| `attach-hang` | Print nothing, sleep past the test timeout. |
| `attach-crash` | Print a grant, then exit 1 without waiting for stdin. |
| `endpoint-owner` | Print a home endpoint with `access: "owner"` and the current surface. |
| `endpoint-other` | Flip the state file to another holder; print `read_only`. |
| `endpoint-node` | Print the agent-shaped node document (`owner: null`, `access: read_only`). |
| `endpoint-stopped` | Print a stopped document with null URL/auth. |
| `attach-old` | No `attach` subcommand; print an unknown-command error. |

The fake records every argv, stdin state and release marker under a temp dir so
tests can assert:

- `--takeover` is present only when the user confirmed a live owner;
- no `--node` or shell is used;
- only one attach child is spawned per session;
- stdin stays open while held and closes on release;
- the canary token never appears in any captured file.

### 7.2 Token-leak assertions

Every writer test uses a canary like `fake-lease-canary-<random>` in the grant.
Assert it is absent from:

- `Debug` of the grant, manager, session record and errors;
- `serde_json::to_string` of `ChatSessionView`, `ChatWriterView` and every event;
- the log buffer and any test logger output;
- every file under the temp app-data/home dir;
- the Tauri command result bytes and the Mock/UI DOM;
- a panic message (where the test can capture one).

The engine Basic credential gets the same treatment. A test that only checks the
view is not enough: the token path includes stdout parsing, error mapping, the
manager struct and the bridge.

### 7.3 State-machine and lifecycle tests

At minimum:

- probe: available for a co-located home-hosted document; `node_hosted` for an
  agent document; `vowp_too_old` for a missing command; `orchestrator_stopped`
  and `not_home` for the matching documents;
- `take_over`: free attach without `--takeover`; live owner after confirmation
  with `--takeover`; duplicate call returns the held view and spawns once;
- grant validation: wrong owner session, wrong native session, non-loopback URL,
  missing auth, `access != owner` all release and fail;
- release: stdin closed, child exits, state available, no token file;
- unexpected child exit: state lost, composer gate, no auto-retry;
- watcher: still owner keeps held; another owner flips lost and reaps the child;
  one transient error retries, two fail; focus/pre-send check runs;
- send gate: setup/engine-backed still send; a read-only orchestrator rejects
  with `chat_read_only`; a held orchestrator sends;
- sleep/reissue: a stale lease flips lost, a second attach gets a new surface id
  and a new token, the old token is not reused;
- session mismatch: attach releases and returns `chat_not_mirrored`;
- app drop/shutdown: the child sees EOF, the release marker is written, no
  process remains.

### 7.4 Desktop, bridge and UI tests

- Serialize the command result and the event payloads; assert no token, surface
  id, URL, port, native id, epoch or path.
- MockBridge: `takeOver` returns the preview/typed gate; it never fabricates a
  lease. Web bridge has no method.
- UI: Take over absent when unavailable; present when available; dialog names the
  holder; running extra line; confirm calls the bridge once; success chip and
  composer; failure shows the safe reason; Release; lost mid-turn; node-hosted
  reason; axe light/dark; keyboard Escape/focus.
- Existing ownership tests (`chatOwnership.test.tsx`) stay green: an older bridge
  with no `writer` keeps the current chip/notice behavior.

### 7.5 Real VOWP checks (owner-run)

These cannot run in CI and must not use a paid model automatically. They need a
co-located VOWP home with a live home-hosted opencode orchestrator and a second
writer surface (a second StackSpirit window, `vowp chat`, or a paired agent
surface). Use placeholders in the report; never record a URL, port, password,
token, native id or real hostname.

| Check | Why it needs the real home |
|---|---|
| `vowp orchestrator attach` prints one grant and holds until EOF | The real CLI and DB are the contract; the fake cannot prove the grant shape. |
| `vowp session lease <SESSION>` names the DesktopUi surface while held | Confirms the surface kind, holder and heartbeat. |
| A second writer without `--takeover` is refused with the holder name | Confirms the live-owner conflict text/exit class. |
| A second writer with `--takeover` replaces the lease; the first app flips read-only within one watch interval | Confirms the watch and the 60 s/5 s semantics end to end. |
| Closing the app releases within seconds; `session lease` shows no owner | Confirms the stdin EOF/release path on the real OS. |
| Kill the app hard; the lease is stale within 60 s and a new attach succeeds | Confirms the crash bound without promising a clean release. |
| Sleep past 60 s; the next check is read-only and a new attach gets a new token | Confirms the sleep/reissue rule. |
| An OpenCode window keeps writing after the takeover | Confirms the cooperative limit; the owner must see it before the UI claims enforcement. |
| A node-hosted orchestrator returns the typed `node_hosted` state and no button | Confirms the honest fallback on today's Mac. |
| No token appears in the app log, a snapshot, the DOM or an error card | An end-to-end scan with a canary is not possible on the real grant; use a debug build that wraps the child and checks captured output. |

The existing `scripts/verify-orchestrator-chat.mjs` remains the OpenCode/streaming
checklist. A future `scripts/verify-session-takeover.mjs --check` should print a
fixed checklist and must not perform a real takeover on its own.

### 7.6 CI rules

- No network, real `vowp`, real engine, keychain, `~/.vowp`, `~/.vowp-agent` or
  paid model call.
- Fake CLI only; temp `HOME`/`VOWP_HOME`/`PATH`; no real app-data tree.
- Fixtures use placeholders and synthetic canaries.
- `node scripts/check-doc-links.mjs` and
  `bash scripts/check-public-readiness.sh` stay green.
- A test that only asserts `true`, or that skips the token-leak assertion, is not
  a writer test.

## 8. What this design deliberately does not do

- It does not implement, claim or enable takeover; the current read-only half is
  unchanged and the composer remains the only gate.
- It does not add a loopback engine proxy or hard writer fencing. A direct-engine
  writer is still possible until that slice lands.
- It does not add a node-side attach or an SSH attach. A node-hosted Local session
  stays read-only in this design.
- It does not use `vowp session takeover` as the chat writer path; that command
  has no durable token/heartbeat.
- It does not auto-take-over on open, on focus, on reconnect or after sleep.
- It does not persist, log or render the lease token, surface id, epoch or lease
  timestamps.
- It does not change the VOWP durable-log rule, the OpenCode HTTP/SSE contract,
  the server-chat gate or the renderer security boundary.
- It does not rotate the engine password or stop the OpenCode window.
- It does not promise a clean release after a crash; it promises no intentional
  orphan and a 60 s stale bound.
- It does not make the fleet ownership banner and the chat writer action the same
  thing.

## 9. Owner dependency and recommendation

The live Local path is node-hosted, and VOWP 0.5.1's only durable attach command
for a desktop UI is home-side. That is a VOWP-side gap, not a StackSpirit UI bug.
The options are:

| Option | What it needs | Recommendation |
|---|---|---|
| **A. VOWP node-side attach** — `vowp agent orchestrator attach <NAME> --holder … [--takeover]` on the node's paired surface, mirroring the home command | A small VOWP slice; the node already has the agent, the app endpoint and the paired surface. StackSpirit's manager/UI stay the same. | **Recommended** for today's Mac deployment. |
| **B. Co-located home deployment** — run StackSpirit on the VOWP home with a home-hosted orchestrator | The Phase 1 home endpoint fallback; no proxy needed because the engine is loopback on the same host. | Good for a home console; not today's Mac. |
| **C. Remote home attach + engine proxy/forward** | The deferred loopback engine proxy or an approved SSH forward. | Out of scope; do not combine it with this design. |
| **D. Keep read-only** | Nothing. | The honest fallback until A or B lands. |

Do not ship a Take over button that always returns `node_hosted`. The value of
this design is that the manager, grant handling, token rules, confirmation and
release path are ready when A or B lands, and that the app never lies about the
current state before then.

## 10. References

VOWP (read-only; never edited from a StackSpirit job):

- [`runtime/docs/design/v0.5.1/session-ownership.md`](../runtime/docs/design/v0.5.1/session-ownership.md)
  — the lease model, `DesktopUi`, attach, takeover, read-only rules and the
  explicit statement that the proxy is deferred.
- [`runtime/crates/vowp-cli/src/cmd/orchestrator.rs`](../runtime/crates/vowp-cli/src/cmd/orchestrator.rs)
  — the `attach` command, arguments, grant printing, stdin hold and release.
- [`runtime/crates/vowp-core/src/surface/orchestrator.rs`](../runtime/crates/vowp-core/src/surface/orchestrator.rs)
  — `Orchestrator::attach`, `AttachGuard`, the node-hosted refusal and the
  endpoint `owner`/`access` projection.
- [`runtime/crates/vowp-core/src/surface/takeover.rs`](../runtime/crates/vowp-core/src/surface/takeover.rs)
  — `takeover_inner`, `TAKEOVER_DRAIN_SECS` and the takeover note.
- [`runtime/crates/vowp-core/src/surface/lease.rs`](../runtime/crates/vowp-core/src/surface/lease.rs)
  — `LeaseGuard`, the 5 s heartbeat, the 60 s stale window and `check_lease_with`.
- [`runtime/crates/vowp-protocol/src/surface.rs`](../runtime/crates/vowp-protocol/src/surface.rs)
  — `SessionOwner`, `OrchestratorAttach`, `OrchestratorEndpoint`, `EndpointAccess`.
- [`runtime/crates/vowp-agent/src/orchestrator.rs`](../runtime/crates/vowp-agent/src/orchestrator.rs)
  — the agent endpoint: `location: node`, `owner: None`, `access: ReadOnly`.
- [`runtime/crates/vowp-cli/src/cmd/agent.rs`](../runtime/crates/vowp-cli/src/cmd/agent.rs)
  — `vowp agent orchestrator` has only `endpoint`, not `attach`.

StackSpirit:

- [`docs/ORCHESTRATOR-CHAT.md`](ORCHESTRATOR-CHAT.md) §4.9 and §5 — the read-only
  ownership contract, the renderer boundary and the separate worker-session
  takeover.
- [`docs/HANDOFF-DESKTOP-UI.md`](HANDOFF-DESKTOP-UI.md) §5.1 item 3 — the owner
  decision and the open takeover/writer-fencing item.
- [`docs/SERVER-CHAT-OPTIONS.md`](SERVER-CHAT-OPTIONS.md) §1.1 — the direct-attach
  lease gap and why the proxy is the eventual enforcement point.
- `crates/spirit-core/src/engine/chat/` — `model.rs` (`ChatOwnershipView`,
  `ChatAccess`), `endpoint.rs` (`ResolvedEndpoint`, the agent resolver),
  `local.rs` (`send_turn`, the session record, `ChatEndpoint`).
- `crates/spirit-core/src/vowp/client.rs` — `session_takeover`, the separate
  one-shot worker-session path.
- `packages/spirit-ui/src/next/ChatView.tsx` — the chip, `ReadOnlyNotice` and the
  fleet `OwnershipBanner`.
- `packages/spirit-ui/src/bridge/SpiritBridge.ts` — `ChatBridgeSurface`, the
  optional `chat` methods and `sessions.takeoverSession`.
- `scripts/check-doc-links.mjs`, `scripts/check-public-readiness.sh` — the static
  checks for this document.
