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
# Rust field access such as `args.local_port` must not be mistaken for a host.
printf 'fn probe() { let _ = args.local_port; let _ = self.local.path(); let _ = listener.local_addr(); }\n' > "$repo/benign.rs"

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

# 4b. --public-tree scans only the paths that would ship. A finding under a
#     manifest prune entry is skipped; a finding in a shipping path is fatal.
mkdir -p "$repo/private" "$repo/keep"
printf 'private %s/private\n' "$home_path" > "$repo/private/bad.txt"
printf 'private %s\n' "$ip" > "$repo/keep/bad.txt"
rm -f "$repo/bad-allowlist.txt"
cat > "$repo/scan-manifest.txt" <<'EOF'
prune scripts/audit
prune private
EOF
git -C "$repo" add -A
git -C "$repo" commit -qm "add public-tree scan fixture"
public_out=$tmp/public-tree-scan.txt
if run_scan --public-tree --manifest scan-manifest.txt > "$public_out" 2>&1; then
    fail "a shipping-path finding should fail --public-tree"
fi
grep -q 'keep/bad.txt' "$public_out" || fail "--public-tree did not report the shipping-path finding"
if grep -q 'private/bad.txt' "$public_out"; then
    fail "--public-tree reported a finding under a pruned path"
fi
if grep -q 'benign.rs.*private_host' "$public_out"; then
    fail "Rust field access was reported as a private host"
fi
git -C "$repo" rm -q keep/bad.txt
git -C "$repo" commit -qm "remove the shipping-path finding"
public_pass=$tmp/public-tree-pass.txt
if ! run_scan --public-tree --manifest scan-manifest.txt > "$public_pass" 2>&1; then
    cat "$public_pass" >&2
    fail "--public-tree should pass when only pruned paths have findings"
fi

# 5. The public-tree checker rejects pruned and disallowed paths and reports
#    missing required paths. A separate repository keeps this fixture clean.
prepo=$tmp/public-repo
mkdir -p "$prepo/scripts/audit" "$prepo/.github/workflows" "$prepo/docs" "$prepo/scripts"
cp "$script_dir/public-tree-check.sh" "$script_dir/public-tree.txt" "$prepo/scripts/audit/"
chmod +x "$prepo/scripts/audit/public-tree-check.sh"
git init -q -b main "$prepo"
git -C "$prepo" config user.name "Public Tree Test"
git -C "$prepo" config user.email "$(printf 'public@%s' example.invalid)"
cat > "$prepo/public-tree-test.txt" <<'EOF'
allow .github
allow docs
allow README.md
allow scripts
allow public-tree-test.txt
prune AGENTS.md
prune internal/
require .github/workflows/ci.yml
require README.md
require docs/keep.md
require scripts/tool.sh
EOF
printf 'ci\n' > "$prepo/.github/workflows/ci.yml"
printf 'readme\n' > "$prepo/README.md"
printf 'keep\n' > "$prepo/docs/keep.md"
printf 'tool\n' > "$prepo/scripts/tool.sh"
git -C "$prepo" add -A
git -C "$prepo" commit -qm "clean public-tree fixture"

run_check() {
    (cd "$prepo" && scripts/audit/public-tree-check.sh "$@")
}

run_check --index --manifest public-tree-test.txt >/dev/null 2>&1 ||
    fail "clean public-tree fixture should pass --index"
run_check --worktree --manifest public-tree-test.txt >/dev/null 2>&1 ||
    fail "clean public-tree fixture should pass --worktree"
run_check --rev HEAD --manifest public-tree-test.txt >/dev/null 2>&1 ||
    fail "clean public-tree fixture should pass --rev"
run_check --index >/dev/null 2>&1 &&
    fail "the real manifest should not match the small fixture"

mkdir -p "$prepo/internal"
printf 'internal\n' > "$prepo/AGENTS.md"
printf 'internal\n' > "$prepo/internal/build.txt"
git -C "$prepo" add -A
git -C "$prepo" commit -qm "add pruned paths"
prune_out=$tmp/prune.txt
if run_check --index --manifest public-tree-test.txt > "$prune_out" 2>&1; then
    fail "pruned paths should fail the index check"
