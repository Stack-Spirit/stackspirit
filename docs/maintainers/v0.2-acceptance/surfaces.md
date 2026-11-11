# v0.2 acceptance — surfaces (T3)

This is the acceptance record for §13.5 tests **4 (surfaces, app changes and the
`vowp chat` fallback)** and **5 (SSH drop during a laptop-executor turn)**. It is
the T3 card `v2-t3-surfaces`.

**Result on the tested build: test 4 PASSES for the real opencode app surfaces.
The opencode desktop web UI (the UI the Mac desktop app loads) and the
`opencode mini` terminal UI over a real SSH PTY shared one live session: each
sent a message and saw the other's turn and reply. The move to `vowp chat` with
a capsule, the old app's VOWP write refusal, and a combined mid-turn lease move
with a running turn drained and continued all pass. Physical Mac and phone
hardware was not available to this Linux worker, so the device/form-factor
click-through remains a non-blocking owner follow-up, not an untested app
surface. Test 5 passes with one workaround: a real SSH connection was killed
mid-turn, the executor spooled 12 events, reconnected and replayed them with no
gap or duplicate.**

The run also found one product defect that affects remote execution: an explicit
`--project <name>` fails with `io: No such file or directory` unless the command
runs inside the checkout. `__exec` always sends the project name to the home, so
test 5 needed the workaround in open item 2; the spool/replay behaviour itself is
not affected.

## Environment

- Date: 2026-11-11 (UTC).
- Worktree: `job/v2-t3-surfaces`. The acceptance runs used the binary built at
  `3c57f7c`; the final branch is rebased onto `1b7a9b8`. The delta
  `3c57f7c..1b7a9b8` touches only capacity admission/supervisor tests and code
  (`crates/vowp-cli/tests/capacity.rs`, `crates/vowp-core/src/capacity/**`,
  `crates/vowp-core/src/fleet/mod.rs`,
  `crates/vowp-core/src/loops/supervisor.rs` and their tests), no surface,
  lease, session or remote-ingest code.
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

## Test 4 — surfaces: PASS (real opencode UIs; physical devices are a follow-up)

### 4a — one live opencode session in the desktop UI and the phone terminal UI

The desktop side is the opencode desktop web UI opened through the one-time
pair link in Chromium; this is the same renderer the Mac desktop app loads. The
phone side is `opencode mini` in an interactive SSH PTY through the same tunnel,
which is what a phone SSH app displays. Both are the real app UIs, not the
HTTP/CLI equivalent, and both drove the same live native session. Physical Mac
and phone hardware was not available, so the device-specific click-through
remains a non-blocking owner follow-up.

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

The server was loopback-only, and `opencode pair` printed the one-time link and
the matching `ssh -L` command. A private sshd was started on `127.0.0.1`; the
pair link was opened through the tunnel in Chromium (the desktop UI), and the
same tunnel carried an interactive `opencode mini` terminal UI over SSH:

```sh
ssh -N -L "127.0.0.1:$LPORT:127.0.0.1:$PORT" -p "$SSHPORT" "$USER@127.0.0.1"
# desktop UI: open a fresh pair link at 127.0.0.1:$LPORT in a browser
# phone UI:   ssh -tt ... 'OPENCODE_PASSWORD=<pair token> \
#             opencode mini --server http://127.0.0.1:$LPORT \
#             --session <native> --model fake/fake-model'
```

The desktop UI composer sent `turn one from the opencode desktop UI`; the phone
terminal showed that turn and its reply, then sent `turn two from the phone
terminal UI`; the desktop UI showed the phone turn and reply. The API message
list for the same native session contained both user turns and both assistant
replies:

```text
users 2 assistants 2
user_texts ['turn two from the phone terminal UI', 'turn one from the opencode desktop UI']
assistant_texts ['hello from the fake local model', 'hello from the fake local model']
RESULT: PASS
```

Screenshots of the desktop UI before and after the phone turn are in
`test4ui-desktop-turn.png` and `test4ui-desktop-sees-phone.png`; the terminal
pane capture is in `test4ui-phone-turn.txt`. The pair link was treated as a
secret: the evidence contains only the redacted `/auth/connect/<code>` form,
not the code or the redeemed token.

### 4b — moving from opencode to `vowp chat` after an idle app turn

