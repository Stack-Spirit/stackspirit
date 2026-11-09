#!/bin/sh
# Real, hermetic test for scripts/audit/scan.sh.
#
# Creates a temporary git repository, plants synthetic secret-shaped values,
# and checks that the scanner:
#   - finds every rule;
#   - prints file:commit:line and masks every matched value;
#   - exits 1 for working-tree findings and 0 for a clean tree;
#   - reports old-history findings as informational by default;
#   - rejects an allowlist entry with an empty reason.
#
# The synthetic values are assembled from fragments so this script itself
# does not contain a token, private path, IP address or e-mail address.

set -u

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P) || exit 2
tmp=$(mktemp -d "${TMPDIR:-/tmp}/vowp-audit-test.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

repo=$tmp/repo
mkdir -p "$repo/scripts/audit" "$repo/keys"
cp "$script_dir/scan.sh" "$script_dir/allowlist.txt" "$repo/scripts/audit/"
chmod +x "$repo/scripts/audit/scan.sh"

git init -q -b main "$repo"
git -C "$repo" config user.name "Audit Test"
git -C "$repo" config user.email "$(printf 'audit@%s' example.invalid)"

# Build synthetic values without writing a complete pattern in this file.
home_path=$(printf '/%s/%s' home alice)
ip=$(printf '%s.%s.%s.%s' 192 168 1 10)
email=$(printf '%s@%s' alice example.org)
sk=$(printf 'sk-%s' "$(printf 'A%.0s' $(seq 1 30))")
ghp=$(printf 'ghp_%s' "$(printf 'B%.0s' $(seq 1 30))")
pat=$(printf 'github_pat_%s' "$(printf 'C%.0s' $(seq 1 24))")
aws=$(printf 'AKIA%s' '1234567890ABCDEF')
pem=$(printf '%s%s' '-----BEGIN RSA' ' PRIVATE KEY-----')
slack=$(printf 'xoxb-%s' "$(printf '1%.0s' $(seq 1 12))")
host=$(printf '%s.%s' vps local)
ssh_line=$(printf 'ssh %s@%s' alice "$host")

cat > "$repo/secrets.txt" <<EOF
ip $ip
home $home_path/private
email $email
key $sk
gh $ghp
pat $pat
aws $aws
pem $pem
slack $slack
host $host
$ssh_line
EOF
printf 'TOKEN=%s\n' "not-a-real-value" > "$repo/.env"
printf 'fake key placeholder\n' > "$repo/keys/id_rsa"
dd if=/dev/zero of="$repo/big.bin" bs=1024 count=1100 2>/dev/null
printf 'loopback 127.0.0.1\nplaceholder /home/user/project\nreserved test@example.invalid\n' > "$repo/benign.txt"

git -C "$repo" add -A
git -C "$repo" commit -qm "synthetic secrets"

fail() {
    echo "self-test: FAIL: $*" >&2
    exit 1
}

run_scan() {
    (cd "$repo" && scripts/audit/scan.sh "$@")
}

# 1. Every rule is found and the working-tree scan is fatal.
summary_out=$tmp/summary.txt
if run_scan --worktree --summary > "$summary_out" 2>&1; then
    fail "working-tree scan should fail on synthetic secrets"
fi
for category in ipv4 home_path email token private_host ssh_host secret_path large_blob; do
    grep -q "  $category .*blocked" "$summary_out" || fail "category not reported: $category"
done

# 2. Every value is masked: the full synthetic value must not appear.
full_out=$tmp/full.txt
if run_scan --worktree > "$full_out" 2>&1; then
    fail "working-tree scan should fail without --summary"
fi
grep -q 'secrets.txt:worktree:' "$full_out" || fail "file:commit:line is missing"
for raw in "$sk" "$ghp" "$pat" "$aws" "$pem" "$slack" "$ip" "$home_path" "$email" "$host"; do
    if grep -Fq -- "$raw" "$full_out"; then
        fail "scanner printed a full matched value"
    fi
done

# 3. A malformed allowlist (empty reason) is rejected with exit 2.
printf 'ipv4|secrets.txt|%s|\n' "$ip" > "$repo/bad-allowlist.txt"
if run_scan --worktree --allowlist bad-allowlist.txt >/dev/null 2>&1; then
    fail "malformed allowlist should exit 2"
else
    status=$?
    [ "$status" -eq 2 ] || fail "malformed allowlist exited $status, expected 2"
fi

# 4. Remove the synthetic secrets: the current tree is clean, old history is not.
git -C "$repo" rm -q secrets.txt .env keys/id_rsa big.bin
git -C "$repo" commit -qm "remove synthetic secrets"
run_scan --worktree >/dev/null 2>&1 || fail "clean working tree should pass"
run_scan --rev HEAD >/dev/null 2>&1 || fail "clean HEAD tree should pass"
if run_scan --rev HEAD~1 >/dev/null 2>&1; then
    fail "the old tree with secrets should fail --rev"
fi

history_out=$tmp/history.txt
run_scan --history > "$history_out" 2>&1 || fail "history should be informational by default"
grep -q 'history findings are informational' "$history_out" || fail "no informational history message"
if run_scan --history --strict-history >/dev/null 2>&1; then
    fail "history should fail with --strict-history"
fi

echo "self-test: PASS: all scanner rules, masking, allowlist and history modes work."
