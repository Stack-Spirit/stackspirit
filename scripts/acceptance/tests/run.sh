#!/usr/bin/env bash
# scripts/acceptance/tests/run.sh — end-to-end self-test for the acceptance rig.
#
# It builds both VPS variants and the phone image (unless --no-build), starts
# each rig, checks SSH/sudo/phone/privacy, creates both fixture projects, runs
# the v0.1 home generator inside the container (downloading the published
# binary), and tears everything down. It never touches the host's ~/.vowp or
# tmux server: all containers and state live under ${TMPDIR:-/tmp}/...
#
# Usage:
#   bash scripts/acceptance/tests/run.sh
#   bash scripts/acceptance/tests/run.sh --no-build
#   bash scripts/acceptance/tests/run.sh --keep
#   bash scripts/acceptance/tests/run.sh --skip-v01
#
# Requirements: docker (daemon reachable), bash, ssh, ssh-keygen. The v0.1
# check needs network access to the GitHub release (or pass --skip-v01).
#
# Exit status 0 means every assertion passed.

set -Eeuo pipefail

unset TMUX TMUX_PANE 2>/dev/null || true

HERE=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
ACCEPTANCE_DIR=$(CDPATH='' cd -- "$HERE/.." && pwd)
RIG="$ACCEPTANCE_DIR/rig.sh"
PREFIX=selftest-$$
NO_BUILD=0
KEEP=0
SKIP_V01=0
V01_JOBS=2
VPS_IMAGE_BASE=${VOWP_ACCEPTANCE_VPS_IMAGE:-vowp-acceptance/vps}
PHONE_IMAGE=${VOWP_ACCEPTANCE_PHONE_IMAGE:-vowp-acceptance/phone:local}
STARTED_AT=$(date +%s)

say() {
    printf 'acceptance-selftest: %s\n' "$*"
}

fail() {
    printf 'acceptance-selftest: FAIL: %s\n' "$*" >&2
    exit 1
}

usage() {
    sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --no-build) NO_BUILD=1; shift ;;
        --keep) KEEP=1; shift ;;
        --skip-v01) SKIP_V01=1; shift ;;
        --prefix)
            [ "$#" -ge 2 ] || fail "--prefix needs a value"
            PREFIX=$2
            shift 2
            ;;
        -h|--help) usage ;;
        *) fail "unknown option: $1" ;;
    esac
done

case "$PREFIX" in
    ''|*[!A-Za-z0-9_.-]*) fail "invalid prefix: $PREFIX" ;;
esac

require() {
    command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}
require docker
require ssh
require ssh-keygen
docker version >/dev/null 2>&1 || fail "docker daemon is not reachable"

# Every rig this test creates is named <prefix>-<stage>; cleanup removes only
# those containers and state directories. It never prunes unrelated containers.
created=()
cleanup() {
    status=$?
    trap - EXIT INT TERM
    if [ "$status" -ne 0 ]; then
        printf '%s\n' "acceptance-selftest: failed after $(( $(date +%s) - STARTED_AT ))s" >&2
        for name in "${created[@]}"; do
            [ -n "$name" ] || continue
            bash "$RIG" logs --name "$name" --tail 40 >&2 || true
        done
        printf '%s\n' "acceptance-selftest: kept rigs for inspection: ${created[*]:-<none>}" >&2
        printf '%s\n' "acceptance-selftest: remove them with: bash $RIG down --name <name>" >&2
    else
        if [ "$KEEP" -eq 1 ]; then
            printf '%s\n' "acceptance-selftest: kept rigs: ${created[*]:-<none>}" >&2
        else
            for name in "${created[@]}"; do
                [ -n "$name" ] || continue
                bash "$RIG" down --name "$name" >/dev/null 2>&1 || true
            done
        fi
    fi
    exit "$status"
}
trap cleanup EXIT INT TERM

assert_contains() {
    local haystack=$1
    local needle=$2
    local description=$3
    case "$haystack" in
        *"$needle"*) ;;
        *) fail "$description (wanted '$needle' in: $haystack)" ;;
    esac
}

assert_not_contains() {
    local haystack=$1
    local needle=$2
    local description=$3
    case "$haystack" in
        *"$needle"*) fail "$description (did not want '$needle')" ;;
        *) ;;
    esac
}