fi
grep -q 'pruned path present: AGENTS.md' "$prune_out" || fail "AGENTS.md was not reported as pruned"
grep -q 'pruned path present: internal/build.txt' "$prune_out" || fail "internal/build.txt was not reported as pruned"
if run_check --rev HEAD --manifest public-tree-test.txt >/dev/null 2>&1; then
    fail "pruned paths should fail the revision check"
fi

git -C "$prepo" rm -q AGENTS.md internal/build.txt
git -C "$prepo" commit -qm "remove pruned paths"
git -C "$prepo" rm -q docs/keep.md
git -C "$prepo" commit -qm "drop a required path"
required_out=$tmp/required.txt
if run_check --index --manifest public-tree-test.txt > "$required_out" 2>&1; then
    fail "a missing required path should fail the index check"
fi
grep -q 'required path missing: docs/keep.md' "$required_out" || fail "the missing required path was not reported"

mkdir -p "$prepo/docs"
printf 'keep\n' > "$prepo/docs/keep.md"
git -C "$prepo" add -A
git -C "$prepo" commit -qm "restore the required path"
mkdir -p "$prepo/site"
printf 'page\n' > "$prepo/site/index.html"
git -C "$prepo" add -A
git -C "$prepo" commit -qm "add a disallowed top-level path"
top_out=$tmp/top-level.txt
if run_check --index --manifest public-tree-test.txt > "$top_out" 2>&1; then
    fail "a disallowed top-level path should fail the index check"
fi
grep -q 'top-level path not allowed: site/index.html' "$top_out" || fail "the disallowed top-level path was not reported"

git -C "$prepo" rm -r -q site
git -C "$prepo" commit -qm "remove the disallowed top-level path"
run_check --index --manifest public-tree-test.txt >/dev/null 2>&1 ||
    fail "the repaired fixture should pass again"

# 6. The public-tree content scrub rewrites the known build-process paragraph
#    in the index only, is idempotent, accepts an already-clean file, and fails
#    on a duplicate or a partial paragraph.
srepo=$tmp/scrub-repo
mkdir -p "$srepo/scripts/audit" "$srepo/scripts/release"
cp "$script_dir/public-tree-scrub.sh" "$script_dir/public-tree-scrub.py" "$srepo/scripts/audit/"
chmod +x "$srepo/scripts/audit/public-tree-scrub.sh"
git init -q -b main "$srepo"
git -C "$srepo" config user.name "Public Scrub Test"
git -C "$srepo" config user.email "$(printf 'scrub@%s' example.invalid)"

run_scrub() {
    (cd "$srepo" && scripts/audit/public-tree-scrub.sh --index)
}

show_staged() {
    (cd "$srepo" && git show :scripts/release/version-test.rs)
}

cat > "$srepo/scripts/release/version-test.rs" <<'EOF'
//! Integration test for the `vowp --version` output.
//!
//! The publishable `vowp-cli` package manifest lives at the workspace root (see the comment
//! in the root `Cargo.toml`), so this test is wired up as the `version` target there. It is
//! the same check as `crates/vowp-cli/tests/version.rs`, which the packaging job does not own
//! and therefore cannot update with the release version.

use std::process::Command;
EOF
git -C "$srepo" add -A
git -C "$srepo" commit -qm "scrub fixture with the old paragraph"
original=$(cat "$srepo/scripts/release/version-test.rs")
run_scrub >/dev/null 2>&1 || fail "the scrub should rewrite the old paragraph"
if ! show_staged | grep -q 'asserts the same version that the root package publishes'; then
    fail "the scrub did not write the product paragraph"
fi
if show_staged | grep -q 'packaging job does not own'; then
    fail "the scrub left the build-process paragraph in the index"
