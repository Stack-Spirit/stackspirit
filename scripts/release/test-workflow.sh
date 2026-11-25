#!/usr/bin/env bash
# Offline tests for .github/workflows/release.yml and scripts/release/check-release.sh.
# Spec: docs/design/relay-and-packaging.md section 5.7. Needs python3 with PyYAML.
# Run: bash scripts/release/test-workflow.sh
set -euo pipefail

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
WORKFLOW=${WORKFLOW:-$ROOT/.github/workflows/release.yml}
CHECK=$ROOT/scripts/release/check-release.sh
TMP=$(mktemp -d "${TMPDIR:-/tmp}/vowp-test-workflow.XXXXXX")
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

failures=0
ok() { echo "ok - $*"; }
bad() { echo "FAIL - $*" >&2; failures=$((failures + 1)); }

python3 -c 'import yaml' 2>/dev/null || { echo "test-workflow: PyYAML is required" >&2; exit 1; }

# --- workflow structure ---------------------------------------------------------------------
py_out=$(python3 -I - "$WORKFLOW" <<'PY'
import re, sys, yaml
path = sys.argv[1]
text = open(path).read()
wf = yaml.safe_load(text)
jobs = wf["jobs"]
out = []
def check(name, cond):
    out.append(("ok" if cond else "FAIL") + " - " + name)

# id-token: write only in the two publish jobs
holders = sorted(j for j, d in jobs.items() if (d.get("permissions") or {}).get("id-token") == "write")
check("id-token: write only in publish-npm and publish-pypi (found %s)" % holders,
      holders == ["publish-npm", "publish-pypi"])
check("no workflow-level id-token", "id-token" not in (wf.get("permissions") or {}))
check("top-level permissions are exactly contents: read", wf.get("permissions") == {"contents": "read"})

# no stored registry credential
for secret in ("NPM_TOKEN", "PYPI_API_TOKEN", "TWINE_PASSWORD", "CARGO_REGISTRY_TOKEN", "NODE_AUTH_TOKEN"):
    check("no %s" % secret, secret not in text)
check("no secrets.* reference", "secrets." not in text)

# environments
check("publish-npm uses environment npm", jobs["publish-npm"].get("environment") == "npm")
check("publish-pypi uses environment pypi", jobs["publish-pypi"].get("environment") == "pypi")
check("github-release uses environment release", jobs["github-release"].get("environment") == "release")

# the GitHub release is draft only, tag push only, and publishes no registry
gr = jobs["github-release"]
check("github-release runs only on a tag push", "push" in gr["if"] and "refs/tags/" in gr["if"])
check("github-release has contents: write only", gr.get("permissions") == {"contents": "write"})
steps = [s for s in gr["steps"] if "softprops/action-gh-release" in s.get("uses", "")]
check("github-release creates exactly one release step", len(steps) == 1)
if steps:
    w = steps[0]["with"]
    check("release is draft: true", w.get("draft") is True)
gr_text = yaml.safe_dump(gr)
check("github-release never runs npm publish or the PyPI action",
      "npm publish" not in gr_text and "pypi-publish" not in gr_text)
# only the publish jobs may publish
for j, d in jobs.items():
    t = yaml.safe_dump(d)
    publishes = "npm publish" in t or "gh-action-pypi-publish" in t
    check("%s %s publish" % (j, "may" if j in ("publish-npm", "publish-pypi") else "must not"),
          publishes == (j in ("publish-npm", "publish-pypi")))
    if j in ("publish-npm", "publish-pypi"):
        check("%s runs only in publish mode" % j, "mode == 'publish'" in d["if"])
        check("%s never builds" % j, "cargo build" not in t and "package-archive" not in t)
check("a tag push alone cannot reach a publish job",
      all("push" not in jobs[j]["if"] for j in ("publish-npm", "publish-pypi")))
check("release event triggers publish", wf[True]["release"]["types"] == ["published"])
check("dispatch publish defaults to false", wf[True]["workflow_dispatch"]["inputs"]["publish"]["default"] is False)
check("publish-npm uses --provenance --access public", "--provenance --access public" in yaml.safe_dump(jobs["publish-npm"]))
check("publish-npm installs npm@11", "npm@11" in yaml.safe_dump(jobs["publish-npm"]))
check("verify-install needs both publish jobs", set(jobs["verify-install"]["needs"]) >= {"publish-npm", "publish-pypi"})

# every uses: is pinned to a 40-char SHA
uses = re.findall(r"^\s*(?:-\s*)?uses:\s*(\S+)", text, re.M)
unpinned = [u for u in uses if not re.fullmatch(r"[\w.-]+/[\w./-]+@[0-9a-f]{40}", u)]
check("every uses: is a 40-character commit SHA (%d actions)" % len(uses), uses and not unpinned)
if unpinned:
    out.append("FAIL - unpinned: %s" % unpinned)

# build matrix is exactly the four supported targets
targets = sorted(m["target"] for m in jobs["build"]["strategy"]["matrix"]["include"])
check("build matrix is exactly the four targets",
      targets == sorted(["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
                         "x86_64-apple-darwin", "aarch64-apple-darwin"]))

# artifact contents and smoke tests
for needle, label in [
    ("package-archive.sh", "archives"), ("--product stackspirit", "private stackspirit archive"),
    ("build-npm-packages.sh", "npm tarballs"), ("build-pypi-wheels.sh", "wheels"),
    ("SHA256SUMS", "SHA256SUMS"), ("npx -y \"stackspirit@$VERSION\" --version", "npx --version smoke"),
    ("npx -y -p \"stackspirit@$VERSION\" vowp --version", "npx vowp-name smoke"),
    ("\"method\":\"initialize\"", "mcp initialize smoke"), ("uvx \"stackspirit@$VERSION\" --version", "uvx --version smoke"),
    ("uvx --from \"stackspirit==$VERSION\" vowp --version", "uvx vowp-name smoke"),
    ("check-release.sh", "version check"), ("docs/release/", "release notes from docs/release"),
    ("attest-build-provenance", "attestation"), ("packaging/homebrew/update.sh", "homebrew artifact"),
]:
    check("workflow has %s" % label, needle in text)
check("homebrew is an artifact, never pushed to a tap", "git push" not in text)
print("\n".join(out))
PY
) || { echo "$py_out"; echo "test-workflow: workflow parse failed" >&2; exit 1; }
while IFS= read -r line; do
  case $line in ok\ -*) echo "$line" ;; *) echo "$line" >&2; failures=$((failures + 1)) ;; esac
