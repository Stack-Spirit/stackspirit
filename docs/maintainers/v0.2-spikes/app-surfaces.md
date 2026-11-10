# S3 — app surfaces spike (opencode + Claude Code)

Status: **done**, with two small owner checks (the Mac desktop app click-through and a
Claude Remote Control join from a real claude.ai login). Card:
`VOWP-V2-PLAN.md` §13.3 `v2-s3-app-surface-spike`; design: §7.4.
Date: 2026-11-10. Reproduce with `scripts/spikes/apps/run.sh`.

## 1. What was tested, and against what

All checks ran on the Linux build host against **localhost only**: a throwaway
`XDG_*` tree, a throwaway `HOME` for Claude Code, a private `sshd` on
`127.0.0.1`, a private `TMUX_TMPDIR`, a fake OpenAI-compatible model, and a
dummy stdio MCP server. No real `~/.vowp`, no real `~/.claude`, no default tmux
server, no provider key, and no network request.

| Component | Version |
|---|---|
| opencode | 2.0.28 (`serve`, `pair`, `api`, `service`, `mcp`) |
| Claude Code | 2.1.319 |
| OpenSSH | 9.6p1 |
| Python | 3.12.3 |
| OS | Linux |

The opencode v2 CLI is not the v1 CLI: the HTTP API lives under `/api/...`, the
CLI has `opencode serve/pair/api/service`, and the SSE stream is `/api/event`.
F5/F6 must pin or version-check this, or every command below changes.

The script disables model-list fetching and autoupdate
(`OPENCODE_DISABLE_MODELS_FETCH=1`, `OPENCODE_DISABLE_AUTOUPDATE=1`) and Claude
Code's nonessential traffic (`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`); the
opencode log from the final run had no `models.dev` refresh. Everything the
spike talks to is on `127.0.0.1`.

The reproduction script performs **21 checks**, all passing:

```text
$ KEEP=1 scripts/spikes/apps/run.sh
...
PASS  opencode service on 127.0.0.1:<port>
PASS  opencode server listens on loopback only
PASS  opencode MCP server 'spike' is connected
PASS  opencode mcp list shows spike connected
PASS  pair link points at the private server
PASS  pair output prints the SSH tunnel command
PASS  SSH tunnel 127.0.0.1:<local> -> sshd:<ssh> -> opencode:<server>
PASS  pair link redeemed through the tunnel (token accepted as Basic auth password)
PASS  pair link is single-use
PASS  paired CLI listed sessions through the tunnel
PASS  session <session-id> created in <project> through the tunnel
PASS  turn one sent through opencode api + the tunnel
PASS  phone-style opencode --server continued the same session
PASS  message list and session export contain both turns
PASS  the fake model answered both turns on 127.0.0.1
PASS  live opencode events map to user.message, assistant.message, turn and cost events
PASS  Claude Code sees the project .mcp.json (pending the one-time approval)
PASS  Claude Code connects to the project-local MCP server
PASS  claude mcp get reports the server connected
PASS  the dummy MCP server answered tools/list for Claude Code
PASS  this Claude Code build has the Remote Control command
PASS=21 FAIL=0
RESULT: PASS
```

## 2. (a) opencode server, `opencode pair`, SSH tunnel, two surfaces

### 2.1 What works

The managed v2 service is the simplest "user service on the VPS":

```sh
cd <orchestrator-directory>
opencode service set port <port>      # private XDG_STATE_HOME in the spike
opencode service start                # prints http://127.0.0.1:<port>
opencode pair                         # one-time link, 5 minutes
```

`opencode pair` printed exactly the join path the desktop app needs:

```text
  Open a link to connect. Links work once and expire in 5 minutes.

  http://127.0.0.1:<port>/auth/connect/<code>

  Over SSH? Forward the port, then open the link on your machine:
  ssh -L <port>:127.0.0.1:<port> <host>
  If port <port> is busy locally, forward another port and use it in the link.
```

The spike then did what the Mac does:

1. Started a private `sshd` (throwaway host/client keys, `-F /dev/null`, private
   `known_hosts`) and forwarded `127.0.0.1:<local>` to the service through
   `ssh -N -L 127.0.0.1:<local>:127.0.0.1:<server>`.
2. Opened the pair link **through the forwarded port** and exchanged the
   one-time code for a token.
3. Used the token as the password for the server's HTTP Basic auth
   (`opencode:<token>`) — **not** as a Bearer token.
4. Listed sessions and created the throwaway session in the orchestrator
   project with `POST /api/session` and
   `location: {"directory": "<orchestrator-directory>"}`.
5. Sent the first turn with `opencode api` through the tunnel.
6. Sent the second turn from a blank "phone" directory with
   `opencode run --server http://127.0.0.1:<local> --session <session-id>
   --model fake/fake-model --format json "turn two from the phone"`.

