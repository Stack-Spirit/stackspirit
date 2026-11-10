#!/usr/bin/env bash
# S3 app-surfaces spike: prove the opencode and Claude Code surfaces locally.
#
# Everything runs against 127.0.0.1 with throwaway XDG/HOME/SSH state, a fake
# OpenAI-compatible model and a dummy stdio MCP server. It never contacts the
# network, the real ~/.vowp, the default tmux server or a real harness login.
#
# Usage:
#   scripts/spikes/apps/run.sh            # clean up the temp directory
#   KEEP=1 scripts/spikes/apps/run.sh     # keep the evidence directory
#
# The script prints PASS/FAIL lines and exits non-zero if a check fails.
set -euo pipefail
umask 077

HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
KEEP=${KEEP:-0}
WORK=$(mktemp -d "${TMPDIR:-/tmp}/vowp-s3-apps.XXXXXX")
EVIDENCE="$WORK/evidence"
PROJECT="$WORK/project"
PHONE="$WORK/phone"
mkdir -p "$EVIDENCE" "$PROJECT" "$PHONE" "$WORK/tmux" "$WORK/ssh" "$WORK/xdg/config" "$WORK/xdg/data" "$WORK/xdg/state" "$WORK/xdg/cache" "$WORK/phone/config" "$WORK/phone/data" "$WORK/phone/state" "$WORK/phone/cache" "$WORK/claude/home" "$WORK/claude/config"

# Keep every child out of the real harness/TUI/tmux state.
unset TMUX TMUX_PANE OPENCODE OPENCODE_SESSION_ID OPENCODE_PASSWORD OPENCODE_SERVER OPENCODE_CONFIG OPENCODE_CONFIG_CONTENT
export XDG_CONFIG_HOME="$WORK/xdg/config"
export XDG_DATA_HOME="$WORK/xdg/data"
export XDG_STATE_HOME="$WORK/xdg/state"
export XDG_CACHE_HOME="$WORK/xdg/cache"
export TMUX_TMPDIR="$WORK/tmux"
export OPENCODE_DISABLE_AUTOUPDATE=1
export OPENCODE_DISABLE_MODELS_FETCH=1
export MCP_DUMMY_LOG="$EVIDENCE/mcp-opencode.log"

PASS=0
FAIL=0
NOTES=()

say() { printf '\n== %s ==\n' "$*"; }
ok() { printf 'PASS  %s\n' "$*"; PASS=$((PASS + 1)); }
bad() { printf 'FAIL  %s\n' "$*" >&2; FAIL=$((FAIL + 1)); }
note() { printf 'NOTE  %s\n' "$*"; NOTES+=("$*"); }
die() { printf 'FATAL %s\n' "$*" >&2; exit 1; }

redact() {
  sed -E \
    -e 's#/auth/connect/[A-Za-z0-9_-]+#/auth/connect/<code>#g' \
    -e 's/"password":"[^"]*"/"password":"<redacted>"/g' \
    -e 's/"token":"[^"]*"/"token":"<redacted>"/g'
}

free_port() {
  python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
}

wait_tcp() {
  local port=$1
  local i
  for i in $(seq 1 150); do
    if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
      return 0
    fi
    sleep 0.1
  done
  return 1
}

wait_http() {
  local url=$1
  local i code
  for i in $(seq 1 150); do
    code=$(curl -s -o /dev/null -w '%{http_code}' "$url" 2>/dev/null || true)
    if [ -n "$code" ] && [ "$code" != "000" ]; then
      return 0
    fi
    sleep 0.1
  done
  return 1
}

oe() {
  env XDG_CONFIG_HOME="$WORK/xdg/config" \
      XDG_DATA_HOME="$WORK/xdg/data" \
      XDG_STATE_HOME="$WORK/xdg/state" \
      XDG_CACHE_HOME="$WORK/xdg/cache" \
      OPENCODE_DISABLE_AUTOUPDATE=1 \
      OPENCODE_DISABLE_MODELS_FETCH=1 \
      "$@"
}

