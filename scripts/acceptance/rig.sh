#!/usr/bin/env bash
# scripts/acceptance/rig.sh — reusable Docker acceptance rig.
#
# Creates a disposable "fresh VPS" container with sshd (with sudo, or a
# no-sudo variant) and a second container that is only an SSH client, plus
# fixture helpers and a v0.1 home generator. Every scenario runs with a
# temporary VOWP_HOME and a private tmux socket inside the containers; the host
# home, ~/.vowp and tmux server are never touched.
#
# See scripts/acceptance/README.md for the full guide.
#
# Usage: bash scripts/acceptance/rig.sh <command> [options]
#
# Commands:
#   build  [--variant sudo|no-sudo|all] [--no-cache]
#   up     [--name NAME] [--variant sudo|no-sudo] [--memory SIZE] [--no-build]
#   down   [--name NAME] [--keep-state]
#   status [--name NAME]
#   env    [--name NAME]
#   ssh    [--name NAME] [--as owner|phone] [--] <command...>
#   phone  [--name NAME] [--] <command...>
#   exec   [--name NAME] [--] <command...>
#   logs   [--name NAME] [--tail N]
#   fixture      [--name NAME] <hello|memory-hog> [<dest>] [fixture options]
#   fixture-pair [--name NAME] [<dir>]
#   v01-home     [--name NAME] [make-v01-home options]
#   selftest     [test options]
#   list
#
# Examples:
#   bash scripts/acceptance/rig.sh build
#   bash scripts/acceptance/rig.sh up --name demo
#   eval "$(bash scripts/acceptance/rig.sh env --name demo)"
#   bash scripts/acceptance/rig.sh ssh 'uname -a'
#   bash scripts/acceptance/rig.sh phone 'echo phone ok'   # runs on the home
#   bash scripts/acceptance/rig.sh down --name demo
#
# Requirements: docker with a reachable daemon, bash, ssh, ssh-keygen. The
# v0.1 generator also needs curl/wget and python3, but those are in the image.

set -Eeuo pipefail

# A caller inside tmux exports these. Never let them reach a host tmux call.
unset TMUX TMUX_PANE 2>/dev/null || true

HERE=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
STATE_ROOT=${VOWP_ACCEPTANCE_STATE_DIR:-${TMPDIR:-/tmp}/vowp-acceptance}
VPS_IMAGE_BASE=${VOWP_ACCEPTANCE_VPS_IMAGE:-vowp-acceptance/vps}
PHONE_IMAGE=${VOWP_ACCEPTANCE_PHONE_IMAGE:-vowp-acceptance/phone:local}
IN_CONTAINER_SCRIPTS=/opt/vowp-acceptance
DEFAULT_NAME=default

say() {
    printf 'acceptance-rig: %s\n' "$*"
}

warn() {
    printf 'acceptance-rig: warning: %s\n' "$*" >&2
}

die() {
    printf 'acceptance-rig: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
}

require_docker() {
    command -v docker >/dev/null 2>&1 || die "docker is required but was not found on PATH"
    docker version >/dev/null 2>&1 || die "the docker daemon is not reachable"
}

validate_name() {
    case "$1" in
        ''|.|..|.*)
            die "invalid rig name: '$1' (must not be empty, '.', '..' or start with a dot)"
            ;;
        *[!A-Za-z0-9_.-]*)
            die "invalid rig name: '$1' (use letters, digits, dot, underscore or hyphen)"
            ;;
    esac
    [ "${#1}" -le 48 ] || die "rig name is too long (max 48 characters)"
}

validate_variant() {
    case "$1" in
        sudo|no-sudo) ;;
        *) die "variant must be sudo or no-sudo, not '$1'" ;;
    esac
}

name=${RIG_NAME:-$DEFAULT_NAME}
state_dir() {
    printf '%s/%s\n' "${STATE_ROOT%/}" "$name"
}

state_file() {
    printf '%s/state.env\n' "$(state_dir)"
}

load_state() {
    local file
    file=$(state_file)
    [ -f "$file" ] || die "no rig named '$name' in $STATE_ROOT (run 'up' first)"
    # shellcheck disable=SC1090
    . "$file"
}

container_running() {
    [ "$(docker inspect -f '{{.State.Running}}' "$1" 2>/dev/null || true)" = "true" ]
}

# --- build -----------------------------------------------------------------

