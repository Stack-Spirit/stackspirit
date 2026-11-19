# Server chat options

Status: decision note for the owner, 2026-11-19. Document only; no code, contract or UI change.
Scope: the **Server** location chip for `global` and `project:<name>` chat in the Meadow
desktop shell. The setup assistant and Local chat are out of scope except where they show
what the same transport can reuse.

The question is narrow: after VOWP 0.5's orchestrator endpoint, how should StackSpirit reach
an orchestrator whose `opencode serve` runs on the VPS home? The answer is a product and
security decision before it is an implementation decision.

## 0. Short version

VOWP 0.5 now gives the app the endpoint document it was waiting for. It does **not** give the
Mac a route to a home-hosted loopback server:

- a **home-hosted** orchestrator returns a loopback URL (`http://127.0.0.1:<port>`) and a
  Basic credential, but that address is loopback **on the VPS**. The Mac cannot dial it
  directly.
- a **node-hosted** orchestrator correctly returns `location: "node"` with `url`,
  `native_session_id` and `auth` all `null` from the home command. The Mac must use
  `vowp agent orchestrator endpoint` on the machine that runs the app.

So the old blocker ("there is no endpoint command") is gone, and the remaining question is
the transport. There are four honest options:

| Option | One-line summary | Server chat works? |
|---|---|---|
| **A** | Keep Server gated until a first-class transport exists | No |
| **B** | App-owned SSH loopback forward to the home-hosted `opencode serve` | Yes, while the app is open and the home is reachable |
| **C** | Move the orchestrator Local and treat "Server" as "the log lives on the server" | No Server runtime; Local chat only |
| **D** | Use VOWP's own control-machine tunnel (`vowp tunnel`) or a read-only transcript | Yes/partial, with a different trust or product trade-off |

**Recommendation: B**, with **C** as the fallback if the owner does not want a long-lived
SSH forward. A leaves the ask unmet; D1 is not cleaner for an app because it adds a second
SSH trust path, and D2 is read-only. The exact decision is in
[§8](#8-recommendation-and-the-exact-owner-decision).

## 1. Vocabulary and invariants

| Term | Meaning here |
|---|---|
| **home** | The VPS that owns the VOWP database, leases and session logs. |
| **node** | An enrolled Mac agent that can run an app host. VOWP reports its name in `node`. |
| **Server** | StackSpirit's name for an app host on the **home**. The log is on the home. |
| **Local** | StackSpirit's name for an app host on **this Mac** (a VOWP node). The log is still on the home; VOWP mirrors the turns. |
| **Endpoint** | The `OrchestratorEndpoint` JSON document from `vowp orchestrator endpoint` (home) or `vowp agent orchestrator endpoint` (agent machine). |
| **Forward** | A local `ssh -N -L 127.0.0.1:<local>:127.0.0.1:<remote>` that maps a home loopback port to a Mac loopback port. |
| **Direct attach** | StackSpirit speaking the OpenCode v1 HTTP/SSE contract to the app host's loopback. It does not go through VOWP's session lease. |

Invariants this note preserves:

1. **One durable log.** `global` and `project` sessions keep their log on the home. The app
   attaches to a VOWP-reported native session; it never creates an unmirrored one under a
   `global`/`project` key.
2. **No fake reply and no silent fallback.** If the transport is not available, the UI shows
   a typed gate/error. It never falls back to the bundled engine or to preview text.
3. **The chip reflects the transport.** It is read-only and derived from
   `ChatSessionView.location`, not from a user toggle.
4. **Loopback only.** The bundled engine binds `127.0.0.1`, the forward binds `127.0.0.1`,
   and the remote side is `127.0.0.1`. There is no public listener and no wildcard bind.
5. **The renderer never receives a URL, port, password, identity path, remote path or
   native session id.** Those stay in `spirit-core`.
6. **The home log remains the VOWP log.** StackSpirit does not open VOWP files or write a
   second session log.

### 1.1 The direct-attach gap (applies to B, C and D1)

VOWP owns a session lease: one surface writes a session at a time. A UI that speaks the
app's HTTP API directly bypasses that lease. VOWP 0.5.1 records this as a known gap:

> A session has a lease with one writer, but a UI that attaches directly to the
> orchestrator's loopback engine bypasses it, so two surfaces (the OpenCode window and a
> desktop UI) can write to one session.

