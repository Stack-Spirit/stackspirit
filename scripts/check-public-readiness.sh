#!/usr/bin/env bash
# Public-readiness scan for the StackSpirit monorepo.
#
# Scans every tracked file in the working tree, with no network access, for:
#   - secret-shaped values (provider tokens, private-key headers, registry tokens),
#   - globally routable IPv4 addresses (loopback, private, link-local, CGNAT,
#     multicast and the RFC 5737 documentation ranges are not public endpoints),
#   - personal /home/<user> and /Users/<user> paths outside the documented
#     placeholder and service accounts,
#   - tracked .env files and private-key file names,
#   - personal e-mail addresses outside the reserved example domains.
#
# Intentional test fixtures and documented placeholders are allowed by the
# embedded lists below. The scan never prints a matched value in full: it shows
# the first four characters and masks the rest, so a report can be committed
# without copying the value it is reporting.
#
# Usage:
#   bash scripts/check-public-readiness.sh
#   bash scripts/check-public-readiness.sh --history
#   bash scripts/check-public-readiness.sh --history --strict-history
#   bash scripts/check-public-readiness.sh --rev <commit>
#   bash scripts/check-public-readiness.sh --help
#
# The default mode scans `git ls-files` in the working tree at HEAD. --history
# walks every commit reachable from `git rev-list --all`, deduplicates blobs by
# object id, and applies the same rules and size/binary limits. Secret-class
# findings are always fatal in history mode; privacy-class findings (home
# paths, public IPv4, personal e-mail) are reported with counts but exit 0
# unless --strict-history is also passed. --strict-history implies --history.
#
# --rev <commit> scans the tree of one commit, not the working tree. Every
# finding class is fatal there, because that tree is what a public branch would
# publish. --rev is mutually exclusive with --history and --strict-history.
#
# Exit status:
#   0  no blocked finding in the selected scope
#   1  at least one blocked finding in the selected scope
#   2  usage error, not a git repository, or python3 is unavailable
#
# A repository with public history should be checked with --history before its
# visibility is changed. A candidate public commit should be checked with
# --rev before it is pushed.

set -euo pipefail

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)

usage() {
  sed -n '2,/^set -euo pipefail$/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
}

history=0
strict_history=0
rev=

while [ "$#" -gt 0 ]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    --history)
      history=1
      ;;
    --strict-history)
      history=1
      strict_history=1
      ;;
    --rev)
      [ "$#" -ge 2 ] || {
        echo "check-public-readiness: --rev requires a commit" >&2
        exit 2
      }
      rev=$2
      shift
      ;;
    --rev=*)
      rev=${1#*=}
      ;;
    *)
      echo "check-public-readiness: unknown argument: $1 (try --help)" >&2
      exit 2
      ;;
  esac
  shift
done

if [ -n "$rev" ] && [ "$history" -eq 1 ]; then
  echo "check-public-readiness: --rev cannot be combined with --history or --strict-history" >&2
  exit 2
fi
if [ -n "$rev" ]; then
  if ! rev_commit=$(git -C "$root" rev-parse --verify --quiet "$rev^{commit}" 2>/dev/null); then
    echo "check-public-readiness: not a commit: $rev" >&2
    exit 2
  fi
  rev=$rev_commit
fi

command -v python3 >/dev/null 2>&1 || {
  echo "check-public-readiness: python3 is required" >&2
  exit 2
}
git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1 || {
  echo "check-public-readiness: not a git working tree: $root" >&2
  exit 2
}

mode=worktree
[ "$history" -eq 1 ] && mode=history
[ -n "$rev" ] && mode=rev

exec python3 -I - "$root" "$mode" "$strict_history" "$rev" <<'PY'
import ipaddress
import os
import re
import subprocess
import sys
import time

root = sys.argv[1]
mode = sys.argv[2] if len(sys.argv) > 2 else "worktree"
strict_history = len(sys.argv) > 3 and sys.argv[3] == "1"
rev = sys.argv[4] if len(sys.argv) > 4 else ""
os.chdir(root)

MAX_BYTES = 4 * 1024 * 1024
MAX_SECRET_ROWS = 500
MAX_PRIVACY_ROWS = 200