assert_rig_containers_gone() {
    local label=$1
    local name=$2
    local home=$3
    local phone=$4
    local network=$5
    local listing

    if docker inspect "$home" >/dev/null 2>&1; then
        fail "$label: down left home container $home"
    fi
    if docker inspect "$phone" >/dev/null 2>&1; then
        fail "$label: down left phone container $phone"
    fi
    if docker network inspect "$network" >/dev/null 2>&1; then
        fail "$label: down left network $network"
    fi
    listing=$(bash "$RIG" list) || fail "$label: rig.sh list failed"
    if printf '%s\n' "$listing" | awk -v n="$name" '$3 == n { found = 1 } END { exit(found ? 0 : 1) }'; then
        fail "$label: down left the rig visible in rig.sh list"
    fi
}

assert_rig_gone() {
    local label=$1
    local name=$2
    local home=$3
    local phone=$4
    local network=$5
    local state=$6

    assert_rig_containers_gone "$label" "$name" "$home" "$phone" "$network"
    if [ -e "$state" ]; then
        fail "$label: down left state at $state"
    fi
}

file_sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        fail "sha256sum or shasum is required"
    fi
}

# --- static checks and the host-side privacy helper ------------------------

say "checking script syntax"
while IFS= read -r script; do
    bash -n "$script" || fail "bash -n failed: $script"
done < <(find "$ACCEPTANCE_DIR" -type f -name '*.sh' | sort)

say "checking the host-side private tmux wrapper"
host_root=$(mktemp -d "${TMPDIR:-/tmp}/vowp-acceptance-selftest.XXXXXX")
(
    # shellcheck source=../lib.sh
    . "$ACCEPTANCE_DIR/lib.sh"
    acceptance_env_init "$host_root/runtime" "selftest-host-$$"
    [ -d "$TMUX_TMPDIR" ] || exit 1
    if tmux -L "$(printf '%s' default)" ls >/dev/null 2>&1; then
        exit 1
    fi
    if TMUX_TMPDIR= tmux ls >/dev/null 2>&1; then
        exit 1
    fi
    if tmux -S /tmp/forbidden-tmux-socket ls >/dev/null 2>&1; then
        exit 1
    fi
    case "$(tmux -V 2>&1)" in
        tmux\ *) ;;
        *) exit 1 ;;
    esac
    acceptance_kill_private_tmux
) || fail "the host-side private tmux helper did not behave"
rm -rf -- "$host_root"

say "checking unsafe rig names and destinations are refused before writes"
guard_root=$(mktemp -d "${TMPDIR:-/tmp}/vowp-acceptance-guard.XXXXXX")
mkdir -p "$guard_root/names" "$guard_root/child"
for bad_name in . .. .hidden; do
    if VOWP_ACCEPTANCE_STATE_DIR="$guard_root/names/state" \
        bash "$RIG" up --name "$bad_name" --no-build >/dev/null 2>&1; then
        fail "up accepted the unsafe rig name '$bad_name'"
    fi
done
if [ -n "$(ls -A "$guard_root/names" 2>/dev/null)" ]; then
    fail "an unsafe rig name wrote state under the state root"
fi
if (cd "$guard_root/child" && bash "$ACCEPTANCE_DIR/fixtures/project.sh" hello ../escape >/dev/null 2>&1); then
    fail "fixtures/project.sh accepted a '..' destination"
fi
[ ! -e "$guard_root/escape" ] || fail "fixtures/project.sh escaped its destination"
if (cd "$guard_root/child" && bash "$ACCEPTANCE_DIR/make-v01-home.sh" --dest ../escape >/dev/null 2>&1); then
    fail "make-v01-home.sh accepted a '..' destination"
fi
[ ! -e "$guard_root/escape" ] || fail "make-v01-home.sh escaped its destination"
rm -rf -- "$guard_root"

# --- build -----------------------------------------------------------------

if [ "$NO_BUILD" -eq 1 ]; then
    say "skipping image build (--no-build)"
    docker image inspect "$VPS_IMAGE_BASE:sudo" >/dev/null 2>&1 || fail "missing image $VPS_IMAGE_BASE:sudo"
    docker image inspect "$VPS_IMAGE_BASE:no-sudo" >/dev/null 2>&1 || fail "missing image $VPS_IMAGE_BASE:no-sudo"
    docker image inspect "$PHONE_IMAGE" >/dev/null 2>&1 || fail "missing image $PHONE_IMAGE"
