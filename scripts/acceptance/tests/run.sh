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
    real_first=$(acceptance_find_real_tmux)
    case "$real_first" in
        "$host_root"/*) exit 1 ;;
    esac
    acceptance_env_init "$host_root/runtime" "selftest-host-$$"
    [ -d "$TMUX_TMPDIR" ] || exit 1
    real_second=$(acceptance_find_real_tmux)
    [ "$real_first" = "$real_second" ] || exit 1
    acceptance_env_init "$host_root/runtime" "selftest-host-$$"
    real_third=$(acceptance_find_real_tmux)
    [ "$real_first" = "$real_third" ] || exit 1
    # Even without the cached path, the generated wrapper on PATH is skipped.
    unset VOWP_ACCEPTANCE_REAL_TMUX
    real_scanned=$(acceptance_find_real_tmux)
    [ "$real_first" = "$real_scanned" ] || exit 1
    grep -Fq "exec \"$real_first\"" "$host_root/runtime/bin/tmux" || exit 1
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

say "checking the destination allowlist and the generator's exit codes"
dest_root=$(mktemp -d "${TMPDIR:-/tmp}/vowp-acceptance-dest.XXXXXX")
mkdir -p "$dest_root/fake-home"
ln -s /etc "$dest_root/etc-link"
# The historical bypass: the validator truncated this path at the newline and
# checked only /tmp/.../safe, while rm -rf would follow the symlinked parent.
# $'...' preserves the trailing newline; $(printf 'safe\n') would strip it and
# silently turn this into an ordinary symlinked-parent case.
newline_leaf=$'safe\n'
mkdir -p "$dest_root/$newline_leaf"
ln -s /etc "$dest_root/$newline_leaf/link"
newline_symlink_dest="$dest_root/$newline_leaf/link/verify-target"
case "$newline_symlink_dest" in
    *$'\n'*) ;;
    *) fail "the newline destination fixture lost its newline" ;;
esac
real_home=${HOME:-}
(
    # Use a dedicated home so the good-path assertion also works when the
    # self-test itself runs as root (the validator refuses /root).
    HOME="$dest_root/fake-home"
    export HOME
    # shellcheck source=../lib.sh
    . "$ACCEPTANCE_DIR/lib.sh"
    newline_dest=$(printf '/tmp/vowp-acceptance\nlock')
    # The control-character check rejects newline_dest before normalisation, so
    # assert the normaliser itself keeps the newline. A reverted `read -a <<<`
    # would silently truncate here even though the validator still rejected it.
    [ "$(acceptance_normalize_path "$newline_dest")" = "$newline_dest" ] || {
        printf 'acceptance_normalize_path truncated at a newline\n' >&2
        exit 1
    }
    for bad in / /etc /etc/passwd /usr /usr/bin /bin /sbin /boot /dev /proc /sys \
        /var /var/lib /opt /root /root/keep /home /tmp /var/tmp \
        "$dest_root/etc-link" "$dest_root/etc-link/passwd" "$newline_dest" \
        "$newline_symlink_dest"; do
        if (acceptance_validate_dest "$bad") >/dev/null 2>&1; then
            printf 'accepted unsafe destination: %s\n' "$bad" >&2
            exit 1
        fi
    done
    # A system directory must not become an allowlist root through HOME.
    for bad_home in / /etc /usr /bin /sbin /boot /dev /proc /sys /var /opt /root /home; do
        if (HOME="$bad_home" acceptance_validate_dest "$bad_home/acceptance-target") >/dev/null 2>&1; then
            printf 'accepted a destination under an unsafe HOME root: %s\n' "$bad_home" >&2
            exit 1
        fi
    done
    if (HOME=/etc acceptance_validate_dest /etc/passwd) >/dev/null 2>&1; then
        printf 'accepted /etc/passwd with HOME=/etc\n' >&2
        exit 1
    fi
    # The real home itself is unsafe unless it sits under the /tmp allowlist.
    case "$real_home" in
        ""|/tmp/*|/var/tmp/*) ;;
        *)
            if (HOME="$real_home" acceptance_validate_dest "$real_home") >/dev/null 2>&1; then
                printf 'accepted the real home as a destination: %s\n' "$real_home" >&2
                exit 1
            fi
            ;;
    esac
    # A normal real home still authorizes its own children. System roots and
    # /root are deliberately excluded so the assertion stays host-independent.
    if [ -n "$real_home" ] && ! acceptance_unsafe_allowlist_root "$real_home"; then
        (HOME="$real_home" acceptance_validate_dest "$real_home/vowp-acceptance-dest") || exit 1
    fi
    for good in /tmp/vowp-acceptance-dest /var/tmp/vowp-acceptance-dest \
        "$HOME/vowp-acceptance-dest"; do
        (acceptance_validate_dest "$good") || exit 1
    done
    (cd /tmp && acceptance_validate_dest vowp-acceptance-dest) || exit 1
    if (acceptance_validate_dest /etc 2) >/dev/null 2>&1; then
        exit 1
    else
        [ "$?" -eq 2 ] || exit 1
    fi
) || fail "the destination allowlist did not behave"
rm -rf -- "$dest_root"

if bash "$ACCEPTANCE_DIR/make-v01-home.sh" --help >/dev/null 2>&1; then
    :
else
    fail "make-v01-home.sh --help did not exit 0"
fi
expect_bad_generator_args() {
    local label=$1
    local rc
    shift
    if bash "$ACCEPTANCE_DIR/make-v01-home.sh" "$@" >/dev/null 2>&1; then
        fail "make-v01-home.sh accepted bad arguments: $label"
    else
        rc=$?
    fi
    [ "$rc" -eq 2 ] || fail "make-v01-home.sh bad arguments exited $rc, not 2: $label"
}
expect_bad_generator_args "non-numeric jobs" --jobs nope --dest /tmp/unused
expect_bad_generator_args "missing dest value" --dest
expect_bad_generator_args "unknown option" --unknown
expect_bad_generator_args "empty binary" --dest /tmp/unused --binary ""
expect_bad_generator_args "empty version" --dest /tmp/unused --version ""
expect_bad_generator_args "zero timeout" --dest /tmp/unused --timeout 0

say "checking the generator turns missing tools into exit 1"
tool_root=$(mktemp -d "${TMPDIR:-/tmp}/vowp-acceptance-tools.XXXXXX")
expect_missing_generator_tool() {
    local tool=$1
    local dir="$tool_root/$tool"
    local present rc
    mkdir -p "$dir"
    ln -s "$(command -v date)" "$dir/date"
    ln -s "$(command -v dirname)" "$dir/dirname"
    # Stub the other required tools: the precheck only asks whether they exist,
    # so the test does not need git/tar/python3 on the self-test host.
    for present in git tar python3; do
        [ "$present" = "$tool" ] && continue
        printf '#!/bin/sh\nexit 0\n' >"$dir/$present"
        chmod 0755 "$dir/$present"
    done
    if PATH="$dir" /bin/bash "$ACCEPTANCE_DIR/make-v01-home.sh" \
        --dest "$tool_root/missing-$tool" >/dev/null 2>&1; then
        fail "make-v01-home.sh ran without $tool"
    else
        rc=$?
    fi
    [ "$rc" -eq 1 ] || fail "make-v01-home.sh without $tool exited $rc, not 1"
    [ ! -e "$tool_root/missing-$tool" ] || fail "make-v01-home.sh wrote a destination without $tool"
}
expect_missing_generator_tool git
expect_missing_generator_tool tar
rm -rf -- "$tool_root"

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

say "checking the fresh home has no ~/.vowp before setup"
bash "$RIG" ssh --name "$sudo_name" 'test ! -e "$HOME/.vowp"' \
    || fail "the fresh home already has ~/.vowp before setup"

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

say "checking the phone container's private tmux wrapper"
phone_env=(
    -e "HOME=/home/phone"
    -e "TMUX_TMPDIR=$RIG_PHONE_TMUX_TMPDIR"
    -e "VOWP_ACCEPTANCE_TMUX_SOCKET=$RIG_TMUX_SOCKET"
)
docker exec -u phone "${phone_env[@]}" "$sudo_phone" \
    sh -c 'test -d "$TMUX_TMPDIR" && test -n "$VOWP_ACCEPTANCE_TMUX_SOCKET"' \
    || fail "the phone container has no private tmux environment"
out=$(docker exec -u phone "${phone_env[@]}" "$sudo_phone" \
    sh -c 'socket=$(printf "%s" default); tmux -L "$socket" ls' 2>&1 || true)
assert_contains "$out" "refusing the default tmux socket" "the phone tmux wrapper did not refuse the default socket"
out=$(docker exec -u phone "${phone_env[@]}" "$sudo_phone" \
    sh -c 'TMUX_TMPDIR= tmux ls' 2>&1 || true)
assert_contains "$out" "refusing an unset or shared TMUX_TMPDIR" "the phone tmux wrapper did not refuse an empty TMUX_TMPDIR"
out=$(docker exec -u phone "${phone_env[@]}" "$sudo_phone" \
    sh -c 'tmux -S /tmp/forbidden-tmux-socket ls' 2>&1 || true)
assert_contains "$out" "refusing an explicit -S socket path" "the phone tmux wrapper did not refuse an explicit -S"

say "checking up repairs a stopped or missing phone container"
home_id=$(docker inspect -f '{{.Id}}' "$sudo_home")
docker stop "$sudo_phone" >/dev/null
out=$(bash "$RIG" up --name "$sudo_name") \
    || fail "up did not repair the stopped phone container"
assert_contains "$out" "repairing the stopped or missing phone container" \
    "up did not report the stopped phone repair"
[ "$(docker inspect -f '{{.Id}}' "$sudo_home")" = "$home_id" ] \
    || fail "up recreated the home container while repairing the phone"
[ "$(docker inspect -f '{{.State.Running}}' "$sudo_phone")" = "true" ] \
    || fail "up left the phone container stopped"
bash "$RIG" phone --name "$sudo_name" 'echo phone-repair-ok' >/dev/null \
    || fail "the repaired phone container cannot reach the home"
docker rm -f "$sudo_phone" >/dev/null
out=$(bash "$RIG" up --name "$sudo_name") \
    || fail "up did not recreate a missing phone container"
assert_contains "$out" "repairing the stopped or missing phone container" \
    "up did not report the missing phone repair"
[ "$(docker inspect -f '{{.Id}}' "$sudo_home")" = "$home_id" ] \
    || fail "up recreated the home container while recreating the phone"
bash "$RIG" phone --name "$sudo_name" 'echo phone-repair-ok' >/dev/null \
    || fail "the recreated phone container cannot reach the home"

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
say "checking ssh --as phone selects the phone key, not the owner key"
owner_comment="vowp-acceptance-$sudo_name-owner"
docker exec -u vowp "$sudo_home" sh -c '
set -e
cd /home/vowp/.ssh
cp authorized_keys authorized_keys.full
grep -v -- "$1" authorized_keys.full > authorized_keys.tmp
cat authorized_keys.tmp > authorized_keys
rm -f authorized_keys.tmp
test "$(wc -l < authorized_keys)" -eq 1
grep -q -- "-phone" authorized_keys
' sh "$owner_comment" || fail "could not isolate the phone key in authorized_keys"
if out=$(bash "$RIG" ssh --as phone --name "$sudo_name" 'echo phone-key-ok'); then
    phone_key_rc=0
else
    phone_key_rc=$?
    out=
fi
if bash "$RIG" ssh --as owner --name "$sudo_name" true >/dev/null 2>&1; then
    owner_key_rc=0
else
    owner_key_rc=$?
fi
docker exec -u vowp "$sudo_home" sh -c '
set -e
cd /home/vowp/.ssh
cat authorized_keys.full > authorized_keys
rm -f authorized_keys.full
' || fail "could not restore the owner key after the phone-key check"
[ "$phone_key_rc" -eq 0 ] || fail "ssh --as phone failed with only the phone key authorized"
assert_contains "$out" "phone-key-ok" "ssh --as phone did not use the phone key"
[ "$owner_key_rc" -ne 0 ] || fail "ssh --as owner still succeeded with the owner key removed"
bash "$RIG" ssh --as owner --name "$sudo_name" 'echo owner-key-restored' >/dev/null \
    || fail "the owner key does not work after the phone-key check"

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
# Drain stdin: seed-jobs.sh pipes the brief under pipefail, so exiting early
# can SIGPIPE the writer and stop the loop after one job.
cat >/dev/null
EOS
chmod +x /home/vowp/fake-vowp
rm -f /home/vowp/seed-args
cd /home/vowp/projects/hello
VOWP=/home/vowp/fake-vowp .acceptance/seed-jobs.sh 4 fixture >/dev/null
test "$(wc -l < /home/vowp/seed-args)" -eq 4
grep -q -- "--id fixture-1" /home/vowp/seed-args
grep -q -- "--id fixture-4" /home/vowp/seed-args
' || fail "seed-jobs.sh did not add the expected jobs"

say "running the memory-hog worker once and checking its peak RSS"
out=$(bash "$RIG" ssh --name "$sudo_name" '
set -e
cd /home/vowp/projects/hog
log=/home/vowp/hog-worker.log
: >"$log"
printf "%s\n" "You are the worker for job hog-smoke of project hog. write your report to /home/vowp/projects/hog/.vowp/jobs/hog-smoke/report.md" >/home/vowp/hog-prompt
start=$(date +%s)
VOWP_ACCEPTANCE_WORKER_LOG="$log" .acceptance/bin/hog-worker </home/vowp/hog-prompt &
worker_pid=$!
hog_pid=
attempt=0
while [ "$attempt" -lt 200 ]; do
    hog_pid=$(sed -n "s/.* allocating [0-9]*MiB for [0-9]*s pid=\([0-9][0-9]*\).*/\1/p" "$log" 2>/dev/null | tail -n 1)
    [ -n "$hog_pid" ] && break
    kill -0 "$worker_pid" 2>/dev/null || break
    sleep 0.1
    attempt=$((attempt + 1))