A fresh orchestrator (`t3b`) used the same real opencode host. Its app-side
conversation got one desktop turn and completed before the move; this sub-check
proves the app→chat lease move, capsule and old-tool refusal, not the mid-turn
drain (4d covers that). The old app's VOWP write tool was called through
`vowp mcp` with the orchestrator's real surface id and lease token file:

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
the lease moved and the old app's token no longer matched, as the MCP refusal
shows. The new app continued from the capsule. The old opencode app was not
closed by VOWP (it cannot close another vendor's window); its VOWP write channel
is what becomes read-only, exactly as §7.4 states.

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

### 4d — combined app/model move while a turn is running

This is the card's combined clause: an old surface kind `opencode` holds the
lease and has a real VOWP run in flight; the old app's VOWP write tool is used;
a new `vowp chat` surface takes over mid-turn; the old run drains and is
interrupted; a capsule is written; the new app continues from it. The old
executor is the deterministic fake CLI harness (the real opencode desktop app
itself is the owner check), but the lease, MCP write gate, drain, switch and
capsule are the production VOWP paths.

The old app surface acquired the lease and began a slow turn through the same
`turn-begin`/`__exec` transport every surface uses:

```sh
SID=$($V --home "$VOWP_HOME" --project t3app --json session new \
        --title "t3 mid-turn app move" --executor cli:acceptance-hello \
        --cwd "$FIXTURE" | jq -r .id)
TOKEN=$($V --home "$VOWP_HOME" --project t3app session lease "$SID" \
          --acquire opencode --holder desktop --node acceptance-local)
# request.json carries {lease, prompt: "T3 SLOW TURN", executor, purpose, node, cwd}
$V --home "$VOWP_HOME" --project t3app --json session turn-begin "$SID" \
   --executor cli:acceptance-hello --purpose start --node acceptance-local \
   --request request.json > grant.json
$V --home "$VOWP_HOME" --project "$FIXTURE" __exec \
   --grant grant.json --spool-dir spool &
```

While the old turn was streaming, the old app's `vowp mcp` write succeeded. A
new chat surface then took over and the old MCP call was repeated:

```text
OLD_TURN_RUNNING=1 spool_lines=2
CHAT_TAKEOVER_MS=25 READY=1
MCP_REFUSAL_MS=53
OLD_EXEC_EXIT=75

old __exec stderr:
Taken over by acceptance-local at 05:34. This surface is now read-only (follow mode).

old MCP after the move:
{"isError":true,"content":[{"type":"text","text":"refused: Taken over by acceptance-local at 05:34. This surface is now read-only (follow mode)."}]}
```

The session log shows the running turn drained and the new surface continuing:

```text
events 30 seq_contiguous True
switches [('takeover', 'cli:acceptance-hello', 'cli:acceptance-hello')]
capsules [('cap_01M74HM97TXYVY3WKTA1KJ5NTN', 8)]
turns [(1, 'interrupted'), (2, 'end_turn')]
handoff 1 assistant 14
RESULT: PASS
```

The old run ends `interrupted`; the new turn is turn 2 with `note: handoff`
and its 12 fake-harness lines. The old `__exec` exited **75** with the exact
notice. This closes the mid-turn mechanics; the remaining device/form-factor
click-through is the non-blocking owner follow-up.

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

The home's sequence numbers are exactly `1..19`: 3 framing events
(`session.created`, the attach note, the executor attach), 2 turn events
(`turn.started`, `user.message`), 12 assistant messages, the turn end and the
executor detach. The 12 assistant lines are unique. Nothing was lost in the
killed connection and nothing was applied twice.

### The project-lookup workaround

The first attempt failed before the harness started because
`$V --home "$H" --project t3ssh session ...` exits 1 with
`{"error":{"kind":"io","message":"io: No such file or directory (os error 2)"}}`
when run outside the checkout, while the same command without `--project` from
inside the checkout works. `__exec` always sends `--project <name>` to the
home, so test 5 used a throwaway `vowp` wrapper on the home that dropped the
flag and ran from `/home/vowp/projects/t3ssh`. This is a product defect, not a
rig or fixture issue; see open item 2.

## Open items

1. **Physical-device follow-up (non-blocking).** The real opencode desktop UI,
   the interactive terminal UI over SSH, the MCP write refusal, the drain and
   the capsule all passed with opencode 2.0.28 and the production VOWP paths.
   The Linux worker had no Mac or phone hardware, so these device-specific
   click-throughs remain for the owner (about 5 minutes):
   - On the Mac: `vowp tunnel open t3 --remote-port <port>` and the printed
     `opencode pair` link; open the orchestrator project, send one message, and
     confirm the phone sees it.
   - On the phone: `ssh -t <home> 'cd <orchestrator> && opencode --server
     http://127.0.0.1:<port> --session <native>'` (the terminal UI); send one
     message and confirm the Mac sees it.
   - Optional: Claude Code Remote Control from the Claude desktop/mobile apps.
     This needs a claude.ai login on the home; the run had none, as S3 recorded.

2. **Fix job: explicit `--project <name>` fails outside the checkout.**
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
   workaround above. The failure is the relative harness mapping resolved
   against the current directory in `crates/vowp-core/src/deps.rs`
   (`mapping_path` / `config.project.path`); the other fix is to set
   `config.project.path` from the resolved checkout in
   `crates/vowp-cli/src/ctx.rs`. This was recorded by T2 as a related
   `project add` symptom; T3 shows it also breaks the remote-ingest path.

3. **opencode MCP model exposure remains the §16.20 open assumption.** The
   project-local `opencode.json` registers `vowp mcp` and a scripted MCP client
   can call its tools; the old-app refusal in 4b used that real stdio server.
   The run did not prove that opencode 2.0.28 offers those tools to a model,
   because the fake model's tool list in the S3 spike did not include them. The
   tool-capable fallback is `vowp chat`; Claude Code needs the one-time
   `.mcp.json` approval.

## Reproduction summary

- Test 4: create a real opencode orchestrator with a private `XDG_*` tree,
  point its global config at a loopback fake OpenAI-compatible model, redeem a
  fresh `opencode pair` link through a private SSH tunnel, and drive the same
  native session from the opencode desktop web UI in a browser and from
  `opencode mini` in an interactive SSH PTY. Both UIs see each other's turns.
  For the app→chat move, call a write tool through `vowp mcp` before and after
  `vowp chat --takeover`. For the combined mid-turn case (4d), acquire an
  `opencode` surface lease, start a slow turn through `turn-begin`/`__exec`,
  check the old MCP write succeeds, take over with a second `vowp chat` surface
  while the turn is streaming, and verify the old `__exec` exits 75, the old
  MCP write refuses, the capsule is written and the new turn starts with the
  handoff message. The owner Mac/phone click-through is still required.
- Test 5: use the T0 rig's `vps:sudo` home, a temporary home and fixture
  project, a surface-only `laptop` node, and a private SSH alias; kill the
  executor's SSH child after the spool has three events and check the home log
  for contiguous sequence numbers and exactly one copy of every event.

## Gate status

This card changes only this document. The workspace gates run on the final
commit are reported in the job report; `cargo fmt --all`,
`cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace` are clean.
