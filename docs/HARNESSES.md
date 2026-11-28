# Harnesses

The Universal Harness Registry describes the coding harnesses StackSpirit can launch on
this Mac or the connected Home VPS: how to install one, how to sign in, whether it can
back the embedded chat or only run as a stock CLI worker, and what the owner should know
about API-key versus subscription terms.

The registry has one explicit capability split, `HarnessSurface`:

| Wire value | Meaning |
|---|---|
| `orchestrator_and_workers` | OpenCode HTTP or an official ACP / Codex app-server harness that can back the embedded chat and run workers. |
| `workers_only_terminal` | A stock CLI shown in the terminal worker view. StackSpirit never wraps its consumer OAuth sign-in in the embedded chat. |

`HarnessManifest.surface` is authoritative. `supports_embedded_chat` remains a legacy
mirror (`true` exactly for `orchestrator_and_workers`) so older bridge payloads stay
readable, and validation rejects a manifest where the two disagree. A `cli-pty`
protocol must also be `workers_only_terminal`; an orchestrator cannot be a terminal
screen-scraper.

It is reachable at **Settings → Harnesses** in the desktop app and is backed by
`spirit-core`. It is real, not a mock: the desktop host reads the registry, probes both
targets, and owns the one-click sign-in child processes. The panel is desktop-only for
connect. Preview mode lists the built-in presets and refuses every probe, change and
connect with a typed `preview_unavailable` error; the read-only Web bridge has no harness
surface at all.

## Where to find it (desktop Settings)

Settings is a centered modal over the shell, not a separate page: Esc, the close button
or a backdrop click returns to where you were. Its left rail has five items:
**General** (theme, appearance and workspace defaults), **Account**,
**Server & Machines**, **Harnesses** and **CLI & MCP Connect**. The harness registry is
the **Harnesses** item. The MCP server itself is documented in [`MCP.md`](MCP.md); this
section only maps the desktop controls.

**Harnesses** pins OpenCode at the top with its collapsible fixed-height Providers
dropdown, then lists the remaining built-ins and valid custom manifests in two sections:

- **Orchestrator + Workers (ACP / Embedded Chat)**
- **Workers Only (Stock CLI Terminal View)**

Each card carries the protocol, the surface badge, the **Embedded Chat** or
**Worker-Only** capability badge, the harness's API-key or subscription chips, and its
ToS note. The panel probes **Local · this Mac** and the connected Home and offers the two
Connect buttons for those targets. Connect installs the harness and starts its own
sign-in; the embedded chat still runs on OpenCode, and choosing a non-OpenCode chat
transport is not live yet.

The **+ Add custom harness** form picks the surface first. **Orchestrator + Workers**
defaults the protocol to `acp`; **Workers Only** forces `cli-pty`. Changing the protocol
updates the surface the same way, so a custom manifest cannot claim a terminal protocol
as an embedded-chat orchestrator.

**CLI & MCP Connect** switches between **This Mac** and the connected Home and shows two
copy-only blocks: the CLI install command (`npm install -g stackspirit`) and the local
stdio MCP JSON entry. The snippet contains no token, key, password or SSH argument; the
CLI resolves the saved Home through the desktop host. The same panel offers the built-in
external CLI sign-ins. See [`MCP.md`](MCP.md) for the server, its tools and client setup.

**Server & Machines** is where a Home server is added, re-pinned or removed, an
Always-On Machine is connected through the optional encrypted relay, and server secrets
and password login are managed. See [`HANDOFF-DESKTOP-UI.md`](HANDOFF-DESKTOP-UI.md) §3
for the SSH and server-secret model.

**Account** is the optional StackSpirit sign-in: it syncs Always-On Homes and opens the
Managed Homes catalog. Local and SSH servers need no account. See
[`STRATEGY.md`](STRATEGY.md) §4.

## Where it lives