claude_local() {
  env -u ANTHROPIC_API_KEY -u ANTHROPIC_AUTH_TOKEN \
      HOME="$WORK/claude/home" \
      CLAUDE_CONFIG_DIR="$WORK/claude/config" \
      CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
      "$@"
}

SERVICE_STOPPED=0
stop_service() {
  if [ "$SERVICE_STOPPED" -eq 0 ]; then
    ( cd "$PROJECT" && oe opencode service stop >/dev/null 2>&1 ) || true
    SERVICE_STOPPED=1
  fi
}

TUNNEL_PID=""
SSHD_PID=""
FAKE_PID=""
EVENTS_PID=""
cleanup() {
  local status=$?
  set +e
  if [ -n "$EVENTS_PID" ]; then kill "$EVENTS_PID" 2>/dev/null; wait "$EVENTS_PID" 2>/dev/null; fi
  if [ -n "$TUNNEL_PID" ]; then kill "$TUNNEL_PID" 2>/dev/null; wait "$TUNNEL_PID" 2>/dev/null; fi
  if [ -n "$SSHD_PID" ]; then kill "$SSHD_PID" 2>/dev/null; wait "$SSHD_PID" 2>/dev/null; fi
  stop_service
  if [ -n "$FAKE_PID" ]; then kill "$FAKE_PID" 2>/dev/null; wait "$FAKE_PID" 2>/dev/null; fi
  if [ "$KEEP" = "1" ]; then
    printf '\nkept evidence in %s\n' "$WORK"
  else
    rm -rf "$WORK"
  fi
  exit "$status"
}
trap cleanup EXIT INT TERM

say "preconditions and versions"
for cmd in opencode claude python3 curl ssh ssh-keygen; do
  command -v "$cmd" >/dev/null 2>&1 || die "missing command: $cmd"
done
SSHD=${SSHD:-/usr/sbin/sshd}
[ -x "$SSHD" ] || die "missing sshd at $SSHD (set SSHD=...)"
{
  printf 'opencode: '; oe opencode --version
  printf 'claude:   '; claude --version
  printf 'ssh:      '; ssh -V 2>&1
  printf 'python3:  '; python3 --version
} | tee "$EVIDENCE/versions.txt"

PROJECT_REAL=$(cd "$PROJECT" && pwd -P)
FPORT=$(free_port)
OPORT=$(free_port)
SSHPORT=$(free_port)
LPORT=$(free_port)

say "start the local fake model (no network, no key)"
FAKE_TOOL_CALL=1 FAKE_TOOL_NAME=vowp_spike_echo python3 "$HERE/fake_openai.py" "$FPORT" >"$WORK/fake.port" 2>"$EVIDENCE/fake-openai.log" &
FAKE_PID=$!
wait_tcp "$FPORT" || die "fake model did not start"
grep -q '^[0-9][0-9]*$' "$WORK/fake.port" || die "fake model did not print its port"

cat >"$PROJECT/opencode.json" <<EOF
{
  "\$schema": "https://opencode.ai/config.json",
  "autoupdate": false,
  "share": "disabled",
  "disabled_providers": ["opencode"],
  "provider": {
    "fake": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Fake Local",
      "options": {"baseURL": "http://127.0.0.1:$FPORT/v1", "apiKey": "spike-key"},
      "models": {
        "fake-model": {
          "name": "Fake Model",
          "tool_call": true,
          "limit": {"context": 8192, "output": 1024}
        }
      }
    }
  },
  "model": "fake/fake-model",
  "small_model": "fake/fake-model",
  "mcp": {
    "spike": {
      "type": "local",
      "command": ["python3", "$HERE/mcp_dummy.py"],
      "enabled": true,
      "timeout": 5000
    }
  }
}
EOF

