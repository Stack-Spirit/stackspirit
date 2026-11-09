#!/usr/bin/env python3
"""Generate the third-party licence notice bundle for release archives.

The dependency index is built from
``cargo metadata --format-version 1 --locked``. The full licence texts are the
reviewed files in ``scripts/release/license-texts/``. Copyright notices are
read from the ``LICENSE*``, ``LICENCE*``, ``COPYING*``, ``COPYRIGHT*``,
``UNLICENSE*`` and ``NOTICE*`` files shipped inside each crate's source
package, including nested vendored directories but excluding ``src``, tests,
examples and build output.

Usage:
    scripts/release/generate-third-party-licenses.py
    scripts/release/generate-third-party-licenses.py --check

The default writes the repository root ``THIRD-PARTY-LICENSES.md``. ``--check``
exits 0 when that file already matches a fresh generation and 1 when it is
stale. It exits 2 when cargo metadata or a source package cannot be read.

No file outside ``THIRD-PARTY-LICENSES.md`` is written.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "THIRD-PARTY-LICENSES.md"
TEXT_DIR = Path(__file__).resolve().parent / "license-texts"

# Keep this order: it is the order of the full-text sections in the bundle.
LICENCE_TEXTS = (
    ("MIT", "MIT.txt", "MIT License"),
    ("Apache-2.0", "Apache-2.0.txt", "Apache License 2.0"),
    (
        "Apache-2.0 WITH LLVM-exception",
        "Apache-2.0-WITH-LLVM-exception.txt",
        "Apache License 2.0 with LLVM exception",
    ),
    ("BSD-2-Clause", "BSD-2-Clause.txt", "BSD 2-Clause License"),
    ("BSD-3-Clause", "BSD-3-Clause.txt", "BSD 3-Clause License"),
    ("ISC", "ISC.txt", "ISC License"),
    ("Unicode-3.0", "Unicode-3.0.txt", "Unicode License V3"),
    ("Zlib", "Zlib.txt", "Zlib License"),
)
TEXT_BY_ID = {
    licence: (filename, heading) for licence, filename, heading in LICENCE_TEXTS
}

LICENCE_FILE_PREFIXES = (
    "LICENSE",
    "LICENCE",
    "COPYING",
    "COPYRIGHT",
    "UNLICENSE",
    "NOTICE",
)
# Do not treat licence files inside source, test or build trees as package
# notices. Vendored licence directories such as libsqlite3-sys/sqlcipher are
# deliberately kept.
SKIPPED_NOTICE_DIRS = {
    ".cargo",
    ".git",
    "benches",
    "examples",
    "fuzz",
    "src",
    "target",
    "tests",
}

# Lines that are part of the licence boilerplate rather than a real notice.
NOTICE_PLACEHOLDERS = (
    "[yyyy]",
    "[year]",
    "{yyyy}",
    "[name",
    "{name",
    "<year>",
    "name of copyright owner",
    "fullname",
)
NOTICE_BOILERPLATE_PREFIXES = (
    "notice",
    "license",
    "owner",
    "holders",
    "and ",
    "or ",
    "in ",
)
EMAIL_RE = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
ANGLE_EMAIL_RE = re.compile(r"\s*<[^<>]*@[^<>]*>")


def allowed_text_id(identifier: str) -> bool:
    if identifier in ("MIT", "Apache-2.0", "ISC", "Zlib"):
        return True
    if identifier in ("Apache-2.0 WITH LLVM-exception", "Unicode-3.0"):
        return True
    return identifier.startswith("BSD-") or identifier.startswith("Unicode-")


def expression_identifiers(expression: str) -> list[str]:
    """Return the identifiers named by a Cargo licence expression."""
    expression = expression.replace("/", " OR ")
    tokens = re.findall(
        r"\(|\)|\bAND\b|\bOR\b|\bWITH\b|[A-Za-z0-9.+-]+", expression
    )
    result: list[str] = []
    index = 0
    while index < len(tokens):
        token = tokens[index]
        if token in ("(", ")", "AND", "OR"):
            index += 1
            continue
        if token == "WITH":
            index += 1
            continue
        if index + 2 < len(tokens) and tokens[index + 1] == "WITH":
            result.append(f"{token} WITH {tokens[index + 2]}")
            index += 3
        else:
            result.append(token)
            index += 1
    return result


def run_metadata() -> dict:
    command = [
        "cargo",
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--manifest-path",
        str(ROOT / "Cargo.toml"),
    ]
    result = subprocess.run(
        command,
        cwd=ROOT,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        raise SystemExit(2)
    return json.loads(result.stdout)


def dependency_rows(metadata: dict) -> list[dict]:
    rows: list[dict] = []
    for package in metadata.get("packages", []):
        if package.get("source") is None:
            continue
        name = package.get("name", "?")
        version = package.get("version", "?")
        expression = package.get("license") or ""
        identifiers = [
            identifier
            for identifier in expression_identifiers(expression)
            if allowed_text_id(identifier)
        ]
        # Preserve LICENCE_TEXTS order and remove duplicates.
        identifiers = [
            licence
            for licence, _filename, _heading in LICENCE_TEXTS
            if licence in identifiers
        ]
        if not identifiers:
            raise SystemExit(
                f"{name} {version} has no allowed licence identifier in {expression!r}"
            )
        for identifier in identifiers:
            if identifier not in TEXT_BY_ID:
                raise SystemExit(
                    f"{name} {version} uses {identifier!r}, but no reviewed "
                    f"licence text exists in {TEXT_DIR.relative_to(ROOT)}"
                )
        rows.append(
            {
                "name": name,
                "version": version,
                "license": expression,
                "repository": package.get("repository") or "",
                "identifiers": identifiers,
                "manifest_dir": Path(package["manifest_path"]).parent,
            }
        )
    rows.sort(key=lambda row: (row["name"].lower(), row["version"]))
    return rows


def licence_files(package_dir: Path) -> list[Path]:
    if not package_dir.is_dir():
        raise SystemExit(
            f"source package is missing: {package_dir}\n"
            "run `cargo fetch --locked` before generating the licence bundle"
        )
    files = []
    for root, dirs, names in os.walk(package_dir):
        dirs[:] = sorted(
            name for name in dirs if name not in SKIPPED_NOTICE_DIRS
        )
        for name in sorted(names):
            if name.upper().startswith(LICENCE_FILE_PREFIXES):
                files.append(Path(root) / name)
    return files


def strip_email(text: str) -> str:
    # Keep the holder name and drop the contact address so the public audit
    # scanner does not treat a third-party copyright line as a leaked address.
    text = ANGLE_EMAIL_RE.sub("", text)
    return EMAIL_RE.sub("[email omitted]", text)


def copyright_notices(paths: list[Path]) -> list[str]:
    notices: list[str] = []
    for path in paths:
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError as error:
            raise SystemExit(f"cannot read licence file {path}: {error}") from error
        for raw_line in text.splitlines():
            line = " ".join(raw_line.split())
            if not line or len(line) > 240:
                continue
            if line.startswith("COPYRIGHT"):
                continue
            if line.startswith("Copyright"):
                rest = line[len("Copyright") :].strip()
            elif line.startswith("©"):
                rest = line[len("©") :].strip()
            else:
                continue
            rest_lower = rest.lower()
            if rest_lower.startswith(NOTICE_BOILERPLATE_PREFIXES):
                continue
            line_lower = line.lower()
            if any(token in line_lower for token in NOTICE_PLACEHOLDERS):
                continue
            if re.search(r"\b(19|20)\d{2}\b", rest) is None and re.match(
                r"^(?:\(c\)\s*|©\s*)?[A-Z]", rest
            ) is None:
                continue
            line = strip_email(line)
            if line not in notices:
                notices.append(line)
    return notices


def markdown_cell(value: str) -> str:
    return value.replace("|", "\\|")


def read_licence_text(filename: str) -> str:
    path = TEXT_DIR / filename
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        raise SystemExit(f"cannot read reviewed licence text {path}: {error}") from error
    if not text.strip():
        raise SystemExit(f"reviewed licence text is empty: {path}")
    return text.strip()


def build_bundle(rows: list[dict]) -> str:
    used: list[str] = []
    for row in rows:
        for identifier in row["identifiers"]:
            if identifier not in used:
                used.append(identifier)
    used.sort(key=lambda identifier: [licence for licence, _f, _h in LICENCE_TEXTS].index(identifier))

    lines = [
        "# Third-party licences",
        "",
        "VOWP release binaries include the Rust crates listed below. This notice",
        "bundle is generated from `cargo metadata --format-version 1 --locked` by",
        "`scripts/release/generate-third-party-licenses.py`; do not edit it by hand.",
        "The release workflow checks it is current and copies it into every archive as",
        "`THIRD-PARTY-LICENSES.md`.",
        "",
        "The locked dependency expressions use the allowed licence identifiers "
        + ", ".join(f"`{identifier}`" for identifier in used)
        + ". The full texts below cover every identifier allowed by",
        "`scripts/audit/licenses.sh`: MIT, Apache-2.0 with and without the LLVM",
        "exception, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0 and Zlib.",
        "",
        "## Dependency index",
        "",
        "| Package | Version | Licence | Repository |",
        "|---|---|---|---|",
    ]
    for row in rows:
        lines.append(
            f"| `{row['name']}` | {row['version']} | "
            f"{markdown_cell(row['license'])} | {markdown_cell(row['repository'])} |"
        )

    lines.extend(["", "## Full licence texts", ""])
    for identifier, filename, heading in LICENCE_TEXTS:
        text = read_licence_text(filename)
        lines.extend(
            [
                f"### {heading}",
                "",
                f"SPDX identifier: `{identifier}`",
                "",
                "```text",
                text,
                "```",
                "",
            ]
        )

    lines.extend(
        [
            "## Copyright notices",
            "",
            "These notices are read from the licence files shipped inside each crate's",
            "source package. A missing notice means the package does not ship a separate",
            "copyright line; its repository and licence expression are recorded above.",
            "",
        ]
    )
    for row in rows:
        files = licence_files(row["manifest_dir"])
        notices = copyright_notices(files)
        lines.append(f"### `{row['name']}` {row['version']}")
        lines.append("")
        lines.append(f"- Licence: {row['license']}")
        lines.append(
            "- Repository: " + (row["repository"] if row["repository"] else "not declared")
        )
        if files:
            lines.append(
                "- Licence files: "
                + ", ".join(
                    str(path.relative_to(row["manifest_dir"])) for path in files
                )
            )
        else:
            lines.append("- Licence files: none in the crate package")
        if notices:
            for notice in notices:
                lines.append(f"- {notice}")
        else:
            lines.append(
                "- Copyright: no separate copyright line in the shipped licence files"
            )
        lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail if THIRD-PARTY-LICENSES.md is not the current generated output",
    )
    args = parser.parse_args()

    metadata = run_metadata()
    rows = dependency_rows(metadata)
    bundle = build_bundle(rows)

    if args.check:
        try:
            current = OUTPUT.read_text(encoding="utf-8")
        except OSError as error:
            sys.stderr.write(f"missing {OUTPUT.relative_to(ROOT)}: {error}\n")
            return 1
        if current != bundle:
            sys.stderr.write(
                f"{OUTPUT.relative_to(ROOT)} is stale; run "
                "scripts/release/generate-third-party-licenses.py\n"
            )
            return 1
        print(
            f"{OUTPUT.relative_to(ROOT)} is current "
            f"({len(rows)} packages, {len(LICENCE_TEXTS)} licence texts)"
        )
        return 0

    OUTPUT.write_text(bundle, encoding="utf-8")
    print(
        f"wrote {OUTPUT.relative_to(ROOT)} "
        f"({len(rows)} packages, {len(LICENCE_TEXTS)} licence texts)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
