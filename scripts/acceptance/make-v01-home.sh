#!/usr/bin/env bash
# scripts/acceptance/make-v01-home.sh — generate a disposable v0.1 home.
#
# The generator downloads the published VOWP 0.1.0 binary (SHA-256 verified),
# initialises one git project, adds fake-harness jobs, runs the real v0.1
# supervisor until they merge, and stops it. The result is a home with genuine
# v0.1 history for the migration acceptance test (T4):
#
#   <dest>/home/vowp.db           v0.1 database
#   <dest>/project/               the git checkout registered in that database
#   <dest>/origin.git             its local bare origin
#   <dest>/manifest.json          version, job states, row counts, log counts
#   <dest>/fake-worker.log        every fake-harness turn (if any)
#
# It never uses the caller's ~/.vowp, HOME independent of <dest>/runtime, or
# tmux server: a private TMUX_TMPDIR and socket wrapper are created under
# <dest>/runtime. The fake harness never calls a model or the network. The only
# network access is the one-time release download; pass --binary PATH to use a
# local published 0.1.0 binary instead.
#
# Usage:
#   make-v01-home.sh --dest DIR [options]
#
# Options:
#   --dest DIR          output directory (required; must be empty or --force)
#   --binary PATH       use this v0.1.0 binary instead of downloading
#   --version VERSION   release version to download (default 0.1.0)
#   --name NAME         VOWP project name (default legacy)
#   --jobs N            jobs to add and finish (default 2; 1..16)
#   --timeout SECS      supervisor timeout (default 180)
#   --force             replace a non-empty destination
#   -h, --help          show this help
#
# Environment:
#   VOWP_V010_BASE_URL      release download base (default GitHub releases)
#   VOWP_V010_CACHE         archive cache (default ${TMPDIR:-/tmp}/...)
#   VOWP_V010_TARGET        override the release archive target
#
# Exit status:
#   0  a v0.1 home with real history exists and manifest.json was written
#   1  setup or supervisor failed
#   2  bad arguments or a missing tool

set -Eeuo pipefail

unset TMUX TMUX_PANE 2>/dev/null || true

HERE=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=lib.sh
. "$HERE/lib.sh"

VERSION=${VOWP_V01_VERSION:-0.1.0}
BASE_URL=${VOWP_V010_BASE_URL:-https://github.com/syver777/vowp/releases/download}
DEST=
BINARY=
PROJECT_NAME=legacy
JOBS=2
TIMEOUT=180
FORCE=0
STARTED_AT=$(date +%s)

usage() {
    sed -n '2,44p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
}

say() {
    printf 'make-v01-home: %s\n' "$*"
}

die() {
    printf 'make-v01-home: error: %s\n' "$*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --dest)
            [ "$#" -ge 2 ] || die "--dest needs a directory"
            DEST=$2
            shift 2
            ;;
        --binary)
            [ "$#" -ge 2 ] || die "--binary needs a path"
            BINARY=$2
            shift 2
            ;;
        --version)
            [ "$#" -ge 2 ] || die "--version needs a value"
            VERSION=$2
            shift 2
            ;;
        --name)
            [ "$#" -ge 2 ] || die "--name needs a value"
            PROJECT_NAME=$2
            shift 2
            ;;
        --jobs)
            [ "$#" -ge 2 ] || die "--jobs needs a number"
            JOBS=$2
            shift 2
            ;;
        --timeout)
            [ "$#" -ge 2 ] || die "--timeout needs seconds"
            TIMEOUT=$2
            shift 2
            ;;
        --force) FORCE=1; shift ;;
        -h|--help) usage ;;
        *) die "unknown option: $1" ;;
    esac
done

[ -n "$DEST" ] || { printf '%s\n' "make-v01-home: --dest is required" >&2; usage; }
case "$JOBS" in
    ''|*[!0-9]*) die "--jobs must be a whole number" ;;
esac
[ "$JOBS" -ge 1 ] && [ "$JOBS" -le 16 ] || die "--jobs must be between 1 and 16"
case "$TIMEOUT" in
    ''|*[!0-9]*) die "--timeout must be a whole number" ;;
