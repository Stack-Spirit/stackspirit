# Harness-backed embedded chat

Status: owner decision recorded 2026-11-28, design updated the same day. The owner
answered question `01M8EYDCVXEZS92FCADETEB8F2` with **B — ACP (Agent Client Protocol) first** and asked
for the broader registry split. `ss-179` landed that `HarnessSurface` split
(**Orchestrator + Workers (ACP / Embedded Chat)** versus **Workers Only (Stock CLI
Terminal View)**), the expanded presets, the `HarnessesPanel` grouping and a reusable
ACP stdio JSON-RPC client (`crates/spirit-core/src/harnesses/acp.rs`) with mock-process
tests. That client is a foundation, not a wired chat transport: **no non-OpenCode
harness-backed conversation can start yet**. OpenCode remains the only live
embedded-chat transport, and the selection/gates plus the `HarnessChatAdapter` wiring
remain Phase 1/2 work. This document changes no runtime.

Scope: how a harness selected in the Universal Harness Registry could back
StackSpirit's own embedded chat without weakening the current OpenCode path, the
one-writer rule or the renderer boundary. It covers the selection model, the Rust
adapter contract, the four chat-capable protocol families, safety/honesty gates, a
phased delivery plan and a test strategy.

Non-goals: no implementation in this document, no registry schema change beyond the
`ss-179` surface split, no handoff claim, no new secret store, no `cli-pty` chat, no
remote (Home VPS) chat transport, and no change to the VOWP-owned `global`/`project`
log contract. The handoff remains the source of truth for what is live; nothing here
overrides it.

## 0. Short version

- The embedded chat that exists today is **OpenCode-only**. The bundled OpenCode
  sidecar backs the setup assistant and the local project fallback; VOWP-reported
  local orchestrators are also OpenCode. `ss-179` added the registry's
  `HarnessSurface` split (**Orchestrator + Workers (ACP / Embedded Chat)** versus
  **Workers Only (Stock CLI Terminal View)**) and a reusable ACP stdio client
  (`crates/spirit-core/src/harnesses/acp.rs`). Consumer-OAuth CLIs such as Claude Code
  and Gemini CLI are workers-only and are never wrapped in embedded chat. Connect still
  only installs and signs in: there is no chat-transport selector, no harness-chat gate
  and no `HarnessChatAdapter` wiring from an ACP session into chat yet.
- **Recommendation:** one **global default** for new app-owned local conversations,
  captured by each session at creation. Existing sessions never move when the default
  changes. A per-session picker is deferred until there is more than one eligible
  app-owned conversation kind (or VOWP supports a non-OpenCode app).
- The **VOWP boundary is real**: `global` and `project` chats keep their durable log
  in VOWP and the endpoint validator only accepts `app: "opencode"`. A local CLI
  harness cannot mirror turns into that log in this phase. Those sessions stay
  OpenCode and show `VOWP · OpenCode`, not a harness selector.
- The adapter contract is transport-agnostic: start, send a turn, stream safe events,
  abort, list models when proven, handle native ids, honour ownership, stay bounded
  and reap every process. `opencode-http`, `stream-json`, `codex-json` and `acp` map
  onto it; `cli-pty` remains worker-only.
