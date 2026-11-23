# Orchestrator chat contract: bundled OpenCode server and the VOWP 0.5 endpoint

Status: implementation contract, 2026-11-18; live-probe facts applied 2026-11-18; the
Local `global`/`project` endpoint resolver landed 2026-11-19 (ss-61) behind the VOWP 0.5
command gate. The setup free-model default, inline `setModel` and usage projection landed
2026-11-19 (ss-72). A real free-model turn on 2026-11-20 proved the `msg` `messageID`
prefix (the `msg_<ULID>` id form) and the live `tokens.total` shape (ss-74); `messageID`
log preservation and the `message.part.*` cadence remain owner-turn unknowns. Live
tool-activity rows and explicit inline permission Allow/Reject cards landed 2026-11-20
(ss-81); the pinned `1.19.5` `/doc` proves the permission list and reply routes, while a
live tool/permission turn remains owner verification. OpenCode `session.status` retry
surfacing and Stop recovery landed 2026-11-20 (ss-83): a free-tier limit now shows a calm
inline notice with Switch model, Connect a provider and Stop retrying, and Stop aborts the
turn and re-attaches the same session ready, so it never locks the composer. Server chat
remains gated.
Audience: the follow-up chat jobs named in §8, the VOWP owner, and the desktop reviewer.

This document specifies how the Meadow shell's orchestrator chat moves from the preview store
in `packages/spirit-ui/src/next/conversations.ts` onto a real OpenCode server:

1. the exact local command, port policy, auth, health check, reuse rule and HTTP/SSE
   surface for the bundled StackSpirit Engine;
2. the server path through VOWP `0.5`'s planned `vowp orchestrator endpoint`; the
   app-owned SSH loopback forward is designed but **not approved** (decision A,
   2026-11-19);
3. the bridge types, methods, event flow, state machine and `MockBridge` behavior;
4. the security rules that keep provider keys, surface tokens, SSH key paths, endpoint
   URLs and remote paths out of the renderer;
5. small, disjoint follow-up jobs with `owns` and `needs`, and an explicit split between
   what can land now and what is blocked on VOWP `0.5`.

The document is deliberately precise about what the current tree proves, what the live
probe of the pinned `opencode` `1.19.5` sidecar proved, and what still needs an owner model
turn or a server endpoint. The live probe (ss-43) fetched the manifest-pinned sidecar,
ran the repository probe in a private XDG tree, and fetched `/doc`; §2.5 records its
answers. The 2026-11-20 real free-model turn recorded the `msg` `messageID` prefix (the
`msg_<ULID>` id form) and the live token shape; do not turn an unproven delta cadence,
user-message log preservation or other real-turn runtime behavior into a claim.

Contents: §1 vocabulary and rules; §2 local engine and v1 HTTP/SSE contract; §3 server
endpoint and SSH forward; §4 bridge types, methods and state machine; §5 security; §6
error states; §7 what can land now; §8 follow-up jobs; §9 owner verification; §10 open
questions; §11 references; §12 chat UX state after ss-116..ss-118.

| Surface | State on 2026-11-19 |
|---|---|
| Setup assistant on the bundled engine | Contract complete; the live probe pinned the v1 surface; core/view jobs can land; no VOWP dependency |
| `global`/`project` on the home | **Gated** on VOWP `0.5` `orchestrator endpoint`; the app-owned forward (§3.3) is not approved |
| `global`/`project` Local on the owner's Mac | **Live**, verified 2026-11-19: the installed VOWP returns a `global` endpoint document (loopback URL, Basic auth, `native_session_id: null`), and the ss-62 continuity rule attaches to the newest host session. |
| `global`/`project` Local on this Linux host | **Still `chat_not_configured`** — not live. `/home/dev/.local/bin/vowp` is `0.4.0`; its `agent orchestrator` subcommand has no local orchestrator record on this host (`vowp agent orchestrator endpoint global --json` exits `3` with `not_found`), so there is no endpoint to attach to. The bundled engine is not silently used as a fallback. |
| Local/Server switch | Out of scope; `vowp orchestrator move` (VOWP `0.5`, HANDOFF §5 step 6) |
| Preview chat in `MockBridge` | Stays, explicitly `origin: "preview"` |

## 0. Sources and pinned versions

| Item | Value | Evidence |
|---|---|---|
| Desktop design source | `docs/HANDOFF-DESKTOP-UI.md` §3 "Sessions", §4 "Real versus preview", §5 step 4 | Read in this job |
| Preview chat store and view | `packages/spirit-ui/src/next/conversations.ts`, `ChatView.tsx`, `NextShell.tsx` | Read in this job |
| Bridge surface and DTOs | `packages/spirit-ui/src/bridge/{wire,SpiritBridge,MockBridge}.ts` | Read in this job |
| Engine lifecycle and process | `crates/spirit-core/src/engine/{process,runtime,mod,supervisor}.rs` | Read in this job |
| Desktop engine commands and state | `apps/spirit-desktop/src-tauri/src/commands/engine.rs`, `state.rs` | Read in this job |
| Bundled engine pin | `engine/manifest.json`: product `opencode`, version `1.19.5`, MIT, loopback host `127.0.0.1`, health `/global/health` | Read in this job |
| Live pinned-engine probe | `.vowp/jobs/ss-43-open-chat-probe-live/report.md`: `opencode` `1.19.5` on `x86_64-unknown-linux-gnu`; `serve --help`, `GET /doc` (OpenAPI 3.1.0, 162 paths), session/message/abort shapes, SSE and restart facts | Read in this job; incorporated in §2.1–§2.5 |
| Fetch/verify script | `scripts/fetch-engine.mjs` (downloads and verifies the pinned sidecar) | Read in this job |
| VOWP installed locally | `vowp 0.4.0` | Read-only `vowp --version` |
| VOWP `0.5` scope | `vowp` repository `docs/design/v0.5/scope.md`, accepted 2026-11-18; `orchestrator endpoint` and `orchestrator move` are **planned** | Read in this job |
| VOWP local-orchestrator design | `vowp` repository `docs/design/v0.4/local-orchestrator.md` | Read in this job |
| VOWP protocol shapes | `vowp` repository `crates/vowp-protocol/src/{surface,agent,session}.rs` | Read in this job |
| OpenCode app-surface spike | `vowp` repository `docs/maintainers/v0.2-spikes/app-surfaces.md`; it pins opencode **2.0.28**, which is **not** the StackSpirit pin | Reference only |

The `vowp` repository is the source of truth for VOWP commands and JSON. The OpenCode
binary is the source of truth for its HTTP API. If this document disagrees with either,
fix the document and the contract test; do not guess at runtime.

## 1. Vocabulary and non-negotiable rules

| Term | Meaning in this document |
|---|---|
| **StackSpirit Engine** | The bundled `opencode` sidecar pinned in `engine/manifest.json`, supervised by `spirit-core`. It listens on an ephemeral loopback port. |
| **Orchestrator server** | The OpenCode server that owns one orchestrator's native session. It may be the StackSpirit Engine (when VOWP reports that pairing), or a VOWP-managed `opencode serve` on the Mac or the home. |
| **Native session** | An OpenCode `session` document/id. It is not a VOWP session id and not the shell's `SessionId`. |
| **VOWP session** | The durable session record and log on the home. VOWP owns it and mirrors attached OpenCode turns into it. |
| **Shell `SessionId`** | The routing key `"setup" \| "global" \| "project:<name>"`. It is a UI key, not a native or VOWP id. |
| **Location** | Where the orchestrator server actually runs for this session: `local` (this Mac) or `server` (the selected home). For `global`/`project`, both keep the durable log on the home; `local` never means "local-only log". |
| **Transport** | The concrete way the app reaches that server: direct loopback (local engine) or a verified SSH loopback forward (server). |
| **Turn** | One user message and the assistant work it starts. A turn is in flight from `send` until the server signals idle or the app stops it. |
| **Bridge** | The `SpiritBridge` implementation: `TauriBridge` on desktop, `MockBridge` for preview, `HttpSseBridge` for Web. |

Rules that every follow-up job must preserve:

1. **One durable log for `global`/`project`.** Setup is local-only by design. Global and
   project sessions keep their log on the home. The app only attaches to an OpenCode
   server that VOWP reports (or to the bundled engine for setup); it never creates an
   unmirrored native session under a global/project key.
2. **One bundled engine.** The app never starts a second `opencode serve` for models or
   setup. The existing `EngineSupervisor` is the only owner of the bundled process. Chat
   reuses it, waits for it, or reports an honest engine error.
3. **No fake assistant reply in the real path.** A real bridge either receives model text
   or shows an error/gate. It never falls back to a canned reply, and it never sets
   `origin: "preview"`.
4. **The chip reflects the actual transport.** The current label-only Local/Server switch
   is removed from `ChatView`. The chip is derived from `ChatSessionView.location`, which
   is derived from the endpoint, not from a user toggle. Moving an orchestrator is
   `vowp orchestrator move` (VOWP `0.5`, HANDOFF §5 step 6), not this contract.
5. **The renderer never receives a credential, endpoint URL, SSH key path or remote
   path.** It receives only the safe `ChatSessionView`, `ChatMessage` and `ChatEvent`
   shapes in this document. The core holds the URL, port, auth header, forward process and
   working directory.
6. **Loopback only.** The bundled engine binds `127.0.0.1`. The SSH forward binds
   `127.0.0.1`. There is no public listener, no `0.0.0.0`, and no wildcard `-L` bind.
7. **Explicit owner approval only.** If OpenCode asks for a tool permission, the chat
   pauses the turn, shows an inline "Permission needed" card with the redacted request,
   and sends nothing until the owner clicks Allow once, Always allow or Reject. Nothing
   auto-approves.

## 2. Local path

### 2.1 The exact bundled command and lifecycle

The bundled engine is the sidecar resolved by `EngineBinaryLocator::for_current_exe()`:
next to the application executable, in its `binaries/` directory, in `../Resources`
(including `../Resources/binaries`), or in `../lib/stackspirit`. It **never** resolves `opencode` from `PATH`, the current
directory or a shell profile. The sidecar file is `opencode-<TARGET_TRIPLE>` (or
`opencode.exe-<TARGET_TRIPLE>` on Windows); tests and repair flows may use an explicit
binary, but the chat client never chooses one.

The command is exactly:

```text
<BUNDLED_OPENCODE> serve --hostname 127.0.0.1 --port <RESERVED_PORT>
```

`serve_args` in `crates/spirit-core/src/engine/process.rs` is the source of truth. The live
`serve --help` of the pinned `1.19.5` sidecar lists exactly these options:

```text
-h, --help
-v, --version
--print-logs
--log-level
--pure
--port
--hostname
--mdns
--mdns-domain
--cors
```

`--port` defaults to `0` (ephemeral), `--hostname` to `127.0.0.1`, `--mdns` to `false`
(enabling it "defaults hostname to 0.0.0.0"), `--mdns-domain` to `opencode.local` and
`--cors` to `[]`. There is **no** `--auth`, `--password`, `--token` or `--secret` flag, and
the help text contains none of those words. The launch uses only `serve --hostname
127.0.0.1 --port <RESERVED_PORT>` and must never add `--mdns`, `--cors`, `--print-logs` or
`--pure`. The child starts with:

- `stdin = null`, `stdout`/`stderr` piped to the bounded, redacted `EngineLogBuffer`
  (200 lines, 1,000 characters per line);
- `OPENCODE_AUTH_CONTENT` in the child environment **only** when the OS keychain holds
  one or more provider keys. That value is a provider-auth document, not a server token.
  It never appears in argv, a request URL, a file written by StackSpirit or a log line;
- **no** `OPENCODE_SERVER_PASSWORD` or `OPENCODE_SERVER_USERNAME`. The bundled engine keeps
  the optional HTTP Basic credential unset and relies on loopback plus the private XDG tree
  (see §2.1.2). Because the current spawn inherits the desktop environment, `chat-core`
  must remove these two variables explicitly;
- no `current_dir` in the current code. The working directory is therefore inherited from
  the desktop process, which is not a stable workspace (see §2.1.3). The chat job sets it
  to the private setup workspace.

#### 2.1.1 Port reservation and release

`reserve_loopback_port()` does exactly this:

1. `TcpListener::bind(("127.0.0.1", 0))` asks the OS for an ephemeral loopback port.
2. Read `.local_addr().port()`.
3. Drop the listener immediately.
4. Pass that explicit port to the child: `--port <RESERVED_PORT>`.