This is not new to Server chat; the live Local path has the same property today. VOWP 0.5.1
plans a desktop surface kind, an active-surface read model and an explicit takeover, but
that is **design only** and not shipped. Any option below that direct-attaches (B, C, and
D's tunnel) inherits this gap. The owner should decide whether that is acceptable until
VOWP's takeover lands. It is called out again in each option rather than repeated as a
hidden caveat.

## 2. What the VOWP 0.5 endpoint actually gives us

### 2.1 The two commands

```text
vowp [--home <HOME>] --json orchestrator endpoint <NAME>
vowp [--json] agent orchestrator endpoint <NAME>
```

The home command is a **home-side inspection**. It reads the home's
`~/.vowp/orchestrators/<name>/orchestrator.json` and, for a live home host, the app's
private `0600` service state. It never opens SSH, a socket or a node's files; the global
`--node` flag is consumed and is not an SSH redirect.

The agent command is **local to the agent machine**. It reads the agent's own
`~/.vowp-agent/orchestrators/<name>/` state and the app's private service file. It never
contacts the home.

VOWP's v0.5 scope marks the endpoint slice **implemented in the tree** on 2026-11-19; it was
not a public release tag at that point, and the version string was not bumped. The installed
binary on this host contains both commands but reports `0.4.0` (see §2.3). Probe the command,
not the version string.

### 2.2 The documents

A live **home-hosted** `opencode` orchestrator (shape; no real values):

```json
{
  "name": "<NAME>",
  "app": "opencode",
  "session_id": "<VOWP_SESSION_ID>",
  "location": "home",
  "node": null,
  "state": "running",
  "url": "http://127.0.0.1:<REMOTE_PORT>",
  "native_session_id": "<NATIVE_SESSION_ID>",
  "auth": {
    "header": "Authorization",
    "scheme": "Basic",
    "username": "opencode",
    "password": "<SERVICE_PASSWORD>"
  }
}
```

A **node-hosted** orchestrator from the home command (shape):

```json
{
  "name": "<NAME>",
  "app": "opencode",
  "session_id": "<VOWP_SESSION_ID>",
  "location": "node",
  "node": "<NODE_NAME>",
  "state": "running",
  "url": null,
  "native_session_id": null,
  "auth": null
}
```

Rules that matter for the options:

- `url` is only `http://127.0.0.1:<port>` or `http://localhost:<port>`. VOWP refuses
  `0.0.0.0`, a LAN address, `https`, a missing port and a zero port. The home host
  canonicalises to `127.0.0.1`. StackSpirit's Local resolver also accepts `[::1]`; the
  Server forward in B is IPv4-only (`127.0.0.1` on both ends), so B must reject `[::1]`
  rather than silently forward to the wrong address.
- `auth` is present only for a live local endpoint. Its password is a secret. VOWP's custom
  `Debug` redacts it; it appears only in the explicit `--json` endpoint document, never in
  a card, `snapshot.json`, `orchestrator ls`, a log or an error string.
- `native_session_id` is "when the local host knows one". A home-hosted `opencode` host
  records the native id it creates. A node-hosted `opencode` host may not: the desktop
  client creates that app session, so the agent command can report `null`. That is why the
  Local path has a discovery rule and the Server path does not.
- A `state` other than `running` or `degraded` (for example `stopped`, `node_offline`,
  `failed`) leaves the endpoint values null. Chat must gate; it must not start or resume
  the orchestrator itself.
- Only `app: "opencode"` is in this contract. `claude_code` and `chat` have no OpenCode
  loopback endpoint and are a typed gate.

### 2.3 What was verified live, and what was not

Read-only checks on this Linux host on 2026-11-19 (no IDs or secrets printed):

- `vowp --version` reports `vowp 0.4.0`, but the binary contains `orchestrator endpoint`,
  `orchestrator move` and `agent orchestrator endpoint`. VOWP 0.5.1's baseline note explains
  this: the v0.5 code-complete snapshot was built before the version string was bumped. **Do
  not use the version string as the endpoint gate; probe the command.**
- `vowp --json orchestrator ls` shows a `global` with `node` set and `state: running`, and
  an `asif` with `node: null` and `state: stopped`.
- `vowp --json orchestrator endpoint global` returns `location: "node"`, the node name,
  `state: "running"`, and `url`, `native_session_id` and `auth` all `null`. This is the
  correct metadata-only answer for a node-hosted orchestrator.
- `vowp --json orchestrator endpoint asif` returns `location: "home"`, `state: "stopped"`,
  and all three endpoint values `null`. A stale service file is not treated as live.
- `vowp agent orchestrator endpoint global --json` exits `3` with `not_found` on this host:
  this Linux host has no local orchestrator record. It is not the Mac.
- `vowp control status --json` returns the home-role guard; the `vowp tunnel` role guard is
  code-verified as control-only, so a forward is opened from the machine that uses it.

Not probed live because it would mutate the owner's state or needs the owner's Mac:

- a live **home-hosted** `opencode` endpoint with a non-empty `url`/`auth` (the only
  home-hosted card here, `asif`, is stopped);
- `vowp agent orchestrator endpoint` on the owner's Mac;
- a real home-hosted `opencode` `/global/health` response and a model turn through it.

Those are marked as assumptions below, not as proven facts.

### 2.4 The human path VOWP already documents

For a home-hosted `opencode` orchestrator, the card's `join.desktop` text already tells the
owner to run:

```text
vowp tunnel open '<NAME>' --remote-port <REMOTE_PORT>
```

on the **control machine**, then open the OpenCode pair link. That is a human path, not an
app transport: it depends on VOWP's control-machine configuration and the user's SSH
config. It is the basis of option D.

## 3. Option A — keep Server chat gated

### What the user sees

- Global/project chat opens on **Local** by default, exactly as today. Local works when the
  Mac agent reports an endpoint.
- Choosing Server (currently only through the typed bridge call/tests; there is no visible
  switch) shows the typed `chat_gated` banner, the composer is disabled, and no assistant
  reply appears.
- The current gate text says: "Server chat needs VOWP 0.5's `vowp orchestrator endpoint`."
  That is stale on a machine with the v0.5 endpoint commands (this host has them); on an
  older binary it would still be true. In either case the copy should name the actual
  missing transport, not a version. The honest gate is "Server chat needs a transport this
  app has not been approved to open" or "Server chat is not enabled in this build."
- The user can still use the VOWP/OpenCode app or `vowp chat` outside StackSpirit.

### What has to be built

Nothing for the transport. The only work is a copy/test correction:

- `packages/spirit-ui/src/bridge/MockBridge.ts` and the gate strings in
  `apps/spirit-desktop/e2e/*` and `packages/spirit-ui/src/test/next/nextShell.test.tsx`
  must stop naming the missing VOWP 0.5 endpoint;
- `crates/spirit-core/src/engine/chat/local.rs` keeps the `ChatSessionLocation::Server`
  gate;
- `docs/ORCHESTRATOR-CHAT.md` §3.1/§7.2 and `docs/HANDOFF-DESKTOP-UI.md` §5 step 4 are stale
  on "VOWP 0.4.0 has no endpoint"; a later docs job should correct them. This note does not
  edit them.

### Security properties

- No new listener, process, credential, URL or trust path.
- No long-lived SSH child and no local port exposure.
- The least new code and the smallest attack surface of any option.
- The direct-attach lease gap is not introduced because there is no attach.

### Failure modes

- The feature is unavailable. There is no date, because the first-class transport is not
  designed yet. VOWP 0.5.1's session ownership is design-only; a VOWP-owned proxy or turn
  API is not in the tree.
- The owner may believe Server chat is "coming with 0.5" and be wrong; the UI copy must say
  what is actually missing.
- The app still reads a Local endpoint; if the Mac agent is disconnected, Local is gated
  too. A adds no new Local failure.

### Size

**S.** One small UI/test job, roughly half a day. No core or VOWP change.

### Verdict

The safest option, with no new security surface. It does not solve the ask. Choose it only
if the owner explicitly prefers no Server chat for now and can name the first-class transport
being awaited.

## 4. Option B — app-owned SSH loopback forward

This is the design already sketched as `chat-server` in
[`docs/ORCHESTRATOR-CHAT.md`](ORCHESTRATOR-CHAT.md) §8.5, with the endpoint now real.

### What the user sees

- Opening Server shows "Connecting…", then a green `Server · <host label>` chip once the
  forward is healthy and the native session is attached.
- The composer is enabled. A turn is real: the user message is sent to the home's
  `opencode serve` through the forward, and the model reply is rendered. The current
  transport reports `streaming_supported: false`, so the reply appears as one complete
  message, exactly like Local today. This option does not add token streaming.
- The session is the same VOWP-mirrored session; the home log receives the turn through
  VOWP's surface mirror (the home host for Server; the agent bridge for Local). The app does
  not create a new native session.
- If the home orchestrator is `stopped`, `node_offline` or `failed`: a typed gate says
  "Resume it in VOWP"; the composer is disabled. The app does not start it.
- If the orchestrator is node-hosted on **this Mac**: the home endpoint says
  `location: "node"`; the app says "This orchestrator runs on this Mac; open Local" and does
  not silently switch. If it is node-hosted on **another Mac**: a typed gate says
  StackSpirit does not dial another Mac.
- If the app is `claude_code` or `chat`: "This orchestrator uses <app>; the OpenCode chat
  contract does not apply."
- If the host disconnects or the forward dies: the chip shows not connected, the composer
  is disabled, and a typed `host_disconnected`/`ssh_forward_failed` error appears. There is
  no Local fallback.
- If the endpoint has no usable native session: `chat_not_mirrored`; the user opens the
  conversation in OpenCode/VOWP. The Server path does **not** use the Local "newest host
  session" discovery rule.
- No URL, port, password, identity path or native id ever appears in the DOM, an error, a
  log or a report.

### What has to be built

#### 4.1 Home endpoint read over the existing verified SSH connection

Add one read to the VOWP client boundary:

```text
vowp [--home <HOME>] --json orchestrator endpoint <NAME>
```

- Use the existing `SshVowpClient`/`SshRemote` with the `HostProfile`'s `SshOptions`
  (`BatchMode=yes`, `StrictHostKeyChecking=yes`, app-private `known_hosts`, optional
  identity file, `IdentitiesOnly=yes`). Do not use a shell; argv only.
- Read timeout 20 s (a read), stdout cap 64 KiB, stderr bounded and redacted.
- Map exit `2` to "the installed VOWP has no endpoint command" and exit `3` to "no
  orchestrator"; do not gate on `vowp --version`, because a v0.5 binary can report
  `0.4.0` (verified here).
- Parse the typed document into a core-only `OrchestratorEndpointDoc`:
  `name`, `app`, `session_id`, `location`, `node`, `state`, `url`, `native_session_id`,
  `auth { header, scheme, username, password }`.
- The document must not derive `Serialize` to a chat DTO. Its `Debug` must redact the URL,
  port, session ids and password. The password belongs in `Zeroizing<String>`; the raw
  stdout buffer that contained it should also be zeroized or dropped promptly. Parse errors
  must not include the body.
- Gate before any forward:
  - `location == "home"`; a `node` location is the Local path or a "another Mac" gate.
  - `app == "opencode"`.
  - `state` is `running` or `degraded`.
  - `url` parses as a strict loopback URL. For this IPv4 forward, accept only
    `127.0.0.1:<port>` or `localhost:<port>` and normalize to `127.0.0.1`; reject
    `[::1]` rather than forward it to an IPv4 remote side. Reject userinfo, query, fragment,
    path and port zero.
  - `auth` is present and has a usable header/scheme/password. For a forwarded endpoint,
    `auth: null` is `vowp_incompatible`: a local TCP port on the Mac must not expose an
    unauthenticated remote loopback server.
  - `native_session_id` is usable; `None` is `chat_not_mirrored` for Server (no discovery).
  - If a stored VOWP `session_id` exists, it must match; otherwise `chat_not_mirrored`.

#### 4.2 SSH forward

Add a long-lived forward with the exact argv:

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

- `-L` is always explicitly `127.0.0.1:<local>:127.0.0.1:<remote>`. Never `*`, `0.0.0.0`,
  `-g`, a wildcard bind or a remote host supplied by the renderer.
- Reserve `<LOCAL_PORT>` with the existing `reserve_loopback_port()` shape (bind
  `127.0.0.1:0`, read the port, drop). `ExitOnForwardFailure=yes` makes a bind race fail
  immediately; retry exactly once with a fresh port, then return `ssh_forward_failed`.
- Start only when the `HostProfile` is `connected` and its host-key pin is verified against
  the app's `known_hosts`. A host-key mismatch is `host_key_mismatch`; there is no
  "accept new" path.
- The forward is one process per `(host_id, remote_port)`, ref-counted by attached Server
  sessions. Start it before the first HTTP request; wait for `GET /global/health` with the
  endpoint credential (3 s) or process exit.
- Kill it when the last session detaches, the host disconnects, the endpoint changes or the
  app exits. Never leave a live forward on purpose.
- Capture stderr bounded and redacted. Do not log argv (it contains the app-private
  known_hosts and identity paths); do not show raw stderr. Map `Permission denied` to
  `ssh_auth`, host-key failure to `host_key_mismatch`, and a forward bind/exit to
  `ssh_forward_failed`.
- The existing `CommandRunner` runs a command to completion and has no long-lived spawn.
  B needs either a small `ForwardRunner`/`spawn` extension or a dedicated child handle in
  `spirit-core`; tests must be able to assert the argv and simulate an early exit without a
  real network. Prefer a small trait so the core still owns the argv and tests never dial.
- `SshOptions` and `SshRemote` currently derive `Debug` including the identity path. The
  forward type must not log those values; add a redacted `Debug` at the forward boundary.

#### 4.3 Server chat provider

Reuse the existing OpenCode v1 HTTP/SSE client and state machine, but with a different
endpoint source and `ChatSessionLocation::Server`:

- `start` resolves the home endpoint, starts/reuses the forward, verifies
  `GET /global/health`, probes the v1/v2 dialect, attaches the reported native session,
  reads the log, and ensures the event pump. It never calls `ensure_engine`, never creates
  a native session and never calls `discover_native_session`.
- `attach` re-resolves the endpoint, reuses/restarts the forward, verifies the native
  session and backfills. If VOWP restarted the host and the password/port changed, it
  re-resolves; it does not keep the old forward.
- `send`, `stop`, `log` and the pump reuse the existing code. `stop` aborts/detaches; it
  never stops the VOWP orchestrator or the SSH host.
- The pump map in `LocalChatProvider` is keyed only by port. A local forward port can
  collide with a stale engine/orchestrator port after a restart. Key Server pumps by a
  private endpoint id (host id + remote port + generation) or extend the key; do not let a
  stale pump serve the wrong server.
- The transport keeps `streaming_supported: false` until the owner-turn verification in
  `docs/ORCHESTRATOR-CHAT.md` §9 pins the v1 SSE cadence. B does not claim streaming.

The clean implementation is to extract the HTTP/pump/log/state core from
`LocalChatProvider` into a transport that takes an endpoint provider, then have Local and
Server providers supply the endpoint. Duplicating `local.rs` would be faster to start and
worse to maintain; the size range below includes both.

#### 4.4 Tauri, bridge and UI

- `ChatStartInput.location` already carries `server`; `ChatSessionView.location` already
  serializes it. No new wire type is needed.
- The core chat service (owned by the desktop `AppState`) holds the forward supervisor, ties
  it to the host connection and app exit, and passes the endpoint to the chat provider. The
  Tauri command only routes calls; the renderer still sees only the safe view.
- `ChatView` already renders `Server · <label>` from `view.location`; the existing chip
  title says "Runs on the server. Works while this Mac is closed." Keep the claim only if
  the owner accepts that the **orchestrator** keeps running; the app itself still has to be
  open on some Mac to chat.
- Replace the stale gate copy and add the typed errors:
  `host_disconnected`, `ssh_forward_failed`, `chat_auth`, `chat_not_mirrored`,
  `protocol_unsupported` and `chat_gated` for state/app/location. The existing Local
  errors (`path_unmapped`, `local_agent_not_connected`) stay Local-only.
- The gate banner links to the host connection, not to a hidden toggle; there is no
  Settings consent switch in the recommended design. If the owner wants an explicit
  "Allow Server chat" consent, that is a separate small Settings job (the old `chat-settings`
  idea), not a hidden default.

### Security properties

- **Same trust path.** The forward uses the app's verified host key, app-private
  `known_hosts`, key auth, `BatchMode`, no agent forwarding, no X11 and no local command.
  It does not use the user's default SSH config.
- **Loopback only.** Local and remote binds are `127.0.0.1`; no public listener.
- **Credential handling.** The endpoint password is the OpenCode Basic credential. It is
  read from the home command's stdout over the encrypted SSH channel, held in zeroizing
  memory, attached only as the HTTP header, never serialized to the renderer, never logged,
  never put in argv, and never persisted.
- **A missing credential is a refusal.** Unlike the Local path, Server requires `auth`;
  `auth: null` is not "trusted loopback" once the port is exposed on the Mac.
- **Bounded blast radius.** A local process on the Mac can reach the forwarded port if it
  guesses the ephemeral local port, but it still needs the Basic credential. The forward is
  killed on detach/exit; a crash may orphan it (see failure modes).
- **Server compromise.** A compromised home can serve malicious model text, but it does not
  obtain the Mac's provider keys or any StackSpirit secret. The host key pin prevents a
  different server from being substituted.
- **Lease gap.** Direct attach bypasses VOWP's session lease (see §1.1). B does not fix
  this; it inherits the Local path's behavior. Until VOWP 0.5.1's takeover lands, the
  OpenCode window and StackSpirit can both write the session. The app never auto-approves a
  tool permission, but it cannot enforce one-writer semantics on the HTTP path.

### Failure modes

| Failure | Detection | User sees | Recovery |
|---|---|---|---|
| Host not connected | profile state / forward start | `host_disconnected`; composer disabled | Reconnect the host |
| SSH auth / key rejected | ssh exit 255 + stderr class | `ssh_auth`/`ssh_forward_failed` | Fix the key; retry |
| Host key mismatch | `StrictHostKeyChecking` / pin check | `host_key_mismatch` | Re-pin out of band |
| VOWP endpoint command missing | exit 2 | `chat_not_configured` | Upgrade VOWP; probe again |
| Orchestrator not found | exit 3 | `chat_not_configured` | Create it in VOWP |
| Node-hosted on this Mac | `location: node` + node matches agent | "Use Local", no switch | Open Local |
| Node-hosted on another Mac | `location: node` + different node | `chat_gated`; do not dial | Use that machine |
| Stopped/degraded/node_offline/failed | endpoint `state` or null `url` | `chat_gated`; "Resume it in VOWP" | Resume in VOWP |
| Unsupported app | `app` | `chat_gated` | Different app |
| Non-loopback URL / userinfo / `0.0.0.0` | URL validator | `vowp_incompatible` | Fix VOWP; never guess |
| Auth missing | endpoint `auth: null` | `vowp_incompatible` | Fix the host credential |
| Auth rejected | 401/403 | `chat_auth` | Re-resolve after VOWP restart |
| Forward bind race / early exit | `ExitOnForwardFailure` / child exit | `ssh_forward_failed` after one retry | Retry; check the host |
| Forward dies mid-turn | child exit / health fail | `host_disconnected`; turn may still run on the server | Reattach and backfill; do not claim stopped |
| v2 `/api/...` server | health 404 + v2 probe | `protocol_unsupported` | Use the pinned v1 server or wait for a v2 adapter |
| Native id null/unusable | endpoint | `chat_not_mirrored` | Open the conversation in VOWP; VOWP must report it |
| Session id mismatch | endpoint vs stored | `chat_not_mirrored` | Do not attach another log |
| Native session 404 | HTTP 404 | `session_lost` | Recreate in VOWP; setup can resume |
| Permission asked | SSE `permission.asked` | `permission_required`; never approve | OpenCode app |
| App crash with a live forward | next start finds an orphan/pid file | none until cleanup | Kill on next start; residual orphan risk |
| VOWP restarts the host | new port/password | `chat_auth`/`host_disconnected` | Re-resolve endpoint and restart forward |

### Size

**M–L.** Rough production ranges:

| Piece | Production | Tests |
|---|---|---|
| Endpoint read/parse/gate/redaction | 200–300 | 150–250 |
| SSH forward + lifecycle + child handle | 300–450 | 250–400 |
| Server provider / transport extraction | 500–800 | 400–600 |
| Tauri/state/bridge/UI copy | 100–200 | 100–200 |
| **Total** | **1,100–1,750** | **900–1,450** |

Two to three focused jobs (endpoint + forward, provider, view/live verification) and roughly
**3–5 worker days** plus half a day of owner live verification. The biggest schedule risk is
the `LocalChatProvider` refactor: a new provider that duplicates it is smaller now but
creates two copies of the HTTP/pump state machine. The second risk is the long-lived child:
the existing runner has no spawn API, so the forward needs a small, testable boundary.

### Verdict

The only option that delivers real Server chat now through the app's existing verified SSH
trust path. It is the planned `chat-server` job with the endpoint now available. It carries
the lease gap and the orphan/forward risks, both of which must be explicit in the owner
decision.

## 5. Option C — move the orchestrator Local and treat "Server" as the log

### What the user sees

- There is no Server runtime. Global and project chat always open on **Local · this Mac**,
  which is already the default in `packages/spirit-ui/src/next/conversations.ts` (`desired =
  location ?? "local"`).
- The location chip becomes `Local · this Mac`; a separate read-only line says
  `Log on <host>` or `Mirrored to <host>`. The old Server chip and its "Works while this Mac
  is closed" tooltip are removed; the orchestrator would no longer keep running on the
  server.
- Chat works while the Mac is awake and the VOWP agent is connected. If the Mac sleeps,
  closes or the agent disconnects, the composer is unavailable; the log is still on the
  server and readable through VOWP's own surfaces (`vowp session tail`, the web window, or
  `vowp chat`).
