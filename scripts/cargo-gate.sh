#!/bin/sh
# Run cargo from the repository root after ensuring the pinned Tauri sidecar exists.
#
# tauri-build validates bundle.externalBin while compiling apps/spirit-desktop, so the
# gitignored sidecar in apps/spirit-desktop/src-tauri/binaries/ must exist before any
# workspace cargo command reaches Rust code. The fetch is checksum-verified and a no-op
# when the pinned binary is already present.
#
# Usage: scripts/cargo-gate.sh <cargo args...>
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)

cd "$repo_root"

node scripts/fetch-engine.mjs
exec cargo "$@"
