# v0.2 acceptance — surfaces (T3)

This is the acceptance record for §13.5 tests **4 (surfaces, app changes and the
`vowp chat` fallback)** and **5 (SSH drop during a laptop-executor turn)**. It is
the T3 card `v2-t3-surfaces`.

**Result on the tested build: test 4 passes for the real opencode 2.0.28 surface
and the `vowp chat` fallback. Test 5 passes with one workaround: a real SSH
connection was killed mid-turn, the executor spooled 12 events, reconnected and
replayed them with no gap or duplicate. The Mac desktop-app UI and a physical
phone SSH app were not available to this Linux worker; the protocol those apps
use was exercised with the real opencode binary, and the remaining owner checks
are recorded under [Open items](#open-items).**

The run also found one product defect that affects remote execution: an explicit
`--project <name>` fails with `io: No such file or directory` unless the command
runs inside the checkout. `__exec` always sends the project name to the home, so
test 5 needed the workaround in open item 1; the spool/replay behaviour itself is
not affected.

## Environment

- Date: 2026-11-11 (UTC).
- Worktree: `job/v2-t3-surfaces`, based on `3c57f7c` (`origin/main` at the time).
- Binary under test: `cargo build -p vowp-cli --bin vowp`; it reports
  `vowp 0.1.0` (the package version during the v0.2 pre-release), SHA-256
  `320916762ddf7ed636c2bb2eaa1ed56ff1d8dedc49e9ef95cb82151f1463973c`.
- opencode: `opencode v2.0.28` (the version the S3 spike pinned). OpenSSH
  `9.6p1`.
- Test 4 ran on the Linux build host with a private `HOME`, `XDG_*` tree,
  `VOWP_HOME`, `TMUX_TMPDIR` and tmux socket. The opencode service used a private
  XDG tree under the orchestrator directory and a local fake OpenAI-compatible
  model on `127.0.0.1`; no provider key, real model, public port or real
  `~/.vowp` was used.
- Test 5 ran in the T0 rig: home container `vowp-acceptance-t3-home` (image
  `vowp-acceptance/vps:sudo`, `sha256:3c19867d4d633df0ca5f0dda7fa06300e69816a3857c1488be05184c915011ab`,
  Ubuntu 24.04) and the disposable phone client. The home was a temporary
  `/home/vowp/acceptance/t3/home`; `TMUX_TMPDIR` was
  `/home/vowp/.acceptance/tmux` and the socket was `vowp-acceptance-t3`. The host
  home, host `~/.vowp`, host tmux server, network and real harnesses were never
  used.
- The rig was removed after the evidence was collected with
  `bash scripts/acceptance/rig.sh down --name t3` (exit 0; containers, network
  and generated keys removed).

`V` below is the built `vowp` and `$ROOT` is a private temporary directory:

```sh
V=/path/to/target/debug/vowp
ROOT=/tmp/vowp-t3
mkdir -p "$ROOT"/{home,user,tmp,tmux,bin}
export HOME="$ROOT/user" TMPDIR="$ROOT/tmp" TMUX_TMPDIR="$ROOT/tmux"
export VOWP_HOME="$ROOT/home" VOWP_ACCEPTANCE_TMUX_SOCKET=t3
export PATH="$ROOT/bin:$PATH"
```

## Test 4 — surfaces: PASS, with owner-only UI checks

### 4a — one live opencode session from two devices

A real opencode orchestrator was created with the VOWP host. The project is a
throwaway git checkout; the opencode host reads a loopback fake model from its
private XDG config (the same `fake_openai.py` shape the S3 spike used):

```sh
mkdir -p "$ROOT/project" && cd "$ROOT/project"
git init -q -b main && printf 't3\n' > README.md
git add README.md && git -c user.name=t3 -c user.email=t3@example.invalid commit -qm init
$V init --name t3proj --no-framework --no-color
$V orchestrator new --name t3 --app opencode \
    --executor opencode:deepseek/deepseek-flash#max --no-color
```

The fake-model configuration is the one recorded in the
[S3 app-surface spike](../v0.2-spikes/app-surfaces.md) (provider `fake`,
model `fake-model`, loopback `127.0.0.1`). The chat checks in 4b and 4c used a
second throwaway T0 `hello` fixture registered as `t3app`. Its fake CLI harness was extended in the temporary
fixture only to answer the capsule writer with a valid `CapsuleDraft` and to
stream 12 lines for a prompt containing `T3 SLOW TURN`; it never calls a model
or the network.

`orchestrator new` started an opencode 2.0.28 managed service under
`$VOWP_HOME/orchestrators/t3/.opencode`, acquired the opencode surface lease and
printed join instructions with the real port and native session:

```text
orchestrator t3 (opencode, running) session 01M74EYV7C769RRX56WE4XAH4A
  desktop: 1. On the control machine: vowp tunnel open 't3' --remote-port 45489
           2. On the home: cd '<orchestrator>' && XDG_STATE_HOME='<private>' opencode pair
           3. Open the one-time link it prints (5 minutes, single use) ...
  phone:   ssh -t <home-ssh-target> '... opencode --server http://127.0.0.1:45489 --session <native>'
  chat:    vowp chat 01M74EYV7C769RRX56WE4XAH4A
```

The server was loopback-only (`ss` showed `127.0.0.1:45489`, not `0.0.0.0`),
and `opencode pair` printed the one-time link and the matching `ssh -L` command.
A private sshd was started on `127.0.0.1`, the pair link was redeemed through
the tunnel, and a phone-style opencode client continued the same native session:

```sh
ssh -N -L "127.0.0.1:$LPORT:127.0.0.1:$PORT" -p "$SSHPORT" "$USER@127.0.0.1"
# redeem the one-time link through 127.0.0.1:$LPORT
opencode run --server "http://127.0.0.1:$LPORT" --session "$NATIVE" \
    --model fake/fake-model --format json "turn two from the phone"
```

The desktop/API side sent `turn one from the desktop side`; the message list read
back through the phone tunnel contained both turns:

```text
user=2 assistant=2 texts=['hello from the fake local model', 'hello from the fake local model']
RESULT: PASS
LISTEN: loopback-only 45489
```

The pair link was treated as a secret: the evidence contains only the redacted
`/auth/connect/<code>` form, not the code or the redeemed token.

### 4b — moving from opencode to `vowp chat`, with the old app cut off

A fresh orchestrator (`t3b`) used the same real opencode host. Its app-side
conversation got one desktop turn, then the old app's VOWP write tool was called
through `vowp mcp` with the orchestrator's real surface id and lease token file:

```sh
$V mcp --session "$SID" --surface "$SURFACE" --token-file "$ORCH/lease.token" <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"capacity_set","arguments":{"target":2,"reason":"t3 before the app move"}}}
EOF
```

Before the move the write succeeded and recorded the MCP actor
(`session:<id>/surface:<id>`, `isError:false`). A `vowp chat` surface then took
the session over and switched the executor across the app boundary:

```sh
printf 'T3 SLOW TURN\n' |
  $V --home "$VOWP_HOME" --project t3app chat "$SID" \
     --takeover --executor cli:acceptance-hello
```

While chat held the moved lease, the same old-app MCP call was repeated. It was
refused **24 ms** after the request (well inside the 2 s bound) with exactly:

```text
refused: Taken over by acceptance-local at 04:57. This surface is now read-only (follow mode).
```

The session log recorded the whole move on one session id:

```text
events 28 seq_contiguous True
switch_reasons ['takeover', 'user']
capsules ['cap_01M74FFJR1RZD5DNYQSVGTGX91', 'cap_01M74FFKANTC3DRZMHZMAEEWDT']
handoff_messages 1 assistant 12
RESULT: PASS
```

The new chat turn's user message carried `note: handoff` and the capsule text;
the old opencode keeper stopped when its token no longer matched, and the new
app continued from the capsule. The old opencode app was not closed by VOWP (it
cannot close another vendor's window); its VOWP write channel is what becomes
read-only, exactly as §7.4 states.

### 4c — the `vowp chat` fallback drains a turn mid-flight

A project session with the fake CLI executor was started, then surface A began a
slow turn (12 lines over about 4 s). Surface B took over while A was still
running:

```sh
$V --home "$VOWP_HOME" --project t3app chat "$SID"          # surface A, slow turn
printf 'continue after the chat takeover\n' |
  $V --home "$VOWP_HOME" --project t3app chat "$SID" --takeover   # surface B
```

The log shows the drain and the continuation: the takeover switch wrote a
capsule, the old turn ended `interrupted`, surface B started turn 2 with the
handoff message and finished `end_turn`. Surface A printed the exact notice and
followed the rest of the session read-only:

```text
events 29
switch [{'reason': 'takeover', 'capsule_id': 'cap_01M74F7589R5BGFW3WQP4YFW2F', ...}]
capsules [{'at_seq': 7, 'capsule_id': 'cap_01M74F7589R5BGFW3WQP4YFW2F', ...}]
turn_ended [{'turn': 1, 'stop_reason': 'interrupted'}, {'turn': 2, 'stop_reason': 'end_turn'}]
RESULT: PASS

A stderr:
Taken over by acceptance-local at 04:52. This surface is now read-only (follow mode).
```

All sequence numbers were contiguous (`1..29`), so the mid-turn move did not
leave a gap or duplicate in the session log.

## Test 5 — SSH drop during a laptop-executor turn: PASS

### Setup

The home container was a temporary VOWP home with one fixture project
(`t3ssh`, the T0 `hello` fixture) and a surface-only node named `laptop`.
Start it with the T0 rig; inside the home container these commands use the
home's `vowp` and `H`:

```sh
bash scripts/acceptance/rig.sh up --name t3 --variant sudo --no-build
# inside the home container:
H=/home/vowp/acceptance/t3/home
vowp --home "$H" node add laptop -
```

The laptop side was the build host with its own temporary `VOWP_HOME`, a
private tmux directory and a private `VOWP_CONTROL_HOME` whose `control.json`
named the rig's SSH target and the home path:

```json
{"role":"control","home":"vowp-home","ssh_target":"t3home",
 "remote_home":"/home/vowp/acceptance/t3/home","created_at":"..."}
```

The home created a session with the fake CLI executor, acquired an executor
lease, and wrote the `turn-begin` grant (the grant file was mode 0600; its token
is never printed here):

```sh
# inside the home container
SID=$(vowp --home "$H" --project t3ssh --json session new \
        --title "t3 ssh drop" --executor cli:acceptance-hello \
        --cwd /home/vowp/projects/t3ssh | jq -r .id)
TOKEN=$(vowp --home "$H" --project t3ssh session lease "$SID" \
          --acquire opencode --holder laptop --node laptop)
# request.json carries {lease, prompt, executor, purpose, node, cwd}; cwd is the
# laptop checkout, not the home checkout.
vowp --home "$H" --project t3ssh --json session turn-begin "$SID" \
   --executor cli:acceptance-hello --purpose start --node laptop \
   --request request.json > grant.json
# copy grant.json (mode 0600) to the laptop, then run the executor there:
vowp --home <laptop-home> --project <laptop-fixture> __exec \
   --grant grant.json --spool-dir <laptop-spool>
```

The fake harness streamed 12 lines with 0.35 s between lines. On the laptop,
`vowp __exec` ran the harness and opened a real SSH connection to the home's
`vowp session ingest`; the executor-side spool was a private directory on the
laptop.

### The drop and replay

The SSH child was killed when the spool held three events, not at a process
boundary. `__exec` reconnected on the next event and replayed from the first
unacknowledged frame. The two SSH connections are visible in the wrapper log:

```text
DROP at spool_lines=3 ssh_pid=2267754
EXEC_EXIT=0
SSH_CONNECTIONS=2
{"ack_window":64,"done":true,"resumed":false,
 "run_id":"01M74F9VPZYTJHESGW1HNJMRFR",
 "session_id":"01M74F9VF199RWA8TWRT78QP2C"}
```

The home log and the executor spool agree exactly:

```text
session 01M74F9VF199RWA8TWRT78QP2C state idle last_seq 19 executor cli:acceptance-hello
events 19 assistant_messages 12
first ['ssh drop line 01', 'ssh drop line 02', 'ssh drop line 03']
last  ['ssh drop line 10', 'ssh drop line 11', 'ssh drop line 12']
RESULT: PASS

spool_events 12 summary 1 acked_events 12 summary_acked True
RESULT: PASS
```

The home's sequence numbers are exactly `1..19` (three framing events, 12
assistant messages, the turn end and the executor detach), and the 12 assistant
lines are unique. Nothing was lost in the killed connection and nothing was
applied twice.

### The project-lookup workaround

The first attempt failed before the harness started because
`$V --home "$H" --project t3ssh session ...` exits 1 with
`{"error":{"kind":"io","message":"io: No such file or directory (os error 2)"}}`
when run outside the checkout, while the same command without `--project` from
inside the checkout works. `__exec` always sends `--project <name>` to the
home, so test 5 used a throwaway `vowp` wrapper on the home that dropped the
flag and ran from `/home/vowp/projects/t3ssh`. This is a product defect, not a
rig or fixture issue; see open item 1.

## Open items

1. **Fix job: explicit `--project <name>` fails outside the checkout.**
   On the test home with project `t3ssh` registered at
   `/home/vowp/projects/t3ssh`:

   ```sh
   cd /home/vowp
   vowp --home /home/vowp/acceptance/t3/home --project t3ssh --json session ls
   # {"error":{"hint":null,"kind":"io","message":"io: No such file or directory (os error 2)"}}

   cd /home/vowp/projects/t3ssh
   vowp --home /home/vowp/acceptance/t3/home --json session ls
   # exits 0 and lists the session
   ```

   This makes `vowp session ingest --project <name>` fail when `__exec` reaches
   the home over SSH, so a laptop-executor turn cannot run without the
   workaround above. Suggested owner paths: `crates/vowp-cli/src/ctx.rs`
   (`resolve_project_root` / the home context) and/or
   `crates/vowp-store/src/db.rs` (`Store::open` / `open_with_project`). This
   was recorded by T2 as a related `project add` symptom; T3 shows it also
   breaks the remote-ingest path.

2. **Owner-only app checks.** The Linux worker cannot install the Mac opencode
   desktop app or a physical phone SSH app. The server, pair link, tunnel, Basic
   auth, same-session client and API were all exercised with the real 2.0.28
   binary, but these UI checks remain (about 5 minutes):
   - On the Mac: `vowp tunnel open t3 --remote-port <port>` and the printed
     `opencode pair` link; open the orchestrator project, send one message, and
     confirm the phone sees it.
   - On the phone: `ssh -t <home> 'cd <orchestrator> && opencode --server
     http://127.0.0.1:<port> --session <native>'` (the terminal UI); send one
     message and confirm the Mac sees it.
   - Optional: Claude Code Remote Control from the Claude desktop/mobile apps.
     This needs a claude.ai login on the home; the run had none, as S3 recorded.

3. **opencode MCP model exposure remains the §16.20 open assumption.** The
   project-local `opencode.json` registers `vowp mcp` and a scripted MCP client
   can call its tools; the old-app refusal in 4b used that real stdio server.
   The run did not prove that opencode 2.0.28 offers those tools to a model,
   because the fake model's tool list in the S3 spike did not include them. The
   tool-capable fallback is `vowp chat`; Claude Code needs the one-time
   `.mcp.json` approval.

## Reproduction summary

- Test 4: create a real opencode orchestrator with a private `XDG_*` tree,
  point its global config at a loopback fake OpenAI-compatible model, send a
  desktop/API turn, redeem a fresh `opencode pair` link through a private SSH
  tunnel, and continue the same native session with `opencode run --server`.
  For the move, call a write tool through `vowp mcp` before and after
  `vowp chat --takeover`; for the drain, run a slow fake-harness turn on one
  chat surface and take over with a second.
- Test 5: use the T0 rig's `vps:sudo` home, a temporary home and fixture
  project, a surface-only `laptop` node, and a private SSH alias; kill the
  executor's SSH child after the spool has three events and check the home log
  for contiguous sequence numbers and exactly one copy of every event.

## Gate status

This card changes only this document. The workspace gates run on the final
commit are reported in the job report; `cargo fmt --all`,
`cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace` are clean.