- No Local/Server switch, no Server gate, no new transport error. The app is honest that
  the orchestrator is local and the log is remote.

### What has to be built

The move itself is a VOWP mutation:

```text
vowp --json orchestrator move <NAME> --node <MAC_NODE>
```

Run on the **home**, not over a remote `--node` redirect. VOWP validates the target (an
active enrolled node, a path map for a project-bound orchestrator), stops the current host,
releases the lease, writes a handoff capsule on the existing session, and starts the app on
the target with the **same `SessionId`** and the same home log. An unknown, pending,
revoked or unmapped target is refused before the current host is stopped. The target node
must already have a paired surface for the session; otherwise the move refuses with
`surface_missing` or `surface_ambiguous` and starts nothing.

Two implementation levels:

- **Manual (smallest):** the owner runs the move once for `global` and each project. StackSpirit
  only removes the Server path, updates the chip/copy and tests. No core command, no new
  bridge method.
- **App-owned move (optional):** add `VowpClient::orchestrator_move(name, target)` over the
  existing SSH client, a bridge method and a confirmed UI action ("Run on this Mac"). Validate
  `LocalAgentView.is_connected()` and the agent node name first; require an explicit
  confirmation because the move stops the current host and changes where the orchestrator
  runs. Refresh the endpoint and reattach the Local chat after success. Map
  `node_offline`/`path_unmapped`/`conflict` honestly.