- Phase 1 ships selection, gates and a transport-less dry run. Phase 2 adds the first
  adapter, **ACP (the owner's choice)**; the reusable `acp.rs` client has landed, but
  the chat adapter, streaming mapping and contract suite have not. Phase 3 adds the
  next. Phase 4 adds model/usage parity. Every phase has a one-line acceptance test and
  a suggested `owns`.

## 1. Current state

### 1.1 The embedded chat that exists

| Piece | What is real today |
|---|---|
| Public core boundary | `ChatTransport` in `crates/spirit-core/src/engine/chat/local.rs` (`start`, `attach`, `send_turn`, `stop`, `provision_orchestrator`, `set_model`, `log`); `LocalChatProvider` implements it. |
| Engine boundary | `ChatEngine` (`ensure_ready`, `selected_model`, `available_models`, `select_model`, `status`) is injected; the desktop wires it to the single `EngineSupervisor`. Chat never spawns a second engine. |
| Bundled engine | `engine/manifest.json` pins OpenCode `1.19.5`; `serve --hostname 127.0.0.1 --port <reserved>`; health `GET /global/health`. The contract is `docs/ORCHESTRATOR-CHAT.md` §2.1–§2.2. |
| Session keys | `ChatSessionId` is only `setup`, `global` or `project:<name>`. It is a shell routing key, never a native or VOWP session id. |
| Safe DTOs | `ChatSessionView`, `ChatMessage`, `ChatEvent`, `ChatUsageView`, `ChatModelView`, `ChatOwnershipView` and `ChatAccess` in `crates/spirit-core/src/engine/chat/model.rs`. None carries an endpoint URL, port, credential, native id, workspace or binary path. |
| Event pump | `ChatEventBus` adds a monotonic revision; `message` upserts by id, `delta` only appends, `done` closes the turn, `error` is typed. A revision gap asks the transport to reload its log. |
| Streaming honesty | Production starts with `StreamSupport::Unpinned`; the pinned v1 event names are recorded, but the runtime cadence of `message.part.updated`, `message.part.delta` and `session.idle` is not proven without a model turn. The setup chat uses the final message response and says live text is unavailable. |
| VOWP local chat | `global`/`project` attach to a VOWP-reported local orchestrator endpoint. The resolver rejects an app other than `opencode`; Server chat remains gated. |
| Renderer path | The desktop `TauriBridge` and `MockBridge` implement `SpiritBridge.chat`; the read-only Web bridge omits it. Preview messages are the only `origin: "preview"` values. |

The important gap: every one of these paths is named OpenCode. `LocalChatProvider` is
the only provider, and the desktop `AppState` holds exactly one
`Arc<LocalChatProvider>`.

### 1.2 The registry is a capability claim, not a transport

`crates/spirit-core/src/harnesses/` owns the registry:

- `HarnessProtocol` has five values: `opencode-http`, `stream-json`, `codex-json`,
  `acp` and `cli-pty`.
- `HarnessManifest.surface` is the authoritative split: `orchestrator_and_workers`
  can back the embedded chat; `workers_only_terminal` is a stock CLI for the terminal
  worker view. `supports_embedded_chat` remains a legacy mirror of the same fact.
  Neither says a transport exists, and neither selects one.
- `HarnessProbe` copies the protocol, surface and capability into every result, then
  reports `binary` (`found`/`missing`/`unknown`), `auth` (`configured`/`missing`/
  `unknown`) and `readiness` (`ready`/`install_required`/`auth_required`/`unknown`).
- `HarnessConnectPlan` installs and starts the harness's own sign-in. It does not
  start a chat transport.
- The desktop panel groups by `surface`, labels a harness **Embedded Chat** or
  **Worker-Only**, and says plainly that the embedded chat still runs on OpenCode.
  `docs/HARNESSES.md` records the same limit.

Built-ins that matter for chat (the full catalog is in `docs/HARNESSES.md`):

| id / group | Protocol | Surface | Chat eligibility in this design |
|---|---|---|---|
| `opencode` | `opencode-http` | `orchestrator_and_workers` | yes; resolves to the existing bundled provider |
| `codex` | `codex-json` | `orchestrator_and_workers` | later; adapter remains probe-first |
| ACP orchestrator presets (`goose`, `cursor-cli`, `github-copilot`, …) | `acp` | `orchestrator_and_workers` | eligible after the ACP adapter is proven; ACP is the owner-selected first family |
| `claude-code` | `stream-json` | `workers_only_terminal` | no; consumer OAuth stays in its own CLI |
| `gemini-cli` | `acp` | `workers_only_terminal` | no; consumer OAuth stays in its own CLI |
| `antigravity`, `aider`, `crush`, `amp` | `cli-pty` | `workers_only_terminal` | no; terminal worker view only |

Two registry facts still matter for the gate:

1. `HarnessManifest::validate()` rejects a `surface`/`supports_embedded_chat` mismatch
   and any `cli-pty` harness that claims `orchestrator_and_workers`. The chat gate must
   still re-check the surface on the resolved manifest, so an older or hand-edited
   payload cannot bypass it.
2. `HarnessRegistry::save_custom()` overwrites an existing custom id, so a bound
   harness's protocol can change under a session. The selection layer must refuse that
   edit while the id is the default or an active session uses it.

### 1.3 Typed gates and errors that already exist

The repo already uses typed gates for other capabilities. They are the pattern to
follow, not chat gates to reuse:

- `LocalOrchestratorGate` (`Unknown`, `Probing`, `Ready`, `WaitingForVowpPhase3`,
  `Incompatible`, `Unreachable`) and `AsifGate` (`Unknown`, `Probing`, `Ready`,
  `WaitingForVowpAsif`, `Incompatible`, `Unreachable`) live in
  `crates/spirit-core/src/engine/gate.rs`. `GateSnapshot` adds the Mac-side agent
  status and `GateFeatureFlags`; actions stay disabled unless the gate is `Ready` and
  the matching flag is on.
- The chat transport already distinguishes `chat_gated`, `chat_not_configured`,
  `chat_not_started`, `chat_busy`, `session_lost`, `stream_lost`,
  `stop_unconfirmed`, `no_model`, `engine_not_ready` and `chat_model_gated`. They are
  `SpiritError` values with stable codes and fixed message prefixes.
- `StreamSupport` is a typed capability: `Pinned` or `Unpinned { reason }`. The view
  reports `streaming_supported` from it, never from a guess.

There is **no** `HarnessChatGate` today. The registry's `HarnessReadiness` is a probe
summary, not a chat gate: it says a binary and sign-in were seen, not that a protocol
adapter, a permission path and a process lifetime are implemented.

### 1.4 Gaps a design must close

1. No persisted choice of chat harness and no way to show it in the composer/header.
2. No lower-level adapter contract separate from `ChatTransport`, so every transport
   would otherwise be bolted into `LocalChatProvider`.
3. No gate that turns manifest/probe facts into `ready`, `missing`, `auth_required`,
   `worker_only`, `unsupported_protocol`, `not_wired` or `incompatible`.
4. No safe event vocabulary for CLI protocols (JSONL/JSON-RPC) equivalent to the
   OpenCode SSE mapper.
5. No process ownership/lifetime rules for a second kind of child process.
6. No fixture/fake-CLI strategy for protocols that are only available on the owner's
   machine.
7. No guard that prevents a custom manifest edit or delete from silently changing a
   bound session.

## 2. Scope boundary: which sessions may use a harness

This is the part that keeps the design honest.

| Session kind | Who owns the durable log | Harness-backed now? | Why |
|---|---|---|---|
| `setup` | The app (bundled engine, private workspace) | **Yes**, when the selected adapter is ready | The app creates the native session and owns its transcript. |
| `global` | VOWP (home) | **No**; stays OpenCode | The endpoint resolver requires `app: "opencode"`; a local harness cannot mirror turns into the VOWP session log. |
| `project:<name>` with a VOWP orchestrator | VOWP (home) | **No**; stays OpenCode | Same reason. The owner decision is that the session log always lives on the server. |
| `project:<name>` one-click fallback | The app, but keyed as a VOWP project session | **No** in Phases 1–4; stays OpenCode | The fallback is explicitly upgradeable to the real VOWP orchestrator. Binding it to a CLI harness would remove that upgrade path. A future explicit “local-only project chat” is a separate product decision. |
| Any future app-owned local conversation | The app | Yes, under the same contract | The adapter contract is reusable; a new session kind only needs a binding and a UI entry. |
| Home VPS target | The harness runs remotely | **No** | Chat transport is Local-only in this design; the registry can still install/probe/connect the Home VPS for worker use. |

Consequences:

- A non-OpenCode harness backs **app-owned local** conversations, not VOWP
  orchestrator sessions.
- The selection UI must say this next to the control; it must not imply that a project
  orchestrator log will move to an ACP orchestrator such as Goose or Codex.
- If the owner later wants a harness-backed project log, that is a VOWP app/durability
  question, not an adapter question. It is not silently approximated by a local
  transcript.
- `cli-pty` stays worker-only in every session kind. It needs a terminal emulator and
  screen scraping, which is a different contract (and a poor source of truth for
  chat).

## 3. Selection model

### 3.1 The two options

| Dimension | Global default | Per-session choice |
|---|---|---|
| User surface | One control in Settings → Harnesses | A picker in each new conversation, plus Settings for the default |
| New sessions | Use the current default | Use an explicit choice, falling back to the default |
| Existing sessions | Must be pinned or the default change silently rebinds them | Naturally pinned by the session record |
| Mixed needs | One harness for every eligible new chat | Different harnesses for different conversations |
| VOWP boundary | Must exclude VOWP sessions explicitly | The same exclusion is still required, but the per-session record makes it visible |
| State | One `default_harness_id` | Default plus a binding for every session; durable sessions need durable bindings |
| UI/test cost | Low: one radio group and one header chip | Higher: a picker, per-session validation, stale/missing bindings, more e2e states |
| Risk of silent migration | Real unless sessions capture their harness at creation | Low by construction |

### 3.2 Recommendation: global default, captured at session creation

Recommend the **global default** for Phase 1:

- Settings → Harnesses gets one **Chat transport** control. It applies only to new
  **app-owned local** conversations. The existing “Default harness · OpenCode”
  section is the natural place; do not add a second selector in the chat composer.
- Each session captures `harness_id` and `protocol` when it is created. Changing the
  default never changes an existing session. This is the no-silent-migration
  guarantee without a per-session picker.
- VOWP `global`/`project` sessions are always `opencode` and show a read-only
  `VOWP · OpenCode` chip. They are not candidates for the default.
- Per-session choice is deferred. It becomes necessary when there is more than one
  app-owned local conversation kind, or when VOWP can own a non-OpenCode app. Shipping
  it now would add a selector that has only one eligible session to act on.

This is still a two-level model: a global default for new sessions and an immutable
per-session capture. The capture is not a user-facing “choice”; it is a safety record.
A future job can add an explicit per-session override and a private binding file
without changing the default settings file.

### 3.3 Persistence (settings file)

Store the default in a small, versioned app settings file in the desktop app-data
directory, next to `engine.json`:

```text
<app-data>/chat.json
```

`JsonEnginePreferences` already establishes the pattern: versioned JSON, atomic staged
write plus rename, no secret field. Follow it. The host resolves the path; the
renderer never receives it.

```json
{
  "version": 1,
  "default_harness_id": "opencode"
}
```

Rules:

- `version` mismatch: do not guess. Keep the bundled setup path available, surface a
  safe warning, and leave harness chat disabled until the owner re-selects.
- Missing file: `opencode` is the legacy default. This is not a fallback from a
  selected harness; it is the documented current state.
- `default_harness_id` absent or `null`: same as `opencode`.
- `default_harness_id` names an unknown or unsupported harness: **gate**, do not fall
  back. The owner sees `harness_missing` or `harness_protocol_unsupported` and chooses
  another default explicitly.
- Corrupt file: a typed `chat_settings_unreadable` notice in Settings; no silent reset.
  The owner can open the existing OpenCode setup assistant as an explicit action while
  fixing it. Do not rewrite the file behind their back.
- The file carries no key, token, native id, endpoint, port, binary path or workspace.
  A native id is core-only and stays in memory in Phase 1. If sessions become durable
  later, a **separate** private binding file is required; do not derive the binding
  from the current default.
- Write with owner-only permissions (`0600` on Unix) and a `0700` parent where the host
  creates it. Atomic temp-plus-rename, like `JsonEnginePreferences`.

Custom-manifest guards (Phase 1, no code in this document):

- `deleteCustom(id)` returns `conflict` while `id` is the default or an active session
  is bound to it. The owner must choose another default (or close the session) first.
- `saveCustom` that changes the `protocol` of a default/bound id returns `conflict`.
  A text/label/ToS edit is harmless; a protocol edit is not. Do not silently rebind.
- A manifest that disappears is not an error at list time; a bound session gates with
  `harness_missing` and keeps its transcript readable.

### 3.4 What the composer and header show

Keep the existing read-only location chip and add one harness chip. The chip is
derived from the session record, never from a renderer toggle.

| Session | Header chips | Composer |
|---|---|---|
| Setup on OpenCode | `Local · this Mac` + `Harness · OpenCode` + state | Unchanged. Model picker/usage from the bundled engine. |
| Setup on an ACP orchestrator | `Local · this Mac` + `Harness · <name>` + state | Enabled when the gate is ready. If the adapter cannot prove a model list, the picker says “Model chosen by <name>” and is not a fake list. |
| `global`/`project` VOWP | `Local · this Mac` or `Server · name` + `VOWP · OpenCode` + ownership chip | Unchanged. No harness selector. |
| Bound harness missing | `Local · this Mac` + `Harness · <name>` + `Needs attention` | Disabled. Gate card shows Install / Connect / “Use OpenCode”. |
| Preview | Existing preview badge and copy | No harness selection; every real action is `preview_unavailable`. |

Additional rules:

- The chip's tooltip says “This conversation is bound to X. Changing the default
  affects new conversations only.” For a VOWP session it says “VOWP owns this log; the
  chat transport is OpenCode.”
- Do not add a harness dropdown to `ComposerBar`. The owner changes the default in
  Settings. A future per-session override can add a picker to the new-conversation
  flow, not to the composer of a running session.
- The model picker is honest: `available_models: None` means “this transport cannot
  report a verified list”; `[]` means “verified none”. Do not show OpenCode's models
  in another harness's session or vice versa.
- `set_model` stays gated (`chat_model_gated`) until Phase 4 proves a harness model
  switch. The model is chosen by the harness's own config/flags otherwise.
- Attachments stay hidden/disabled for a harness until the adapter proves them. A
  rejected attachment is `invalid_input`, not a silently dropped file.
- When `streaming_supported` is false, keep the existing honest line: the reply appears
  when the turn completes. Do not call a final-response render “token-by-token”.

### 3.5 When the selected harness is missing or unauthenticated

The selection is a preference, not a promise. Preflight is checked at selection time
and again at session start; a cached probe is valid for at most 60 seconds.

| State | Selection UI | New session | Existing bound session | Fallback? |
|---|---|---|---|---|
| Binary `missing` | “Use for chat” disabled; show the copy-only install command and Probe again | `harness_missing` gate; no child spawned | `harness_missing`; transcript stays readable | No. “Use OpenCode” is an explicit owner action. |
| Binary `found`, auth `missing` | Selectable; show Connect and the ToS note | `harness_auth_required` gate; no child spawned | Gate on next start; no child | No. |
| Binary `found`, auth `unknown` | Selectable; fixed “cannot prove sign-in” note | Attempt the start; if the harness reports an auth failure, map to `harness_auth_required` and stop | Same | No. |
| Version/schema mismatch | Selectable only after a probe; warning | `harness_incompatible`; no child | Gate; no retry loop | No. |
| `worker_only` or `cli-pty` | Not listed as chat-capable; a custom claim shows a warning | `harness_worker_only` or `harness_protocol_unsupported` | Same | No. |
| Adapter not implemented (Phase 1) | Selectable for planning; dry run only | `harness_not_wired`; no child | Same | No. |
| Child exits or stream dies | — | `harness_exited` or `harness_stream_lost`; state `error` | Same; Retry means the same harness | No. |
| Another writer (`access: read_only`) | — | Composer disabled; no child writer | Same | No. |

The UI always offers an explicit **Use OpenCode** action next to a gate. It writes
`opencode` as the default (or opens an OpenCode setup session) and says so. That is not
a silent fallback; it is a new owner choice.

### 3.6 How existing OpenCode sessions keep working

- With no settings file, the default is `opencode`; the setup assistant and VOWP
  sessions behave exactly as they do today.
- A session captures its harness at creation. Changing the default to an ACP
  orchestrator does not move an existing OpenCode session, recreate its native session
  or rewrite its transcript.
- VOWP `global`/`project` sessions are OpenCode by contract, not by default. They
  cannot be rebound by this feature. The endpoint resolver rejects any app other than
  `opencode` with a typed `chat_gated` error that says StackSpirit chat only speaks
  OpenCode.
- A harness switch is never implicit. If a future job adds “move this conversation to
  X”, it must be an explicit owner action, create a new native session and keep the
  old transcript readable. It must not pretend the old log was migrated.
- If the default names a harness that is later deleted or edited, the default gates;
  it does not fall back to OpenCode. The owner can choose OpenCode explicitly.

### 3.7 The first protocol: ACP, owner-selected

Owner question `01M8EYDCVXEZS92FCADETEB8F2` is answered: **B — ACP (Agent Client
Protocol) first**. OpenCode stays the default overarching harness for BYOK providers,
with no StackSpirit token billing or intermediation, and the only live embedded chat.
ACP is the first non-OpenCode family Phase 2 targets;
the reusable client has landed in `crates/spirit-core/src/harnesses/acp.rs`, but the
chat adapter does not exist yet.

The owner's surface split also settles the CLI question: Claude Code (`stream-json`)
and Gemini CLI (`acp` protocol, worker view) are **workers-only**. Their consumer OAuth
stays in their own CLI and neither is wrapped in embedded chat, regardless of protocol
family. Other orchestrator-group ACP presets remain eligible for the adapter once it is
proven.

| Family | Status after the decision | What remains |
|---|---|---|
| `opencode-http` | Live; bundled default | none for the current embedded chat |
| `acp` | Owner-selected first family; `acp.rs` client landed | real-harness probe plus `HarnessChatAdapter` wiring, streaming/permission mapping and the contract suite |
| `stream-json` | Later; Claude Code is workers-only | a custom orchestrator-group manifest would need a probe before any chat adapter |
| `codex-json` | Later; orchestrator-group preset | event schema and sandbox/permission semantics remain probe-first |

No ranking beyond the owner's choice is implied.

## 4. Adapter contract

### 4.1 Boundary and proposed trait

Do not put each protocol inside `LocalChatProvider`. Add a lower-level adapter trait
and a provider/router that implements the existing `ChatTransport` surface:

```text
SpiritBridge.chat
  -> ChatTransport (existing public core boundary)
       -> LocalChatProvider        (bundled OpenCode + VOWP, unchanged)
       -> HarnessChatProvider      (new: owns one bound adapter)
            -> Arc<dyn HarnessChatAdapter>
                 -> OpenCodeAdapter | StreamJsonAdapter | CodexJsonAdapter | AcpAdapter
```

The renderer never chooses an adapter. `HarnessChatProvider` resolves the session's
captured `harness_id`; an unknown, missing or changed harness is a typed gate.

Proposed core types (names are illustrative; no code ships in this document):

```rust
#[async_trait]
pub trait HarnessChatAdapter: Send + Sync {
    fn harness_id(&self) -> &str;
    fn protocol(&self) -> HarnessProtocol;

    /// Reads the manifest/probe/version and returns a safe gate. Never spawns.
    async fn preflight(&self) -> Result<HarnessChatGate, SpiritError>;

    /// Starts or attaches one native session. May spawn a long-lived child.
    async fn start(&self, input: HarnessStart) -> Result<HarnessSession, SpiritError>;

    /// Accepts one turn and streams protocol-neutral events.
    async fn send_turn(
        &self,
        session: &HarnessSession,
        turn: HarnessTurn,
        events: mpsc::Sender<HarnessEvent>,
    ) -> Result<HarnessAccepted, SpiritError>;

    /// Aborts the active turn and reports whether the stop is confirmed.
    async fn abort(&self, session: &HarnessSession) -> Result<HarnessAbort, SpiritError>;

    /// Reaps the child and releases the session. Idempotent.
    async fn close(&self, session: &HarnessSession) -> Result<(), SpiritError>;

    /// `None` means the harness cannot report a verified list; `Some([])` means none.
    fn models(&self) -> Option<Vec<ChatModelView>> {
        None
    }
}
```

The event vocabulary is deliberately separate from `ChatEvent` so adapters never
invent revisions or serialize protocol frames:

```rust
pub enum HarnessEvent {
    NativeSession { id: String, title: Option<String> }, // core-only
    MessageStart { message_id: String },
    TextDelta { message_id: String, part_id: Option<String>, text: String },
    TextReplace { message_id: String, part_id: Option<String>, text: String },
    Tool { activity: ChatToolActivity },
    Permission { prompt: ChatPermissionPrompt },
    Usage { usage: ChatUsageView },
    Done { message_id: String, truncated: bool },
    Error { error: SpiritError },
}
```

`HarnessChatProvider` maps these to the existing `ChatEvent` values, adds the
monotonic revision, keeps a safe in-memory transcript for `log`, and emits the
optimistic user message exactly as `LocalChatProvider` does. A `TextReplace` is
required because some protocols send cumulative text; `TextDelta` only appends. The
renderer already handles both.

`HarnessStart` / `HarnessTurn` / `HarnessSession` are core-only. They may carry the
absolute binary path, workspace, native id and process guard; none of those fields may
derive `Serialize` or a non-redacting `Debug`. `ChatSessionView` only gains safe,
optional harness metadata (id, display name, protocol, gate state).

### 4.2 Lifecycle and one writer

- `start` is idempotent for a session key. A second `start` while a child is alive
  returns the current view; it does not spawn a second child.
- Exactly one turn may be in flight per session. The adapter holds a turn lock; a
  second `send` returns `chat_busy` before it writes to stdin or spawns a child.
- Long-lived protocols (OpenCode HTTP, stream-json, ACP) keep one child per session for
  the session's lifetime. `codex-json` is expected to be one child per turn; it still
  holds the same per-session lock and resume id.
- `abort` is a turn control, never a session teardown. It clears the turn and returns
  a stopped view. If the protocol cannot confirm the stop, return `stop_unconfirmed`;
  do not pretend the turn stopped.
- `close` is idempotent and waits for the child with a bounded grace period, then
  kills it. App exit, provider drop and a last-session detach all call it.
- The provider never catches an adapter error and tries another harness. The only
  cross-harness path is the owner's explicit **Use OpenCode** action.

### 4.3 Bounded output and timeouts

Use the existing values where they exist; the proposed values are marked.

| Limit | Value | Source |
|---|---|---|
| Probe (binary/auth) | 8 s | `harnesses::probe::PROBE_TIMEOUT` (existing) |
| Start/handshake | 15 s | proposed; same scale as the sign-in allowance |
| One turn | 10 min | `chat::local::TURN_CAP` (existing) |
| Abort | 3 s, one caller retry | existing chat contract |
| Stream idle | 60 s without a byte | existing chat contract |
| HTTP/SSE reconnect | 5 attempts, 250 ms → 4 s | existing chat contract |
| Accepted HTTP body | 16 MiB | `MAX_HTTP_BODY` (existing) |
| One JSONL/JSON-RPC line | 1 MiB | proposed |
| One turn's protocol bytes | 16 MiB | proposed; reuse the HTTP cap |
| Assistant text | 32,000 chars | `MAX_MESSAGE_CHARS` (existing) |
| Tool rows | 64 per message | `MAX_ACTIVITY_ITEMS` (existing) |
| Tool/permission snippet | 160 chars | `MAX_ACTIVITY_CHARS` (existing) |
| Captured stderr | 200 lines, 1,000 chars/line | same shape as `EngineLogBuffer` |
| Concurrent harness children | 4 | proposed; typed `harness_busy` above it |

Over-limit behavior is typed, not silent: `harness_output_too_large`,
`harness_turn_timeout`, `harness_stream_lost` or a capped text with a `done.truncated`
flag. Never forward a raw oversized frame to the renderer.

### 4.4 Session and native-id handling

- `ChatSessionId` remains the only routing key the renderer sees.
- `HarnessSession.native_id: Option<String>` is core-only. `Some` after the protocol
  reports it; `None` while a one-shot protocol has not started its first turn.
- `start` may create or resume. `attach` reuses the stored native id and never
  creates. If the id is gone, return `harness_session_lost`; do not invent one.
- The binding records `harness_id` and `protocol` with the native id. Never pass a
  stream-json session id to Codex or an ACP session id to OpenCode. A changed protocol
  is `harness_protocol_changed`, not a retry.
- Phase 1 stores no native ids in `chat.json`. The current chat is in-memory per app
  run; if sessions become durable, add a separate private binding file, not a field in
  the public settings.
- Codex may report its thread id only on the first turn. The session may be
  `attached: true` with `native_id: None` until then; `attach` before the first turn is
  a typed “not started yet”, not a fabricated id.
- A native id is not a secret, but it is core-only: no `Debug`, error, event,
  `ChatSessionView` or log line carries it.

### 4.5 Model list and usage

- `models()` returns `Option<Vec<ChatModelView>>`. `None` means “cannot verify”;
  `Some(vec![])` means “verified none”. The existing UI already distinguishes these.
- Phase 1–3 do not add a harness model picker. `set_model` returns
  `chat_model_gated`; the harness owns its config/flags.
- Phase 4 adds a `ChatModelTier::Harness` value (do **not** reuse `Included` or
  `Byok`). Until then a harness model list stays `None` rather than borrowing
  OpenCode's labels.
- `ChatUsageView` is already all-optional. Map only fields the protocol reports; never
  fill missing counters with zero. `turns` increments on a confirmed `done`, and
  `cost_usd` appears only when a provider reports it.
- Context limits are reported only when the harness reports them. A harness that does
  not report usage shows “not reported”, not an empty zero meter.
- Reasoning variants/attachments are Phase 4 capabilities. A protocol that cannot
  carry them rejects with `invalid_input`/`harness_attachment_unsupported`; it does not
  drop them silently.
- The OpenCode setup assistant injects an app-owned `SETUP_SYSTEM_PROMPT`. A harness
  adapter may carry an app-owned instruction only through a proven protocol channel
  (for example a CLI `--append-system-prompt` flag). If no such channel exists, do not
  inject hidden text and do not pretend the session is the setup assistant; keep that
  session on OpenCode until the channel is proven.

### 4.6 Ownership and lease interaction

- A local harness session has one writer: StackSpirit's adapter child. Report
  `ChatAccess::Owner` and `ownership: None`, matching the bundled engine.
- A VOWP orchestrator keeps its existing rule: `ChatAccess::ReadOnly` means the
  composer is disabled and the adapter must not spawn a writer. `ChatOwnershipView`
  stays display-only.
- Never copy a VOWP lease token into the renderer, a chat DTO, an error, a log or an
  adapter argument. VOWP's own MCP server is the model: `runtime/crates/vowp-cli/src/mcp/backend.rs`
  re-reads the token file on every write and refuses a taken-over surface; its
  ownership view is token-free. If a harness session is ever mirrored to VOWP, it must
  go through that same gate, not a cached “I am the owner” boolean.
- A bound session is one writer even if the harness itself can run elsewhere. If the
  harness reports a session lock (for example, another process is resuming the same
  native id), return `harness_session_locked` (conflict); do not open a second writer.

### 4.7 Protocol map

Summary:

| Protocol | Process shape | Text input | Text output | Permission | Native id | Status |
|---|---|---|---|---|---|---|
| `opencode-http` | One bundled `opencode serve` owned by `EngineSupervisor` | HTTP `POST /session/{id}/message` | SSE + final message response | `permission.asked` + reply route | OpenCode session id | Pinned by repo docs; stream cadence owner-run |
| `stream-json` | Long-lived if streaming input is proven; otherwise one child per turn with `--resume` | NDJSON on stdin | NDJSON on stdout | Control request or policy; probe | stream-json session id; `--resume` | Claude Code is workers-only; a custom stream-json orchestrator remains probe-first |
| `codex-json` | One child per turn; resume by thread id | Prompt via stdin if supported | JSONL on stdout | Sandbox/approval policy only; probe | `thread.started.thread_id`; `exec resume` | Probe-first; no Codex binary here |
| `acp` | One long-lived JSON-RPC child per session | `session/prompt` | `session/update` notifications | `session/request_permission` | `session/new` / `session/load` session id | Owner-selected first; `acp.rs` client landed; chat adapter and real-harness probe pending |

#### `opencode-http`

Known from this repo (no probe needed for the bundled path):

- The bundled sidecar is pinned at `1.19.5` and started by the existing
  `EngineSupervisor`; chat must not spawn a second `opencode serve`.
- `GET /global/health`, `POST /session`, `GET /session/{id}/message`,
  `POST /session/{id}/message` (with `messageID` beginning `msg`), `POST
  /session/{id}/abort`, `GET /session/status`, `GET /permission`, `POST
  /permission/{id}/reply`, and `GET /event` SSE are recorded in
  `docs/ORCHESTRATOR-CHAT.md` §2.2.
- The v1 event mapping is in `crates/spirit-core/src/engine/chat/pump.rs` and
  `sse.rs`: `message.updated`, `message.part.updated` (cumulative text),
  `message.part.delta` (append), `session.idle`, `session.status`,
  `permission.asked`/`permission.replied`, `session.error`.
- Models come from `/config/providers`, falling back to `/provider` filtered by its
  `connected` list; usage comes from the native message log.

In this design, the registry `opencode` preset is **not** used to launch a PATH binary.
The existing bundled provider remains the `opencode-http` adapter, which keeps the
one-engine rule in `docs/ORCHESTRATOR-CHAT.md`: chat reuses the single supervised
sidecar instead of starting a second `opencode serve`. The registry still installs and
signs in the PATH binary for future use. Making an external OpenCode installation back
chat is a separate decision because it would replace the bundled engine, not add a
second one.

Needs a probe/owner run:

- The runtime cadence/ordering of `message.part.updated`, `message.part.delta` and
  `session.idle` during a real model turn; the production path already reports
  `streaming_supported: false` for this reason.
- Whether the user message with the sent `messageID` is preserved in the read log.
- Any external OpenCode binary would need `GET /global/health` version/schema gating;
  it is out of scope until the one-engine rule is revisited.

#### `stream-json`

Claude Code is `workers_only_terminal` after the `ss-179` split, so no built-in chat
adapter targets this subsection. The mechanics stay for a future custom
`orchestrator_and_workers` stream-json manifest; Phase 2 does not implement them.

Known from the local `claude --help` (`2.1.319` on the design host; not the bundled
engine):

- `-p/--print`, `--input-format stream-json`, `--output-format stream-json`,
  `--include-partial-messages`, `--resume <id>`, `--session-id <uuid>`, `--model`,
  `--permission-mode` and `--permission-prompts`, `--append-system-prompt`, `--effort`.
- The intended shape is a long-lived print-mode child with streaming input. If the
  probe shows print mode is one-shot, use one child per turn and `--resume`; the
  adapter contract supports both.
- A session id can be supplied or resumed, so the adapter can keep the native id
  core-only.

Needs a probe (do not ship from the flag list alone):

- The exact NDJSON input line shape and the output event names/fields (`system` init,
  `assistant`, `user` tool result, `stream_event` deltas, `result` usage/cost).
- The permission path: whether a `control_request`/`control_response` (or
  `--permission-prompt-tool`) can pause a turn and return an owner decision. Until it
  is proven, a stream-json adapter must run a mode that cannot prompt (for example
  `plan`) or must refuse tools that require approval. Never auto-approve.
- Abort semantics: an interrupt control request if available; otherwise terminate the
  child, wait for exit, and report a confirmed local stop. The next turn resumes with
  `--resume` only if the probe proves it.
- Output/usage/tool-block shapes, streaming cadence, and what happens on auth expiry.
- Whether `--append-system-prompt` carries the setup assistant's app-owned prompt; if
  not, do not pretend the setup assistant is identical.

#### `codex-json`

Not installed in this workspace. Treat every shape as probe-first:

- The registry value `codex-json` means “Codex CLI's JSON output”. The public CLI
  family is `codex exec --json` (one non-interactive turn) and
  `codex exec resume <id> --json` (continue a thread), with `--model`, `--cd`,
  `--sandbox`/approval flags. None of that is pinned by a binary or fixture here.
- Needs a probe: the exact JSONL event names and fields (`thread.started`,
  `turn.started`, item events, `turn.completed` usage), whether the prompt can be sent
  on stdin without appearing in argv, the session id exposure, streaming cadence,
  usage/cost fields and the error envelope.
- Permissions are a sandbox/approval policy, not an interactive allow/deny prompt.
  If a command is blocked, map the item/turn error to a tool/error row; do not invent
  a permission card and do not run with full access by default.
- Abort is the child process: kill it, wait for exit, and report a confirmed local
  stop. A one-shot process has no long-lived control channel.
- Models are `None` unless a verified list command appears in the probe. A `--model`
  string is not a picker.

#### `acp`

Not installed in this workspace; the orchestrator-group ACP presets (for example Goose
and Cursor CLI) share the protocol value but not the launch command. Gemini CLI also
speaks ACP but is workers-only under the owner's split, so it is not a chat target. The
ACP family is JSON-RPC 2.0 over stdio; the local `vowp mcp` server
(`runtime/crates/vowp-cli/src/mcp/jsonrpc.rs`) proves a newline-delimited JSON-RPC
request/notification loop for a **different** protocol, so use it only as a framing
precedent.

`ss-179` landed a reusable client at `crates/spirit-core/src/harnesses/acp.rs`:
newline-delimited JSON-RPC 2.0 with `protocolVersion: 1`, `initialize`, `session/new`,
`session/prompt`, streaming `session/update`, `session/request_permission` and
`session/cancel`, with no `fs/*`/`terminal/*` client capabilities. It is not a
`HarnessChatAdapter`, it is not selected by chat and the chat path does not spawn it.
The remaining real-harness and adapter work is:

- The exact framing accepted by each CLI, the ACP protocol version it negotiates, and
  its start command. Do not guess a binary flag from the registry name.
- Real `initialize` capabilities/auth methods, `session/new` (cwd, MCP servers,
  models), `session/load`, and the `session/update` variants that carry
  text/tool/plan/usage.
- `session/request_permission` option ids and how `allow_once`, `allow_always`,
  `reject_once` and `reject_always` map to `ChatPermissionDecision`.
- Whether the harness requires client `fs/*` or `terminal/*` callbacks. Do **not**
  advertise those capabilities in Phase 2; a chat adapter is not a terminal or file
  proxy. If the harness cannot work without them, gate it.
- Model list/usage fields in the `session/new` or prompt response; if absent, report
  `None`.
- Abort: send `session/cancel`, await a cancelled prompt response within 3 s, then
  kill the child and report `stop_unconfirmed` if the response never arrives.

### 4.8 `cli-pty` stays worker-only

`cli-pty` is an interactive terminal, not a structured stream. Chat would have to
emulate a terminal, scrape ANSI output and guess when a turn ends. That is a different
product and a worse safety story. Keep it worker-only:

- The selection list excludes every `workers_only_terminal` manifest and every
  `cli-pty` manifest.
- A stale or hand-edited manifest that claims `protocol = "cli-pty"` with
  `orchestrator_and_workers` is rejected by `validate()` at save/load time; if one
  reaches the gate anyway, the gate rejects it with `harness_protocol_unsupported`.
- No terminal emulator, pty or screen scraper is added to the chat wheel.

### 4.9 Fixture and version policy

- Every adapter fixture records the CLI/product version it was recorded against.
- `preflight` reads the harness's safe version (`--version` or an equivalent health
  document) and compares it with the fixture's supported range.
