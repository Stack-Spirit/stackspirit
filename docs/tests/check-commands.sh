#!/usr/bin/env bash
# Check that every `vowp` command shown in the user documentation exists in
# docs/design/cli.md. The design file is the single source of truth for the
# v0.1 command surface; this script never keeps its own command list.
#
# Scope: README.md, docs/concepts/**, docs/guides/**, docs/security.md and
# docs/troubleshooting.md. docs/design/** is the reference, not a consumer.
set -euo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd -- "$here/../.." && pwd)"

if ! command -v python3 >/dev/null 2>&1; then
  printf 'FAIL  python3 is required\n' >&2
  exit 1
fi

python3 - "$root" <<'PY'
import re
import shlex
import sys
from pathlib import Path

root = Path(sys.argv[1])
cli_path = root / "docs" / "design" / "cli.md"

if not cli_path.is_file():
    print(f"FAIL  missing {cli_path}", file=sys.stderr)
    sys.exit(1)

cli = cli_path.read_text(encoding="utf-8")
families = {"job", "session", "inbox", "node", "schedule"}

# --- Parse the command surface out of cli.md -------------------------------
#
# Commands appear in cli.md in three shapes:
#   * a backticked invocation, e.g. `vowp init ...` or `vowp status --check`
#   * a table row whose first cell is only the subcommand, e.g. `job add ...`
#   * a heading with shorthands: `vowp inbox ls ... · show ... · ask ...`
# We derive the set; the documentation side is checked against it below.

known = set()


def add_invocation(text):
    """Add the command path found at the start of a piece of text."""
    match = re.search(r"\bvowp\s+(--version|--help|[a-z_][a-z0-9_-]*)", text)
    if not match:
        return None
    command = match.group(1)
    if command == "--help":
        # clap provides --help, but cli.md documents the command surface, not
        # that flag. Do not treat it as an invocable command.
        return None
    path = command
    rest = text[match.end():].lstrip()
    if command in families:
        sub = re.match(r"([a-z_][a-z0-9_-]*)", rest)
        if sub:
            path = f"{command} {sub.group(1)}"
    known.add(path)
    return path


# Backticked spans (headings, tables, prose).
for span in re.findall(r"`([^`\n]+)`", cli):
    add_invocation(span)
    if "·" in span and "vowp " in span:
        parts = span.split("·")
        family = None
        first = re.search(r"\bvowp\s+([a-z_][a-z0-9_-]*)", parts[0])
        if first:
            family = first.group(1)
        if family in families:
            for part in parts[1:]:
                sub = re.match(r"\s*([a-z_][a-z0-9_-]*)", part)
                if sub:
                    known.add(f"{family} {sub.group(1)}")

# Table rows that name only the subcommand.
for family, sub in re.findall(
    r"`(job|session|inbox|node|schedule)\s+([a-z_][a-z0-9_-]*)", cli
):
    known.add(f"{family} {sub}")

required = {
    "init", "doctor", "up", "down", "status", "board", "plan", "attach",
    "graph", "chat", "cost", "loops", "promote", "rollback", "releases",
    "reindex", "backup", "schema", "version", "--version", "__window",
    "job add", "job update", "job ls", "job show", "job retry", "job park",
    "job nudge", "job restart", "job replan", "job fail", "job cancel",
    "session ls", "session show", "session new", "session chat",
    "session switch", "session capsule", "session export", "session import",
    "session watch", "session tail",
    "inbox ls", "inbox show", "inbox ask", "inbox answer", "inbox dismiss",
    "node add", "node ls", "node rm",
    "schedule add", "schedule ls", "schedule pause", "schedule resume",
    "schedule rm",
}
missing_in_cli = sorted(required - known)
if missing_in_cli:
    print(
        "FAIL  cli.md parser did not find required command(s): "
        + ", ".join(missing_in_cli),
        file=sys.stderr,
    )
    sys.exit(1)

# A family name alone ("vowp job") is a section title, not an invocable
# command. Keep only concrete two-word paths.
for family in families:
    known.discard(family)