The Local chat path itself does not change: `vowp agent orchestrator endpoint <name>` already
returns the Mac loopback URL/auth, and `LocalChatProvider` already attaches. The VOWP agent
attaches the paired surface when it starts the orchestrator, so its bridge mirrors the turns
to the home log.

### Security properties

- **No app-owned forward, no new local listener, no port exposure.** The orchestrator runs
  on the Mac; VOWP's outbound agent connection mirrors its turns. StackSpirit speaks only to
  the local OpenCode loopback with Basic auth.
- **Same lease gap as today.** StackSpirit attaches directly to the local OpenCode HTTP
  server and bypasses VOWP's lease. The VOWP agent owns the lease and mirrors the turns; a
  second writer (the OpenCode window plus StackSpirit) can still interleave. VOWP 0.5.1's
  takeover design is the eventual fix.
- **No credential in StackSpirit.** The local endpoint password comes from
  `vowp agent orchestrator endpoint` and is already core-only; C does not add a new secret
  path.
- **The home log is still the durable copy.** The Mac has no second log; the agent's spool
  is an outbox, not a log. If the mirror is down, the log can lag or miss turns even though
  chat works locally (see failure modes).
- **Project context is the Mac checkout.** This is the owner's stated reason for Local by
  default; it is a feature, not a security claim. The server no longer runs the project's
  chat app; server-side tools that need the home checkout are not available to this session.