The release is the listener drop, before the child starts. The window is tiny; the child
is given the explicit port, so it can never bind a public interface. If the child fails
to bind because another process took the port in that window, health never passes and the
supervisor reports `Degraded`/`Failed`; the explicit Restart action takes a fresh port.
There is no fixed port and no port reuse across engine restarts. Chat must always read
`EngineStatus.port` (or the endpoint's local port) and treat a changed port as a new
endpoint. The mock's port is a fixture; never hardcode it.

The engine port is released when the child is killed. `ProcessEngineRuntime::shutdown`
kills and reaps the child; `Drop` calls `kill_now`; `EngineSupervisor::stop` clears
`status.port`; `EngineSupervisor::Drop` kills the runtime. The desktop app exiting leaves
no orphan. Chat does not reserve a port for the bundled engine and does not spawn the
child itself.

#### 2.1.2 Auth and health

- **Local auth:** the pinned `1.19.5` build has **no CLI auth, password or token flag**.
  It does honor an optional HTTP Basic credential from the environment:
  - **`OPENCODE_SERVER_PASSWORD` set:** every HTTP route, including
    `GET /global/health`, requires HTTP Basic. Without credentials it returns `401` with
    `WWW-Authenticate: Basic realm="Secure Area"`; the correct password returns `200`; a
    wrong password or another user returns `401`. The default username is `opencode`;
    `OPENCODE_SERVER_USERNAME` replaces it.
  - **Both variables unset:** unauthenticated `GET /global/health` returns `200`, and
    startup prints this exact warning on **stdout** before the listening line:

    ```text
    Warning: OPENCODE_SERVER_PASSWORD is not set; server is unsecured.
    opencode server listening on http://127.0.0.1:<PORT>
    ```

    With the password set, the warning is absent. Startup stderr is empty in both cases.
  - **StackSpirit decision:** the bundled engine leaves both variables unset and relies on
    loopback plus the app-private XDG tree. The app does not implement Basic auth for the
    bundled engine yet. The "server is unsecured" warning is the expected startup line and
    must not be treated as an engine failure; it is captured by the existing stdout reader.
    Because the current spawn inherits the desktop environment, `chat-core` must
    **explicitly remove** `OPENCODE_SERVER_PASSWORD` and `OPENCODE_SERVER_USERNAME` from the
    child env. If the desktop itself is launched with the password set and the child
    inherits it, the app's unauthenticated health check would 401. This is a required
    change, not optional hardening.
  - `OPENCODE_AUTH_CONTENT` is provider auth, not a server credential; never mistake it for
    one, never add it to a request, and never log it.
- **Health:** `GET http://127.0.0.1:<PORT>/global/health` with a 3 s read timeout. The
  body must contain `"healthy": true`; `"version"` is optional and is surfaced as the
  engine version. A non-2xx status, a non-JSON body or `healthy != true` is an engine
  error. The live probe returned `200` with `{ "healthy": true, "version": "1.19.5" }`.
- **Other proven engine reads:** these are for the model catalog and provider list, not
  chat. `GET /doc` is an OpenAPI document, not a chat read.
  - **Model catalog order:** read `GET /config/providers` first (3 s). Fall back to
    `GET /provider` (10 s; the document can be several MiB) only when that read fails
    (a transport error or a non-2xx status; a 2xx answer is used as is). `/provider` is not gone: it stays the model-catalog fallback.
    `fetch_providers` (the BYOK provider list) is a separate call and still reads
    `/provider` first; do not treat the provider list and the model catalog as one read.
  - **Connected filter:** when a provider document carries a `connected` id array (the
    live `/provider` does: `connected: ["opencode"]`), only models from those providers
    are usable. `all` entries outside `connected` are disconnected catalog rows (the live
    document lists 100+ of them) and must not be offered. The array is optional: the
    parser filters only when it is present, and otherwise keeps every row.
  - **Live failure:** selecting a disconnected row fails the turn with
    `ProviderModelNotFoundError` (HTTP 500); the old `/provider`-first code offered
    `opencode-go/beam-free` and hit exactly that. Fixed in `39171a6`; regression test
    `parse_models_filters_disconnected_providers_when_connected_list_is_present`.
- **No CORS and no browser fetch:** the renderer never calls the engine. All requests go
  through `spirit-core` and Tauri commands.

#### 2.1.3 Private engine state and workspaces (required change)

The current `spawn_child` does not set `XDG_DATA_HOME`, `XDG_STATE_HOME` or
`XDG_CONFIG_HOME`. That means the bundled engine shares the owner's personal OpenCode
state: its session list, its `opencode.json`, its MCP servers and its logs. Chat must not
show or mutate the owner's personal sessions, and the setup assistant must not inherit an
unrelated project config.

The core chat job must start the bundled child with an app-private tree:

```text
<APP_DATA>/engine/data      -> XDG_DATA_HOME
<APP_DATA>/engine/state     -> XDG_STATE_HOME
<APP_DATA>/engine/config    -> XDG_CONFIG_HOME
<APP_DATA>/engine/workspaces/setup
<APP_DATA>/engine/workspaces/global
<APP_DATA>/engine/workspaces/project/<PROJECT_NAME>
```

All directories are created `0700`; any generated `AGENTS.md` is `0600`. The child's
`current_dir` is set to `<APP_DATA>/engine/workspaces/setup`, so the setup assistant has a
deterministic single workspace; the directory header in §2.2.2 is honored, but the setup
assistant still uses one workspace. The engine's data, state and config roots are the
app-private tree; it does not use the owner's OpenCode session, config or state
directories. The provider key still arrives only through `OPENCODE_AUTH_CONTENT`. This is
a change to `ProcessEngineRuntime` and needs a regression test that the env and
`current_dir` are set on the child and that a session created in one app run is still
listed after an engine restart.

The live probe verified the persistence half with the pinned binary: a session was created
in a private `HOME`/`XDG_*` tree, the child was stopped with SIGTERM (no forced kill), the
server restarted on the same tree and port, and all created sessions were still listed and
retrievable. The owner's OpenCode data, config, state and cache roots were unchanged
across the run. The regression test itself is still missing in the current tree; this is
the contract for `chat-core`, not a claim that the current spawn already does it.

The runtime needs the app-data root at construction (for example
`ProcessEngineRuntime::with_private_state(<APP_DATA>)`); `AppState::new` already receives
that directory. The existing `for_current_exe()` constructor keeps its default for tests,
which use an explicit fake binary and no private tree.

The `setup` workspace is the only one needed to land the first real chat. Its
`AGENTS.md` says, in substance: "You are the StackSpirit setup assistant. Never ask for a
password, token or key in chat. Use the secure server card. Do not claim a server is
connected until StackSpirit confirms a verified key-only login. Ask one short question at
a time." It contains no secret and no machine path.

For a project workspace, the core resolves the project's local checkout from VOWP's
path map. If the project is not mapped, the error is `path_unmapped` and the UI points to
Settings → Advanced → Engine → Local orchestrator paths. The renderer never receives the
absolute checkout path.

#### 2.1.4 If the app already runs the engine for models

The engine is a singleton inside `SpiritCore`/`AppState`. `engine_status` calls
`ensure_engine_setup`, and `store_key` stops and starts a live bundled engine after a key
change. Chat must never call `ProcessEngineRuntime::spawn`, `reserve_loopback_port` or
`Command::new` directly. `chat.start` calls the same `ensure_engine_setup` path (or a
small wrapper with the same semantics) and then attaches:

| Engine state | Chat action |
|---|---|
| `NotConfigured` | Start once; wait for `Ready`; attach. |
| `Starting` | Wait up to the supervisor's 10 s health budget (41 probes, 250 ms apart — 40 intervals); then attach or return `engine_not_ready`. |
| `Ready` with a port | Reuse it. Verify `/global/health` with a 3 s timeout first; if unhealthy, restart once and wait. |
| `Stopped` | Start once; wait; attach. |
| `Degraded`/`Failed` | Restart once through the supervisor; if still not `Ready`, return the engine's redacted `last_error`. |
| `External` | Do **not** spawn or guess a port. For setup, return `engine_external` ("the setup assistant needs the bundled StackSpirit Engine; switch to it in Settings → Advanced"). For `global`/`project`, use the VOWP endpoint; the external harness is model-only. |
| Restarted while attached | The `SpiritEvent::Engine` event publishes the new status. The event pump sees the port change, reconnects, re-reads the session log and dedupes. A short `connecting` state is normal; do not show an error unless the restart fails. |
| Two windows / two views | One engine, one event pump per endpoint, one chat service. Views subscribe; they do not spawn or duplicate the stream. |

If the app already runs a **VOWP local orchestrator server** as well, that server is not
the bundled engine. For `global`/`project`, the endpoint is authoritative: attach to the
server VOWP reports, and leave the bundled engine for setup/models. Do not kill VOWP's
server on app exit, and do not assume `EngineStatus.port` is the orchestrator. The setup
assistant still uses the bundled engine.

#### 2.1.5 What starts a native session

- `setup`: the app creates one native session in the private setup workspace, with the
  title `StackSpirit setup`. It is local-only; no VOWP session and no server log. It may
  be resumed across app restarts from the private data dir.
- `global`/`project` local — the target is a VOWP local orchestrator. When VOWP reports a
  `native_session_id`, the app attaches to it. When VOWP reports `null`, the app lists the
  host's own sessions and continues the newest one in the orchestrator workspace
  (§4.3, ss-62). It **attaches**, never creates, and never invents a reply; an empty or
  unreadable list is `chat_not_configured`, not a local-only session under a
  global/project key.
- `global`/`project` server: same rule; the VOWP endpoint reports the native session. The
  app attaches and never creates an unmirrored server session.

The app stores a small mapping in `<APP_DATA>/chat/sessions.json` (`0600`):

```json
{
  "schema": "stackspirit-chat-sessions/1",
  "sessions": {
    "setup": {
      "native_session_id": "<NATIVE_SESSION_ID>",
      "location": "local",
      "updated_at": "<TIMESTAMP>"
    },
    "global": {
      "host_id": "<HOST_ID>",
      "vowp_session_id": "<VOWP_SESSION_ID>",
      "native_session_id": "<NATIVE_SESSION_ID>",
      "location": "server",
      "updated_at": "<TIMESTAMP>"
    }
  }
}
```

It contains no token, URL, provider key, SSH path or remote path. The server mapping is a
cache; VOWP's endpoint wins on every attach. The app never deletes a native or VOWP
session to clean up; `stop` only aborts the turn and detaches.

### 2.2 OpenCode HTTP contract

The pinned OpenCode `1.19.5` uses the **v1** contract: `/global/health`, `/provider`,
`/config/providers`, `/session`, `/session/{id}/message`, `/session/{id}/abort`, `/event`
and `/doc`. The live `/doc` also lists many `/api/...` paths in this build. Their presence
is **not** proof that a server is the `2.0.28` v2 dialect; the client chooses this v1
contract and gates on `GET /global/health` (§3.4). Do not mix v1 and v2 paths at runtime.
The v2 spike is useful for event semantics and security lessons only; its paths are not
evidence for the pinned sidecar.

#### 2.2.1 Endpoint table

| Purpose | Request | Response (expected) | Status |
|---|---|---|---|
| Health | `GET /global/health` | `{ "healthy": true, "version": "<VERSION>" }` | **Proven live**: 200 with `healthy: true`, `version: "1.19.5"` |
| Create session | `POST /session` with `{ "title": "<TITLE>" }` | A `Session`: `{ "id", "slug", "projectID", "directory", "title", "version", "time": { "created", "updated" } }` | **Proven live**: 200; the runtime object also carried `cost`, `path` and `tokens` |
| List sessions | `GET /session` | `Session[]` | **Proven live**: every created session was listed |
| Get session | `GET /session/{id}` | `Session` | **Proven live** |
| Send a turn | `POST /session/{id}/message` with `{ "messageID"?: "<USER_MESSAGE_ID>", "model"?: { "providerID", "modelID" }, "parts": [{ "type": "text", "text": "<PROMPT>" }] }` | `{ "info": AssistantMessage, "parts": Part[] }` | **Proven live with a real turn (2026-11-20)**: the free-model turn returned 200 `{info, parts}` with `cost: 0` and `tokens.total: 12260`, `tokens.input: 12247`, `tokens.output: 3`, `tokens.reasoning: 10`, `tokens.cache.read: 0`; `/doc` remains the schema source (`parts` is the only required field, `messageID` and `model` are optional) |
| `messageID` prefix | `messageID` must start with `msg` | A `msg_<ULID>` id is accepted; any other prefix is rejected | **Proven live (2026-11-20)**: the real sidecar returned `HTTP 400 BadRequest` for a non-`msg` id (exact payload in §2.2.5); the assistant message carries `parentID: msg_...` |
| `messageID` in the user log | `GET /session/{id}/message` includes the user message with the sent `messageID` | Still unproven | **Owner-turn unknown**: `/doc` accepts the field, but preservation of the *user* message in the read log is not proven. Keep the role/text/time reconciliation in §4.6 until §9 pins it |
| Read the log | `GET /session/{id}/message` | `Array<{ "info": Message, "parts": Part[] }>` | **Proven live**: `[]` for a new session; `/doc` requires both `info` and `parts`; it is **not** a flat `Message[]`. The endpoint accepts `directory`, `workspace`, `limit` and `before` query parameters. The parser stays tolerant. |
| Abort a turn | `POST /session/{id}/abort` | `boolean` | **Proven live**: idle abort returns 200 `true`; the session still exists afterwards |
| Session activity | `GET /session/status` | A map keyed by native session id; each value is the `session.status` union (`idle`, `busy`, `retry`) | **Incident-proven (2026-11-20)**: the retry entry carried `attempt`, `message`, `next` epoch ms and `action.reason/title/message/provider`; used on attach/reconnect so a retry that started before the UI attached is visible |
| List pending permissions | `GET /permission` | `PermissionRequest[]`: `{ "id", "sessionID", "permission", "patterns", "metadata", "always", "tool"? }` | **Proven by `/doc`**; the core filters to the attached native session id |
| Answer a permission | `POST /permission/{requestID}/reply` with `{ "reply": "once" \| "always" \| "reject" }` | `boolean` | **Proven by `/doc`**; fake-engine tested |
| Legacy permission reply | `POST /session/{sessionID}/permissions/{permissionID}` with `{ "response": "once" \| "always" \| "reject" }` | `boolean` | **Proven by `/doc`**; fallback only when the primary route returns 404/405 |
| Live events | `GET /event`, `Accept: text/event-stream` | SSE frames with `data: { "id", "type", "properties" }` | **Proven live**: 200 `text/event-stream`; first `server.connected`; `server.heartbeat` about every 10 s |
| OpenAPI | `GET /doc` | OpenAPI 3.1.0 document with 162 paths | **Proven live**: 200 `application/json`, 479,667 bytes |
| Delete session | `DELETE /session/{id}` | `{ "ok": true }` | **Proven** by the key-verification cleanup; chat must **not** use it |

The runtime `Session` keys were `cost`, `directory`, `id`, `path`, `projectID`, `slug`,
`time`, `title`, `tokens` and `version`. `/doc` also allows optional `agent`, `metadata`,
`model`, `parentID`, `permission`, `revert`, `share`, `summary` and `workspaceID`, and
`time` may also carry `compacting` and `archived`. The required subset above is correct;
keep ignoring unknown fields and keep `directory` core-only.

The `model` field is optional in the request. Use it for the local setup turn: split the
selected `ModelChoice.id` at the first `/` into `providerID`/`modelID` exactly as
`verify_against_engine` does. For a server-hosted orchestrator, omit `model` and let the
orchestrator's own OpenCode config choose, unless the VOWP endpoint reports a model that
the client is allowed to use. The renderer never constructs this body.

The current key-verification path creates a session with `{}`, sends one message with a
20 s timeout, and deletes the session. Chat must not copy that lifecycle: it keeps the
session, uses a 10-minute turn cap, and aborts instead of deleting.

#### 2.2.2 Workspace / directory selection

An OpenCode server has a default directory. The live probe confirmed that a foreground
`serve` defaults a new session's `directory` to the child's `cwd` (the private `base`
workspace), and that both selectors work on the pinned `1.19.5`:

- `POST /session` with `x-opencode-directory: <WORKSPACE>` created a session whose
  `directory` was that workspace, and `GET /session/{id}` with the same header returned
  200. The header is honored but **not documented** in `/doc`.
- `POST /session?directory=<WORKSPACE>` also created a session in that workspace; `/doc`
  documents the `directory` query on create, list and message reads.

The implementation rule:

1. The core sends `x-opencode-directory: <WORKSPACE>` on session create/list/message/log
   requests. `<WORKSPACE>` is resolved in core; the renderer never supplies it.
2. The pinned build honors the header on create/get, so keep it first. If a later build or
   an endpoint rejects it, fall back to the documented `directory` query rather than
   guessing.
3. The setup assistant still uses a single private workspace (the setup directory).
   A Local `global`/`project` session attaches to VOWP's reported orchestrator endpoint
   (§4.3, ss-61); it does not send the setup workspace header and does not select a
   workspace from the renderer. Directory selection is not the gate. Do not ship a
   project session that silently writes to the app's inherited working directory.

The session response's `directory` field is core-only. The core must drop it before any
`ChatSessionView` is serialized. It is a raw local path, and the renderer must never see
it.

#### 2.2.3 SSE event mapping

`GET /event` is a global SSE stream for the server, not a per-session stream. One pump
per endpoint reads it and fans out to the matching chat sessions. The pump must not open
one SSE connection per `ChatView`. It ignores events for sessions it does not own,
including the temporary session used by key verification.

Expected frame shape (tolerant parser; the pinned `/doc` includes an `id` that the parser
ignores):

```text
data: {"id":"<EVENT_ID>","type":"server.connected","properties":{}}

data: {"id":"<EVENT_ID>","type":"server.heartbeat","properties":{}}

data: {"id":"<EVENT_ID>","type":"message.updated","properties":{"info":{"id":"<MESSAGE_ID>","role":"assistant","time":{"created":0,"completed":null},"error":null}}}

data: {"id":"<EVENT_ID>","type":"message.part.updated","properties":{"sessionID":"<NATIVE_SESSION_ID>","part":{"id":"<PART_ID>","messageID":"<MESSAGE_ID>","sessionID":"<NATIVE_SESSION_ID>","type":"text","text":"<TEXT_SO_FAR>"},"time":0}}

data: {"id":"<EVENT_ID>","type":"message.part.delta","properties":{"sessionID":"<NATIVE_SESSION_ID>","messageID":"<MESSAGE_ID>","partID":"<PART_ID>","field":"text","delta":"<DELTA>"}}

data: {"id":"<EVENT_ID>","type":"session.idle","properties":{"sessionID":"<NATIVE_SESSION_ID>"}}
```

The parser accepts `data:` lines, concatenates multi-line data until a blank line, ignores
comments (`:`), and ignores unknown event types. It may accept an `event:` field but does
not require one. The pinned build uses `{ "id", "type", "properties" }`; the parser ignores
`id` and still accepts a missing one. The event type is `type` and the payload is
`properties`.

| OpenCode event | Chat action |
|---|---|
| `server.connected` | Mark the pump connected; no UI message. |
| `server.heartbeat` | Liveness only; `properties` is empty. Explicitly ignore it; do not render it or log a line per heartbeat. Any byte still counts under the 60 s rule in §2.2.4. |
| `session.updated` | Refresh the session title/metadata if present; no text. |
| `message.updated` with `info.role == "assistant"` | Create or update the assistant message shell (`id` from `info.id`, `origin: "model"`, `streaming: true`). If `info.error` is non-null, map it to the error rules in §6 and stop the turn. |
| `message.part.updated` with `part.type == "text"` | The cumulative `TextPart.text` source. Set the part text to the new `text` (never append it) and compute the append-only delta from the last text for `(messageID, partID)`. If the new text starts with the old text, emit a `delta` for the suffix. If it does not (a rewrite, shrink or non-prefix update), emit a `message` event with the full replacement text. |
| `message.part.delta` with `field == "text"` | Append `delta` to that `(messageID, partID)`. A `field` other than `text` is ignored. The pinned probe cannot prove the emission cadence without a model turn; the core may apply deltas or explicitly ignore them and rely on `message.part.updated` plus the `session.idle` log backfill. It must pick one behavior and must not claim token-by-token streaming. |
| `message.part.updated` with `part.type == "tool"` | Queue one coalesced log refresh for the session; the refreshed log renders compact redacted tool rows from the part state. Raw input, output and error text are never rendered. |
| `message.part.removed` | Remove that part's text; if the message has no text left and no error, remove the placeholder. |
| `message.removed` | Minor note only: a full-message removal is not reflected in the rendered log until the next log read; `session.idle`/reattach/reconnect backfill covers it. The pump may drop the message if present, but must not treat the event as the durable deletion. |
| `session.idle` | Read the log once if the assistant text may be incomplete, then emit `done` for the active assistant message; set the session `ready`. This is the reconciliation point when deltas are ignored. |
| `session.status` | Parse the `idle`/`busy`/`retry` union from `properties.status`. `retry` sets `ChatSessionView.retry` (`attempt`, `message`, `next` epoch ms and `action.reason/title/message/provider`) and keeps the turn in `streaming`; `idle`/`busy` clear it. The retry is also read from `GET /session/status` on attach/reconnect so a session already asleep in a retry loop surfaces immediately. |
| `session.error` | Emit `error`; set the session `error`. Do not show the raw error object. |
| `permission.asked` | Publish a `ChatPermissionPrompt` in `pending_permissions` and leave the turn waiting. Nothing auto-approves. If the event carries no id, reconcile with `GET /permission` filtered to the native session id. |
| `permission.replied` | Remove the matching prompt. An id-less reply also reconciles with `GET /permission`. |
| Everything else | Ignore. Do not render raw tool JSON, step internals or delta-only UI events. |

`permission.updated` is **not** in the pinned `/doc`; the v1 event name is
`permission.asked`. Watching only `permission.updated` would make a permission request
invisible and could hang the turn. The v1 schema requires the request `id`; the pump still
reconciles with `GET /permission` when a runtime event omits it, instead of dropping the
request. A request must never become invisible and hang the turn.
`permission.v2.asked`/`permission.v2.replied` are v2-only and belong to a later v2
adapter, not this pump.

`message.part.updated` is the cumulative source of truth: set the text to the part's
`text`, never append it. `message.part.delta` is a separate event; when the core applies
it, append only the `delta` for `field == "text"`. The pinned `TextPart` has no `delta`
field. The probe cannot prove whether the delta event is emitted per token, per part, or
at all during a real turn, so the core has two honest choices: apply deltas and reconcile
them against cumulative updates, or explicitly ignore deltas and rely on
`message.part.updated` plus the `session.idle` log backfill. Pick one and do not describe
either as token-by-token streaming.

The event stream is not a durable log. The pump is allowed to miss frames. Every
`start`/`attach`/reconnect reads `GET /session/{id}/message` and replaces the rendered
messages with the log before it resumes applying events. Each `ChatEvent` carries a
monotonic `revision`; a gap in the renderer triggers a `chat.log` backfill for that
session.

A single SSE connection can exceed one ChatView's lifetime. It lives while at least one
chat session on that endpoint is attached. It is closed when the last session detaches,
the engine stops, the host disconnects or the app exits.

#### 2.2.4 Timeouts, cancellation and reconnect

| Operation | Timeout / retry |
|---|---|
| TCP connect to a loopback endpoint | 700 ms |
| Health check | 3 s read |
| Session create / get / list / log | 3 s read; no automatic retry inside one UI action |
| `POST /session/{id}/message` | The Tauri command returns a receipt after the request is accepted locally. The background task may wait up to **10 minutes** for the turn. There is no 20 s chat timeout; the key-verification 20 s is not the chat contract. |
| Abort | 3 s; one retry |
| SSE connect | 700 ms TCP, 5 s to response headers |
| SSE idle | 60 s without a byte → treat as disconnected and reconnect. The server sends comments, `server.connected` and a `server.heartbeat` data event with empty `properties` about every 10 s; any byte, including a comment, counts as liveness. |
| SSE reconnect | 5 attempts with 250 ms, 500 ms, 1 s, 2 s, 4 s (jittered); then `stream_lost`, state `error`, manual Retry. |
| Log page | 3 s; at most 200 messages per page; 50 by default |

The live probe saw `server.connected` first (about 11 ms after connect) and
`server.heartbeat` at about 10.6, 20.7, 30.7, 40.7, 50.8 and 60.8 s. A 3 s SSE window cannot
establish that interval; the 65 s observation did. `server.heartbeat` is an undocumented
runtime data event (it is not in `/doc`) and is explicitly ignored, but it still satisfies
the liveness rule. The 60 s rule is unchanged.

Cancellation:

- The primary cancel is `POST /session/{id}/abort`. Stop is a **turn control, not a
  session teardown**: as soon as the abort is sent the core clears `active_turn`, `retry`
  and `pending_permissions`, keeps the native session attached, invalidates the background
  turn and returns the `stopped` view with the legacy `closed`/`stopped` notice. The client
  immediately `attach`es the same native session, which returns `ready`; the composer is
  usable without a separate Start step, so the owner can switch model or send another turn.
  A late `POST message` response from the aborted turn is ignored by its turn sequence.
- If abort is unsupported or unconfirmed (404/405 or a transport error), the app still
  re-attaches ready and shows a non-fatal `stop_unconfirmed` notice: "The turn may still
  be running on the orchestrator." It does **not** delete the session and does not pretend
  the turn stopped.
- `stop` is not `vowp orchestrator stop`. It never stops the VOWP app host and never
  deletes the native session.

Reconnect:

1. On an SSE error, keep the background `POST message` alive; it may still return the
   final assistant message.
2. Reconnect with the backoff above.
3. Before resuming events, `GET /session/{id}/message` and replace the message list.
4. If the engine port changed (an engine restart), resolve the new port, verify the
   session with `GET /session/{id}`, and resume. If the session is 404, fail with
   `session_lost`.
5. If SSE is permanently unavailable, set `streaming_supported: false` and use the
   `POST message` response as one complete assistant `message` event. The UI says "Live
   text is unavailable; the reply appears when the turn completes." This is honest and
   still real; no fake text.

#### 2.2.5 Streaming fallback and the send sequence

`chat.send` must be safe to call once per user action:

1. Reject if the session is not `ready` (`chat_busy` or `chat_not_started`) or the text is
   empty/too long (`invalid_input`).
2. Trim the text, cap it at 32,000 characters, and create the user `ChatMessage` with
   `origin: "user"`, `role: "user"`, `created_at` now and a `msg_<ULID>` id. Send that
   id as `messageID`. The prefix is mandatory: on 2026-11-20 the real bundled `1.19.5`
   sidecar rejected any other prefix with `HTTP 400 BadRequest`:

   ```json
   {"name":"BadRequest","data":{"message":"Expected a string starting with \"msg\", got \"local:...\"\n  at [\"messageID\"]","kind":"Payload"}}
   ```

   The same request with `msg_<ULID>` returned `HTTP 200` and a real assistant message.
   The assistant reply references the user message as `info.parentID`. Whether the user
   message itself appears in `GET /session/{id}/message` is still an owner-turn unknown,
   so keep the role/text/time reconciliation fallback in §4.6.
3. Set the session `streaming` and emit a `session` event.
4. Start (or reuse) the endpoint's event pump. If the pump cannot start within 5 s, mark
   `streaming_supported: false` and continue with the final-response path.
5. Spawn the `POST /session/{id}/message` task with the 10-minute cap.
6. Return `ChatSendReceipt { session, message, accepted: true }` immediately. `accepted`
   means the request was accepted locally, not that the model answered. The core also
   broadcasts the user message as a `message` event for other subscribers; the receipt and
   the event share the id, so the store upserts rather than duplicating.
7. When the POST returns:
   - If the assistant message was already emitted by SSE (same `info.id`), do not emit a
     duplicate; emit `done` and set `ready`.
   - If no SSE text arrived, emit one complete `message` event from the response and then
     `done`.
   - If the response carries `info.error`, map it per §6, emit `error`, and set `error`.
8. When the turn ends with no text and no error, emit `done` with an empty message and
   remove the placeholder. Never invent a reply.

The `ChatEvent.delta` only appends. A non-prefix rewrite is represented by a `message`
event that replaces the message's text. The renderer must replace, not append, on that
event.

### 2.3 Proven, live-probed and still unknown

| Claim | Status | Evidence / probe |
|---|---|---|
| The bundled binary is chosen only from the sidecar/resource dirs, never `PATH` | **Proven** | `EngineBinaryLocator` and its test |
| The argv is `serve --hostname 127.0.0.1 --port <ephemeral>` | **Proven** | `serve_args`, `spawn_child`, test asserting no `0.0.0.0` |
| The port is reserved by binding `127.0.0.1:0` and dropping the listener | **Proven** | `reserve_loopback_port` |
| Health is `GET /global/health` with `healthy: true` | **Proven live** | `ProcessEngineRuntime::health`, fake engine, and the probe: 200 with `healthy: true` and `version: "1.19.5"` |
| `POST /session` with `{}` creates a session | **Proven live** | `verify_against_engine` and the probe (`{"title":"<TITLE>"}`); the live response was a `Session` |
| `POST /session/{id}/message` accepts `{ model, parts }` | **Proven live with a real turn (2026-11-20)** | `verify_against_engine` and the live turn: 200 `{info, parts}` on a free model with `cost: 0` and `tokens.total: 12260` (`input 12247`, `output 3`, `reasoning 10`, `cache.read 0`). `/doc` remains the schema source: `parts` is the only required body field; `messageID` and `model` are optional |
| `messageID` must start with `msg` | **Proven live (2026-11-20)** | The real sidecar returned `HTTP 400 BadRequest` for a non-`msg` id (exact payload in §2.2.5); `msg_<ULID>` was accepted on the live turn and the assistant message carries `parentID: msg_...` |
| The user message with the sent `messageID` is preserved in `GET /session/{id}/message` | **Owner-turn unknown** | `/doc` accepts the field, but the live turn did not pin the *user*-message log entry. Keep the role/text/time reconciliation in §4.6 until §9 pins it |
| The model catalog is `/config/providers`, not `/provider.all` | **Proven live (2026-11-20)** | The live `/provider` document lists 100+ disconnected catalog providers in `all` next to `connected: ["opencode"]`; offering `opencode-go/beam-free` from `all` failed a turn with `ProviderModelNotFoundError` (HTTP 500). Fixed in `39171a6`: `/config/providers` is read first, `/provider` is the fallback, and `all` is filtered by `connected`. Regression test `parse_models_filters_disconnected_providers_when_connected_list_is_present`. `connected` is optional; the parser filters only when it is present |
| `DELETE /session/{id}` exists | **Proven** | key-verification cleanup |
| A provider error can arrive in an HTTP 200 body (`info.error.statusCode`) | **Proven** | `classify_message_response`; `/doc` types `AssistantMessage.error` as the provider-error union |
| The child is killed on stop/drop; no orphan | **Proven** | process tests |
| The engine is a singleton and `ensure_engine_setup` restarts an unhealthy `Ready` engine | **Proven** | `state.rs` |
| `store_key` restarts a live bundled engine | **Proven** | `commands/engine.rs` |
| `GET /session`, `GET /session/{id}`, `GET /session/{id}/message` shapes | **Proven live** | Probe and `/doc`: `Session[]` and `{info,parts}[]`; the message log is not a flat list. Keep the tolerant parser |
| `GET /event` SSE path, event names and `{id,type,properties}` envelope | **Proven live** | 200 `text/event-stream`; first `server.connected`; `server.heartbeat` about every 10 s; the spike is v2, not evidence |
| `message.part.updated` carries cumulative text, not only deltas | **Proven by `/doc`; real-turn payload/cadence owner-turn** | `/doc` `TextPart.text` is cumulative and has no `delta`; the separate `message.part.delta` event is also pinned |
| `message.part.delta` is a separate append event | **Proven by `/doc`; cadence owner-turn** | `/doc`: `{sessionID,messageID,partID,field,delta}`; `field == "text"` appends. The probe cannot prove emission without a model turn |
| `POST /session/{id}/abort` exists and cancels a turn | **Proven live** | 200 boolean `true`; an idle abort is safe and the session still exists |
| `x-opencode-directory` selects the workspace | **Proven live** | Honored on create/get (undocumented); the documented `?directory=` query is also honored |
| A private `XDG_*` tree isolates sessions and config | **Proven live** | Probe: create, SIGTERM, restart with the same tree, list/retrieve every session; the owner's OpenCode roots were unchanged. The `ProcessEngineRuntime` regression test is still required by §2.1.3 |
| The local `serve` has no CLI auth flag, but `OPENCODE_SERVER_PASSWORD` is an optional env credential | **Proven live** | `serve --help` has no auth flag; set: 401 / 200 with Basic; unset: expected "server is unsecured" warning and 200 unauthenticated |
| The server orchestrator uses OpenCode v1 | **Assumed** | Health/version gate; the live 1.19.5 `/doc` also lists `/api/...` paths, but their presence is not proof of v2 |
| VOWP `0.5` `orchestrator endpoint` ships with auth metadata | **Planned, not shipped** | VOWP `0.5` scope; `vowp 0.4.0 --help` has no endpoint command |
| VOWP can attach a surface to an already-running StackSpirit Engine | **Not available in 0.4.0** | `vowp agent --help` has no `surface` command; PLAN §9.8 says no attach command |
| `vowp orchestrator move` preserves the native session | **Landed (`ss-97`, `ss-98`); live Mac check pending** | `vowp orchestrator move --help` exits 0; HANDOFF §5 step 6 |
| `POST /session/{id}/message` accepts an omitted `model` | **Schema confirms optional; runtime server/owner** | `/doc` marks `model` optional; a successful server turn is still an owner/server check |
| `GET /doc` exposes OpenAPI | **Proven live** | 200 `application/json`, OpenAPI 3.1.0, 162 paths, 479,667 bytes |
| `permission.asked` is the v1 permission event | **Proven by `/doc`** | `permission.updated` is absent; `permission.replied` and v2-only `permission.v2.*` are separate |
| The pinned permission reply route | **Proven by `/doc`** | The pinned `1.19.5` `/doc` lists `POST /permission/{requestID}/reply` with `{ "reply": "once" \| "always" \| "reject" }` and the legacy `POST /session/{sessionID}/permissions/{permissionID}` with `{ "response": ... }`. The core uses the former and falls back to the latter only on 404/405; both are fake-engine tested |
| `GET /permission` lists pending requests for a native session | **Proven by `/doc`; fake-tested** | `PermissionRequest` requires `id`, `sessionID`, `permission`, `patterns`, `metadata` and `always`; the core keeps the entries for the attached native id |
| Tool parts carry a compact status and input | **Proven by `/doc`** | `ToolPart.state` is `pending`, `running`, `completed` or `error`; the core reads only the tool name, status and a known input hint, never output or error text |
| `server.heartbeat` is a liveness data event | **Proven live; undocumented in `/doc`** | Empty `properties`, about every 10 s; explicitly ignored, any byte counts |
| `message.removed` exists | **Proven by `/doc`; minor** | `{sessionID,messageID}`; a full removal is covered by the next log read/backfill |

### 2.4 Probe commands

Run these only against the **pinned** sidecar, never the `opencode` on `PATH`, and never
with a provider key or a paid model turn. The probe job ran these commands (or the
equivalent repository probe) against the pinned `1.19.5` sidecar; keep them for a manual
re-probe. The commands use a temporary private XDG tree, disable model fetching and
autoupdate, and use loopback only. Replace `<TARGET_TRIPLE>` with the host triple. Do not
commit the probe transcript if it contains a real path or session id; the probe report
uses placeholders.

```sh
BIN="apps/spirit-desktop/src-tauri/binaries/opencode-<TARGET_TRIPLE>"
PROBE_ROOT="$(mktemp -d)"
export HOME="$PROBE_ROOT/home"
export XDG_DATA_HOME="$PROBE_ROOT/data"
export XDG_STATE_HOME="$PROBE_ROOT/state"
export XDG_CONFIG_HOME="$PROBE_ROOT/config"
export OPENCODE_DISABLE_MODELS_FETCH=1
export OPENCODE_DISABLE_AUTOUPDATE=1
# The pinned server honors this optional HTTP Basic credential on every route. Leave it
# unset for the unauthenticated loopback case; otherwise every curl below returns 401.
unset OPENCODE_SERVER_PASSWORD OPENCODE_SERVER_USERNAME
mkdir -p "$HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$XDG_CONFIG_HOME"

"$BIN" --version
"$BIN" serve --help

PORT="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
"$BIN" serve --hostname 127.0.0.1 --port "$PORT" >"$PROBE_ROOT/serve.log" 2>&1 &
SERVE_PID=$!

for _ in $(seq 1 100); do
  curl -fsS "http://127.0.0.1:$PORT/global/health" && break
  sleep 0.1
done

# OpenAPI, if present: the safest source for every path and schema.
curl -fsS "http://127.0.0.1:$PORT/doc" >"$PROBE_ROOT/openapi.json" || true

# Create one session. This does not call a model.
curl -fsS -X POST "http://127.0.0.1:$PORT/session" \
  -H 'content-type: application/json' \
  -d '{"title":"probe"}' >"$PROBE_ROOT/session.json"
SESSION_ID="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["id"])' "$PROBE_ROOT/session.json")"
python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("directory"))' "$PROBE_ROOT/session.json"

# Directory selection: create a second session with the v1 header and compare its
# reported directory. This still does not call a model.
PROBE_WORKSPACE="$PROBE_ROOT/workspace"
mkdir -p "$PROBE_WORKSPACE"
curl -fsS -X POST "http://127.0.0.1:$PORT/session" \
  -H 'content-type: application/json' \
  -H "x-opencode-directory: $PROBE_WORKSPACE" \
  -d '{"title":"probe-directory"}' >"$PROBE_ROOT/session-dir.json"
python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("directory"))' "$PROBE_ROOT/session-dir.json"

# List sessions and the (empty) message log. No model.
curl -fsS "http://127.0.0.1:$PORT/session"
curl -fsS "http://127.0.0.1:$PORT/session/$SESSION_ID/message"

# SSE hello: proves framing and server.connected. A 3 s window cannot show the ~10 s
# server.heartbeat; use a 65 s window for that.
timeout 3 curl -fsS -N -H 'Accept: text/event-stream' "http://127.0.0.1:$PORT/event" || true

# Abort on an idle session: proves the route exists without starting a model turn.
curl -s -o /dev/null -w '%{http_code}\n' -X POST "http://127.0.0.1:$PORT/session/$SESSION_ID/abort"

kill "$SERVE_PID" 2>/dev/null || true
wait "$SERVE_PID" 2>/dev/null || true

# Restart with the same private tree and list sessions: proves persistence.
"$BIN" serve --hostname 127.0.0.1 --port "$PORT" >"$PROBE_ROOT/serve2.log" 2>&1 &
SERVE_PID=$!
for _ in $(seq 1 100); do curl -fsS "http://127.0.0.1:$PORT/global/health" && break; sleep 0.1; done
curl -fsS "http://127.0.0.1:$PORT/session"
kill "$SERVE_PID" 2>/dev/null || true
wait "$SERVE_PID" 2>/dev/null || true

rm -rf "$PROBE_ROOT"
```

Do not run `POST /session/{id}/message` in the probe. A real turn needs a model; the
brief forbids a paid model. The request body and the `{info,parts}` response are pinned
by `/doc`. The 2026-11-20 real free-model turn (outside this probe) pinned the `msg`
`messageID` prefix and the live token object; it did not establish `messageID` log
preservation or the real `message.part.updated` payload. The owner live verification in
§9 still runs one free included model turn and records the real delta cadence,
`session.idle` ordering and `messageID` preservation.

### 2.5 Live probe answers and remaining owner-turn unknowns

The live probe (ss-43) ran the pinned `opencode` `1.19.5` sidecar in a private XDG tree.
These answers are pinned; the items that need a model turn or a server endpoint stay
explicitly unknown below. Any answer that contradicts §2.2 is a contract change to this
document, not a runtime guess.

| # | Question | Live answer |
|---|---|---|
| 1 | `serve --help` flags and auth | The flags are exactly `-h/--help`, `-v/--version`, `--print-logs`, `--log-level`, `--pure`, `--port` (default `0`), `--hostname` (default `127.0.0.1`), `--mdns` (default `false`; it defaults the hostname to `0.0.0.0`), `--mdns-domain` (default `opencode.local`) and `--cors` (default `[]`). There is no `--auth`, `--password`, `--token` or `--secret` flag. `OPENCODE_SERVER_PASSWORD` (with optional `OPENCODE_SERVER_USERNAME`, default `opencode`) turns on HTTP Basic on every route: 401 without credentials, 200 with the right ones. Unset, the server prints `Warning: OPENCODE_SERVER_PASSWORD is not set; server is unsecured.` and serves 200 unauthenticated. `--mdns` must stay off. |
| 2 | Session and message JSON | `POST /session` returns a `Session` with `id`, `slug`, `projectID`, `directory`, `title`, `version` and `time`; the runtime also had `cost`, `path` and `tokens`. `GET /session` is `Session[]`. `GET /session/{id}/message` is an array of `{info, parts}` (not a flat list); it was `[]` for a new session. |
| 3 | `GET /event` | 200 `text/event-stream`; the first frame is `server.connected` at about 11 ms. The envelope is `{id, type, properties}`. `server.heartbeat` is a data event with empty `properties` about every 10 s (observed at 10.6, 20.7, 30.7, 40.7, 50.8 and 60.8 s). A 3 s window cannot establish the interval. |
| 4 | Abort | `POST /session/{id}/abort` exists and returns 200 with boolean `true` on an idle session; the session still exists afterwards, so an idle abort is safe. |
| 5 | Directory selection | A foreground `serve` defaults a new session to the child's `cwd`. `x-opencode-directory` is honored on create/get but is not documented in `/doc`; the documented `?directory=` query is also honored on create/list/message. |
| 6 | Restart persistence | With the same private XDG tree, a SIGTERM/restart (clean, no forced kill) kept all created sessions listed and retrievable. |
| 7 | Owner isolation | The child received a private `HOME`/`XDG_*`/`TMPDIR` tree; the owner's OpenCode data, config, state and cache roots were unchanged. The private tree was removed on exit. |
| 8 | `message.part.updated` payload | `/doc` pins cumulative `TextPart.text` and no `delta` field on the part. The pinned build has a separate `message.part.delta` with `{sessionID, messageID, partID, field, delta}`; `field == "text"` appends. The **real-turn payload and emission cadence are owner-turn unknowns**. |
| 9 | `POST /session/{id}/message` | `/doc` pins the body keys (`agent`, `format`, `messageID`, `model`, `noReply`, `parts`, `system`, `tools`, `variant`; only `parts` required), the optional `messageID` and `model`, the 200 `{info, parts}` response, and `AssistantMessage.error` as the provider-error union. Live 2026-11-20: the real sidecar requires a `msg` `messageID` prefix and returns HTTP 400 `BadRequest` for any other prefix (exact payload in §2.2.5); `msg_<ULID>` returned 200 `{info, parts}` with `cost: 0` and `tokens: {total: 12260, input: 12247, output: 3, reasoning: 10, cache: {read: 0, write: 0}}`. The assistant message carries `parentID: msg_...`. Preservation of the **user** message in `GET /session/{id}/message` and a real omitted-`model` turn remain **owner-turn/server unknowns**. |
| 10 | `/doc` | Present: 200 `application/json`, OpenAPI 3.1.0, 162 paths, 479,667 bytes. It confirms the v1 paths and shapes and also lists `/api/...` paths (not proof of a v2 server). It does not document `x-opencode-directory` or `server.heartbeat`. |

Still unknown without an owner model turn or a server endpoint:

- The `message.part.updated` and `message.part.delta` emission cadence, and whether the
  delta event is emitted at all during a real turn.
- The real `message.part.updated` payload in a live turn. The schema is pinned; the runtime
  contents and timing are not.
- `session.idle` ordering relative to the final `message.part.updated`.
- `messageID` preservation of the user message in `GET /session/{id}/message`. The `msg`
  prefix requirement itself is proven live (2026-11-20, §2.2.5); the user-message log
  entry is not.
- Runtime `info.error` behavior in a real provider failure. The 200-with-error envelope is
  already proven by the key-verification path; the probe did not repeat it.
- `POST /session/{id}/message` with an omitted `model` against a server-hosted orchestrator.
- Whether `x-opencode-directory` is required on `GET /session` and
  `GET /session/{id}/message`. It is proven on create/get; the query form is documented on
  all three.

These are deferred to the §9 owner verification. They are not blockers for the setup chat
contract and must not be turned into claims in code or UI.

## 3. Server path

> **Gate:** the Server path is **not enabled** until VOWP `0.5`'s
> `vowp orchestrator endpoint` ships. The owner declined the app-owned SSH forward in
> §3.3 (decision A, 2026-11-19), so it is not a gate option. Until the endpoint ships the
> UI shows a typed `chat_gated` error and never a fake reply. A Local `global`/`project`
> session uses the Mac-side `vowp agent orchestrator endpoint` instead (§4.3): it
> attaches when the installed VOWP provides the command and reports a mirrored session,
> and otherwise returns `chat_not_configured`/`chat_gated`/`chat_not_mirrored`.

### 3.1 The VOWP 0.5 endpoint (preferred)

The server path uses VOWP's read model for the orchestrator and its endpoint:

1. Read the orchestrator list over the verified SSH connection:
   `vowp --json orchestrator ls` (the existing `SshVowpClient` style). The response is an
   array of `OrchestratorCard` values.
2. Resolve the card for the shell session:
   - `global`: the card with `project == null` (the global orchestrator). If there is more
     than one, show a picker; do not guess. If there is none, `chat_not_configured`.
   - `project:<name>`: the card with `project == "<name>"`. If there is none,
     `chat_not_configured` with a hint to create it from the project's Workers page.
   - Only `app == "opencode"` is in this contract. A `claude-code` or `chat` orchestrator
     is `chat_unsupported_app`; do not try the OpenCode HTTP API.
   - A `state` other than `running`/`degraded` is a typed gate: `stopped` shows a Resume
     action owned by the existing orchestrator lifecycle, not by chat; `node_offline`/
     `failed` name the state. The chat never starts or resumes an orchestrator itself.
3. Read the endpoint with the planned VOWP `0.5` command:

   ```text
   vowp --json orchestrator endpoint <ORCHESTRATOR_NAME>
   ```

   The command is **not in VOWP 0.4.0**. `vowp orchestrator --help` lists only
   `new`, `ls`, `stop` and `resume`. `vowp 0.5`'s accepted scope says the command is
   planned. The app must gate on both the installed version (>= `0.5.0`) and a successful
   read; a version string alone is not proof (the existing gate rule).

4. For a Mac-hosted orchestrator, the local inspection command is planned as:

   ```text
   vowp --json agent orchestrator endpoint <ORCHESTRATOR_NAME>
   ```

   It runs on this Mac through the local agent client, not SSH. The app checks
   `LocalAgentView.state == "connected"` first. If the local agent is not enrolled or not
   connected, `local_agent_not_connected`; no fallback.

The expected endpoint document (a **proposed** shape; the 0.5 implementation pins it):

```json
{
  "name": "<ORCHESTRATOR_NAME>",
  "app": "opencode",
  "session_id": "<VOWP_SESSION_ID>",
  "location": "server",
  "node": "<NODE_NAME>",
  "state": "running",
  "url": "http://127.0.0.1:<REMOTE_PORT>",
  "native_session_id": "<NATIVE_SESSION_ID>",
  "auth": {
    "kind": "basic",
    "username": "<AUTH_USERNAME>",
    "password": "<AUTH_SECRET>"
  }
}
```

The exact auth delivery is the open question in §10. The client accepts only these
shapes, and only in core:

- `auth.kind == "basic"` with `username` + `password` (the password is a secret);
- `auth.kind == "header"` with `name` + `value` (the value is a secret);
- `auth` absent (loopback-trusted server).

If the document returns only a username and a file path, this version does **not** read
the file; it returns `vowp_incompatible` and asks the owner to upgrade. If the URL contains
userinfo (`user:pass@`), a query string, a path other than `/`, a non-loopback host or a
`0.0.0.0` host, reject with `vowp_incompatible`. The core accepts only `127.0.0.1`,
`localhost` or `[::1]` plus a port and normalizes `localhost` to `127.0.0.1`. The URL and
auth are never serialized to a `ChatSessionView`, `ChatMessage`, `ChatEvent` or Tauri
error.

`native_session_id` is normally reported for `global`/`project`. For the Local endpoint,
a null value triggers the host-session discovery rule in §4.3 (ss-62). The Server path is
still gated; it does not use discovery and does not create a native session. That is how
the contract enforces "the log always lives on the server".

If the home document reports `location: "local"` and `node` is this Mac, the app reads the
local endpoint with the local-agent command and uses that URL/auth; the home URL is not
used. If the node is a different machine, the chat is gated (`chat_not_configured` or
`node_offline`); StackSpirit does not dial another Mac.

### 3.2 The SSH loopback forward

When the endpoint lives on the home, the Mac reaches it through the app's own SSH loopback
forward. This is the same verified SSH path as every other host command, using
`SshOptions` and `HostProfile`, not a new trust path.

Exact argv shape:

```text
ssh
  -N
  -o BatchMode=yes
  -o StrictHostKeyChecking=yes
  -o UserKnownHostsFile=<APP_DATA>/known_hosts
  -o ConnectTimeout=8
  -o ServerAliveInterval=15
  -o ServerAliveCountMax=3
  -o ExitOnForwardFailure=yes
  -o ForwardAgent=no
  -o ForwardX11=no
  -o PermitLocalCommand=no
  [-i <IDENTITY_FILE> -o IdentitiesOnly=yes]
  -p <SSH_PORT>
  -L 127.0.0.1:<LOCAL_PORT>:127.0.0.1:<REMOTE_PORT>
  -- <SSH_USER>@<SSH_HOST>
```

Rules:

- The local bind address is explicit `127.0.0.1`; never `*`, `0.0.0.0`, `-g` or a
  wildcard `-L`.
- The remote side is `127.0.0.1:<REMOTE_PORT>`, the server's loopback. The remote host is
  not configurable by the renderer.
- `<LOCAL_PORT>` is reserved with the same `reserve_loopback_port()` helper (bind
  `127.0.0.1:0`, read, drop). `ExitOnForwardFailure=yes` turns a bind race into an
  immediate exit; retry exactly once with a fresh port, then fail with
  `ssh_forward_failed`.
- The forward is started only for a `HostProfile` whose host-key pin is already verified
  and whose connection state is `connected`. A failed host-key check is
  `host_key_mismatch`; there is no "ignore" or "accept new" option in this flow.
- The identity file is the profile's path, passed to `-i` with `IdentitiesOnly=yes`. The
  path is core-only and is never put in an error, a log line or the DOM.
- The forward process is one per `(host_id, remote_port)`, ref-counted by attached chat
  sessions. Start it before the first HTTP request; wait for health (3 s) or process exit.
  Kill it when the last session detaches, the host disconnects, the endpoint changes or
  the app exits. Never leave an orphan `ssh`.
- Capture stderr bounded and redacted. Do not log the argv (it contains the app-private
  known_hosts and identity paths). Do not show the stderr tail raw; map it to the safe
  codes in §6.
- No `-A`, `-X`, `-D` or `-R`; no `sshpass`; no password prompt (`BatchMode=yes`).
- The forward URL is `http://127.0.0.1:<LOCAL_PORT>`; the remote port and URL stay in
  core. The auth header from the endpoint is attached to every request by the HTTP client.

If the owner already runs VOWP's tunnel manager, `vowp --json tunnel open <NAME>
--remote-port <REMOTE_PORT>` can produce an equivalent local port and `vowp tunnel close
<NAME>` cleans it up. It is **not** the default: it uses VOWP's own SSH configuration,
not StackSpirit's app-private known_hosts and pin. Use it only as an explicitly
owner-managed alternative, and never assume its local port in the app's own flow.

### 3.3 The app-owned SSH forward (designed, **not approved** by the owner on 2026-11-19)

**Appendix only:** decision **A** (`ss-50`, 2026-11-19) closed this path — Server chat
waits for VOWP `0.5`'s `vowp orchestrator endpoint`. The design below is kept for
reference and is not a live fallback.

The design assumed the server path could be enabled only by an explicit owner approval.
That approval is not a secret and is not a default:

1. The owner enables the forward in Settings → Advanced → Chat (the separate
   `chat-settings` job). The chat gate banner links there; the chat panel carries no
   hidden toggle.
2. The owner supplies the orchestrator's **remote loopback port** and its **auth
   credential** (if any) through app-owned secure fields. The credential is write-only:
   the host consumes it once, keeps it in zeroizing memory for the session, and never
   returns, logs or persists it. The owner re-enters it after an app restart.
3. The app starts the forward from §3.2 with the supplied remote port and uses the
   supplied credential only in the HTTP `Authorization` header. Once the forward is
   healthy, it probes `GET /global/health` **before sending any turn**. A v2 `/api/...`
   server is closed and reported as `protocol_unsupported` until a v2 adapter job ships
   (see §8). The probe never sends a model turn.
4. The owner did not approve this path (decision A), so the server path stays gated. The
   UI says exactly: "Server chat needs VOWP 0.5's `vowp orchestrator endpoint`." It does
   not show a fake reply.

Do not parse VOWP's internal `host.json`, `service.json` or snapshot files in this
fallback. They are internal, may contain a password, and are not a stable contract. The
endpoint command is the supported read.

### 3.4 Version gate: v1 vs v2

The pinned StackSpirit Engine is `1.19.5` (v1 contract). The VOWP spike used opencode
`2.0.28` (v2 API under `/api/...`). A server orchestrator may be either. The client must
not guess:

- Probe `GET /global/health` on the endpoint. Success → v1 dialect; use this contract.
- If it returns 404 and `GET /api/...` style health succeeds, the server is v2. Return
  `protocol_unsupported` with a hint that the server orchestrator must run the pinned
  `1.19.5` sidecar or wait for the v2 adapter.
- The VOWP `0.5` endpoint should report an app version/dialect. If it does, use it; if it
  does not, the health probe above is authoritative.
- The live `1.19.5` `/doc` lists many `/api/...` paths alongside the v1 paths. Do not
  treat their presence as proof of v2; a health-200 server still uses this v1 contract.
  The client gates on `/global/health`, not on the absence of `/api/...`.
- The v2 spike's `/api/session`, `/api/event` and Basic-auth findings are not evidence
  for the pinned sidecar. A v2 adapter is a separate job, not a runtime branch in this
  contract.

### 3.5 Session-location rule

| Shell session | Where it runs | Where the durable log lives | Chip |
|---|---|---|---|
| `setup` | The bundled engine, this Mac | Nowhere durable; local setup transcript only | Fixed `Local · this Mac`; no switch |
| `global` | The VOWP orchestrator for the global session, on the home or a reported Mac node | The home (VOWP session log) | `Server · <HOST_LABEL>` or `Local · this Mac` exactly as the endpoint reports |
| `project:<name>` | That project's VOWP orchestrator, on the home or a reported Mac node | The home (VOWP session log) | Same rule |

- A `local` location for `global`/`project` means "the orchestrator server runs on this
  Mac and VOWP mirrors its turns to the home log". It never means "local-only log".
- If VOWP cannot report a mirrored endpoint, the session is not attached. The app does
  not fall back to the bundled engine, the preview store or a local-only log.
- The current `⇄ Run on server` / `Run on this Mac` button is removed. It is a label-only
  switch today; the real move is `vowp orchestrator move` (VOWP `0.5`, HANDOFF §5 step 6)
  and is out of scope of this contract. The chip is read-only.
- **Host control (cross-reference, not part of the engine contract).** The session host can
  be moved explicitly with `vowp orchestrator move` through the chat `Host` control
  (HANDOFF §5 step 6; `ss-97`, `ss-98`). It is distinct from the read-only transport
  location chip: the chip only reports where the attached endpoint runs, while the Host
  control asks VOWP to move the orchestrator. A successful move refreshes the card and asks
  the user to reconnect.
- If the host is disconnected, the chip shows the last known location with a disconnected
  state and the composer is disabled. No silent fallback.

### 3.6 What is gated

| Path | State on 2026-11-18 | Gate |
|---|---|---|
| Setup assistant on the bundled engine | Can land now | The live probe §2.5 pins the v1 API details; no VOWP dependency |
| `global`/`project` on the server, VOWP endpoint | Blocked | VOWP `0.5` `orchestrator endpoint` (not shipped) |
| `global`/`project` on the server, app-owned forward | Not approved | Owner decision A (2026-11-19): designed but declined; Server chat waits for VOWP `0.5` `orchestrator endpoint` |
| `global`/`project` Local on a VOWP local orchestrator | Implemented behind the VOWP 0.5 command gate; fake-`vowp` tests only here | `vowp agent orchestrator endpoint` (0.5). The installed `vowp 0.4.0` binary has no such subcommand, so the typed result there is `chat_not_configured`; the app attaches to the reported endpoint and never uses the bundled engine for this session |
| Explicit session host move | Landed (`ss-97`, `ss-98`); live Mac check pending | `vowp orchestrator move` through the chat `Host` control, HANDOFF §5 step 6; the transport chip stays read-only and the D6 gate stays `Waiting` until the owner's agent reports `0.4.0+` |
| Live chat through the bundled engine for setup | Ready for the core job | §2 |

## 4. Bridge contract

### 4.1 Move the four preview types into `wire.ts`

`packages/spirit-ui/src/bridge/wire.ts` becomes the single source for these types. The
preview store imports and re-exports them so existing imports keep working during the
transition.

```ts
/** Where an orchestrator session runs, as reported by the transport. */
export type SessionLocation = "local" | "server";

/** "setup" is the local first-run assistant; "global" and projects are orchestrators. */
export type SessionId = "setup" | "global" | `project:${string}`;

export type ChatMessageRole = "user" | "assistant";
export type ChatMessageOrigin = "user" | "model" | "app" | "preview";

export interface ChatMessage {
  /** Stable across reconnects; derived from the native message id when one exists. */
  id: string;
  role: ChatMessageRole;
  /** Paragraphs of plain text; never HTML, markdown or a tool dump. */
  text: string[];
  /** Canonical RFC 3339 UTC string with millis. */
  created_at: Timestamp;
  /** Who produced this message. The real path never sets "preview". */
  origin: ChatMessageOrigin;
  /** True while assistant text is still arriving. */
  streaming?: boolean;
  /**
   * Compact, redacted tool-call rows for this assistant turn. Absent when the message
   * has no tool parts; a later update without it keeps the rows already delivered.
   */
  tools?: ChatToolActivity[];
  /**
   * A details-panel hint. Only app/preview messages set it; model text never does.
   * The renderer maps it to `PanelContent`.
   */
  show?: ChatPanelTarget | null;
}

export type ChatToolStatus = "pending" | "running" | "completed" | "error";

/** One compact tool-call row. Never carries raw input, output or error text. */
export interface ChatToolActivity {
  /** The native part id. */
  id: string;
  /** The tool name, for example `bash`. */
  name: string;
  status: ChatToolStatus;
  /** A short redacted hint such as the command or path. */
  summary?: string;
}

/** One permission request the engine is waiting on. */
export interface ChatPermissionPrompt {
  /** The native request id, needed to reply. */
  id: string;
  /** The requested action, for example `bash` or `external_directory`. */
  permission: string;
  /** The requested patterns, already redacted for display. */
  patterns: string[];
  created_at: Timestamp;
}

export type ChatPermissionDecision = "once" | "always" | "reject";

/** The provider action OpenCode attaches to a retry status. */
export interface ChatRetryAction {
  /** Stable reason such as `free_tier_limit` or `rate_limit`. */
  reason: string;
  /** Short title suitable for a notice heading. */
  title: string;
  /** One or two calm sentences from the provider. */
  message: string;
  /** Provider id such as `opencode`. */
  provider: string;
}

/**
 * One active `session.status` retry. OpenCode can sleep for hours before the next
 * attempt; the UI shows this instead of a permanent “Replying…” line. `next` is epoch
 * milliseconds.
 */
export interface ChatRetryStatus {
  attempt: number;
  message: string;
  next?: number | null;
  action?: ChatRetryAction | null;
}

/** The details panel target, moved here so `MockBridge` need not import `next/`. */
export type ChatPanelTarget =
  | { kind: "overview" }
  | { kind: "server" }
  | { kind: "inbox" }
  | { kind: "job"; project: string; jobId: string };

/** What the add-project dialog collects. The path is user input, never a leaked path. */
export interface ProjectRequest {
  name: string;
  source:
    | { kind: "git"; url: string }
    | { kind: "folder"; path: string };
}
```

The following stay in `next/conversations.ts` because they are renderer-only and never
cross the bridge:

```ts
import type { ChatMessage, PasswordAuthOutcome } from "../bridge/wire";

/** A UI-only card rendered inside a conversation instead of plain text. */
export type ChatCard =
  | { kind: "secure-server" }
  | { kind: "secure-received"; host: string; fingerprint: string };

/** The renderer's message: transport text plus app-owned cards. */
export interface ChatMessageView extends ChatMessage {
  card?: ChatCard;
}

/** The verified facts a successful onboarding may put in the conversation. */
export interface SecureServerReceipt {
  host: string;
  fingerprint: string;
  password_auth: PasswordAuthOutcome;
}
```

`next/shellUi.ts` imports `ChatPanelTarget` and aliases it:

```ts
import type { ChatPanelTarget } from "../bridge/wire";
export type PanelContent = ChatPanelTarget;
```

This keeps the bridge package free of `next/` imports and preserves the existing
`openPanel({ kind: "overview" })` calls.

Validation:

- `SessionId` is validated in core as `setup`, `global`, or `project:<PROJECT_NAME>`.
  `<PROJECT_NAME>` uses the same slug rules as VOWP; a path, `..`, `/`, a NUL byte or a
  non-UTF-8 value is `invalid_input`. The renderer only sends names, never paths.
- `ProjectRequest.source.path` is the one path-like user input in the add-project dialog.
  It is not a leaked server path: the user chose it. Core validates it and never echoes
  it in an error or a view. Add project is live (`ss-99`, `ss-100`) and passes no
  `--branch`; HANDOFF §5 step 6 records it.

### 4.2 New wire types

```ts
export type ChatSessionState =
  | "idle"
  | "connecting"
  | "streaming"
  | "ready"
  | "stopped"
  | "error";

export type ChatModelTier = "included" | "byok" | "orchestrator";

export interface ChatModelView {
  id: string;
  label: string;
  provider: string;
  /** Added in ss-72; older bridges omit it. */
  tier?: ChatModelTier;
  /** Context window when the engine reports it; otherwise null/absent. */
  context_tokens?: number | null;
}

/**
 * Token and context usage reported by the engine. Every counter is `null` when the
 * engine did not report it; a value is never invented.
 */
export interface ChatUsageView {
  context_tokens_used: number | null;
  context_limit: number | null;
  input_tokens: number | null;
  output_tokens: number | null;
  cache_read_tokens: number | null;
  turns: number;
  cost_usd: number | null;
}

export interface ChatSessionView {
  session: SessionId;
  /** The actual transport location, never the requested label. */
  location: SessionLocation;
  state: ChatSessionState;
  /** True when a native session is attached and usable. */
  attached: boolean;
  /** False when the server has no usable SSE; the UI says so. */
  streaming_supported: boolean;
  model: ChatModelView | null;
  /**
   * Verified models this session's engine can use right now: included free runtime
   * models plus models whose provider key is configured. `undefined` means the
   * transport cannot report a list (an orchestrator owns its config); an empty array
   * is an honest "no usable model".
   */
  available_models?: ChatModelView[];
  /** Live token/context usage, or null/absent when the engine reported none. */
  usage?: ChatUsageView | null;
  started_at: Timestamp | null;
  last_event_at: Timestamp | null;
  error: SpiritErrorDto | null;
  /** Permission requests the engine is waiting on, oldest first. Absent when none. */
  pending_permissions?: ChatPermissionPrompt[];
  /** An active retry wait, absent when OpenCode is not sleeping before another try. */
  retry?: ChatRetryStatus | null;
}

export interface ChatStartInput {
  session: SessionId;
  /** The project name for project:<name>; null for setup/global. Never a path. */
  project: string | null;
  /**
   * A transport hint. Setup is always local. For global/project, VOWP's endpoint is
   * authoritative and may override this hint; the reply reports the actual location.
   */
  location: SessionLocation;
  /**
   * Reuse the stored native session when true (default). For `setup` only,
   * `false` creates a new local session and leaves the old log intact.
   * `global`/`project` never create a session here; they attach to VOWP's.
   */
  resume?: boolean;
}

export interface ChatSendInput {
  session: SessionId;
  text: string;
}

export interface ChatSendReceipt {
  session: SessionId;
  /** The user message as stored. Render this; do not invent another one. */
  message: ChatMessage;
  /** True once the request was accepted locally; assistant text arrives as events. */
  accepted: boolean;
}

export interface ChatLogQuery {
  /** Opaque cursor from a previous page; null/absent starts at the newest page. */
  cursor?: string | null;
  /** 1..200; defaults to 50. */
  limit?: number;
}

export interface ChatLogPage {
  messages: ChatMessage[];
  /** Opaque cursor for older messages; null at the start. */
  next: string | null;
  /** True when the local cache dropped messages and this page may have a gap. */
  truncated: boolean;
}

export type ChatCloseReason =
  | "stopped"
  | "engine_stopped"
  | "host_disconnected"
  | "session_lost"
  | "failed";

export type ChatEvent =
  | { kind: "session"; revision: number; session: ChatSessionView }
  | { kind: "message"; revision: number; session: SessionId; message: ChatMessage }
  | {
      kind: "delta";
      revision: number;
      session: SessionId;
      message_id: string;
      part_id: string | null;
      text: string;
    }
  | {
      kind: "done";
      revision: number;
      session: SessionId;
      message_id: string;
      truncated: boolean;
    }
  | { kind: "error"; revision: number; session: SessionId; error: SpiritErrorDto }
  | { kind: "closed"; revision: number; session: SessionId; reason: ChatCloseReason };
```

`revision` is a monotonic per-bridge counter. A gap (`event.revision > last + 1`) makes
the renderer call `chat.log` for that session and replace the messages. A `message` event
is authoritative and replaces a message with the same `id`; a `delta` only appends.

No type above has a `url`, `port`, `token`, `authorization`, `password`, `key_path`,
`identity_file`, `directory`, `binary_path`, `pid`, `native_session_id`, `ssh_target` or
`remote_port` field. That is intentional. The core keeps those values private.

`ChatModelView.tier` marks an included free runtime model, a configured BYOK model, or an
orchestrator-owned choice. `available_models` is the verified usable list for this
session's engine, in the engine's order; it is not a build-time catalog copy. It holds
the connected providers' models from the preferred catalog read (`/config/providers`
first, `/provider` as fallback, filtered by `connected` when present; see §2.1.2). A
disconnected catalog entry is not part of `available_models`, and the picker must not
show it: selecting one fails the turn with `ProviderModelNotFoundError` (HTTP 500).
`ChatUsageView` comes from the OpenCode assistant message metadata: `total`, `input`,
`output`, `reasoning`, `cache.read`/`cache.write` and `cost`, or `step-finish` parts when
a turn has several steps. `context_tokens_used` is the latest assistant turn's reported
`tokens.total` when present (the live shape's 12,260 includes 10 reasoning tokens);
otherwise it is the sum of the reported components, including reasoning. `input_tokens`,
`output_tokens`, `cache_read_tokens` and `cost_usd` sum the reported assistant messages;
a missing field stays `null`; it is never reported as zero. `turns` counts user messages.
The usage object is omitted when no assistant message reported any of these values.

**Setup auto-default (ss-72).** When the bundled engine is ready and no model has been
explicitly saved, the first verified free runtime model is selected in memory and the
composer unlocks. A configured BYOK model is the fallback only when the engine reports no
free model. If the runtime reports zero free models and no BYOK provider key is configured,
`model` stays `null` and `available_models` is an empty array; no fake free model is
constructed and `send` returns `no_model`. The automatic choice is not written to
`engine.json`; only an explicit `setModel` persists it.

### 4.3 The `chat` bridge surface

Add this optional namespace to `SpiritBridge` in
`packages/spirit-ui/src/bridge/SpiritBridge.ts`:

```ts
export interface ChatBridgeSurface {
  /** Ensure the engine/endpoint and attach/create the native session; idempotent. */
  start(input: ChatStartInput): Promise<ChatSessionView>;
  /** Reuse the stored native session; never creates one. */
  attach(session: SessionId): Promise<ChatSessionView>;
  /** Accept one user turn; assistant text arrives via `subscribe`. */
  send(input: ChatSendInput): Promise<ChatSendReceipt>;
  /** Abort the current turn (when supported) and return the stopped view. */
  stop(session: SessionId): Promise<ChatSessionView>;
  /**
   * Switch the setup assistant's model to one in `available_models` and return the
   * updated view. Persists locally; never writes to VOWP. An orchestrator owns its own
   * model config, so that session rejects. Optional for older bridges.
   */
  setModel?(session: SessionId, modelId: string): Promise<ChatSessionView>;
  /**
   * Answer one pending permission request from an explicit owner click. Nothing is ever
   * approved implicitly; a request that is already gone resolves with the current view.
   * Optional for older bridges.
   */
  replyPermission?(
    session: SessionId,
    permissionId: string,
    decision: ChatPermissionDecision,
  ): Promise<ChatSessionView>;
  /** Read the conversation log; pages move backward from newest, each oldest-first. */
  log(session: SessionId, query?: ChatLogQuery): Promise<ChatLogPage>;
  /** Returns an unsubscribe function; callers must clean up on unmount. */
  subscribe(listener: (event: ChatEvent) => void): () => void;
}

export interface SpiritBridge {
  // ... existing members ...
  /** Optional orchestrator chat. Presence is the capability; Web omits it. */
  chat?: ChatBridgeSurface;
}
```

No `BridgeCapabilities` field is needed. Desktop `TauriBridge` and `MockBridge` implement
`chat`; `HttpSseBridge`/`WebMockBridge` omit it and the Web shell keeps its read-only
behavior. The desktop shell checks `bridge.chat` and shows `chat_unavailable` instead of
the preview store when it is absent.

Semantics:

- `setModel` is setup-only. The core validates the session and the model id, asks the
  bundled engine adapter to select a model from `available_models`, and the adapter writes
  the explicit choice to the local engine preferences (`selected_model_id`) before
  returning the updated view. An automatic first-run default never writes that file. A
  `global`/`project` session rejects with `chat_model_gated` because VOWP's orchestrator
  owns its model config; StackSpirit does not pretend to change it.
- `start` is idempotent. For `setup`, it ensures the bundled engine and creates one native
  session if none is attached; `resume: false` creates a new one and leaves the old log
  intact. For `global`/`project`, `start` resolves the VOWP endpoint and attaches to the
  native session VOWP reports, or continues the host's newest existing session when VOWP
  reports none (§4.3, ss-62); it **never creates one**. An unusable reported native id or
  a different `session_id` is `chat_not_mirrored`; if the mapping is stale, `session_lost`.
  `resume: false` on a `global`/`project` session is a gate ("create a new orchestrator
  session in VOWP"), not a new unmirrored local session.
- **Local `global`/`project` endpoint source (ss-61).** For `location: "local"`,
  `start`/`attach` run `vowp agent orchestrator endpoint <ORCHESTRATOR_NAME> --json` —
  `global` for `ChatSessionId::Global`, the project name for
  `ChatSessionId::Project(name)` — with argv only, a bounded 10 s budget and a minimal
  child environment (`PATH`, `HOME`, `VOWP_AGENT_HOME` only; no inherited provider key
  or `OPENCODE_AUTH_CONTENT`). They attach to the reported loopback `url` and
  `native_session_id`; they never call `ensure_engine`, create a native session or copy
  the setup model into the orchestrator turn. This path is conditional on the installed
  VOWP providing the subcommand: VOWP `0.5` ships it and `0.4.0` does not, so a missing
  or failed command is `chat_not_configured`, a null `url`, a non-`running`/`degraded`
  `state` or a non-`opencode` `app` is `chat_gated`, and an unusable reported native id
  or a different `session_id` than the attached log is `chat_not_mirrored`. The endpoint
  URL/host/port, auth value, VOWP `session_id`, `native_session_id` and workspace path
  stay core-only; no chat DTO, error, log or report carries them. This bullet opens only
  the Local endpoint. Server remains gated pending the owner's transport decision (A).
- **Continue the existing session when no native id is reported (ss-62).** The OpenCode
  orchestrator host does not put a session id in its `service.json`, so a live local
  endpoint may report `native_session_id: null`. When that happens, `start`/`attach`
  must discover the host's existing conversation before attaching:
  1. `GET /api/session` on the same reported loopback URL, with the reported Basic
     credential and, when VOWP reports a workspace, the
     `x-opencode-directory: <ORCHESTRATOR_WORKSPACE>` header. The current VOWP endpoint
     document does not carry a workspace; the transport accepts `workspace` /
     `working_dir` / `directory` forward-compatibly. When none is reported, the
     discovered session's own `location.directory` becomes the workspace header for the
     attach and log requests.
  2. Parse the documented `{"data":[{"id","time":{"updated"},"location":{"directory"}}]}`
     shape and choose the entry with the greatest `time.updated` in milliseconds. When a
     workspace is known, entries in another directory are ignored. When no workspace is
     reported, every usable entry must report the same `location.directory`; a missing or
     mixed workspace is `chat_not_configured`, never a guess.
  3. An empty list or an unexpected shape is `chat_not_configured`; 401/403 is
     `chat_auth`. No path creates a native session (`POST /session` stays setup-only) and
     no path invents a reply. Once chosen, the id is attached and its newest log page is
     read through the normal v1 attach path above.
  When VOWP **does** report a native id, the app uses it and never lists the host's
  sessions. The workspace, the native id and the credential remain core-only.
- `attach` never creates. A missing/stale mapping is `chat_not_started` or
  `session_lost`, not a silent new session.
- `send` rejects while `streaming` (`chat_busy`). There is no queue. The composer is
  disabled accordingly; the check is the race guard.
- `stop` aborts the current turn if supported and returns the legacy `stopped` view; it
  never stops the engine, the VOWP orchestrator host or the server. It clears
  `active_turn`, `retry` and `pending_permissions`, keeps the native session attached and
  invalidates the background turn. The client then `attach`es the same session, which
  returns `ready`, so the composer stays usable and the next `send` is accepted without a
  Start step. An unconfirmed abort adds the `stop_unconfirmed` notice rather than locking
  the chat.
- `log` reads the active OpenCode log; it is not the VOWP session log. The VOWP log stays
  VOWP's. If the native session is lost, `session_lost`; the existing Sessions drawer can
  still show/export the VOWP session.
- `subscribe` is a single in-process subscription; it does not open a second SSE
  connection per caller.

Tauri mapping:

| Bridge method | Tauri command | Event |
|---|---|---|
| `chat.start` | `chat_start(input)` | `spirit://chat` |
| `chat.attach` | `chat_attach(session)` | `spirit://chat` |
| `chat.send` | `chat_send(input)` | `spirit://chat` |
| `chat.stop` | `chat_stop(session)` | `spirit://chat` |
| `chat.log` | `chat_log(session, query)` | no event (read) |

`TauriBridge.chat.subscribe` listens to `spirit://chat` with `listen<ChatEvent>` and
returns the unlisten function. The channel payload is the `ChatEvent` above. The existing
`spirit://state` channel is unchanged; chat deltas do not go through the fleet event
channel. The event forwarder must not log payloads (they contain model text and possibly
user data).

### 4.4 Core types and ownership

```rust
pub enum ChatSessionId {
    Setup,
    Global,
    Project(String),
}

pub enum ChatSessionLocation {
    Local,
    Server,
}

pub enum ChatSessionState {
    Idle,
    Connecting,
    Streaming,
    Ready,
    Stopped,
    Error,
}

pub enum ChatMessageOrigin {
    User,
    Model,
    App,
    Preview,
}

pub enum ChatRole {
    User,
    Assistant,
}

pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    pub text: Vec<String>,
    pub created_at: Timestamp,
    pub origin: ChatMessageOrigin,
    pub streaming: bool,
    /// Compact, redacted tool rows. None when the event cannot know them.
    pub tools: Option<Vec<ChatToolActivity>>,
}

pub enum ChatToolStatus {
    Pending,
    Running,
    Completed,
    Error,
}

pub struct ChatToolActivity {
    pub id: String,
    pub name: String,
    pub status: ChatToolStatus,
    /// A short redacted hint such as the command or path.
    pub summary: Option<String>,
}

pub struct ChatPermissionPrompt {
    pub id: String,
    pub permission: String,
    /// Requested patterns, already redacted for display.
    pub patterns: Vec<String>,
    pub created_at: Timestamp,
}

pub struct ChatRetryAction {
    pub reason: String,
    pub title: String,
    pub message: String,
    pub provider: String,
}

pub struct ChatRetryStatus {
    pub attempt: u32,
    pub message: String,
    /// Epoch milliseconds of the next attempt, when reported.
    pub next: Option<u64>,
    pub action: Option<ChatRetryAction>,
}

pub enum ChatPermissionDecision {
    Once,
    Always,
    Reject,
}
```

`ChatSessionView` carries `pending_permissions: Option<Vec<ChatPermissionPrompt>>`
alongside `error`; it is absent when no request is waiting. It also carries
`retry: Option<ChatRetryStatus>`, absent unless OpenCode is in a `session.status` retry
wait. `next` is epoch milliseconds; `action` mirrors the provider reason, title, message
and provider. The strings are redacted and capped exactly like tool summaries, so raw
provider error JSON never crosses the bridge.

The transport-side endpoint is core-only and has no `Debug`/`Serialize` that could leak
it:

```rust
pub struct ChatEndpoint {
    pub base_url: String,          // loopback only, never serialized
    pub auth: Option<ChatAuth>,    // zeroizing, never serialized
    pub dialect: ChatDialect,      // V1; V2 is gated
    pub location: ChatSessionLocation,
    pub native_session_id: Option<String>, // server attach; core-only
    pub vowp_session_id: Option<String>,   // server mapping; core-only
}

pub enum ChatAuth {
    Basic { username: String, password: SecretString },
    Bearer { token: SecretString },
}
```

The service boundary:

```rust
#[async_trait]
pub trait ChatTransport: Send + Sync {
    async fn start(&self, input: ChatStartInput) -> Result<ChatSessionView, SpiritError>;
    async fn attach(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError>;
    async fn send(&self, session: &ChatSessionId, text: &str) -> Result<ChatSendReceipt, SpiritError>;
    async fn stop(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError>;
    async fn log(&self, session: &ChatSessionId, query: &ChatLogQuery) -> Result<ChatLogPage, SpiritError>;
}

#[async_trait]
pub trait ChatEndpointProvider: Send + Sync {
    async fn endpoint(&self, request: ChatEndpointRequest) -> Result<ChatEndpoint, SpiritError>;
}

#[async_trait]
pub trait ChatEngine: Send + Sync {
    /// Start or reuse the bundled engine; return its loopback port.
    async fn ensure_ready(&self) -> Result<u16, SpiritError>;
    /// Current engine status for the state machine; never serialized to a chat DTO.
    fn status(&self) -> EngineStatus;
}

pub struct ChatEndpointRequest {
    pub session: ChatSessionId,
    pub project: Option<String>,
    pub location: ChatSessionLocation,
    pub host_id: Option<String>,
}
```

- The **local** provider returns the bundled engine's `http://127.0.0.1:<PORT>` with no
  auth and `ChatDialect::V1`. It calls `ensure_engine_setup` and rejects `External`.
- The **server** provider returns the VOWP endpoint plus the forward's local URL and auth.
  It is the follow-up server job.
- `ChatService` owns the session map, the event broadcast sender and a map of
  `EngineEventPump` by endpoint URL. One pump per endpoint; one SSE connection; fan-out to
  all sessions on that endpoint.
- The `spirit://chat` forwarder subscribes to the `ChatEvent` broadcast and emits events.
  It never calls `serde_json::to_string` on a `ChatEndpoint` or auth value.

`ChatService` is separate from the engine supervisor and the fleet store. It may use the
existing `SpiritCore` event bus to observe `EngineStatus` changes, but it must not hold the
core mutex while doing HTTP. A restart event triggers a pump reattach, not a new engine
spawn.

### 4.5 State machine

```text
idle
  start() / attach() -> connecting
    engine ready + native session found -> ready
      send() -> streaming
        delta*/message* -> streaming
        session.status retry -> streaming + retry notice
          session.status idle/busy or turn finishes -> ready, retry cleared
        session.idle / POST response -> ready
        stop() while streaming or retrying -> stopped + attach -> ready (composer usable)
          abort ok -> stopped, then the client attaches ready
          abort unsupported -> stopped + non-fatal stop_unconfirmed, then attaches ready
        SSE lost -> connecting
          log backfill + resubscribe -> streaming/ready
          retries exhausted -> error (stream_lost)
        engine restart -> connecting
          new port + session found -> ready
          session 404 -> error (session_lost)
        host disconnect -> error (host_disconnected)
    engine unavailable / gate / no model -> error
  error --retry--> connecting
  stopped --start/attach--> connecting
```

Additional rules:

- `idle` means "no native session has been opened in this app run". It is not a fake
  ready state.
- `connecting` covers the engine health wait, endpoint read, forward start, session
  attach and log backfill. It has no spinner message with invented progress; the UI says
  "Connecting to <label>…".
- `streaming` is set by a successful `send`; the composer rejects another send. The Stop
  action is available. A `session.status` retry keeps `streaming` but adds `retry`; the
  header says "Retrying…" and the inline notice explains the wait.
- `ready` means attached and idle. The input is enabled.
- `stopped` means the owner stopped the turn (or the engine was stopped cleanly). Stop
  keeps the native session attached and the client re-attaches it to return to `ready`, so
  the composer is usable without a Start step; the view is not a session teardown.
- `error` carries a safe `SpiritErrorDto`. It is never a canned assistant message. The
  partial assistant text, if any, stays visible with an error note.
- On app restart the state is `idle`; `attach` succeeds only if the native session still
  exists. A missing native session is `session_lost`. For `setup`, the UI offers "Start a
  new conversation" (`resume: false`). For `global`/`project`, it offers a gate to
  create/recreate the orchestrator in VOWP and keeps the old log in Sessions; it never
  creates an unmirrored session.

### 4.6 `conversations.ts` and `ChatView.tsx`

`conversations.ts` stops being a canned-reply store. It becomes the renderer-side chat
cache and router:

```ts
interface ConversationState {
  messages: Partial<Record<SessionId, ChatMessageView[]>>;
  views: Partial<Record<SessionId, ChatSessionView>>;
  /** Per-session operation guard; prevents StrictMode double opens. */
  opening: Partial<Record<SessionId, Promise<ChatSessionView> | undefined>>;
  /** The last applied event revision, for gap detection. */
  lastRevision: number;

  open: (session: SessionId, project: string | null, location?: SessionLocation) => Promise<void>;
  send: (session: SessionId, text: string) => Promise<void>;
  stop: (session: SessionId) => Promise<void>;
  loadEarlier: (session: SessionId) => Promise<void>;
  /** Records the verified onboarding card; unchanged behavior. */
  recordSecureServer: (session: SessionId, messageId: string, receipt: SecureServerReceipt) => void;
  noteSecureServerScan: (session: SessionId, messageId: string, host: string) => void;
}

export function initializeConversations(bridge: SpiritBridge): () => void;
```

Behaviour:

- `NextShell` calls `initializeConversations(bridge)` once in the same effect that calls
  `initialize`; it subscribes to `bridge.chat` and returns the cleanup. `ChatView` does
  not subscribe per view.
- `ChatView` on mount calls `open(session, project)`. If `bridge.chat` is absent, it sets
  a `chat_unavailable` view; it never shows the preview replies.
- `open` calls `start` for `setup` if no view exists; for `global`/`project` it calls
  `start`, which resolves the VOWP endpoint and attaches (it does not create). It then
  calls `log` and merges the history. It is guarded by `opening[session]`.
- `send` trims, checks `views[session]?.state === "ready"`, calls
  `bridge.chat.send`, appends the returned user message if it is not already present, and
  lets the `session`/`message` events drive the assistant. It does not append a
  placeholder assistant bubble.
- On a `message` event: upsert by `message.id`. If `origin === "model"` and the message
  already exists, replace its `text` (a rewrite). If `message.show` is present, call
  `useShellUi.getState().openPanel(message.show)`; only app/preview messages set it.
- Reconcile an optimistic user message: the `send` receipt uses a `msg_<ULID>` id. When
  the log or a `message` event supplies the native user message with the same role, text
  and a `created_at` within 5 s, replace the `msg_` message with the native one
  (native id, native timestamp); never show both. The core sends the `msg_` id as
  `messageID` on every local turn, so this fallback normally does nothing; keep it for
  logs that omit the user message.
- On a `delta`: find the assistant message by `message_id`. If missing, create a shell
  with `origin: "model"`, `role: "assistant"`, `text: [""]`, `created_at: now`. Append the
  delta to the last paragraph, splitting on blank lines. If the delta would exceed the
  per-message cap (32,000 characters), keep the first 32,000 and mark a `truncated` note;
  the core also caps the stream.
- On `done`: set `streaming: false`; if the text is empty and there is no error, remove
  the placeholder. Set `ready` from the accompanying `session` event and clear `retry`.
- On `error`: set `views[session].error`, state `error`; keep any partial text. Do not
  append an assistant message with the error text.
- On `closed`: a `stopped` reason ends the turn in place and keeps the session usable (the
  `stop` action re-attaches the same native session for the next turn); every other reason
  sets `error` and detaches. Keep the partial text either way.
- On a revision gap: call `load(session)` (the last 50 messages) and replace the list.
- `loadEarlier` uses `ChatLogPage.next` as the opaque cursor; prepend older messages and
  keep the scroll position.
- The `location` map, `setLocation`, `defaultLocation` and the `⇄` button are removed.
  `ChatView` reads `views[session]?.location`; setup is fixed local. If no view exists,
  the header says "Not connected" (global/project) or "Starting…" (setup), never a
  guessed Local.
- The secure server card stays real. The store adds the app-owned `secure-server` card
  for the setup flow when the loaded history has no card and onboarding is incomplete; it
  is `origin: "app"`, not a model reply. The model never receives the one-time password.
- The details-panel demo still works in preview: `MockBridge` sets `show` on its preview
  messages; the store opens the panel. In the real path, model messages never set `show`.

`ChatView.tsx` changes:

- Remove the `⇄` toggle and the `setLocation` call. The chip uses the actual
  `ChatSessionView.location` and `state`.
- Add a Stop button while `streaming`, a Reconnect button on a retryable `error`, and
  Start/Attach on `stopped`/`idle`. Stop only aborts the turn: the client re-attaches the
  same session and lands on `ready` with the composer enabled, so the owner can switch
  model or send again immediately.
- When `view.retry` is present, render a calm inline notice with the provider title and
  message, the attempt and a short countdown (or the retry time for a long wait), plus
  **Switch model**, **Connect a provider** and **Stop retrying**. Switch model opens and
  focuses the composer picker (enabled even while the turn is locked); Connect a provider
  opens Settings → Engine; Stop retrying aborts and returns to Ready. The header label
  reads "Retrying…" instead of "Replying…" for the whole wait.
- The `error` region carries the same recovery actions where they apply (Switch model,
  Connect a provider) in addition to Reconnect and Start a new conversation.
- Disable the composer unless `state === "ready"`; the placeholder explains why. The
  empty-state suggestion buttons are disabled in the same states; they never inject a
  preview reply in the real path.
- Show a small "Preview" tag on `origin === "preview"` messages; keep the existing
  persistent Preview badge in the sidebar.
- Show a local `role="alert"` error region under the composer for chat errors; use the
  existing `toSpiritError`/`errorTitle` helpers and the safe `hint`. Do not show a raw
  HTTP body, URL, port or path.
- Render `text` as plain paragraphs; add `white-space: pre-wrap` so single newlines are
  preserved. No markdown, HTML or `dangerouslySetInnerHTML`.
- Keep `aria-live` off the streaming text; announce "Reply complete" and state changes in
  a polite status region. The current `aria-live="polite"` wrapper must not read every
  delta.
- The model chip shows the active model, labelled honestly from `view.model.tier`:
  "OpenCode free · <label>" for an included runtime model, the provider/model label for
  BYOK, and the orchestrator's choice for `global`/`project`. A composer-level picker
  switches among `view.available_models` through `chat.setModel` and never requires a trip
  to Settings. Only when the engine genuinely reports no usable model does the chip show
  "Set up a model" and send stay disabled with `no_model`. Live `view.usage` renders the
  context ring and token pills; absent values render nothing rather than zero.

### 4.7 `MockBridge` and preview honesty

`MockBridge` implements `chat` and keeps the preview shell and details-panel demos
working:

- Add scenarios to `MOCK_SCENARIOS`:
  `chat-preview`, `chat-streaming`, `chat-slow`, `chat-stop`, `chat-error`,
  `chat-reconnect`, `chat-free-limit`, `chat-engine-stopped`, `chat-external-engine`,
  `chat-server-gated`, `chat-session-lost`. Existing scenarios keep the default preview
  chat behavior so the current demo URLs keep working. `chat-free-limit` shows the
  `free_tier_limit` retry notice; Stop re-attaches the stored preview session ready and a
  later turn completes.
- `start` returns a deterministic `ChatSessionView` with the requested location (unless
  the scenario gates it), `state: "ready"`, `attached: true`,
  `streaming_supported: true`, the first included free preview model, its
  `available_models` list and sample `usage` (clearly preview data, not a receipt). It
  throws typed errors for the gated/stopped/external scenarios. `setModel` switches among
  the preview models for `setup`; `global`/`project` reject with `chat_model_gated` just
  like the real path.
- `attach` returns the existing view or `chat_not_started`/`session_lost`.
- `send` returns the user message and schedules preview events with `origin: "preview"`.
  The default reply uses the same regex-based hints as the old `previewReply` and sets
  `show` for the overview/server/inbox panels, so the details-panel demo survives.
  `chat-streaming` emits several deltas before `done`; `chat-slow` leaves a turn open for
  the Stop test; `chat-error` emits a safe error; `chat-reconnect` drops once and
  resumes with a full message.
- `stop` clears timers, clears `retry` and returns `ready` with the session still
  attached. `log` returns the messages with `next: null`. `subscribe` returns an
  unsubscribe function. Timers are cleared on `stop` and on the bridge's disposal; no
  timer survives a test.
- Every preview assistant message has `origin: "preview"` and a first paragraph that
  starts with "Preview reply." The text says "In the real app the orchestrator answers
  here." The real path never constructs this origin.
- The preview setup card is still an app-owned card. The mock's setup `send` does not
  pretend a model produced it.
- Tests: `packages/spirit-ui/src/test/bridge.test.ts` covers the chat methods and the
  preview origin; `packages/spirit-ui/src/test/next/nextShell.test.tsx` keeps the preview
  chat + details-panel assertion; `apps/spirit-desktop/e2e/next-shell.spec.ts` keeps the
  shell flow green.

### 4.8 The Web bridge

`HttpSseBridge` and `WebMockBridge` are read-only and do not implement `chat`. The
optional `chat` field means no type break. If a later Web chat is wanted, it must go
through a Web bridge method with the same `ChatSessionView`/`ChatEvent` shapes and a
server-side endpoint; it must not fetch the engine directly from the browser. That is not
in this contract.

## 5. Security

### 5.1 What may cross to the renderer

| Allowed | Why |
|---|---|
| `SessionId`, `SessionLocation`, `ChatSessionState` | Routing and the honest chip |
| `ChatMessage` text, role, origin, timestamps, `show` hint | Render the conversation |
| `ChatModelView` (`id`, `label`, `provider`) | Show the selected model; no key |
| `ChatSessionView.error` (safe `SpiritErrorDto`) | Honest failure copy |
| `ChatEvent` deltas/messages/done/closed | Live text and state |

Never sent to the renderer: endpoint URL, port, `Authorization` header, Basic username or
password, bearer token, surface token, provider key, SSH identity path, app-private
known_hosts path, remote host path, local checkout path, engine binary path, PID, native
session id, VOWP session id, remote port, forward command line.

The existing Settings → Advanced engine diagnostic may continue to show the local
loopback port and binary path under Advanced; that is an existing engine diagnostic, not a
chat field. The chat surface adds no URL/port. No token is ever in the DOM.

### 5.2 Loopback and network

- The bundled engine binds `127.0.0.1` only. `serve_args` is the only argv builder; the
  test that rejects `0.0.0.0` stays.
- The forward binds `127.0.0.1` only; no wildcard, no `-g`, no `0.0.0.0`.
- The remote endpoint URL must be loopback (`127.0.0.1`, `localhost`, `[::1]`) with no
  userinfo, query or non-root path. A public/non-loopback URL is `vowp_incompatible`; never
  fetch it.
- No browser `fetch` to the engine. The Tauri CSP does not need a new `connect-src`.
- One SSE connection per endpoint; no unbounded connections. The reconnect backoff is
  bounded.

### 5.3 SSH, host keys and identity

- The forward uses the existing `SshOptions`: system `ssh`, `BatchMode=yes`,
  `StrictHostKeyChecking=yes`, app-private `UserKnownHostsFile`, `ConnectTimeout=8`,
  keepalives, and `IdentitiesOnly=yes` with the profile's identity file.
- Only a connected `HostProfile` with a verified pin may start a forward. A changed host
  key is `host_key_mismatch`; there is no `accept-new` or `no` path.
- No agent forwarding, X11, reverse forward, dynamic forward, `ProxyCommand` injection,
  `sshpass` or password prompt.
- The identity path is core-only. The forward command line is never logged; stderr is
  bounded and redacted. The error copy names the host label, not the user@host or key
  path.
- On disconnect, kill every forward for that host. On app exit, kill all forwards. The
  forward process is a `Drop`/`kill_now` owner, like the engine child.

### 5.4 Provider keys, surface tokens and the keychain

- Provider keys stay in the OS keychain (or the selected external harness's own login).
  The bundled engine receives them only through `OPENCODE_AUTH_CONTENT` at spawn. Chat
  never reads a provider key and never puts one in a message.
- Server provider keys stay in VOWP's `vowp secret` vault, injected by VOWP's orchestrator
  host. The app never asks for a value, never reads `vowp secret` values and never sends
  one to the model.
- A VOWP surface token or endpoint auth returned by `vowp orchestrator endpoint` is a
  secret. It lives in a zeroizing `SecretString` in core, is used only in the HTTP header,
  and is never serialized, logged, rendered or included in a parse error. The endpoint
  parser reports field names only, never values.
- The renderer never sees "stored ✓" secrets as values; only names/metadata from the
  existing secret store, which is unchanged.

### 5.5 Logs, errors and the DOM

- `EngineLogBuffer` stays bounded/redacted. Chat event bodies (model text, user text, tool
  output) are **not** written to the engine log or the app log. Debug logging, if any,
  records event type/session key only.
- The endpoint JSON, auth header, forward argv and HTTP response bodies are never logged.
- Errors are `SpiritErrorDto` with a safe message and hint. Map HTTP/provider details to
  the codes in §6. Never include a URL, port, path, key, token, raw body or command line.
- The renderer escapes text; no `dangerouslySetInnerHTML`, no markdown HTML, no raw tool
  JSON. Control characters are stripped; message/delta length is capped.
- A model could echo a secret it was given; the app never gives it one. The setup prompt
  tells the model not to ask for a secret in chat. The one-time server password is typed
  into the app-owned card, is React state only until the install call, and is cleared
  before the call. It never enters a `ChatMessage`, the engine, the model or a log.

### 5.6 Files and permissions

- The private engine tree and `chat/sessions.json` are created `0700`/`0600` under the
  app-data directory.
- The chat session mapping contains ids only, never a token or path. It is not a secret,
  but it is app-private.
- Pending permission prompts carry only the native request id, the permission name,
  redacted patterns and a timestamp. Tool rows carry only the tool name, status and a
  redacted hint; tool input, output and error text never enter a view.
- The app sends a permission reply only after an explicit owner click, and only for a
  request currently in `pending_permissions`. Nothing auto-approves.
- No chat data is written to the VOWP home from the app. VOWP writes its own log; the app
  does not open it.
- The app never deletes a native or VOWP session. `stop` aborts/detaches. A new setup
  conversation creates a new local session and leaves the old log intact; a `global`/
  `project` conversation is created in VOWP, not by the app.

### 5.7 Threat notes

- **Renderer compromise/XSS:** the renderer cannot obtain the endpoint URL, auth or key
  path because they never cross the bridge. It can send messages as the owner; that is the
  existing trust boundary of the desktop UI.
- **Local malware:** it can reach the loopback engine as the same OS user. The engine has
  no server auth in the current launch; this is the existing local trust boundary, not a
  new public surface. The setup engine's private tree limits accidental cross-session
  exposure but is not a sandbox.
- **Model output:** it is untrusted text. It is escaped and capped; it cannot open the
  details panel, execute a command or approve a tool.
- **Server compromise:** the forward only exposes the server's loopback port on the
  Mac's loopback; the SSH host key is pinned. A compromised server can serve malicious
  model text; it cannot obtain the Mac's provider keys through chat.
- **Prompt injection:** a project's `AGENTS.md` or a model can ask the app or the
  orchestrator to reveal a secret or a path. The chat itself gives the model no
  StackSpirit tool and no secret, and the card never sends a password to the model. The
  orchestrator's OpenCode tools may read files or run commands as the Mac user; that is
  the existing OpenCode/orchestrator permission boundary, not something this chat can
  sandbox. The app never auto-approves a permission request; only an explicit owner click
  sends a reply. The UI does not claim the model is sandboxed.

## 6. Error states

All errors are safe `SpiritErrorDto` values. `code` is the stable outer code (the first
column); the `chat_*` label after the dash is the specific reason carried in the safe
`message`/`hint` and used by the UI copy and tests. There is no second wire error field.
The UI shows `errorTitle(error)` plus the message and hint; it never shows a raw body.

| State / code | Trigger | UI | Retry |
|---|---|---|---|
| `gate` — `chat_gated` | Server path has no VOWP 0.5 endpoint (the app-owned forward is not approved); a Local endpoint reports a null `url`, a non-`running`/`degraded` `state` or a non-`opencode` `app` | Gate banner naming the exact missing piece; composer disabled | After the gate clears |
| `gate` — `chat_unavailable` | `bridge.chat` is absent (Web) or the bridge cannot run chat | "Chat is not available in this client." | No |
| `gate` — `chat_gated` (unsupported app) | The orchestrator's `app` is `claude-code` or `chat`, not `opencode` | "This orchestrator uses <app>; the OpenCode chat contract does not apply." | No (different app) |
| `gate` — `chat_not_configured` | No local endpoint: the installed VOWP has no `agent orchestrator endpoint`, the local agent is not enrolled/paired, or no orchestrator card exists | "Create the orchestrator in VOWP first." | After enrolling with VOWP or creating the orchestrator |
| `gate` — `chat_not_configured` (discovery) | A null `native_session_id` and the host lists no session, an unreadable/malformed list, a missing or mixed workspace, or an unreachable local app | "This orchestrator has no OpenCode conversation to continue." | After opening the conversation in the orchestrator, or after the app recovers |
| `auth` — `chat_auth` (discovery) | The reported endpoint credential is rejected (401/403) while listing the host's sessions | "The orchestrator rejected its local credential." | After VOWP restarts/refreshes the orchestrator endpoint |
| `gate` — `chat_not_mirrored` | The endpoint reports an unusable `native_session_id`, or reports a different `session_id` than the attached home log | "This orchestrator has no usable mirrored OpenCode session." | After VOWP repairs the mirror |
| `gate` — `path_unmapped` | The project has no local path map for a local session | "Map the project path in Engine → Local orchestrator paths." | After adding the path |
| `gate` — `local_agent_not_connected` | The Mac agent is not enrolled/connected for a local endpoint | "Connect the local agent in Engine → Local orchestrator." | After the agent connects |
| `engine` — `engine_not_ready` | The engine is starting/stopped/degraded after the wait | "StackSpirit Engine is not ready." with the redacted reason | Retry after Restart |
| `engine` — `engine_external` | The selected engine is an external harness, not the bundled engine | "The setup assistant needs StackSpirit Engine; switch to it in Settings → Advanced." | After switching |
| `engine` — `engine_unavailable` | The bundled binary is missing/unreadable | "StackSpirit Engine is unavailable — repair the installation." | Repair |
| `engine` — `no_model` | The runtime reports zero free models and no BYOK provider key is configured for a setup turn | "The bundled engine reports no free model and no provider key is configured." | After adding a key or selecting a verified model |
| `engine` — `protocol_unsupported` | The server is a v2 `/api/...` OpenCode or an unknown dialect | "This orchestrator runs an unsupported OpenCode API; use the pinned engine or wait for the v2 adapter." | No |
| `conflict` — `chat_model_gated` | `setModel` on a `global`/`project` session | "An orchestrator owns its own model choice." | Change it in the orchestrator's OpenCode config |
| `conflict` — `chat_busy` | A second send while streaming | Composer disabled; the race guard shows "Wait for the current reply or stop it." | After done/stop |
| `conflict` — `chat_not_started` | `attach`/`log` before a native session | "Start the conversation first." | Start |
| `not_found` — `session_lost` | The native session was deleted/recreated; the mapping is stale | "The orchestrator's app session was recreated. Its server log is still in Sessions." | Setup: `resume: false`; global/project: create/recreate in VOWP |
| `remote` — `host_disconnected` | The host is not connected or the forward died | "The server is not connected; server chat needs the verified SSH connection." | Reconnect the host |
| `ssh_auth` / `ssh_unavailable` / `host_key_mismatch` — `ssh_forward_failed` | The forward could not start | "The SSH forward to the server failed." (host label only) | After fixing the host/key |
| `remote_timeout` — `stream_lost` | SSE retries exhausted | "Live updates stopped; reconnect to continue." | Reattach/backfill |
| `remote_timeout` — `turn_timeout` | The 10-minute turn cap | "The turn did not finish in time." | Retry after stop |
| `conflict` — `stop_unconfirmed` | Abort is unsupported/failed | "Stopped here; the turn may still be running on the orchestrator." | Check OpenCode; not an error state |
| `engine` — `stream_unsupported` | SSE is unavailable; final response mode is used | "Live text is unavailable; the reply appears when the turn completes." | No; informational |
| `invalid_input` — `invalid_text` | Empty or > 32,000-character message | "Enter a shorter message." | Yes |
| `auth` — `engine_auth` / `chat_auth` | The server endpoint rejects the auth | "The orchestrator rejected the connection credential." | Re-enter/re-pair |

HTTP mapping for the server/client:

| HTTP | Map to |
|---|---|
| 401/403 | `auth` (`chat_auth` / `engine_auth`); never show the challenge |
| 404 on a session/message | `not_found` (`session_lost`) |
| 404 on `/event` | `stream_unsupported`; use final response |
| 409 | `conflict` (`chat_busy`) |
| 429 | `limit` ("The provider rate limit is active.") |
| 5xx | `engine` ("The orchestrator failed the request.") |
| Connection refused | `engine` (`engine_unavailable`) or `remote` (`ssh_forward_failed`) |
| Timeout | `remote_timeout` |

A failed permission reply is not a session error. If the engine rejects the reply or
exposes no permission reply route, the core returns a typed `engine` error and the card
stays visible with the turn still paused; it never pretends the answer was sent.

An HTTP 200 `info.error` in the assistant message uses the same mapping: 401/403 →
`auth`, 429 → `limit`, everything else → `engine` (`provider_error`). Never render the raw
error object.

## 7. What can land now, and what is blocked

### 7.1 Can land locally now

- The wire types in §4.1–4.2 and the `chat` bridge interface (§4.3).
- `MockBridge` chat scenarios and the preview labels; the details-panel demo.
- The core local client for the bundled engine, including the private XDG tree, the
  session/log/message/abort/SSE adapter, the event pump, reconnect/backfill, and the Tauri
  commands. It is tested against a fake HTTP/SSE engine (no real model). The live probe
  pins the v1 paths, session/log shapes, abort, directory selection, restart persistence
  and `server.heartbeat`, so the adapter no longer depends on guesses for those. Delta
  cadence, the real `message.part.updated` payload and `messageID` preservation remain
  owner-turn checks; the real setup turn is owner-verified, not CI.
- `ChatView`/`conversations.ts` wiring for the **setup assistant** and the honest
  `chat_unavailable`/`engine_*` states. The global/project surfaces show the gate until
  the server path opens; they do not show preview replies when `bridge.preview` is false.
- Live tool-activity rows and the inline permission card. Allow once / Always allow /
  Reject call `replyPermission` (`chat_reply_permission`); the pinned 1.19.5 `/doc` proves
  the reply route, and the fake-engine tests cover the round trip. A live tool/permission
  turn is still owner verification, so do not claim it has been observed end to end.
- The removal of the label-only Local/Server toggle.
- The `ChatMessageView` card flow, the secure server card and the onboarding actions are
  unchanged and real.

### 7.2 Blocked on VOWP 0.5

The Server transport code can be built and fake-tested behind the `chat_gated` switch;
what is blocked is enabling the live path and claiming it works.

- Server `global`/`project` chat through the VOWP endpoint and the owner's transport
  decision (A).
- Live verification of Local `global`/`project` chat. The attach code landed (ss-61) and
  is fake-tested against `vowp agent orchestrator endpoint`, but the installed
  `vowp 0.4.0` on this host has no such subcommand. A missing command is
  `chat_not_configured`; an owner Mac with VOWP `0.5` must run §9 before anyone claims
  the live path works.
- The bundled StackSpirit Engine is **not** the VOWP orchestrator app. VOWP runs its own
  `opencode serve` and reports that endpoint; the setup assistant keeps the bundled
  engine.
- The D6 live flip: the owner's agent must report VOWP `0.4.0+` before the D6 gate leaves
  `Waiting`. The move (`ss-97`, `ss-98`) and Add project (`ss-99`, `ss-100`) surfaces
  have landed; see §3.5 and §3.6.

### 7.3 Rules this contract enforces

- The Server path is not silently enabled. It waits for VOWP 0.5's endpoint; the
  app-owned forward in §3.3 is not approved (decision A). The app shows the exact gate.
- The Local chip for `global`/`project` appears only when VOWP reports a local orchestrator
  and the turn is mirrored to the home. A UI-only Local label is forbidden.
- The setup assistant remains local-only. It is not a VOWP session and does not claim a
  server log.
- The app never creates a native session under a `global`/`project` key. It attaches to a
  VOWP-reported one, or shows a gate.

## 8. Split follow-up jobs

IDs are illustrative; the supervisor assigns the real ones. Every job must read this
document first, keep its `owns` list exact, and never edit `docs/ORCHESTRATOR-CHAT.md`
except the doc job that owns it. Code jobs also run the repository's standard gates
(`cargo fmt`, `cargo test`, `cargo clippy`, `npm run typecheck`, `npm run lint`,
`npm test`, the relevant e2e, and `cargo check -p spirit-desktop`).

### 8.1 `opencode-chat-probe`

- **owns:** `scripts/probe-opencode-chat.mjs` (new only)
- **needs:** the pinned sidecar present (run `node scripts/fetch-engine.mjs` on the owner's
  machine; the probe job must not download it). No VOWP dependency.
- **work:** implement §2.4 as a script that fails closed if the binary is not the pinned
  one; write a redacted report with the §2.5 answers; never start a model turn, never use
  the `opencode` on `PATH`, never print a path/id/token. If the sidecar is absent, record
  that and stop; do not guess.
- **done:** the report pins the exact session/log/SSE/abort/directory/persistence facts,
  or explicitly marks each unknown and blocks the core streaming adapter.

### 8.2 `chat-wire-mock`

- **owns:** `packages/spirit-ui/src/bridge/wire.ts`,
  `packages/spirit-ui/src/bridge/SpiritBridge.ts`,
  `packages/spirit-ui/src/bridge/MockBridge.ts`,
  `packages/spirit-ui/src/bridge/index.ts`,
  `packages/spirit-ui/src/test/bridge.test.ts`
- **needs:** this document. No core/VOWP dependency.
- **work:** move `SessionId`, `SessionLocation`, `ChatMessage`, `ProjectRequest` and
  `ChatPanelTarget` into `wire.ts`; add the §4.2 types and the `chat` surface; implement
  the MockBridge scenarios and keep the preview/detail demos; update the bridge tests.
  Do **not** edit `next/conversations.ts` or `ChatView.tsx`; re-exports will follow in the
  view job.
- **done:** typecheck/lint/vitest green; `MockBridge.chat` returns `origin: "preview"`;
  no endpoint URL, credential or machine-path field is added to a wire type
  (`ProjectRequest.source.path` remains user input); the preview shell tests still pass
  (the view job may own the final shell assertion).

### 8.3 `chat-core`

- **owns:** `crates/spirit-core/src/engine/chat/mod.rs`,
  `crates/spirit-core/src/engine/chat/local.rs`,
  `crates/spirit-core/src/engine/chat/pump.rs`,
  `crates/spirit-core/src/engine/chat/sse.rs`,
  `crates/spirit-core/src/engine/chat/model.rs`,
  `crates/spirit-core/src/engine/mod.rs`,
  `crates/spirit-core/src/engine/process.rs` (private XDG env and setup
  `current_dir` only),
  `crates/spirit-core/tests/chat.rs` (new),
  `apps/spirit-desktop/src-tauri/src/commands/chat.rs` (new),
  `apps/spirit-desktop/src-tauri/src/commands/mod.rs`,
  `apps/spirit-desktop/src-tauri/src/events.rs`,
  `apps/spirit-desktop/src-tauri/src/lib.rs`,
  `apps/spirit-desktop/src-tauri/src/state.rs`,
  `apps/spirit-desktop/src/bridge/TauriBridge.ts`
- **needs:** `chat-wire-mock`; the probe for the SSE/abort/directory facts. It may start
  with the proven session/message/health path and add the SSE adapter behind the
  `ChatTransport` trait while the probe runs.
- **work:** implement the local endpoint provider, HTTP client, SSE pump, delta derivation,
  log/abort, session mapping, state machine, error mapping and Tauri commands/events;
  set the private XDG tree and the setup `current_dir`; ensure `ensure_engine_setup`
  reuse; never spawn a second engine; add the fake HTTP/SSE engine tests. Do not edit `next/` or Settings.
- **done:** `cargo fmt/test/clippy` green; a fake SSE test proves deltas, done, reconnect,
  backfill, abort, error-in-200, engine reuse and no duplicate messages; no serialized
  `ChatSessionView`/event contains a URL/port/token/path; the Tauri bridge maps the five
  methods and `spirit://chat`.

### 8.4 `chat-view`

- **owns:** `packages/spirit-ui/src/next/conversations.ts`,
  `packages/spirit-ui/src/next/ChatView.tsx`,
  `packages/spirit-ui/src/next/NextShell.tsx`,
  `packages/spirit-ui/src/next/shellUi.ts` (the `PanelContent` alias only),
  `packages/spirit-ui/src/next/theme.css` (chat state styles),
  `packages/spirit-ui/src/test/next/nextShell.test.tsx`,
  `apps/spirit-desktop/e2e/next-shell.spec.ts`
- **needs:** `chat-wire-mock`, `chat-core`.
- **work:** replace the canned store with `initializeConversations`/`open`/`send`/`stop`/
  `loadEarlier`; implement the state machine, preview tags, honest chip, gate/error
  regions, composer disabling and a11y; keep the secure card and details-panel preview.
  Do not edit `wire.ts`, core or Settings.
- **done:** the setup path is wired to `bridge.chat`; the preview path still works and is
  labelled; `bridge.preview === false` never shows a preview reply; the label-only switch
  is gone; unit/axe/e2e green; the chip shows the view's actual location.

### 8.5 `chat-server`

- **owns:** `crates/spirit-core/src/engine/chat/server.rs` (new),
  `crates/spirit-core/src/vowp/orchestrator_endpoint.rs` (new),
  `crates/spirit-core/src/vowp/mod.rs`, `crates/spirit-core/src/vowp/client.rs`,
  `crates/spirit-core/src/ssh/forward.rs` (new), `crates/spirit-core/src/ssh/mod.rs`,
  `crates/spirit-core/tests/chat_server.rs` (new),
  `apps/spirit-desktop/src-tauri/src/commands/chat_server.rs` (new), and the serial
  integration edits to `lib.rs`/`state.rs`/`commands/chat.rs` after `chat-core` merges.
- **needs:** `chat-core`. The endpoint read and forward can land behind the `chat_gated`
  switch with fake tests, but the **live** server path stays disabled until VOWP `0.5`
  ships `vowp orchestrator endpoint`; the owner did not approve the forward (decision A).
  The job must not implement the consent UI.
- **work:** add the endpoint read to the VOWP client (tolerant JSON, redacted parse
  errors, no values), the local agent endpoint read, the loopback URL validation, the
  version gate, the app-owned `ssh -N -L` forward with `SshOptions`, the ref-counted
  process lifecycle, and the server endpoint provider. Add the local non-secret consent
  read/write commands that `chat-settings` uses. No Settings UI in this job.
- **done:** core tests prove the forward argv is loopback/strict/no-token, the endpoint
  parse never logs a value, a v2 server is refused, the forward is killed on drop, and a
  fake server endpoint works end to end. If the live path is still gated, the report names
  the exact missing gate; it does not block the fake-testable code.

### 8.6 `chat-settings` (not scheduled — the owner declined the fallback)

**Not scheduled:** decision **A** (2026-11-19) did not approve the app-owned forward, so
this consent surface is not built. The design is kept for reference.

- **owns:** `packages/spirit-ui/src/next/SettingsView.tsx` and the one settings test file.
- **needs:** `chat-server`.
- **work:** add the non-secret "Allow SSH forward for server chat" consent toggle and the
  endpoint status/diagnostic; no token field that persists, no URL/port in the DOM, no
  Settings edit in any other job.
- **done:** the consent is explicit, reversible and stored as a local boolean; the card
  says exactly what is gated.

### 8.7 `chat-owner-verification`

- **owns:** `scripts/verify-orchestrator-chat.mjs` (new only); its report under
  `.vowp/jobs/<id>/report.md`.
- **needs:** `chat-view` for the local setup half; `chat-server` for the server half.
  Blocked on the owner running the app with a real free included model and, for the server
  half, VOWP `0.5`'s `vowp orchestrator endpoint`.
- **work:** run §9; capture redacted evidence; verify one free model turn, streaming, stop,
  reload/reattach, engine restart, server log mirroring, no token/path in DOM/log/errors,
  and no local fallback on disconnect. Do not run a paid model; do not create a surface or
  pair a host unless the owner explicitly asks for that verification.
- **done:** the report has the checklist results, redacted evidence, and any remaining
  blocker; no production file is changed except the new script.

### 8.8 `chat-v2-adapter` (conditional, future)

- **owns:** `crates/spirit-core/src/engine/chat/v2/` and its tests only.
- **needs:** `chat-core`; a decision that a v2 server is in scope; the VOWP spike's v2
  evidence re-validated against the actual server version.
- **work:** map `/api/session`, `/api/session/{id}/message`, `/api/event`, Basic auth and
  the v2 event names behind the same `ChatTransport`. Do not mix v1/v2 paths at runtime.
- **done:** a version-detected adapter; v1 stays the pinned default.

### 8.9 Ownership and ordering rules

- `next/conversations.ts` and `ChatView.tsx` are one job (`chat-view`); `bridge/wire.ts`
  and `MockBridge.ts` are another (`chat-wire-mock`). They are serial: the wire job lands
  first, the view job re-exports/imports the moved types and changes the components.
- Core engine files (`engine/process.rs` and the local chat module files in
  `engine/chat/`, excluding `server.rs`) are one job (`chat-core`). The server SSH/VOWP
  files (`engine/chat/server.rs`, `ssh/forward.rs`, the VOWP endpoint read) are a different
  job (`chat-server`). They are serial; the server job owns the later `lib.rs`/`state.rs`
  integration edits.
- Settings is never folded into `chat-view` or `chat-core`; it is `chat-settings`.
- No implementation job edits `docs/HANDOFF-DESKTOP-UI.md`, `README.md`, `docs/PLAN.md` or
  any VOWP file for this work. A later status/docs job may update HANDOFF §5 after the
  features land; it is not part of these jobs.
- If a job discovers that the probe contradicts §2.2, it must stop and file a doc change
  against `docs/ORCHESTRATOR-CHAT.md` before writing code against the new shape.

## 9. Owner live verification

This is the `chat-owner-verification` job's checklist. Use the owner's own machine and
server; redact every real host, path, id and token from the report. Do not run a paid
model. Use one free included model turn and the setup assistant.

### 9.1 Local setup assistant

1. Build/run the desktop app with the bundled engine and the pinned sidecar present. Open
   the setup assistant from the welcome page.
2. Confirm the engine starts/reuses the existing process (no second `opencode` child;
   `ps` shows one bundled child), health is green and the chip is `Local · this Mac`.
3. Send one short message using a free included model chosen from the picker (it lists
   connected providers only; see §2.1.2). Confirm text appears progressively
   (or the honest "reply appears when the turn completes" note if SSE is unsupported), and
   the reply is not labelled Preview. Record whether `message.part.delta` is emitted, the
   real `message.part.updated` payload, and the `session.idle` ordering; do not describe
   the result as token-by-token unless it was observed. The `msg` `messageID` prefix and
   the live `tokens.total` shape are already proven (2026-11-20, §2.2.5); still confirm
   that the user message with that `msg_` id appears in `GET /session/{id}/message`.
4. Press Stop during a long turn. Confirm `ready` with the composer enabled immediately
   after the automatic re-attach, no duplicate assistant message, and either a clean abort
   or the honest `stop_unconfirmed` note. Then send another turn without clicking Start.
5. Reload/restart the app. Confirm the setup session reattaches and the log is intact; no
   duplicate user/assistant messages.
6. In DevTools, search the DOM, console, network and error text for: the endpoint URL, a
   port used by chat, `Authorization`, `Basic`, a token-looking string, the engine binary
   path, the app-data path and any `/Users`, `/home` or `C:\` path. Only the existing
   Advanced engine diagnostic may show the local loopback port/binary path; the chat
   panel must show neither.
7. Restart the engine from Settings while the chat is open. Confirm the chat shows a brief
   connecting state, reattaches and does not duplicate the conversation. Stop the engine
   and confirm the chat shows `engine_not_ready`, not a fake reply.
8. Select an external harness in Settings and reopen setup chat. Confirm
   `engine_external`, not a silent bundled start.

### 9.2 Server chat

1. With VOWP `0.5`'s `vowp orchestrator endpoint` (the app-owned forward is not
   approved), open the global orchestrator. Confirm the
   chip is `Server · <label>` and the composer is enabled only after `ready`.
2. Send one short message. Confirm streaming/done, then run the server-side read
   (redacted):
   `vowp --json session show <VOWP_SESSION_ID> --events 20` (or the equivalent 0.5 read).
   Confirm the same user and assistant turn is in the VOWP log. Record whether the turn
   omitted `model` and whether the server accepted it; that behavior remains an
   owner/server unknown.
3. Disconnect the host. Confirm the chat goes to `host_disconnected`, the composer is
   disabled, and there is no local fallback or preview reply. Reconnect and confirm
   reattach/backfill without duplicates.
4. Confirm the VOWP endpoint/forward is not shown in the DOM, console or a Tauri error;
   confirm the server's remote port, key path and token are absent.
5. If the server runs a v2 OpenCode, confirm `protocol_unsupported` and no partial chat.

### 9.3 Local global/project (after VOWP 0.5)

1. Start a VOWP local orchestrator for the account, with the local agent connected.
2. Confirm the chip is `Local · this Mac` only when VOWP reports that endpoint and the
   turn is mirrored to the home.
3. Send one turn; confirm it is in the home log and that the app did not create a second
   local-only log.
4. Use `vowp orchestrator move` (0.5) and confirm the session continues under the same
   VOWP session and the chip flips to the actual new location. Confirm no duplicate
   messages. This is the step-6 switch, not a chat toggle.

### 9.4 Fail criteria

- A preview/canned reply appears with `bridge.preview === false`.
- A token, endpoint URL, SSH key path or remote path appears in the DOM, console, a log or
  an error.
- A second bundled `opencode` process starts for chat.
- A local-only session is created under `global`/`project`.
- A disconnect silently falls back to local.
- A server turn is not visible in the VOWP log when the endpoint claims a mirrored
  session.
- The chip says Local when the transport is Server, or vice versa.

## 10. Open questions for VOWP and the owner

The §2.5 live-probe questions are answered in §2.5 and are no longer open here. The
remaining §2.5 owner-turn items (delta cadence, `session.idle` ordering, the real
`message.part.updated` payload, `messageID` preservation, a real provider error and an
omitted `model` on a server) are listed there. The questions below are still open for
VOWP/the owner.

These are not blockers for this document; they are blockers for specific follow-up jobs.
Record the answers in the VOWP contract or the job report, not by guessing in code.

1. **Endpoint auth delivery:** does `vowp orchestrator endpoint` return a Basic
   username/password, a ready header value, or only a username plus a private file? The
   app accepts only the first two until a safe file-read design exists.
2. **Local endpoint:** does `vowp agent orchestrator endpoint` exist with the same fields,
   and does it report a local Mac orchestrator started by VOWP?
3. **Bundled engine as the local orchestrator app:** what is the supported way for VOWP
   `orchestrator new --node <mac> --app opencode` to run the StackSpirit sidecar rather
   than the `opencode` on the agent's `PATH`? There is no `--app-path` in 0.4.0, and the
   `vowp agent --help` has no `surface` command. Without this, the local orchestrator
   server is a VOWP-managed OpenCode, not the StackSpirit Engine.
4. **Server version:** does the home orchestrator run the pinned `1.19.5` sidecar, or a
   different `opencode` (the spike used `2.0.28`)? The client needs the v1 dialect or the
   v2 adapter.
5. **Mirroring proof:** does a VOWP-attached surface mirror turns from an app-started
   `opencode serve`, or only from a VOWP-started host? The endpoint's
   `native_session_id` is the minimum proof; the owner verification checks the log.
6. **`orchestrator move`:** does the 0.5 move preserve the exact native OpenCode session
   id, or only the VOWP session id? The chat mapping must know which one survives.
7. **Server path approval (resolved 2026-11-19):** decision **A** — wait for VOWP `0.5`'s
   `vowp orchestrator endpoint`; the app-owned SSH forward (§3.3) is not approved. Revisit
   only if the owner asks.
8. **Setup transcript retention:** should the local setup transcript survive an app
   restart, or be cleared when onboarding completes? This contract keeps it in the
   private engine tree until the owner clears it; the UI does not claim it is a server
   log.

## 11. References

- `docs/HANDOFF-DESKTOP-UI.md` §3 (sessions, chip, log-on-server decision), §4 (preview
  vs real), §5 step 4 (live chat via `opencode serve` and VOWP 0.5).
- `packages/spirit-ui/src/next/conversations.ts`, `ChatView.tsx`, `NextShell.tsx`.
- `packages/spirit-ui/src/bridge/{wire,SpiritBridge,MockBridge}.ts`.
- `crates/spirit-core/src/engine/{process,runtime,mod,supervisor}.rs`.
- `apps/spirit-desktop/src-tauri/src/{commands/engine.rs,state.rs,events.rs}`.
- `engine/manifest.json`, `scripts/fetch-engine.mjs`.
- VOWP `docs/design/v0.4/local-orchestrator.md` §4.3/§5 (surface pairing, attached turns,
  event mapping, path normalization, secret redaction).
- VOWP `docs/design/v0.5/scope.md` §3 (endpoint), §2 (move), §4 (project add).
- VOWP `crates/vowp-protocol/src/{surface,agent,session}.rs` for `OrchestratorCard`,
  `OrchestratorNodeState`, `OrchestratorApp`, `JoinInstructions` and the session/event
  shapes.
- VOWP `docs/maintainers/v0.2-spikes/app-surfaces.md` — reference for the v2 API only;
  the pinned StackSpirit engine is 1.19.5/v1.

## Setup sandbox and instructions contract (ss-80)

A 2026-11-20 live test showed the setup assistant running `find /` through bash (macOS
privacy dialogs), then hanging on an `external_directory: ask` prompt that has no UI. The
contract is now:

- `PrivateEngineTree::prepare` writes `engine/config/opencode/opencode.json` (atomic,
  mode `0600`) on every engine start. Its `permission` block is `deny` for `bash`, `edit`,
  `write`, `read`, `glob`, `grep`, `list`, `external_directory`, `webfetch`, `websearch`,
  `task`, `skill` and `lsp`. No rule is `ask`: the setup chat has no permission UI, so an
  `ask` would stall the turn forever.
- The child gets `OPENCODE_CONFIG` and `OPENCODE_CONFIG_DIR` pointing only at that file and
  directory, `OPENCODE_DISABLE_PROJECT_CONFIG=1`, the private `XDG_*` roots and the setup
  workspace as cwd. `OPENCODE_CONFIG_CONTENT` and `OPENCODE_PERMISSION` are removed from the
  inherited environment, so no external orchestrator `opencode.json` (for example one under
  a VOWP agent home) is loaded.
- `config/opencode/AGENTS.md` (atomic, `0600`; the pinned engine loads only `$OPENCODE_CONFIG_DIR/AGENTS.md` once project config is disabled, so it is not in the workspace) tells the assistant it is chat-only and
  explains the real flows: `+ Connect a new server` in the composer, `Settings -> Add
  server` for key-auth hosts, `Form a cluster` once two or more servers are saved, and
  never to paste passwords, keys or tokens into chat. It contains no host, path or secret.
- Tests: `crates/spirit-core/tests/setup_sandbox.rs`.

## Provider catalog and connect contract (ss-84)

The provider list and the model catalog are separate reads, and they answer different
questions:

- `GET /provider` returns `{ "all": [...], "connected": [...] }`. `crates/spirit-core/src/engine/process/providers.rs`
  parses it into a display-ordered catalog: connected providers first, then the OpenCode /
  OpenCode Go / popular ring, then label. Duplicate ids keep the connected row. An optional
  `connected` array is respected; when it is absent no row is invented as connected.
- `GET /config/providers` returns the configured-provider document. Its rows are all
  connected, and this is the preferred model catalog read (the precedence in §2.1.2 is
  unchanged).
- Each row crosses the bridge as `EngineProvider`: id, label, the engine's `connected` fact,
  its `source` class and `model_count` / `free_model_count`. The renderer groups Connected
  vs Available from that fact, so a provider connected through OpenCode auth or an
  environment variable shows as connected. No provider configuration, environment value or
  key is serialized. A stopped engine reports no rows and the panel says so.

Connecting and removing keys:

- `EngineSupervisor::connect_provider` stores the key through the supplied `Keychain` and
  restarts a live bundled sidecar so its spawn-time `OPENCODE_AUTH_CONTENT` document already
  contains the new provider. `disconnect_provider` deletes it and restarts the same way.
  `refresh_catalog` re-reads models and providers from a healthy live engine without a
  restart. `SpiritCore::connect_provider` / `disconnect_provider` wrap them and publish the
  engine event; the desktop `store_key` and `delete_key` commands call exactly those, then
  refresh the chat adapter's model list. `key_status` keeps the last `verify_key` result
  per provider (cleared when the key changes), so an invalid key is not reported as
  plain `saved`.
- StackSpirit still never writes an engine `auth.json`, never logs the key, and never puts
  it in a bridge DTO, notice or error.
- Removing a key drops that provider's models from the refreshed list. A key that fails
  verification stays in the keychain and is reported as invalid/unverified; it is never
  silently deleted.

UI contract:

- The composer picker groups the verified chat models into **OpenCode Free** (`tier:
  included`) and **Connected providers** (every other tier). Empty groups are omitted.
- After Save key / Remove key the panel re-attaches the setup session so the composer
  picker shows the new model list immediately.
- **Settings → Engine & Models** hosts the panel; the picker footer
  **+ Connect another provider…** opens the same panel in a dialog.
  OpenCode is featured with its free-model note; the panel then shows connected providers
  and a searchable catalog. A provider the engine reports as connected without a keychain
  key is labelled
  "Connected outside the keychain", never given a fake key state.
- Key fields are write-only, cleared before the save call returns, and error copy is
  scrubbed of the typed value.

Tests: `crates/spirit-core/src/engine/providers.rs` and
`crates/spirit-core/tests/engine_providers.rs` cover grouping, dedup, search and the
connect/disconnect restart; `packages/spirit-ui/src/test/next/composer.test.tsx` and
`providerSettings.test.tsx` cover the grouped picker, search, connect, verify, remove and
secret-free errors; `apps/spirit-desktop/e2e/meadow-provider-connect.spec.ts` covers the
light and dark flows with axe WCAG 2.2 AA.

## BYOK keychain, live reload, searchable model picker & Xvfb verification (ss-85 plan)

Live testing on 2026-11-20 uncovered two connected BYOK failures: (1) adding an OpenRouter
API key and clicking **Verify key** failed and appeared to wipe the saved key, and (2) none
of the connected provider's models appeared in the chat model selector. Inspection of
`stackspirit`, the OpenCode reference checkout, and live probes against the pinned `opencode`
`1.19.5` sidecar pinpointed four root causes:

### Root causes

1. **Silent mock keychain (`Cargo.toml` missing `keyring` platform features — primary cause of both bugs):**
   - Root `Cargo.toml` declared `keyring = "3"` with no features. In `keyring` `3.6.3`,
     there are **no default features**: without `apple-native`, `windows-native`, and
     `linux-native`, the crate compiles against `keyring::mock::MockCredential`
     (`CredentialPersistence::EntryOnly`), which keeps the password only inside the
     transient `Entry` struct and drops it immediately when `OsKeychain::set` returns.
   - `OsKeychain::set` still wrote the provider id to `keychain-index.json`, so
     `key_status()` showed OpenRouter as `saved`.
   - On sidecar restart, `keychain_auth_content` called `keychain.get("openrouter")` on a
     fresh `Entry` and got `Ok(None)`, launching `opencode` with **no**
     `OPENCODE_AUTH_CONTENT`. OpenRouter was therefore never connected in the engine, so
     `/config/providers` returned only `opencode` and none of OpenRouter's 371 models
     appeared in chat.
   - Clicking **Verify key** called `verify_provider_key` → `keychain.get("openrouter")` →
     `Ok(None)` → `state: "missing"`. `SettingsPage.tsx` (`upsertKey` + `savedStatus`)
     treats `"missing"` as absent and hid OpenRouter from **Connected providers**, making
     the key appear wiped.
   - **Fix:** enable `keyring = { version = "3", features = ["apple-native", "windows-native", "linux-native"] }`
     in root `Cargo.toml`, and add a `#[cfg(feature = "keychain")]` regression test asserting
     `keyring::default::default_credential_builder().persistence() != CredentialPersistence::EntryOnly`
     plus a cross-instance `OsKeychain` round-trip test.

2. **OpenCode `1.19.5` auth & reload semantics (verified live against pinned `1.19.5`):**
   - `Auth.all()` in `opencode` `1.19.5` checks `process.env.OPENCODE_AUTH_CONTENT` first
     (`JSON.parse`); when set, it overrides `$XDG_DATA_HOME/opencode/auth.json`.
   - `PUT /auth/{providerID}` with `{"type":"api","key":"..."}` writes plaintext
     `$XDG_DATA_HOME/opencode/auth.json` (`0600`) and does **not** reload
     `/config/providers` until `POST /instance/dispose` is called (live probe on `1.19.5`:
     before `PUT`: `[('opencode', 11)]`; after `PUT` without dispose: `[('opencode', 11)]`;
     after `POST /instance/dispose`: `[('openrouter', 371), ('opencode', 11)]`).
   - Passing `OPENCODE_AUTH_CONTENT` at spawn time immediately exposes
     `[('openrouter', 371), ('opencode', 11)]` on `/config/providers` and
     `connected: ['opencode', 'openrouter']` (`source: "api"`) on `/provider` **without**
     writing any plaintext `auth.json` to disk.
   - **Decision:** Keep StackSpirit's zero-plaintext-on-disk contract (`OsKeychain` →
     ephemeral child `OPENCODE_AUTH_CONTENT` → clean sidecar restart; never write
     `auth.json` and never mix `OPENCODE_AUTH_CONTENT` with `PUT /auth/{id}`). Fix
     `EngineSupervisor::refresh_after_provider_change` so a key change while `Starting` is
     never skipped.

3. **Verification probe isolation, model selection & non-destructive UX:**
   - `ProcessEngineRuntime::verify_with_secret` previously spawned a second `opencode serve`
     sharing the live engine's `private_state` (`XDG_DATA_HOME/opencode/opencode.db`) and
     terminated it with `SIGKILL`, risking SQLite lock contention. It must instead spawn the
     verification probe with a dedicated ephemeral `tempfile::tempdir()` XDG tree (still
     passing the key only via `OPENCODE_AUTH_CONTENT`, never on disk).
   - `provider_model()` previously picked the first alphabetical model ID (for OpenRouter,
     `openrouter/aion-labs/aion-2.0`, an expensive reasoning model; `/config/providers`'s
     `default.openrouter` is `google/gemini-3-pro-image-preview`). Verification must pick
     up to 3 active, text-in/text-out, lowest-cost, non-reasoning-preferred candidate models
     from `/config/providers` (with `/provider` fallback) and retry on model-specific errors
     (400/404/`ProviderModelNotFoundError`, never on 401/403).
   - Verification must **never** delete a saved key on `invalid` or `unverified`. In
     `SettingsPage.tsx`, keep the provider in **Connected providers** with an honest badge
     (`Verified` when `state === "saved" && last_checked_at !== null`, `Saved`, `Invalid key`,
     `Unverified`), explicit status message, and `Verify key` / `Replace key` / `Remove key`
     controls. If the keychain genuinely reports `missing`, display an explicit alert rather
     than falling through to `"is still unverified"`.

4. **Zero-restart chat model propagation (`LocalChatProvider` + `EngineHost`):**
   - `EngineHost::usable_chat_models` must include `runtime.free` (`Included`), keychain-
     configured providers (`Byok`), and any non-`opencode` provider the running engine
     reports as connected (e.g. via env var), ordered `OpenCode Free` first, then connected
     providers in `POPULAR_PROVIDER_IDS` order then alphabetically.
   - `store_key`, `verify_key`, `delete_key`, and `restart_engine` must call
     `state.refresh_chat_models().await`, which updates `DesktopChatEngine`'s cache **and**
     invokes `LocalChatProvider::refresh_setup_models().await` to re-point an active setup
     session's endpoint/SSE pump to the restarted engine port and broadcast a fresh
     `ChatEvent::Session` on `spirit://chat`. Every mounted `ComposerBar` updates
     immediately without an app restart, regardless of whether the change came from
     `SettingsPage`, the composer modal, or `EngineView`.

### OpenCode-inspired searchable model picker contract (ss-87, landed)

With OpenRouter connected (`371` models + `11` OpenCode Free models = `382` models), a flat
unsearchable menu is unusable. Modeled on OpenCode's model selector
(`packages/app/src/providers/models/select-dialog.tsx` in the OpenCode reference checkout) while keeping
100% StackSpirit Meadow tokens and WCAG 2.2 AA contrast in light and dark:

- Clicking `composer-model` opens a searchable popover above the composer with:
  - Auto-focused search box (`Search models`, role `searchbox`, calm Meadow border-step
    focus) filtering by model label, model id, and provider label/id, with
    `ArrowUp`/`ArrowDown`/`Enter`/`Escape` keyboard navigation.
  - Bounded scrollable list (`max-height` capped so 370+ models scroll cleanly inside the
    viewport) grouped by provider: **OpenCode Free** first (with `Free` badge), then each
    connected provider as its own named section (e.g. **OpenRouter**, **Anthropic**,
    **OpenAI**, **DeepSeek**, ordered popular-first then alphabetical) with `Connected`
    badge, model label + secondary model id when helpful, and checkmark (`✓`) on the active
    selection.
  - Sticky footer inside the picker with **Manage models** (`data-testid="composer-manage-models"`,
    opening Settings → Engine & Models) and **Connect another provider…**
    (`data-testid="composer-connect-provider"`).
  - **Landed behaviour:** the popover is `role="dialog"` (`model-picker-popover`) with a
    `listbox` of `option` rows; `aria-activedescendant` on the search field tracks the
    highlight. **Manage models** and **Connect another provider…** both close the picker
    and open the in-place `Engine & Models` modal (the `Modal` + `SettingsPage` pattern),
    not `onOpenEngineSettings`, so the owner returns straight to the chat. Search matches
    every whitespace-separated token against label, id, provider id and provider label.
    An empty result shows `No models match “<query>”.`.

### Sequenced follow-up jobs queued by ss-85

| Job | Needs | Scope |
|---|---|---|
| `ss-86-fix-byok-keychain-verify-and-model-sync` | — | Enable `keyring` platform features + persistence guard, ephemeral-XDG `verify_with_secret` + smart candidate model selection, `Starting`-safe sidecar reload, `LocalChatProvider::refresh_setup_models` event broadcast, env-connected models in `usable_chat_models`, non-destructive `SettingsPage` / `EngineView` verify UX |
| `ss-87-searchable-model-picker-and-manage-models` | `ss-86` | Searchable provider-grouped model picker popover with keyboard nav, bounded scroll, free/connected badges, and **Manage models** button in `ComposerBar.tsx` + `SettingsPage.tsx` + `theme.css` (Meadow, WCAG 2.2 AA light & dark) |
| `ss-88-xvfb-byok-model-picker-verify` | `ss-86`, `ss-87` | Real VPS `xvfb-run` desktop boot smoke + live pinned `opencode` `1.19.5` sidecar BYOK round-trip test (key add → verify keeps key → 370+ OpenRouter models appear → search & select model → inspect `opencode.log` for zero errors/leaks → remove key) + Playwright UI E2E under `xvfb-run` |

### Landed in ss-86: real keychain, isolated verify probe, live model sync

- Root `Cargo.toml` now enables `keyring` features `apple-native`, `windows-native` and
  `linux-native`. Without a feature, `keyring` 3 compiles its in-memory mock store, which
  dropped keys the moment `OsKeychain::set` returned. A `keychain`-feature test asserts the
  default backend is not `EntryOnly` and round-trips a secret across two `OsKeychain`
  instances (it skips only when the container blocks the keyutils syscalls).
- `verify_with_secret` runs its throwaway `opencode serve` with a `tempfile::tempdir()` for
  `HOME`, the XDG dirs, `OPENCODE_CONFIG*` and the working directory, so it never touches
  the live engine's SQLite database. The key stays in `OPENCODE_AUTH_CONTENT` for that child.
- `select_verification_models` picks up to three active, text-capable candidates (non-reasoning
  first, then lowest `cost.input`, then `default[provider]`). A model-specific failure
  (400/404, `ProviderModelNotFoundError`, empty error) tries the next candidate; `Verified`
  and `Invalid` return at once.
- `refresh_after_provider_change` also restarts from `Starting`.
  `LocalChatProvider::refresh_setup_models` re-points the setup session at the restarted
  engine port and publishes a fresh `ChatEvent::Session`. `AppState::refresh_chat_models`
  calls it, and `verify_key` now refreshes chat models too.
- `usable_chat_models` also includes non-`opencode` providers the engine reports connected,
  sorted `Included` first, then BYOK by popular rank, provider id and label.
- Settings keeps a key visible after `invalid`/`unverified`, labels it `Verified`, `Saved`,
  `Invalid key` or `Unverified`, offers `Replace key`, shows an alert for `missing`, and
  refreshes the engine, key list and setup models after every verify.

### Landed in ss-87: searchable model picker and Manage models decision

- `ComposerBar.tsx`'s `ModelPicker` is now a popover with a `Search models` searchbox
  (`composer-model-search`, auto-focused on a user click), a bounded scroll list
  (`role="menu"`, `max-height: min(340px, 50vh)`) grouped by provider with `Free` and
  `Connected` badges plus the secondary model id, and a footer outside the scroll area.
  Search matches label, id, provider and tier (every term must match). `ArrowUp`/`ArrowDown`
  move a highlighted option through `aria-activedescendant`, `Enter` selects it, `Escape`
  closes and returns focus to the trigger.
- **Decision: Manage models opens the existing Engine & Models modal** (the same
  `SettingsPage` the connect action uses), not a new surface. It lives in the popover footer
  as `composer-manage-models`, next to `composer-connect-provider`. No new bridge call.
- **Decision: the recovery notice's "Switch model" keeps focus on the trigger** (the `ss-76`
  contract and `meadow-chat-retry.spec.ts` assert it); the search box takes focus only when
  the user opens the picker themselves. Tab reaches it.