esac

PROJECT_NAME=$(acceptance_slug "$PROJECT_NAME")
[ -n "$PROJECT_NAME" ] || die "project name is empty after slugging"

case "$DEST" in
    ""|/|"$HOME"|"$HOME"/) die "refusing an unsafe destination: '$DEST'" ;;
esac
if [ -e "$DEST" ] && [ -n "$(ls -A "$DEST" 2>/dev/null)" ]; then
    if [ "$FORCE" != "1" ]; then
        die "destination is not empty: $DEST (pass --force to replace it)"
    fi
    rm -rf -- "$DEST"
fi
mkdir -p "$DEST"
DEST=$(CDPATH='' cd -- "$DEST" && pwd)

HOME_DIR="$DEST/home"
PROJECT_DIR="$DEST/project"
ORIGIN="$DEST/origin.git"
RUNTIME="$DEST/runtime"
FAKE_LOG="$DEST/fake-worker.log"
LOG="$DEST/supervisor.log"
SOCKET="vowp-v01-$$"

acceptance_env_init "$RUNTIME" "$SOCKET"
export VOWP_HOME="$HOME_DIR"
export VOWP_ACCEPTANCE_WORKER_LOG="$FAKE_LOG"
mkdir -p "$HOME_DIR" "$PROJECT_DIR"

stop_supervisor() {
    [ -f "$HOME_DIR/supervisor.lock" ] || return 0
    (cd "$PROJECT_DIR" 2>/dev/null && "$BINARY" --home "$HOME_DIR" down --force >/dev/null 2>&1) || true
}

cleanup() {
    status=$?
    stop_supervisor
    acceptance_kill_private_tmux
    if [ "$status" -ne 0 ]; then
        say "failed after $(( $(date +%s) - STARTED_AT ))s; partial output kept at $DEST" >&2
        if [ -f "$LOG" ]; then
            say "supervisor log tail:" >&2
            tail -n 80 "$LOG" >&2 || true
        fi
        if [ -f "$HOME_DIR/logs/supervisor.log" ]; then
            say "vowp log tail:" >&2
            tail -n 80 "$HOME_DIR/logs/supervisor.log" >&2 || true
        fi
    fi
    exit "$status"
}
trap cleanup EXIT INT TERM

# --- download or validate the published v0.1.0 binary ----------------------

fetch() {
    src=$1
    dst=$2
    case "$src" in
        https://*) proto='=https' ;;
        http://localhost|http://localhost:*|http://localhost/*) proto='=http' ;;
        http://*) die "refusing insecure release URL: $src" ;;
        *) die "invalid release URL: $src" ;;
    esac
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 2 --connect-timeout 10 --proto "$proto" --proto-redir "$proto" -o "$dst" "$src" \
            || die "download failed: $src"
    elif command -v wget >/dev/null 2>&1; then
        if [ "$proto" = "=https" ]; then
            wget --https-only -q -O "$dst" "$src" || die "download failed: $src"
        else
            wget -q -O "$dst" "$src" || die "download failed: $src"
        fi
    else
        die "curl or wget is required to download the v0.1.0 release"
    fi
}

detect_target() {
    if [ -n "${VOWP_V010_TARGET:-}" ]; then
        printf '%s\n' "$VOWP_V010_TARGET"
        return 0
    fi
    os=$(uname -s 2>/dev/null || true)
    arch=$(uname -m 2>/dev/null || true)
    case "$os:$arch" in
        Linux:x86_64|Linux:amd64) printf '%s\n' x86_64-unknown-linux-gnu ;;
        Linux:aarch64|Linux:arm64) printf '%s\n' aarch64-unknown-linux-gnu ;;
        Darwin:x86_64|Darwin:amd64) printf '%s\n' x86_64-apple-darwin ;;
        Darwin:arm64|Darwin:aarch64) printf '%s\n' aarch64-apple-darwin ;;
        *) die "unsupported platform: ${os:-unknown} ${arch:-unknown}" ;;
    esac
}