# Documented placeholder and service accounts. None of these names identifies a
# person. Keep this list narrow: an unrecognised home path is reported.
ALLOWED_HOME_SEGMENTS = {
    "alice": "placeholder test user",
    "acceptance-release": "acceptance fixture account",
    "control": "acceptance control-home placeholder",
    "demo": "placeholder test user",
    "example": "placeholder example user",
    "fake": "placeholder fake test user",
    "me": "conventional placeholder",
    "o": "short placeholder used by path-scrubbing tests",
    "owner": "conventional placeholder in UI mocks and tests",
    "phone": "acceptance fixture account",
    "projects": "documented relative home directory",
    "stackspirit": "documented StackSpirit service account",
    "test": "placeholder test user",
    "user": "conventional placeholder",
    "vowp": "documented service account",
    "vowp-vault": "documented vault helper account",
    "you": "conventional placeholder in documentation",
}

# File-like names after /home/ (for example <dest>/home/vowp.db) are relative
# paths in a fixture, not accounts. The scanner only treats /home/ as absolute
# when it is not preceded by another path character; this suffix check handles
# the remaining angle-bracket forms.
FILE_SUFFIXES = {
    ".db", ".json", ".jsonl", ".lock", ".log", ".md", ".pid", ".sock",
    ".toml", ".txt", ".yaml", ".yml",
}

ALLOWED_EMAIL_DOMAINS = {
    "example.com",
    "example.invalid",
    "example.net",
    "example.org",
    "example.test",
    "home.example",
    "openssh.com",
    "stackspirit.io",
    "web.test",
}

TOKEN_RULES = [
    ("private_key", re.compile(r"-----BEGIN (?:[A-Z0-9]+ )?PRIVATE KEY-----")),
    ("provider_token", re.compile(r"\bsk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}\b")),
    ("github_token", re.compile(r"\bgh[pousr]_[A-Za-z0-9]{20,}\b")),
    ("github_pat", re.compile(r"\bgithub_pat_[A-Za-z0-9_]{20,}\b")),
    ("gitlab_token", re.compile(r"\bglpat-[A-Za-z0-9_-]{20,}\b")),
    ("aws_access_key", re.compile(r"\bAKIA[0-9A-Z]{16}\b")),
    ("google_api_key", re.compile(r"\bAIza[0-9A-Za-z_-]{35}\b")),
    ("slack_token", re.compile(r"\bxox[abposr]-[A-Za-z0-9-]{10,}\b")),
    ("stripe_live_key", re.compile(r"\b(?:sk|rk)_live_[0-9A-Za-z]{16,}\b")),
    ("pypi_token", re.compile(r"\bpypi-AgEIcHlwaS5vcmc[A-Za-z0-9_-]*")),
    ("npm_token", re.compile(r"\bnpm_[A-Za-z0-9]{36}\b")),
    ("huggingface_token", re.compile(r"\bhf_[A-Za-z0-9]{34}\b")),
]

HOME_RE = re.compile(r"(?<![\w./>-])/(home|Users)/([A-Za-z0-9._-]+)")
IPV4_RE = re.compile(
    r"(?<![\d.])((?:25[0-5]|2[0-4]\d|1?\d?\d)"
    r"(?:\.(?:25[0-5]|2[0-4]\d|1?\d?\d)){3})(?![\d.])"
)
# Scalable Vector Graphics path data is a string of decimal coordinates such
# as `2.94.86-.67.35`; without this exclusion the IPv4 rule reports the
# coordinates as addresses. Only the `d` and `points` attribute values are
# skipped, so an address elsewhere in an SVG or HTML line is still reported.
SVG_PATH_ATTRIBUTE_RE = re.compile(r'\b(?:d|points)="([^"]*)"')
EMAIL_RE = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")

# Exact credential files. A .env.example is documentation, not a secret.
SECRET_NAMES = {".env", ".git-credentials", ".netrc", ".npmrc", ".pypirc"}
SECRET_SUFFIXES = (".pem", ".p12", ".pfx", ".key")
SECRET_PREFIXES = ("id_rsa", "id_dsa", "id_ecdsa", "id_ed25519")

