# Security

StackSpirit is a desktop-first control plane for a VOWP home. This document describes the
security behaviour implemented in this repository today: what is protected, how, and what is
not. It is a design statement, not a warranty or a promise of liability. StackSpirit is
MIT-licensed and provided **"as is"**; see [`LICENSE`](LICENSE).

Use placeholders when reporting or documenting a deployment. Never include a real key,
password, token, hostname, path or secret value in an issue, report, screenshot or log.

## Status

### SSH host-key pinning

- Adding a server scans the host keys and shows every offered `SHA256:` fingerprint. Nothing
  is saved until the owner explicitly confirms a fingerprint.
- The confirmed pin is stored on the host profile. Normal SSH commands use
  `StrictHostKeyChecking=yes`, an app-private `known_hosts` file and `BatchMode=yes`. The
  one-time password bootstrap is the only hop that relaxes `BatchMode` so the askpass
  helper can answer; it still keeps strict host-key checking and the app-private
  `known_hosts`.
- A changed host key fails as the typed `host_key_mismatch` error. Re-pinning is a separate,
  explicit action; StackSpirit never accepts a new key silently.
- `IdentitiesOnly=yes` is used when the profile selects an identity, so SSH does not offer
  unrelated agent keys.

### App-owned SSH key

- StackSpirit creates an ed25519 key pair locally in an app-private directory. The private
  key is an encrypted OpenSSH private key with `0600` permissions; the directory is `0700`.
- A random 32-byte passphrase is generated from the OS random source and stored only in the
  local OS keychain under the `ssh-key:<name>` account. The passphrase is not written to
  argv, the environment, a config file or any `Debug`/`Display`/`Serialize` value.
- The renderer receives the public key and fingerprint, never the private key or passphrase.
- A host profile stores an identity **path**, not key material. The host editor can show
  that local path; the Server secrets card never shows a path, key, token or value.

### One-time-password onboarding

- The server password is entered in an app-owned `type="password"` field. It is used once
  for the password bootstrap that installs the app-owned public key, then forgotten.
- The password is cleared from React state before the host call. It is never stored, never
  returned, and never included in an error, a log or a Tauri event.
- The optional `PasswordAuthentication no` step is reported exactly as `disabled`,
  `not_supported` with its reason, `failed` with its detail, or `not_requested`. Only a
  fresh, verified key-only login counts as a successful install.

### Provider keys

- Local model provider keys used by the StackSpirit Engine are stored in the local OS
  keychain. They are not written to the engine config, the VOWP home or a session log.
- Provider keys can instead be stored on the connected server in VOWP's own vault, through
  Settings → Advanced → **Server secrets**. StackSpirit does not create a second server
  secret store.

### Server secret store

- **List** reads `vowp --json secret ls` and shows only the name, `•••• last4`, the update
  time and, when VOWP reports one, the storage mode. The value is never part of the list
  model.
- **Set** is write-only. The value is sent to the server over the verified SSH connection
  on stdin, after VOWP's hidden prompt. It is never an argv element, an environment
  variable, a log line, an error field, a Tauri event or an input to the assistant.
- **Remove** calls `vowp --json secret rm NAME --yes`. Revoke needs no password, token or
  value.
- The Settings card requires a connected server. Before the first set in the app session it
  shows a prominent warning and requires the explicit checkbox "I understand this stores
  the key on the server". The acknowledgement is held in memory for the session and is not
  written to localStorage.
- The storage-mode badge shows the `mode` that `secret ls --json` reports (`basic`,
  `vault_user` or `encrypted`; an unknown safe token is shown verbatim). `storage: null` is
  shown as exactly that when VOWP reports none (VOWP 0.4.0 reported none). StackSpirit does
  not guess and does not run `vowp secret upgrade` on its own.

### Project add

The sidebar **Add project** dialog runs `vowp project add` on the home through the core
crate. Its boundary:

- `ProjectAddOutcome` never carries the home path. It reports the project name and the
  outcome only, so no remote filesystem location reaches the renderer or the assistant.
- When `project add` fails, the message and hint have absolute paths and `~` tokens replaced
  with `[remote-path]` before they are returned.
- A git URL with embedded credentials (for example `https://user:token@host/repo`) is
  refused before any argv is built, so the credential is never placed in a process
  argument list, a log or an error.
- `vowp project add` has no `--branch`; VOWP clones the default branch and the dialog has no
  branch field.
- This was exercised with fake-`vowp` tests here; no live Mac add run was performed from
  this Linux host.

### What the AI sees

The StackSpirit bridge, renderer, error types and tests carry secret **metadata** only:
name, last four characters, timestamps and a reported storage mode. The AI never receives a
full secret value through StackSpirit.

That is a statement about StackSpirit's code path, not a guarantee about a server it does
not control. On a basic-mode VOWP server, a harness or agent running as the runtime user can
read VOWP's secret files directly. See [Threat model and limits](#threat-model-and-limits).

## How a server secret is stored

The `ss-38-secrets-core` / `ss-34-secrets-backend` implementation landed the full set path;
the normal app path is not a `not_supported` set state. The exact path is:

1. The Settings value field is a controlled `type="password"` input. On submit the card
   copies the value into a local variable and clears the input state **before** the first
   `await`. The value is never rendered as text, logged, persisted or put in an error.
2. The Tauri `secrets_set` command immediately moves the value into zeroizing storage
   (`SecretString`) and clears the original string. Core validates the name against
   `^[A-Z][A-Z0-9_]{0,63}$`, requires a non-empty value of at most 4096 bytes, and rejects
   LF, CR and NUL. An invalid request never reaches an SSH command.
