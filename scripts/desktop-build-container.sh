#!/usr/bin/env bash
# Build and run the pinned Linux desktop build container.
#
# This is the reproducible way to compile the Tauri host (and run the desktop UI
# gates) on a Linux worker that has Docker but no GTK/WebKit development packages.
# It never installs host packages, never uses sudo, and never writes into the
# worktree as root: the container runs as the host UID/GID with HOME=/tmp and keeps
# Cargo, target, and npm caches outside the repository.
#
# See docs/DEV-SETUP.md for the rationale, expected output, and what this does not
# prove (signed macOS bundle, real OS-keychain round trip, browser e2e).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOCKERFILE="$ROOT/docker/desktop-build.Dockerfile"
IMAGE="${STACKSPIRIT_DESKTOP_BUILD_IMAGE:-stackspirit-desktop-build:local}"
CACHE_HOME="${XDG_CACHE_HOME:-${HOME:-/tmp}/.cache}"
CACHE_ROOT="$CACHE_HOME/stackspirit/desktop-build"
RUST_TOOLCHAIN="${STACKSPIRIT_DESKTOP_RUST_TOOLCHAIN:-1.90.0}"

usage() {
  cat <<'EOF'
usage: scripts/desktop-build-container.sh [command]

Commands:
  check         (default) Build the pinned Linux image if it is missing, then run
                the desktop gates inside a container:
                  cargo fmt --all -- --check
                  node scripts/fetch-engine.mjs   (pinned sidecar, gitignored)
                  npm ci
                  npm run typecheck
                  npm run lint
                  npm test
                  npm run build
                  cargo check -p spirit-desktop
                  cargo test -p spirit-desktop
                The sidecar is fetched because tauri-build validates
                bundle.externalBin while compiling the host.
  tauri-build   Fetch the pinned StackSpirit Engine sidecar if it is missing, then
                run `npx tauri build --debug --no-bundle` (host compile plus
                frontend bundle; no AppImage/deb packaging).
  shell         Open an interactive bash shell in the same container environment.
  build         Build or refresh the pinned image only.
  --help, -h    Show this help.

The repository is bind-mounted read-write at /work and runs as your UID/GID.
HOME is /tmp inside the container. Cargo, target, and npm caches live outside the
repository at:
  ${XDG_CACHE_HOME:-$HOME/.cache}/stackspirit/desktop-build/{cargo,target,npm}

Environment:
  STACKSPIRIT_DESKTOP_BUILD_IMAGE      image tag (default: stackspirit-desktop-build:local)
  STACKSPIRIT_DESKTOP_RUST_TOOLCHAIN   pinned toolchain (default: 1.90.0)
  XDG_CACHE_HOME                       base directory for the host caches

Examples:
  scripts/desktop-build-container.sh check
  scripts/desktop-build-container.sh tauri-build
  scripts/desktop-build-container.sh shell

Docker is required. If Docker is not installed or the daemon is not reachable the
script exits non-zero and changes nothing.
EOF
}

note() {
  printf 'desktop-build-container: %s\n' "$*" >&2
}

die() {
  printf 'desktop-build-container: %s\n' "$*" >&2
  exit 1
}

require_docker() {
  if ! command -v docker >/dev/null 2>&1; then
    die "docker is not installed; install Docker Engine and retry (see docs/DEV-SETUP.md)"
  fi
  if ! docker info >/dev/null 2>&1; then
    die "the Docker daemon is not reachable; start it and check that ${USER:-$(id -un)} can run 'docker info'"
  fi
  if [[ "$(id -u)" -eq 0 ]]; then
    die "run this script as the repository owner, not as root, so the worktree stays host-owned"
  fi
}

image_exists() {
  docker image inspect "$IMAGE" >/dev/null 2>&1
}

build_image() {
  require_docker
  [[ -f "$DOCKERFILE" ]] || die "missing Dockerfile: $DOCKERFILE"
  note "building $IMAGE from docker/desktop-build.Dockerfile"
  docker build --file "$DOCKERFILE" --tag "$IMAGE" "$ROOT"
}