- A different version is not automatically “probably fine”. Run a schema probe; until
  it passes, return `harness_incompatible` with a safe detected/required message. The
  fixture can be updated in a small follow-up job.
- An adapter never silently accepts unknown event fields by treating them as text. The
  parser must ignore unknown fields, but a missing required field is a typed protocol
  error.

## 5. Safety and honesty

### 5.1 ToS note at selection

The registry already has the right copy. The chat selector must show the manifest's
`tos_note` and auth-mode chips before the owner chooses:

- **API key (BYOK):** the provider's API billing and terms apply.
- **Subscription sign-in:** the vendor's consumer terms apply, and third-party or
  automated use may be restricted. Claude Pro/Max, ChatGPT, Google and Cursor presets
  say so explicitly.

The selector does not accept terms on the owner's behalf, does not choose an auth mode
and does not convert a subscription login into an API key. The same `tos_note` string
is used verbatim; no new legal copy is invented. The probe's fixed subscription
warning is shown alongside it when applicable.

### 5.2 No secret values in argv, logs or the renderer

- Keys stay in the harness's own credential store, the local OS keychain or VOWP's
  `vowp secret` vault. The chat selection settings file has no credential field.
- Spawn with an absolute binary path and an argv array; never a shell, `sh -c`, a
  command string or an interpolated provider key. User text and attachments go over
  stdin (or a `0600` temp file that is deleted immediately) so they do not appear in
  `ps`.
