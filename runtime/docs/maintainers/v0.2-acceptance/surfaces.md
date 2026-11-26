# v0.2 acceptance — surfaces (T3)

This is the acceptance record for §13.5 tests **4 (surfaces, app changes and the
`vowp chat` fallback)** and **5 (SSH drop during a laptop-executor turn)**. It is
the T3 card `v2-t3-surfaces`.

**Result on the tested build: test 4 PASSES on every path this worker can
automate, and test 5 PASSES. The two physical owner-device click-throughs — the
opencode desktop app on the owner's Mac and the same live session on the
owner's phone over SSH — were NOT RUN. The planner deferred them to the owner
on 2026-11-11 (the "Owner-device scope" decision), and the keeper does not
waive owner acceptance. They are recorded in [PENDING OWNER
ACCEPTANCE](#pending-owner-acceptance) with the exact commands, the expected
result and "not run"; they are not a failure of the automated scope and no
longer block this card. The Claude Remote Control join is a further owner
check and is also pending.**

The automated test 4 evidence covers the two real opencode UIs sharing one live
native session, the app→chat lease move with capsule and old-tool refusal, the
chat fallback mid-turn drain, and a combined app/lease move mid-turn. A fresh
re-check on 2026-11-11 confirmed the orchestrator join text and pair-link URL
shape, and that the old surface's `vowp mcp` write is refused **82 ms** after a
lease move (well inside the 2 s bound). The test-5 record has three runs. The
earlier `t3` run (whose home-side wrapper dropped `--project`) recorded the kill
at three spooled events and a 19-event home log with 12 unique assistant lines.
The `t3final` run used the real `--project` path with `project.path` pinned; it
recorded two SSH connections, 13 spool events and the same 19-event/12-assistant
end state, but no kill point. The follow-up `t3follow` run used current `main`
after the fix with the fixture's stock `vowp.toml`, which has no `project.path`;
it recorded its own kill at three spooled events and the same end state.

The T3 work found one product defect that affects remote execution: an
explicit `--project <name>` failed with `io: No such file or directory` when
the project's `vowp.toml` had no `project.path` and the command ran outside the
checkout. `f-project-path-default` fixes it by defaulting `[project].path` to
the directory containing `vowp.toml` (fix commit `50e63b5`, with `b78ae83`
registering its integration test); the follow-up test-5 run exercises the real
`--project t3ssh` path without the pinned field.

## PENDING OWNER ACCEPTANCE

These are the owner checks. The two physical-device click-throughs were
deferred by the planner on 2026-11-11; the Claude Remote Control join is the
separate 5-minute owner test from §16.3. None was run: this Linux worker has no
Mac, no phone and no logged-in Tailscale path to either device. The owner
should use the final build from this branch. The automated app-UI run used
`3c57f7c`; `crates/vowp-core/src/surface/orchestrator.rs` changed between
`3c57f7c` and `ef3765c` (instructions packaging, resume, mirror cursor), so 4a
was not re-run on the final commit. The opencode-host, lease and remote-ingest
paths are unchanged, and 4e re-checks join/pair/lease/MCP on the final binary.

### 1. opencode desktop app on the Mac

Commands (the same block `vowp orchestrator new` prints, with the real paths,
port and session substituted):

```sh
# On the home, from a registered project checkout (or add --project <project>):
vowp orchestrator new --name t3 --app opencode \
    --executor opencode:deepseek/deepseek-flash#max

# On the control machine (the Mac):
vowp tunnel open t3 --remote-port <port>

# On the home:
cd ~/.vowp/orchestrators/t3 && \
  XDG_STATE_HOME=~/.vowp/orchestrators/t3/.opencode/state opencode pair

# Open the printed one-time
# http://127.0.0.1:<port>/auth/connect/<code> link in the opencode desktop
# app, replacing its port with the tunnel's local port.
```

Expected result: the orchestrator project opens in the Mac desktop app; sending
`owner mac turn` from its composer appears in the live session, and the phone
check below sees that turn and its reply. The join block printed by
`orchestrator new` is the authority for the concrete port, session id and
directory.

**Not run.**

### 2. The same live session on the phone over SSH

Command (from the printed `phone:` line):

```sh
ssh -t <home-ssh-target> \
  'cd <orchestrator-directory> && \
   opencode --server http://127.0.0.1:<port> --session <native-session>'
```

Expected result: the terminal UI opens on the same native session; the turn
sent from the Mac is visible; sending `owner phone turn` makes it appear in the
Mac desktop app and in the session message list. This is the phone's SSH app
path, not a browser or a VOWP-specific app.

**Not run.**

### 3. A real-app move to another app mid-turn

With the Mac opencode app holding the lease and streaming a turn:

```sh
printf 'continue after the app move\n' | vowp chat <session> --takeover

# The old app's write-tool call (same shape as 4b; use the orchestrator's
# real surface id and lease.token):
vowp mcp --session <session> --surface <old-surface> \
    --token-file <orchestrator>/lease.token <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"capacity_set","arguments":{"target":2,"reason":"owner mid-turn check"}}}
EOF
```

Expected result: the old app's next `vowp mcp` write tool call is refused within
**2 s** with exactly
`refused: Taken over by <holder> at <HH:MM>. This surface is now read-only (follow mode).`;
the old turn drains and ends `interrupted`; a capsule is written; the new
`vowp chat` turn starts with `note: handoff` and a capsule id. The automated
4b–4d checks below prove the same mechanics with a scripted client and the fake
CLI executor; this owner check proves the real app's window and write path.

**Not run.**

### 4. Claude Remote Control (the separate 5-minute owner test)

Command (from a registered project checkout, or add `--project <project>`;
a claude.ai login is required and an API key is not enough):

```sh
vowp orchestrator new --name claude-t3 --app claude-code
```

Expected result: `orchestrator claude-t3 (claude-code, running) ...` with join
instructions for `https://claude.ai/code` and the Claude mobile app; a message
sent from the desktop app appears in the mobile app on the same Remote Control
session.

**Not run.** The test home has no claude.ai login, so the login gate was
verified instead:

```text
$ vowp orchestrator new --name t3join-claude --app claude-code
vowp: refused: Claude Remote Control needs a claude.ai login on the home
$ echo $?
5
```

`vowp orchestrator ls` records that orchestrator as `degraded` with the
fallback join text, as §12.3/§7.6 describe. This is the honest refusal path;
the owner join is still pending.

## Environment

- Date: 2026-11-11 (UTC).
- Original record: worktree `job/v2-t3-surfaces`, based on `origin/main`
  `ef3765c`. Follow-up record and test-5 re-run: worktree
  `job/f-t3-record-followups`, based on `origin/main` `b78ae83` (which includes
  `f-project-path-default`).
- The app-UI evidence was collected with the binary at `3c57f7c`:
  `cargo build -p vowp-cli --bin vowp`, `vowp 0.1.0`, SHA-256
  `320916762ddf7ed636c2bb2eaa1ed56ff1d8dedc49e9ef95cb82151f1463973c`.
- The fresh join, lease-refusal and original test-5 re-check ran on the final
  code (`ef3765c` plus the docs-only branch): `vowp 0.1.0`, SHA-256
  `5bc6e07eb5a78ce4c20d76da9a12e06aab6d543e81793416709566b8d47c990b`.
- The follow-up test-5 run used the current-`main` binary built on
  `b78ae83`: `vowp 0.1.0`, SHA-256
  `dbc542a6d88c88eca4a62ca6346c5c7c722a9a0b2fff4be00ecabd481c540336`.
- opencode: `opencode v2.0.28` (the version the S3 spike pinned). OpenSSH
  `9.6p1`.
- Test 4 ran on the Linux build host with a private `HOME`, `XDG_*` tree,
  `VOWP_HOME`, `TMUX_TMPDIR` and tmux socket. The opencode service used a
  private XDG tree under the orchestrator directory and a local fake
  OpenAI-compatible model on `127.0.0.1`; no provider key, real model, public
  port or real `~/.vowp` was used.
- Test 5's earlier `t3` run used `/home/vowp/acceptance/t3/home` and ssh target
  `t3home`; its drop log is cited below. The fresh `t3final` run used home
  container `vowp-acceptance-t3final-home` (image
  `vowp-acceptance/vps:sudo`,
  `sha256:3c19867d4d633df0ca5f0dda7fa06300e69816a3857c1488be05184c915011ab`,
  Ubuntu 24.04), temporary home `/home/vowp/acceptance/t3final/home`,
  `TMUX_TMPDIR` `/home/vowp/.acceptance/tmux` and socket
  `vowp-acceptance-t3final`. The follow-up re-run used rig `t3follow`:
  container `vowp-acceptance-t3follow-home` (same image and digest), temporary
  home `/home/vowp/acceptance/t3follow/home`, the same `TMUX_TMPDIR` and
  socket `vowp-acceptance-t3follow`.
- The laptop executor for test 5 was the build host, not the rig's disposable
  phone client (that client was used only for earlier surface-shape checks;
  the owner phone click-through is pending). The host home, host `~/.vowp`,
  host tmux server, network and real harnesses were never used.
- The `t3final` and `t3follow` rigs were removed after the evidence was
  collected with `bash scripts/acceptance/rig.sh down --name <rig>`;
  containers, network and generated keys were removed.

`V` below is the built `vowp` and `$ROOT` is a private temporary directory:

```sh
V=/path/to/target/debug/vowp
ROOT=/tmp/vowp-t3
mkdir -p "$ROOT"/{home,user,tmp,tmux,bin}
export HOME="$ROOT/user" TMPDIR="$ROOT/tmp" TMUX_TMPDIR="$ROOT/tmux"
export VOWP_HOME="$ROOT/home" VOWP_ACCEPTANCE_TMUX_SOCKET=t3
export PATH="$ROOT/bin:$PATH"
```

## Test 4 — surfaces: PASS (automated scope); owner devices pending

### 4a — one live opencode session in the desktop UI and the phone terminal UI

The desktop side is the opencode desktop web UI opened through the one-time
pair link in Chromium; this is **expected** to be the same web UI the Mac
desktop app loads, but that is an assumption, not something this Linux worker
verified. The phone side is `opencode mini` in an interactive SSH PTY through
the same tunnel, which is what a phone SSH app displays. Both are the real app
UIs, not the HTTP/CLI equivalent, and both drove the same live native session.
Physical Mac and phone hardware were not available, so the device-specific
click-through is covered by the PENDING OWNER ACCEPTANCE section above; it is
not a failure of the transport or UI evidence below.

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
second throwaway T0 `hello` fixture registered as `t3app`. Its fake CLI harness
was extended in the temporary fixture only to answer the capsule writer with a
valid `CapsuleDraft` and to stream 12 lines for a prompt containing
`T3 SLOW TURN`; it never calls a model or the network.

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

The quoted join block is from the initial host run on port 45489; the UI run
resumed the orchestrator on port 39169, and the commands below use `$PORT` for
the actual port. The server was loopback-only, and `opencode pair` printed the
one-time link and the matching `ssh -L` command. A private sshd was started on
`127.0.0.1`; the pair link was opened through the tunnel in Chromium (the
desktop UI), and the same tunnel carried an interactive `opencode mini`
terminal UI over SSH:

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

### 4d — combined app/lease move while a turn is running

This is the card's combined clause: an old surface kind `opencode` holds the
lease and has a real VOWP run in flight; the old app's VOWP write tool is used;
a new `vowp chat` surface takes over mid-turn; the old run drains and is
interrupted; a capsule is written; the new app continues from it. The app
changes even though the executor string stays `cli:acceptance-hello`; no model
switch is claimed for this sub-check. The old executor is the deterministic
fake CLI harness (the real opencode desktop app itself is the owner check), but
the lease, MCP write gate, drain, switch and capsule are the production VOWP
paths. The old surface id `01M74MIDOLDSURFACE00000000` in the scripted MCP call
is **synthetic**: it represents the old app, while the lease, token file, MCP
gate, takeover, drain and capsule are the production paths.

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
click-through is covered by the PENDING OWNER ACCEPTANCE section above.

### 4e — fresh join, pair-link output and write-gate re-check (2026-11-11)

The final code was re-checked directly, without the old evidence:

```text
$ vowp orchestrator new --name t3join --app opencode \
    --executor opencode:deepseek/deepseek-flash#max
orchestrator t3join (opencode, running) session 01M74P8Z6AST9MARPFCY1KDC5R
  directory: /tmp/opencode/t3-final/join/home/orchestrators/t3join
  desktop: 1. On the control machine: vowp tunnel open 't3join' --remote-port 42357
2. On the home: cd '/tmp/.../orchestrators/t3join' && XDG_STATE_HOME='/tmp/.../orchestrators/t3join/.opencode/state' 'opencode' pair
3. Open the one-time link it prints (5 minutes, single use) in the opencode desktop app, replacing its port with the tunnel's local port.
  phone: ssh -t <home-ssh-target> 'cd '\''/tmp/.../orchestrators/t3join'\'' && '\''opencode'\'' --server http://127.0.0.1:42357 --session '\''ses_e369b6f0bffegVAA5j8EjVipuZ'\'''
  chat: vowp chat 01M74P8Z6AST9MARPFCY1KDC5R
```

The matching `opencode pair` output, reproduced with the one-time code
redacted. The temporary raw evidence file that held the code during the
original verification (under the worker's `/tmp/opencode/t3-final/join`
evidence tree) was deleted while preparing this follow-up record; it is gone
now. The committed record never contained the code, and it is not reproduced
here:

```text
Open a link to connect. Links work once and expire in 5 minutes.

  http://127.0.0.1:42357/auth/connect/<code>

Over SSH? Forward the port, then open the link on your machine:
  ssh -L 42357:127.0.0.1:42357 <host>
```

A `vowp mcp` `capacity_set` call succeeded while the old opencode-kind lease
was held, then a `vowp chat --takeover` moved the lease to the chat surface.
The same MCP call then failed in **82 ms** with exactly:

```text
refused: Taken over by acceptance-local at 06:56. This surface is now read-only (follow mode).
```

The capsule step is not claimed for this short re-check (the first throwaway
fixture used here did not implement the capsule writer); 4b and 4d above are
the recorded capsule and drain evidence. The join text, one-time URL shape,
lease move and old-write refusal are all real VOWP paths and match the printed
blocks.

### Temporary fake harness used by 4b–4d

This is the throwaway fixture harness, also kept as `test4-fake-worker.sh` in
the evidence directory. It streams 12 lines for a prompt containing
`T3 SLOW TURN` and answers the capsule writer with a valid `CapsuleDraft`; it
never calls a model or the network.

```sh
#!/usr/bin/env bash
set -Eeuo pipefail
prompt=$(cat)
case "$prompt" in
  "You are the handoff writer for a software-engineering session."*)
    cat <<'JSON'
{"intent":"Continue the T3 surfaces acceptance conversation.","constraints":["Do not invent facts"],"decisions":[{"what":"move to the chat fallback","why":"the owner switched apps"}],"state":{"done":["the app surfaces shared one session"],"in_progress":["continue in the new surface"],"blocked":[],"next_steps":["answer the next message"]},"files":[],"gotchas":["the old app is read-only after takeover"],"open_questions":[]}
JSON
    exit 0
    ;;
esac
case "$prompt" in
  *"T3 SLOW TURN"*)
    for i in $(seq 1 12); do
      printf 't3 slow line %02d\n' "$i"
      sleep 0.35
    done
    exit 0
    ;;
esac
printf 't3 fake answer: %s\n' "$(printf '%s' "$prompt" | head -n 1)"
```

## Test 5 — SSH drop during a laptop-executor turn: PASS

### Setup

The acceptance record has two full fresh runs on the T0 rig, `t3final` and
`t3follow`, plus the earlier `t3` run. The earlier `t3` run used a home-side
wrapper that dropped `--project`; its recorded `DROP at spool_lines=3` is cited
below. The `t3final` run (2026-11-11) used the then-current `ef3765c`-based
binary and pinned the documented `project.path` field in the throwaway fixture
because the `--project <name>` defect was still open; it did not record its
kill point. The follow-up acceptance run (`t3follow`, 2026-11-11) used the
current-`main` binary after `f-project-path-default` (`50e63b5`, with
`b78ae83` registering its test target) and the fixture's stock `vowp.toml`,
which has no `project.path`. The commands below are the `t3follow` run; the
`t3final` run used the same shape with the removed `sed` line.

The home was a temporary VOWP home with one fixture project (`t3ssh`, a T0
`hello` fixture) and a surface-only node named `laptop`. Start it with the T0
rig; inside the home container these commands use the home's `vowp` and `H`:

```sh
bash scripts/acceptance/rig.sh up --name t3follow --variant sudo --no-build
# Copy the built binary into the home and put it on the SSH PATH; the
# `session ingest` child invokes `vowp` by name.
docker cp target/debug/vowp vowp-acceptance-t3follow-home:/home/vowp/vowp
docker exec -u root vowp-acceptance-t3follow-home sh -lc \
  'cp /home/vowp/vowp /usr/local/bin/vowp && chmod 0755 /usr/local/bin/vowp'
# inside the home container:
H=/home/vowp/acceptance/t3follow/home
```

The fixture is created with the T0 generator; its stock `vowp.toml` has no
`project.path`, so the fix defaults it from the `vowp.toml` directory:

```sh
bash /opt/vowp-acceptance/fixtures/project.sh hello \
    /home/vowp/projects/t3ssh --name t3ssh
# Install the streaming fake harness used by the turn (test4-fake-worker.sh).
cp <evidence>/test4-fake-worker.sh \
    /home/vowp/projects/t3ssh/.acceptance/bin/fake-worker
chmod 0755 /home/vowp/projects/t3ssh/.acceptance/bin/fake-worker
```

The home is initialised in a throwaway repository, then the fixture is
registered:

```sh
mkdir -p /home/vowp/initproj && cd /home/vowp/initproj
git init -q -b main && printf 't3 follow home\n' > README.md
git add README.md && git -c user.name=t3 -c user.email=t3@example.invalid commit -qm init
vowp --home "$H" init --name t3follow-home --no-framework
vowp --home "$H" project add /home/vowp/projects/t3ssh
vowp --home "$H" node add laptop -
# from outside the checkout, with no project.path in the fixture:
cd /home/vowp
vowp --home "$H" --project t3ssh --json session ls
# [] (exit 0)
```

The laptop side was the build host with its own temporary `VOWP_HOME`, a
private tmux directory and a private `VOWP_CONTROL_HOME` whose `control.json`
named the rig's SSH target and the home path:

```json
{"role":"control","home":"vowp-home","ssh_target":"t3followhome",
 "remote_home":"/home/vowp/acceptance/t3follow/home","created_at":"..."}
```

For the test only, an `ssh` wrapper on the laptop's `PATH` maps the
`t3followhome` alias to the rig's loopback port and generated owner key; it does
not change the VOWP CLI or the ingest path. The laptop checkout is a second copy
of the same fixture (its `vowp.toml` also has no `project.path`), so
`__exec --project <laptop-checkout>` exercises the same default. The home
created a session with the fake CLI executor, acquired an executor lease, and
wrote the `turn-begin` grant (the grant file was mode 0600; its token is never
printed here):

```sh
# inside the home container
SID=$(vowp --home "$H" --project t3ssh --json session new \
        --title "t3follow ssh drop" --executor cli:acceptance-hello \
        --cwd /home/vowp/projects/t3ssh | jq -r .id)
TOKEN=$(vowp --home "$H" --project t3ssh session lease "$SID" \
          --acquire opencode --holder laptop --node laptop)
# request.json carries {lease, prompt: "T3 SLOW TURN", executor, purpose, node, cwd};
# cwd is the laptop checkout, not the home checkout.
vowp --home "$H" --project t3ssh --json session turn-begin "$SID" \
   --executor cli:acceptance-hello --purpose start --node laptop \
   --request request.json > grant.json
# copy grant.json (mode 0600) to the laptop, then run the executor there:
vowp --home <laptop-home> --project <laptop-checkout> __exec \
   --grant grant.json --spool-dir <laptop-spool>
```

The fake harness streamed 12 lines with 0.35 s between lines. On the laptop,
`vowp __exec` ran the harness and opened a real SSH connection to the home's
`vowp session ingest`; the executor-side spool was a private directory on the
laptop. The home's `session ingest` is invoked as `vowp --home <remote-home>
--project t3ssh session ingest ...` by the real `__exec`, so this run exercises
the project-name path without `project.path`: the fix defaults it to the
fixture directory and the relative harness mapping resolves from there.

### The drop and replay

The SSH child was killed when the spool held three events, not at a process
boundary. `__exec` reconnected on the next event and replayed from the first
unacknowledged frame. The two SSH connections are visible in the wrapper log:

```text
DROP at spool_lines=3 ssh_pid=1624522
EXEC_EXIT=0
SSH_CONNECTIONS=2
{"ack_window":64,"done":true,"resumed":false,
 "run_id":"01M74Y78G5EV0VA4NVH9B6EJK0",
 "session_id":"01M74Y77XWQZ64DEG1QCCTNXB9"}
```

`resumed:false` means this was a fresh `__exec` process, not that the killed
connection was ignored: the same process reconnected and replayed its spool
before finishing. A separate client-crash resume would print `resumed:true`.

The follow-up evidence is in `/tmp/opencode/t3follow/evidence`: `drop.txt` (the
drop log, exit, connection count and status above), `ssh.log` (both `session
ingest --project t3ssh` invocations), `home-check.txt`, `spool-check.txt` and
`home-setup.txt` (the project lookup without `project.path`).

The earlier `t3` run is the source of the other recorded drop line:
`/tmp/opencode/t3-evidence/test5-drop.txt` has `DROP at spool_lines=3`; its
`test5-ssh.log` names `t3home` and `/home/vowp/acceptance/t3/home`; its run id
is `01M74F9VPZYTJHESGW1HNJMRFR`; and its `test5-check.txt` records assistant
text `ssh drop line NN`.

The later `t3final` run has no recorded drop line anywhere under
`/tmp/opencode/t3-final`. Its `/tmp/opencode/t3-final/test5-drop.txt` records
two SSH connections and 13 spool events with run id
`01M74PFYDZX0Q5QN9VYAG65SM7`; `test5-check.txt` records 19 home events and
assistant text `t3 slow line NN`; its ssh wrapper log names `t3finalhome`. The
kill point is not claimed for that run.

The home log and the executor spool agree exactly:

```text
home events 19
types: assistant.message 12, note 1, session.created 1,
       session.executor 2, turn.ended 1, turn.started 1, user.message 1
seqs 1..19 contiguous true
assistant 12 unique 12
first 't3 slow line 01' last 't3 slow line 12'
RESULT: PASS

spool_events 13 (12 events + summary) summary 1 acked_events 12
summary_acked true
RESULT: PASS
```

The home's sequence numbers are exactly `1..19`: 3 framing events
(`session.created`, the attach note, the executor attach), 2 turn events
(`turn.started`, `user.message`), 12 assistant messages, the turn end and the
executor detach. The 12 assistant lines are unique. Nothing was lost in the
killed connection and nothing was applied twice.

### The project-name path without `project.path`

The first T3 attempt failed before the harness started because
`$V --home "$H" --project t3ssh session ...` exited 1 with
`{"error":{"kind":"io","message":"io: No such file or directory (os error 2)"}}`
when run outside the checkout and the project had no `project.path`, while the
same command without `--project` from inside the checkout worked. `__exec`
always sends `--project <name>` to the home, so the original fresh run pinned
`project.path` in the throwaway fixture.

`f-project-path-default` removes that workaround: `load_project_config` fills an
omitted `[project].path` with the directory containing `vowp.toml`, so the
relative harness mapping resolves under the project checkout instead of the
caller's cwd. The follow-up `t3follow` run above used the stock fixture config
(no `project.path`), and the project lookup from `/home/vowp` exited 0 before
the turn. The new integration test covers the same path:

```text
$ cargo test -p vowp-cli --test project_path
test load_project_config_defaults_project_path ... ok
test cli_project_name_resolves_relative_mapping_outside_checkout ... ok
test result: ok. 2 passed; 0 failed
```

This replaces the earlier `t3` run's home-side wrapper that dropped the
`--project` flag: the `t3final` and `t3follow` runs use the real flag and the
real ingest path.

## Open items and followup dispositions

1. **PENDING OWNER ACCEPTANCE — the owner checks above.** They were deferred
   to the owner by the planner on 2026-11-11 (the two device click-throughs) or
   are the separate §16.3 Claude test; they are not waived and are not a
   failure of the automated scope. The exact commands and expected results are
   in the section above. The owner should use the final build.

2. **Resolved — explicit `--project <name>` failed outside the checkout when
   the project config had no `project.path`.** `f-project-path-default` (fix
   commit `50e63b5`, with `b78ae83` registering its integration test) fills an
   omitted `config.project.path` from the directory containing `vowp.toml` in
   `crates/vowp-cli/src/ctx.rs`, so the relative harness mapping resolves under
   the project checkout. The follow-up test-5 run above uses the real
   `--project t3ssh` ingest path with no pinned field, and
   `cargo test -p vowp-cli --test project_path` covers the same resolution from
   an unrelated directory. This removes the workaround from the original run.

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
  handoff message. The owner Mac/phone click-through and the Claude Remote
  Control join are the PENDING OWNER ACCEPTANCE items above.
- Test 5: use the T0 rig's `vps:sudo` home, a temporary home and fixture
  project with no `project.path`, a surface-only `laptop` node, and a private
  SSH alias; kill the executor's SSH child after the spool has three events and
  check the home log for contiguous sequence numbers and exactly one copy of
  every event. The real `__exec` → `vowp session ingest --project t3ssh` path is
  used. The earlier `t3final` run used the same shape with `project.path`
  pinned before the fix.

## Verification followups

The five non-blocking followups from the `v2-t3-surfaces` verify-1 review are
disposed of in this record:

1. **4e pair code:** the 4e text now says the committed output is redacted and
   the temporary raw evidence file was deleted during verification; the code is
   not reproduced.
2. **Final-commit scope:** the record now says
   `crates/vowp-core/src/surface/orchestrator.rs` changed between `3c57f7c`
   and `ef3765c`, that 4a was not re-run on the final commit, and that the
   opencode-host/lease/ingest paths are unchanged while 4e re-checks
   join/pair/lease/MCP.
3. **Join-state citation:** the degraded/fallback join state now cites
   §12.3/§7.6 without "exactly".
4. **Test-5 drop artifact:** the follow-up run's drop log is
   `/tmp/opencode/t3follow/evidence/drop.txt`; the earlier
   `/tmp/opencode/t3-evidence/test5-drop.txt` is the `t3` run's
   `DROP at spool_lines=3` record. The `t3final` run is limited to its recorded
   end state (two connections, 13 spool events, 19 home events) because no drop
   line was recorded for it.
5. **Test-5 project path:** test 5 now passes on the `t3follow` rig without
   `project.path` after `f-project-path-default` (`50e63b5`, with `b78ae83`
   registering its test); the setup and evidence are recorded above.

## Gate status

This card changes only this document. The workspace gates run on the final
commit are reported in the job report; `cargo fmt --all`,
`cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace` are clean.