SECRET_RULES = {
    "private_key", "provider_token", "github_token", "github_pat", "gitlab_token",
    "aws_access_key", "google_api_key", "slack_token", "stripe_live_key",
    "pypi_token", "npm_token", "huggingface_token", "secret_path",
}
PRIVACY_RULES = {"home_path", "ipv4", "email"}

ORDER = [
    "private_key", "provider_token", "github_token", "github_pat", "gitlab_token",
    "aws_access_key", "google_api_key", "slack_token", "stripe_live_key",
    "pypi_token", "npm_token", "huggingface_token", "secret_path", "ipv4",
    "home_path", "email",
]
LABELS = {
    "private_key": "private-key header",
    "provider_token": "provider API token",
    "github_token": "GitHub token",
    "github_pat": "GitHub fine-grained token",
    "gitlab_token": "GitLab token",
    "aws_access_key": "AWS access key",
    "google_api_key": "Google API key",
    "slack_token": "Slack token",
    "stripe_live_key": "Stripe live key",
    "pypi_token": "PyPI token",
    "npm_token": "npm token",
    "huggingface_token": "Hugging Face token",
    "secret_path": "tracked credential file",
    "ipv4": "public IPv4 address",
    "home_path": "personal /home or /Users path",
    "email": "personal e-mail address",
}


def git(*args):
    result = subprocess.run(
        ["git", *args], check=True, capture_output=True, text=True
    )
    return result.stdout


def git_bytes(*args):
    result = subprocess.run(["git", *args], check=True, capture_output=True)
    return result.stdout


def mask(value):
    if len(value) <= 4:
        return value[:1] + "***"
    return value[:4] + "***"


def allowed_home(family, name):
    if name in ALLOWED_HOME_SEGMENTS:
        return True
    suffix = os.path.splitext(name)[1].lower()
    if suffix in FILE_SUFFIXES:
        return True
    return False


def allowed_email(value):
    lower = value.lower()
    local, domain = lower.rsplit("@", 1)
    if lower == "git@github.com":
        return True
    if lower.endswith("@users.noreply.github.com"):
        return True
    # File names such as 128x128@2x.png and systemd units such as
    # user@1001.service are not addresses.
    if re.fullmatch(r"\d+x\d+", local):
        return True
    if domain.endswith(
        (".png", ".ico", ".icns", ".json", ".py", ".rs", ".sh", ".md",
         ".toml", ".txt", ".lock", ".js", ".ts", ".tsx", ".service")
    ):
        return True
    return (
        domain in ALLOWED_EMAIL_DOMAINS
        or domain.endswith(".stackspirit.io")
        or domain.endswith(".invalid")
        or domain.endswith(".example")
        or domain.endswith(".test")
    )


def is_secret_path(path):
    base = os.path.basename(path)
    if base in SECRET_NAMES:
        return True
    if base.startswith(".env.") and base not in {".env.example", ".env.sample", ".env.template"}:
        return True
    if base.endswith(SECRET_SUFFIXES):
        return True
    if base.startswith(SECRET_PREFIXES):
        return True
    return "/.ssh/" in path or path.startswith(".ssh/")


def read_text(path):
    try:
        with open(path, "rb") as handle:
            data = handle.read(MAX_BYTES + 1)
    except OSError:
        return None
    if len(data) > MAX_BYTES:
        return None
    if b"\0" in data[:8192]:
        return None
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return None


def decode_blob(data):
    if len(data) > MAX_BYTES:
        return None
    if b"\0" in data[:8192]:
        return None
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return None


def scan_text(text, counts, emit):
    for lineno, line in enumerate(text.splitlines(), 1):
        for rule, regex in TOKEN_RULES:
            for match in regex.finditer(line):
                counts[rule] = counts.get(rule, 0) + 1
                emit(lineno, rule, mask(match.group(0)))

        for match in HOME_RE.finditer(line):
            family, name = match.group(1), match.group(2)
            if allowed_home(family, name):
                counts["home_path_allowed"] = counts.get("home_path_allowed", 0) + 1
                continue
            counts["home_path"] = counts.get("home_path", 0) + 1
            emit(lineno, "home_path", mask(match.group(0)))

        svg_spans = [
            (span.start(1), span.end(1))
            for span in SVG_PATH_ATTRIBUTE_RE.finditer(line)
        ]
        for match in IPV4_RE.finditer(line):
            value = match.group(1)
            if any(start <= match.start() < end for start, end in svg_spans):
                continue
            try:
                address = ipaddress.ip_address(value)
            except ValueError:
                continue
            if not address.is_global:
                counts["ipv4_reserved"] = counts.get("ipv4_reserved", 0) + 1
                continue
            counts["ipv4"] = counts.get("ipv4", 0) + 1
            emit(lineno, "ipv4", mask(value))

        for match in EMAIL_RE.finditer(line):
            value = match.group(0)
            if allowed_email(value):
                counts["email_allowed"] = counts.get("email_allowed", 0) + 1
                continue
            counts["email"] = counts.get("email", 0) + 1
            emit(lineno, "email", mask(value))