| What | Where |
|---|---|
| Panel | `packages/spirit-ui/src/next/HarnessesPanel.tsx`; mounted by `packages/spirit-ui/src/next/SettingsView.tsx` |
| Bridge contract | `packages/spirit-ui/src/bridge/SpiritBridge.ts` (`HarnessesSurface`, `SpiritBridge.harnesses`) and `packages/spirit-ui/src/bridge/wire.ts` (DTOs) |
| Desktop adapter | `apps/spirit-desktop/src/bridge/TauriBridge.ts` (`harnesses`) |
| Tauri commands | `apps/spirit-desktop/src-tauri/src/harnesses.rs` (`harnesses_list`, `harnesses_save_custom`, `harnesses_delete_custom`, `harnesses_probe`, `harnesses_connect_start`) |
| Core registry | `crates/spirit-core/src/harnesses/` (`manifest.rs`, `probe.rs`, `store.rs`, `toml.rs`, `acp.rs`) |
| Tests | `packages/spirit-ui/src/test/next/harnessesPanel.test.tsx`, `packages/spirit-ui/src/test/next/harnessSettings.test.tsx` |
| Preview | `MockBridge.harnesses` lists the built-in presets; every other call fails with `preview_unavailable` |

The bridge namespace is optional: presence is the capability. It exposes `list`,
`saveCustom`, `deleteCustom`, `probe` and `connectStart`.

## Built-in presets

Built-ins ship in code and are read-only. OpenCode is first, is the documented default
(`DEFAULT_HARNESS_ID`), and stays pinned at the top of the panel. A custom file may not
reuse a built-in id.

**Orchestrator + Workers (ACP / Embedded Chat)**

| id | Name | Binary | Protocol | Auth modes |
|---|---|---|---|---|
| `opencode` | OpenCode | `opencode` | `opencode-http` | API key |
| `cursor-cli` | Cursor CLI | `agent` | `acp` | subscription |
| `github-copilot` | GitHub Copilot | `copilot` | `acp` | subscription |
| `auggie` | Auggie | `auggie` | `acp` | subscription |
| `kiro` | Kiro | `kiro-cli` | `acp` | subscription |
| `goose` | Goose | `goose` | `acp` | API key |
| `codex` | Codex | `codex` | `codex-json` | API key, subscription |
| `cline` | Cline | `cline` | `acp` | API key |
| `kilo` | Kilo Code | `kilocode` | `acp` | API key |
| `qwen-code` | Qwen Code | `qwen` | `acp` | API key, subscription |
| `mistral-vibe` | Mistral Vibe | `vibe` | `acp` | API key |
| `kimi-code` | Kimi Code | `kimi` | `acp` | subscription |
| `minimax-code` | MiniMax Code | `minimax` | `acp` | API key |
| `glm-agent` | GLM Agent | `glm` | `acp` | API key |
| `pi-acp` | Pi ACP | `pi` | `acp` | API key |
| `devin-cli` | Devin CLI | `devin` | `acp` | subscription |
| `factory-droid` | Factory Droid | `droid` | `acp` | subscription |
| `grok-build` | Grok Build | `grok` | `acp` | subscription |
| `junie` | Junie | `junie` | `acp` | subscription |

**Workers Only (Stock CLI Terminal View)**

| id | Name | Binary | Protocol | Auth modes |
|---|---|---|---|---|
| `claude-code` | Claude Code | `claude` | `stream-json` | API key, subscription |
| `gemini-cli` | Gemini CLI | `gemini` | `acp` | API key, subscription |
| `antigravity` | Antigravity | `antigravity` | `cli-pty` | subscription |
| `aider` | Aider | `aider` | `cli-pty` | API key |
| `crush` | Crush | `crush` | `cli-pty` | API key |
| `amp` | Amp | `amp` | `cli-pty` | subscription |

`surface` records whether a preset is an orchestrator/worker backend or a stock CLI.
The panel labels the second group **Workers only** / **Worker-Only** and does not invent
a chat transport for it. Claude Code and Gemini CLI keep their structured worker
protocols (`stream-json`, `acp`) but are deliberately not embedded-chat surfaces because
their consumer OAuth must stay in their own CLI.

## Protocols

| Wire value | Meaning |
|---|---|
| `opencode-http` | OpenCode's local HTTP server |
| `acp` | Agent Client Protocol over stdio |
| `stream-json` | Claude Code's stream-json output |
| `codex-json` | Codex CLI's JSON output; the embedded transport uses `codex app-server` |
| `cli-pty` | An interactive CLI owned through a pseudo-terminal |

The protocol is a manifest fact and is copied into every probe result, together with the
surface, so a stale renderer cannot relabel a harness.

## API key versus subscription terms

`auth_modes` accepts `api_key`, `subscription` or both. `tos_note` is the
plain-language distinction the panel shows on every card:

- **API key (BYOK)**: the provider's API billing and terms apply. StackSpirit never
  assumes a consumer subscription is the same product.