- The child environment is an allowlist. StackSpirit does not inject a provider key
  into a non-OpenCode harness. If a future adapter must use an environment variable,
  it names it, keeps the value out of `Debug` and logs, and never reflects it.
- Capture stdout/stderr through a bounded ring buffer. Redact with
  `crate::error::redact`, cap each line and never forward raw protocol JSON to the
  renderer.
- Errors carry a stable prefix, a safe message and a hint. They never include a URL,
  port, credential, native id, binary path, workspace, raw command line or provider
  response body.
- Tests plant a fake key in argv/env/stdout and assert it is absent from `Debug`, every
  serialized view, every event and every error.

### 5.3 Typed errors

Use the existing `SpiritErrorCode` values and add stable message prefixes. The
renderer matches prefixes/codes; do not parse prose.

| Prefix | Code | When |
|---|---|---|
| `harness_missing` | `not_found` | The selected/bound binary is not on the target |
| `harness_auth_required` | `auth` | The harness reports no sign-in or rejects the credential |
| `harness_worker_only` | `gate` | `surface = workers_only_terminal` |
| `harness_protocol_unsupported` | `gate` | `cli-pty` or an unknown protocol for chat |
| `harness_not_wired` | `gate` | Phase 1 dry run only; no adapter yet |
| `harness_incompatible` | `gate` | Version/schema is not the pinned one |
| `chat_settings_unreadable` | `parse` | `chat.json` is corrupt or a future version |
| `harness_spawn_failed` | `engine` | The child could not start |
| `harness_exited` | `engine` | The child exited mid-turn |
| `harness_protocol_error` | `parse` | A required field/line is missing or malformed |
| `harness_output_too_large` | `parse` | A line/turn exceeds the cap |
| `harness_turn_timeout` | `remote_timeout` | The 10-minute turn cap fired |
| `harness_stream_lost` | `remote_timeout` | The stream went idle/ended before done |
| `harness_session_lost` | `not_found` | The native session is gone |
| `harness_session_locked` | `conflict` | Another process owns the same native session |
| `harness_read_only` | `conflict` | `access: read_only`; no writer |
| `harness_busy` | `conflict` | The concurrent harness-children cap is reached |
| `harness_permission_unavailable` | `gate` | The protocol cannot ask the owner safely |
| `harness_attachment_unsupported` | `invalid_input` | The protocol cannot carry the attachment |

