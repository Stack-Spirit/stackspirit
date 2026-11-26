#!/usr/bin/env bash
# Build one release archive and its per-archive checksum.
#
# Usage:
#   scripts/release/package-archive.sh \
#     --version 0.5.1-dev \
#     --target x86_64-unknown-linux-gnu \
#     --binary target/x86_64-unknown-linux-gnu/release/vowp \
#     --dist dist
#
# The archive is dist/vowp-<version>-<target>.tar.gz and contains a top-level
# directory of the same name with vowp, LICENSE, README.md and
# THIRD-PARTY-LICENSES.md. The script refuses a missing or empty input and
# prints the sorted tar listing.
#
# Private StackSpirit archive (never the public release):
#   scripts/release/package-archive.sh --product stackspirit \
#     --version 0.5.1-dev --target x86_64-unknown-linux-gnu \
#     --stackspirit target/<t>/release/stackspirit --vowp target/<t>/release/vowp \
#     --stackspirit-daemon target/<t>/release/stackspirit-daemon \
#     --vowp-daemon target/<t>/release/vowp-daemon --dist dist
# builds dist/stackspirit-<version>-<target>.tar.gz with all four binaries and
# refuses to build it when any CLI or daemon binary is missing or empty.
#
# Exit status: 0 on success, 2 on a usage error, 1 on a packaging failure.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

usage() {
  sed -n '2,27p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

fail() {
  printf 'package-archive: %s\n' "$*" >&2
  exit 1
}

version=
target=
binary=
dist=dist
product=vowp
stackspirit_bin=
vowp_bin=
stackspirit_daemon_bin=
vowp_daemon_bin=

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      version=$2
      shift 2
      ;;
    --target)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      target=$2
      shift 2
      ;;
    --binary)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      binary=$2
      shift 2
      ;;
    --dist)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      dist=$2
      shift 2
      ;;
    --product)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      product=$2
      shift 2
      ;;
    --stackspirit)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      stackspirit_bin=$2
      shift 2
      ;;
    --vowp)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      vowp_bin=$2
      shift 2
      ;;
    --stackspirit-daemon)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      stackspirit_daemon_bin=$2
      shift 2
      ;;
    --vowp-daemon)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      vowp_daemon_bin=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'package-archive: unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

[ -n "$version" ] || { usage >&2; exit 2; }
[ -n "$target" ] || { usage >&2; exit 2; }
case "$product" in
  vowp) [ -n "$binary" ] || { usage >&2; exit 2; } ;;
  stackspirit)
    [ -z "$binary" ] || fail "--binary is the public VOWP input; use --stackspirit, --vowp, --stackspirit-daemon and --vowp-daemon"
    ;;
  *) fail "unknown product: $product (use vowp or stackspirit)" ;;
esac
case "$version" in
  *[!0-9A-Za-z.+-]*) fail "invalid version: $version" ;;
esac
case "$target" in
  *[!0-9A-Za-z_.-]*) fail "invalid target: $target" ;;
esac

# The binaries this product must carry: "<archive member>=<input path>".
if [ "$product" = stackspirit ]; then
  members=(
    "stackspirit=$stackspirit_bin"
    "vowp=$vowp_bin"
    "stackspirit-daemon=$stackspirit_daemon_bin"
    "vowp-daemon=$vowp_daemon_bin"
  )
else
  members=("vowp=$binary")
fi
for entry in "${members[@]}"; do
  member=${entry%%=*}
  path=${entry#*=}
  [ -n "$path" ] || fail "the $member binary was not given"
  [ -f "$path" ] || fail "$member binary is missing: $path"
  [ -s "$path" ] || fail "$member binary is empty: $path"
done
for file in LICENSE README.md THIRD-PARTY-LICENSES.md; do
  [ -f "$ROOT/$file" ] || fail "$file is missing from $ROOT"
  [ -s "$ROOT/$file" ] || fail "$file is empty in $ROOT"
done

name="${product}-${version}-${target}"
staging="$dist/$name"
archive="$dist/$name.tar.gz"

mkdir -p "$dist"
rm -rf "$staging"
mkdir -p "$staging"

cleanup() {
  rm -rf "$staging"
}
trap cleanup EXIT HUP INT TERM

required=()
for entry in "${members[@]}"; do
  member=${entry%%=*}
  cp "${entry#*=}" "$staging/$member"
  chmod 0755 "$staging/$member"
  required+=("$member")
done
cp "$ROOT/LICENSE" "$ROOT/README.md" "$ROOT/THIRD-PARTY-LICENSES.md" "$staging/"
required+=(LICENSE README.md THIRD-PARTY-LICENSES.md)

for file in "${required[@]}"; do
  [ -s "$staging/$file" ] || fail "$file is missing or empty in $staging"
done

COPYFILE_DISABLE=1 tar -C "$dist" -czf "$archive" "$name"
rm -rf "$staging"
trap - EXIT HUP INT TERM

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$dist" && sha256sum "${name}.tar.gz" > "${name}.tar.gz.sha256")
else
  (cd "$dist" && shasum -a 256 "${name}.tar.gz" > "${name}.tar.gz.sha256")
fi

# Verify the archive itself, not only the staging directory.
listing=$(cd "$dist" && tar -tzf "${name}.tar.gz" | sort)
for file in "${required[@]}"; do
  printf '%s\n' "$listing" | grep -Fxq "$name/$file" || fail "archive is missing $name/$file"
done
printf '%s\n' "$listing"
