#!/usr/bin/env python3
"""Audit the licence of VOWP and every dependency reported by cargo metadata.

Reads `cargo metadata --format-version 1 --locked` from the repository root.
The default output is a human-readable table. `--markdown` prints a Markdown
table and `--third-party` prints the attribution list used for binary releases.

Exit status: 0 when every licence is allowed and the project licence checks
pass; 1 when anything needs review; 2 when cargo metadata cannot be read.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

# The licences the public-audit brief allows. An expression passes when at
# least one branch of every OR is allowed (for example Apache-2.0 OR BSL-1.0).
ALLOWED = {
    "MIT",
    "MIT-0",
    "Apache-2.0",
    "ISC",
    "Zlib",
}


def allowed_identifier(name: str) -> bool:
    if name in ALLOWED:
        return True
    if name.startswith("BSD-"):
        return True
    if name.startswith("Unicode-"):
        return True
    return False


def tokenize(expr: str) -> list[str]:
    # Old-style Cargo expressions use `/` for a choice.
    expr = expr.replace("/", " OR ")
    return re.findall(r"\(|\)|\bAND\b|\bOR\b|\bWITH\b|[A-Za-z0-9.+-]+", expr)


class Parser:
    def __init__(self, tokens: list[str]) -> None:
        self.tokens = tokens
        self.pos = 0

    def peek(self) -> str | None:
        return self.tokens[self.pos] if self.pos < len(self.tokens) else None

    def take(self) -> str:
        token = self.tokens[self.pos]
        self.pos += 1
        return token

    def parse_or(self) -> bool:
        value = self.parse_and()
        while self.peek() == "OR":
            self.take()
            value = self.parse_and() or value
        return value

    def parse_and(self) -> bool:
        value = self.parse_with()
        while self.peek() == "AND":
            self.take()
            value = self.parse_with() and value
        return value

    def parse_with(self) -> bool:
        value = self.parse_primary()
        if self.peek() == "WITH":
            self.take()
            if self.peek() is not None:
                self.take()  # an exception such as LLVM-exception
        return value

    def parse_primary(self) -> bool:
        token = self.peek()
        if token is None:
            return False
        if token == "(":
            self.take()
            value = self.parse_or()
            if self.peek() == ")":
                self.take()
            return value
        self.take()
        return allowed_identifier(token)


def expression_allowed(expr: str | None) -> bool:
    if not expr:
        return False
    tokens = tokenize(expr)
    if not tokens:
        return False
    return Parser(tokens).parse_or()


def run_metadata(root: Path) -> dict:
    command = [
        "cargo",
        "metadata",
        "--format-version",
        "1",
        "--locked",
    ]
    result = subprocess.run(
        command,
        cwd=root,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        raise SystemExit(2)
    return json.loads(result.stdout)


def project_checks(root: Path, packages: list[dict]) -> tuple[list[str], dict]:
    errors: list[str] = []
    info: dict[str, str] = {}

    license_path = root / "LICENSE"
    if not license_path.is_file():
        errors.append("LICENSE is missing")
    else:
        text = license_path.read_text(encoding="utf-8")
        lines = text.splitlines()
        first = lines[0].strip() if lines else ""
        info["license_header"] = first
        if first != "MIT License":
            errors.append(f"LICENSE first line is {first!r}, expected 'MIT License'")
        copyright_line = ""
        for line in lines[:10]:
            if line.strip().startswith("Copyright (c)"):
                copyright_line = line.strip()
                break
        if not copyright_line:
            errors.append("LICENSE has no 'Copyright (c) <year> <owner>' line")
        else:
            match = re.match(r"^Copyright \(c\) (\d{4}) (.+)$", copyright_line)
            if not match:
                errors.append(f"LICENSE copyright line is malformed: {copyright_line!r}")
            else:
                info["copyright"] = copyright_line
                if int(match.group(1)) < 2020:
                    errors.append(f"LICENSE copyright year looks stale: {match.group(1)}")

    workspace_packages = [p for p in packages if p.get("source") is None]
    for package in workspace_packages:
        name = package.get("name", "?")
        license_expr = package.get("license")
        if license_expr != "MIT":
            errors.append(f"{name} has license {license_expr!r}, expected 'MIT'")
    info["workspace_crates"] = str(len(workspace_packages))
    return errors, info


def dependency_rows(packages: list[dict]) -> list[dict]:
    rows = []
    for package in packages:
        if package.get("source") is None:
            continue
        rows.append(
            {
                "name": package.get("name", "?"),
                "version": package.get("version", "?"),
                "license": package.get("license") or "",
                "license_file": package.get("license_file") or "",
                "allowed": expression_allowed(package.get("license")),
                "repository": package.get("repository") or "",
            }
        )
    rows.sort(key=lambda row: (row["name"].lower(), row["version"]))
    return rows


def print_human(rows: list[dict], errors: list[str], info: dict) -> None:
    print("Project licence")
    print(f"  LICENSE: {info.get('license_header', 'missing')}")
    print(f"  {info.get('copyright', 'copyright line missing')}")
    print(f"  workspace crates with license = MIT: {info.get('workspace_crates', '0')}")
    print()
    print("Dependencies")
    name_width = max([len("Package")] + [len(row["name"]) for row in rows])
    print(f"{'Package':<{name_width}}  {'Version':<14}  {'Licence':<45}  Status")
    print(f"{'-' * name_width}  {'-' * 14}  {'-' * 45}  ------")
    for row in rows:
        license_expr = row["license"] or "(no license field)"
        print(
            f"{row['name']:<{name_width}}  {row['version']:<14}  "
            f"{license_expr:<45}  {'ok' if row['allowed'] else 'FLAG'}"
        )
    print()
    total = len(rows)
    flagged = sum(1 for row in rows if not row["allowed"])
    print(f"Summary: {total} third-party package(s), {flagged} flagged")
    if errors:
        print()
        print("Project licence problems:")
        for error in errors:
            print(f"  - {error}")
    print()
    print(
        "Attribution: required for binary distributions (MIT, Apache-2.0, BSD, "
        "ISC, Unicode and Zlib all require notices); not required for the source "
        "repository itself, where Cargo packages carry their own licence files."
    )


def print_markdown(rows: list[dict], errors: list[str], info: dict) -> None:
    print("| Package | Version | Licence | Status |")
    print("|---|---|---|---|")
    for row in rows:
        license_expr = (row["license"] or "(no license field)").replace("|", "\\|")
        print(
            f"| `{row['name']}` | {row['version']} | {license_expr} | "
            f"{'ok' if row['allowed'] else '**FLAG**'} |"
        )
    print()
    print(f"Project licence: {info.get('license_header', 'missing')}")
    print()
    print(info.get("copyright", "copyright line missing"))
    print()
    flagged = sum(1 for row in rows if not row["allowed"])
    print(f"Summary: {len(rows)} third-party package(s), {flagged} flagged.")
    for error in errors:
        print(f"- **Project licence problem:** {error}")


def print_third_party(rows: list[dict]) -> None:
    print("# Third-party licences")
    print()
    print(
        "VOWP release binaries include the Rust crates below. Each crate is "
        "distributed under one of the licences named in its row; the full "
        "licence text ships with the crate source and must be included with a "
        "binary distribution. This list is generated from "
        "`cargo metadata --format-version 1 --locked`."
    )
    print()
    print("| Package | Version | Licence | Repository |")
    print("|---|---|---|---|")
    for row in rows:
        license_expr = (row["license"] or "(no license field)").replace("|", "\\|")
        repository = row["repository"].replace("|", "\\|") if row["repository"] else ""
        print(f"| `{row['name']}` | {row['version']} | {license_expr} | {repository} |")
    print()
    print(
        "The project itself is MIT. This file is not a substitute for the "
        "licence texts: release packaging must include the licence files from "
        "each vendored crate, for example with `cargo about` or a vendored "
        "source bundle."
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--markdown", action="store_true", help="print a Markdown table")
    parser.add_argument(
        "--third-party",
        action="store_true",
        help="print the third-party attribution list for release binaries",
    )
    parser.add_argument(
        "--root",
        default=".",
        help="repository root (default: current directory)",
    )
    args = parser.parse_args()

    root = Path(args.root).resolve()
    metadata = run_metadata(root)
    packages = metadata.get("packages", [])
    errors, info = project_checks(root, packages)
    rows = dependency_rows(packages)

    if args.third_party:
        print_third_party(rows)
    elif args.markdown:
        print_markdown(rows, errors, info)
    else:
        print_human(rows, errors, info)

    flagged = [row for row in rows if not row["allowed"]]
    if flagged or errors:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