- **Subscription sign-in**: the vendor's consumer terms apply, and third-party or
  automated use may be restricted. This is why the preset notes call it out explicitly
  for Claude Pro/Max, ChatGPT, Google and Cursor.

StackSpirit only displays the note. It does not accept vendor terms for the owner, does
not choose an auth mode for a harness, and does not turn a subscription login into an
API key. The note is part of the manifest and is validated like every other text field.

## Custom manifests

Owner-defined manifests live in one strict TOML file per harness:

```text
~/.stackspirit/harnesses/<id>.toml
```

`<id>` must be 1-64 lowercase ASCII letters, digits or hyphens, start and end with a
letter or digit, and match the file stem exactly. Built-in ids are reserved. The file is
written atomically with owner-only permissions (`0700` directory, `0600` file on Unix).

The format is a deliberately small top-level TOML subset: `key = value` with strings,
booleans, integers and string arrays only. Tables, dotted keys, multi-line values and
unknown fields are rejected. In particular there is no `api_key`, `token`, `password`
or `oauth_token` field, so a credential cannot be smuggled into the registry. Free-text
fields also reject obvious secret material such as a private-key block or a provider key
prefix.

Example:

```toml
id = "my-harness"
name = "My Harness"
binary = "my-harness"
install_cmd = "npm install -g my-harness"
auth_cmd = "my-harness login"
protocol = "acp"
surface = "orchestrator_and_workers"
supports_embedded_chat = true
auth_modes = ["api_key", "subscription"]
tos_note = "API-key use follows the provider's API terms. Subscription sign-in follows its consumer terms and may be restricted."
docs_url = "https://example.com/docs"
oauth_callback_port = 1455
builtin = false
```

`surface` is written by every new save. A custom file from before the split may omit it;
the loader then derives the surface from `supports_embedded_chat` instead of rejecting
the file. If both fields are present they must agree, and `protocol = "cli-pty"` must be
`surface = "workers_only_terminal"`.

`builtin` must be false or omitted. An invalid custom file is skipped with a safe
warning in `HarnessListView.warnings`; one broken file does not block the built-ins or
the other custom entries. The panel never shows file contents.

## Probe and connect plan

`list` returns the built-in presets first, then valid custom entries sorted by id, plus
one safe warning per skipped file.

`probe({ id, target })` asks only two safe questions, with an 8-second timeout:

1. Does `<binary> --version` answer on this Mac or on the Home VPS over the saved SSH
   connection? The first safe line is control-character-stripped, redacted and capped at
   120 characters.
2. Does the harness's own non-interactive status command report a stored sign-in?
   StackSpirit runs only `opencode auth list`, `claude auth status` or
   `codex login status`; any other harness reports `unknown` and the panel says the
   sign-in state cannot be proven non-interactively. No credential file is read.

The result is path-free: `binary` is `found`, `missing` or `unknown`, `auth` is
`configured`, `missing` or `unknown`, and `readiness` is the shortest honest summary
(`ready`, `install_required`, `auth_required` or `unknown`). `auth_detail` and warnings
are fixed copy or redacted text, never raw command output.

`probe` also returns the display-safe connect plan:

- `install_command` only when the binary is missing; the panel shows it and never runs
  it automatically.
- `auth_command` only when the binary is present and the manifest's `auth_cmd` parses as
  shell-free argv. Quotes and shell control operators are rejected, not interpreted.
- `requires_loopback_forward` / `forward_port` for a Home VPS sign-in that needs an OAuth
  callback.

### The temporary loopback SSH forward

Some harnesses print an OAuth callback on `127.0.0.1:<port>`. When such a harness is
connected on the Home VPS, the desktop host starts

```text
ssh -N -L 127.0.0.1:<port>:127.0.0.1:<port> ...
```

with the same app-private `known_hosts`, strict host-key checking, identity file,
batch mode and timeouts as every other remote command, plus
`ExitOnForwardFailure=yes`. It waits up to 8 seconds for the local port to accept a
connection. There is no manual tunnel and no user-visible terminal.

The host then starts the sign-in child shell-free, reads the first http(s) URL it prints
(up to 15 seconds) and opens it with the OS handler. The opener refuses anything except
`https://` and loopback `http://127.0.0.1:`, `http://localhost:` or `http://[::1]:` URLs.
If the manifest has no runnable sign-in command, the host opens the harness's
documentation instead. Every child process is kept in host state and is torn down when
the sign-in finishes or the app exits. A second connect for the same harness and target
is refused as a `conflict`.

