#!/usr/bin/env bash
# Build the four `stackspirit` platform wheels (design §4). Never publishes anything.
#
#   build-pypi-wheels.sh [--version X.Y.Z] [--target-dir DIR] [--out DIR]
#
# Binaries are read from DIR/<triple>/release/{stackspirit,vowp,stackspirit-daemon,vowp-daemon}.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
version=""
target_dir="$root/target"
out="$root/dist-pypi"

die() { echo "build-pypi-wheels: $*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --version) version="${2:?--version needs a value}"; shift 2 ;;
    --target-dir) target_dir="${2:?--target-dir needs a value}"; shift 2 ;;
    --out) out="${2:?--out needs a value}"; shift 2 ;;
    --sdist|sdist) die "refusing to build an sdist: stackspirit ships platform wheels only" ;;
    *) die "unknown argument: $1" ;;
  esac
done

if [ -z "$version" ]; then
  version="$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p}' "$root/Cargo.toml" | head -n1)"
  [ -n "$version" ] || die "cannot read the version from Cargo.toml"
fi
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] \
  || die "refusing non-final version '$version': only X.Y.Z is publishable"

targets=(
  "x86_64-unknown-linux-gnu manylinux_2_35_x86_64"
  "aarch64-unknown-linux-gnu manylinux_2_35_aarch64"
  "x86_64-apple-darwin macosx_10_12_x86_64"
  "aarch64-apple-darwin macosx_11_0_arm64"
)

# Check every input before building anything, so a failure leaves no partial set.
for entry in "${targets[@]}"; do
  triple="${entry% *}"
  for bin in stackspirit vowp stackspirit-daemon vowp-daemon; do
    [ -f "$target_dir/$triple/release/$bin" ] || die "missing binary: $target_dir/$triple/release/$bin"
  done
done

mkdir -p "$out"
for entry in "${targets[@]}"; do
  triple="${entry% *}"
  tag="${entry#* }"
  python3 -m build --wheel --no-isolation --outdir "$out" \
    --config-setting "bin-dir=$target_dir/$triple/release" \
    --config-setting "platform-tag=$tag" \
    --config-setting "target=$triple" \
    --config-setting "version=$version" \
    "$root/packaging/pypi/stackspirit" >&2
done
ls "$out"/stackspirit-*.whl