Keep the existing `chat_*` errors unchanged. Update the UI's gate detector
(`isGateError` in `ChatView.tsx`) to recognize the new `harness_*` prefixes instead of
showing them as generic errors. A new `HarnessChatGate` enum should mirror the gate
style:

```text
Unknown | Probing | Ready { harness_id, protocol, version, evidence }
| Missing { harness_id } | AuthRequired { harness_id, detail }
| WorkerOnly { harness_id } | UnsupportedProtocol { harness_id, protocol }
| NotWired { harness_id, protocol } | Incompatible { harness_id, detected, required }
| Unreachable { harness_id, detail }
```

It is safe to serialize: it carries the harness id, protocol, version and fixed
copy/evidence labels, never a path, URL, port, token or native id.

### 5.4 No fallback and no preview replies

- The adapter router never catches an error and tries a different harness. There is no
  “try one harness, then another, then OpenCode” chain.
- The only OpenCode switch is an explicit owner button. It starts a new OpenCode
  conversation or changes the default and says that it did.
- A real turn never constructs `ChatMessageOrigin::Preview`. Preview selection calls
  return `preview_unavailable`; `MockBridge` may list built-ins but cannot save a
  harness choice or start a harness child.
- An empty turn with no text and no error is a `done` with an empty message id; it is
  never a canned reply.
