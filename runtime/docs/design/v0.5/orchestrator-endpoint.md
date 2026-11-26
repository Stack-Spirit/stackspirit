# v0.5 — official orchestrator endpoint discovery

Status: **implemented** for v0.5 Slice 3; v0.5.1 adds the agent-side native-session hint.
`vowp orchestrator endpoint <name>` returns the
typed document a desktop client uses to connect its chat UI to an orchestrator's paired app
process, and `vowp agent orchestrator endpoint <name>` returns the same document on the
machine that runs the app. The scope is in [`scope.md`](scope.md) §3; the command contracts
are in [`../cli.md`](../cli.md) §6.10 and §12.

## Document

`OrchestratorEndpoint` is exported by `vowp schema OrchestratorEndpoint`. Every field is
always present, and an unavailable value is `null`, not an omitted key.

```json
{
  "name": "main",
  "app": "opencode",
  "session_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  "location": "home",
  "node": null,
  "state": "running",
  "url": "http://127.0.0.1:54321",
  "native_session_id": "ses_01HZY4Q3V7J8K9M0N1P2Q3R4S5",
  "auth": {
    "header": "Authorization",
    "scheme": "Basic",
    "username": "opencode",
    "password": "<the live local service password>"
  }
}
```

| Field | Meaning |
|---|---|
| `name` | Orchestrator name. |
| `app` | `opencode`, `claude_code` or `chat`. |
| `session_id` | The home session id the orchestrator mirrors. It is the same id before and after a move. |
| `location` | `home` when this home runs the app host; `node` when an enrolled agent node does. |
| `node` | The node that runs the app, when the record knows one. |
| `state` | The card's lifecycle state (`starting`, `running`, `stopped`, `degraded`, `node_offline`, `failed`). |
| `url` | A loopback URL (`http://127.0.0.1:<port>`), only for a live local endpoint. |
| `native_session_id` | The app's own session id, when the local host knows one: a value in the app's `service.json` first, then the latest id its bridge persisted beside `host.json`, else `null`. |
| `auth` | Local auth material, only for a live local endpoint. |

`OrchestratorAuth` carries `header`, `scheme`, `username` and `password`. Its custom
`Debug` prints `password: [REDACTED]`. The password is a secret: it appears only in the
explicit local endpoint document, never in a card, `snapshot.json`, `fleet status`,
`orchestrator ls`, the supervisor log, a result frame or an error string.

## Home and agent split

The two commands read different places and have different powers.