cmd_build() {
    local variant=all
    local no_cache=0
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --variant)
                [ "$#" -ge 2 ] || die "--variant needs a value"
                variant=$2
                shift 2
                ;;
            --no-cache) no_cache=1; shift ;;
            -h|--help) usage ;;
            *) die "unknown build option: $1" ;;
        esac
    done
    case "$variant" in
        sudo|no-sudo|all) ;;
        *) die "variant must be sudo, no-sudo or all, not '$variant'" ;;
    esac
    require_docker
    local extra=()
    [ "$no_cache" -eq 1 ] && extra+=(--no-cache)

    if [ "$variant" = "sudo" ] || [ "$variant" = "all" ]; then
        say "building $VPS_IMAGE_BASE:sudo"
        docker build ${extra[@]+"${extra[@]}"} \
            --build-arg WITH_SUDO=1 \
            -t "$VPS_IMAGE_BASE:sudo" \
            "$HERE/container"
    fi
    if [ "$variant" = "no-sudo" ] || [ "$variant" = "all" ]; then
        say "building $VPS_IMAGE_BASE:no-sudo"
        docker build ${extra[@]+"${extra[@]}"} \
            --build-arg WITH_SUDO=0 \
            -t "$VPS_IMAGE_BASE:no-sudo" \
            "$HERE/container"
    fi
    say "building $PHONE_IMAGE"
    docker build ${extra[@]+"${extra[@]}"} \
        -f "$HERE/container/phone/Dockerfile" \
        -t "$PHONE_IMAGE" \
        "$HERE/container"
    say "images ready"
}

ensure_images() {
    local variant=$1
    if ! docker image inspect "$VPS_IMAGE_BASE:$variant" >/dev/null 2>&1; then
        say "image $VPS_IMAGE_BASE:$variant is missing; building it"
        if [ "$variant" = "sudo" ]; then
            docker build --build-arg WITH_SUDO=1 -t "$VPS_IMAGE_BASE:sudo" "$HERE/container"
        else
            docker build --build-arg WITH_SUDO=0 -t "$VPS_IMAGE_BASE:no-sudo" "$HERE/container"
        fi
    fi
    if ! docker image inspect "$PHONE_IMAGE" >/dev/null 2>&1; then
        say "image $PHONE_IMAGE is missing; building it"
        docker build -f "$HERE/container/phone/Dockerfile" -t "$PHONE_IMAGE" "$HERE/container"
    fi
}

# --- up --------------------------------------------------------------------

wait_for_home() {
    local container=$1
    local key=$2
    local port=$3
    local attempt
    for ((attempt = 1; attempt <= 60; attempt++)); do
        if ssh -p "$port" -i "$key" \
            -o BatchMode=yes -o ConnectTimeout=2 \
            -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
            -o GlobalKnownHostsFile=/dev/null -o LogLevel=ERROR \
            vowp@127.0.0.1 true >/dev/null 2>&1; then
            return 0
        fi
        if ! container_running "$container"; then
            die "home container stopped while waiting for sshd"
        fi
        sleep 1
    done
    return 1
}

wait_for_phone() {
    local phone_container=$1
    local home_container=$2
    local attempt
    for ((attempt = 1; attempt <= 30; attempt++)); do
        if docker exec -u phone -e HOME=/home/phone "$phone_container" \
            ssh -o BatchMode=yes -o ConnectTimeout=2 home true >/dev/null 2>&1; then
            return 0
        fi
        if ! container_running "$phone_container" || ! container_running "$home_container"; then
            die "a container stopped while waiting for the phone client"
        fi
        sleep 1
    done
    return 1
}

start_phone_container() {
    say "starting phone client $RIG_PHONE_CONTAINER"
    docker run -d \
        --name "$RIG_PHONE_CONTAINER" \
        --hostname phone \
        --label "vowp.acceptance.name=$RIG_NAME" \
        --label "vowp.acceptance.role=phone" \
        --network "$RIG_NETWORK" \
        --env "HOME=/home/phone" \
        --env "TMUX_TMPDIR=$RIG_PHONE_TMUX_TMPDIR" \
        --env "VOWP_ACCEPTANCE_TMUX_SOCKET=$RIG_TMUX_SOCKET" \
        --volume "$RIG_STATE_DIR/phone:/run/vowp-rig/phone:ro" \
        "$PHONE_IMAGE" >/dev/null
}