### Failure modes

| Failure | Detection | User sees | Recovery |
|---|---|---|---|
| Mac asleep/off | Local endpoint unreachable | `local_agent_not_connected` / `engine_unavailable` | Wake the Mac; agent reconnects |
| VOWP agent disconnected | `LocalAgentView` state | `local_agent_not_connected` | Reconnect/enroll |
| Move target offline | `vowp orchestrator move` | typed `node_offline`; old host stays | Connect the node; retry |
| Project path unmapped | move preflight | `path_unmapped`; old host stays | Map the path in VOWP |
| No paired surface | move | `surface_missing`/`surface_ambiguous`; nothing starts | Pair/fix the surface |
| Session conflict | move/agent | `session_conflict`; no new session | Repair the pairing; do not guess |
| Mirror bridge down | home log stops updating | chat still works locally; log-lag warning is not visible in the current bridge | VOWP owner checks the agent; add a future log-freshness signal |
| Two writers | OpenCode window + StackSpirit | interleaved turns; lease does not protect the HTTP path | VOWP 0.5.1 takeover; one writer at a time |
| Mac restarts | Local endpoint changes | reattach/Local reconnect | `vowp agent orchestrator endpoint` again |
| Home unreachable | Local app can still run, but mirror cannot write | local chat may work; home log stale | Reconnect; the app must not claim the log is current |

The mirror-down case is the important honest limitation: C gives chat when the Mac is up,
but it cannot promise the home log is current if VOWP's agent bridge is not connected. The
current UI has no "log as of" indicator for chat; a follow-up would be needed if the owner
wants one.

### Size

- **Manual move + UI semantics:** **S**, roughly 100–200 changed lines and 0.5–1 day. One UI
  job; the owner runs `vowp orchestrator move`.
- **App-owned move:** **M**, roughly 300–600 production lines plus tests, 1–2 jobs and 1–2
  days. No new forward or credential handling.
- **Live verification:** one owner run per moved orchestrator (`global` plus one project),
  confirming the same session id, a real Local turn, and the home log receiving it.

### Verdict

The smallest and safest way to stop pretending Server is a runtime. It matches the owner's
"Local by default" direction. It does not deliver Server chat, and it makes the app depend
on the Mac being awake. Choose it if the owner is willing to say "the orchestrator always
runs on the Mac; the server only owns the log."

## 6. Option D — what else the VOWP contract offers

### 6.1 D1: VOWP's own control-machine tunnel

VOWP already documents `vowp tunnel open <name> --remote-port <port>` as the human join
path for a home-hosted `opencode` orchestrator. The app could use it instead of building
its own forward:

1. Read `vowp --json orchestrator endpoint <name>` over the verified SSH connection to get
   `url`/`auth` (this part is the same as B).
2. On the Mac, run `vowp --json tunnel open <name> --remote-port <REMOTE_PORT>` and parse
   `local_port`/`pid`.
3. Use `http://127.0.0.1:<local_port>` with the endpoint auth.
4. On detach, `vowp --json tunnel close <name>`; on start, reconcile with `vowp tunnel ls`.

**What the user sees:** the same green `Server · <host>` chip and real (non-streaming)
turns as B once the tunnel is healthy. If VOWP control mode is not configured, a setup gate
says "Server chat needs VOWP control mode on this Mac"; if the tunnel dies, the chip shows
not connected with a typed `ssh_forward_failed`/`host_disconnected` error. There is no Local
fallback.

**What has to be built:** a one-time `vowp control init <ssh-target>` on the Mac (VOWP's
control role), a tunnel-name convention, the two command calls, stale-record reconciliation
and the same endpoint read/gate as B. No new `ssh -N` argv in StackSpirit.

**Security properties:** the tunnel binds `127.0.0.1` only and uses
`ExitOnForwardFailure=yes`; it records its pid/ports in VOWP's 0600 `tunnels.json`. It still
needs the endpoint's Basic credential, which stays core-only. The direct-attach lease gap is
the same as B. The important difference is trust: the tunnel uses VOWP's control SSH target
and the user's default SSH configuration, not the app's pinned host key, so StackSpirit
cannot claim the same host-key verification for this path.

**Failure modes:**

| Failure | Detection | User sees | Recovery |
|---|---|---|---|
| Control mode not initialized | `vowp control status` / `tunnel` guard | setup gate | Run `vowp control init` |
| Mac is a home, not control | VOWP role guard | setup gate | Use a control machine |
| Tunnel name conflict | `tunnel open` conflict | typed error; no duplicate | Close the stale tunnel |
| Tunnel process died | `tunnel ls` / child exit | `ssh_forward_failed` | Reopen; reconcile |
| Port race | `ExitOnForwardFailure` | `ssh_forward_failed` | Retry with a new port |
| Host key is not the app pin | user's `known_hosts`/config | no app-level pin error; this is the risk | Make VOWP use the app config first |
| Auth rejected | 401/403 | `chat_auth` | Re-resolve after VOWP restart |
| VOWP version/role mismatch | command exit | `chat_not_configured` | Upgrade/configure VOWP |
| Orphan tunnel on app crash | `tunnel ls` | none until cleanup | Close/reconcile on next start |

Why it is **not cleaner** for the app:

