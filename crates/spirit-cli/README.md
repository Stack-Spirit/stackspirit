# `spirit` CLI

The thin command line over [`spirit-core`](../spirit-core). It reuses the core host
store, SSH runner, `VowpClient` fixture client and view models. It has no Tauri,
React, desktop-app or Web dependency.

```sh
cargo build -p spirit-cli
cargo run -p spirit-cli -- --help
```

The binary is named `spirit`.

## Commands

```text
spirit [--json] [--fixtures] [--fixture <SCENARIO>] [--host <HOST>] <COMMAND>
```

| Command | Purpose |
|---|---|
| `spirit host add --label <L> --ssh <USER@HOST[:PORT]> --fingerprint SHA256:...` | Save a host profile with a confirmed pin |
| `spirit host ls` | List saved hosts |
| `spirit host show [HOST]` | Show one host profile |
| `spirit host remove [HOST]` | Remove one host profile |
| `spirit host trust [HOST] --fingerprint SHA256:...` | Re-scan, verify the fingerprint, and install the app-private `known_hosts` entry |
| `spirit host probe [HOST]` | Run the read-only reachability, auth, VOWP and snapshot probe |
| `spirit host connect [HOST]` | Connect once and print a connection snapshot |
| `spirit host disconnect [HOST]` | Report a disconnected CLI connection |
| `spirit fleet [--project <NAME>]` | Read the fleet view |
| `spirit capacity` | Read the capacity rail |
| `spirit gates` | Read the local-orchestrator and ASIF capability gates |
| `spirit engine status` | Read the saved engine state and preferences without starting anything |
| `spirit version` | Print the CLI version |

`--json` works on every command and writes one JSON document to stdout. Human output is
the default. `HOST` may be an exact host id, a unique id prefix, or a label; with one
saved host it can be omitted. `--host` is the equivalent global option.

### Typical host flow

```sh
# 1. Save the profile and the fingerprint you confirmed out of band.
spirit host add --label vps --ssh dev@203.0.113.10 --port 2222 \
  --fingerprint SHA256:abcdefghijklmnopqrstuvwxyz123456

# 2. Re-scan the host, verify that exact fingerprint, and install the private
#    known_hosts entry. A changed key is a hard conflict; there is no auto-accept.
spirit host trust 01J... --fingerprint SHA256:abcdefghijklmnopqrstuvwxyz123456

# 3. Probe, then connect once.
spirit host probe 01J...
spirit host connect 01J... --json
spirit fleet --json
spirit capacity
spirit gates --json
```

The CLI is one-shot by design: it holds no background connection, poll loop or daemon.
Each `fleet`, `capacity`, `gates`, `host probe` and `host connect` command opens its own
SSH connection and exits. `host disconnect` therefore reports the CLI's disconnected
state rather than stopping a daemon.

## Storage

The CLI keeps its own app-private store under:

```text
$XDG_CONFIG_HOME/stackspirit    (or ~/.config/stackspirit)
├── hosts.json                  schema_version 1, atomic write, mode 0600
├── known_hosts                 app-private, atomic write, mode 0600
└── engine.json                 read-only display input for `engine status`
```

The directory is created with mode 0700 on Unix when a write command first needs it.
Profiles never contain provider keys, passwords, tokens or private-key material. The
identity-file field is a path only; the CLI never reads the key. Read-only commands do
not create the directory.

`host add` does not touch the network. `host trust` is the explicit scan-and-pin step;
`host probe` and `host connect` use the system `ssh` client (through `spirit-core`'s
`SystemCommandRunner`). A missing pin, a changed key or a failed host-key check is
reported as a conflict, not silently accepted.

## Fixture mode

Pass `--fixtures` to preview against `spirit-core`'s documented golden documents. No
host, network, keychain or `~/.vowp` is touched. The command prints a `fixture data`
banner on stderr and still emits real JSON on stdout:

```sh
spirit --fixtures fleet --json
spirit --fixtures capacity --json
spirit --fixtures gates --json

# Pick another documented scenario.
spirit --fixtures --fixture multi-project fleet --project northnoir --json
spirit --fixtures --fixture storage-low capacity
spirit --fixtures --fixture gate-phase3-ready gates --json
```

Defaults are `fresh-single` for `fleet`/`capacity` and `gate-phase3-waiting` for
`gates`. `--fixture` without `--fixtures` is a usage error, and a command that cannot
use fixtures is rejected rather than silently falling back. The `fixtures` feature is
enabled by default for this developer CLI; `--no-default-features` removes the compiled
fixture documents and makes `--fixtures` a usage error.

Scenario names: `fresh-single`, `idle`, `busy`, `storage-low`, `memory-pressure`,
`provider-limit`, `supervisor-down`, `stale-cache`, `multi-project`, `queue-cap`,
`unknown-states`, `no-advice`, `disk-disagreement`, `engine-included`,
`engine-no-included`, `engine-external`, `gate-phase3-waiting`, `gate-phase3-ready`,
`gate-asif-waiting`, `gate-asif-ready`.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `2` | Usage, invalid input, or an unsupported flag combination |
| `3` | Conflict: a host-key mismatch, an ambiguous host, a stale/gated action |
| `4` | Not found: host, project or item |
| `5` | Remote command, SSH transport, local store, or output I/O failure |

Under `--json` a failure writes the core `SpiritError` object
(`{"code","message","hint","retryable"}`) to stderr. Human mode writes a short
`spirit: ...` message and an optional `hint:` line. Command-line parser failures exit 2
before the CLI error path.

## Safety boundaries

- No provider key is accepted, read, logged or written. `engine status` reports only the
  saved engine choice/model id and explicitly says the keychain was not read.
- No Tauri, React or desktop-app imports.
- No `.env`, `~/.vowp`, VOWP database, session log or lease is read.
- Tests inject `FakeCommandRunner` and use temporary config directories; they perform no
  real SSH and no real keychain access.
