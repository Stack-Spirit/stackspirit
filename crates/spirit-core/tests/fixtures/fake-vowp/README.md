# Hermetic fake `vowp`

`vowp` is a POSIX shell fixture for `crates/spirit-core/tests/chat_writer.rs`. It never
calls a real VOWP binary, the network, `~/.vowp`, `~/.vowp-agent`, the keychain or an
engine. The test copies it to a private temporary `PATH` and sets `VOWP_AGENT_HOME` to a
private state directory.

## State files

The test writes and reads files under `$VOWP_AGENT_HOME`:

| File | Meaning |
|---|---|
| `mode` | The scenario (see below). Defaults to `attach-grant`. |
| `canary` | The test's planted lease-token prefix. Read only; never written back. |
| `grant-variant` | `ok`, `wrong-session`, `wrong-native`, `non-loopback`, `no-auth`, `read-only`, `wrong-surface`, `other-app` or `malformed`. |
| `session` / `native` | The VOWP home session id and native OpenCode session id. |
| `url` | The loopback endpoint printed in documents and grants. |
| `node` | The safe node name used in the owner and label. |
| `owner-state` | The owner state printed by the watch document. Defaults to `attached`. |
| `attach-count` | Incremented on every grant. |
| `surface-ids.log` | The surface id of every grant, for reissue assertions. |
| `argv.log` | Every invocation and its argv, one `CALL` block per process. |
| `attach-pid` | The pid of the last grant child. |
| `attach-env.log` | The grant child's environment, for the allow-list assertion. |
| `stdin-open` | Present while the grant child holds stdin open. |
| `hold-start-at` / `hold-eof-at` | Timestamps around the stdin hold, used by the drop diagnostics. |
| `released` | Written after the child observes stdin EOF. |
| `watch-count` | Watch-endpoint attempts for the retry scenarios. |

The canary token is printed only on stdout and is never written to any file.

## Modes

| Mode | Behavior |
|---|---|
| `attach-grant` | Print a valid pretty-printed grant, hold stdin, write `released` on EOF. |
| `attach-conflict` | Print a VOWP-shaped `conflict` error and exit 4. |
| `attach-node` | Print a VOWP-shaped `refused` error and exit 5. |
| `attach-usage` | Print nothing on stdout, a usage line on stderr, exit 2. |
| `attach-hang` | Print nothing and wait for the parent to give up. |
| `attach-crash` | Print a valid grant, then exit 1 without waiting for stdin. |
| `attach-old` | No `attach` subcommand; exit 2 for `--help` and the real command. |
| `endpoint-owner` | Print a home endpoint with `access: "owner"`. |
| `endpoint-other` | Print a home endpoint owned by another surface. |
| `endpoint-node` | Print the agent-shaped node document. |
| `endpoint-home-node` | Agent has no record; the home command prints node metadata. |
| `endpoint-stopped` | Print a stopped document with null URL/auth. |
| `not-home` | Both endpoint commands exit 3. |
| `watch-once-fail` | Like `attach-grant`; the first `--surface` watch fails, the next succeeds. |
| `watch-always-fail` | Like `attach-grant`; every `--surface` watch fails. |

`attach-grant`, `watch-once-fail` and `watch-always-fail` print an owner endpoint when the
home command is called with `--surface`; without it they print an available, read-only
document so the probe can run before an attach.
