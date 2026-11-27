# MCP server, fleet tools and dashboard

`stackspirit mcp` is StackSpirit's local stdio MCP server. It speaks newline-delimited
JSON-RPC 2.0 over stdin/stdout, so an MCP client starts it as a child process and no
network port is opened. The preserved `vowp` alias runs the same server and command tree.

The server has three modes: a read-only no-flag server, a full session server, and the
local agent proxy. The read-only and full servers also serve the
`ui://stackspirit/fleet-status` MCP Apps widget; the proxy does not.

## What runs where

| Mode | Command | Runs on | What it serves |
|---|---|---|---|
| Read-only | `stackspirit mcp` | The machine with the local home, or a control machine for the saved Home | Six read tools and the fleet-status resource; every write tool is refused with JSON-RPC `-32601` |
| Full session | `stackspirit mcp --session <id> --surface <id> --token-file <file>` | The Home, or a control machine routing to it | All 16 tools and the resource; session writes check the lease token |
| Local agent proxy | `stackspirit mcp --agent-socket <socket> --surface <id>` | An enrolled Mac or Linux agent node | `initialize`, `ping` and `tools/list` locally; `tools/call` is forwarded over the agent socket; no resources |

`vowp mcp` works in every row. The no-flag mode is what `npx -y stackspirit mcp` starts.

The full server requires `--session`, `--surface` and `--token-file` together.
`--agent-socket` selects the proxy, requires `--surface`, and conflicts with `--session`
and `--token-file`. A local orchestrator registers the proxy form in its configuration;
the proxy needs a paired surface, opens no store and reads no secret.

With no flags, `stackspirit mcp` is read-only. On a Home it reads the local state. On a
control machine it resolves the saved Home through the desktop host and reads through that
saved route; when the Home is unreachable it can serve the cached snapshot and marks it
stale. The client entry never names an SSH target.

## Connect a client

The minimal entry for any MCP client is:

```json
{
  "mcpServers": {
    "stackspirit": {
      "command": "stackspirit",
      "args": ["mcp"]
    }
  }
}
```

It contains no token, API key, password or SSH argument. The saved machine is resolved by
the CLI, not by an argument in the file. The desktop app's Settings → **CLI & MCP Connect**
panel shows this entry for **This Mac** and for the saved Home; the Home entry uses a
readable key such as `stackspirit-<home>` but the command is identical. The panel is
copy-only: it does not run the command, write a client config, start the server or open a
tunnel.

If the CLI is not installed globally, `npx -y stackspirit mcp` starts the same server. The
Settings panel uses the installed `stackspirit` binary.

## Tools

The full server lists 16 tools; the read-only server lists the six read tools below. A
`tools/call` result always has a `content` array with one `text` item and an `isError`
flag. `fleet_dashboard` puts Markdown in that item; the other tools put compact JSON
there. A tool failure is not a JSON-RPC error: it returns `isError: true` and text in the
form `<kind>: <message>` so the model sees the refusal. The local proxy adds `code` and
`retryable` members to a forwarded failure.

### Read tools

| Tool | Arguments | Returns |
|---|---|---|
| `fleet_status` | `project` (optional) | JSON `FleetStatus`: the snapshot, its source (`local`, `remote` or `cache`), `fetched_at` and `stale` |
| `fleet_dashboard` | `project` (optional) | One Markdown document with a Markdown heading, an ASCII gauge summary, a project table and any active brakes |
| `fleet_queue` | `project`, `state` (optional) | JSON array of queued and waiting jobs with project, job id, title and state |
| `inbox_list` | `project`, `all` (optional) | JSON array of open questions and approvals; `all` also returns answered and dismissed items |
| `capacity_report` | none | JSON capacity report: target, brakes, host sample, estimates, waiting jobs and cluster totals |
| `session_tail` | `session`, `after_seq`, `limit` | JSON array of session events; `limit` defaults to 50 and is capped at 1000 |

`session_tail` defaults to the server's bound session in full mode. In the read-only
server there is no bound session, so `session` is required, and a tail runs only on the
Home.

### Write tools

Write tools need a session-bound server: the full session server directly, or the local
agent proxy forwarding to the Home. The read-only server refuses them. Session and fleet
writes check the lease token; a moved session's old surface gets a `lease_revoked` failure
and the write is not retried. Read tools keep working after a lease loss.

| Tool | Arguments | Returns |
|---|---|---|
| `inbox_answer` | `id`, `answer` | The updated inbox item, with its parked job resolved |
| `job_add` | A `NewJob` document | The created job; its brief is written too |
| `job_pause` | `job`, `project`, `reason` | The parked job row |
| `job_retry` | `job`, `project` | The retried job row |
| `capacity_set` | `target`, `mode`, `override`, `reason` | The written capacity target |
| `tunnel_open` | `name`, `remote_port`, `local_port`, `host` | The opened tunnel; control machine only |
| `tunnel_close` | `name` | The closed tunnel; control machine only |
| `secret_request` | `name` | A one-time paste page URL and expiry; the secret value is never returned |
| `session_handoff` | `session`, `reason` | The capsule id and sequence |
| `session_resume` | `session`, `surface`, `takeover`, `limit` | The session, latest capsule, event tail and lease; takeover is Home-only |