done
test -n "$hog_pid" || { echo "hog pid missing" >&2; exit 1; }
vmhwm=0
attempt=0
while [ "$attempt" -lt 200 ]; do
    if [ -r "/proc/$hog_pid/status" ]; then
        vmhwm=$(grep "^VmHWM:" "/proc/$hog_pid/status" | tr -s "[:space:]" " " | cut -d" " -f2)
    fi
    [ "${vmhwm:-0}" -ge 24576 ] && break
    kill -0 "$hog_pid" 2>/dev/null || break
    sleep 0.1
    attempt=$((attempt + 1))
done
wait "$worker_pid"
end=$(date +%s)
test $((end - start)) -ge 2
test -f src/hog-smoke.txt
test -f .vowp/jobs/hog-smoke/report.md
grep -q "allocating 32MiB" "$log"
test "${vmhwm:-0}" -ge 24576
echo "hog-worker-ok elapsed=$((end-start)) vmhwm_kb=$vmhwm"
')
assert_contains "$out" "hog-worker-ok" "the memory-hog worker did not finish"
assert_contains "$out" "vmhwm_kb=" "the memory-hog worker did not report a peak RSS"

say "checking rig.sh v01-home forwards the release overrides"
env_check=$(
    VOWP_V010_BASE_URL=http://127.0.0.1:9 \
        VOWP_V010_TARGET=acceptance-env-check \
        bash "$RIG" v01-home --name "$sudo_name" --dest /home/vowp/acceptance/env-check 2>&1 || true
)
assert_contains "$env_check" "http://127.0.0.1:9/v0.1.0/" \
    "rig.sh v01-home did not forward VOWP_V010_BASE_URL"
assert_contains "$env_check" "acceptance-env-check" \
    "rig.sh v01-home did not forward VOWP_V010_TARGET"
bash "$RIG" ssh --name "$sudo_name" 'rm -rf /home/vowp/acceptance/env-check' \
    || fail "could not remove the v01-home forwarding probe"

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
bash "$RIG" ssh --name "$nosudo_name" 'test ! -e "$HOME/.vowp"' \
    || fail "the fresh no-sudo home already has ~/.vowp before setup"

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