say "start opencode serve as a private user service"
( cd "$PROJECT" && oe opencode service set port "$OPORT" >/dev/null )
( cd "$PROJECT" && oe opencode service start ) 2>&1 | tee "$EVIDENCE/service-start.txt"
wait_http "http://127.0.0.1:$OPORT/api/info" || die "opencode service did not start"
PW=$(python3 - "$XDG_STATE_HOME/opencode/service.json" <<'PY'
import json, sys
print(json.load(open(sys.argv[1]))["password"])
PY
)
[ -n "$PW" ] || die "no server password in service.json"
ok "opencode service on 127.0.0.1:$OPORT"

if ss -ltn 2>/dev/null | grep -qE "0\.0\.0\.0:$OPORT|\[::\]:$OPORT"; then
  bad "opencode server is listening on a non-loopback address"
else
  ok "opencode server listens on loopback only"
fi

say "warm up the project-local MCP server"
MCP_READY=0
for i in $(seq 1 100); do
  curl -sS -u "opencode:$PW" "http://127.0.0.1:$OPORT/api/mcp?location%5Bdirectory%5D=$PROJECT" >"$EVIDENCE/mcp-status.json"
  if grep -q '"status":"connected"' "$EVIDENCE/mcp-status.json"; then MCP_READY=1; break; fi
  sleep 0.1
done
if [ "$MCP_READY" -eq 1 ]; then
  ok "opencode MCP server 'spike' is connected"
else
  bad "opencode MCP server did not reach connected"
fi
( cd "$PROJECT" && oe opencode mcp list ) >"$EVIDENCE/mcp-list.txt" 2>&1 || true
if grep -q 'spike.*connected' "$EVIDENCE/mcp-list.txt"; then
  ok "opencode mcp list shows spike connected"
else
  note "opencode mcp list did not show the project server; API status is the evidence"
fi

say "generate a pair link and forward it through a private SSH tunnel"
( cd "$PROJECT" && oe opencode pair ) >"$EVIDENCE/pair-raw.txt" 2>&1 || die "opencode pair failed"
redact <"$EVIDENCE/pair-raw.txt" | tee "$EVIDENCE/pair.txt"
LINK=$(grep -oE 'http://127\.0\.0\.1:[0-9]+/auth/connect/[A-Za-z0-9_-]+' "$EVIDENCE/pair-raw.txt" | head -1 || true)
[ -n "$LINK" ] || die "pair output had no connect link"
case "$LINK" in
  *":$OPORT/auth/connect/"*) ok "pair link points at the private server" ;;
  *) bad "pair link does not point at the private server" ;;
esac
grep -q "ssh -L $OPORT:127.0.0.1:$OPORT" "$EVIDENCE/pair-raw.txt" && ok "pair output prints the SSH tunnel command" || bad "pair output has no SSH tunnel command"

ssh-keygen -q -t ed25519 -N '' -f "$WORK/ssh/host_key"
ssh-keygen -q -t ed25519 -N '' -f "$WORK/ssh/client_key"
cp "$WORK/ssh/client_key.pub" "$WORK/ssh/authorized_keys"
chmod 600 "$WORK/ssh/authorized_keys"
cat >"$WORK/ssh/sshd_config" <<EOF
Port $SSHPORT
ListenAddress 127.0.0.1
HostKey $WORK/ssh/host_key
PidFile $WORK/ssh/sshd.pid
AuthorizedKeysFile $WORK/ssh/authorized_keys
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
PermitRootLogin no
StrictModes no
AllowUsers $(id -un)
LogLevel ERROR
EOF
"$SSHD" -D -e -f "$WORK/ssh/sshd_config" >"$EVIDENCE/sshd.log" 2>&1 &
SSHD_PID=$!
wait_tcp "$SSHPORT" || die "private sshd did not start"
ssh -N -F /dev/null \
  -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile="$WORK/ssh/known_hosts" \
  -o IdentitiesOnly=yes -i "$WORK/ssh/client_key" -o ExitOnForwardFailure=yes \
  -L "127.0.0.1:$LPORT:127.0.0.1:$OPORT" -p "$SSHPORT" "$(id -un)@127.0.0.1" \
  >"$EVIDENCE/tunnel.log" 2>&1 &