write_state() {
    local file
    file=$(state_file)
    {
        printf 'RIG_NAME=%q\n' "$RIG_NAME"
        printf 'RIG_STATE_DIR=%q\n' "$RIG_STATE_DIR"
        printf 'RIG_VARIANT=%q\n' "$RIG_VARIANT"
        printf 'RIG_HOME_CONTAINER=%q\n' "$RIG_HOME_CONTAINER"
        printf 'RIG_PHONE_CONTAINER=%q\n' "$RIG_PHONE_CONTAINER"
        printf 'RIG_NETWORK=%q\n' "$RIG_NETWORK"
        printf 'RIG_SSH_HOST=%q\n' "$RIG_SSH_HOST"
        printf 'RIG_SSH_PORT=%q\n' "$RIG_SSH_PORT"
        printf 'RIG_SSH_USER=%q\n' "$RIG_SSH_USER"
        printf 'RIG_OWNER_KEY=%q\n' "$RIG_OWNER_KEY"
        printf 'RIG_PHONE_KEY=%q\n' "$RIG_PHONE_KEY"
        printf 'RIG_USER_HOME=%q\n' "$RIG_USER_HOME"
        printf 'RIG_HOME=%q\n' "$RIG_HOME"
        printf 'RIG_TMUX_TMPDIR=%q\n' "$RIG_TMUX_TMPDIR"
        printf 'RIG_PHONE_TMUX_TMPDIR=%q\n' "$RIG_PHONE_TMUX_TMPDIR"
        printf 'RIG_TMUX_SOCKET=%q\n' "$RIG_TMUX_SOCKET"
        printf 'RIG_IN_CONTAINER_SCRIPTS=%q\n' "$RIG_IN_CONTAINER_SCRIPTS"
    } >"$file"
    chmod 0600 "$file"
}

cmd_up() {
    local variant=sudo
    local memory=
    local no_build=0
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --name)
                [ "$#" -ge 2 ] || die "--name needs a value"
                name=$2
                shift 2
                ;;
            --variant)
                [ "$#" -ge 2 ] || die "--variant needs a value"
                variant=$2
                shift 2
                ;;
            --memory)
                [ "$#" -ge 2 ] || die "--memory needs a value"
                memory=$2
                shift 2
                ;;
            --no-build) no_build=1; shift ;;
            -h|--help) usage ;;
            *) die "unknown up option: $1" ;;
        esac
    done
    validate_name "$name"
    validate_variant "$variant"
    require_docker

    if [ -e "$(state_file)" ]; then
        load_state
        if container_running "$RIG_HOME_CONTAINER"; then
            if container_running "$RIG_PHONE_CONTAINER"; then
                say "rig '$name' is already up"
                say "run: eval \"\$(bash $0 env --name $name)\""
                return 0
            fi
            RIG_PHONE_TMUX_TMPDIR=${RIG_PHONE_TMUX_TMPDIR:-/home/phone/.acceptance/tmux}
            if ! docker image inspect "$PHONE_IMAGE" >/dev/null 2>&1; then
                die "phone image $PHONE_IMAGE is missing; run 'bash $0 build' to repair the phone container"
            fi
            say "home container is running; repairing the stopped or missing phone container"
            docker rm -f "$RIG_PHONE_CONTAINER" >/dev/null 2>&1 || true
            if ! start_phone_container; then
                docker rm -f "$RIG_PHONE_CONTAINER" >/dev/null 2>&1 || true
                die "could not start the replacement phone container"
            fi
            if ! wait_for_phone "$RIG_PHONE_CONTAINER" "$RIG_HOME_CONTAINER"; then
                docker logs "$RIG_PHONE_CONTAINER" >&2 || true
                docker rm -f "$RIG_PHONE_CONTAINER" >/dev/null 2>&1 || true
                die "the repaired phone container could not reach the home container"
            fi
            write_state
            say "rig '$name' is up (phone container repaired)"
            say "phone: bash $0 phone --name $name"
            say "env:   eval \"\$(bash $0 env --name $name)\""
            return 0
        fi
        say "reusing kept state at $RIG_STATE_DIR"
        docker rm -f "$RIG_PHONE_CONTAINER" >/dev/null 2>&1 || true
        docker rm -f "$RIG_HOME_CONTAINER" >/dev/null 2>&1 || true
        docker network rm "$RIG_NETWORK" >/dev/null 2>&1 || true
    fi

    if [ "$no_build" -eq 0 ]; then
        ensure_images "$variant"
    else
        docker image inspect "$VPS_IMAGE_BASE:$variant" >/dev/null 2>&1 \
            || die "image $VPS_IMAGE_BASE:$variant is missing (remove --no-build)"
        docker image inspect "$PHONE_IMAGE" >/dev/null 2>&1 \
            || die "image $PHONE_IMAGE is missing (remove --no-build)"
    fi

    RIG_NAME=$name
    RIG_STATE_DIR=$(state_dir)
    RIG_VARIANT=$variant
    RIG_HOME_CONTAINER="vowp-acceptance-$name-home"
    RIG_PHONE_CONTAINER="vowp-acceptance-$name-phone"
    RIG_NETWORK="vowp-acceptance-$name"
    RIG_SSH_HOST=127.0.0.1
    RIG_SSH_USER=vowp
    RIG_USER_HOME=/home/vowp
    RIG_HOME=/home/vowp/.vowp
    RIG_TMUX_TMPDIR=/home/vowp/.acceptance/tmux
    RIG_PHONE_TMUX_TMPDIR=/home/phone/.acceptance/tmux
    RIG_TMUX_SOCKET="vowp-acceptance-$name"
    RIG_IN_CONTAINER_SCRIPTS=$IN_CONTAINER_SCRIPTS

    mkdir -p "$RIG_STATE_DIR/keys" "$RIG_STATE_DIR/phone"
    chmod 0700 "$RIG_STATE_DIR" "$RIG_STATE_DIR/keys" "$RIG_STATE_DIR/phone"

    if [ ! -f "$RIG_STATE_DIR/keys/owner" ]; then
        ssh-keygen -q -t ed25519 -N '' -C "vowp-acceptance-$name-owner" \
            -f "$RIG_STATE_DIR/keys/owner"
    fi
    if [ ! -f "$RIG_STATE_DIR/keys/phone" ]; then
        ssh-keygen -q -t ed25519 -N '' -C "vowp-acceptance-$name-phone" \
            -f "$RIG_STATE_DIR/keys/phone"
    fi
    RIG_OWNER_KEY="$RIG_STATE_DIR/keys/owner"
    RIG_PHONE_KEY="$RIG_STATE_DIR/keys/phone"
    cat "$RIG_OWNER_KEY.pub" "$RIG_PHONE_KEY.pub" >"$RIG_STATE_DIR/authorized_keys"
    chmod 0600 "$RIG_STATE_DIR/authorized_keys"

    install -m 0600 "$RIG_PHONE_KEY" "$RIG_STATE_DIR/phone/id_ed25519"
    cat >"$RIG_STATE_DIR/phone/config" <<EOF