- A gate is a gate: the transcript area shows a typed card, not a fake assistant
  paragraph.

### 5.5 Cooperative ownership: one writer

- One adapter child per session; one turn at a time; a turn lock around `send`.
- `read_only` disables the composer and refuses to spawn a writer.
- `start` and `attach` never create a second native session while one is attached.
- VOWP's lease token stays core-only and is re-read on every write by the layer that
  owns it. The adapter never caches it, logs it or passes it to the harness.
- A harness that reports its own lock (`harness_session_locked`) is a conflict, not a
  reason to bypass the lock.
- The app can run the setup assistant and a VOWP orchestrator at the same time only
  because they are different sessions/writers; the same session is never double-opened.

### 5.6 Process lifetime and cleanup

- `HarnessSession` owns a child guard. Drop kills the child and (on Unix) its process
  group; Windows uses a job object where available. Tests must prove no orphan.
- App exit, provider drop, session close and a failed start all reap.
- Long-lived children are closed when the last session on that adapter detaches;
  one-shot children exit after `done` or are killed on abort/timeout.
- A crashed child never leaves a half-open session. State becomes `error` with
  `harness_exited`; Retry starts the same harness, not a different one.
- The process cap (4 concurrent harness children) is global to the app and typed
  `harness_busy`; it protects the machine from a pathological loop.