TUNNEL_PID=$!
wait_tcp "$LPORT" || die "SSH tunnel did not open the local port"
ok "SSH tunnel 127.0.0.1:$LPORT -> sshd:$SSHPORT -> opencode:$OPORT"

TUNNEL_LINK=${LINK/127.0.0.1:$OPORT/127.0.0.1:$LPORT}
PAIR_CODE=$(curl -sS -o "$WORK/token.json" -w '%{http_code}' "$TUNNEL_LINK" || true)
if [ "$PAIR_CODE" = "200" ] && python3 - "$WORK/token.json" >"$WORK/token.txt" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
print(data["token"])
PY
then
  TOKEN=$(cat "$WORK/token.txt")
  ok "pair link redeemed through the tunnel (token accepted as Basic auth password)"
else
  bad "pair link redemption failed with HTTP $PAIR_CODE"
  TOKEN=""
fi
REUSE_CODE=$(curl -sS -o "$WORK/pair-reuse.json" -w '%{http_code}' "$TUNNEL_LINK" || true)
if [ "$REUSE_CODE" = "401" ] && grep -q 'expired or already used' "$WORK/pair-reuse.json"; then
  ok "pair link is single-use"
else
  bad "pair link was not rejected on reuse (HTTP $REUSE_CODE)"
fi

if [ -z "$TOKEN" ]; then
  die "no pairing token; cannot exercise the app-side API"
fi
BASE="http://127.0.0.1:$LPORT"
AUTH="opencode:$TOKEN"

say "the paired app client can list sessions and create one in the project"
( cd "$PROJECT" && oe env OPENCODE_PASSWORD="$TOKEN" opencode session list --server "$BASE" --format json ) >"$EVIDENCE/session-list-before.json" 2>"$EVIDENCE/session-list-before.err" || true
python3 - "$EVIDENCE/session-list-before.json" <<'PY' && ok "paired CLI listed sessions through the tunnel" || bad "paired CLI could not list sessions"
import json, sys
json.load(open(sys.argv[1]))
PY

CREATE_BODY=$(python3 - "$PROJECT" <<'PY'
import json, sys
print(json.dumps({"title": "VOWP S3 throwaway", "location": {"directory": sys.argv[1]}}))
PY
)
curl -sS -u "$AUTH" -X POST "$BASE/api/session" -H 'content-type: application/json' --data "$CREATE_BODY" >"$EVIDENCE/session-create.json"
SID=$(python3 - "$EVIDENCE/session-create.json" "$PROJECT" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))["data"]
assert data["location"]["directory"] == sys.argv[2], data
print(data["id"])
PY
)
[ -n "$SID" ] && ok "session $SID created in $PROJECT through the tunnel" || die "session creation failed"

say "capture live server events while two surfaces talk to the same session"
: >"$EVIDENCE/events.sse"
curl -sS -N -u "$AUTH" "$BASE/api/event" >"$EVIDENCE/events.sse" 2>"$EVIDENCE/events.err" &
EVENTS_PID=$!
sleep 0.3

PROMPT_ONE=$(python3 - <<'PY'
import json
print(json.dumps({"text": "turn one from the desktop side"}))
PY
)
( cd "$PROJECT" && oe env OPENCODE_PASSWORD="$TOKEN" opencode api --server "$BASE" POST "/api/session/$SID/prompt" --data "$PROMPT_ONE" ) >"$EVIDENCE/prompt-one.json" 2>"$EVIDENCE/prompt-one.err" || true
if grep -q '"type":"user"' "$EVIDENCE/prompt-one.json"; then
  ok "turn one sent through opencode api + the tunnel"
else
  bad "turn one prompt failed"
fi
for i in $(seq 1 150); do
  curl -sS -u "$AUTH" "$BASE/api/session/$SID/message" >"$WORK/messages-one.json"
  if python3 - "$WORK/messages-one.json" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))["data"]