fi
[ "$(cat "$srepo/scripts/release/version-test.rs")" = "$original" ] ||
    fail "the scrub changed the private working tree"

run_scrub >/dev/null 2>&1 ||
    fail "the scrub should be idempotent on an already-scrubbed file"
if show_staged | grep -q 'packaging job does not own'; then
    fail "the idempotent scrub reintroduced the old paragraph"
fi

# A duplicate old paragraph is ambiguous and must fail with the count.
cat > "$srepo/scripts/release/version-test.rs" <<'EOF'
//! Integration test for the `vowp --version` output.
//!
//! The publishable `vowp-cli` package manifest lives at the workspace root (see the comment
//! in the root `Cargo.toml`), so this test is wired up as the `version` target there. It is
//! the same check as `crates/vowp-cli/tests/version.rs`, which the packaging job does not own
//! and therefore cannot update with the release version.
//!
//! The publishable `vowp-cli` package manifest lives at the workspace root (see the comment
//! in the root `Cargo.toml`), so this test is wired up as the `version` target there. It is
//! the same check as `crates/vowp-cli/tests/version.rs`, which the packaging job does not own
//! and therefore cannot update with the release version.

use std::process::Command;
EOF
git -C "$srepo" add scripts/release/version-test.rs
duplicate_out=$tmp/scrub-duplicate.txt
if run_scrub > "$duplicate_out" 2>&1; then
    fail "the scrub should fail on a duplicate old paragraph"
fi
grep -q 'found 2 copies of the old paragraph' "$duplicate_out" ||
    fail "the duplicate failure did not report the count"

# A partial old paragraph must fail instead of passing as already clean.
cat > "$srepo/scripts/release/version-test.rs" <<'EOF'
//! Integration test for the `vowp --version` output.
//!
//! It checks that `vowp --version` succeeds and prints the package version.
//! and therefore cannot update with the release version.

use std::process::Command;
EOF
git -C "$srepo" add scripts/release/version-test.rs
partial_out=$tmp/scrub-partial.txt
if run_scrub > "$partial_out" 2>&1; then
    fail "the scrub should fail on a partial old paragraph"
fi
grep -q 'partial paragraph' "$partial_out" ||
    fail "the partial failure did not identify the paragraph"

# An already-clean file (the paragraph absent, as in the private tree) is a
# no-op and must still pass.
cat > "$srepo/scripts/release/version-test.rs" <<'EOF'
//! Integration test for the `vowp --version` output.
//!
//! It checks that `vowp --version` succeeds, prints `vowp <package version>`, and reports
//! the expected package version `0.1.0`.

use std::process::Command;
EOF
git -C "$srepo" add scripts/release/version-test.rs
clean=$(cat "$srepo/scripts/release/version-test.rs")
clean_out=$tmp/scrub-clean.txt
run_scrub > "$clean_out" 2>&1 || fail "the scrub should pass on an already-clean file"
grep -q 'already clean' "$clean_out" || fail "the already-clean scrub did not report a no-op"
[ "$(show_staged)" = "$clean" ] || fail "the already-clean scrub changed the index"
[ "$(cat "$srepo/scripts/release/version-test.rs")" = "$clean" ] ||
    fail "the already-clean scrub changed the working tree"

# 6b. The build-marker scan finds job ids, private trailers, pruned paths and
#     build-diary wording, skips pruned paths, and accepts the product trailer.
mrepo=$tmp/marker-repo
mkdir -p "$mrepo/scripts/audit" "$mrepo/docs" "$mrepo/docs/maintainers"
cp "$script_dir/public-tree-markers.sh" "$mrepo/scripts/audit/"
chmod +x "$mrepo/scripts/audit/public-tree-markers.sh"
cat > "$mrepo/scripts/audit/public-tree.txt" <<'EOF'
allow docs
allow scripts
prune scripts/audit
prune docs/maintainers
require docs/keep.md
EOF
printf 'AI-Assisted: <model> via VOWP\nA normal product document.\n' > "$mrepo/docs/keep.md"
printf 'job v2-b1-spec\nAI-Assisted: deepseek/deepseek-flash via vowp-night\nsee docs/maintainers/private.md\nmerged 2026-11-10\n' > "$mrepo/docs/bad.md"
printf 'pruned notes: vowp-night v2-g1-vault scripts/audit/\n' > "$mrepo/scripts/audit/notes.md"
git init -q -b main "$mrepo"
git -C "$mrepo" config user.name "Marker Test"
git -C "$mrepo" config user.email "$(printf 'marker@%s' example.invalid)"
git -C "$mrepo" add -A
git -C "$mrepo" commit -qm "marker fixture"