# Generated by scripts/acceptance/rig.sh. Disposable phone client.
Host home
    HostName home
    User vowp
    Port 22
    IdentityFile ~/.ssh/id_ed25519
    IdentitiesOnly yes
    StrictHostKeyChecking no
    UserKnownHostsFile ~/.ssh/known_hosts
    GlobalKnownHostsFile /dev/null
    LogLevel ERROR
    ServerAliveInterval 30
    ServerAliveCountMax 3
EOF
    chmod 0600 "$RIG_STATE_DIR/phone/config"

    if ! docker network inspect "$RIG_NETWORK" >/dev/null 2>&1; then
        docker network create --label "vowp.acceptance.name=$name" "$RIG_NETWORK" >/dev/null
    fi

    local memory_args=()
    if [ -n "$memory" ]; then
        memory_args=(--memory "$memory")
    fi

    say "starting home container $RIG_HOME_CONTAINER ($variant)"
    docker run -d \
        --name "$RIG_HOME_CONTAINER" \
        --hostname vps \
        --label "vowp.acceptance.name=$name" \
        --label "vowp.acceptance.role=home" \
        --network "$RIG_NETWORK" \
        --network-alias home \
        -p 127.0.0.1::22 \
        --env "VOWP_ACCEPTANCE_TMUX_SOCKET=$RIG_TMUX_SOCKET" \
        --env "VOWP_TMUX_SOCKET=$RIG_TMUX_SOCKET" \
        --env "VOWP_ACCEPTANCE_VARIANT=$variant" \
        --env "TMUX_TMPDIR=$RIG_TMUX_TMPDIR" \
        --volume "$RIG_STATE_DIR/authorized_keys:/run/vowp-rig/authorized_keys:ro" \
        --volume "$HERE:$IN_CONTAINER_SCRIPTS:ro" \
        ${memory_args[@]+"${memory_args[@]}"} \
        "$VPS_IMAGE_BASE:$variant" >/dev/null || {
        docker rm -f "$RIG_HOME_CONTAINER" >/dev/null 2>&1 || true
        docker network rm "$RIG_NETWORK" >/dev/null 2>&1 || true
        die "could not start the home container"
    }

    local mapping
    mapping=$(docker port "$RIG_HOME_CONTAINER" 22/tcp 2>/dev/null | head -n 1)
    RIG_SSH_PORT=${mapping##*:}
    case "$RIG_SSH_PORT" in
        ''|*[!0-9]*)
            docker logs "$RIG_HOME_CONTAINER" >&2 || true
            docker rm -f "$RIG_HOME_CONTAINER" >/dev/null 2>&1 || true
            docker network rm "$RIG_NETWORK" >/dev/null 2>&1 || true
            die "could not read the published SSH port"
            ;;
    esac

    if ! wait_for_home "$RIG_HOME_CONTAINER" "$RIG_OWNER_KEY" "$RIG_SSH_PORT"; then
        docker logs "$RIG_HOME_CONTAINER" >&2 || true
        docker rm -f "$RIG_HOME_CONTAINER" >/dev/null 2>&1 || true
        docker network rm "$RIG_NETWORK" >/dev/null 2>&1 || true
        die "sshd did not become ready in $RIG_HOME_CONTAINER"
    fi

    if ! start_phone_container; then
        docker rm -f "$RIG_PHONE_CONTAINER" >/dev/null 2>&1 || true
        docker rm -f "$RIG_HOME_CONTAINER" >/dev/null 2>&1 || true
        docker network rm "$RIG_NETWORK" >/dev/null 2>&1 || true
        die "could not start the phone container"
    fi

    if ! wait_for_phone "$RIG_PHONE_CONTAINER" "$RIG_HOME_CONTAINER"; then
        docker logs "$RIG_PHONE_CONTAINER" >&2 || true
        docker rm -f "$RIG_PHONE_CONTAINER" >/dev/null 2>&1 || true
        docker rm -f "$RIG_HOME_CONTAINER" >/dev/null 2>&1 || true
        docker network rm "$RIG_NETWORK" >/dev/null 2>&1 || true
        die "the phone client could not reach the home container"
    fi

    write_state
    say "rig '$name' is up"
    say "home:  ssh -p $RIG_SSH_PORT -i $RIG_OWNER_KEY vowp@127.0.0.1"
    say "phone: bash $0 phone --name $name"
    say "env:   eval \"\$(bash $0 env --name $name)\""
}