Both surfaces ended up in the same session: the exported session had two user
messages and two assistant messages, and the SSE stream showed two
`session.execution.started`/`session.execution.succeeded` pairs for the same
`sessionID`. The second client did not need the project config; the server
resolved the session's location and the explicit model.

### 2.2 What was not run

The **Mac opencode desktop app itself** is not installable on this headless
Linux VPS. The protocol it uses was proven end-to-end (pair link, tunnel, token,
project location, session create/list, prompt, live events). The remaining
2-minute owner check is: on the Mac, run the printed `ssh -L` command, click the
pair link in the desktop app, open the orchestrator project, and send one
message; it must appear in the throwaway session and in the live event stream.

### 2.3 Gotchas

- **The managed service's default location is `$HOME`, not its start
  directory.** `opencode service start` run inside `<project>` still created
  sessions in the caller's home when the request had no `location`. Always pass
  `location.directory` (or make the app open the project) for a VOWP
  orchestrator. A **foreground** `opencode serve` *does* default to its start
  directory, but `opencode pair` only discovers `$XDG_STATE_HOME/opencode/service.json`;
  a foreground server under systemd needs VOWP to write a matching `service.json`
  (tested) or VOWP must manage the pair link itself.
- **The pair link is single-use and expires in 5 minutes.** Reusing it returned
  HTTP 401 with `Pairing link expired or already used`. Never log the code or
  token; treat both as secrets.
- **Auth is Basic, not Bearer.** `Authorization: Bearer <token>` was rejected;
  `Authorization: Basic base64("opencode:<token>")` worked. The server password
  printed at startup works the same way.
- **Do not run the opencode CLI on the VPS without `--server` or a private
  `XDG_STATE_HOME`.** It attaches to the global background service (the one in
  this environment listened on port 49374) and can conflict with the owner's or
  another job's service. The spike always used a private state tree; its
  cleanup stopped only that service.
- **`opencode run` quotes a prompt passed as a separate argument.** The phone
  turn appeared in the log as `"turn two from the phone"` (with quotes). It is
  cosmetic but worth knowing when mirroring transcripts.
- **`/session` is the web app, not the JSON API.** Use `/api/session`,
  `/api/session/<id>/message`, `/api/session/<id>/prompt`, `/api/event` and
  `/api/mcp`.

## 3. (b) Live messages, `session export`, and the VOWP event mapping

### 3.1 Reading the conversation

- **Live:** `GET /api/event` is the useful stream. It is SSE with a
  `server.connected` hello, heartbeats, and typed events. The spike captured
  this sequence for one turn:

  ```text
  session.inbox.enqueued        seq 1
  session.execution.started     seq 2
  session.instructions.updated  seq 3
  session.inbox.delivered       seq 4
  session.step.started          seq 5
  session.text.started          seq 6
  session.text.delta            (no durable seq)
  session.text.ended            seq 7
  session.step.streamed         seq 8
  session.step.ended            seq 9
  session.usage.updated         (no durable seq)
  session.execution.succeeded   seq 10
  ```

  `GET /api/experimental/session/<id>/log?follow=true` only sent a
  `log.synced` line in this probe; use `/api/event` for the mirror and
  `/api/experimental/session/<id>/export` (or the CLI) for backfill.

- **CLI API client:** `opencode api --server http://127.0.0.1:<local>
  GET /api/session/<id>/message` returned the full message list through the
  tunnel with the pair token. It works for `POST .../prompt` too.
- **Export:** `opencode session export --server http://127.0.0.1:<local>
  <session-id>` returned `{ "info": ..., "messages": [...] }`. For the two-turn
  session it had 7 entries: 2 user, 2 assistant text, 2 idle and 1
  `model-switched`; the final assistant text matched the live message list.

### 3.2 Mapping to VOWP events

`scripts/spikes/apps/map_events.py` is the executable version of this table.
It writes one VOWP-shaped envelope per durable event, with
`source.harness = "opencode"`, `source.native_session_id = <session-id>`, a
monotonic `seq`, an RFC3339-millisecond timestamp, and `replayable = true`.

