#!/usr/bin/env bash
# Hermetic self-test for the history-aware public-readiness scan.
#
# Builds a throwaway git repository, commits synthetic fixtures, and checks the
# scanner's history, one-commit (--rev) and working-tree modes. No network, no
# real secrets, and no dependency outside git, bash and Python 3.
#
# Usage:
#   bash scripts/check-public-readiness-history.test.sh
#
# Exit status: 0 when every check passes, 1 on the first failure.

set -euo pipefail

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
scanner="$script_dir/check-public-readiness.sh"
[ -f "$scanner" ] || {
  echo "history-test: FAIL: scanner not found: $scanner" >&2
  exit 1
}

tmp=$(mktemp -d "${TMPDIR:-/tmp}/ss-readiness-history.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
repo=
repo_initial=

# Keep the fixture independent of the caller's git configuration.
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_SYSTEM=/dev/null
export GIT_AUTHOR_NAME="History Test"
export GIT_AUTHOR_EMAIL="history-test@example.com"
export GIT_COMMITTER_NAME="History Test"
export GIT_COMMITTER_EMAIL="history-test@example.com"

commit_number=0
commit_all() {
  commit_number=$((commit_number + 1))
  stamp=$(printf '2026-01-01T00:00:%02dZ' "$commit_number")
  GIT_AUTHOR_DATE="$stamp" GIT_COMMITTER_DATE="$stamp" \
    git -C "$repo" add -f -A
  GIT_AUTHOR_DATE="$stamp" GIT_COMMITTER_DATE="$stamp" \
    git -C "$repo" commit -q -m "$1"
  git -C "$repo" rev-parse HEAD
}

new_repo() {
  repo="$tmp/$1"
  mkdir -p "$repo/scripts"
  cp "$scanner" "$repo/scripts/check-public-readiness.sh"
  git -C "$repo" init -q
  git -C "$repo" config user.name "History Test"
  git -C "$repo" config user.email "history-test@example.com"
  git -C "$repo" config commit.gpgsign false
  git -C "$repo" config core.hooksPath /dev/null
  commit_number=0
  printf 'clean fixture\n' > "$repo/README.md"
  repo_initial=$(commit_all "initial clean fixture")
}

output=
status=0
run_scan() {
  set +e
  output=$(bash "$repo/scripts/check-public-readiness.sh" "$@" 2>&1)
  status=$?
  set -e
}

fail() {
  echo "history-test: FAIL: $*" >&2
  echo "--- scanner output ---" >&2
  printf '%s\n' "$output" >&2
  exit 1
}

assert_status() {
  [ "$status" -eq "$1" ] || fail "expected exit $1, got $status"
}

assert_contains() {
  printf '%s\n' "$output" | grep -Fq -- "$1" || fail "missing: $1"
}

assert_not_contains() {
  if printf '%s\n' "$output" | grep -Fq -- "$1"; then
    fail "unexpected: $1"
  fi
}

# Synthetic values are assembled at runtime so this test file itself never
# contains a token-shaped literal or a non-placeholder home path.
token="sk-$(printf 's%023d' 1)"
home_path="/home/$(printf 'realuser')"

new_repo token-history

# 1. A clean commit tree passes --rev and prints the commit and blob counts.
run_scan --rev "$repo_initial"
assert_status 0
assert_contains "scope: tree of one commit"
assert_contains "revision: $repo_initial"
assert_contains "tree: "
assert_contains "PASS"
assert_not_contains "scope: all commits"

# 2. --rev is mutually exclusive with --history/--strict-history and requires
#    a commit argument.
run_scan --rev "$repo_initial" --history
assert_status 2
assert_contains "cannot be combined"
run_scan --rev
assert_status 2
assert_contains "--rev requires a commit"

# 3. A clean history passes.
run_scan --history
assert_status 0
assert_contains "history: commits walked: 1"
assert_contains "history: secret-class findings: 0"
assert_contains "history: privacy-class findings: 0"
assert_contains "history: PASS"
assert_contains "history: exit status: 0"

# 4. A token added in one commit is reported at that commit, masked, and is
#    fatal in --history mode.
printf 'token=%s\n' "$token" > "$repo/token.txt"
token_head=$(commit_all "add synthetic token fixture")
token_short=${token_head:0:12}
run_scan --history
assert_status 1
assert_contains "$token_short token.txt:1: provider_token: sk-s***"
assert_contains "history: secret-class findings: 1"
assert_contains "history: FAIL: secret-class finding(s)"
assert_contains "history: exit status: 1"
assert_not_contains "$token"

# 5. The same token commit fails --rev even after the token leaves HEAD, and
#    the finding row is masked.
run_scan --rev "$token_head"
assert_status 1
assert_contains "revision: $token_head"
assert_contains "token.txt:1: provider_token: sk-s***"
assert_contains "FAIL: 1 blocked finding(s)"
assert_not_contains "$token"

# 6. Removing the token later does not erase it from the history report.
git -C "$repo" rm -q token.txt
commit_all "remove synthetic token fixture" >/dev/null
run_scan --history
assert_status 1
assert_contains "$token_short token.txt:1: provider_token: sk-s***"
assert_not_contains "$token"

# 7. The clean HEAD commit passes --rev; the working-tree mode on the same
#    fixture also passes and does not emit history lines.
run_scan --rev HEAD
assert_status 0
assert_contains "revision: "
assert_contains "PASS"
run_scan
assert_status 0
assert_contains "scope: tracked working tree at HEAD"
assert_contains "PASS"
assert_not_contains "scope: all commits"

new_repo privacy-history

# 8. A personal home path is reported as privacy-class, passes plain
#    --history, and fails only with --strict-history.
printf 'path=%s\n' "$home_path" > "$repo/home.txt"
home_head=$(commit_all "add synthetic home path fixture")
home_short=${home_head:0:12}
run_scan --history
assert_status 0
assert_contains "history: privacy-class findings: 1"
assert_contains "$home_short home.txt:1: home_path: /hom***"
assert_contains "history: PASS: no secret-class finding"
assert_not_contains "$home_path"
run_scan --history --strict-history
assert_status 1
assert_contains "history: privacy-class findings: 1"
assert_contains "history: FAIL: privacy-class finding(s)"
assert_contains "$home_short home.txt:1: home_path: /hom***"
assert_contains "history: exit status: 1"
assert_not_contains "$home_path"

# 9. The same home-path commit fails --rev, although plain --history passes:
#    every finding class is fatal in the tree that would be published.
run_scan --rev "$home_head"
assert_status 1
assert_contains "revision: $home_head"
assert_contains "personal /home or /Users path: 1 blocked"
assert_contains "home.txt:1: home_path: /hom***"
assert_contains "FAIL: 1 blocked finding(s)"
assert_not_contains "$home_path"

# Remove the home path from HEAD; it remains in history.
git -C "$repo" rm -q home.txt
commit_all "remove synthetic home path fixture" >/dev/null
run_scan --rev HEAD
assert_status 0
assert_contains "PASS"

# 10. A tracked credential file in history is a secret_path finding.
printf 'DEMO=ok\n' > "$repo/.env"
env_head=$(commit_all "add tracked env fixture")
env_short=${env_head:0:12}
run_scan --history
assert_status 1
assert_contains "$env_short .env:0: secret_path: .env"
assert_contains "history: secret-class findings: 1"

# 11. The working-tree mode keeps its current behaviour on the same fixture:
#    the tracked .env is still a blocked secret_path finding.
run_scan
assert_status 1
assert_contains "tracked credential file: 1 blocked"
assert_contains ".env:0: secret_path: .env"
assert_contains "FAIL: 1 blocked finding(s)"
assert_not_contains "scope: all commits"

echo "history-test: PASS"