# --- down ------------------------------------------------------------------

cmd_down() {
    local keep_state=0
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --name)
                [ "$#" -ge 2 ] || die "--name needs a value"
                name=$2
                shift 2
                ;;
            --keep-state) keep_state=1; shift ;;
            -h|--help) usage ;;
            *) die "unknown down option: $1" ;;
        esac
    done
    validate_name "$name"
    require_docker

    local home="vowp-acceptance-$name-home"
    local phone="vowp-acceptance-$name-phone"
    local network="vowp-acceptance-$name"
    local dir
    dir=$(state_dir)
    if [ -f "$dir/state.env" ]; then
        load_state
        home=$RIG_HOME_CONTAINER
        phone=$RIG_PHONE_CONTAINER
        network=$RIG_NETWORK
    fi

    local failures=0
    local container
    say "removing rig '$name'"

    # An absent container/network is already down. A present one that cannot
    # be removed is a real failure: reporting success would leave a rig behind
    # and make the self-test's teardown check meaningless.
    for container in "$phone" "$home"; do
        if docker inspect "$container" >/dev/null 2>&1; then
            if ! docker rm -f "$container" >/dev/null 2>&1; then
                warn "could not remove container $container"
                failures=$((failures + 1))
            fi
        fi
    done
    if docker network inspect "$network" >/dev/null 2>&1; then
        if ! docker network rm "$network" >/dev/null 2>&1; then
            warn "could not remove network $network"
            failures=$((failures + 1))
        fi
    fi

    if [ "$keep_state" -eq 1 ]; then
        say "kept state at $dir"
    elif [ -e "$dir" ]; then
        if ! rm -rf -- "$dir"; then
            warn "could not remove state at $dir"
            failures=$((failures + 1))
        else
            say "removed state at $dir"
        fi
    else
        say "no state at $dir"
    fi

    if [ "$failures" -ne 0 ]; then
        die "could not fully remove rig '$name' ($failures removal failure(s))"
    fi
}

# --- status / env / logs ----------------------------------------------------

