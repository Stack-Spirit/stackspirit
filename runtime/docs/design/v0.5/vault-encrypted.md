# Encrypted-at-rest vault mode (v0.5)

Status: **implemented** in v0.5 Slice 5. The backend is the `age` keyfile mode in
`vowp-core::vault`; the non-interactive `vowp secret set --stdin` path and the storage mode in
`vowp secret ls --json` ship with it. `systemd-creds` is **not implemented** in this slice and
must not be presented as shipped (see §8).

The authoritative storage and injection behaviour remains
[`v0.2/home-and-surfaces.md`](../v0.2/home-and-surfaces.md) §9 and §13.2. This note records the
new mode, its key custody, the upgrade path and the CLI contract. `docs/design/cli.md`,
`docs/guides/operating.md` and `docs/reference/**` are updated by the v0.5 close-out docs job,
not here.

## 1. Modes

`~/.vowp/secrets/mode` selects one storage backend. A missing file means `basic`; an unknown
value is a configuration error, never a silent fallback.

| Mode | File contents | Storage | Owner | Protects against |
|---|---|---|---|---|
| **vault user** | `vault` | `/home/vowp-vault/secrets/<NAME>`, mode 0600 | `vowp-vault` | the runtime user reading the files directly |
| **basic** | `basic` (or no file) | `~/.vowp/secrets/<NAME>`, mode 0600 | the runtime user | other Unix users, not the agent's own user |
| **encrypted** | `encrypted` | `~/.vowp/secrets/encrypted/<NAME>`, mode 0600, age ciphertext | the runtime user | an offline copy of the home without `age.key` |

`VaultMode` in `vowp-protocol` gains `Encrypted`, serialised as `encrypted`. `SecretInfo` is
unchanged: the wire metadata still contains only the name, last four characters and timestamps.

## 2. File layout

```
~/.vowp/secrets/
  mode                     # "encrypted\n", mode 0600
  age.key                  # age identity, mode 0600, generated on first encrypted write
  encrypted/<NAME>         # age ciphertext, mode 0600, one file per secret
```

The age identity is an `age` X25519 secret key. It is generated only when no ciphertext exists.
If the key is missing while encrypted files are present, every read and write is a typed error:
VOWP does not generate a replacement key that could not decrypt the existing values. The key is
never listed as a secret (the listing walks the `secret_index` table and the ciphertext
directory, not arbitrary files under `secrets/`), never printed, and never backed up, because
`secrets/` is excluded from every backup.

The `secret_index` table still stores metadata only: name, last four characters, `created_at`
and `updated_at`. Values never enter the database.

## 3. Key custody and honest limits

Encrypted mode protects a copy of the home that does not include the key: a stolen disk, a
snapshot of `~/.vowp`, or a stray copy of the ciphertext directory. It is **not** protection
from code running as the same user. The identity is a plaintext file at mode 0600 owned by the
runtime user, and a harness spawned as that user can read it and every ciphertext file.

- There is no key rotation, no passphrase, no hardware keystore and no recovery path in this
  slice. Losing `age.key` loses every encrypted value; restore the key from the owner's own
  protected copy or set the secrets again.
- The key is not in a backup. Restoring a home restores only metadata and no values.
- The metadata (name, last four characters, timestamps) stays plaintext in the database.
- A value that was already sent to a provider, written into a harness transcript, or printed
  by an agent is outside this backend. Rotate it at the provider.

## 4. Commands

```sh
vowp secret set <NAME> --stdin    # non-interactive: read the value from stdin
vowp secret set <NAME>            # hidden prompt, needs a terminal
vowp secret ls                    # table plus `storage: <mode>`
vowp secret ls --json             # one object: mode + SecretInfo array
vowp secret rm <NAME> --yes
vowp secret upgrade --encrypted   # encrypt basic-mode secrets
vowp secret upgrade               # existing vault-user upgrade, unchanged
vowp secret request <NAME>        # one-time 127.0.0.1 paste page
```

### `--stdin` contract

- The value is read from the process stdin, never from argv. The only positional argument is the
  name.
- Exactly one trailing `\n` or `\r\n` is stripped. A further newline remains part of the value.
- The result must not be empty; an empty value is refused with exit 2.
- No TTY is required. The hidden-prompt form keeps its existing `EchoGuard` behaviour and
  refuses a non-terminal stream, now decided through the CLI's `IoStreams` rather than a global
  stdin check.

### `ls --json`

