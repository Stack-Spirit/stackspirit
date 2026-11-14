#!/usr/bin/env bash
# Verify the fetched StackSpirit Engine sidecars against the generated lock file.
#
# The pinned archive digest lives in engine/manifest.json and is checked by
# scripts/fetch-engine.mjs before extraction. This script re-checks the extracted
# binaries against the digest recorded at fetch time, so a CI host cannot bundle a
# binary that changed after verification.
#
# `--self-test` proves that a checksum mismatch fails using a small fixture under
# engine/fixtures/. It creates no network traffic and writes only to a temp dir.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURE="$ROOT/engine/fixtures/self-test.txt"
FIXTURE_DIGEST_FILE="$ROOT/engine/fixtures/self-test.sha256"
BINARIES_DIR="$ROOT/apps/spirit-desktop/src-tauri/binaries"
LOCK_FILE="$BINARIES_DIR/engine-lock.json"
MANIFEST_FILE="$ROOT/engine/manifest.json"

usage() {
  cat >&2 <<'EOF'
usage: bash scripts/verify-engine-checksum.sh [--self-test]

Without arguments, verifies every extracted sidecar against
apps/spirit-desktop/src-tauri/binaries/engine-lock.json.
--self-test proves that a wrong digest is rejected.
EOF
  exit 2
}

hash_file() {
  local file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    echo "verify-engine-checksum: no sha256sum or shasum on PATH" >&2
    return 1
  fi
}

# verify_checksum <file> <expected> <label>
verify_checksum() {
  local file="$1"
  local expected="$2"
  local label="$3"
  if [[ ! -f "$file" ]]; then
    echo "verify-engine-checksum: missing $label ($file)" >&2
    return 1
  fi
  local actual
  actual="$(hash_file "$file")"
  if [[ "$actual" != "$expected" ]]; then
    echo "verify-engine-checksum: checksum mismatch for $label" >&2
    echo "  expected: $expected" >&2
    echo "  actual:   $actual" >&2
    return 1
  fi
  echo "verify-engine-checksum: $label ok"
}

self_test() {
  local expected
  expected="$(tr -d '[:space:]' <"$FIXTURE_DIGEST_FILE")"
  if [[ ${#expected} -ne 64 ]]; then
    echo "verify-engine-checksum: fixture digest is not a SHA-256" >&2
    exit 1
  fi
  verify_checksum "$FIXTURE" "$expected" "self-test fixture"
  local mismatch="0000000000000000000000000000000000000000000000000000000000000000"
  if verify_checksum "$FIXTURE" "$mismatch" "self-test mismatch" >/dev/null 2>&1; then
    echo "verify-engine-checksum: self-test FAILED: a wrong checksum was accepted" >&2
    exit 1
  fi
  echo "verify-engine-checksum: self-test ok (a wrong checksum is rejected)"
}

verify_binaries() {
  if [[ ! -f "$MANIFEST_FILE" ]]; then
    echo "verify-engine-checksum: missing engine/manifest.json" >&2
    exit 1
  fi
  if [[ ! -f "$LOCK_FILE" ]]; then
    echo "verify-engine-checksum: no engine lock file; run node scripts/fetch-engine.mjs first" >&2
    exit 1
  fi

  local rows
  rows="$(
    node - "$MANIFEST_FILE" "$LOCK_FILE" <<'NODE'
const fs = require("node:fs");
const manifest = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const lock = JSON.parse(fs.readFileSync(process.argv[3], "utf8"));
if (lock.schema !== "stackspirit-engine-lock/1") {
  console.error("verify-engine-checksum: unsupported lock schema");
  process.exit(1);
}
for (const triple of Object.keys(manifest.targets ?? {})) {
  const entry = lock.targets?.[triple];
  if (!entry) continue;
  if (entry.version !== manifest.version) {
    console.error(`verify-engine-checksum: ${triple} was fetched for ${entry.version}, manifest pins ${manifest.version}`);
    process.exit(1);
  }
  const binary = `opencode-${triple}${triple.includes("windows") ? ".exe" : ""}`;
  console.log(`${binary}\t${entry.binary_sha256}`);
}
NODE
  )"

  if [[ -z "$rows" ]]; then
    echo "verify-engine-checksum: no sidecars recorded in $LOCK_FILE; run node scripts/fetch-engine.mjs" >&2
    exit 1
  fi

  local checked=0
  while IFS=$'\t' read -r binary expected; do
    [[ -z "$binary" ]] && continue
    verify_checksum "$BINARIES_DIR/$binary" "$expected" "$binary"
    checked=$((checked + 1))
  done <<<"$rows"

  if [[ "$checked" -eq 0 ]]; then
    echo "verify-engine-checksum: no sidecars were verified" >&2
    exit 1
  fi
  echo "verify-engine-checksum: checked $checked sidecar(s)"
}

case "${1:-}" in
  --self-test)
    [[ $# -eq 1 ]] || usage
    self_test
    ;;
  --help | -h)
    usage
    ;;
  "")
    verify_binaries
    ;;
  *)
    usage
    ;;
esac