### What Connect does (and does not)

Connect installs the selected harness and starts its own sign-in. For a Home VPS
sign-in that needs an OAuth callback, the host can also run the temporary loopback
forward described above. That is the whole of Connect today.

The embedded chat transport still runs on the bundled OpenCode engine. An
`orchestrator_and_workers` manifest says the harness could back that transport, but the
panel does not switch chat to it: choosing a non-OpenCode harness as the chat transport
is not wired yet. Worker-only harnesses are unaffected — they are for jobs, not chat,
and the panel keeps labelling them **Worker-Only**.

The capability table above describes the manifest field and its intended meaning, not a
live chat-transport selector. The panel shows a short inline note to the same effect so
a successful Connect is not mistaken for a chat switch.

See [`HARNESS-CHAT.md`](HARNESS-CHAT.md) for the transport-agnostic design of the next
phase: the selection model, the adapter contract for the four chat protocols, the safety
gates and the phased plan. It is a design only, and the first protocol is still an open
owner choice, so no transport is live.

## ACP client core

`crates/spirit-core/src/harnesses/acp.rs` is the reusable ACP stdio JSON-RPC 2.0 client.
It implements the core handshake and turn framing:

- `initialize` with newline-delimited JSON-RPC 2.0 and `protocolVersion: 1`. The client
  advertises **no** `fs/*` or `terminal/*` capabilities.
- `session/new` with a working directory and no MCP servers; the returned native session
  id is kept in the client.
- `session/prompt` with one text content block.
- Streaming `session/update` notifications delivered to an `AcpObserver` in arrival
  order; known text/tool/plan variants are typed and an unknown kind stays `Unknown`.
- `session/request_permission`: the observer chooses `allow_once`, `allow_always`,
  `reject_once`, `reject_always` or cancel. The default is cancel, never auto-approve. A
  missing matching option sends `cancelled` instead of guessing.
- `session/cancel` as a notification.
- Bounded reads (1 MiB per line), a 30 s request/read deadline and a 10-minute prompt
  deadline. JSON-RPC errors become typed `parse` failures; raw payloads are not echoed.

`AcpClient::spawn` owns one child with piped stdin/stdout, no captured stderr and
`kill_on_drop(true)`; `shutdown` closes stdin and reaps it. `AcpClient::from_io` is the
hermetic seam used by deterministic mock-process tests in `acp.rs` and
`crates/spirit-core/tests/harnesses.rs`. The client only reads auth-method **ids** from
`initialize`; any accompanying metadata is discarded. It never reads a credential file
and never extracts, stores or logs a provider OAuth token.

This is core plumbing, not a live chat selector. Wiring an ACP harness into the embedded
chat, the selection UI and permission policy is still the phased work described in
[`HARNESS-CHAT.md`](HARNESS-CHAT.md).

## Security limits

- The manifest schema has no secret field, unknown fields are rejected, and free-text
  fields reject obvious secret material. No key, token, password or private key is
  stored by this feature.
- The ACP client never reads a credential file, never puts a credential in argv or the
  environment, and keeps only safe auth-method ids from `initialize`. A provider OAuth
  token is never extracted or stored, and worker-only consumer sign-in is never wrapped
  in the embedded chat.
- Probing never reads a harness credential file and never prints a credential value. The
  only auth checks are the three safe status commands listed above; their output is
  reduced to a fixed readiness description.
- Probe results carry no local path, known_hosts path, identity path, SSH target or raw
  command output. The renderer receives only the safe view.
- One-click connect is desktop-only. Preview lists the built-ins and returns
  `preview_unavailable` for `saveCustom`, `deleteCustom`, `probe` and `connectStart`; it
  never fakes a probe result, a saved file or a running sign-in. The Web bridge omits
  the namespace.
- The forward binds loopback only and is owned by the host. The app never asks the owner
  to run `ssh` or copy a callback URL between machines.
- `docs_url` must be `https://`. The browser opener additionally refuses non-loopback
  plain HTTP.

Provider keys still belong in the harness's own store, the local OS keychain, or VOWP's
`vowp secret` vault for server-side use. See [`SECURITY.md`](../SECURITY.md) and
[`HANDOFF-DESKTOP-UI.md`](HANDOFF-DESKTOP-UI.md) §3–§4.