- Verification: `npm run typecheck`, `lint` and the full `@stackspirit/ui` vitest suite
  pass. The Playwright specs were updated but not run in this job.

### Landed in ss-88: real-sidecar Xvfb verification (2026-11-21)

`ss-88` adds a repeatable, mock-free check of the BYOK flow on the Linux VPS (Xvfb,
`webkit2gtk-4.1`, Playwright Chromium, the pinned `opencode` `1.19.5` sidecar from
`node scripts/fetch-engine.mjs`, never `PATH`). The only key is a clearly synthetic
dummy assembled from parts at runtime; every check also scans for it. No real key,
`.env` file or secret value is read, printed or committed.

| Layer | File | What runs for real |
|---|---|---|
| Rust | `crates/spirit-core/tests/byok_live_sidecar.rs` | `ProcessEngineRuntime` + `EngineSupervisor` + `LocalChatProvider` + `verify_provider_key` over the real sidecar, in a `tempfile` app-data dir, with `FakeKeychain` always and the real `OsKeychain` under `--features keychain` (the test re-execs itself through a tiny helper that joins a fresh session keyring first, because this container starts with a read-only one) |
| Browser | `apps/spirit-desktop/e2e/xvfb-byok-live.spec.ts` | Real Chromium bridges `SettingsPage` and `ComposerBar` to a live sidecar's `/provider`, `/config/providers` and verification replies (the key lives only in the Node harness's memory and reaches the sidecar through `OPENCODE_AUTH_CONTENT`); axe WCAG 2.2 AA in light and dark |
| Runner | `scripts/verify-byok-xvfb.mjs` | fetch engine → Rust live test → desktop binary boot smoke under `xvfb-run -a` → the Playwright spec under `xvfb-run -a` |

