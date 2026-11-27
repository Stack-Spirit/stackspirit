# Harnesses

The Universal Harness Registry describes the coding harnesses StackSpirit can launch on
this Mac or the connected Home VPS: how to install one, how to sign in, whether it can
back the embedded chat or only run as a worker, and what the owner should know about
API-key versus subscription terms.

It is reachable at **Settings → Advanced → Harnesses** in the desktop app and is backed
by `spirit-core`. It is real, not a mock: the desktop host reads the registry, probes
both targets, and owns the one-click sign-in child processes. The panel is desktop-only
for connect. Preview mode lists the built-in presets and refuses every probe, change and
connect with a typed `preview_unavailable` error; the read-only Web bridge has no harness
surface at all.

## Where it lives

| What | Where |
|---|---|
| Panel | `packages/spirit-ui/src/next/HarnessesPanel.tsx`; mounted by `packages/spirit-ui/src/next/SettingsView.tsx` |
| Bridge contract | `packages/spirit-ui/src/bridge/SpiritBridge.ts` (`HarnessesSurface`, `SpiritBridge.harnesses`) and `packages/spirit-ui/src/bridge/wire.ts` (DTOs) |
| Desktop adapter | `apps/spirit-desktop/src/bridge/TauriBridge.ts` (`harnesses`) |
| Tauri commands | `apps/spirit-desktop/src-tauri/src/harnesses.rs` (`harnesses_list`, `harnesses_save_custom`, `harnesses_delete_custom`, `harnesses_probe`, `harnesses_connect_start`) |
| Core registry | `crates/spirit-core/src/harnesses/` (`manifest.rs`, `probe.rs`, `store.rs`, `toml.rs`) |
| Tests | `packages/spirit-ui/src/test/next/harnessesPanel.test.tsx`, `packages/spirit-ui/src/test/next/harnessSettings.test.tsx` |
| Preview | `MockBridge.harnesses` lists the built-in presets; every other call fails with `preview_unavailable` |

The bridge namespace is optional: presence is the capability. It exposes `list`,
`saveCustom`, `deleteCustom`, `probe` and `connectStart`.

## Built-in presets

Built-ins ship in code and are read-only. OpenCode is first and is the documented
default (`DEFAULT_HARNESS_ID`). A custom file may not reuse a built-in id.

| id | Name | Binary | Protocol | Embedded Chat | Auth modes |
|---|---|---|---|---|---|
| `opencode` | OpenCode | `opencode` | `opencode-http` | yes | API key |
| `claude-code` | Claude Code | `claude` | `stream-json` | yes | API key, subscription |
| `codex` | Codex | `codex` | `codex-json` | yes | API key, subscription |
| `gemini-cli` | Gemini CLI | `gemini` | `acp` | yes | API key, subscription |
| `antigravity` | Antigravity | `antigravity` | `cli-pty` | no | subscription |
| `cursor-cli` | Cursor CLI | `cursor-agent` | `cli-pty` | no | subscription |
| `aider` | Aider | `aider` | `cli-pty` | no | API key |
| `goose` | Goose | `goose` | `acp` | yes | API key |

`supports_embedded_chat` records whether the preset can back StackSpirit's embedded chat
transport; `false` means the harness is worker-only and the panel labels it
**Worker-Only**. The registry does not invent a chat transport for a worker-only
harness.

## Protocols

| Wire value | Meaning |
|---|---|
| `opencode-http` | OpenCode's local HTTP server |
| `acp` | Agent Client Protocol over stdio |
| `stream-json` | Claude Code's stream-json output |
| `codex-json` | Codex CLI's JSON output |
| `cli-pty` | An interactive CLI owned through a pseudo-terminal |

The protocol is a manifest fact and is copied into every probe result so a stale
renderer cannot relabel a harness.

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
protocol = "stream-json"
supports_embedded_chat = true
auth_modes = ["api_key", "subscription"]
tos_note = "API-key use follows the provider's API terms. Subscription sign-in follows its consumer terms and may be restricted."
docs_url = "https://example.com/docs"
oauth_callback_port = 1455
builtin = false
```

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

The embedded chat transport still runs on the bundled OpenCode engine. A registry
harness with `supports_embedded_chat = true` says it could back that transport, but the
panel does not switch chat to it: choosing a non-OpenCode harness as the chat transport
is not implemented yet. Worker-only harnesses are unaffected — they are for jobs, not
chat, and the panel keeps labelling them **Worker-Only**.

The capability table above describes the manifest field and its intended meaning, not a
live chat-transport selector. The panel shows a short inline note to the same effect so
a successful Connect is not mistaken for a chat switch.

## Security limits

- The manifest schema has no secret field, unknown fields are rejected, and free-text
  fields reject obvious secret material. No key, token, password or private key is
  stored by this feature.
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
