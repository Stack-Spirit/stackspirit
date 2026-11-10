# S3 app-surfaces spike scripts

Reproduces the checks in `docs/maintainers/v0.2-spikes/app-surfaces.md` using only
`127.0.0.1`, temporary XDG/HOME/SSH state, a fake OpenAI-compatible model and a
dummy stdio MCP server.

```sh
scripts/spikes/apps/run.sh          # cleans up its temporary directory
KEEP=1 scripts/spikes/apps/run.sh   # keeps the evidence directory and prints its path
```

The script needs `opencode` (v2 CLI with `serve`, `pair`, `api`), `claude` (Claude
Code), `python3`, `curl`, `ssh`, `ssh-keygen` and `sshd`. It never touches the real
`~/.vowp`, `~/.claude`, the default tmux server or the network. It exits non-zero if
any check fails and prints `PASS`/`FAIL`/`NOTE` lines.

Files:

| File | Purpose |
|---|---|
| `run.sh` | Starts a private opencode service, a private sshd + tunnel, redeems a pair link, drives two clients on one session, checks MCP in opencode and Claude Code, and writes evidence. |
| `fake_openai.py` | OpenAI-compatible chat-completions server on `127.0.0.1`; returns a fixed answer and can ask for the `vowp_spike_echo` MCP tool if the client offers it. |
| `mcp_dummy.py` | One-tool stdio MCP server; appends `initialize`/`tools/list` lines to `MCP_DUMMY_LOG` so the caller can prove a client connected. |
| `map_events.py` | Maps a captured opencode `/api/event` SSE stream to VOWP-shaped `user.message`, `assistant.message`, `turn.*`, `tool.*`, `cost` and `error` events. |

`run.sh` uses `opencode service start` (the managed v2 service) and a real
`ssh -L` tunnel, so it exercises the same join path the desktop app uses. The Mac
desktop app itself is not runnable on the Linux home; the report records that as
the owner's short check.