download_binary() {
    target=$(detect_target)
    cache=${VOWP_V010_CACHE:-${TMPDIR:-/tmp}/vowp-acceptance-cache}
    mkdir -p "$cache"
    tag="v${VERSION}"
    archive="vowp-${VERSION}-${target}.tar.gz"
    base="${BASE_URL%/}/$tag"
    archive_path="$cache/$archive"
    sums_path="$cache/SHA256SUMS-$VERSION-$target"

    if [ ! -s "$archive_path" ] || [ ! -s "$sums_path" ]; then
        say "downloading $base/$archive"
        fetch "$base/SHA256SUMS" "$sums_path"
        fetch "$base/$archive" "$archive_path"
    fi

    expected=$(awk -v name="$archive" '$2 == name { print $1; exit }' "$sums_path")
    case "$expected" in
        ''|*[!0-9a-fA-F]*) die "no valid SHA256 for $archive in $sums_path" ;;
    esac
    [ "${#expected}" -eq 64 ] || die "invalid SHA256 length for $archive"
    actual=$(acceptance_sha256_file "$archive_path" | tr 'A-F' 'a-f')
    expected=$(printf '%s\n' "$expected" | tr 'A-F' 'a-f')
    [ "$actual" = "$expected" ] || die "SHA256 mismatch for $archive: expected $expected, got $actual"

    if tar -tzf "$archive_path" | grep -Eq '(^/|(^|/)\.\.(/|$))'; then
        die "archive contains an unsafe path: $archive"
    fi
    extract="$cache/extract-${VERSION}-${target}"
    rm -rf -- "$extract"
    mkdir -p "$extract"
    tar -xzf "$archive_path" -C "$extract"
    found="$extract/vowp-${VERSION}-${target}/vowp"
    if [ ! -x "$found" ]; then
        found=$(find "$extract" -type f -name vowp | head -n 1)
    fi
    [ -n "$found" ] && [ -x "$found" ] || die "archive does not contain an executable vowp"
    BINARY=$found
    ARCHIVE_PATH=$archive_path
    ARCHIVE_SHA256=$actual
}

if [ -n "$BINARY" ]; then
    [ -f "$BINARY" ] || die "binary not found: $BINARY"
    [ -x "$BINARY" ] || die "binary is not executable: $BINARY"
    ARCHIVE_PATH=
    ARCHIVE_SHA256=
else
    download_binary
fi
BINARY=$(CDPATH='' cd -- "$(dirname -- "$BINARY")" && pwd)/$(basename -- "$BINARY")
version_text=$("$BINARY" --version 2>&1) || die "binary failed to run: $BINARY --version"
case "$version_text" in
    "vowp $VERSION"*) ;;
    *) die "expected 'vowp $VERSION', got: $version_text" ;;
esac
BINARY_SHA256=$(acceptance_sha256_file "$BINARY")
say "$version_text at $BINARY"

# --- create the v0.1 home and project --------------------------------------

install -m 0755 "$HERE/fixtures/fake-worker.sh" "$DEST/fake-worker.sh"
mkdir -p "$PROJECT_DIR"

git -C "$PROJECT_DIR" init -q -b main
git -C "$PROJECT_DIR" config user.name "VOWP v0.1 Acceptance Fixture"
git -C "$PROJECT_DIR" config user.email "acceptance@example.invalid"
printf '.vowp/\n' >"$PROJECT_DIR/.gitignore"
printf '%s\n' "# v0.1 acceptance fixture" >"$PROJECT_DIR/README.md"
git -C "$PROJECT_DIR" add .gitignore README.md
git -C "$PROJECT_DIR" commit -q -m "v0.1 fixture: initial commit"
git init -q --bare -b main "$ORIGIN"
git -C "$PROJECT_DIR" remote add origin "$ORIGIN"

(
    cd "$PROJECT_DIR"
    "$BINARY" --home "$HOME_DIR" init --no-framework --name "$PROJECT_NAME"
) >"$DEST/init.log" 2>&1 || {
    cat "$DEST/init.log" >&2
    die "v0.1 init failed"
}