else
    say "building images"
    bash "$RIG" build --variant all || fail "image build failed"
fi

# --- sudo variant ----------------------------------------------------------

sudo_name="$PREFIX-sudo"
created+=("$sudo_name")
say "starting the sudo rig ($sudo_name)"
bash "$RIG" up --name "$sudo_name" --variant sudo || fail "sudo rig did not start"
eval "$(bash "$RIG" env --name "$sudo_name")" || fail "rig.sh env failed for the sudo rig"
sudo_home=$RIG_HOME_CONTAINER
sudo_phone=$RIG_PHONE_CONTAINER
sudo_network=$RIG_NETWORK
sudo_state=$RIG_STATE_DIR

say "checking SSH, sudo and the fresh-VPS contract"
out=$(bash "$RIG" ssh --name "$sudo_name" 'id -un; hostname; command -v sudo; command -v vowp || true') \
    || fail "SSH into the sudo rig failed"
assert_contains "$out" "vowp" "SSH user is not vowp"
assert_contains "$out" "vps" "container hostname is not vps"
assert_contains "$out" "/usr/bin/sudo" "sudo is missing from the sudo variant"
assert_not_contains "$out" "/vowp" "the image already contains a vowp binary"

bash "$RIG" ssh --name "$sudo_name" 'sudo -n true' || fail "passwordless sudo does not work"
bash "$RIG" ssh --name "$sudo_name" 'test -d "$TMUX_TMPDIR" && test -n "$VOWP_ACCEPTANCE_TMUX_SOCKET"' \
    || fail "the SSH session has no private tmux environment"
bash "$RIG" ssh --name "$sudo_name" 'case "$TMUX_TMPDIR" in /home/vowp/*) ;; *) exit 1;; esac' \
    || fail "TMUX_TMPDIR is not under the user's home"
out=$(bash "$RIG" ssh --name "$sudo_name" 'socket=$(printf "%s" default); tmux -L "$socket" ls' 2>&1 || true)
assert_contains "$out" "refusing the default tmux socket" "the image tmux wrapper did not refuse the default socket"
out=$(bash "$RIG" ssh --name "$sudo_name" 'TMUX_TMPDIR= tmux ls' 2>&1 || true)
assert_contains "$out" "refusing an unset or shared TMUX_TMPDIR" "the image tmux wrapper did not refuse an empty TMUX_TMPDIR"
out=$(bash "$RIG" ssh --name "$sudo_name" 'tmux -S /tmp/forbidden-tmux-socket ls' 2>&1 || true)
assert_contains "$out" "refusing an explicit -S socket path" "the image tmux wrapper did not refuse an explicit -S"

say "checking the phone's separate SSH connection"
out=$(bash "$RIG" phone --name "$sudo_name" 'echo phone-ok; id -un; hostname') \
    || fail "the phone client could not reach the home"
assert_contains "$out" "phone-ok" "phone command did not run"
assert_contains "$out" "vowp" "phone did not log in as vowp"
assert_contains "$out" "vps" "phone reached the wrong host"

say "checking env, status, list, logs, the phone key and the RIG_NAME default"
out=$(bash "$RIG" ssh 'echo default-name-ok') \
    || fail "commands after 'eval \"\$(rig.sh env)\"' do not use RIG_NAME"
assert_contains "$out" "default-name-ok" "the RIG_NAME default did not reach ssh"
out=$(bash "$RIG" status --name "$sudo_name") || fail "rig.sh status failed"
assert_contains "$out" "home:      running" "status does not report the running home"
out=$(bash "$RIG" list) || fail "rig.sh list failed"
assert_contains "$out" "$sudo_name" "list does not show the running rig"
out=$(bash "$RIG" logs --name "$sudo_name" --tail 2) || fail "rig.sh logs failed"
assert_contains "$out" "--- home" "logs do not include the home container"
out=$(bash "$RIG" ssh --as phone --name "$sudo_name" 'echo phone-key-ok') \
    || fail "ssh --as phone failed"