Rust flow verified against the real `1.19.5` binary: start (`/global/health` healthy, `opencode`
free models only, `openrouter` not connected) → `connect_provider` restarts the sidecar, no
`auth.json` anywhere under `engine/data`, `/config/providers` and `/provider` report
`openrouter` connected with 100+ models → `verify_provider_key` through the ephemeral-XDG probe
returns `Invalid` (the synthetic key; `Unverified` offline) and the key is still in the
keychain → setup chat lists OpenCode Free and `openrouter/*` (`anthropic/`, `google/`) models →
`set_model` to an `openrouter/*` model updates `ChatSessionView.model` → `opencode.log` is
non-empty with zero panics, `SQLITE_BUSY`/`database is locked`, crashes or key occurrences
(also in `supervisor.logs()` and every file of the private tree) → `disconnect_provider`
removes the key, restarts, drops the `openrouter/*` models. A second test proves
`Verified → Saved`, `Unverified → Unverified` (including the `429`/`503` classifier cases) and
`Invalid → Invalid` all leave the saved key intact, and `Missing` only occurs for an empty
keychain.

Playwright flow verified: Setup chat picker shows only **OpenCode Free** → **Manage models**
opens Engine & Models → search `openrouter`, save the key (the host verifies once on save) →
**Verify key** reports `Invalid key` honestly and OpenRouter stays under **Connected
providers** with `Verify key`, `Replace key` and `Remove key` → the composer picker shows the
live OpenRouter group (100+ rows) beside OpenCode Free, `claude` and `gemini` searches narrow
it, and selecting an OpenRouter model updates the trigger → the sidecar's `opencode.log` has
zero panics/lock errors/key occurrences → no sidecar process survives the test.

