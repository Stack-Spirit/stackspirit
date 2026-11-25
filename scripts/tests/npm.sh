#!/usr/bin/env bash
# Offline end-to-end check of the `stackspirit` npm launcher with the real engine binary.
#
# Builds the vowp binary (or uses VOWP_BIN), packs the five npm tarballs from it, installs them
# into a temporary project with no network, and proves `stackspirit --version` and
# `stackspirit mcp` (read-only, no flags) through the launcher. Hermetic: temporary HOME and
# VOWP_HOME, a private TMUX_TMPDIR, no registry, no tmux server started.
#
# Run: bash scripts/tests/npm.sh
set -euo pipefail

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
TMP=$(mktemp -d "${TMPDIR:-/tmp}/stackspirit-npm-e2e.XXXXXX")
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

fail() { echo "npm.sh: FAIL: $*" >&2; exit 1; }
ok() { echo "ok - $*"; }

command -v node >/dev/null 2>&1 && command -v npm >/dev/null 2>&1 || fail "node and npm are required"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) key=linux-x64 ;;
  Linux-aarch64|Linux-arm64) key=linux-arm64 ;;
  Darwin-x86_64) key=darwin-x64 ;;
  Darwin-arm64) key=darwin-arm64 ;;
  *) fail "unsupported host $(uname -s)-$(uname -m)" ;;
esac

if [ -n "${VOWP_BIN:-}" ]; then
  bin=$VOWP_BIN
else
  (cd "$ROOT" && cargo build --quiet -p vowp-cli --bin vowp) || fail "cargo build failed"
  target_dir=$(cd "$ROOT" && cargo metadata --format-version 1 --no-deps | node -p 'JSON.parse(require("fs").readFileSync(0,"utf8")).target_directory')
  bin=$target_dir/debug/vowp
fi
[ -x "$bin" ] || fail "no engine binary at $bin"

version=$(node -p 'require(process.argv[1]).version' "$ROOT/packaging/npm/stackspirit/package.json")
export HOME=$TMP/home VOWP_HOME=$TMP/vowp-home TMUX_TMPDIR=$TMP/tmux
mkdir -p "$HOME" "$VOWP_HOME" "$TMUX_TMPDIR" "$TMP/bins"
unset TMUX TMUX_PANE VOWP_CONTROL_HOME
export npm_config_offline=true npm_config_audit=false npm_config_fund=false npm_config_update_notifier=false
export npm_config_cache=$TMP/npm-cache npm_config_userconfig=$TMP/npmrc npm_config_globalconfig=$TMP/gnpmrc

# Only `stackspirit` is the real engine (stripped: a debug build is hundreds of MiB, over the
# 64 MiB tarball cap). The other three names are tiny stand-ins so the four-file contract holds.
cp "$bin" "$TMP/bins/stackspirit"
strip "$TMP/bins/stackspirit" 2>/dev/null || true
chmod 0755 "$TMP/bins/stackspirit"
for name in vowp stackspirit-daemon vowp-daemon; do
  printf '#!/bin/sh\necho "standin:%s $*"\n' "$name" > "$TMP/bins/$name"
  chmod 0755 "$TMP/bins/$name"
done

# Every platform target gets the same four files; the build script's contract is unchanged.
args=(--version "$version" --out "$TMP/out")
for k in linux-x64 linux-arm64 darwin-x64 darwin-arm64; do args+=(--target "$k=$TMP/bins"); done
bash "$ROOT/scripts/release/build-npm-packages.sh" "${args[@]}" >"$TMP/build.log" 2>&1 || { cat "$TMP/build.log" >&2; fail "build-npm-packages.sh"; }
[ "$(ls "$TMP/out"/*.tgz | wc -l)" -eq 5 ] || fail "expected five tarballs"
(cd "$TMP/out" && (sha256sum -c SHA256SUMS >/dev/null 2>&1 || shasum -a 256 -c SHA256SUMS >/dev/null 2>&1)) || fail "SHA256SUMS does not verify"
ok "five tarballs and a verifying SHA256SUMS"

project=$TMP/project
mkdir -p "$project"
cat > "$project/package.json" <<JSON
{"name":"npm-e2e","version":"1.0.0","private":true,
 "dependencies":{"stackspirit":"file:$TMP/out/stackspirit-$version.tgz"},
 "overrides":{"@stackspirit/$key":"file:$TMP/out/stackspirit-$key-$version.tgz"}}
JSON
(cd "$project" && npm install --ignore-scripts --offline --no-audit --no-fund >"$TMP/install.log" 2>&1) || { cat "$TMP/install.log" >&2; fail "offline npm install"; }
[ -x "$project/node_modules/@stackspirit/$key/bin/stackspirit" ] || fail "platform binary not installed"
ok "offline install of the packed tarballs"

out=$(cd "$project" && npx --no-install stackspirit --version) || fail "stackspirit --version failed"
case $out in *stackspirit*|*StackSpirit*) ok "stackspirit --version: $out" ;; *) fail "unexpected --version output: $out" ;; esac
vout=$(cd "$project" && npx --no-install vowp --version) || fail "vowp shim failed"
[ "$vout" = "standin:vowp --version" ] || fail "vowp shim ran the wrong file: $vout"
ok "vowp shim runs bin/vowp with its arguments"

# stackspirit mcp: no flags, read-only, through the launcher.
requests=$(cat <<'JSON'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{}}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"capacity_set","arguments":{"target":2}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"job_add","arguments":{}}}
JSON
)
responses=$(cd "$project" && printf '%s\n' "$requests" | npx --no-install stackspirit mcp) || fail "stackspirit mcp exited non-zero"
printf '%s\n' "$responses" | node -e '
  const lines = require("fs").readFileSync(0, "utf8").trim().split("\n").map((l) => JSON.parse(l));
  const by = (id) => lines.find((m) => m.id === id);
  const assert = require("assert");
  assert.equal(by(1).result.serverInfo.name, "stackspirit-engine");
  assert.equal(by(1).result.protocolVersion, "2025-03-26");
  const names = by(2).result.tools.map((t) => t.name).sort();
  assert.deepEqual(names, ["capacity_report", "fleet_queue", "fleet_status", "inbox_list", "session_tail"]);
  for (const id of [3, 4]) {
    assert.equal(by(id).error.code, -32601);
    assert.match(by(id).error.message, /--session/);
  }
' || fail "read-only MCP responses are wrong"
ok "stackspirit mcp lists five read tools and refuses writes with -32601"

# A partial flag group must fail, not fall back to read-only.
if (cd "$project" && npx --no-install stackspirit mcp --surface s </dev/null >"$TMP/partial.out" 2>"$TMP/partial.err"); then
  fail "partial flag group must fail"
fi
grep -q -- '--token-file' "$TMP/partial.err" || fail "partial flag error does not name the missing flag"
ok "a partial flag group is an error"

# --omit=optional: the launcher reports the reinstall message.
rm -rf "$project/node_modules/@stackspirit"
if (cd "$project" && npx --no-install stackspirit --version >"$TMP/o.out" 2>"$TMP/o.err"); then fail "missing platform package must exit non-zero"; fi
grep -q 'without --omit=optional' "$TMP/o.err" || fail "missing the reinstall message"
ok "missing platform package prints the reinstall message"

node --test "$ROOT"/packaging/npm/test/*.test.js >"$TMP/node-test.log" 2>&1 || { cat "$TMP/node-test.log" >&2; fail "node tests"; }
ok "node --test packaging/npm/test/*.test.js"
echo "npm.sh: all checks passed"
