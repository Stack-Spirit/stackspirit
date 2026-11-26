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
#   bash scripts/check-public-readiness.sh --help
#
# Exit status:
#   0  no blocked finding in the tracked working tree
#   1  at least one blocked finding
#   2  usage error, not a git repository, or python3 is unavailable
#
# The scan is a working-tree scan. A repository with public history should also
# be checked with a history-aware scanner before its visibility is changed.

set -euo pipefail

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)

usage() {
  sed -n '2,28p' "$0" | sed 's/^# \{0,1\}//'
}

case "${1:-}" in
  -h|--help)
    usage
    exit 0
    ;;
  "")
    ;;
  *)
    echo "check-public-readiness: unknown argument: $1 (try --help)" >&2
    exit 2
    ;;
esac

command -v python3 >/dev/null 2>&1 || {
  echo "check-public-readiness: python3 is required" >&2
  exit 2
}
git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1 || {
  echo "check-public-readiness: not a git working tree: $root" >&2
  exit 2
}

exec python3 -I - "$root" <<'PY'
import ipaddress
import os
import re
import subprocess
import sys

root = sys.argv[1]
os.chdir(root)

MAX_BYTES = 4 * 1024 * 1024

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
EMAIL_RE = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")

# Exact credential files. A .env.example is documentation, not a secret.
SECRET_NAMES = {".env", ".git-credentials", ".netrc", ".npmrc", ".pypirc"}
SECRET_SUFFIXES = (".pem", ".p12", ".pfx", ".key")
SECRET_PREFIXES = ("id_rsa", "id_dsa", "id_ecdsa", "id_ed25519")


def git(*args):
    result = subprocess.run(
        ["git", *args], check=True, capture_output=True, text=True
    )
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


files = [path for path in git("ls-files").splitlines() if path]
if not files:
    print("public-readiness: FAIL: no tracked files found", file=sys.stderr)
    raise SystemExit(2)

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

    for lineno, line in enumerate(text.splitlines(), 1):
        for rule, regex in TOKEN_RULES:
            for match in regex.finditer(line):
                counts[rule] = counts.get(rule, 0) + 1
                findings.append((path, lineno, rule, mask(match.group(0))))

        for match in HOME_RE.finditer(line):
            family, name = match.group(1), match.group(2)
            if allowed_home(family, name):
                counts["home_path_allowed"] = counts.get("home_path_allowed", 0) + 1
                continue
            counts["home_path"] = counts.get("home_path", 0) + 1
            findings.append((path, lineno, "home_path", mask(match.group(0))))

        for match in IPV4_RE.finditer(line):
            value = match.group(1)
            try:
                address = ipaddress.ip_address(value)
            except ValueError:
                continue
            if not address.is_global:
                counts["ipv4_reserved"] = counts.get("ipv4_reserved", 0) + 1
                continue
            counts["ipv4"] = counts.get("ipv4", 0) + 1
            findings.append((path, lineno, "ipv4", mask(value)))

        for match in EMAIL_RE.finditer(line):
            value = match.group(0)
            if allowed_email(value):
                counts["email_allowed"] = counts.get("email_allowed", 0) + 1
                continue
            counts["email"] = counts.get("email", 0) + 1
            findings.append((path, lineno, "email", mask(value)))

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

order = [
    "private_key", "provider_token", "github_token", "github_pat", "gitlab_token",
    "aws_access_key", "google_api_key", "slack_token", "stripe_live_key",
    "pypi_token", "npm_token", "huggingface_token", "secret_path", "ipv4",
    "home_path", "email",
]
labels = {
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

for rule in order:
    if rule in counts:
        print(f"public-readiness: {labels[rule]}: {counts[rule]} blocked")

if findings:
    print("public-readiness: findings (values masked):")
    for path, lineno, rule, value in findings:
        print(f"  {path}:{lineno}: {rule}: {value}")
    print(f"public-readiness: FAIL: {len(findings)} blocked finding(s)")
    raise SystemExit(1)

print("public-readiness: PASS: no secret, public IP, personal home path, tracked .env or personal e-mail found")
PY