#### Commands

```sh
node scripts/fetch-engine.mjs
scripts/cargo-gate.sh test -p spirit-core --features keychain --test byok_live_sidecar -- --nocapture --test-threads=1
xvfb-run -a npx playwright test e2e/xvfb-byok-live.spec.ts      # from apps/spirit-desktop
node scripts/verify-byok-xvfb.mjs                               # everything above plus the desktop boot smoke
BYOK_ONLY=rust|boot|e2e node scripts/verify-byok-xvfb.mjs       # one step while debugging
BYOK_ALLOW_KEYCHAIN_SKIP=1 node scripts/verify-byok-xvfb.mjs    # allow a keychain skip on an unusual host
```

The runner requires the real keychain on Linux x86_64 (`BYOK_REQUIRE_OS_KEYCHAIN=1`); the
test's keyring-join helper makes that pass here. On other hosts the test probes the native
backend and records a skip instead.

#### Verified results (this VPS, sidecar `opencode` `1.19.5`)

- Rust live flow, `FakeKeychain`: passed; verify state `Invalid`; `opencode.log` 51 lines,
  errors 0, leaks 0. `cargo test -p spirit-core` (default features) passes with it.
- Rust live flow, **real `OsKeychain`**: passed with `--features keychain` and
  `BYOK_REQUIRE_OS_KEYCHAIN=1`; verify state `Invalid`; the saved key was still in the
  kernel keyring after verification and the flow removed it at the end; `opencode.log`
  51 lines, errors 0, leaks 0. The container's inherited session keyring is read-only, so
  the test re-execs itself through a helper that joins a fresh anonymous session keyring;
  no secret store outside the test's unique service was touched.