def run_worktree():
    files = [path for path in git("ls-files").splitlines() if path]
    if not files:
        print("public-readiness: FAIL: no tracked files found", file=sys.stderr)
        return 2

    counts = {}
    findings = []
    scanned = 0

    for path in files:
        # Never open a tracked credential file: its path is enough to report it.
        if is_secret_path(path):
            counts["secret_path"] = counts.get("secret_path", 0) + 1
            findings.append((path, 0, "secret_path", path))
            continue

        text = read_text(path)
        if text is None:
            continue
        scanned += 1

        def emit(lineno, rule, value, path=path):
            findings.append((path, lineno, rule, value))

        scan_text(text, counts, emit)

    # The scan file itself must not hide findings behind duplicate matches.
    findings.sort(key=lambda item: (item[0], item[1], item[2], item[3]))

    print("public-readiness: repository <git top level>")
    print("public-readiness: scope: tracked working tree at HEAD")
    print(f"public-readiness: scanned {scanned} of {len(files)} tracked text files")
    print(
        "public-readiness: allowed placeholders: "
        f"{counts.get('home_path_allowed', 0)} home path(s), "
        f"{counts.get('ipv4_reserved', 0)} reserved/private IP(s), "
        f"{counts.get('email_allowed', 0)} reserved e-mail(s)"
    )

    for rule in ORDER:
        if rule in counts:
            print(f"public-readiness: {LABELS[rule]}: {counts[rule]} blocked")

    if findings:
        print("public-readiness: findings (values masked):")
        for path, lineno, rule, value in findings:
            print(f"  {path}:{lineno}: {rule}: {value}")
        print(f"public-readiness: FAIL: {len(findings)} blocked finding(s)")
        return 1

    print("public-readiness: PASS: no secret, public IP, personal home path, tracked .env or personal e-mail found")
    return 0