# --- Scan the documentation ------------------------------------------------

doc_files = [root / "README.md"]
for directory in ("concepts", "guides"):
    doc_files.extend(sorted((root / "docs" / directory).glob("*.md")))
doc_files.extend([root / "docs" / "security.md", root / "docs" / "troubleshooting.md"])
doc_files = [p for p in doc_files if p.is_file()]


def parse_command(line):
    """Return (path, original line) for a line that invokes vowp, else None."""
    line = line.strip()
    if line.startswith("$ "):
        line = line[2:].strip()
    if not line:
        return None
    # Accept the documented way to run the workspace binary.
    prefix = "cargo run -p vowp-cli --bin vowp -- "
    if line.startswith(prefix):
        line = "vowp " + line[len(prefix):]
    if not re.match(r"^vowp(\s|$)", line):
        return None
    # Drop a trailing shell comment. Do not split on '#' without whitespace:
    # executor variants such as 'opencode:deepseek/deepseek-flash#max' use it.
    line = re.sub(r"\s+#\s.*$", "", line).strip()
    try:
        tokens = shlex.split(line, comments=False, posix=True)
    except ValueError:
        tokens = line.split()
    if not tokens or tokens[0] != "vowp":
        return None
    i = 1
    if i < len(tokens) and tokens[i] == "--version":
        return "--version", line
    global_with_value = {"--node", "--project", "--home"}
    while i < len(tokens) and tokens[i].startswith("-"):
        if tokens[i] in global_with_value:
            i += 2
        else:
            i += 1
    if i >= len(tokens):
        return None
    command = tokens[i]
    if not re.fullmatch(r"--version|[a-z_][a-z0-9_-]*", command):
        # A placeholder or output line, not a command invocation.
        return None
    path = command
    if command in families and i + 1 < len(tokens):
        sub = tokens[i + 1]
        if not re.fullmatch(r"[a-z_][a-z0-9_-]*", sub):
            return None
        if not sub.startswith("-"):
            path = f"{command} {sub}"
    return path, line


references = []
for path in doc_files:
    lines = path.read_text(encoding="utf-8").splitlines()
    in_fence = False
    fence_marker = ""
    for number, line in enumerate(lines, 1):
        stripped = line.strip()
        if stripped.startswith("```") or stripped.startswith("~~~"):
            marker = stripped[:3]
            if not in_fence:
                in_fence = True
                fence_marker = marker
            elif marker == fence_marker:
                in_fence = False
                fence_marker = ""
            continue
        if in_fence:
            parsed = parse_command(line)
            if parsed:
                references.append((path, number, parsed[0], parsed[1]))
        else:
            # Inline code spans on prose lines and tables.
            for span in re.findall(r"`([^`\n]+)`", line):
                parsed = parse_command(span)
                if parsed:
                    references.append((path, number, parsed[0], parsed[1]))

failures = []
for path, number, command, line in references:
    if command not in known:
        rel = path.relative_to(root)
        failures.append(f"{rel}:{number}: `{command}` is not in docs/design/cli.md\n    {line}")

# Until the CLI lands, every page that shows a vowp command must say so. When
# the implementation lands, n12b removes the marks and uses "Status: current"
# (or equivalent) instead; the checker accepts either declaration.
for path in sorted({path for path, _, _, _ in references}):
    text = path.read_text(encoding="utf-8")
    if "*(planned)*" not in text and "status: current" not in text.lower():
        rel = path.relative_to(root)
        failures.append(
            f"{rel}: shows vowp commands but neither marks them *(planned)* "
            "nor declares 'Status: current'"
        )

if failures:
    for failure in failures:
        print(f"FAIL  {failure}", file=sys.stderr)
    print(
        f"FAIL  check-commands: {len(failures)} undocumented command reference(s)",
        file=sys.stderr,
    )
    sys.exit(1)

distinct = sorted({command for _, _, command, _ in references})
print(
    f"PASS  check-commands: {len(references)} references, "
    f"{len(distinct)} distinct commands, all defined in docs/design/cli.md"
)
print("      " + ", ".join(distinct))
PY