cmd_status() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --name)
                [ "$#" -ge 2 ] || die "--name needs a value"
                name=$2
                shift 2
                ;;
            -h|--help) usage ;;
            *) die "unknown status option: $1" ;;
        esac
    done
    validate_name "$name"
    load_state
    printf 'name:      %s\n' "$RIG_NAME"
    printf 'variant:   %s\n' "$RIG_VARIANT"
    printf 'state:     %s\n' "$RIG_STATE_DIR"
    printf 'home:      %s\n' "$RIG_HOME_CONTAINER"
    printf 'phone:     %s\n' "$RIG_PHONE_CONTAINER"
    printf 'network:   %s\n' "$RIG_NETWORK"
    printf 'ssh:       %s@%s:%s\n' "$RIG_SSH_USER" "$RIG_SSH_HOST" "$RIG_SSH_PORT"
    printf 'tmux:      TMUX_TMPDIR=%s socket=%s\n' "$RIG_TMUX_TMPDIR" "$RIG_TMUX_SOCKET"
    if container_running "$RIG_HOME_CONTAINER"; then
        printf 'home:      running\n'
    else
        printf 'home:      stopped\n'
    fi
    if container_running "$RIG_PHONE_CONTAINER"; then
        printf 'phone:     running\n'
    else
        printf 'phone:     stopped\n'
    fi
}

cmd_env() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --name)
                [ "$#" -ge 2 ] || die "--name needs a value"
                name=$2
                shift 2
                ;;
            -h|--help) usage ;;
            *) die "unknown env option: $1" ;;
        esac
    done
    validate_name "$name"
    load_state
    printf 'export RIG_NAME=%q\n' "$RIG_NAME"
    printf 'export RIG_VARIANT=%q\n' "$RIG_VARIANT"
    printf 'export RIG_STATE_DIR=%q\n' "$RIG_STATE_DIR"
    printf 'export RIG_HOME_CONTAINER=%q\n' "$RIG_HOME_CONTAINER"
    printf 'export RIG_PHONE_CONTAINER=%q\n' "$RIG_PHONE_CONTAINER"
    printf 'export RIG_NETWORK=%q\n' "$RIG_NETWORK"
    printf 'export RIG_SSH_HOST=%q\n' "$RIG_SSH_HOST"
    printf 'export RIG_SSH_PORT=%q\n' "$RIG_SSH_PORT"
    printf 'export RIG_SSH_USER=%q\n' "$RIG_SSH_USER"
    printf 'export RIG_OWNER_KEY=%q\n' "$RIG_OWNER_KEY"
    printf 'export RIG_PHONE_KEY=%q\n' "$RIG_PHONE_KEY"
    printf 'export RIG_USER_HOME=%q\n' "$RIG_USER_HOME"
    printf 'export RIG_HOME=%q\n' "$RIG_HOME"
    printf 'export RIG_TMUX_TMPDIR=%q\n' "$RIG_TMUX_TMPDIR"
    printf 'export RIG_PHONE_TMUX_TMPDIR=%q\n' "$RIG_PHONE_TMUX_TMPDIR"
    printf 'export RIG_TMUX_SOCKET=%q\n' "$RIG_TMUX_SOCKET"
    printf 'export RIG_IN_CONTAINER_SCRIPTS=%q\n' "$RIG_IN_CONTAINER_SCRIPTS"
}

cmd_logs() {
    local tail=80
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --name)
                [ "$#" -ge 2 ] || die "--name needs a value"
                name=$2
                shift 2
                ;;
            --tail)
                [ "$#" -ge 2 ] || die "--tail needs a value"
                tail=$2
                shift 2
                ;;
            -h|--help) usage ;;
            *) die "unknown logs option: $1" ;;
        esac
    done
    validate_name "$name"
    load_state
    printf '%s\n' "--- home ($RIG_HOME_CONTAINER) ---"
    docker logs --tail "$tail" "$RIG_HOME_CONTAINER" 2>&1 || true
    printf '%s\n' "--- phone ($RIG_PHONE_CONTAINER) ---"
    docker logs --tail "$tail" "$RIG_PHONE_CONTAINER" 2>&1 || true
}

# --- SSH and exec ----------------------------------------------------------

ssh_args() {
    printf '%s\0' \
        -p "$RIG_SSH_PORT" \
        -i "$1" \
        -o IdentitiesOnly=yes \
        -o BatchMode=yes \
        -o StrictHostKeyChecking=no \
        -o UserKnownHostsFile=/dev/null \
        -o GlobalKnownHostsFile=/dev/null \
        -o LogLevel=ERROR \
        -o ConnectTimeout=10
}