mkdir -p "$PROJECT_DIR/.vowp/adapters"
cat >"$PROJECT_DIR/.vowp/adapters/fake-worker.toml" <<EOF
# Generated by scripts/acceptance/make-v01-home.sh.
name = "fake-worker"
provider = "fake"

[command]
argv = ["$DEST/fake-worker.sh"]
prompt_via = "stdin"

[output]
format = "text"
text_chunk_lines = 1

[exit]
ok = [0]
EOF

cat >"$PROJECT_DIR/vowp.toml" <<EOF
# Generated by scripts/acceptance/make-v01-home.sh. Fake harness only.
[node]
name = "acceptance-v01"
max_workers = 2
max_verifiers = 1
tick_secs = 1
tz_offset_minutes = 0

[project]
name = "$PROJECT_NAME"
remote = "origin"
default_branch = "main"
check_cmds = []
lock_files = ["Cargo.lock"]
always_allowed = ["Cargo.lock"]
push = true

[profiles.planner]
ladder = ["cli:fake-worker"]
on_quota = "park"

[profiles.keeper]
ladder = ["cli:fake-worker"]
on_quota = "park"

[profiles.worker]
ladder = ["cli:fake-worker"]
on_quota = "park"

[profiles.verifier]
ladder = ["cli:fake-worker"]
on_quota = "park"

[harnesses.fake-worker]
mapping = ".vowp/adapters/fake-worker.toml"
EOF

printf '.vowp/\n' >"$PROJECT_DIR/.gitignore"
git -C "$PROJECT_DIR" add -A
git -C "$PROJECT_DIR" commit -q -m "v0.1 fixture: fake harness config"
git -C "$PROJECT_DIR" push -q -u origin main