| opencode event | VOWP event | Rule |
|---|---|---|
| `session.created` | `session.created` | session title |
| `session.inbox.enqueued` (`item.type = user`) | `user.message` | `item.payload.text` |
| `session.execution.started` | `turn.started` | one per execution |
| `session.text.ended` | `assistant.message` | **full text**; `text.started`/`text.delta` are UI-only |
| `session.tool.called` | `tool.call` | `id` + final `input` |
| `session.tool.success` | `tool.result` | `ok: true` + content |
| `session.tool.failed` | `tool.result` | `ok: false` + error |
| `session.usage.updated` | `cost` | tokens + USD (0 for this local model) |
| `session.execution.succeeded` | `turn.ended` | `outcome: succeeded` |
| `session.execution.interrupted` | `turn.ended` | `outcome: interrupted` |
| `session.execution.failed` | `error` then `turn.ended` | failure message |
| `session.step.failed` | `error` | step failure |
| `session.step.started` / `.streamed` / `.ended` | — | step boundaries; `usage.updated` carries tokens |
| `session.tool.input.started` / `.ended` | — | streaming args; `tool.called` has the final input |
| `session.text.started` / `.delta` | — | ephemeral UI streaming |
| `session.instructions.updated` | — (or `note`) | context/instructions changed |
| `session.inbox.delivered` | — | delivery ack |
| `session.model.selected` | `switch` or `note` | only treat as a VOWP switch when the executor/model changes; the spike used one model |
| `session.compaction.*` | `note`/`capsule` per F5 policy | not observed |
| `server.connected`, `project.updated`, `mcp.*` | — | not session content |

The spike run mapped **10** events: 2 `user.message`, 2 `assistant.message`,
2 `turn.started`, 2 `turn.ended`, 2 `cost`. That is exactly the durable shape
of the two-turn conversation; the streaming deltas and step/internal events
were skipped.

Notes for the production mirror:

- Coalesce multiple `session.text.ended` parts by `assistantMessageID` if the
  assistant emits text around tool calls; VOWP `assistant.message` is the
  durable full message.
- Keep `session.tool.input.ended` only as raw display data; do not log it as a
  second `tool.call`.