## 6. Phased delivery

Each phase is a separate job or two small jobs. The names below are suggestions, not
current code. A phase must not claim a transport is live until its acceptance test and
the owner-run check pass.

| Phase | Scope | One-line acceptance test | Suggested `owns` |
|---|---|---|---|
| **1** | Selection + gates + transport-less dry run | “A fresh profile shows OpenCode selected and the existing setup chat still runs on the bundled engine; selecting any non-OpenCode chat harness returns `harness_not_wired` and starts no process; missing/worker-only harnesses are never selectable.” | Core selection/gate + desktop command + bridge + Settings/ChatView UI + tests (split 1a core, 1b UI if needed) |
| **2** | First adapter: ACP (owner decision) | “A new app-owned local conversation on the selected harness completes one real turn through the adapter with safe text/tool/permission/usage/done events, and no other harness process is started.” | ACP adapter mapping + contract/fixture tests + `apps/spirit-desktop/src-tauri/src/state.rs` wiring |
| **3** | Next adapter | “The second transport passes the same `HarnessChatAdapter` conformance suite without changing the provider, the public `ChatTransport` surface or the UI.” | Second adapter file + fixtures + shared contract-test parameter |
| **4** | Model/usage parity | “A harness that reports models and usage fills the picker and context meter with real values; one that does not reports `None`/‘not reported’, with no zero-filled counters.” | Model/usage mapping, `ChatModelTier::Harness`, `ComposerBar`/`ContextMeter`, bridge DTOs + tests |

### Phase 1 — selection, gates, dry run

Deliver:

- `chat.json` settings (read/write, atomic, versioned) and a `ChatPreferencesStore`
  boundary like `EnginePreferencesStore`.
- `HarnessChatGate` and `preflight`/`dry_run`; no `HarnessChatAdapter` start path is
  enabled. A non-OpenCode selection returns `harness_not_wired`.
- Settings → Harnesses “Chat transport” control with eligibility, ToS note and probe
  status. A header chip and a gate card in `ChatView`; no composer dropdown.
- `HarnessChatProvider`/router skeleton that resolves the captured harness and routes
  `opencode` to the existing `LocalChatProvider`. Other ids return the gate.
- Tests for selection persistence, eligibility, corrupt/unknown settings, custom
  manifest guards, preview, and “no child spawned”.

Suggested `owns`:

```text
crates/spirit-core/src/engine/chat/selection.rs            (new)
crates/spirit-core/src/engine/chat/mod.rs
crates/spirit-core/src/engine/chat/harness.rs              (new, gate/types only)
crates/spirit-core/src/harnesses/manifest.rs               (eligibility helper)
crates/spirit-core/src/harnesses/store.rs                  (default/bound guard)
crates/spirit-core/tests/harness_chat_selection.rs         (new)
apps/spirit-desktop/src-tauri/src/state.rs
apps/spirit-desktop/src-tauri/src/commands/chat.rs
apps/spirit-desktop/src/bridge/TauriBridge.ts
packages/spirit-ui/src/bridge/SpiritBridge.ts
packages/spirit-ui/src/bridge/wire.ts
packages/spirit-ui/src/bridge/MockBridge.ts
packages/spirit-ui/src/next/HarnessesPanel.tsx
packages/spirit-ui/src/next/ChatView.tsx
packages/spirit-ui/src/test/next/harnessChatSelection.test.tsx (new)
```

If this is too large for one job, split at the bridge: 1a core selection/gate/dry run;
1b desktop bridge + UI + e2e. The acceptance test stays the same.

### Phase 2 — the first adapter: ACP (owner decision)

The owner selected **B — ACP (Agent Client Protocol) first**. `ss-179` already landed
the reusable stdio client in `crates/spirit-core/src/harnesses/acp.rs`: newline-
delimited JSON-RPC 2.0 with `protocolVersion: 1`, `initialize`, `session/new`,
`session/prompt`, streaming `session/update`, `session/request_permission` and
`session/cancel`; no `fs/*`/`terminal/*` client capabilities; a 1 MiB line cap, a 30 s
request/read deadline and a 10-minute prompt deadline; and deterministic mock-process
tests through `AcpClient::from_io`.

That is protocol plumbing, not a chat transport. `acp.rs` is **not** a
`HarnessChatAdapter`, is not selected by a chat session, is not called by
`LocalChatProvider`, and does not map ACP updates to `HarnessEvent`/`ChatEvent`. Phase 2
still has to deliver:

- a `HarnessChatAdapter` for an `orchestrator_and_workers` ACP harness, with the safe
  `preflight` gate and child lifetime/ownership owned by chat;
- the `session/update` and permission mapping into `HarnessEvent`, including
  text/tool/permission/usage/done, without forwarding raw JSON to the renderer;
- the fake-harness contract suite and fixtures from §7, with the real owner-run check
  still pending;
- `HarnessChatProvider`/router wiring so a session bound to the ACP harness reaches
  that adapter; OpenCode remains the only live path until this lands.

A successful ACP turn is not claimed until the owner-run check passes. Worker-only
harnesses, including Claude Code and Gemini CLI, stay in the terminal worker view and
are never wrapped in embedded chat.

Target: the first adapter must back an app-owned local conversation. Prefer the setup
assistant only if ACP proves a channel for the app-owned `SETUP_SYSTEM_PROMPT`;
otherwise add a minimal app-owned "Local chat" entry in a companion job. Never drop the
setup prompt silently and never label a generic harness chat as the setup assistant.

Common: the fake CLI, fixtures, the contract suite and the real owner-run check. No
VOWP session changes, no `cli-pty`.

Suggested `owns`:

```text
crates/spirit-core/src/harnesses/acp.rs                     (client; extend only if the probe requires)
crates/spirit-core/src/engine/chat/acp.rs                   (new chat adapter mapping)
crates/spirit-core/src/engine/chat/harness.rs               (adapter wiring/router)
crates/spirit-core/tests/harness_chat_contract.rs           (new)
crates/spirit-core/tests/fixtures/harness-chat/acp/         (new)
crates/spirit-core/examples/fake-harness.rs                 (new, hermetic)
apps/spirit-desktop/src-tauri/src/state.rs
apps/spirit-desktop/src-tauri/src/commands/chat.rs
packages/spirit-ui/src/next/ChatView.tsx                    (gate copy/chip only)
```

### Phase 3 — the next adapter

Repeat the Phase 2 shape with the next owner choice. The acceptance test is the shared
contract suite plus one adapter file and fixtures; the provider, `ChatTransport` and
UI must not need a rewrite.

Suggested `owns`:

```text
crates/spirit-core/src/engine/chat/<protocol>.rs            (new)
crates/spirit-core/tests/fixtures/harness-chat/<protocol>/  (new)
crates/spirit-core/tests/harness_chat_contract.rs           (parameter only)
```

### Phase 4 — model/usage parity

Deliver:

- `ChatModelTier::Harness` and verified model lists for adapters that can report them.
- Usage folding (`input`, `output`, cache, context, cost, turns) from real protocol
  fields; no zero fill.
- Reasoning variants, attachments and `set_model` only where the adapter proves them;
  otherwise keep the typed gate.
- Optional durable session bindings if chat sessions become durable; separate private
  file, never the public settings.

Suggested `owns`:

```text
crates/spirit-core/src/engine/chat/model.rs                 (new tier)
crates/spirit-core/src/engine/chat/harness.rs               (usage/model mapping)
crates/spirit-core/src/engine/chat/<protocol>.rs            (verified fields)
packages/spirit-ui/src/bridge/wire.ts
packages/spirit-ui/src/next/ComposerBar.tsx
packages/spirit-ui/src/next/ContextMeter.tsx
packages/spirit-ui/src/test/next/harnessChatModels.test.tsx (new)
```

