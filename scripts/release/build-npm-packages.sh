#!/usr/bin/env bash
# Build the public `stackspirit` npm tarballs: four platform packages and the root launcher.
#
# Spec: docs/design/relay-and-packaging.md §3.5. Offline: it only runs `tar` and `npm pack`.
#
#   scripts/release/build-npm-packages.sh --version 0.6.0 \
#     --target linux-x64=dist/stackspirit-0.6.0-x86_64-unknown-linux-gnu.tar.gz \
#     --target linux-arm64=... --target darwin-x64=... --target darwin-arm64=... \
#     --out dist/npm
#
# A target value is a release archive (.tar.gz) or a directory holding the four binaries.
# Nothing is published; the tarballs and SHA256SUMS in --out are the publish job's only input.
set -euo pipefail

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
NPM_DIR=$ROOT/packaging/npm
BINARIES=(stackspirit vowp stackspirit-daemon vowp-daemon)
KEYS=(linux-x64 linux-arm64 darwin-x64 darwin-arm64)
MAX_BYTES=$((64 * 1024 * 1024))

fail() { echo "build-npm-packages: $*" >&2; exit 1; }

version=""
out=""
declare -A targets=()
while [ $# -gt 0 ]; do
  case $1 in
    --version) version=${2:?--version needs a value}; shift 2 ;;
    --out) out=${2:?--out needs a value}; shift 2 ;;
    --target)
      spec=${2:?--target needs KEY=PATH}
      key=${spec%%=*}
      [ "$key" != "$spec" ] || fail "--target needs KEY=PATH, got $spec"
      case " ${KEYS[*]} " in *" $key "*) ;; *) fail "unknown target key $key" ;; esac
      targets[$key]=${spec#*=}
      shift 2 ;;
    -h|--help) sed -n 2,13p "$0"; exit 0 ;;
    *) fail "unknown argument $1" ;;
  esac
done
[ -n "$version" ] || fail "--version is required"
[ -n "$out" ] || fail "--out is required"
for key in "${KEYS[@]}"; do
  [ -n "${targets[$key]:-}" ] || fail "missing --target $key=..."
done
command -v npm >/dev/null 2>&1 || fail "npm is required"

mkdir -p "$out"
out=$(CDPATH='' cd -- "$out" && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/stackspirit-npm.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
export npm_config_update_notifier=false npm_config_audit=false npm_config_fund=false
export npm_config_cache=$work/npm-cache

json_field() { node -e 'const v=require(process.argv[1])[process.argv[2]];process.stdout.write(typeof v==="string"?v:"")' "$1" "$2"; }

file_mode() { stat -c '%a' "$1" 2>/dev/null || stat -f '%Lp' "$1"; }

# Verifies a packed tarball: only allowed members, size cap. $1 tarball, rest = allowed regexes.
check_tarball() {
  local tarball=$1; shift
  local size member ok pattern
  size=$(wc -c < "$tarball")
  [ "$size" -le "$MAX_BYTES" ] || fail "$tarball is $size bytes, over 64 MiB"
  while IFS= read -r member; do
    ok=0
    for pattern in "$@"; do
      if [[ $member =~ $pattern ]]; then ok=1; break; fi
    done
    [ "$ok" -eq 1 ] || fail "$tarball contains unexpected member $member"
  done < <(tar -tzf "$tarball")
}

# Verifies every bin/* member of a platform tarball is a 0755 regular file.
check_binary_modes() {
  local tarball=$1 line mode
  while IFS= read -r line; do
    case $line in
      *" package/bin/"*)
        mode=${line%% *}
        [ "$mode" = "-rwxr-xr-x" ] || fail "$tarball has a binary that is not a 0755 regular file: $line" ;;
    esac
  done < <(tar -tvzf "$tarball")
}