- Playwright live spec: passed in light and dark (about 45 s each); `opencode.log` 24 lines,
  errors 0, leaks 0; axe clean (the one defect axe found then is resolved; see the resolved record below).
- `node scripts/verify-byok-xvfb.mjs`: **exits 0** after the shutdown fix below: fetch, both
  Rust flows (FakeKeychain and OsKeychain), the desktop boot smoke (6 s, `SIGTERM` clean, no
  orphan `opencode`, sidecar seen during boot, `opencode.log` errors 0, leaks 0) and both
  Playwright runs pass.
- `cargo test -p spirit-desktop --test engine_shutdown`: the `SIGTERM` regression test
  (real pinned sidecar) passes.

#### Defect fixed in ss-88: sidecar orphaned on `SIGTERM`

The boot smoke found that `target/debug/spirit-desktop` left one `opencode serve` parented to
PID 1 after `SIGTERM` (still alive 14 minutes later). `lib.rs` had no signal handler and no
`RunEvent` hook, so `EngineSupervisor`'s `Drop` (`kill_now`) never ran. The owner extended
the job to fix it: `run()` now builds the app and, on `RunEvent::ExitRequested`/`Exit`, calls
`stop_engine` (`EngineRuntime::kill_now`); `shutdown_on_signal` waits for `SIGTERM`/`SIGINT`,
stops the engine and then calls `app.exit(0)`. The signal path uses a libc
`signal()` handler that sets an atomic flag polled every 50 ms, so no manifest change is
needed. The runner still signals only the app process, never the process group, so the app must stop its own child.