cmd_ssh() {
    local as=owner
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --name)
                [ "$#" -ge 2 ] || die "--name needs a value"
                name=$2
                shift 2
                ;;
            --as)
                [ "$#" -ge 2 ] || die "--as needs owner or phone"
                as=$2
                shift 2
                ;;
            --) shift; break ;;
            -h|--help) usage ;;
            *) break ;;
        esac
    done
    validate_name "$name"
    case "$as" in
        owner|phone) ;;
        *) die "--as must be owner or phone" ;;
    esac
    load_state
    local key=$RIG_OWNER_KEY
    [ "$as" = "phone" ] && key=$RIG_PHONE_KEY
    local args=()
    while IFS= read -r -d '' item; do
        args+=("$item")
    done < <(ssh_args "$key")
    if [ "$#" -eq 0 ]; then
        args+=(-t)
        exec ssh "${args[@]}" "$RIG_SSH_USER@$RIG_SSH_HOST"
    fi
    exec ssh "${args[@]}" "$RIG_SSH_USER@$RIG_SSH_HOST" "$@"
}

cmd_phone() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --name)
                [ "$#" -ge 2 ] || die "--name needs a value"
                name=$2
                shift 2
                ;;
            --) shift; break ;;
            -h|--help) usage ;;
            *) break ;;
        esac
    done
    validate_name "$name"
    load_state
    local tty=()
    if [ -t 0 ] && [ -t 1 ]; then
        tty=(-it)
    fi
    local env_args=(
        -e "HOME=/home/phone"
        -e "TMUX_TMPDIR=${RIG_PHONE_TMUX_TMPDIR:-/home/phone/.acceptance/tmux}"
        -e "VOWP_ACCEPTANCE_TMUX_SOCKET=$RIG_TMUX_SOCKET"
    )
    if [ "$#" -eq 0 ]; then
        exec docker exec ${tty[@]+"${tty[@]}"} -u phone "${env_args[@]}" "$RIG_PHONE_CONTAINER" ssh home
    fi
    exec docker exec ${tty[@]+"${tty[@]}"} -u phone "${env_args[@]}" "$RIG_PHONE_CONTAINER" ssh home "$@"
}

cmd_exec() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --name)
                [ "$#" -ge 2 ] || die "--name needs a value"
                name=$2
                shift 2
                ;;
            --) shift; break ;;
            -h|--help) usage ;;
            *) break ;;
        esac
    done
    validate_name "$name"
    load_state
    local tty=()
    if [ -t 0 ] && [ -t 1 ]; then
        tty=(-it)
    fi
    local env_args=(
        -e "HOME=$RIG_USER_HOME"
        -e "VOWP_HOME=$RIG_HOME"
        -e "TMUX_TMPDIR=$RIG_TMUX_TMPDIR"
        -e "VOWP_ACCEPTANCE_TMUX_SOCKET=$RIG_TMUX_SOCKET"
        -e "VOWP_TMUX_SOCKET=$RIG_TMUX_SOCKET"
    )
    if [ "$#" -eq 0 ]; then
        exec docker exec ${tty[@]+"${tty[@]}"} -u vowp "${env_args[@]}" -w "$RIG_USER_HOME" \
            "$RIG_HOME_CONTAINER" bash -l
    fi
    if [ "$#" -eq 1 ]; then
        exec docker exec ${tty[@]+"${tty[@]}"} -u vowp "${env_args[@]}" -w "$RIG_USER_HOME" \
            "$RIG_HOME_CONTAINER" bash -lc "$1"
    fi
    exec docker exec ${tty[@]+"${tty[@]}"} -u vowp "${env_args[@]}" -w "$RIG_USER_HOME" \
        "$RIG_HOME_CONTAINER" "$@"
}

# --- fixtures --------------------------------------------------------------

container_env_args() {
    printf '%s\0' \
        -e "HOME=$RIG_USER_HOME" \
        -e "VOWP_HOME=$RIG_HOME" \
        -e "TMUX_TMPDIR=$RIG_TMUX_TMPDIR" \
        -e "VOWP_ACCEPTANCE_TMUX_SOCKET=$RIG_TMUX_SOCKET" \
        -e "VOWP_TMUX_SOCKET=$RIG_TMUX_SOCKET"
}

