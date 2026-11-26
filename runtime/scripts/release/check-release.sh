#!/usr/bin/env bash
# Check that a release version is consistent across the tree. Offline; never publishes.
#
# Usage:
#   scripts/release/check-release.sh --version X.Y.Z [--tag vX.Y.Z] [--publish] [--root DIR]
#
# Rejects: a tag that is not v<version>; a version that is not the Cargo.toml workspace version;
# a non-final version (-dev, -alpha, -beta, -rc) when --publish is given; an npm root package,
# npm platform package, optionalDependency or PyPI pyproject version that differs; a Homebrew
# formula that pins a different literal version (a __*_VERSION__ placeholder is fine).
# Registry "already published" checks run in the workflow, not here.
#
# Exit status: 0 consistent, 1 inconsistent, 2 usage error.
set -euo pipefail

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
version=""
tag=""
publish=0
while [ $# -gt 0 ]; do
  case $1 in
    --version) version=${2:?--version needs a value}; shift 2 ;;
    --tag) tag=${2:?--tag needs a value}; shift 2 ;;
    --publish) publish=1; shift ;;
    --root) root=${2:?--root needs a value}; shift 2 ;;
    -h|--help) sed -n 2,13p "$0"; exit 0 ;;
    *) echo "check-release: unknown argument $1" >&2; exit 2 ;;
  esac
done
[ -n "$version" ] || { echo "check-release: --version is required" >&2; exit 2; }

errors=0
bad() { echo "check-release: $*" >&2; errors=$((errors + 1)); }

printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$' \
  || bad "version is not semver-like: $version"
if [ -n "$tag" ] && [ "$tag" != "v$version" ]; then
  bad "tag $tag is not v$version"
fi
if [ "$publish" -eq 1 ] && ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  bad "refusing non-final version $version on a publish path"
fi

cargo=$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p}' "$root/Cargo.toml" | head -n1)
[ "$cargo" = "$version" ] || bad "Cargo.toml workspace version is '$cargo', not $version"

# JSON "version" and optionalDependencies via python (always present on runners).
json_versions() {
  python3 -I - "$1" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
print(d.get("version", ""))
for v in (d.get("optionalDependencies") or {}).values():
    print(v)
PY
}
npm_files=("$root/packaging/npm/stackspirit/package.json")
for key in linux-x64 linux-arm64 darwin-x64 darwin-arm64; do
  npm_files+=("$root/packaging/npm/platforms/$key/package.json")
done
for f in "${npm_files[@]}"; do
  if [ ! -f "$f" ]; then bad "missing $f"; continue; fi
  while IFS= read -r v; do
    [ "$v" = "$version" ] || bad "$f has version '$v', not $version"
  done < <(json_versions "$f")
done

pyproject="$root/packaging/pypi/stackspirit/pyproject.toml"
if [ -f "$pyproject" ]; then
  py=$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$pyproject" | head -n1)
  [ "$py" = "$version" ] || bad "$pyproject version is '$py', not $version"
else
  bad "missing $pyproject"
fi

for formula in "$root"/packaging/homebrew/*.rb; do
  [ -f "$formula" ] || continue
  lit=$(sed -n 's/^ *version "\([^"]*\)".*/\1/p' "$formula" | head -n1)
  case $lit in
    ""|__*_VERSION__) ;;
    "$version") ;;
    *) bad "$formula pins version $lit, not $version" ;;
  esac
done

if [ "$errors" -ne 0 ]; then exit 1; fi
echo "check-release: ok ($version)"