ensure_image() {
  require_docker
  if image_exists; then
    note "using image $IMAGE (run '$(basename "${BASH_SOURCE[0]}") build' to refresh it)"
  else
    build_image
  fi
}

EXTRA_DOCKER_ARGS=()

run_in_container() {
  mkdir -p "$CACHE_ROOT/cargo" "$CACHE_ROOT/target" "$CACHE_ROOT/npm"
  docker run \
    --rm \
    "${EXTRA_DOCKER_ARGS[@]}" \
    --user "$(id -u):$(id -g)" \
    --workdir /work \
    --env HOME=/tmp \
    --env CARGO_HOME=/cache/cargo \
    --env CARGO_TARGET_DIR=/cache/target \
    --env npm_config_cache=/cache/npm \
    --env RUSTUP_TOOLCHAIN="$RUST_TOOLCHAIN" \
    --env CARGO_TERM_COLOR=never \
    --volume "$ROOT:/work" \
    --volume "$CACHE_ROOT:/cache" \
    "$IMAGE" \
    "$@"
}

CHECK_SCRIPT='set -euo pipefail
step() { printf "\n==> desktop-build: %s\n" "$1"; }

step "cargo fmt --all -- --check"
cargo fmt --all -- --check

# tauri-build validates bundle.externalBin while compiling, so the pinned sidecar
# must exist before cargo check/test. The fetch is checksum-verified, is a no-op
# when the gitignored binary and lock already match, and never commits the binary.
step "node scripts/fetch-engine.mjs (pinned engine sidecar, gitignored)"
node scripts/fetch-engine.mjs

# npm before the Rust steps: the Tauri build script reads frontendDist ("../dist"),
# so the frontend build must exist before the Rust host is checked.
step "npm ci"
npm ci

step "npm run typecheck"
npm run typecheck

step "npm run lint"
npm run lint

step "npm test"
npm test

step "npm run build"
npm run build

step "cargo check -p spirit-desktop"
cargo check -p spirit-desktop

step "cargo test -p spirit-desktop"
cargo test -p spirit-desktop

step "all desktop gates passed"'

TAURI_BUILD_SCRIPT='set -euo pipefail
step() { printf "\n==> desktop-build: %s\n" "$1"; }

if [[ ! -d node_modules ]]; then
  step "npm ci (node_modules is missing)"
  npm ci
fi

# The pinned sidecar is gitignored. Fetching it inside the container keeps the
# worktree host-owned and lets `tauri build` see bundle.externalBin. It is
# checksum-verified by scripts/fetch-engine.mjs and is never committed.
step "node scripts/fetch-engine.mjs (pinned engine sidecar, gitignored)"
node scripts/fetch-engine.mjs

cd apps/spirit-desktop
step "npx tauri build --debug --no-bundle"
npx --no-install tauri build --debug --no-bundle

step "tauri-build finished"'

run_check() {
  run_in_container bash -c "$CHECK_SCRIPT"
}

run_tauri_build() {
  run_in_container bash -c "$TAURI_BUILD_SCRIPT"
}

run_shell() {
  EXTRA_DOCKER_ARGS=(-i)
  if [[ -t 0 && -t 1 ]]; then
    EXTRA_DOCKER_ARGS+=(-t)
  fi
  note "interactive shell; the repository is at /work and HOME is /tmp"
  run_in_container bash
}

# The container runs as the host UID/GID, so this should always be empty. It is a
# guard against a future change that would leave root-owned files in the worktree.
assert_no_root_owned_files() {
  local offender
  offender="$(find "$ROOT" -xdev -user 0 -print -quit 2>/dev/null || true)"
  if [[ -n "$offender" ]]; then
    die "root-owned path left in the worktree: $offender"
  fi
}

if [[ $# -gt 1 ]]; then
  usage >&2
  exit 2
fi

case "${1:-check}" in
  check)
    ensure_image
    run_check
    assert_no_root_owned_files
    ;;
  tauri-build)
    ensure_image
    run_tauri_build
    assert_no_root_owned_files
    ;;
  shell)
    ensure_image
    run_shell
    ;;
  build)
    build_image
    ;;
  -h | --help | help)
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