def run_rev(commit):
    full = git("rev-parse", "--verify", commit + "^{commit}").strip()
    tree_id = git("rev-parse", "--verify", full + "^{tree}").strip()

    counts = {}
    findings = []
    entries = 0
    blobs = 0
    scanned = 0
    skipped_large = 0
    skipped_binary = 0
    unreadable = 0

    try:
        tree = git_bytes("ls-tree", "-r", "-z", "-l", full)
    except subprocess.CalledProcessError:
        print(
            f"public-readiness: FAIL: cannot read the tree of {full}",
            file=sys.stderr,
        )
        return 2

    for record_bytes in tree.split(b"\0"):
        if not record_bytes:
            continue
        try:
            meta, path_bytes = record_bytes.split(b"\t", 1)
            file_mode, object_type, object_id, size_text = meta.split()
        except ValueError:
            continue
        entries += 1
        if object_type != b"blob":
            continue
        blobs += 1

        path = path_bytes.decode("utf-8", "surrogateescape")
        oid = object_id.decode("ascii", "replace")

        # Credential paths are findings from the path alone; never open them.
        if is_secret_path(path):
            counts["secret_path"] = counts.get("secret_path", 0) + 1
            findings.append((path, 0, "secret_path", path))
            continue

        try:
            size = int(size_text)
        except ValueError:
            skipped_binary += 1
            continue
        if size > MAX_BYTES:
            skipped_large += 1
            continue

        try:
            data = git_bytes("cat-file", "blob", oid)
        except subprocess.CalledProcessError:
            unreadable += 1
            continue

        text = decode_blob(data)
        if text is None:
            skipped_binary += 1
            continue
        scanned += 1

        def emit(lineno, rule, value, path=path):
            findings.append((path, lineno, rule, value))

        scan_text(text, counts, emit)

    findings.sort(key=lambda item: (item[0], item[1], item[2], item[3]))

    print("public-readiness: repository <git top level>")
    print("public-readiness: scope: tree of one commit")
    print(f"public-readiness: revision: {full}")
    print(f"public-readiness: tree: {tree_id} ({entries} entries, {blobs} blobs)")
    print(
        f"public-readiness: scanned {scanned} of {blobs} blobs in "
        f"{full[:12]} ({skipped_large} over size cap, "
        f"{skipped_binary} binary/undecodable, {unreadable} unreadable)"
    )
    print(
        "public-readiness: allowed placeholders: "
        f"{counts.get('home_path_allowed', 0)} home path(s), "
        f"{counts.get('ipv4_reserved', 0)} reserved/private IP(s), "
        f"{counts.get('email_allowed', 0)} reserved e-mail(s)"
    )

    for rule in ORDER:
        if rule in counts:
            print(f"public-readiness: {LABELS[rule]}: {counts[rule]} blocked")

    if findings:
        print("public-readiness: findings (values masked):")
        for path, lineno, rule, value in findings:
            print(f"  {path}:{lineno}: {rule}: {value}")
        print(f"public-readiness: FAIL: {len(findings)} blocked finding(s)")
        return 1

    print(
        "public-readiness: PASS: no secret, public IP, personal home path, "
        "tracked .env or personal e-mail found in the commit tree"
    )
    return 0