- `vowp tunnel` requires the **control** role (`NodeRole::Control`) and a
  `~/.vowp-control/control.json` (or `VOWP_CONTROL_HOME`). The Mac is an agent node; control
  mode is a separate VOWP configuration, and the app cannot assume it exists.
- The tunnel's `ssh -N ... -L 127.0.0.1:<local>:<host>:<remote> <ssh_target>` uses VOWP's
  `control.json.ssh_target` and the **user's default SSH config**. It does not set
  `StrictHostKeyChecking=yes`, an app-private `UserKnownHostsFile`, `IdentitiesOnly` or
  `ForwardAgent=no`. It is a different trust path from the app's pinned `HostProfile`.
  Using it would make StackSpirit's "verified host key, key auth" claim false unless the app
  also creates an app-private `HOME`/`SSH_CONFIG` and configures VOWP to use it. At that
  point it is B with an extra VOWP dependency.
- The tunnel process records a pid/ports in VOWP's `tunnels.json`; the app must close it and
  reconcile stale records. That is the same lifecycle work as B, split across two owners.
- It still needs the endpoint's Basic credential, so the secret path is unchanged.
- It is the exact command VOWP prints in `join.desktop`, so it is the right path for a human
  who is already in the VOWP control machine. It is not automatically the right path for a
  desktop app with its own host profile and key pin.

**Size:** S–M in StackSpirit (200–400 production lines plus tests, 1–2 days), but plus a
one-time control-mode setup and a security review of the second SSH path. **Verdict:** not
recommended unless the owner wants VOWP to own the tunnel and accepts that StackSpirit's
host-key pin no longer governs it. If D1 is chosen, make VOWP's SSH trust match the app's
pin first (a separate, larger job).

### 6.2 D2: read-only server transcript (stopgap, not chat)

If the owner chooses A but still wants visibility, the app can read the home session log
over the existing verified SSH connection:

```text
vowp [--home <HOME>] --json session show <SESSION_ID>   # metadata, if needed
vowp [--home <HOME>] session tail <SESSION_ID> [--follow] --pretty
```

`session show` returns the session metadata; `session tail` is the transcript source. Map
the events to `ChatMessage` and render a read-only transcript.

**What the user sees:** the Server transcript with an "as of" timestamp and a banner
"Reply in VOWP or OpenCode"; no composer, no Stop, no send. It is useful when the owner
wants to read what the orchestrator did without opening a terminal; it is not a chat.

**What has to be built:** a read-only VOWP log reader over the existing SSH client, an event
mapper to `ChatMessage`, pagination/tail handling and a read-only view. No auth, no forward,
no new secret, no write path.

**Security properties:** read-only; no credential handling and no listener. The log can
contain model text, tool output and paths, so the existing redaction and size caps still
apply; the home log is already the durable copy. It cannot approve a permission or send a
turn.

**Failure modes:** the home is unreachable (`host_disconnected`); the session is missing
(`not_found`); the tail is malformed/oversized (typed read error, cap); the log is stale
("as of" time, no live claim); no composer is available by design.

**Size:** S, 150–300 production lines plus tests, 0.5–1 day. **Verdict:** a useful companion
to A, not a replacement for B.

## 7. Comparison

| | **A** gate | **B** app forward | **C** Local + log | **D1** VOWP tunnel | **D2** transcript |
|---|---|---|---|---|---|
| Server runtime | existing, no chat | yes | no | yes | yes, read-only |
| Chat works while the app is open | no | yes | yes | yes | no send |
| Server keeps running while the Mac is closed | yes (existing) | yes | no | yes | yes |
| New listener | none | 127.0.0.1 forward | none | 127.0.0.1 tunnel | none |
| New SSH trust path | none | same as existing | none | VOWP control SSH (different) | same as existing |
| Credential path | none | endpoint Basic over SSH stdout | endpoint Basic over local agent | endpoint Basic over SSH stdout | none |
| Direct-attach lease gap | no attach | yes | yes (today) | yes | no |
| New code | none | M–L | S/M | S–M + setup | S |
| Owner setup | none | host connected | move orchestrator | `vowp control init` | none |
| Biggest risk | feature gap | orphan/lease gap | Mac must be awake | trust divergence | not chat |

## 8. Recommendation and the exact owner decision

### 8.1 Recommendation

**Recommend B.** It is the recommended option that delivers Server chat now using the
app's existing verified SSH trust path. The VOWP 0.5 endpoint removes the manual remote-port
and credential entry that the earlier appendix needed; it does not by itself approve an
app-owned forward. That approval is the decision below. The planned `chat-server` job in
`docs/ORCHESTRATOR-CHAT.md` §8.5 already names the pieces.

The residual costs are real and must be accepted explicitly:

1. one long-lived `ssh -N` child while a Server chat is attached, with a possible orphan on
   an app crash;
2. a direct OpenCode attach that bypasses VOWP's session lease until VOWP 0.5.1's takeover
   lands (the same gap Local has today);
3. non-streaming replies until the owner-turn verification pins the v1 SSE cadence;
4. a Server session that needs the home reachable and the VOWP host running; the app never
   starts or resumes it.

**C is the fallback** if the owner will not approve a long-lived forward or the direct-attach
gap. It is smaller, needs no new trust path, and matches "Local by default". It gives up
Server as a runtime and makes chat depend on the Mac being awake. **A is honest but does not
meet the ask**; it should name the first-class transport being awaited, not "VOWP 0.5's
endpoint", which exists. **D1 is not recommended** for the app; it is the human join path and
uses a second SSH trust configuration. **D2** is a read-only stopgap only.

### 8.2 The exact decision

The owner must choose one:

1. **B (recommended):** approve the app-owned SSH loopback forward with the exact argv,
   loopback-only bind, verified host key, key auth, endpoint-supplied Basic credential and
   ref-counted lifecycle in §4. Include the explicit acceptance of the direct-attach lease
   gap and the possible orphan forward until VOWP 0.5.1's takeover/cleanup.
2. **C:** approve removing Server as a runtime and renaming the chip to `Local · this Mac`
   with `Log on <host>`. Move `global` and each project with
   `vowp orchestrator move <NAME> --node <MAC_NODE>` (or approve the app-owned move action).
   Accept that chat is unavailable when the Mac is asleep or the agent is disconnected, and
   that the home log can lag if the agent's mirror bridge is down.
3. **A:** keep the gate, correct the stale "VOWP 0.5 endpoint" copy, and name the
   first-class transport being awaited. No date is promised. If the owner wants visibility
   meanwhile, add D2 (read-only transcript).
4. **D (not recommended):** approve VOWP control mode plus `vowp tunnel` and accept that
   the forward uses VOWP's SSH configuration rather than the app's host-key pin; or approve
   D2 as a read-only stopgap while A is in force.

If B is chosen, the owner should also answer these three sub-questions:

- May StackSpirit keep one `ssh -N` forward open while a Server chat is attached, and kill it
  when the last chat detaches (with cleanup on the next start after a crash)?