3. Core builds the remote argv as exactly `vowp --json secret set <NAME>`. The value is not
   an argv element and is not added to the child environment. This one command uses
   `ssh -tt` to force a remote pty, while keeping `BatchMode=yes`,
   `StrictHostKeyChecking=yes`, the app-private known-hosts file and `IdentitiesOnly=yes`
   when an identity is selected.
4. StackSpirit waits for VOWP's exact hidden prompt `value for <NAME>: `, which VOWP prints
   only after its `stty -echo` guard has run. Only then does it write `<VALUE>\n` to stdin.
   It never writes before the prompt.
5. Captured pty output is stripped of terminal control sequences, and only the final JSON
   object is parsed. A successful result is `{name,last4}`; a name mismatch is a typed
   `vowp_incompatible` error. Failures are classified by exit code and never embed raw
   stderr or the value. The value is zeroized when the request is dropped.
6. The card shows `stored NAME, ends in …last4`, refreshes the list from VOWP and clears the
   form. On failure the typed error is shown, the value stays cleared, and the form remains
   usable.

If a future host or VOWP build reports `set` as unsupported, the card disables the store
form and shows the exact reason, while leaving list and revoke usable. In the current
implementation, the prompt-gated set path above is available.

## Threat model and limits

### In scope for StackSpirit's design

- Keeping secret values out of argv, the environment, StackSpirit logs, StackSpirit errors,
  Tauri events and the assistant's context.
- Requiring an explicitly verified SSH host key before a profile is used.
- Making server-secret storage explicit: a connected host, a first-use warning, an explicit
  acknowledgement and a visible list/revoke path.
- Reporting unknown or unsupported states honestly instead of guessing.

### A compromised Mac

An attacker with the owner's macOS/Linux account (or admin access) can read the OS keychain
entries available to that account, use the app-owned SSH key, run `ssh` or `vowp` directly,
read StackSpirit's local data, and ask the app to perform actions. StackSpirit does not
defend against a compromised Mac. OS account separation, disk encryption and a locked
screen are the boundary.

### A compromised server

An attacker with root or the VOWP runtime user on the server can read or replace the vault,
change VOWP's behaviour, inspect commands and use any stored value. Host-key pinning proves
that the server offering the key is the one previously confirmed; it does not prove that the
server is uncompromised. A server that keeps the same host key but is under someone else's
control is still compromised.

### A host key changed after pinning

SSH refuses the connection and StackSpirit reports `host_key_mismatch`. Do not re-pin unless
the new fingerprint has been verified out of band (provider console, server console or a
known-good channel). Re-pinning to make an error go away defeats the pin.

### The vault at rest in VOWP 0.4.0

VOWP 0.4.0 does **not** encrypt the secret store at rest. Basic mode writes a mode-`0600`
file owned by the VOWP runtime user. Vault-user mode moves the files to a separate Unix
account, but the sudoers helper can read and write any stored name, so code that can invoke
the helper can reach every stored secret. Unix permissions and disk encryption are the
boundary, not encryption by VOWP. VOWP 0.5 is the planned upgrade that adds encrypted
storage at rest and a clean storage-mode report. StackSpirit does not claim the current
vault is encrypted and does not run `vowp secret upgrade` without the owner.

In particular, on a basic-mode server a harness or agent running as the same runtime user
can read the secret files directly. StackSpirit's UI and bridge never hand the AI a value,
but that does not make a basic-mode server safe from an agent with shell access. VOWP's
exact-value redaction covers the paths it documents (session events, the snapshot,
supervisor logs and command-gate evidence); a transformed or encoded copy is not guaranteed
to be caught.

### Export packages

StackSpirit does not claim that `.asif.zip` export packages are secret-free. They can contain
session content, repository data and other sensitive material. Treat an export as sensitive
and do not publish it without reviewing it.

### Local provider keys

Local provider keys live in the OS keychain and are used by the local engine over loopback
for verification and model calls. Another process with access to the same OS account may be
able to reach the keychain entry depending on the platform's keychain policy. The key is not
a hardware-backed secret by default.

### Metadata and traffic

Host labels, provider names, secret names, timestamps and storage modes are metadata, not
values, but they can still be sensitive. SSH protects traffic in transit. Provider API
traffic is protected by the provider's own transport, not by StackSpirit. VOWP and the
server may keep their own logs and backups; StackSpirit does not control them.

### No liability promise

The MIT licence provides StackSpirit **"as is"**, without warranty. This document explains
the design; it is not a legal guarantee, a service-level agreement or an insurance policy.
A paid service would need its own terms reviewed by a lawyer.

## Reporting a vulnerability

Please report suspected vulnerabilities privately. Do not open a public issue with exploit
details, a real secret, a live hostname or a token.

- Use the repository's private security-advisory channel (for example GitHub Security
  Advisories) or the owner's documented private contact. If neither is available, ask the
  owner for a private channel before sharing details.
- Include the StackSpirit version or commit, the operating system, the VOWP version, the
  steps to reproduce and the impact you expect.
- Refer to secrets by **name** only. Redact values, tokens, hostnames and paths from logs and
  screenshots.
- Reports are handled on a best-effort basis. There is no bug-bounty programme and no
  response-time SLA.

## Supported versions

- **StackSpirit:** the current `main` branch and the latest `0.1.x` pre-release. This is
  pre-release software; security fixes land on `main` and in the next release. Older
  pre-release builds are not supported.
- **VOWP:** `0.4.0` is the tested server contract for the host-key, onboarding and
  `vowp secret ls/set/rm` behaviour described here. A later `0.4.x` release is expected to
  work but is not guaranteed until it is tested. Older VOWP builds that lack the JSON
  `vowp secret` commands are not supported for the Server secrets card.
- **Surfaces:** the desktop app is the supported surface for server secrets. The read-only
  Web shell does not expose the secret store. The preview/mock bridge never stores a secret;
  it reports that the desktop host is required.