sums=()
for key in "${KEYS[@]}"; do
  source_path=${targets[$key]}
  stage=$work/platform-$key
  extract=$work/extract-$key
  mkdir -p "$stage/bin" "$extract"

  template=$NPM_DIR/platforms/$key/package.json
  [ -f "$template" ] || fail "missing platform template $template"
  [ "$(json_field "$template" version)" = "$version" ] ||
    fail "version mismatch: $template is $(json_field "$template" version), --version is $version"

  if [ -d "$source_path" ]; then
    from=$source_path
  elif [ -f "$source_path" ]; then
    tar -xzf "$source_path" -C "$extract" || fail "cannot extract $source_path"
    tops=$(find "$extract" -mindepth 1 -maxdepth 1)
    [ "$(printf '%s\n' "$tops" | wc -l)" -eq 1 ] && [ -d "$tops" ] ||
      fail "$source_path must contain exactly one top-level directory"
    from=$tops
  else
    fail "target $key: $source_path is neither an archive nor a directory"
  fi

  for name in "${BINARIES[@]}"; do
    path=$from/$name
    [ -e "$path" ] || [ -L "$path" ] || fail "target $key is missing $name"
    [ ! -L "$path" ] || fail "target $key: $name is a symlink"
    [ -f "$path" ] || fail "target $key: $name is not a regular file"
    [ -s "$path" ] || fail "target $key: $name is empty"
    [ "$(file_mode "$path")" = "755" ] || fail "target $key: $name has mode $(file_mode "$path"), expected 0755"
    cp "$path" "$stage/bin/$name"
    chmod 0755 "$stage/bin/$name"
  done
  cp "$template" "$stage/package.json"
  cp "$NPM_DIR/stackspirit/LICENSE" "$stage/LICENSE"

  (cd "$stage" && npm pack --loglevel=error --ignore-scripts --pack-destination "$out" >/dev/null) ||
    fail "npm pack failed for $key"
  tarball=$out/stackspirit-$key-$version.tgz
  [ -f "$tarball" ] || fail "npm pack did not produce $tarball"
  check_tarball "$tarball" '^package/package\.json$' '^package/LICENSE$' '^package/bin/[a-z-]+$'
  check_binary_modes "$tarball"
  sums+=("$(basename "$tarball")")
done

# Root package.
root_stage=$work/root
mkdir -p "$root_stage"
cp -R "$NPM_DIR/stackspirit/." "$root_stage/"
[ "$(json_field "$root_stage/package.json" version)" = "$version" ] ||
  fail "version mismatch: root package is $(json_field "$root_stage/package.json" version), --version is $version"
node -e '
  const pkg = require(process.argv[1]);
  const want = process.argv[2];
  const bad = Object.entries(pkg.optionalDependencies || {}).filter(([, v]) => v !== want);
  if (bad.length || Object.keys(pkg.optionalDependencies || {}).length !== 4) {
    console.error("optionalDependencies must be exactly four packages at " + want);
    process.exit(1);
  }
  for (const hook of ["preinstall", "install", "postinstall"]) {
    if (pkg.scripts && pkg.scripts[hook]) { console.error("install hook " + hook); process.exit(1); }
  }
' "$root_stage/package.json" "$version" || fail "root package.json is not publishable"
(cd "$root_stage" && npm pack --loglevel=error --ignore-scripts --pack-destination "$out" >/dev/null) || fail "npm pack failed for the root package"
root_tarball=$out/stackspirit-$version.tgz
[ -f "$root_tarball" ] || fail "npm pack did not produce $root_tarball"
check_tarball "$root_tarball" '^package/package\.json$' '^package/LICENSE$' '^package/README\.md$' \
  '^package/bin/[a-z-]+\.js$' '^package/lib/[a-z-]+\.js$'
sums+=("$(basename "$root_tarball")")

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$out" && sha256sum "${sums[@]}" > SHA256SUMS)
else
  (cd "$out" && shasum -a 256 "${sums[@]}" > SHA256SUMS)
fi
printf '%s\n' "${sums[@]}"