def run_history():
    commit_rows = []
    for line in git("rev-list", "--all", "--timestamp").splitlines():
        parts = line.split()
        if len(parts) != 2:
            continue
        try:
            commit_rows.append((int(parts[0]), parts[1]))
        except ValueError:
            continue
    # Oldest first so the first occurrence of a blob is its earliest commit.
    commit_rows.sort(key=lambda item: (item[0], item[1]))

    counts = {}
    secret_rows = []
    privacy_rows = []
    secret_total = 0
    privacy_total = 0
    secret_path_seen = set()
    content_done = set()
    all_blobs = set()
    text_scanned = 0
    skipped_large = 0
    skipped_binary = 0
    missing_blobs = 0
    earliest_secret = {}

    def record(commit_ts, commit, path, lineno, rule, value):
        nonlocal secret_total, privacy_total
        row = (commit_ts, commit, path, lineno, rule, value)
        if rule in SECRET_RULES:
            secret_total += 1
            if rule not in earliest_secret:
                earliest_secret[rule] = row
            if len(secret_rows) < MAX_SECRET_ROWS:
                secret_rows.append(row)
        elif rule in PRIVACY_RULES:
            privacy_total += 1
            if len(privacy_rows) < MAX_PRIVACY_ROWS:
                privacy_rows.append(row)

    for commit_ts, commit in commit_rows:
        try:
            tree = git_bytes("ls-tree", "-r", "-z", "-l", commit)
        except subprocess.CalledProcessError:
            print(
                f"public-readiness: history: warning: cannot read tree for {commit}",
                file=sys.stderr,
            )
            continue

        for record_bytes in tree.split(b"\0"):
            if not record_bytes:
                continue
            try:
                meta, path_bytes = record_bytes.split(b"\t", 1)
                file_mode, object_type, object_id, size_text = meta.split()
            except ValueError:
                continue
            if object_type != b"blob":
                continue

            path = path_bytes.decode("utf-8", "surrogateescape")
            oid = object_id.decode("ascii", "replace")
            all_blobs.add(oid)

            # Credential paths are findings from the path alone; never open them.
            if is_secret_path(path):
                key = (path, oid)
                if key not in secret_path_seen:
                    secret_path_seen.add(key)
                    counts["secret_path"] = counts.get("secret_path", 0) + 1
                    record(commit_ts, commit, path, 0, "secret_path", path)
                continue

            if oid in content_done:
                continue
            content_done.add(oid)

            try:
                size = int(size_text)
            except ValueError:
                skipped_binary += 1
                continue
            if size > MAX_BYTES:
                skipped_large += 1
                continue

            try:
                data = git_bytes("cat-file", "blob", oid)
            except subprocess.CalledProcessError:
                # The object may have disappeared mid-scan; never fail on it.
                missing_blobs += 1
                continue
            if data is None:
                missing_blobs += 1
                continue

            text = decode_blob(data)
            if text is None:
                skipped_binary += 1
                continue
            text_scanned += 1

            def emit(lineno, rule, value, commit_ts=commit_ts, commit=commit, path=path):
                record(commit_ts, commit, path, lineno, rule, value)

            scan_text(text, counts, emit)

    secret_rows.sort(key=lambda item: (item[0], item[2], item[3], item[4], item[5]))
    privacy_rows.sort(key=lambda item: (item[0], item[2], item[3], item[4], item[5]))

    print("public-readiness: repository <git top level>")
    print("public-readiness: scope: all commits reachable from git rev-list --all")
    print(f"public-readiness: history: commits walked: {len(commit_rows)}")
    print(
        "public-readiness: history: unique blobs: "
        f"{len(all_blobs)} ({text_scanned} text scanned, "
        f"{skipped_large} over size cap, {skipped_binary} binary/undecodable, "
        f"{missing_blobs} disappeared or unreadable)"
    )
    print(
        "public-readiness: history: allowed placeholders: "
        f"{counts.get('home_path_allowed', 0)} home path(s), "
        f"{counts.get('ipv4_reserved', 0)} reserved/private IP(s), "
        f"{counts.get('email_allowed', 0)} reserved e-mail(s)"
    )
    print(f"public-readiness: history: secret-class findings: {secret_total}")
    print(f"public-readiness: history: privacy-class findings: {privacy_total}")
    print(
        "public-readiness: history: rule counts: "
        + " ".join(f"{rule}={counts.get(rule, 0)}" for rule in ORDER)
    )

    if earliest_secret:
        print("public-readiness: history: earliest commit per secret class:")
        for rule in ORDER:
            if rule in earliest_secret:
                commit_ts, commit, path, lineno, _, _ = earliest_secret[rule]
                when = time.strftime("%Y-%m-%d", time.gmtime(commit_ts))
                print(
                    f"  {rule}: {commit[:12]} ({when}, {path}:{lineno})"
                )
    else:
        print("public-readiness: history: earliest secret-class finding: none")

    if secret_rows:
        print("public-readiness: history: secret-class findings (values masked):")
        for commit_ts, commit, path, lineno, rule, value in secret_rows:
            print(f"  {commit[:12]} {path}:{lineno}: {rule}: {value}")
        if secret_total > len(secret_rows):
            print(
                "  ... "
                f"{secret_total - len(secret_rows)} more secret-class row(s) not shown"
            )

    if privacy_rows:
        print(
            "public-readiness: history: privacy-class findings "
            f"(values masked, showing {len(privacy_rows)} of {privacy_total}):"
        )
        for commit_ts, commit, path, lineno, rule, value in privacy_rows:
            print(f"  {commit[:12]} {path}:{lineno}: {rule}: {value}")
        if privacy_total > len(privacy_rows):
            print(
                "  ... "
                f"{privacy_total - len(privacy_rows)} more privacy-class row(s) not shown"
            )

    if secret_total:
        print(
            "public-readiness: history: FAIL: secret-class finding(s); "
            "do not publish this history"
        )
        exit_code = 1
    elif strict_history and privacy_total:
        print(
            "public-readiness: history: FAIL: privacy-class finding(s) "
            "with --strict-history"
        )
        exit_code = 1
    elif privacy_total:
        print(
            "public-readiness: history: PASS: no secret-class finding; "
            f"{privacy_total} privacy-class finding(s) reported but not fatal "
            "(use --strict-history to make them fatal)"
        )
        exit_code = 0
    else:
        print("public-readiness: history: PASS: no secret, public IP, personal home path, tracked .env or personal e-mail found in history")
        exit_code = 0

    print(f"public-readiness: history: exit status: {exit_code}")
    return exit_code


if mode == "history":
    raise SystemExit(run_history())
if mode == "rev":
    raise SystemExit(run_rev(rev))
raise SystemExit(run_worktree())
PY
