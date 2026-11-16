# The seven-day soak runbook

> **Status: current; owner-deferred.** Every command on this page exists in this tree. The seven-day
> soak itself has **not** been run: the owner will run this runbook on their Mac and home when they
> choose, and until then the results table in [§6](#6-results-table) remains at `not run`. No number
> is claimed here, and no pass may be written without an observed value in the same row.

This is the operator procedure for the soak in
[`docs/design/v0.4/hardening.md`](../design/v0.4/hardening.md#83-soak) §8.3: one home, one Mac node
and seven days, with SSH and one WSS window. The security review (§8.2) and the rollback drill
(§8.4) are recorded in [`docs/releases/v0.4-hardening.md`](../releases/v0.4-hardening.md); this page
turns their shape into a run the owner can execute and record. It is a runbook, not a result.

The run has two rules:

1. **Evidence over claims.** For every row, keep the command, its exact output, the UTC date, the
   recorded commit and the number you measured. A `pass` needs the number.
2. **Stop instead of guessing.** A failed or inconclusive measurement stays `not run`; follow
   [§7](#7-failure-handling) and open an inbox question.

## 1. Topology and prerequisites

| Item | The soak run |
|---|---|
| Home | One VPS home with the supervisor, the global database and the session logs, reached as `<home-ssh-target>`. |
| Node | One Mac node named `<node>`, with its own node key, harness login and provider environment. |
| Duration | Seven consecutive days and nights, recorded as a start and end date in UTC. |
| Transports | SSH stdio for the run and exactly **one** WSS window ([§3](#3-the-wss-window)). |
| Push project | `<project-a>` with `[offload] result_transport = "push"` (the default). |
| Bundle project | `<project-b>` with `[offload] result_transport = "bundle"`. |
| Commit | `<soak-commit>`, recorded on Day 0 from the home checkout and the node checkout. |
| Owner-only | The Mac, its Wi-Fi, the TLS proxy with its DNS name and certificate, the pre-phase backup and the previous binary. |

Placeholders used below:

| Placeholder | Value |
|---|---|
| `<home-ssh-target>` | The SSH destination of the home, for example `<user>@<vps>`. |
| `<home-checkout>` | The home's checkout of this repository, used for the commit and the two project registrations. |
| `<mac-project-a>`, `<mac-project-b>` | The two project checkouts on the Mac. |
| `<project-a>`, `<project-b>` | The two project names in their `vowp.toml` files. |
| `<node>` | The Mac node name, for example `mac-studio`. |
| `<home-node>` | The home's local node name, `home` in a default install. |
| `<session-id>` | The session of the offloaded run under measurement. |
| `<surface-id>` | The Mac surface under measurement. |
| `<job-a>`, `<job-b>` | The newest offloaded job in each project. |
| `<backup-job>` | A job id present in the pre-phase backup used by the rollback drill. |
| `<soak-commit>` | The commit recorded on Day 0. |
| `<home-host>` | The public host name the WSS proxy serves. |

Before Day 0:

- The home is initialised and both projects are registered: `vowp project ls` lists `<project-a>`
  and `<project-b>`.
- The Mac checkouts exist at the commit under soak, with the same project names in their
  `vowp.toml` files. `<project-a>`'s node checkout has push access to the project remote;
  `<project-b>`'s node checkout needs read/fetch access to the base commit only, because its result
  is a bundle.
- The Mac has its own harness login and provider environment. VOWP never copies the owner's login,
  provider key or git credential to a node.
- For the rollback drill: a pre-phase (schema v6) backup of the home and the pre-phase binary,
  copied into a temporary directory when the drill starts. The drill never opens the live home.
- For the WSS window: a reverse proxy the owner operates, a certificate for `<home-host>` and a
  DNS record. VOWP never runs ACME, stores a proxy key or writes proxy configuration.
- The evidence refers to secrets by **name** only. No value, token, key or code goes in a command,
  a log line or the results table.

**Owner-only steps.** The Mac may need the owner physically or at its console: taking Wi-Fi down,
killing the agent process, and logging in after a reboot. The home's proxy, DNS and certificate
are also the owner's. Those steps are marked **Owner action (Mac)** or **Owner action (proxy)**.

## 2. Day 0 setup

Run the steps in order. Record every command and its output in the run log; the run header at the
end of this section is the summary.

### 2.1 Build and install the same binary

On the home:

```sh
cargo build --release --workspace
./scripts/install.sh --bin target/release/vowp
vowp version
```

On the Mac, build the same commit and put `target/release/vowp` on the agent account's `PATH`
before `vowp agent service install`. The service unit captures the installing shell's `PATH`;
a binary outside it is not found after a reboot.

```sh
cargo build --release --workspace
install -m 0755 target/release/vowp ~/.local/bin/vowp
command -v vowp
vowp version
```

Build both from the same commit. `vowp version` prints the version and, when run inside a
checkout, that checkout's short HEAD; record the full `git rev-parse HEAD` in the run header on
both machines. A binary and a checkout from different commits are different runs.

### 2.2 Confirm the two projects on the home

```sh
vowp project ls
vowp status
```

`project ls` must list `<project-a>` and `<project-b>` as active. If a supervisor is already
running, do not start a second one; `vowp status` reports the pid.

### 2.3 Enroll the node over SSH

On the home, create the node and the one-time code:

```sh
vowp node enroll <node> --label mac
```

The command prints the 52-character code once and the matching `vowp agent enroll` line. The code
expires after ten minutes. On the Mac, enroll with both project checkouts and paste the code on
stdin:

```sh
vowp agent enroll --target <home-ssh-target> --code-file - \
  --project <mac-project-a> --project <mac-project-b>
vowp agent paths ls
vowp agent status
```

`--code-file -` keeps the code out of `argv`, `ps` and the log. Enrollment writes
`~/.vowp-agent/agent.json` and the node key, then performs the
`AgentHello` → `HomeChallenge` → `AgentAuth` → `HomeWelcome` handshake. Record the node name, key
fingerprint, protocol and the two mapped projects.

The WSS form of the same enrollment is used only in the WSS window ([§3](#3-the-wss-window)):

```sh
vowp agent enroll --wss wss://<home-host>/agent/v1 \
  --code-file - --project <mac-project-a> --project <mac-project-b>
```

A self-signed deployment adds `--tls-fingerprint sha256:<64-lowercase-hex>`; a mismatch is a
refusal (exit 5), never a warning. The URL must be exactly `wss://host[:port]/agent/v1`.

### 2.4 Install the service on the Mac

These commands run on the Mac as the agent account and use the real per-user service manager;
they never use sudo.

```sh
vowp agent service install --print
vowp agent service install --now
vowp agent service status
```

`--print` writes nothing and invokes no manager; it prints the unit text, the unit path and the
manager commands. `--now` writes exactly one unit and runs the current user's manager commands.
Record the unit path, whether it is loaded, the reported pid and the last exit status. The unit
contains no token, code, key, provider value or absolute project path.

### 2.5 Enable offload for the two projects on the home

Each project's `vowp.toml` is a separate file. For `<project-a>` (push, the default):

```toml
[offload]
enabled = true
default = "any"
result_transport = "push"

[offload.projects.<project-a>]
placement = "any"
```

For `<project-b>` (bundle):

```toml
[offload]
enabled = true
default = "any"
result_transport = "bundle"

[offload.projects.<project-b>]
placement = "any"
```

Then print the effective policy and the read model:

```sh
vowp --project <project-a> offload policy
vowp --project <project-b> offload policy
vowp offload status
```

Record `enabled`, `default`, `max_remote_runs`, `grace_secs`, `result_transport`, the project rows
(`placement`, `home_only`, `allow_secrets`) and the node row (`max_workers`, `allow_battery`,
`labels`). The two transports are per assignment and neither side falls back: a bundle run must
not push, and a push run must not send a bundle.

### 2.6 Start the supervisor and record the run header

```sh
vowp up
vowp status
git -C <home-checkout> rev-parse HEAD
vowp version
date -u +%Y-%m-%dT%H:%M:%SZ
```

`vowp up` exits 4 if a supervisor is already running; use the running one and record its pid. The
node reconnects to the running supervisor on its own.

Record the run header before the first daily check:

| Field | Record |
|---|---|
| Soak commit | `<soak-commit>` from `git -C <home-checkout> rev-parse HEAD`. |
| Day 0 (UTC) | The `date -u` output. |
| Day 7 (UTC) | Filled at the end of the seventh day. |
| Home | `<home-ssh-target>` and the `vowp version` line. |
| Node | `<node>` and the `vowp version` line on the Mac. |
| Transports | SSH stdio; one WSS window with its start and end dates. |
| Projects | `<project-a>` push, `<project-b>` bundle. |
| Offload policy | The three outputs from §2.5. |
| Results | `not run` until a measurement is written. |

## 3. The WSS window

Run exactly one WSS window, for example one day in the middle of the soak. SSH is the transport
for the rest of the run. The window exercises the optional path: the home's loopback listener, the
owner's TLS proxy and the `wss://` agent transport. The proxy is a trusted home component: it sees
the decrypted frames, including surface tokens and prompts. Keep its access logs free of frame
bodies; a compromised proxy is a home compromise. See
[the WSS trust boundary](../security.md#the-wss-trust-boundary).

### 3.1 Open the home listener

Add the ingress table to the home's `~/.vowp/config.toml`:

```toml
[ingress.wss]
enabled = true
listen = "127.0.0.1:8743"
path = "/agent/v1"
max_connections = 64
max_frame_bytes = 1048576
handshake_timeout_secs = 10
```

`listen` must be a loopback address and `path` must be exactly `/agent/v1`. Restart the
supervisor so it starts the listener child:

```sh
vowp up --replace
ss -ltn 'sport = :8743'
```

`--replace` drains the running supervisor and starts this binary. Record the listener address and
the supervisor pid.

### 3.2 Put the owner's proxy in front of it

**Owner action (proxy).** Terminate TLS 1.3 for `<home-host>`, forward only `/agent/v1` to
`127.0.0.1:8743`, and pass the WebSocket upgrade. The proxy owns the public port, the certificate
and TLS; VOWP binds only loopback. The Caddy and nginx shapes are in
[the WSS deployment guide](agent-node.md#wss-deployment). Do not put the proxy on shared
infrastructure and do not log frame bodies.

### 3.3 Switch the node to WSS

The home has no in-place transport switch: an existing node cannot be re-enrolled, and
`vowp node enroll` is exit 4 for an existing node. Switch at a quiet boundary with no active run.
The Mac keeps its node key, `agent.json` and surface tokens; `vowp node rm` removes only the home
row, its credential, its path map and its node limits. The design also names editing the stopped
agent's `~/.vowp-agent/agent.json` transport block as the fallback; the enrollment below is the
shipped command path.

1. **Mac node (owner; service manager):** stop the service and keep the state directory.

   ```sh
   vowp agent service uninstall --yes
   ```

2. **Home:** remove the row and create a fresh one-time code under the same node name.

   ```sh
   vowp node rm <node>
   vowp node enroll <node> --label mac
   ```

3. **Mac node (owner; service manager):** enroll with the WSS flags, re-map both projects and start the service.

   ```sh
   vowp agent enroll --wss wss://<home-host>/agent/v1 \
     --code-file - --project <mac-project-a> --project <mac-project-b>
   vowp agent service install --now
   vowp agent status --json
   ```

4. Record the `[agent.transport]` block in `~/.vowp-agent/agent.json` (kind `wss`, the URL and
   `tls = "system"` or `pin`), the first-connection fingerprint, the protocol and the connection
   state.

A later certificate that differs from the first recorded fingerprint is refused before the first
frame; the shipped recovery is to re-enroll. Record any fingerprint warning. There is no fallback
to SSH: a WSS agent whose proxy is down reconnects with backoff and spools events.

### 3.4 Close the window

Reverse the switch after the window, again with no active run:

1. **Mac node (owner; service manager):** `vowp agent service uninstall --yes`.
2. **Home:** `vowp node rm <node>`, then `vowp node enroll <node> --label mac`.
3. **Mac node (owner; service manager):** enroll over SSH again and restart the service.

   ```sh
   vowp agent enroll --target <home-ssh-target> --code-file - \
     --project <mac-project-a> --project <mac-project-b>
   vowp agent service install --now
   vowp agent status
   ```

4. **Home:** turn the ingress table off (`enabled = false` or remove the table) and run
   `vowp up --replace`. Confirm `ss -ltn 'sport = :8743'` no longer lists the listener.
5. **Owner action (proxy):** stop or disable the proxy route for `<home-host>`. The listener is
   loopback-only, so a live proxy route without it fails closed; close it anyway to end the window.

Record the window's start and end dates and the transport evidence in the run header.

## 4. Daily checks

Run these once a day, at roughly the same time, and write one line per check in the run log. Keep
the command output, not only the summary.

| Check | Where | Command | Record |
|---|---|---|---|
| Supervisor and jobs | Home | `vowp status` | Supervisor pid and uptime, jobs by state, slots busy/total, cost today, open inbox. |
| Fleet | Home | `vowp fleet status` | Snapshot source and staleness, supervisor, projects running/ready/blocked, capacity brake, limits, inbox. |
| Node card | Home | `vowp node show <node>` | Kind, status, presence, labels, mapped projects, agent version, protocol, last error, last seen. |
| Capacity history | Home | `vowp capacity history --since 7d --bucket 1h` | Oldest and newest `AT`, number of points, any gap, the `window:` footer. |
| Agent connection | Mac node | `vowp agent status` | Connection, last heartbeat, pending spool bytes, key fingerprint, earliest token expiry. |
| Surfaces | Home | `vowp surface ls` | Each surface's state, `PREFIX`, `EXPIRES`, `OVERLAP`/`PENDING`, and `agent status --json` on the Mac. |
| Event and seq counts | Home | `vowp session show <session-id>` | `events:` (`last_seq`) and, with `--json --events 1`, the last event's `seq` and `id`. |
| Index health | Home | `vowp reindex --session <session-id>` | `events`, `torn tails` and `problems`. A torn tail or a problem is a finding. |
| Secret scan | Home | `vowp job show <job-a> --gates` and `vowp job show <job-b> --gates` | The `secret_scan` line and its evidence tail for the newest offloaded job in each project. |
| Service | Mac node (service manager) | `vowp agent service status` | Unit path, loaded, pid, last exit status. |
| Offload read model | Home | `vowp offload status` | Node presence, mapped, slots, disk, battery state, last error; live assignments; ready jobs and every waiting reason. |
| Log and backup | Home | `vowp backup` and `tail -n 100 ~/.vowp/logs/supervisor.log` | Newest backup directory; any `WARN`/`ERROR` line and its target. |

`vowp agent doctor` on the Mac is a useful extra line: it reports the state directory, the service
unit, surface expiry and copied login material by name only. It is a warning list, not a gate.

A job whose gate list omits `secret_scan` is not evidence for §5.4; use the default gates or add
the gate before the run.

`vowp capacity history` is read-only and safe while the supervisor runs. A missing bucket is a
gap, not a zero: record `—`, never `0`.

## 5. Scenario measurements

Run each scenario once during the seven days. Keep the commands and the raw output; the results
table only receives a number that came from one of these runs. The numbering of the results table
matches the order below.

### 5.1 Forced disconnect: no lost event, duplicate seq or duplicate event id

Use a session that is writing: an offloaded job or a local agent chat turn on the Mac. The home
owns the log; the node spools before it sends.

1. **Home, before the disconnect:** record the last durable event.

   ```sh
   vowp session show <session-id> --json --events 1
   ```

   Record `last_seq` as `N` and the last event's `seq` and `id`.

2. **Owner action (Mac):** take the node offline for at least 30 seconds and well under the
   600-second run grace: turn Wi-Fi off, or stop the WSS proxy during the WSS window. Keep the job
   or turn running so the node writes to its spool. On the Mac, `vowp agent status` must show
   `connection: reconnecting` and a non-zero `pending_spool_bytes`.

3. **Restore the connection.** On the Mac, wait until `vowp agent status` shows
   `connection: connected` and `pending_spool_bytes: 0`. A spool that never drains is a finding,
   not a slow replay.

4. **Home, after the reconnect:** record the new head and re-run the index check.

   ```sh
   vowp session show <session-id> --json --events 1
   vowp reindex --session <session-id> --json
   ```

Record `N`, the new `last_seq` `M`, the delta `M - N`, the new last event id, `torn_tails` and
`problems`. **Pass** when the spool is empty, `M` is greater than `N`, `torn_tails` is `0` and
`problems` is empty. The reindex reader reports a gap or a duplicate sequence as a skipped session
in `problems`; a duplicate event id makes the command exit nonzero with a conflict naming the id.
Either outcome is a finding. A lost event that never drained shows as an empty spool with a delta
below the work the node performed, which is an inconclusive measurement, not a pass.

### 5.2 Takeover read-only under 2 seconds with a capsule

Preconditions: a live Mac surface holds the lease on `<session-id>` with no turn running (for
example `vowp chat <session-id> --agent` in a Mac terminal), and a second surface can run on the
home or phone. A running turn is drained for up to five seconds by design; wait for it before
this measurement.

1. **Home:** take the lease with the hidden interface and time the command with a wall clock.

   ```sh
   date -u +%Y-%m-%dT%H:%M:%SZ
   vowp session takeover <session-id> --kind chat --holder phone --node <home-node>
   date -u +%Y-%m-%dT%H:%M:%SZ
   ```

   The hidden `session takeover` is the implementation interface; the user path is
   `vowp chat <session-id> --takeover`. The command returns after the lease has moved.

2. **Mac node:** the old surface prints
   `Taken over by phone at HH:MM. This surface is now read-only (follow mode).` Attempt one write
   and confirm it is refused. Record the notice and the refusal.

3. **Home:** record the capsule the takeover wrote.

   ```sh
   vowp session show <session-id> --capsule
   ```

Record the two timestamps and their difference, the Mac notice, the refusal and the capsule id,
trigger `takeover` and `at seq`. **Pass** when the difference is under 2 seconds and the capsule
exists. A takeover that waits for a turn to drain for five seconds is over the budget; keep the
number.

### 5.3 A 1,000-event replay under 10 seconds

This measures the agent's spool replay after a reconnect, not a reindex. Use a run that can produce
at least 1,000 events; a long test or build job is the reliable source.

1. **Home, before the disconnect:** record the session head.

   ```sh
   vowp session show <session-id> --json --events 1
   ```

   Record `last_seq` as `N`.

2. **Owner action (Mac):** disconnect the node and keep the job or turn running until it has
   produced at least 1,000 events. The node keeps working; the home sees nothing.

3. **Restore the connection and time the drain.** Start one clock at the moment you restore the
   connection (network on, or the WSS proxy up) and stop it when the Mac shows
   `pending_spool_bytes: 0` and the home head has advanced by at least 1,000.

   ```sh
   # on the Mac, the moment you restore the connection:
   date -u +%Y-%m-%dT%H:%M:%SZ
   # on the home, poll until the head reaches N + 1000:
   vowp session show <session-id> --json
   # on the Mac, when the spool reaches 0:
   vowp agent status
   date -u +%Y-%m-%dT%H:%M:%SZ
   ```

   On the Mac, `vowp agent status` must reach `pending_spool_bytes: 0`. On the home,
   `vowp session show <session-id>` must reach at least `N + 1000`.

Record the start and end timestamps, the elapsed seconds, `M - N` and the spool state. The
interval includes the agent's reconnect backoff, so it is an **upper bound** on the replay: a
value under 10 seconds is a pass. A value over 10 seconds is inconclusive unless the agent's log
shows that the replay itself finished under 10 seconds; do not subtract a guessed backoff. If the
delta is below 1,000, the run did not produce the required replay: repeat it, and do not write a
pass.

### 5.4 `secret_scan` clean and no home-only secret in a frame or spool

1. **Home:** for the newest offloaded job in each project, print the gate results.

   ```sh
   vowp job show <job-a> --gates
   vowp job show <job-b> --gates
   ```

   Record the `secret_scan` result and its evidence tail. The gate stores only the file, line and
   rule name, never the matched text.

2. **Home:** record the project policy that decides whether a secret can leave at all.

   ```sh
   vowp --project <project-a> offload policy --json
   vowp --project <project-b> offload policy --json
   ```

   Record `allow_secrets` for both projects and `home_only`. A project that needs a home-only
   secret is not offloaded.

3. **Mac node:** record the local warning list, which reports copied login material by name only.

   ```sh
   vowp agent doctor
   ```

**Pass** when every offloaded diff's `secret_scan` line is a pass, both projects' `allow_secrets`
lists contain no home-only secret, and `agent doctor` reports no copied login. There is no command
that prints a frame or a spool body; the policy (a home-only secret never resolves on a node), the
gate result and the node's warning list are the available evidence. `secret_scan` does not catch a
transformed or encoded copy, so a value seen in a prompt or log is still a finding.

### 5.5 Offline rotation delivered on reconnect

Preconditions: `<surface-id>` is attached to `<node>` and its token is current.

1. **Home, before the disconnect:** record the surface token prefix and expiry.

   ```sh
   vowp surface ls --json
   ```

   Record `prefix` and `expires_at` for `<surface-id>`.

2. **Owner action (Mac):** take the node offline (Wi-Fi off, or stop the WSS proxy) and confirm
   on the Mac that `vowp agent status` shows `connection: reconnecting`.

3. **Home:** queue the rotation while the node is offline. `--wait 30s` exits 7 with
   `rotation_pending` when the node is still offline; that is the expected result, not a failure.
   `--grace 1h` keeps the old token valid for the reconnect window.

   ```sh
   vowp surface rotate <surface-id> --wait 30s --grace 1h
   vowp surface ls
   ```

   Record the exit code, the `rotation_pending` output, the new prefix, the new expiry, the
   old-token `overlap_until` and the `PENDING` deadline.

4. **Restore the connection.** On the Mac, wait for `connection: connected` and record the
   surface array:

   ```sh
   vowp agent status --json
   ```

   Record `surfaces[]` for `<surface-id>`: `prefix`, `expires_at` and `state`.

5. **Home, after the reconnect:** record the home's view of the same surface.

   ```sh
   vowp surface ls --json
   ```

**Pass** when the home's `surface ls` prefix and expiry match the Mac's `agent status` prefix and
expiry exactly, the Mac's `surfaces[]` state is `paired`, and the old token's overlap has not
expired. A mismatch, or a rotation that is still `PENDING` after the reconnect, is a finding.

### 5.6 Killed agent restarted; revoked agent not restarted

Run the kill first and the revoke last: a revoked node does not reconnect. Do this with no active
run; a crash mid-run is a separate failure test, not this service check.

1. **Mac node, before the kill:** record the service and the pid.

   ```sh
   vowp agent service status
   vowp agent status
   ```

   Record the unit path, the reported pid `P1` and the connection state.

2. **Owner action (Mac):** kill the process, not the service. SIGKILL is the crash path; SIGTERM
   is a clean drain and proves a different behaviour.

   ```sh
   kill -9 <P1>
   ```

3. **Mac node, after the manager restarts it:** record the new pid, the last exit and the
   connection.

   ```sh
   vowp agent service status
   vowp agent status
   ```

   Record `P1`, the new pid `P2`, the elapsed time, `last_exit` and the connection state. **Pass**
   when `P2` is different from `P1`, the unit is still loaded, and the agent reconnects. A service
   that stays stopped after a crash is a finding.

4. **Home:** revoke the node and check the card.

   ```sh
   vowp node revoke <node> --yes
   vowp node show <node>
   ```

   Record the `credentials_revoked` count and `status: revoked`.

5. **Mac node:** confirm the terminal state and wait past one restart interval.

   ```sh
   vowp agent status
   vowp agent service status
   ```

   Wait at least 30 seconds and run both commands again. **Pass** when the agent reports the
   revoked or stopped connection, the unit is not loaded or has no live pid, and the second check
   still shows no restart. The launchd `run` marker is removed and the systemd unit exits 5, so
   neither manager restarts it.

A revoked node is not re-enrolled by these steps. If the soak must continue, repeat the transport
switch in [§3.3](#33-switch-the-node-to-wss) with the `--target` SSH form.

### 5.7 A supervisor-stop gap visible in `capacity history`

Do this with no active run. The supervisor writes one rollup for each completed five-minute bucket;
a stop longer than one bucket leaves a gap, and a gap is not a zero.

1. **Home, before the stop:** record the recent history.

   ```sh
   vowp capacity history --since 1h --bucket 5m
   ```

2. **Home:** stop the supervisor and record the UTC time. Wait at least six minutes.

   ```sh
   vowp down --drain
   date -u +%Y-%m-%dT%H:%M:%SZ
   ```

3. **Home:** start it again, wait for a tick, and read the same window.

   ```sh
   vowp up
   date -u +%Y-%m-%dT%H:%M:%SZ
   vowp capacity history --since 1h --bucket 5m
   ```

4. **Home:** record the seven-day coverage and the retention bound.

   ```sh
   vowp capacity history --since 7d --bucket 1h --json
   ```

Record the stop and start times, the missing `AT` values (the gap), the points on either side,
the oldest and newest `at`, `bucket_secs` and `retention_secs`. **Pass** when the gap matches the
stop window, every missing bucket is absent rather than zero, and the seven-day window is covered
within the 30-day retention.

### 5.8 The §8.4 rollback drill on a restored copy

This drill is **copy-only**. It never opens the live home, never starts a live supervisor, never
touches the live tmux server, never calls a service manager and never uses sudo. The first four
steps of §8.4 (stop the service, `down --drain`, revoke the node, turn off WSS and offload) are
covered by the scenarios above, the close-out in
[§5.9](#59-close-out-offload-off-and-wss-off) and the record in
[`../releases/v0.4-hardening.md`](../releases/v0.4-hardening.md); the copy-only restore and the
previous-binary checks are repeated here because they are the part the soak must prove on the
owner's own data.

Set up a private drill root. No tmux command runs in the drill; the private `TMUX_TMPDIR` exists
anyway, so even an accidental tmux call cannot reach the live server.

```sh
DRILL=/tmp/opencode/v4-soak-drill
rm -rf "$DRILL"
mkdir -p "$DRILL"
export HOME="$DRILL/home" XDG_CONFIG_HOME="$DRILL/xdg" TMUX_TMPDIR="$DRILL/tmux"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$TMUX_TMPDIR"
unset TMUX TMUX_PANE
VOWP_NEW=$(command -v vowp)
VOWP_OLD=<path-to-the-pre-phase-binary>
cp -a <pre-phase-backup> "$DRILL/backup-v6"
$VOWP_NEW --home "$DRILL/home" --json agent service install --print
test ! -e "$XDG_CONFIG_HOME/systemd/user/vowp-agent.service"
```

`--print` writes nothing and invokes no manager; the `test` confirms it on Linux. On macOS check
the printed `~/Library/LaunchAgents/dev.vowp.agent.plist` path instead. A pre-phase backup that
does not exist is a blocked prerequisite, not a reason to run the drill against the live home.
The evidence page §6 records the `$VOWP_OLD` build from `7a47edb` (a detached temporary worktree
with a private `CARGO_TARGET_DIR`).

The restore command needs an initialised `--home` for its command context even though it reads no
store from it. Create a throwaway context home and project inside `$DRILL`, then restore:

```sh
mkdir -p "$DRILL/ctx-repo"
git -C "$DRILL/ctx-repo" init -q
cd "$DRILL/ctx-repo"
$VOWP_NEW --home "$DRILL/ctx-home" init --name soak-drill
$VOWP_NEW --home "$DRILL/ctx-home" backup \
  --restore "$DRILL/backup-v6" --to-home "$DRILL/home-v6-copy" --json
```

Verify the restored copy with the previous binary:

```sh
$VOWP_OLD --home "$DRILL/home-v6-copy" --project <project-a> status --json
$VOWP_OLD --home "$DRILL/home-v6-copy" --project <project-a> job ls
$VOWP_OLD --home "$DRILL/home-v6-copy" --project <project-a> job show <backup-job> --json
$VOWP_OLD --home "$DRILL/home-v6-copy" --project <project-a> board --once
$VOWP_OLD --home "$DRILL/home-v6-copy" --project <project-a> project ls
$VOWP_OLD --home "$DRILL/home-v6-copy" --project <project-a> graph --export -
printf 'Restored copy write check.\n' | \
  $VOWP_OLD --home "$DRILL/home-v6-copy" --project <project-a> job add \
    --id v03-restored --title "restored write" --kind build --brief -
```

The restore verifies every manifest hash and row count before it swaps the staged home into
`--to-home`. Record the restore report, the `status`, `job ls`, `job show`, `board --once`,
`project ls` and `graph --export -` output, and the new `job add` row. These v0.3 flows are the
"v0.3 flows still pass" evidence; they read the project checkouts named in the backup and write
only inside `$DRILL`.

Upgrade a second copy with the soak binary, then prove the previous binary refuses the v7 file
without changing it:

```sh
cp -a "$DRILL/home-v6-copy" "$DRILL/home-v7-copy"
$VOWP_NEW --home "$DRILL/home-v7-copy" --project <project-a> status --json
sha256sum "$DRILL/home-v7-copy/vowp.db"
$VOWP_OLD --home "$DRILL/home-v7-copy" --project <project-a> status --json
sha256sum "$DRILL/home-v7-copy/vowp.db"
```

The first `sha256sum` is before the old binary; the second must be identical. The old binary is
expected to exit 1 with a `database schema version 7 is newer than supported 6` error. Record
both hashes, the exit code and the error text. The automatic pre-migration backup under
`$DRILL/home-v7-copy/backups/` is the rollback file; record its name.

**Pass** when the restore report verifies, every v0.3 read flow and the write flow pass on the
restored v6 copy, the previous binary refuses the v7 copy, and the v7 file's hash is unchanged.
No live home, supervisor, tmux server, service manager or sudo command is used.

### 5.9 Close-out: offload off and WSS off

After the last daily check, turn both optional features off on the live home and record it. This
is the §8.4 step 4 evidence; it is not a results row.

1. **Home:** set `enabled = false` in the `[offload]` table of both `vowp.toml` files.
2. **Home:** apply the change and confirm the effective policy.

   ```sh
   vowp up --replace
   vowp --project <project-a> offload policy
   vowp --project <project-b> offload policy
   vowp offload status
   ```

   Record `enabled: false` for both projects and no live assignment in the read model.

3. **Home:** close the WSS window if it is still open, following [§3.4](#34-close-the-window).
   Confirm `ss -ltn 'sport = :8743'` lists no listener.
4. Record the Day 7 UTC date in the run header and the close-out outputs in the run log.

## 6. Results table

One row per §8.3 criterion. Fill a row only after the scenario has run. `Observed` holds the
number and the evidence pointer; `Result` starts at `not run` and may become `pass` or `fail`.
A `pass` without a number in `Observed` is invalid.

| # | §8.3 criterion | Observed | Date (UTC) | Commit | Result |
|---|---|---|---|---|---|
| 1 | Forced disconnect: no lost event, no duplicate `seq`, no duplicate event id. | not run | not recorded | `<soak-commit>` | not run |
| 2 | Takeover makes the old surface read-only in under 2 seconds and writes a capsule. | not run | not recorded | `<soak-commit>` | not run |
| 3 | A 1,000-event replay finishes in under 10 seconds. | not run | not recorded | `<soak-commit>` | not run |
| 4 | `secret_scan` finds nothing on any offloaded diff; no home-only secret is in a frame or spool. | not run | not recorded | `<soak-commit>` | not run |
| 5 | A rotation queued while the node is offline arrives on reconnect; the `surface ls` prefix/expiry matches the agent's status. | not run | not recorded | `<soak-commit>` | not run |
| 6 | A killed agent is restarted by the service; a revoked agent is not restarted. | not run | not recorded | `<soak-commit>` | not run |
| 7 | Rollup rows cover the seven days within the retention bound; `capacity history` reports the stopped-supervisor gap. | not run | not recorded | `<soak-commit>` | not run |
| 8 | The rollback drill completes on a restored copy and the v0.3 flows still pass. | not run | not recorded | `<soak-commit>` | not run |

Record the raw command output next to the table, for example
`/tmp/opencode/v4-soak-evidence/<n>-<date>.txt`. The table is the summary; the raw file is the
evidence a verifier reads.

## 7. Failure handling

When a check or a scenario does not produce the required number:

1. **Stop the measurement.** Do not continue to the next step of that scenario.
2. **Capture the evidence before changing anything:** the command, its full output, the UTC time,
   the commit, the node and session, and any service or log state. Do not edit the log or the
   databases to make a check pass.
3. **Do not write a pass.** Leave the row `not run` if the measurement is inconclusive; write
   `fail` with the observed number if it ran and missed the criterion.
4. **Stop the soak** if the finding is a lost event, a duplicate event id, a secret value in a
   frame or spool, or a node that cannot be revoked. Keep the home and its logs for inspection;
   do not rotate or delete the evidence.
5. **Open an inbox question** with the finding, the evidence pointer and the question for the
   owner:

   ```sh
   vowp --project <project-a> inbox ask "soak: <criterion> failed" --kind question \
     --body "<observed value, command, date, commit, evidence path>"
   ```

   The item is per project; `vowp fleet inbox` reads every project plus the node inbox. Record its
   id in the run log.

The soak is not a green light for a release. The release gates in §8.5 still apply, and the
recorded evidence page keeps its own status until the owner fills the table.

## 8. What this runbook does not claim

- It does not claim the soak was run, that any criterion passed, or that a number is known.
- It does not replace the security review, the rollback drill or the release gates.
- It does not promise a date, a tag or a release.
- It does not copy a credential to the node, put a token in a URL, or put a secret value in a
  command, a log line or a results row.

Next: [The agent node](agent-node.md) for enrollment, service, rotation, WSS, bundle and capacity
history; [Operating a home](operating.md) for the supervisor, backups and logs; or the
[security model](../security.md) for the trust boundaries.