run_markers() {
    (cd "$mrepo" && scripts/audit/public-tree-markers.sh "$@")
}

marker_summary=$tmp/marker-summary.txt
if run_markers --worktree --summary > "$marker_summary" 2>&1; then
    fail "build markers in a shipping path should fail"
fi
for category in ai_trailer build_diary job_id private_path; do
    grep -q "  $category" "$marker_summary" || fail "marker category not reported: $category"
done
if grep -q 'scripts/audit/notes.md' "$marker_summary"; then
    fail "the marker scan reported a pruned path"
fi
marker_out=$tmp/marker-full.txt
if run_markers --worktree > "$marker_out" 2>&1; then
    fail "the full marker scan should fail too"
fi
grep -q 'docs/bad.md:.*job v2-b1-spec' "$marker_out" || fail "the job id was not reported"
grep -q 'docs/bad.md:.*via vowp-night' "$marker_out" || fail "the private trailer was not reported"
grep -q 'docs/bad.md:.*docs/maintainers/private.md' "$marker_out" || fail "the private path was not reported"
if grep -q 'docs/keep.md' "$marker_out"; then
    fail "the product AI-Assisted trailer was reported as private"
fi
if run_markers --index --summary >/dev/null 2>&1; then
    fail "the staged marker scan should fail"
fi

# A clean revision passes; the script lives outside that fixture, so its own
# pattern text is not part of the scanned tree.
mclean=$tmp/marker-clean
mkdir -p "$mclean/docs"
printf 'AI-Assisted: <model> via VOWP\nA normal product document.\n' > "$mclean/docs/keep.md"
git init -q -b main "$mclean"
git -C "$mclean" config user.name "Marker Test"
git -C "$mclean" config user.email "$(printf 'marker@%s' example.invalid)"
git -C "$mclean" add -A
git -C "$mclean" commit -qm "clean marker fixture"
"$script_dir/public-tree-markers.sh" --root "$mclean" --rev HEAD >/dev/null 2>&1 ||
    fail "a clean revision should pass the marker scan"

# 7. The real manifest carries the owner's prune list and owner exceptions.
for pruned in AGENTS.md docs/dogfood docs/fresh-user docs/maintainers \
    docs/release/0.1.0-rc.1.md scripts/acceptance scripts/audit scripts/dogfood \
    scripts/fresh-user scripts/spikes site; do
    if ! grep -qx "prune $pruned" "$script_dir/public-tree.txt" &&
        ! grep -qx "prune $pruned/" "$script_dir/public-tree.txt"; then
        fail "real manifest does not prune: $pruned"
    fi
done
grep -qx 'require docs/design/job-map.md' "$script_dir/public-tree.txt" ||
    fail "real manifest does not require docs/design/job-map.md"
grep -qx 'require docs/design/v0.2/' "$script_dir/public-tree.txt" ||
    fail "real manifest does not require docs/design/v0.2/"
grep -qx 'require docs/setup/' "$script_dir/public-tree.txt" ||
    fail "real manifest does not require docs/setup/"
grep -qx 'require THIRD-PARTY-LICENSES.md' "$script_dir/public-tree.txt" ||
    fail "real manifest does not require THIRD-PARTY-LICENSES.md"

echo "self-test: PASS: scanner rules, masking, allowlist, history, public-tree prune, build markers and content-scrub idempotence work."