- May Server chat use a direct OpenCode attach even though it bypasses VOWP's lease until
  0.5.1? If no, B is blocked and the only honest paths are A or a future VOWP-owned turn
  API; C and D1 have the same gap for Local/Server, and D2 avoids it by being read-only.
- May Server chat ship non-streaming first, with the same `streaming_supported: false`
  behavior as Local, until the owner-turn verification pins SSE?

If C is chosen, the owner should confirm that "Server" in the UI means **where the log
lives**, not where the orchestrator runs, and that the `Server · <host>` chip is retired.

## 9. Facts, evidence and assumptions

### 9.1 Verified in code (read in this job)

| # | Fact | Evidence |
|---|---|---|
| 1 | `vowp orchestrator endpoint` is a home-side inspection; it never opens SSH or reads a node's files. | `vowp/crates/vowp-cli/src/cmd/orchestrator.rs`; `vowp/docs/design/cli.md` §6.10 |
| 2 | `OrchestratorEndpoint` carries `name`, `app`, `session_id`, `location`, `node`, `state`, `url`, `native_session_id` and `auth`. `OrchestratorAuth` carries header/scheme/username/password; its `Debug` redacts the password. | `vowp/crates/vowp-protocol/src/surface.rs` |
| 3 | A home-hosted live `opencode` endpoint fills `url`, `native_session_id` and `auth`; a node-hosted record returns the metadata-only document with all three `null`. | `vowp/crates/vowp-core/src/surface/orchestrator.rs` (`endpoint`); `vowp-core/tests/orchestrator_endpoint.rs` (`orchestrator_endpoint_home_reads_the_live_local_service_file`, `orchestrator_endpoint_home_is_metadata_only_for_a_node`, `orchestrator_endpoint_remote_never_returns_auth`) |
| 4 | The home `opencode` host records the native session id it creates; the endpoint reads it from the host state. | `vowp/crates/vowp-core/src/surface/opencode_host.rs` (`start`, `endpoint`) |
| 5 | `vowp agent orchestrator endpoint` reads the agent's own state and reports `location: "node"`; a live `opencode` host fills the loopback `url` and Basic `auth`; `native_session_id` can be `null` because the desktop creates that app session. | `vowp/docs/design/v0.5/orchestrator-endpoint.md`; `vowp/docs/design/cli.md` §12 |
| 6 | `vowp orchestrator move` preserves the same `SessionId` and the same home log; it validates the target before stopping the current host. | `vowp/docs/design/cli.md` §6.10; `vowp/docs/design/v0.4/local-orchestrator.md` §5.7 |
| 7 | `vowp tunnel` is control-machine-only and refuses on the home; its argv is `ssh -N -o BatchMode=yes -o ExitOnForwardFailure=yes -o ServerAliveInterval=15 -L 127.0.0.1:<local>:<host>:<remote> <target>`, with no app-private known_hosts, identity pin or `StrictHostKeyChecking=yes`. | `vowp/crates/vowp-cli/src/cmd/tunnel.rs`; `vowp/docs/design/cli.md` §6.4 |
| 8 | VOWP 0.5.1 records the `native_session_id` gap for a node-hosted `opencode` host and the direct-attach lease/ownership gap; its session-ownership slice is design-first. | `vowp/docs/design/v0.5.1/scope.md` |
| 9 | StackSpirit's Local path runs `vowp agent orchestrator endpoint <name> --json` with argv only, a 10 s budget and a minimal environment; it parses the endpoint and keeps the URL/port/auth/session id core-only. | `crates/spirit-core/src/engine/chat/endpoint.rs`; `crates/spirit-core/src/engine/chat/local.rs` (`resolve_local_endpoint`) |
| 10 | StackSpirit's chat transport already supports a host, port, optional directory header and a zeroizing Basic credential; `request_head` attaches the auth header. | `crates/spirit-core/src/engine/chat/local.rs` (`ChatEndpoint`, `request_head`, `http_request`) |
| 11 | `start_orchestrator` uses discovery only when the endpoint reports no native id; the Server path must not reuse that rule. | `crates/spirit-core/src/engine/chat/local.rs` (`open_orchestrator`, `discover_native_session`) |
| 12 | `validate_start` gates `ChatSessionLocation::Server` today; the UI opens `global`/`project` on Local by default. | `crates/spirit-core/src/engine/chat/local.rs`; `packages/spirit-ui/src/next/conversations.ts` |
| 13 | The chip is derived from `ChatSessionView.location`; the Server tooltip says "Runs on the server. Works while this Mac is closed." | `packages/spirit-ui/src/next/ChatView.tsx` |
| 14 | The current gate copy names VOWP 0.5's endpoint, so it is stale now that the endpoint exists. | `packages/spirit-ui/src/bridge/MockBridge.ts`; `apps/spirit-desktop/e2e/next-shell.spec.ts`; `packages/spirit-ui/src/test/next/nextShell.test.tsx` |
| 15 | `SshOptions`/`ssh_command_inner` already build the strict host-key, app-known_hosts, key-auth argv used by every VOWP command. | `crates/spirit-core/src/ssh/mod.rs` |
| 16 | `SshOptions`/`SshRemote` derive `Debug` including the identity path, so the forward must not log them. | `crates/spirit-core/src/ssh/mod.rs` |
| 17 | `CommandRunner` runs a command to completion and has no long-lived spawn; a forward needs a small new boundary. | `crates/spirit-core/src/ssh/runner.rs` |
| 18 | The VOWP agent attaches the paired surface when it starts an orchestrator, which is the mirror path for Local turns. | `vowp/crates/vowp-agent/src/orchestrator.rs` (`finish_start`, `SurfaceAttachment`) |

### 9.2 Verified live (read-only, this host, 2026-11-19)

| # | Check | Result |
|---|---|---|
| 1 | `vowp --version` | `vowp 0.4.0`, although the binary contains the v0.5 commands |
| 2 | `vowp orchestrator --help` | lists `new`, `ls`, `move`, `stop`, `resume`, `endpoint` |
| 3 | `vowp agent orchestrator --help` | lists `endpoint` |
| 4 | `vowp --json orchestrator ls` | `global` is node-hosted and running; `asif` is home-hosted and stopped |
| 5 | `vowp --json orchestrator endpoint global` | `location: "node"`, node present, `state: "running"`, `url`/`native_session_id`/`auth` all `null` |
| 6 | `vowp --json orchestrator endpoint asif` | `location: "home"`, `state: "stopped"`, `url`/`native_session_id`/`auth` all `null` |
| 7 | `vowp agent orchestrator endpoint global --json` | exit `3`, `not_found`: no local record on this Linux host |
| 8 | `vowp control status --json` | guard: this host is a home; a tunnel is opened elsewhere |

No real session id, host name, path, port or password is recorded above.

### 9.3 Assumptions (not proven here)