The document is exactly one object (this is a deliberate breaking change from the earlier bare
array):

```json
{"mode":"basic","secrets":[{"name":"NAME","last4":"1234","created_at":"…","updated_at":"…"}]}
```

`mode` is `basic`, `vault_user` or `encrypted`; `secrets` is the existing `SecretInfo` array.
Human output keeps the table and adds one `storage: <mode>` line. `secret set --json` still
prints one `SecretInfo`, and `secret rm --json` still prints `{"name":"NAME","removed":true}`.

### `upgrade --encrypted`

`vowp secret upgrade --encrypted` moves basic-mode values into `secrets/encrypted/`:

1. Every value is read, encrypted and written as ciphertext.
2. Only after all of them succeeded are the plaintext copies removed.
3. The mode file is written last, so an interrupted upgrade leaves basic mode active.
4. Existing `SecretInfo` rows are not touched, preserving `last4`, `created_at` and `updated_at`.

In vault-user mode the command refuses with a guard error and leaves the mode unchanged. When
the mode is already `encrypted` it is idempotent and reports the existing mode. The plain
`vowp secret upgrade` keeps its existing vault-user behaviour exactly; in encrypted mode it is
a no-op that reports `secrets: encrypted` and never switches away from the ciphertext.

## 5. SSH-pipe routing

On a control machine, `secret set/ls/rm/upgrade` route to the home over SSH. The hidden-prompt
form still uses `ssh -t` because the home's prompt needs a terminal. `secret set NAME --stdin`
sets `remote::is_interactive` to false, so the same command runs as `ssh` **without** `-t` and
the caller's stdin is piped straight to the home:

```sh
printf '%s\n' "$VALUE" | vowp --node home secret set PROVIDER_KEY --stdin
```

The value crosses the encrypted SSH channel, is never placed in argv, and is not echoed. The
remote command still names `--stdin`, so both sides agree on the non-interactive path.

## 6. Injection and redaction

`env_pass`, promotion `env_pass` and the hidden `__vault-exec` read through the same
mode-aware `read_value_impl`. In encrypted mode the ciphertext is decrypted with `age.key`; a
missing, unreadable or wrong key is a typed error, never a plaintext fallback and never a
silent empty value.

`Redactor::load` collects names from the index and the ciphertext directory, decrypts each
value, and keeps the existing longest-first exact-match behaviour. Event payloads, raw mirrors,
capsules, the snapshot, the supervisor log, gate evidence and ledger rows are redacted exactly
as before. Encryption at rest does not change redaction coverage or its timing.

## 7. What is not encrypted

- `age.key` itself. It is a mode-0600 plaintext identity, not a passphrase-protected key.
- Metadata in `secret_index` and the mode file.
- Values already held by a provider or harness, and values printed into a raw report or run
  mirror while a turn is still running.
- Session logs, capsules, the database and the rest of the home. Only stored secret files are
  ciphertext.
- Backups. Every mode excludes `secrets/`, so a backup never contains plaintext or ciphertext
  values.

## 8. Explicit non-goal: `systemd-creds`

`systemd-creds` is **not implemented** in this slice. The accepted v0.5 scope mentioned it as a
possible Linux backend; this job ships the age keyfile backend only. A node that needs
systemd-bound credentials must wait for a later slice; nothing in the commands, help text or
this note may claim otherwise.

## 9. Tests

Hermetic integration tests live in `crates/vowp-core/tests/vault.rs` and use a temporary home,
a fixed `FakeClock` and no network or tmux:

- `encrypted_mode_round_trips_without_plaintext_on_disk`
- `encrypted_mode_refuses_a_wrong_or_missing_key`
- `upgrade_basic_to_encrypted_keeps_metadata_and_removes_plaintext`
- `upgrade_encrypted_refuses_vault_user_mode`
- `encrypted_mode_lists_and_redacts_without_exposing_values`
- the extended `mode_file_is_read_and_unknown_values_are_refused`

CLI coverage is in `crates/vowp-cli/src/cmd/secret.rs` (argument and stdin parsing),
`crates/vowp-cli/tests/maint.rs` (`secret_set_stdin_and_ls_json_report_the_mode_without_values`,
`secret_ls_json_is_an_object_and_rm_keeps_its_shape`,
`secret_set_stdin_refuses_an_empty_value`) and `crates/vowp-cli/tests/control.rs`
(`secret_set_stdin_routes_without_a_tty_and_keeps_the_tty_form`).
