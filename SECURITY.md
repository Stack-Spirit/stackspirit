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

- Local model provider keys used by the StackSpirit Engine are stored by the desktop app's
  `PrivateKeyStore` in `<app-data>/engine/state/provider-keys.json`: an atomically written
  JSON file with mode `0600` inside a directory with mode `0700`. Key material is held in
  `Zeroizing` buffers while in memory and error messages never contain a key value. This
  file is not encrypted at rest (see [Local provider keys](#local-provider-keys)). Keys are
  not written to the engine config, the VOWP home or a session log. The desktop app does
  not use the macOS Keychain for provider keys, so it triggers no Keychain prompts for them.
- The SSH app-key passphrase is a separate secret and is still kept in the local OS keychain
  under `ssh-key:<name>` (see [App-owned SSH key](#app-owned-ssh-key)).
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

### Web/PWA account session

- The optional account route is `/cloud`; the `/` daemon cockpit still needs no account and
  sends no `Authorization` header to the daemon.
- The OAuth callback arrives as `#auth_token=<token>`. `consumeAuthTokenFragment` reads it
  once in the auth panel's first-render state initializer, validates the opaque bearer
  pattern and calls `history.replaceState` with only the path and query, so the fragment is
  removed from the address bar and history before any view renders. A malformed
  `auth_token` is scrubbed the same way and is not stored.
- A valid token lives in `apps/spirit-web/src/auth/session.ts` module memory only, for the
  lifetime of the page. It is never written to `localStorage`, `sessionStorage`, a cookie,
  a log, an error message or a rendered field. The auth panel receives the parsed value in
  memory to load the account; it does not display it. Sign-out and cloud-runtime disposal
  clear it.
- The token is attached only as `Authorization: Bearer` to explicit StackSpirit API
  requests. Those requests use `credentials: "omit"`, `redirect: "error"` and
  `cache: "no-store"`. The account token is never sent to the local daemon or used for
  `/api/v1` reads.
- The older no-browser-token rule still holds for the daemon `VOWP_API_TOKEN`, provider
  keys and SSH material: the no-account cockpit holds none of them. The account bearer
  token is a separate cloud credential used only against the StackSpirit API.
- The one-time machine-enrollment token is a different credential. The onboarding card
  deliberately renders it once, with its expiry, so the owner can paste it into
  `relay enroll`; it is not the account session token.
- Limits: memory-only keeps the token out of browser storage and history, but any script
  running in the page origin, a browser extension, or a person with the open browser
  profile can read page memory while the session lives. A reload or PWA restart loses the
  session and the owner signs in again. See [`docs/WEB.md`](docs/WEB.md) §11.2.

### Cloud account and relay

- The account client is opt-in: constructing it makes no request and no socket, and
  account-free local and SSH mode works without signing in. Nothing calls the cloud until
  the owner starts sign-in or opens the plan catalogue.
- The Web `WebCloudClient` and the shared desktop `StackSpiritCloudClient` keep the access
  token and the pending device authorization in memory only and do not persist an access or
  refresh token. Device sign-in polls `/v1/auth/device/token` with the `device_code` only in
  the request body. The account, machine, plan and provisioning read models carry no bearer
  token, device code, relay ticket or provider key; the pending device view shows the user
  code and verification URL, not the polling code. None of the bearer token, refresh token,
  device code, relay ticket or provider key appears in a StackSpirit log, an error message
  or a rendered field.
- `spirit-core` also implements the refresh path. `CloudSession` wraps access and refresh
  tokens in `Zeroizing`, its `Debug` prints `[redacted]`, and `CloudRequest` /
  `CloudResponse` `Debug` prints header names and body lengths only. An opted-in
  `JsonCloudSessionStore` writes `<app-data>/cloud-session.json` atomically at mode `0600`
  in a mode-`0700` directory, refuses to read a symlink or a file wider than `0600`, and
  stores the access and refresh tokens as plain JSON. That file is not encrypted at rest.
  The current desktop cloud runtime uses the shared in-memory client, so it does not create
  that file.
- A relay ticket is short-lived (the client clamps it to 30–300 seconds) and scoped. Its
  request carries only public key material (a P-256 JWK), a public nonce, the client kind,
  a TTL and the requested scopes. The ticket is a credential held in client memory; it is
  not put in the WebSocket URL, logged or rendered. The client sends it in `client.hello`
  over WSS.
- After the relay hello, the channel is encrypted end to end: ephemeral P-256 ECDH,
  HKDF-SHA256, AES-256-GCM, and verification of the machine's Ed25519 signature. Relay
  HTTP request and response frames are sealed before they cross the relay; the initial
  hello carries the ticket and public handshake fields, not an SSH or provider credential.
  If the browser lacks the required WebCrypto primitives the relay fails closed; there is
  no plaintext fallback.
- Limits: the account API sees the account e-mail, machine names, presence and ticket
  requests. The relay sees connection and routing metadata and the ticket. End-to-end
  encryption protects relayed frame contents from the relay, not from a compromised
  browser, account or machine endpoint. Account e-mail and machine labels are personal
  metadata. This is not an anonymity or traffic-analysis-resistance claim. See
  [`docs/STRATEGY.md`](docs/STRATEGY.md) §4, [`docs/WEB.md`](docs/WEB.md) §11.5–§11.6 and
  the [`0.6.1` release note](runtime/docs/release/0.6.1.md); the optional account and relay
  shipped in [`0.6.0`](runtime/docs/release/0.6.0.md).

### Local MCP server

- `stackspirit mcp` is a local stdio server: newline-delimited JSON-RPC 2.0 over stdin and
  stdout. It opens no network port. The no-flag read-only server lists six read tools and
  answers every write tool with JSON-RPC `-32601`; the full server needs
  `--session`, `--surface` and `--token-file`, and the local agent proxy uses
  `--agent-socket` and `--surface`. This local stdio server is the shipped MCP surface;
  there is no hosted MCP endpoint in this tree.
- Session and fleet writes check the lease token. A moved session's old surface gets a
  `refused` failure with the takeover notice, and the write is not retried; read tools keep
  working after a lease loss.
- `fleet_dashboard` and the `ui://stackspirit/fleet-status` MCP Apps resource are
  zero-secret: they render the Home name, supervisor state, counts, host gauges, project
  names, job cards, brake details and cluster counts. They do not read or render a token,
  lease, secret name or value, SSH target or project path. Every dynamic HTML field is
  escaped; Markdown table and fence metacharacters are escaped or removed. The resource is
  self-contained HTML with inline CSS, no script and no remote fetch.
- The local agent proxy answers `initialize`, `server/discover`, `ping` and `tools/list`
  locally and forwards `tools/call` over the private agent socket. It never opens a store, reads a secret or
  prints an argument value, token or absolute local path. It keeps the fleet read tools and
  the session tools the Home executor can run, but hides `fleet_dashboard`, the tunnel and
  `secret_request` tools and the CLI-only control tools. It has no `resources` capability,
  so no MCP Apps widget is served through the proxy.
- Limits: the server inherits the OS account and process boundary. Anything that can run
  the CLI as that account, or attach to its stdio, can call the tools that server exposes.
  The full server's lease token gates session writes; it is not an OS sandbox. Tool results
  can contain fleet, project and session metadata and event text; the server does not read
  VOWP's secret-store values into them. See [`docs/MCP.md`](docs/MCP.md) and the
  [`0.6.1` release note](runtime/docs/release/0.6.1.md).

### Harness registry and one-click connect

- Owner-defined manifests live in `~/.stackspirit/harnesses/<id>.toml`. The file is written
  atomically with a mode-`0700` directory and a mode-`0600` file. The schema has no
  credential field, unknown fields are rejected, and free-text fields reject obvious
  secret material such as a private-key block or a provider key prefix. The registry stores
  no secret value.
- A probe asks only two safe questions, with an 8-second timeout: does `<binary> --version`
  answer, and does the harness's own non-interactive status command report a stored
  sign-in? StackSpirit runs only `opencode auth list`, `claude auth status` or
  `codex login status`; any other harness reports `unknown`. It never reads a credential
  file. The version line is control-character-stripped, redacted and capped at 120
  characters; auth details and warnings are fixed copy or redacted text. Probe results are
  path-free.
- The connect plan shows the install command but never runs it. An `auth_cmd` is parsed as
  shell-free argv; quotes and shell control operators are rejected, not interpreted. When
  a sign-in command exists, the desktop host spawns that child, reads the first `http(s)`
  URL it prints and opens it; otherwise it opens the harness documentation. The opener
  refuses anything except `https://` and loopback `http://127.0.0.1:`,
  `http://localhost:` or `http://[::1]:` URLs.
- A Home VPS sign-in that needs an OAuth callback gets a temporary
  `ssh -N -L 127.0.0.1:<port>:127.0.0.1:<port>` forward with the same app-private
  `known_hosts`, strict host-key checking, `BatchMode=yes`, identity selection and
  `ExitOnForwardFailure=yes` as every other remote command. Both the forward and the
  sign-in child are kept in host state and torn down when the sign-in finishes or the app
  exits. A second connect for the same harness and target is refused as a `conflict`.
- API-key and subscription sign-in are different products. The panel shows the vendor's
  consumer-terms note for a subscription; StackSpirit does not accept those terms for the
  owner, choose an auth mode, or turn a subscription login into an API key.
- Limits: Connect starts the harness's own sign-in, so the harness may store credentials in
  its own store; StackSpirit neither reads nor manages them. The loopback forward is
  reachable by local processes while it runs. A compromised OS account can read the
  harness's store or run its binary directly. See [`docs/HARNESSES.md`](docs/HARNESSES.md)
  and the [`0.6.1` release note](runtime/docs/release/0.6.1.md).

### Encrypted R2 backups

- `stackspirit backup push --recipient age1...` (or `STACKSPIRIT_BACKUP_RECIPIENT`) writes
  the normal verified home snapshot into a private mode-`0700` temporary directory, packs a
  path-safe archive, encrypts it locally with `age` to the public x25519 recipient, and
  uploads only the ciphertext to the per-machine R2 route. The upload carries a SHA-256
  header and is capped at 64 MiB. API authentication is the account session token in the
  `Authorization` header; the token is not placed in the URL or the archive. The add-on is
  off by default.
- Restore downloads the ciphertext, verifies its reported size, SHA-256 and `age` header,
  decrypts it locally with the private identity, and extracts the path-safe archive. The
  identity must be a regular mode-`0600` file, not a symlink and at most 16 KiB. The
  archiver skips symlinks; the extractor rejects absolute or parent-relative paths,
  duplicate paths, unsafe modes and trailing data, and verifies every file hash. The core
  restore then verifies file hashes and row counts before swapping the fresh home into
  place.
- The private identity is never uploaded or printed. The plaintext snapshot exists only in
  the private temporary work directory while the command runs, and that directory is
  removed when the command ends. R2 sees the ciphertext object name, size and timestamp
  and the machine id; it does not receive the plaintext or the age identity.
- Limits: anyone with the age identity and a downloaded ciphertext can decrypt it, so
  identity compromise defeats backup confidentiality. A process running as the same OS
  account can read the plaintext temporary snapshot while a push runs. R2 and the cloud
  operator can withhold or delete objects. The runtime CLI's `<home>/cloud-session.json`
  is also mode `0600` and is not encrypted at rest. See
  [`runtime/docs/release/0.6.0.md`](runtime/docs/release/0.6.0.md) and the
  [`0.6.1` release note](runtime/docs/release/0.6.1.md).

### What the AI sees

The StackSpirit bridge, renderer, error types and tests carry secret **metadata** only:
name, last four characters, timestamps and a reported storage mode. The AI never receives a
full secret value through StackSpirit.

The MCP dashboard and widget, harness probes, and cloud account and machine read models
follow the same rule: status, counts, names and public metadata, not a token, lease, secret
value, SSH private key or provider key. The full MCP server can return fleet data, session
events and a session lease, and `secret_request` returns a one-time paste URL; none of those
paths returns the secret-store value.

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
- Keeping account bearer and refresh tokens, device codes, relay tickets, harness
  credentials and backup identities out of logs, errors and rendered read models.
- Keeping the MCP dashboard and widget zero-secret and read-only, and uploading only age
  ciphertext for an encrypted backup.
- Reporting unknown or unsupported states honestly instead of guessing.

### A compromised Mac

An attacker with the owner's macOS/Linux account (or admin access) can read the OS keychain
entries available to that account (such as the SSH key passphrase), read the `0600`
provider-key and cloud-session files, read a signed-in browser's in-memory account token,
use the app-owned SSH key and the harness sign-ins, run `ssh` or `vowp` directly, read
StackSpirit's local data, and ask the app to perform actions. StackSpirit does not defend
against a compromised Mac. OS account separation, disk encryption and a locked screen are
the boundary.

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

Local provider keys live in `PrivateKeyStore`, the mode-`0600` file
`<app-data>/engine/state/provider-keys.json` in a mode-`0700` directory (atomic writes,
`Zeroizing` in-memory buffers, secret-free errors). They are passed to the bundled engine
only over loopback or ephemeral environment for verification and model calls, and are never
written to the engine config, the VOWP home or session logs.

Limits: any process running as the same OS account can read a mode-`0600` file, so this
protects against other OS users, not against the same account. The keys are not encrypted at
rest and are not hardware-backed by default; disk encryption and OS account separation are
the boundary (see [A compromised Mac](#a-compromised-mac)). An `OsKeychain` backend remains
available in `spirit-core` as an opt-in; the desktop app does not use it by default, and
keys previously saved to the Keychain are not automatically migrated, so they may need to be
re-entered and the old Keychain entries removed by the owner.

### Web account and cloud sessions

The Web account token is memory-only: that keeps it out of browser storage and history, not
out of page memory. A script in the page origin, a browser extension, a person at an open
profile or a compromised browser can read it while the session lives. A reload drops it, so
there is no persistent browser token to revoke; the server-side token lifetime still
applies. The core `cloud-session.json` store, when a host opts into it, is a plain
mode-`0600` file: any process running as the same OS account can read its access and
refresh tokens, and the file is not encrypted at rest.

The account API sees the account e-mail, machine names, presence and ticket requests; the
relay sees connection and routing metadata and the ticket. The relayed HTTP request and
response frames are end-to-end encrypted, but that protects the frame contents from the
relay, not from a compromised browser, account or machine endpoint. The one-time enrollment
token is deliberately shown once and is sensitive until it expires.

### MCP, harness and backup boundaries

The local MCP server has no authentication beyond the OS process and pipe that start it. A
process running as the same OS account can call the tools that server exposes; the full
server's lease token gates session writes but is not an OS sandbox against that account.
Harness probes read only binary and auth status, but Connect starts the harness's own
sign-in, whose credential store StackSpirit does not manage. The temporary loopback forward
is reachable by local processes while it runs.

Backup confidentiality depends on the age identity. Anyone who has the identity and a
downloaded ciphertext can decrypt it; a process running as the same OS account can read the
plaintext temporary snapshot while a push runs. R2 and the cloud operator see object names,
sizes, timestamps and the machine id, and can withhold or delete objects.

### Metadata and traffic

Host labels, provider names, secret names, timestamps and storage modes are metadata, not
values, but they can still be sensitive. Account e-mail, machine names and presence, relay
ticket metadata and backup object names, sizes and timestamps are also metadata. SSH and
the relay's TLS protect traffic in transit; relayed HTTP frames are additionally encrypted
end to end. Provider API traffic is protected by the provider's own transport, not by
StackSpirit. VOWP, the cloud API, the relay and R2 may keep their own logs and backups;
StackSpirit does not control them.

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

- **StackSpirit desktop:** the current `main` branch and the latest `0.1.x` pre-release.
  This is pre-release software; security fixes land on `main` and in the next release. Older
  pre-release builds are not supported.
- **StackSpirit runtime and CLI:** the `0.6.x` line; the current release is `0.6.1`
  ([release note](runtime/docs/release/0.6.1.md)). This document describes the `0.6.1`
  behaviour of the local MCP server, harness registry and encrypted R2 backups.
- **VOWP:** `0.4.0` is the tested server contract for the host-key, onboarding and
  `vowp secret ls/set/rm` behaviour described here. A later `0.4.x` release is expected to
  work but is not guaranteed until it is tested. Older VOWP builds that lack the JSON
  `vowp secret` commands are not supported for the Server secrets card.
- **Surfaces:** the desktop app is the supported surface for server secrets and harness
  connect. The no-account Web cockpit and the account/relay cockpit do not expose the server
  secret store. The local MCP server is stdio-only. The preview/mock bridge never stores a
  secret; it reports that the desktop host is required.
