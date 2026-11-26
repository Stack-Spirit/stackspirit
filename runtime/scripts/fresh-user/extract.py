#!/usr/bin/env python3
"""Build the fresh-user test plan from the user documentation.

The plan is a bash script with one timed step per documented command. Commands
are read from fenced `sh` blocks in:

  * docs/guides/install.md, section "Build and install by hand"
  * docs/guides/quickstart.md, sections 1, 2, 4, 5 and 7
  * examples/hello-vowp/README.md, the main "Run it" command

The setup contract (`docs/setup/agent-setup.md`) does have fenced `sh` blocks,
but that path installs and checks a real home over SSH with a signed-in
harness. The fresh-user container has no SSH client, systemd user manager or
harness login, so the plan follows the manual build and the local quickstart
instead; the setup path and the install prerequisite checks run elsewhere.
Nothing is copied by hand: if a documented command changes, the test runs the
new command. If a section or block disappears, extraction fails loudly so the
two cannot drift apart silently. Commands explicitly marked ``# (planned)``
in the quickstart guide are skipped and logged (they need a planner model
turn).

Usage:
    extract.py --root REPO --out PLAN.sh [--manifest PLAN.json]
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

# Working-directory symbols. The generated script defines the real paths.
REPO = "repo"
QUICKSTART = "quickstart"

# (file, heading without markup, working directory, honour `# (planned)`,
#  optional substring that every selected block must contain)
SELECTIONS: list[tuple[str, str, str, bool, str | None]] = [
    # The manual build installs the binary the rest of the plan uses.
    ("docs/guides/install.md", "Build and install by hand", REPO, False, None),
    # The quickstart numbers its sections, but the plan runs them in dependency
    # order: register the project (section 2) before checking the home
    # (section 1), because doctor and fleet status inspect the registered home.
    ("docs/guides/quickstart.md", "2. Register a project", QUICKSTART, True, "vowp init"),
    ("docs/guides/quickstart.md", "2. Register a project", QUICKSTART, True, "git commit"),
    ("docs/guides/quickstart.md", "1. Check the home", QUICKSTART, True, None),
    ("docs/guides/quickstart.md", "4. See the fleet", QUICKSTART, True, "vowp status"),
    ("docs/guides/quickstart.md", "5. Add a first job", QUICKSTART, True, "vowp job add"),
    ("docs/guides/quickstart.md", "7. Stop cleanly", QUICKSTART, True, None),
    ("examples/hello-vowp/README.md", "Run it", REPO, False, None),
]

COMMAND_LANGUAGES = {"sh", "bash", "shell"}
FENCE_RE = re.compile(r"^(\s*)(`{3,}|~{3,})\s*([^\s`]*)")


@dataclass
class Block:
    """One fenced command block."""

    rel: str
    heading: str
    fence_line: int
    lines: list[tuple[int, str]]

    @property
    def text(self) -> str:
        return "\n".join(text for _, text in self.lines)


@dataclass
class Step:
    """One command extracted from a block."""

    number: int
    rel: str
    start_line: int
    end_line: int
    cwd: str
    text: str
    label: str
    planned: bool
    note: str = ""

    @property
    def doc(self) -> str:
        if self.start_line == self.end_line:
            return f"{self.rel}:{self.start_line}"
        return f"{self.rel}:{self.start_line}-{self.end_line}"


@dataclass
class Plan:
    """Everything the generated script will run or skip."""

    steps: list[Step] = field(default_factory=list)
    skipped: list[dict[str, object]] = field(default_factory=list)


def norm_heading(text: str) -> str:
    """Normalise a heading for matching (drop markup and surrounding space)."""
    return text.replace("`", "").replace("*", "").strip()


def parse_blocks(root: Path, rel: str) -> list[Block]:
    """Return every fenced command block in *rel*, with its section heading."""
    path = root / rel
    if not path.is_file():
        raise SystemExit(f"fresh-user: missing document: {rel}")
    lines = path.read_text(encoding="utf-8").splitlines()
    blocks: list[Block] = []
    heading = ""
    in_fence = False
    marker = ""
    language = ""
    fence_line = 0
    body: list[tuple[int, str]] = []

    for number, raw in enumerate(lines, 1):
        if not in_fence:
            heading_match = re.match(r"^(#{1,6})\s+(.*)$", raw)
            if heading_match:
                if len(heading_match.group(1)) == 2:
                    heading = heading_match.group(2).strip()
                continue
            fence_match = FENCE_RE.match(raw)
            if fence_match:
                in_fence = True
                marker = fence_match.group(2)
                language = fence_match.group(3).lower()
                fence_line = number
                body = []
            continue

        stripped = raw.lstrip()
        if stripped.startswith(marker[0] * len(marker)) and stripped.rstrip() == marker:
            if language in COMMAND_LANGUAGES:
                blocks.append(Block(rel, heading, fence_line, body))
            in_fence = False
            marker = ""
            language = ""
            body = []
            continue
        body.append((number, raw))

    if in_fence:
        raise SystemExit(f"fresh-user: unterminated fence in {rel} at line {fence_line}")
    return blocks


def split_commands(block: Block) -> list[tuple[int, int, str, bool]]:
    """Split a block into shell commands.

    A command is one logical line: backslash continuations are joined, and a
    heredoc (``<<'DELIM'``) is consumed up to its delimiter. The returned tuple
    is ``(start_line, end_line, text, planned)``.
    """
    commands: list[tuple[int, int, str, bool]] = []
    lines = block.lines
    index = 0
    block_planned = any("(planned)" in text.lower() for _, text in lines)

    while index < len(lines):
        number, raw = lines[index]
        stripped = raw.strip()
        if not stripped or stripped.startswith("#"):
            index += 1
            continue

        start = number
        collected: list[str] = []
        while index < len(lines):
            number, raw = lines[index]
            collected.append(raw)
            index += 1
            heredoc = re.search(r"<<-?\s*(['\"]?)([A-Za-z_][A-Za-z0-9_]*)\1", raw)
            if heredoc:
                delimiter = heredoc.group(2)
                while index < len(lines):
                    body_number, body_line = lines[index]
                    collected.append(body_line)
                    index += 1
                    if body_line.strip() == delimiter:
                        break
                break
            if raw.rstrip().endswith("\\"):
                continue
            break

        text = "\n".join(collected)
        commands.append((start, start + len(collected) - 1, text, block_planned))
    return commands


def command_label(text: str) -> str:
    """A short one-line label for logs."""
    first = text.splitlines()[0].rstrip()
    if first.endswith("\\"):
        first = first[:-1].rstrip()
    if len(text.splitlines()) > 1:
        first += " ..."
    return re.sub(r"\s+", " ", first).strip()


def substitute(rel: str, text: str) -> tuple[str, str]:
    """Replace a documented placeholder with the test's concrete value.

    Only the placeholders the selected sections deliberately use are replaced.
    Anything else runs exactly as written.
    """
    if rel == "docs/guides/quickstart.md":
        replacements = {
            "<your-repo>": "$QUICKSTART_PROJECT",
            "<your-project>": "quickstart-project",
        }
    else:
        replacements = {}

    note = ""
    for placeholder, value in replacements.items():
        if placeholder in text:
            text = text.replace(placeholder, value)
            note = f"documented placeholder {placeholder} -> {value}"
    return text, note


def build_plan(root: Path) -> Plan:
    """Read the docs and return the ordered plan."""
    by_key: dict[tuple[str, str], list[Block]] = {}
    all_blocks: dict[str, list[Block]] = {}
    for rel in sorted({selection[0] for selection in SELECTIONS}):
        blocks = parse_blocks(root, rel)
        all_blocks[rel] = blocks
        for block in blocks:
            by_key.setdefault((rel, norm_heading(block.heading)), []).append(block)

    plan = Plan()
    number = 0
    for rel, heading, cwd, honour_planned, match in SELECTIONS:
        blocks = by_key.get((rel, norm_heading(heading)))
        if not blocks:
            known = "\n  ".join(
                f"{block.fence_line}: {block.heading}" for block in all_blocks.get(rel, [])
            )
            raise SystemExit(
                f"fresh-user: no fenced command block under {rel} heading "
                f"{heading!r}; known blocks:\n  {known}"
            )
        if match is not None:
            blocks = [block for block in blocks if match in block.text]
            if not blocks:
                raise SystemExit(
                    f"fresh-user: no block under {rel} heading {heading!r} "
                    f"contains {match!r}"
                )

        if rel == "examples/hello-vowp/README.md":
            # "Run it" also shows a KEEP=1 inspection variant and the commands
            # to inspect its work directory. The main flow is the single block
            # that runs the demo.
            main = [
                block
                for block in blocks
                if any(
                    line.strip() == "FAKE=1 examples/hello-vowp/run.sh"
                    for _, line in block.lines
                )
            ]
            optional = [block for block in blocks if block not in main]
            for block in optional:
                plan.skipped.append(
                    {
                        "doc": f"{rel}:{block.fence_line + 1}",
                        "command": command_label(block.text),
                        "reason": "optional inspection variant, not part of the first run",
                    }
                )
            blocks = main

        for block in blocks:
            for start, end, text, block_planned in split_commands(block):
                text, note = substitute(rel, text)
                planned = honour_planned and block_planned
                if planned:
                    plan.skipped.append(
                        {
                            "doc": f"{rel}:{start}",
                            "command": command_label(text),
                            "reason": "documented as planned",
                        }
                    )
                    continue
                number += 1
                plan.steps.append(
                    Step(
                        number=number,
                        rel=rel,
                        start_line=start,
                        end_line=end,
                        cwd=cwd,
                        text=text,
                        label=command_label(text),
                        planned=planned,
                        note=note,
                    )
                )
    if not plan.steps:
        raise SystemExit("fresh-user: the documentation produced no commands")
    return plan


def shq(value: str) -> str:
    """Quote a string for bash single quotes."""
    return "'" + value.replace("'", "'\\''") + "'"


def emit(plan: Plan, out: Path) -> None:
    """Write the bash plan script."""
    cwd_var = {
        REPO: "FRESH_REPO",
        QUICKSTART: "QUICKSTART_PROJECT",
    }
    lines: list[str] = [
        "#!/usr/bin/env bash",
        "# Generated by scripts/fresh-user/extract.py from the user documentation.",
        "# Do not edit: edit the docs or the extractor, then rerun the test.",
        "set -euo pipefail",
        "",
        'FRESH_REPO=${FRESH_REPO:-/work/vowp}',
        'WORK=${FRESH_WORK:-/work/fresh}',
        'QUICKSTART_PROJECT="$WORK/quickstart/my-project"',
        'LOG_DIR="$WORK/logs"',
        'TIMINGS="$WORK/timings.tsv"',
        'export CARGO_TARGET_DIR="$FRESH_REPO/target"',
        'export TMUX_TMPDIR="$WORK/tmux"',
        'export CARGO_TERM_COLOR=never',
        'export PATH="$HOME/.local/bin:/work/fake-bin:$PATH"',
        'unset TMUX TMUX_PANE 2>/dev/null || true',
        "",
        'mkdir -p "$WORK" "$LOG_DIR" "$TMUX_TMPDIR"',
        ': > "$TIMINGS"',
        "",
        '# A fresh user has a normal git identity; the demo repository it creates',
        '# needs one for its first (empty) commit.',
        'git config --global user.name "Fresh User"',
        'git config --global user.email "fresh@example.invalid"',
        'git config --global init.defaultBranch main',
        "",
        'make_project() {',
        '    project=$1',
        '    origin=$2',
        '    rm -rf "$project" "$origin"',
        '    mkdir -p "$(dirname "$project")"',
        '    git init -q -b main "$project"',
        '    git -C "$project" commit -q --allow-empty -m "initial commit"',
        '    git init -q --bare -b main "$origin"',
        '    git -C "$project" remote add origin "$origin"',
        '    git -C "$project" push -q -u origin main',
        '}',
        "",
        'printf "%s\\n" "[fresh-user] preparing the demo repository under $WORK"',
        'make_project "$QUICKSTART_PROJECT" "$WORK/quickstart/origin.git"',
        "",
        'STEP_INDEX=0',
        'STEP_START_NS=0',
        'CURRENT_DOC=""',
        'CURRENT_LABEL=""',
        '',
        'step_begin() {',
        '    CURRENT_DOC=$1',
        '    CURRENT_LABEL=$2',
        '    STEP_INDEX=$((STEP_INDEX + 1))',
        '    STEP_START_NS=$(date +%s%N)',
        '    printf "\\n[fresh-user] step %02d %s - %s\\n" "$STEP_INDEX" "$CURRENT_DOC" "$CURRENT_LABEL"',
        '}',
        '',
        'step_end() {',
        '    now=$(date +%s%N)',
        '    elapsed=$((now - STEP_START_NS))',
        '    printf "[fresh-user] ok   %02d %6.2fs %s\\n" "$STEP_INDEX" "$(awk -v ns="$elapsed" "BEGIN { print ns / 1000000000 }")" "$CURRENT_DOC"',
        '    printf "%s\\t%s\\t%s\\t%s\\n" "$STEP_INDEX" "$CURRENT_DOC" "$elapsed" "$CURRENT_LABEL" >> "$TIMINGS"',
        '}',
        '',
        'step_fail() {',
        '    status=${1:-1}',
        '    printf "\\n[fresh-user] FAIL %s - %s (exit %s)\\n" "$CURRENT_DOC" "$CURRENT_LABEL" "$status" >&2',
        '    printf "[fresh-user] the failing command came from %s\\n" "$CURRENT_DOC" >&2',
        '    exit "$status"',
        '}',
        '',
        'check_log() {',
        '    step=$1',
        '    pattern=$2',
        '    doc=$3',
        '    what=$4',
        '    log=$(printf "%s/%03d.log" "$LOG_DIR" "$step")',
        '    if ! grep -Eq "$pattern" "$log"; then',
        '        printf "\\n[fresh-user] FAIL documented output missing at %s: %s\\n" "$doc" "$what" >&2',
        '        printf "[fresh-user] expected pattern: %s\\n" "$pattern" >&2',
        '        printf "[fresh-user] actual output:\\n" >&2',
        '        sed "s/^/    /" "$log" >&2 || true',
        '        exit 1',
        '    fi',
        '    printf "[fresh-user] ok   output %s: %s\\n" "$doc" "$what"',
        '}',
        '',
        'cleanup() {',
        '    status=$?',
        '    trap - EXIT INT TERM',
        '    if [ -f "$QUICKSTART_PROJECT/vowp.toml" ] && [ -d "$HOME/.vowp" ]; then',
        '        (cd "$QUICKSTART_PROJECT" && vowp down --force) >/dev/null 2>&1 || true',
        '    fi',
        '    exit "$status"',
        '}',
        'trap cleanup EXIT INT TERM',
        "",
    ]

    first_success: int | None = None
    version_pattern = r"^vowp [0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?( |$)"
    for step in plan.steps:
        variable = cwd_var[step.cwd]
        lines.append(f"# {step.doc}")
        if step.note:
            lines.append(f"# {step.note}")
        lines.append(f"step_begin {shq(step.doc)} {shq(step.label)}")
        lines.append("set +e")
        lines.append("{")
        lines.append(f'    cd "${variable}"')
        for command_line in step.text.splitlines():
            lines.append(command_line)
        lines.append(f'}} 2>&1 | tee "$LOG_DIR/{step.number:03d}.log"')
        lines.append("__fresh_status=${PIPESTATUS[0]}")
        lines.append("set -e")
        lines.append('[ "$__fresh_status" -eq 0 ] || step_fail "$__fresh_status"')
        lines.append("step_end")
        lines.append("")

        command = step.text.strip()
        if (
            first_success is None
            and (
                command == "cargo run -p vowp-cli --bin vowp -- --version"
                or command == "vowp --version"
            )
        ):
            first_success = step.number

        # The docs print (or name) these outputs; check them rather than only
        # trusting the exit code.
        if re.search(r"\bvowp\b.*--version$", command) or command.endswith("-- --version"):
            lines.append(
                f"check_log {step.number} {shq(version_pattern)} "
                f"{shq(step.doc)} 'vowp prints its version'"
            )
            lines.append("")
        if re.match(r"vowp init --name ", command):
            project = command.split("--name", 1)[1].strip().split()[0]
            pattern = (
                rf"^initialised project {re.escape(project)} at .* "
                rf"\([0-9]+ files written, 0 skipped\)$"
            )
            lines.append(
                f"check_log {step.number} {shq(pattern)} "
                f"{shq(step.doc)} 'the documented init summary line'"
            )
            lines.append("")
        if command == "vowp doctor":
            lines.append(
                f"check_log {step.number} {shq(r'^doctor: ok$')} "
                f"{shq(step.doc)} 'doctor reports no failures'"
            )
            lines.append("")
        if command == "FAKE=1 examples/hello-vowp/run.sh":
            lines.append(
                f"check_log {step.number} {shq(r'all three jobs reached done')} "
                f"{shq(step.doc)} 'the hello-vowp example completes all three jobs'"
            )
            lines.append("")

    lines.extend(
        [
            "",
            "# Documented commands that the docs themselves mark as planned.",
        ]
    )
    for skipped in plan.skipped:
        lines.append(
            "printf '[fresh-user] skip %s (%s): %s\\n' "
            f"{shq(str(skipped['doc']))} {shq(str(skipped['reason']))} {shq(str(skipped['command']))}"
        )

    lines.extend(
        [
            "",
            "if [ -n " + shq(str(first_success or "")) + " ]; then",
            '    FIRST_SUCCESS_SECONDS=$(awk -F "\\t" -v n="' + str(first_success or 0) + '" '
            '\'$1 <= n { sum += $3 } END { printf "%d", sum / 1000000000 }\' "$TIMINGS")',
            '    TOTAL_SECONDS=$(awk -F "\\t" \'{ sum += $3 } END { printf "%d", sum / 1000000000 }\' "$TIMINGS")',
            '    printf "\\n[fresh-user] time to first success (vowp --version): %ss\\n" "$FIRST_SUCCESS_SECONDS"',
            '    printf "[fresh-user] total documented-command time: %ss\\n" "$TOTAL_SECONDS"',
            "fi",
            "",
            'printf "\\n[fresh-user] PASS: every selected documented command succeeded\\n"',
            "",
        ]
    )

    out.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path, help="repository root")
    parser.add_argument("--out", required=True, type=Path, help="bash plan to write")
    parser.add_argument("--manifest", type=Path, help="optional JSON manifest")
    args = parser.parse_args()

    plan = build_plan(args.root.resolve())
    emit(plan, args.out)
    if args.manifest:
        manifest = {
            "steps": [
                {
                    "number": step.number,
                    "doc": step.doc,
                    "cwd": step.cwd,
                    "label": step.label,
                }
                for step in plan.steps
            ],
            "skipped": plan.skipped,
        }
        args.manifest.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