raise SystemExit(0 if any(m["type"] == "assistant" for m in data) else 1)
PY
  then break; fi
  sleep 0.1
done

( cd "$PHONE" && env XDG_CONFIG_HOME="$WORK/phone/config" XDG_DATA_HOME="$WORK/phone/data" XDG_STATE_HOME="$WORK/phone/state" XDG_CACHE_HOME="$WORK/phone/cache" OPENCODE_DISABLE_AUTOUPDATE=1 OPENCODE_DISABLE_MODELS_FETCH=1 OPENCODE_PASSWORD="$TOKEN" opencode run --server "$BASE" --session "$SID" --model fake/fake-model --format json "turn two from the phone" ) >"$EVIDENCE/phone-run.jsonl" 2>"$EVIDENCE/phone-run.err" || true
if grep -q 'hello from the fake local model' "$EVIDENCE/phone-run.jsonl"; then
  ok "phone-style opencode --server continued the same session"
else
  bad "phone-style opencode --server did not complete a turn"
fi
sleep 0.5
kill "$EVENTS_PID" 2>/dev/null || true
wait "$EVENTS_PID" 2>/dev/null || true
EVENTS_PID=""

curl -sS -u "$AUTH" "$BASE/api/session/$SID/message" >"$EVIDENCE/messages.json"
( cd "$PROJECT" && oe env OPENCODE_PASSWORD="$TOKEN" opencode session export --server "$BASE" "$SID" ) >"$EVIDENCE/session-export.json" 2>"$EVIDENCE/session-export.err" || true
python3 - "$EVIDENCE/messages.json" "$EVIDENCE/session-export.json" <<'PY'
import json, sys
messages = json.load(open(sys.argv[1]))["data"]
assistant = [m for m in messages if m.get("type") == "assistant" and any(p.get("type") == "text" for p in m.get("content", []))]
users = [m for m in messages if m.get("type") == "user"]
export = json.load(open(sys.argv[2]))
print("messages: %d user, %d assistant; export: %d messages" % (len(users), len(assistant), len(export.get("messages", []))))
assert len(users) >= 2, users
assert len(assistant) >= 2, assistant
assert len(export.get("messages", [])) >= 4, export
PY
ok "message list and session export contain both turns" || bad "message list or session export is incomplete"

if [ "$(grep -c 'POST model=fake-model' "$EVIDENCE/fake-openai.log" 2>/dev/null || true)" -ge 2 ]; then
  ok "the fake model answered both turns on 127.0.0.1"
else
  bad "the fake model did not answer both turns"
fi
if grep -q 'tools=.*vowp_spike_echo' "$EVIDENCE/fake-openai.log"; then
  ok "opencode offered the MCP tool to the model"
else
  note "opencode connected the MCP server but did not offer its tool in the model tool list (v2.0.28); see the report"
fi

say "map opencode events to VOWP-shaped events"
python3 "$HERE/map_events.py" --sse "$EVIDENCE/events.sse" --session "$SID" --out "$EVIDENCE/vowp-events.jsonl" >"$EVIDENCE/map-summary.json" || true
cat "$EVIDENCE/map-summary.json"
python3 - "$EVIDENCE/map-summary.json" <<'PY'
import json, sys
s = json.load(open(sys.argv[1]))
c = s["vowp_events"]
assert c.get("user.message", 0) >= 2, c
assert c.get("assistant.message", 0) >= 2, c
assert c.get("turn.started", 0) >= 2, c
assert c.get("turn.ended", 0) >= 2, c
assert c.get("cost", 0) >= 2, c
print("mapped:", c)
PY
ok "live opencode events map to user.message, assistant.message, turn and cost events" || bad "event mapping is incomplete"