- The model in the spike is `fake-model`; F5 must write the real
  `source.model` (opencode's `model` on the message/assistant event).
- `session.usage.updated` has no `durable` seq; still record it as `cost`, but
  do not use it for ordering.

## 4. (c) Project-local MCP server in opencode and Claude Code

The dummy server (`scripts/spikes/apps/mcp_dummy.py`) is stdio, has one tool
(`vowp_spike_echo`), and appends method names to `MCP_DUMMY_LOG`.

### 4.1 opencode

Project-local `opencode.json`:

```json
{
  "mcp": {
    "spike": {
      "type": "local",
      "command": ["python3", "<repo>/scripts/spikes/apps/mcp_dummy.py"],
      "enabled": true,
      "timeout": 5000
    }
  }
}
```

Results:

```text
$ opencode mcp list
✓ spike  connected

$ curl -u "opencode:<password>" \
    "http://127.0.0.1:<port>/api/mcp?location%5Bdirectory%5D=<project>"
{"location":{"directory":"<project>"},"data":[{"name":"spike","status":{"status":"connected"}}]}
```

The dummy server log proves the load, not just a status flag:

```text
start pid=...
request initialize
notification notifications/initialized
request tools/list
tools_list vowp_spike_echo
```

**Finding for F5/F6:** opencode v2.0.28 connected the MCP server and fetched
its tool list, but the model request's `tools` array contained only the
built-ins (`edit`, `glob`, `grep`, `question`, `read`, `shell`, `skill`,
`subagent`, `webfetch`, `websearch`, `write`, `execute`) — not
`vowp_spike_echo`, even after the MCP status was `connected` before the first
prompt. The fake model never called the MCP tool. Treat "MCP server connected"
as proof of configuration, **not** as proof that the model can call the tool
in this build. F5/F6 should re-test with a real provider and check for a
tool-permission/config flag or an upstream fix before promising VOWP tools to
the orchestrator.

### 4.2 Claude Code

Project-local `.mcp.json`:

```json
{
  "mcpServers": {
    "vowp-spike": {
      "type": "stdio",
      "command": "python3",
      "args": ["<repo>/scripts/spikes/apps/mcp_dummy.py"],
      "env": {"MCP_DUMMY_LOG": "<evidence>/mcp-claude.log"}
    }
  }
}
```

Results:

```text
$ claude mcp list          # first run, before approval
vowp-spike: python3 ... - ⏸ Pending approval (run `claude` to approve)

$ claude mcp get vowp-spike
Scope: Project config (shared via .mcp.json)
Status: ✔ Connected
Type: stdio
```

Claude Code sends an MCP `server/discover` request before `initialize` in this
version. The minimal dummy server returns `method not found` for it and Claude
continues to `initialize`/`tools/list`; a production VOWP MCP server should
implement discovery or tolerate the error.

The one-time approval is a security gate. The spike pre-approved the server in
its **private** `CLAUDE_CONFIG_DIR/.claude.json` only to let a non-interactive
`claude mcp list` health-check it:

```json
{
  "projects": {
    "<project>": {
      "enabledMcpjsonServers": ["vowp-spike"],
      "disabledMcpjsonServers": [],
      "hasTrustDialogAccepted": true
    }
  }
}
```

The real VOWP setup must show the owner the project config and let them approve
it, or write only VOWP's own project-local config and never the user's global
Claude config.

## 5. (d) Claude Code Remote Control

The installed Claude Code has the command:

```text
$ claude remote-control --help
Remote Control - Control local sessions from claude.ai/code or the Claude mobile app
...
--name <name>        Name for the session (shown in claude.ai/code)
```

It needs a **claude.ai subscription login**; an API key is not enough. In a
throwaway `HOME`/`CLAUDE_CONFIG_DIR`, `claude auth status` returned
`{"loggedIn": false, "authMethod": "none"}` — exactly the state the plan assumes
for the VPS.

The spike did **not** run Remote Control with a real login. This build host's
`dev` user does have one, but using it would attach a throwaway session to the
owner's claude.ai account and subscription; the plan's VPS assumption is no
login. This is the 5-minute owner test (also written to evidence as
`remote-control-owner-test.md`):

```sh
cd ~/.vowp/orchestrators/<name>       # or any scratch directory
claude remote-control --name "VOWP S3 spike"
# open https://claude.ai/code or the Claude mobile app,
# find "VOWP S3 spike", send one short message,
# confirm both directions, then Ctrl-C in the terminal.
```

Record: Claude Code version, app used (desktop/web/mobile), and the send-to-see
time. If the owner's VPS stays without a claude.ai login, keep Remote Control
as "planned, needs a claude.ai login" in the docs; do not attempt OAuth from the
setup agent.

## 6. Recommendations for F5, F6 and T3

### F5 (app hosts + transcript mirror)

- Prefer the managed `opencode service start` for the orchestrator, because
  `opencode pair` discovers it natively. Always pass
  `location.directory = <orchestrator-dir>` on session creation (or make the
  app open that project); the service's default location is `$HOME`.
- If F5 runs a foreground `opencode serve` under systemd instead, write a
  matching `$XDG_STATE_HOME/opencode/service.json` (`id`, `version`, `url`,
  `pid`, `password`) so `opencode pair` works, or manage the pair link without
  the CLI. The foreground server is the one whose default location is its
  working directory.
- Bind `127.0.0.1` only. The pair link is loopback; the Mac and phone reach it
  through `ssh -L`. Never open a public port.
- Generate a pair link on demand, show the tunnel command, and treat the code
  and token as secrets. One link = one device; request a new link for a second
  device.
- Mirror `/api/event`, not the session-log endpoint. Log `text.ended` as
  `assistant.message`, tool events as `tool.call`/`tool.result`,
  `usage.updated` as `cost`, and execution boundaries as `turn.*`; ignore
  deltas, step internals and delivery acks. Use `session export` for backfill.
- Warm up the MCP connection (wait for `/api/mcp` `connected`) before the first
  owner turn. Do not claim the model can call MCP tools until the exposure gap
  above is re-tested with a real provider.
- Re-test on the target opencode version; v2.0.28's API and service layout are
  what this spike proves.

### F6 (`vowp mcp`)

- A stdio server with plain threads matches both clients. Test against opencode
  and Claude Code, not just a scripted client.
- In opencode, `opencode mcp list` is per-project; run it from the project or
  query `/api/mcp?location[directory]=...`. If it prints "No MCP servers
  configured" from the right directory, trust the API status with an explicit
  location. A server can be `connected` while the model still cannot call it.
- In Claude Code, `.mcp.json` is project-scoped and needs one-time approval;
  expect `Pending approval` in non-interactive health checks. Support
  `server/discover` or return a valid error.
- Do not write the owner's global app config; write only VOWP's project-local
  config under the orchestrator directory.

### T3 (real-app acceptance)

The remaining checks are the Mac opencode desktop app join (pair + tunnel +
send one message) and a real Claude Remote Control join from the owner's
claude.ai account. The phone in this spike was a second CLI client over the
tunnel, not a phone SSH app UI.

## 7. Reproduction

```sh
# from the repository root
KEEP=1 scripts/spikes/apps/run.sh
```

The script uses only localhost and temporary state. `KEEP=1` leaves the
evidence under `/tmp/vowp-s3-apps.*/evidence/` (versions, pair output with the
code redacted, MCP logs, messages, export, SSE capture and the VOWP mapping).
It prints `PASS=21 FAIL=0` and `RESULT: PASS` on this host; it exits non-zero
on any failed check. It does not use tmux at all, so no tmux server is touched.

## 8. Limits

- The Mac desktop app and a phone SSH app were not run; the pair/tunnel/API
  path they use was proven, and the owner checks are above.
- Claude Remote Control was not run with a real login; the command surface and
  the login gate were confirmed, and the 5-minute owner test is above.
- The model was a local fake, so tool-call behavior is not representative of a
  real provider. The MCP tool exposure gap is an opencode v2.0.28 observation;
  re-test after upgrading.
- One Linux host and one version matrix; F5 must also cover macOS/Linux homes
  in the normal workspace checks.