| # | Assumption | Why it matters |
|---|---|---|
| 1 | The owner's Mac has a live node-hosted `global` orchestrator and the v0.5 `vowp agent orchestrator endpoint` command | Local chat is live per the handoff; the exact Mac read was not available in this job. Probe the command, not the version string |
| 2 | A live home-hosted `opencode` endpoint returns a non-empty password and a usable `native_session_id` | Code/tests support it; not live because the only home-hosted card here is stopped |
| 3 | The home's `opencode` server answers the pinned v1 `/global/health` contract | The client gates on it; a v2 server would be `protocol_unsupported` |
| 4 | VOWP's agent bridge mirrors StackSpirit's direct Local turns to the home log | The design/agent code attaches the surface; a live end-to-end check is still owner work |
| 5 | VOWP's home surface mirror records StackSpirit's direct Server turns to the home log | The home host owns the mirror for a home-hosted app; not live-checked here |
| 6 | The Mac and the VPS are on the same VOWP home and the host profile points at that home | The endpoint's `session_id` and the stored log must be the same |
| 7 | A long-lived `ssh -N` child can be killed on detach, but an app crash may leave it until cleanup | Standard process behavior; no parent-death signal on macOS |
| 8 | The owner's Mac can run `vowp control init` and `vowp tunnel` without conflicting with its agent role | D1 only; not checked on the Mac |
| 9 | The owner accepts non-streaming Server replies initially | The current transport disables live text until the owner-turn verification |

## 10. Implementation sketch if B is approved

This is a plan, not a change made by this job. It follows the existing `chat-server` split and
[`docs/ORCHESTRATOR-CHAT.md`](ORCHESTRATOR-CHAT.md) §4.3, §7.2 and §8.5.

### 10.1 Files and jobs

| Job | Owns (illustrative) | Work |
|---|---|---|
| `chat-server-endpoint` | `crates/spirit-core/src/vowp/orchestrator_endpoint.rs` (new), `crates/spirit-core/src/vowp/mod.rs`, `crates/spirit-core/src/vowp/client.rs`, `crates/spirit-core/tests/chat_server.rs` (new) | Home endpoint read, typed doc, redacted parse, gates, zeroizing credential |
| `chat-server-forward` | `crates/spirit-core/src/ssh/forward.rs` (new), `crates/spirit-core/src/ssh/mod.rs`, a small spawn boundary in `ssh/runner.rs` or a `ForwardRunner` | Exact argv, port reservation/retry, child lifecycle, stderr classification, ref-count |
| `chat-server-provider` | `crates/spirit-core/src/engine/chat/server.rs` (new), the serial integration edits in `chat/local.rs`/`mod.rs` | Endpoint source, health/v1 gate, native attach, forward lifecycle, Server location, pump key |
| `chat-server-view` | `apps/spirit-desktop/src-tauri/src/commands/chat*.rs`, `state.rs`, `packages/spirit-ui/src/next/ChatView.tsx`, the gate tests/e2e | Enable Server, chip copy, typed errors, no hidden toggle |

The exact `owns` lists must be written by the supervisor; this table is only a split.

### 10.2 Acceptance tests

- Endpoint document parse/gate: home vs node, state, app, null URL, non-loopback URL,
  userinfo/query/path, null auth, unusable native id, session-id mismatch, exit 2/3,
  oversized output, parse error with no body/secret.
- Forward argv: `-N`, strict host key, app-known_hosts, identity only with
  `IdentitiesOnly`, no `-A`/`-X`/`-D`/`-R`/`-g`/`0.0.0.0`/`*`, remote side `127.0.0.1`,
  `ExitOnForwardFailure`.
- Forward lifecycle: ref-count, kill on last detach, restart on port change, bind race
  retry once, early exit, stderr redaction, no raw argv in logs.
- Server provider: fake endpoint server through a fake forward; health 200/401/404/v2;
  attach without discovery; no native session creation; log/send/stop; host disconnect;
  no Local fallback; pump key uniqueness; no URL/port/password in any serialized view,
  event, error or report.
- UI: Server chip only after attach, gate on typed errors, composer disabled, no preview
  reply, no stale "VOWP 0.5 endpoint" copy.
- Owner live: a real home-hosted `opencode` orchestrator, one free model turn, the home log
  receiving it, stop, reattach, forward kill, host disconnect, and no secret/path in the
  DOM/log/error. Redact the evidence.

### 10.3 What B does not include

- Token-by-token streaming (separate owner-turn verification).
- VOWP 0.5.1 session ownership/takeover; B direct-attaches and accepts the gap.
- A Local/Server switch or `vowp orchestrator move` UI; that is C or the later switch job.
- A second SSH configuration or `vowp tunnel`; B uses the app's existing `HostProfile`.
- Any change to the home secret store, provider keys, or VOWP files.

## 11. References

- [`docs/ORCHESTRATOR-CHAT.md`](ORCHESTRATOR-CHAT.md): §2 local engine, §3 server path,
  §4.3 `chat` surface, §7.2 blocked work, §8.5 `chat-server`, §9 owner verification.
- [`docs/HANDOFF-DESKTOP-UI.md`](HANDOFF-DESKTOP-UI.md): §3 sessions and decision A, §5
  step 4 (chat surfaces) and step 6 (move/switch).
- VOWP repository (read-only):
  - `docs/design/v0.5/orchestrator-endpoint.md`
  - `docs/design/v0.5/scope.md`
  - `docs/design/v0.5.1/scope.md`
  - `docs/design/v0.4/local-orchestrator.md` §5.7
  - `docs/design/cli.md` §6.4, §6.10, §12
  - `crates/vowp-protocol/src/surface.rs`
  - `crates/vowp-core/src/surface/orchestrator.rs`
  - `crates/vowp-core/src/surface/opencode_host.rs`
  - `crates/vowp-agent/src/orchestrator.rs`
  - `crates/vowp-cli/src/cmd/tunnel.rs`
- StackSpirit code: `crates/spirit-core/src/engine/chat/{endpoint,local,model,pump}.rs`,
  `crates/spirit-core/src/ssh/{mod,runner}.rs`, `crates/spirit-core/src/vowp/client.rs`,
  `packages/spirit-ui/src/next/{conversations.ts,ChatView.tsx}`.

## 12. Decision recap

**Recommendation: B.** The owner must choose one of the four decisions in
[§8.2](#82-the-exact-decision):

- **B (recommended):** approve the app-owned SSH loopback forward in §4, including the
  direct-attach lease gap and the possible orphan forward until VOWP 0.5.1's takeover.
- **C (fallback):** move the orchestrators Local, retire the Server runtime, and show
  `Log on <host>` instead; accept that chat needs the Mac awake.
- **A:** keep the gate, correct the stale "VOWP 0.5 endpoint" copy, and name the first-class
  transport being awaited. No date is promised; D2 can provide a read-only transcript.
- **D (not recommended):** use VOWP control mode and `vowp tunnel`, or D2 as a stopgap;
  accept the second SSH trust path or the lack of a composer.

If B is chosen, the three sub-questions in §8.2 (long-lived forward, direct attach vs the
lease, non-streaming first) must be answered before implementation starts.