done <<< "$py_out"

# --- the checker must reject a mutated workflow ---------------------------------------------
mutate_expect_fail() {
  local label=$1 sedexpr=$2
  sed "$sedexpr" "$WORKFLOW" > "$TMP/mut.yml"
  if WORKFLOW=$TMP/mut.yml bash "$0" --self-mutation >/dev/null 2>&1; then bad "mutation not caught: $label"; else ok "mutation caught: $label"; fi
}
if [ "${1:-}" = "--self-mutation" ]; then
  # Re-run only the structural checks against $WORKFLOW; exit non-zero on any failure.
  exec bash "$0" --structure-only
fi
if [ "${1:-}" = "--structure-only" ]; then exit "$failures"; fi

mutate_expect_fail "NPM_TOKEN added" 's/^permissions:/  # NPM_TOKEN\npermissions:/'
mutate_expect_fail "publish-npm environment removed" '/^    environment: npm$/d'
mutate_expect_fail "publish-pypi environment removed" '/^    environment: pypi$/d'
mutate_expect_fail "release no longer a draft" 's/^          draft: true$/          draft: false/'
mutate_expect_fail "an action unpinned" '0,/@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0/s//@v4/'
mutate_expect_fail "a matrix target dropped" '/target: aarch64-apple-darwin$/d'
mutate_expect_fail "publish default true" '0,/default: false/s//default: true/'