say "project-local MCP server in Claude Code"
cat >"$PROJECT/.mcp.json" <<EOF
{
  "mcpServers": {
    "vowp-spike": {
      "type": "stdio",
      "command": "python3",
      "args": ["$HERE/mcp_dummy.py"],
      "env": {"MCP_DUMMY_LOG": "$EVIDENCE/mcp-claude.log"}
    }
  }
}
EOF
( cd "$PROJECT" && claude_local claude mcp list ) >"$EVIDENCE/claude-mcp-pending.txt" 2>&1 || true
grep -q 'Pending approval' "$EVIDENCE/claude-mcp-pending.txt" && ok "Claude Code sees the project .mcp.json (pending the one-time approval)" || note "Claude Code did not report the expected pending approval"

python3 - "$WORK/claude/config/.claude.json" "$PROJECT_REAL" <<'PY'
import json, os, sys
path, project = sys.argv[1], sys.argv[2]
data = {}
if os.path.exists(path):
    try:
        data = json.load(open(path))
    except json.JSONDecodeError:
        data = {}
data.setdefault("projects", {})[project] = {
    "enabledMcpjsonServers": ["vowp-spike"],
    "disabledMcpjsonServers": [],
    "hasTrustDialogAccepted": True,
}
json.dump(data, open(path, "w"), indent=2)
PY
( cd "$PROJECT" && claude_local claude mcp list ) >"$EVIDENCE/claude-mcp-list.txt" 2>&1 || true
cat "$EVIDENCE/claude-mcp-list.txt"
if grep -q 'Connected' "$EVIDENCE/claude-mcp-list.txt"; then
  ok "Claude Code connects to the project-local MCP server"
else
  bad "Claude Code did not connect to the project-local MCP server"
fi
( cd "$PROJECT" && claude_local claude mcp get vowp-spike ) >"$EVIDENCE/claude-mcp-get.txt" 2>&1 || true
grep -q 'Connected' "$EVIDENCE/claude-mcp-get.txt" && ok "claude mcp get reports the server connected" || bad "claude mcp get did not report connected"
if grep -q 'tools_list vowp_spike_echo' "$EVIDENCE/mcp-claude.log" 2>/dev/null; then
  ok "the dummy MCP server answered tools/list for Claude Code"
else
  note "no Claude MCP server log; connection status is the evidence"
fi

say "Claude Remote Control: command surface and the honest limit"
( cd "$PROJECT" && claude_local claude auth status ) >"$EVIDENCE/claude-auth.json" 2>&1 || true
cat "$EVIDENCE/claude-auth.json"
( cd "$PROJECT" && claude_local claude remote-control --help ) >"$EVIDENCE/claude-remote-help.txt" 2>&1 || true
if grep -q 'Remote Control' "$EVIDENCE/claude-remote-help.txt"; then
  ok "this Claude Code build has the Remote Control command"
else
  bad "this Claude Code build has no Remote Control command"
fi
cat >"$EVIDENCE/remote-control-owner-test.md" <<'EOF'
# Claude Code Remote Control owner test (about 5 minutes)

This spike deliberately did not use the build user's claude.ai login. Run this
on a machine where the owner is signed in:

1. `cd ~/.vowp/orchestrators/<name>` (or any scratch directory).
2. `claude remote-control --name "VOWP S3 spike"`.
3. Open https://claude.ai/code or the Claude mobile app, find the session
   "VOWP S3 spike", and send one short message.
4. Confirm the message appears in the terminal and the terminal's answer
   appears in the app.
5. Press Ctrl-C in the terminal. Remote Control must stop; the app must show
   the session as disconnected.

Record: Claude Code version, the app used (desktop/web/mobile), and the time
between sending and seeing the reply.
EOF
note "Claude Remote Control was not exercised with a real login; see remote-control-owner-test.md"

say "notes"
if [ "${#NOTES[@]}" -eq 0 ]; then
  printf 'none\n'
else
  printf '%s\n' "${NOTES[@]}"
fi
printf '\nPASS=%d FAIL=%d\n' "$PASS" "$FAIL"
if [ "$FAIL" -ne 0 ]; then
  printf 'RESULT: FAIL\n'
  exit 1
fi
printf 'RESULT: PASS\n'