## The fleet dashboard

`fleet_dashboard` is the zero-secret summary a model or a terminal can read directly. It
renders the live snapshot and capacity report as:

1. a `# StackSpirit fleet — <home>` heading and a supervisor/source/fetched/freshness line;
2. a fenced `text` block with supervisor status, worker slots, CPU, RAM and disk gauges in
   ten-cell ASCII form (`[####------]`), and the open inbox count;
3. a Projects table with state, running, ready and blocked counts;
4. a Brakes list when a brake is active, and a cluster line when worker nodes are online.

The optional `project` argument restricts the snapshot to one project. The output is one
MCP text content item containing Markdown, not a JSON document, so a client does not need
to parse gauges or tables. The tool definition also carries
`_meta.ui.resourceUri: "ui://stackspirit/fleet-status"` for clients that can render the
MCP Apps resource.

## The `ui://stackspirit/fleet-status` resource

The home and read-only servers list one resource:

| Field | Value |
|---|---|
| URI | `ui://stackspirit/fleet-status` |
| Name | `StackSpirit fleet status` |
| MIME type | `text/html;profile=mcp-app` |
| Contents | One `text` item containing a complete HTML document |

`resources/list` advertises the resource with `listChanged: false` and
`subscribe: false`; `resources/read` returns the rendered widget for that URI and a
resource-not-found error for any other URI. The widget is self-contained Meadow HTML:
inline CSS, no script, no remote font, image or stylesheet, and no network fetch. It
renders the same facts as the Markdown dashboard.

A client must support MCP Apps UI resources to display it: it reads the tool's
`_meta.ui.resourceUri`, calls `resources/read`, and renders the
`text/html;profile=mcp-app` content in its app view. A plain MCP client still receives the
Markdown from `fleet_dashboard`; it simply does not render the widget.

## The local agent proxy

The proxy is the agent-node mode:

```sh
stackspirit mcp --agent-socket <socket> --surface <id>
```

It answers `initialize`, `server/discover`, `ping` and `tools/list` on the node, then
forwards each `tools/call` as a typed request over the agent socket served by
`stackspirit agent run`. The Home checks the surface token scope and the live lease and
runs the same core path as its own server. The proxy never opens a store, reads a secret,
or prints an argument value, token or absolute local path.

The proxy advertises only the tools the Home executor can run. It keeps the fleet reads,
`inbox_answer`, `job_add`, `job_pause`, `job_retry`, `capacity_set`, `session_handoff` and
`session_resume`; it hides `fleet_dashboard`, `tunnel_open`, `tunnel_close`,
`secret_request` and the CLI-only control tools. It has no `resources` capability, so an
MCP Apps client cannot read the status widget through it.

The dashboard is hidden from the proxy because the Home executor behind the agent socket
has no `fleet_dashboard` arm yet. Until that path supports it, use the direct Home or
read-only server to get the dashboard and the widget; the proxy still serves the other
fleet reads and session tools.

## Safety

Both the Markdown tool and the HTML widget use one renderer and the same zero-secret
facts:

- No token, lease token, secret name or secret value is read or rendered.
- Every dynamic HTML field is escaped (`&`, `<`, `>`, `"`, `'`), so a hostile snapshot
  value cannot open a tag or attribute.
- Markdown values escape table and formatting metacharacters and fold newlines; values
  inside the ASCII code fence drop backticks and control characters. A crafted project
  name cannot break the table or the fence.
- The dashboard payload contains the Home name, supervisor state, counts, host gauges,
  project names, job cards, brake details and cluster counts. It does not carry a token,
  lease, secret, SSH target or project path.
- Both surfaces are read-only. Rendering either one never writes the database, opens a
  tunnel, requests a secret, answers an inbox item or starts a job.
- The read-only server never serves a write tool: the front door answers JSON-RPC
  `-32601` and the backend refuses the same calls again if reached directly.

The runtime test suite crafts a hostile supervisor version and asserts that the widget
escapes it rather than emitting a live script tag. The same suite covers the resource list
and read path, the read-only tool set and the proxy's filtered catalogue.

## Limits and planned work

- The local stdio server is the shipped MCP surface. A hosted Remote MCP endpoint at
  `mcp.stackspirit.io` is planned future work, not part of this release.
- The local agent proxy does not serve `fleet_dashboard` or the MCP Apps resource today;
  the direct Home and read-only servers do.
- On a control machine, fleet reads can fall back to the cached snapshot and report
  `stale`; the dashboard shows the same freshness marker.
- MCP Apps rendering depends on the client. The widget is read-only and has no controls.

## See also

- [`README.md`](../README.md) — install, surfaces and the documentation index.
- [`docs/ORCHESTRATOR-CHAT.md`](ORCHESTRATOR-CHAT.md) — how a local orchestrator registers
  the `vowp mcp --agent-socket` server with the bundled engine.
- [`docs/HARNESSES.md`](HARNESSES.md) — the harness registry and one-click sign-in on this
  Mac or the saved Home.
- [`SECURITY.md`](../SECURITY.md) — the security model, secret handling and reporting
  route.
- [`runtime/README.md`](../runtime/README.md) — the runtime CLI, daemons, MCP and packaging.