add_job() {
    id=$1
    body=$(cat <<EOF
# Goal

Create \`src/${id}.txt\` with exactly one line: \`fake acceptance worker finished ${id}\`.

# Read first

- \`README.md\` in this worktree.

# Deliverables

- \`src/${id}.txt\`.

# Tests required

- The report gate must pass and the changed path must stay inside \`owns\`.

# Done when

\`src/${id}.txt\` exists and the report has \`status: done\`.
EOF
)
    (
        cd "$PROJECT_DIR"
        printf '%s\n' "$body" | "$BINARY" --home "$HOME_DIR" job add \
            --id "$id" \
            --title "v0.1 fixture $id" \
            --kind build \
            --owns "src/${id}.txt" \
            --gates "report,owns_respected" \
            --brief - >/dev/null
    ) || die "v0.1 job add failed for $id"
}

index=1
while [ "$index" -le "$JOBS" ]; do
    add_job "v01-job-$index"
    index=$((index + 1))
done

say "starting the v0.1 supervisor for $JOBS job(s)"
(
    cd "$PROJECT_DIR"
    "$BINARY" --home "$HOME_DIR" up --detach
) >"$DEST/up.log" 2>&1 || {
    cat "$DEST/up.log" >&2
    die "v0.1 up --detach failed"
}

deadline=$(( $(date +%s) + TIMEOUT ))
final_states=
while :; do
    if (cd "$PROJECT_DIR" && "$BINARY" --home "$HOME_DIR" --json job ls --all) >"$DEST/jobs.json" 2>"$DEST/jobs.err"; then
        final_states=$(
            python3 - "$DEST/jobs.json" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as handle:
    rows = json.load(handle)
if isinstance(rows, dict):
    rows = rows.get("jobs", [])
if not rows:
    raise SystemExit("no jobs")
print(",".join(f"{row.get('id')}={row.get('state')}" for row in rows))
PY
        ) || final_states=
        case "$final_states" in
            *"=failed"*|*"=blocked"*|*"=parked"*|*"=stalled"*)
                die "a v0.1 job did not finish: $final_states"
                ;;
        esac
        all_done=1
        IFS=',' read -r -a states <<<"$final_states"
        for state in "${states[@]}"; do
            case "$state" in
                *=done) ;;
                *) all_done=0 ;;
            esac
        done
        [ "$all_done" -eq 1 ] && [ -n "$final_states" ] && break
    fi
    if [ "$(date +%s)" -ge "$deadline" ]; then
        die "timed out after ${TIMEOUT}s waiting for v0.1 jobs: ${final_states:-unknown}"
    fi
    sleep 1
done

say "jobs finished: $final_states"
stop_supervisor
sleep 1
acceptance_kill_private_tmux

# --- manifest --------------------------------------------------------------

python3 - "$DEST" "$HOME_DIR" "$PROJECT_DIR" "$PROJECT_NAME" "$JOBS" \
    "$version_text" "$BINARY" "$BINARY_SHA256" "${ARCHIVE_PATH:-}" "${ARCHIVE_SHA256:-}" <<'PY'
import datetime
import json
import os
import pathlib
import sqlite3
import sys

dest = pathlib.Path(sys.argv[1])
home = pathlib.Path(sys.argv[2])
project = pathlib.Path(sys.argv[3])
project_name = sys.argv[4]
jobs_requested = int(sys.argv[5])
version_text = sys.argv[6]
binary = sys.argv[7]
binary_sha = sys.argv[8]
archive = sys.argv[9]
archive_sha = sys.argv[10]

db = home / "vowp.db"
connection = sqlite3.connect(db)
connection.row_factory = sqlite3.Row
try:
    user_version = connection.execute("PRAGMA user_version").fetchone()[0]
    tables = [
        row[0]
        for row in connection.execute(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
        )
    ]
    row_counts = {}
    for table in tables:
        row_counts[table] = connection.execute(f'SELECT COUNT(*) FROM "{table}"').fetchone()[0]
    jobs = [
        dict(row)
        for row in connection.execute(
            "SELECT id, state, kind, role FROM jobs ORDER BY id"
        )
    ]
finally:
    connection.close()

session_logs = sorted((home / "sessions").glob("*/log.jsonl"))
session_bytes = sum(path.stat().st_size for path in session_logs)
run_files = sorted((home / "runs").glob("*.jsonl"))

manifest = {
    "format": 1,
    "kind": "v0.1-home",
    "created_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "vowp_version": version_text[5:] if version_text.startswith("vowp ") else version_text,
    "vowp_version_text": version_text,
    "vowp_binary": binary,
    "vowp_binary_sha256": binary_sha,
    "release_archive": archive or None,
    "release_archive_sha256": archive_sha or None,
    "home": str(home),
    "project": str(project),
    "project_name": project_name,
    "database": str(db),
    "schema_version": user_version,
    "row_counts": row_counts,
    "jobs_requested": jobs_requested,
    "jobs": jobs,
    "sessions": {
        "count": len(session_logs),
        "log_files": [str(path) for path in session_logs],
        "bytes": session_bytes,
    },
    "runs": {
        "count": len(run_files),
        "files": [str(path) for path in run_files],
    },
}
with open(dest / "manifest.json", "w", encoding="utf-8") as handle:
    json.dump(manifest, handle, indent=2, sort_keys=True)
    handle.write("\n")
print(json.dumps(manifest, indent=2, sort_keys=True))
PY

cat >"$DEST/README.md" <<EOF
# Disposable VOWP v0.1 home

Generated by \`scripts/acceptance/make-v01-home.sh\` at $(date -u '+%Y-%m-%dT%H:%M:%SZ').

- Database: \`home/vowp.db\` (v0.1 schema)
- Project: \`project/\` (registered at that absolute path)
- Local origin: \`origin.git\`
- Version: $version_text
- Jobs: $JOBS, all finished through the real v0.1 supervisor
- Evidence: \`manifest.json\` (row counts and session/run counts)

The project path is recorded in the database, so use this directory in place.
Do not move \`home/\` without also moving \`project/\` and updating the row.
For migration, point the v0.2 binary at this home:

    vowp --home "$HOME_DIR" migrate

The home was produced with a private \`TMUX_TMPDIR\`, a private tmux socket and
a fake harness. No real harness or model was called.
EOF

say "wrote $DEST/manifest.json"
say "done in $(( $(date +%s) - STARTED_AT ))s"