assert_contains "$out" "phone-key-ok" "ssh --as phone did not use the phone key"

say "creating fixture projects"
bash "$RIG" fixture --name "$sudo_name" hello /home/vowp/projects/hello \
    || fail "hello fixture creation failed"
bash "$RIG" fixture --name "$sudo_name" memory-hog /home/vowp/projects/hog --hog-mb 32 --hog-hold 2 \
    || fail "memory-hog fixture creation failed"
out=$(bash "$RIG" ssh --name "$sudo_name" '
set -e
test -x /home/vowp/projects/hello/.acceptance/bin/fake-worker
test -x /home/vowp/projects/hog/.acceptance/bin/hog-worker
test -f /home/vowp/projects/hello/vowp.toml
test -f /home/vowp/projects/hello/.acceptance/seed-jobs.sh
test -f /home/vowp/projects/hello-origin.git/HEAD
test -f /home/vowp/projects/hog-origin.git/HEAD
/home/vowp/projects/hello/.acceptance/bin/fake-worker --version
')
assert_contains "$out" "acceptance-fake-worker 1.0.0" "the fake worker does not answer --version"

say "checking fixture-pair and seed-jobs.sh"
bash "$RIG" fixture-pair --name "$sudo_name" /home/vowp/projects/pair >/dev/null \
    || fail "fixture-pair failed"
bash "$RIG" ssh --name "$sudo_name" '
set -e
test -f /home/vowp/projects/pair/hello/vowp.toml
test -f /home/vowp/projects/pair/memory-hog/vowp.toml
cat >/home/vowp/fake-vowp <<"EOS"
#!/bin/sh
printf "%s\n" "$*" >> /home/vowp/seed-args
EOS
chmod +x /home/vowp/fake-vowp
rm -f /home/vowp/seed-args
cd /home/vowp/projects/hello
VOWP=/home/vowp/fake-vowp .acceptance/seed-jobs.sh 2 fixture >/dev/null
test "$(wc -l < /home/vowp/seed-args)" -eq 2
grep -q -- "--id fixture-1" /home/vowp/seed-args
grep -q -- "--id fixture-2" /home/vowp/seed-args
' || fail "seed-jobs.sh did not add the expected jobs"

say "running the memory-hog worker once"
out=$(bash "$RIG" ssh --name "$sudo_name" '
set -e
cd /home/vowp/projects/hog
start=$(date +%s)
printf "%s\n" "You are the worker for job hog-smoke of project hog. write your report to /home/vowp/projects/hog/.vowp/jobs/hog-smoke/report.md" \
    | VOWP_ACCEPTANCE_WORKER_LOG=/home/vowp/hog-worker.log .acceptance/bin/hog-worker
end=$(date +%s)
test $((end - start)) -ge 2
test -f src/hog-smoke.txt
test -f .vowp/jobs/hog-smoke/report.md
grep -q "allocating 32MiB" /home/vowp/hog-worker.log
echo "hog-worker-ok elapsed=$((end-start))"
')
assert_contains "$out" "hog-worker-ok" "the memory-hog worker did not finish"

if [ "$SKIP_V01" -eq 0 ]; then
    say "generating a published v0.1 home inside the rig (network)"
    bash "$RIG" v01-home --name "$sudo_name" --dest /home/vowp/acceptance/v01 --jobs "$V01_JOBS" \
        || fail "the v0.1 home generator failed"
    out=$(bash "$RIG" ssh --name "$sudo_name" "jq -e '.vowp_version == \"0.1.0\" and .schema_version == 1 and (.jobs | length) == $V01_JOBS and ([.jobs[].state] | all(. == \"done\")) and .row_counts.jobs == $V01_JOBS and .row_counts.runs >= $V01_JOBS and .sessions.count >= 1' /home/vowp/acceptance/v01/manifest.json")
    assert_contains "$out" "true" "the v0.1 manifest assertions failed"
    bash "$RIG" ssh --name "$sudo_name" 'test ! -e /home/vowp/.vowp' \
        || fail "the v0.1 generator touched /home/vowp/.vowp"
fi

if [ "$KEEP" -eq 1 ]; then
    say "keeping the sudo rig ($sudo_name)"
else
    say "stopping the sudo rig"
    bash "$RIG" down --name "$sudo_name" || fail "down failed for the sudo rig"
    assert_rig_gone "sudo rig" "$sudo_name" "$sudo_home" "$sudo_phone" "$sudo_network" "$sudo_state"
    created=("${created[@]/$sudo_name/}")
fi

# --- no-sudo variant -------------------------------------------------------

nosudo_name="$PREFIX-nosudo"
created+=("$nosudo_name")
say "starting the no-sudo rig ($nosudo_name)"
bash "$RIG" up --name "$nosudo_name" --variant no-sudo || fail "no-sudo rig did not start"
eval "$(bash "$RIG" env --name "$nosudo_name")" || fail "rig.sh env failed for the no-sudo rig"
nosudo_home=$RIG_HOME_CONTAINER
nosudo_phone=$RIG_PHONE_CONTAINER
nosudo_network=$RIG_NETWORK
nosudo_state=$RIG_STATE_DIR

out=$(bash "$RIG" ssh --name "$nosudo_name" 'id -u; command -v sudo || true; command -v vowp || true') \
    || fail "SSH into the no-sudo rig failed"
assert_contains "$out" "1001" "the no-sudo rig did not run as the unprivileged user"
assert_not_contains "$out" "sudo" "the no-sudo image contains sudo"
assert_not_contains "$out" "/vowp" "the no-sudo image contains a vowp binary"

bash "$RIG" phone --name "$nosudo_name" 'echo phone-nosudo-ok' >/dev/null || fail "phone failed on the no-sudo rig"
bash "$RIG" fixture --name "$nosudo_name" hello /home/vowp/projects/hello \
    || fail "fixture creation failed on the no-sudo rig"
bash "$RIG" ssh --name "$nosudo_name" 'test -x /home/vowp/projects/hello/.acceptance/bin/fake-worker' \
    || fail "the no-sudo fixture worker is missing"

if [ "$KEEP" -eq 1 ]; then
    say "keeping the no-sudo rig ($nosudo_name)"
else
    say "stopping the no-sudo rig"
    bash "$RIG" down --name "$nosudo_name" || fail "down failed for the no-sudo rig"
    assert_rig_gone "no-sudo rig" "$nosudo_name" "$nosudo_home" "$nosudo_phone" "$nosudo_network" "$nosudo_state"
    created=("${created[@]/$nosudo_name/}")
fi

if [ "$KEEP" -eq 0 ]; then
    keep_name="$PREFIX-keep"
    created+=("$keep_name")
    say "checking down --keep-state and key reuse"
    bash "$RIG" up --name "$keep_name" >/dev/null || fail "keep-state rig did not start"
    eval "$(bash "$RIG" env --name "$keep_name")" || fail "rig.sh env failed for the keep-state rig"
    keep_home=$RIG_HOME_CONTAINER
    keep_phone=$RIG_PHONE_CONTAINER
    keep_network=$RIG_NETWORK
    keep_state=$RIG_STATE_DIR
    key_before=$(file_sha256 "$RIG_OWNER_KEY")
    bash "$RIG" ssh --name "$keep_name" 'echo before-keep' >/dev/null || fail "keep-state rig SSH failed"
    bash "$RIG" down --name "$keep_name" --keep-state || fail "down --keep-state failed"
    assert_rig_containers_gone "keep-state rig" "$keep_name" "$keep_home" "$keep_phone" "$keep_network"
    [ -e "$keep_state" ] || fail "down --keep-state removed the state directory"
    bash "$RIG" up --name "$keep_name" >/dev/null || fail "up after down --keep-state failed"
    key_after=$(file_sha256 "$RIG_OWNER_KEY")
    [ "$key_before" = "$key_after" ] || fail "up after --keep-state generated a new owner key"
    bash "$RIG" ssh --name "$keep_name" 'echo after-keep' >/dev/null || fail "reused keep-state rig SSH failed"
    bash "$RIG" down --name "$keep_name" || fail "final down failed for the keep-state rig"
    assert_rig_gone "keep-state rig" "$keep_name" "$keep_home" "$keep_phone" "$keep_network" "$keep_state"
    created=("${created[@]/$keep_name/}")
fi

say "PASS in $(( $(date +%s) - STARTED_AT ))s"