#### Defects found by ss-88: all resolved

No ss-88 open defects remain. The three items the Xvfb run found are fixed on local `main`:

1. **Picker scroll region not keyboard-focusable** — fixed: `.nx-model-menu__scroll` has
   `tabIndex={0}` in `ComposerBar.tsx`, and `ss-104` (`120fd49`) removed the
   `scrollable-region-focusable` carve-outs from `meadow-provider-connect.spec.ts` and
   `xvfb-byok-live.spec.ts`; both specs now assert the full WCAG tag set.
2. **Picker clipped by the chat header at 1280×720** — fixed by `ss-89` (`5d09668`,
   `4dc9b0a`): the picker stays below the header and the spec exercises the pointer path.
3. **Settings notice read `Openrouter`** — fixed by `ss-103` (`24bb5cf`): Settings uses the
   shared `domain/engine` `providerLabel`, so it reads `OpenRouter`.

## 12. Chat UX state after ss-116..ss-118 (2026-11-23)

Verified by ss-119 against the merged ss-116..ss-118 state (results in the job report).

### 12.1 Real VOWP universal project orchestrator and OpenCode 2.x `/api` prefix (ss-116, ss-116b)

- **Project sessions are real orchestrators (ss-122b).** One-click Start
  (`chat_provision_orchestrator`) runs `ensure_project_orchestrator` in
  `commands/local_orchestrator.rs` when a host and an enrolled local node are connected
  (`LocalOrchestratorGate::Ready { node: Some(..) }`), then passes the result to
  `LocalChatProvider::provision_orchestrator_with(project, Some(KnownOrchestratorSession))`:
  1. **Home record.** `orchestrator_list`; if no card matches (`name == project` or
     `project == project`), `orchestrator_new { node, app: opencode, project, name: project,
     force: false }`. VOWP persists the home record (with its fresh `session_id`) *before*
     `vowp-agent` tries to start the app, and the agent may refuse with `surface_missing` or
     `surface_ambiguous`. A failed `orchestrator_new` therefore re-reads the list and recovers the
     card; only a failure with no persisted record is reported.
  2. **Dedicated `opencode` surface.** The Mac agent state (`surfaces/<id>/session` and `state`)
     identifies an active surface bound to `card.session_id`; it is reused only when home
     `surface ls --node` still lists it as a non-revoked `opencode` surface. Otherwise home
     `surface pair` mints a one-time code and the Mac runs `vowp --json agent pair --code-file -
     --surface <id> --session <id> --app opencode` with the code on **stdin**. The code is never
     logged, stored, put in argv or returned.
  3. **Binding.** `<agent state>/orchestrators/<project>/config.json` (`$VOWP_AGENT_HOME`, else
     `~/.vowp-agent`; directory `0700`, atomic `0600` write, existing fields preserved) gets
     `"surface"` and `"session"`, which is what `vowp-agent`'s `resolve_binding` reads. With
     `global` already paired on the same Mac the project is bound to its own surface and never hits
     `surface_ambiguous`.
  4. **Move / resume.** A card on another node is moved to the local node; one that is not
     `running`/`starting` (including a `failed` one left by a refused `new`) is resumed. An offline
     node is reported, not hidden. `vowp-agent` then writes `orchestrators/<project>/opencode.json`
     with `vowp mcp --agent-socket <socket> --surface <surface>` and serves
     `vowp agent orchestrator endpoint <project> --json`, which core resolves and attaches to.