cmd_fixture() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --name)
                [ "$#" -ge 2 ] || die "--name needs a value"
                name=$2
                shift 2
                ;;
            --) shift; break ;;
            -h|--help) usage ;;
            *) break ;;
        esac
    done
    validate_name "$name"
    load_state
    [ "$#" -ge 1 ] || die "fixture needs a kind: hello or memory-hog"
    local kind=$1
    shift
    local dest=${1:-}
    if [ -n "$dest" ] && [ "${dest#-}" = "$dest" ]; then
        shift
    else
        dest="$RIG_USER_HOME/projects/$kind"
    fi
    local env_args=()
    while IFS= read -r -d '' item; do
        env_args+=("$item")
    done < <(container_env_args)
    docker exec -i -u vowp "${env_args[@]}" -w "$RIG_USER_HOME" \
        "$RIG_HOME_CONTAINER" bash "$IN_CONTAINER_SCRIPTS/fixtures/project.sh" "$kind" "$dest" "$@"
}

cmd_fixture_pair() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --name)
                [ "$#" -ge 2 ] || die "--name needs a value"
                name=$2
                shift 2
                ;;
            --) shift; break ;;
            -h|--help) usage ;;
            *) break ;;
        esac
    done
    validate_name "$name"
    load_state
    local dir=${1:-"$RIG_USER_HOME/projects"}
    shift || true
    local env_args=()
    while IFS= read -r -d '' item; do
        env_args+=("$item")
    done < <(container_env_args)
    docker exec -i -u vowp "${env_args[@]}" -w "$RIG_USER_HOME" \
        "$RIG_HOME_CONTAINER" bash "$IN_CONTAINER_SCRIPTS/fixtures/project.sh" hello "$dir/hello" "$@"
    docker exec -i -u vowp "${env_args[@]}" -w "$RIG_USER_HOME" \
        "$RIG_HOME_CONTAINER" bash "$IN_CONTAINER_SCRIPTS/fixtures/project.sh" memory-hog "$dir/memory-hog" "$@"
}

cmd_v01_home() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --name)
                [ "$#" -ge 2 ] || die "--name needs a value"
                name=$2
                shift 2
                ;;
            --) shift; break ;;
            -h|--help) usage ;;
            *) break ;;
        esac
    done
    validate_name "$name"
    load_state
    local has_dest=0
    for arg in "$@"; do
        [ "$arg" = "--dest" ] && has_dest=1
    done
    local prefix=()
    if [ "$has_dest" -eq 0 ]; then
        prefix=(--dest "$RIG_USER_HOME/acceptance/v01-home")
    fi
    local env_args=()
    while IFS= read -r -d '' item; do
        env_args+=("$item")
    done < <(container_env_args)
    # docker exec does not inherit the caller's environment, so forward the
    # documented generator overrides explicitly.
    if [ -n "${VOWP_V010_BASE_URL:-}" ]; then
        env_args+=(-e "VOWP_V010_BASE_URL=$VOWP_V010_BASE_URL")
    fi
    if [ -n "${VOWP_V010_TARGET:-}" ]; then
        env_args+=(-e "VOWP_V010_TARGET=$VOWP_V010_TARGET")
    fi
    docker exec -i -u vowp "${env_args[@]}" \
        -e "VOWP_V010_CACHE=$RIG_USER_HOME/.cache/vowp-acceptance" \
        -w "$RIG_USER_HOME" \
        "$RIG_HOME_CONTAINER" bash "$IN_CONTAINER_SCRIPTS/make-v01-home.sh" ${prefix[@]+"${prefix[@]}"} "$@"
}

# --- misc ------------------------------------------------------------------

cmd_list() {
    require_docker
    docker ps -a \
        --filter label=vowp.acceptance.role=home \
        --format '{{.Names}}\t{{.Status}}\t{{.Label "vowp.acceptance.name"}}'
}

cmd_selftest() {
    exec bash "$HERE/tests/run.sh" "$@"
}

# --- dispatch --------------------------------------------------------------

[ "$#" -ge 1 ] || usage
command=$1
shift
case "$command" in
    build) cmd_build "$@" ;;
    up) cmd_up "$@" ;;
    down) cmd_down "$@" ;;
    status) cmd_status "$@" ;;
    env) cmd_env "$@" ;;
    ssh) cmd_ssh "$@" ;;
    phone) cmd_phone "$@" ;;
    exec) cmd_exec "$@" ;;
    logs) cmd_logs "$@" ;;
    fixture) cmd_fixture "$@" ;;
    fixture-pair) cmd_fixture_pair "$@" ;;
    v01-home) cmd_v01_home "$@" ;;
    list) cmd_list "$@" ;;
    selftest) cmd_selftest "$@" ;;
    -h|--help|help) usage ;;
    *) die "unknown command: $command (run '$0 help')" ;;
esac