# --- check-release.sh over a synthetic manifest tree ----------------------------------------
make_tree() {
  local d=$1 v=$2 key
  mkdir -p "$d/packaging/npm/stackspirit" "$d/packaging/pypi/stackspirit" "$d/packaging/homebrew"
  printf '[workspace.package]\nversion = "%s"\n' "$v" > "$d/Cargo.toml"
  printf '{"name":"stackspirit","version":"%s","optionalDependencies":{"@stackspirit/linux-x64":"%s"}}\n' "$v" "$v" \
    > "$d/packaging/npm/stackspirit/package.json"
  for key in linux-x64 linux-arm64 darwin-x64 darwin-arm64; do
    mkdir -p "$d/packaging/npm/platforms/$key"
    printf '{"name":"@stackspirit/%s","version":"%s"}\n' "$key" "$v" > "$d/packaging/npm/platforms/$key/package.json"
  done
  printf '[project]\nname = "stackspirit"\nversion = "%s"\n' "$v" > "$d/packaging/pypi/stackspirit/pyproject.toml"
  printf 'class Vowp < Formula\n  version "__VOWP_VERSION__"\nend\n' > "$d/packaging/homebrew/vowp.rb"
}
good=$TMP/good
make_tree "$good" 1.2.3
if bash "$CHECK" --root "$good" --version 1.2.3 --tag v1.2.3 --publish >/dev/null 2>&1; then
  ok "check-release accepts a consistent tree"
else bad "check-release rejected a consistent tree"; fi

expect_reject() {
  local label=$1; shift
  if bash "$CHECK" "$@" >/dev/null 2>&1; then bad "check-release accepted: $label"; else ok "check-release rejects: $label"; fi
}
expect_reject "wrong tag" --root "$good" --version 1.2.3 --tag v1.2.4
expect_reject "version differs from Cargo.toml" --root "$good" --version 1.2.4 --tag v1.2.4
for pre in 1.2.3-dev 1.2.3-alpha.1 1.2.3-beta.1 1.2.3-rc.1; do
  t=$TMP/pre-$pre; make_tree "$t" "$pre"
  expect_reject "non-final $pre on a publish path" --root "$t" --version "$pre" --tag "v$pre" --publish
done
t=$TMP/pre-ok; make_tree "$t" 1.2.3-rc.1
if bash "$CHECK" --root "$t" --version 1.2.3-rc.1 --tag v1.2.3-rc.1 >/dev/null 2>&1; then
  ok "check-release allows a non-final version on a build-only path"; else bad "non-final build-only path rejected"; fi

mutate_tree() {
  local label=$1 file=$2 expr=$3 t=$TMP/m-$RANDOM
  make_tree "$t" 1.2.3
  sed -i "$expr" "$t/$file"
  expect_reject "$label" --root "$t" --version 1.2.3 --tag v1.2.3 --publish
}
mutate_tree "Cargo version mismatch" Cargo.toml 's/1\.2\.3/1.2.9/'
mutate_tree "npm root version mismatch" packaging/npm/stackspirit/package.json 's/"version":"1\.2\.3"/"version":"1.2.9"/'
mutate_tree "npm optionalDependency mismatch" packaging/npm/stackspirit/package.json 's/"@stackspirit\/linux-x64":"1\.2\.3"/"@stackspirit\/linux-x64":"1.2.9"/'
for key in linux-x64 linux-arm64 darwin-x64 darwin-arm64; do
  mutate_tree "npm platform $key version mismatch" "packaging/npm/platforms/$key/package.json" 's/1\.2\.3/1.2.9/'
done
mutate_tree "PyPI version mismatch" packaging/pypi/stackspirit/pyproject.toml 's/1\.2\.3/1.2.9/'
mutate_tree "Homebrew literal version mismatch" packaging/homebrew/vowp.rb 's/__VOWP_VERSION__/1.2.9/'
t=$TMP/missing; make_tree "$t" 1.2.3; rm "$t/packaging/npm/platforms/darwin-arm64/package.json"
expect_reject "a missing platform package" --root "$t" --version 1.2.3 --tag v1.2.3

if [ "$failures" -ne 0 ]; then echo "test-workflow: $failures failure(s)" >&2; exit 1; fi
echo "test-workflow: all checks passed"