- **Start never trusts a stale fallback.** `provision_orchestrator_with` always re-checks VOWP's
  endpoint first. A project session that earlier fell back to the bundled engine is upgraded to
  the real endpoint as soon as one resolves (it stops being engine-backed). Without an endpoint
  the fallback session is reused (no second native session) while its VOWP context, `AGENTS.md`
  and MCP registration are refreshed.
- **`vowp` MCP command.** Only `["vowp","mcp","--agent-socket",<socket>,"--surface",<surface>]`
  is ever produced, and only when both values are known (`KnownOrchestratorSession::mcp_command`).
  The installed CLI rejects `--agent` and requires `--surface`; `vowp mcp --help` is the contract.
- **Bundled-engine fallback context.** With a known universal session a `project:<name>` session on
  the bundled engine (a) registers the `vowp` MCP server with `POST /mcp` scoped to the project
  workspace directory, (b) carries the project name, the universal `session_id` and the
  `session_resume` / `fleet_status` / `fleet_queue` / `inbox_list` instructions (or an honest
  "tools not wired" line) in the per-message system prompt, and (c) refreshes
  `workspaces/project/<name>/AGENTS.md` with the same non-secret text (only a file StackSpirit
  wrote; an owner-authored one is left alone). No secret, token, pairing code, surface id or path
  is in any of them. `POST /mcp` is never sent for Setup, and the Setup `opencode.json`
  (deny-all, `OPENCODE_DISABLE_PROJECT_CONFIG=1`, no MCP) is unchanged (`setup_sandbox.rs`).
- **Verified against the real pinned sidecar (`opencode` 1.19.5, Setup's isolated env).** The
  global config reports every tool `deny`; a session created with
  `"permission":[{"permission":"*","pattern":"*","action":"ask"}]` still got a pending `bash`
  permission request while a plain session got none, so the per-session `ask` rule overrides
  Setup's deny-all for project sessions. `POST /mcp {name, config:{type:"local",command,
  enabled:true}}` with `x-opencode-directory` answered `{"vowp":{"status":"connected"}}`. A
  restarted engine forgets the registration, so core re-registers it on the next turn after a
  port change.
- **Not claimed live.** The live `vowp` tool calls need a running `vowp-agent`; tests use mock
  VOWP/agent clients and the fake engine, not a VPS. `LocalOrchestratorGate` and `AsifGate` stay.
- **Engine fallback is explicit.** Only an error starting with `chat_not_configured` (no VPS host
  or enrolled local node connected) falls back to the bundled engine. Any other provisioning error
  is shown to the owner.
- **Bundled-engine project/global sessions** run in `workspaces/global` and
  `workspaces/project/<name>` with the project-orchestrator instructions sent as the per-message
  system prompt (the pinned engine ignores workspace `AGENTS.md`). Unlike the setup chat they keep
  OpenCode's default tool access with every permission at `ask`, so the owner answers each prompt
  in the permission card. The setup prompt never leaks into project sessions.
- **OpenCode 2.x `/api` prefix.** `ChatEndpoint.api_prefix` is detected once per endpoint: an
  HTML (2xx, non-JSON) answer on `/session` with real JSON under `/api/session` means a 2.x host,
  and every request path then gets the `/api` prefix. Any doubt or error keeps the unprefixed 1.x
  paths. The 2.x `{"data": [...]}` message-list envelope is unwrapped to a bare array.
- OpenRouter provider verification retries transient failures (ss-116).
- Gates stay: `LocalOrchestratorGate` and `AsifGate` are unchanged by this work.

### 12.2 DeepSeek-Harness message chrome and telemetry strip (ss-117b)

- **User message:** a **Copy** button and a local-time `MM/DD HH:MM` stamp under the bubble.
- **Assistant message:** a **Completed** header (duration and step count when known) and a footer
  with tokens (including cache), model, preview tag, retry state and timestamp. Model and retry
  show on every reply; session usage only on the latest reply.
- **Bottom telemetry strip** under the composer: turns, steps, tok/s, tok and cache hit ratio,
  and the context ring (`ContextMeter`). Steps, tok/s and cache hit are omitted when unavailable.
- Coverage: `chatMessages.test.tsx` and `e2e/meadow-chat-messages.spec.ts` (axe WCAG 2.2 AA,
  light and dark).

### 12.3 Attachments and reasoning effort (ss-118)

- **Left-aligned `+`** in the composer attaches images and files (chips with remove). There is no
  Workspace Write dropdown. Limits: at most 8 attachments per turn and 8 MiB per `data:` URI
  (`MAX_ATTACHMENTS`, `MAX_ATTACHMENT_URL_BYTES`); oversized files show a clear error.
  Attachments travel as `ChatAttachment { filename, mime, url }` and reach OpenCode as `file`
  parts; chips clear after send.
- **Reasoning effort selector** is driven by OpenCode: the model catalog carries
  `variants` (for example `low`, `high`) and the selected value is sent as `variant`. The selector
  is hidden for models with no declared variants.
- Coverage: `composerAttachments.test.tsx` and `e2e/composer-attach-reasoning.spec.ts` (light
  and dark).