## 7. Test strategy

### 7.1 The shared contract suite

One test module, `crates/spirit-core/tests/harness_chat_contract.rs`, runs against
every adapter (Phase 2 onward):

- `start` → `send_turn` → text events → `done`; no duplicate message ids.
- `TextDelta` appends; `TextReplace` replaces; a non-prefix cumulative update becomes
  a replace, not double text.
- Tool rows are redacted and capped; raw tool input/output/error never crosses.
- A permission request pauses the turn; allow/reject is the only path; nothing
  auto-approves; a protocol that cannot ask returns `harness_permission_unavailable`.
- `abort` returns stopped or `stop_unconfirmed`; a late frame from the aborted turn is
  dropped.
- A child crash → `harness_exited`; a malformed line → `harness_protocol_error`; an
  oversized line/turn → `harness_output_too_large`; a 10-minute turn →
  `harness_turn_timeout`.
- A second `send` while streaming → `chat_busy`; a second `start` does not spawn.
- `close`/drop reaps the child; no orphan; `log` replays the safe transcript.
- No secret in `Debug`, serialized views, events, errors or captured stderr.
- An unknown harness/protocol/version gates before any child is spawned.
- A real turn never produces `ChatMessageOrigin::Preview`.

The same suite is parameterized by a small `AdapterFixture` so Phase 3 cannot weaken
the contract to fit a new protocol.

### 7.2 Fixture-based protocol tests

Fixtures live under `crates/spirit-core/tests/fixtures/harness-chat/<protocol>/` and
are recorded text, not live calls:

- `opencode-http`: reuse the existing fake-engine/SSE fixtures; add a fixture for a
  permission prompt, a retry status and a malformed event.
- `stream-json`: init, assistant text, tool use/result, partial deltas, result usage,
  auth failure, malformed line, huge line.
- `codex-json`: `thread.started`, turn start, item events, `turn.completed` usage,
  failure, malformed line.
- `acp`: initialize, session/new, text chunk, tool call/update, request_permission
  (all four options), prompt response with usage, cancel, JSON-RPC error.

Fixtures use placeholders, never a real native id, path, host or credential. A fixture
records the product version and the date it was recorded.

### 7.3 The hermetic fake CLI

Use a small example binary (`crates/spirit-core/examples/fake-harness.rs`, proposed)
instead of a test that calls a real `claude`/`codex`/`gemini`/`goose`/`opencode`:

- It accepts `--protocol <name> --fixture <path>` and speaks that protocol on
  stdin/stdout, driven by a fixture script.
- It can simulate slow output, a permission request, a crash, an oversized line, an
  auth error and a cancellation.
- Tests create a temp `HOME`/`XDG_CONFIG_HOME`, prepend a temp directory containing a
  shim with the harness's exact binary name, and never touch the real user store or
  the network.
- The same fake runs the CLI-side test of the adapter schema before a real harness is
  involved.
- No test invokes tmux or a real pty; `cli-pty` is refused by the gate.

### 7.4 Desktop and UI tests

- Selecting a default persists; reload shows it.
- Only eligible harnesses appear; worker-only and `cli-pty` claims are excluded/warned.
- The ToS note and auth-mode chips are visible before selection.
- A missing/auth-required selection shows the gate card and a disabled composer; no
  process is spawned.
- Changing the default does not change an existing bound session's header tooltip.
- A VOWP session shows `VOWP · OpenCode` and no selector.
- Preview returns `preview_unavailable` for save/start and never renders a real reply.
- Axe/WCAG in light and dark for the new control and gate card.
- A secret-planting test asserts no key/path/native id appears in the DOM or a bridge
  payload.

### 7.5 Real harness checks (owner-run)

These cannot run in CI and must not use a paid model automatically:

| Check | Why it needs the real harness |
|---|---|
| Install/auth state and the exact `--version` | The binary and sign-in are owner-specific |
| One real turn per chosen protocol | Event schema, ordering and final shape are not proven by fixtures alone |
| Permission request allow/reject | The control channel and option ids are vendor-specific |
| Abort a long turn | Confirms the process/control path, not just a fake |
| Resume after app restart | Native-id persistence is protocol-specific |
| Model list and usage/cost | The harness reports its own values; no invented defaults |
| Subscription sign-in behavior | Consumer terms and vendor enforcement are outside the app |
| Process cleanup on macOS/Windows | Process-tree behavior is platform-specific |
| No secret in argv/logs/DOM | An end-to-end scan with a fake planted value |

The existing `scripts/verify-orchestrator-chat.mjs` remains the owner checklist for
OpenCode/streaming. A future harness-chat script should print the same kind of fixed
checklist and must not make a real turn on its own.

### 7.6 CI rules

- No network, no real harness binary, no real `~/.stackspirit`, `~/.vowp` or keychain.
- No paid model call, no subscription login, no OAuth callback.
- Fixtures are tracked text; fake binaries are generated in the temp dir.
- `node scripts/check-doc-links.mjs` and `bash scripts/check-public-readiness.sh` stay
  green; a new doc must not add a secret-shaped string, a global IP or a personal home
  path.

## 8. What this design deliberately does not do

- It does not implement or claim a harness chat transport. ACP is the owner-chosen
  first family, and the reusable `acp.rs` client has landed, but no chat path is wired
  to it and no non-OpenCode harness conversation can start.
- It does not change `LocalChatProvider`, the bundled engine, the Tauri bridge or the
  chat UI. The `ss-179` registry `HarnessSurface` split and `HarnessesPanel` grouping
  landed separately; this document does not reopen them.
- It does not make VOWP `global`/`project` logs harness-agnostic; that is a VOWP app
  decision.
- It does not add Home VPS chat transport or an SSH stdio forward.
- It does not enable `cli-pty` chat or add a terminal emulator.
- It does not add a provider-key store; keys remain in the harness's own store, the OS
  keychain or the VOWP vault.
- It does not add a per-session picker in Phase 1; the global default plus the captured
  binding is the smaller, safer first step.
- It does not promise model/usage/attachments/reasoning parity before Phase 4; those
  report `None`/gated until proven.

## 9. References

- [`docs/HARNESSES.md`](HARNESSES.md) — registry, protocols, probe, connect and the
  current honesty note.
- [`docs/ORCHESTRATOR-CHAT.md`](ORCHESTRATOR-CHAT.md) — the pinned OpenCode v1
  contract, SSE mapping, ownership and error rules.
- [`docs/HANDOFF-DESKTOP-UI.md`](HANDOFF-DESKTOP-UI.md) — owner decisions (the session
  log lives on the server), §5 chat status, §6 streaming/owner-run caveats.
- [`docs/SERVER-CHAT-OPTIONS.md`](SERVER-CHAT-OPTIONS.md) — the VOWP endpoint and
  remote-transport boundary this design does not reopen.
- [`SECURITY.md`](../SECURITY.md) — keys, redaction and the VOWP secret path.
- `crates/spirit-core/src/engine/chat/` — `local.rs`, `model.rs`, `pump.rs`,
  `endpoint.rs`, `sse.rs`.
- `crates/spirit-core/src/harnesses/` — `manifest.rs`, `probe.rs`, `store.rs`,
  `toml.rs`, `acp.rs` (reusable ACP stdio client; not a chat adapter).
- `crates/spirit-core/src/engine/gate.rs` — `LocalOrchestratorGate`, `AsifGate`,
  `GateSnapshot`.
- `runtime/crates/vowp-cli/src/mcp/` — JSON-RPC stdio framing (`jsonrpc.rs`) and the
  lease gate/ownership pattern (`backend.rs`); a precedent, not an ACP implementation.