- `vowp orchestrator endpoint <name> [--json]` is a **home-side inspection**. It reads the
  home's `~/.vowp/orchestrators/<name>/orchestrator.json`. For a `home` location with a live
  host it calls the app host's own `endpoint` method, which reads the private 0600 service
  state (`host.json` and opencode's `service.json`). It never opens SSH, a socket or a
  node's files, and it never starts a tunnel or a browser. The global `--node` flag is not
  an SSH redirect for this command; it is consumed and the home record is still read here.
- `vowp agent orchestrator endpoint <name> [--json]` is **local to the agent machine**. It
  reads `~/.vowp-agent/orchestrators/<name>/config.json` (or `$VOWP_AGENT_HOME`), the
  recorded `host.json`, and the 0600 app `service.json` named by `service_json`. It never
  contacts the home. The document always has `location: "node"` because the app runs on
  that machine; `node` is the node name recorded in `agent.json`, or `null` before
  enrollment.

A remote record on the home returns the base document with `url`, `native_session_id` and
`auth` all `null`. The human output says:

```text
run on that machine: vowp agent orchestrator endpoint main --json
```

## Loopback and credential rules

- The only accepted URL is `http://127.0.0.1:<port>` or `http://localhost:<port>`. The home
  host canonicalises it to `http://127.0.0.1:<port>` and refuses `0.0.0.0`, a LAN address,
  `https`, a missing port or a zero port.
- `auth` is present only while the app host's recorded pid is alive and its service state is
  a regular file narrower than 0600. A service file left by an earlier run is not a live
  endpoint.
- The opencode endpoint uses the app's own Basic credentials: header `Authorization`,
  scheme `Basic`, username `opencode`, and the service password. Claude Remote Control
  opens no local port and returns `null` for `url`, `native_session_id` and `auth`; the
  `vowp chat` fallback is the same.
- The explicit local command is the only command that returns `auth.password`. Error
  strings name the orchestrator and the failure class (`not found`, `the app endpoint
  state is not readable`, `config_invalid`) and never echo a path, port, password, token or
  pair link.

## Native session id

OpenCode's `service.json` normally carries `password`, `pid`, `port`, `url` and `version`, not a
session id. The agent's opencode bridge learns the app-side id from the `sessionID` on the events
it mirrors and keeps the latest one in `<agent-state>/orchestrators/<name>/native-session.json`
(0600, next to `host.json`). The 0600 file is written atomically through the same private helper
as the host state, on first sight and whenever the app switches session, and contains only:

```json
{ "native_session_id": "ses_01HZY4Q3V7J8K9M0N1P2Q3R4S5" }
```

An opencode bridge loaded from a service file named by an orchestrator's private `config.json`
finds this path itself, so a normal start, resume or reconnect records the id without extra
wiring.

It never carries the Basic-auth password, the loopback URL, a port or a local path. A stop, a
failed host or a fresh app process clears the hint; a re-created app session replaces it. The
endpoint prefers a non-empty value in the app's `service.json`, then this persisted id, and
otherwise returns `null`. It never guesses from the app's newest `time.updated`. A node-hosted
record on the home still returns `null`, because only the agent-side command reads the hint.

## Stopped, remote, unknown and corrupt cases

| Case | Document |
|---|---|
| Live home-hosted opencode | `location: home`, loopback `url`, native id when the host knows one, `auth` with the password. |
| Stopped home host | `location: home`, `state: stopped`, `url`/`native_session_id`/`auth` null. A stale service file and a stale native-session hint are ignored. |
| Home-hosted Claude or chat | `location: home`, `url`/`auth` null: there is no loopback endpoint. |
| Node-hosted (home command) | `location: node`, the recorded node, the same home `session_id`, all three endpoint values null. No node file is read. |
| Agent command, live opencode | `location: node`, loopback `url`, `auth`; `native_session_id` is the service-file value, else the id the local bridge persisted, else null. |
| Agent command, stopped | The base document with `state: stopped` and all three endpoint values null; the native-session hint was removed. |
| Unknown name | Typed not-found (`not_found` on the agent). |
| Corrupt or too-wide private file | Typed `config_invalid` on the agent or an invalid error on the home; the message carries no path, port or secret. |

## Named tests

Protocol (`crates/vowp-protocol/tests/surface.rs` and `tests/schema.rs`):

- `orchestrator_endpoint_round_trips_home_and_node_documents`
- `orchestrator_endpoint_schema_is_exported_and_auth_debug_redacts_the_password`
- `schema_export_covers_listed_types`

Core (`crates/vowp-core/tests/orchestrator_endpoint.rs`):

- `orchestrator_endpoint_home_is_metadata_only_for_a_node`
- `orchestrator_endpoint_home_reads_the_live_local_service_file`
- `orchestrator_endpoint_stopped_or_unknown_has_no_credentials`
- `orchestrator_card_snapshot_and_ls_never_contain_the_endpoint_password`

Agent (`crates/vowp-agent/tests/orchestrator.rs`):

- `agent_orchestrator_endpoint_reads_a_live_0600_service_file`
- `agent_orchestrator_endpoint_reports_the_persisted_native_session_id`
- `agent_orchestrator_endpoint_stopped_and_unknown_are_metadata_only`
- `agent_orchestrator_endpoint_errors_never_echo_a_path_or_password`

Bridge (`crates/vowp-agent/tests/bridge.rs`):

- `opencode_bridge_persists_a_native_session_id_0600_without_secrets`
- `opencode_bridge_replaces_the_persisted_native_session_id_on_change`

CLI (`crates/vowp-cli/tests/orchestrator.rs` and `tests/agent.rs`):

- `orchestrator_endpoint_json_reports_home_and_node_locations`
- `orchestrator_endpoint_remote_never_returns_auth`
- `orchestrator_endpoint_redacts_a_corrupt_private_file_error`
- `agent_orchestrator_endpoint_json_returns_local_auth_and_ls_stays_clean`

Every test uses a temporary home or agent directory, a private 0600 service file and a
live process id; none opens a real app, a socket, SSH or the real home directory. Test
passwords are assembled at runtime so no secret-shaped literal is committed.
