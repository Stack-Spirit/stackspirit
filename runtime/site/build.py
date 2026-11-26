#!/usr/bin/env python3
"""Build the VOWP static site from the repository's Markdown.

The site is deliberately dependency-free: this script uses only the Python
standard library and never touches the network.  It renders a small Markdown
subset (headings, paragraphs, fenced code, inline code, lists, tables, links,
blockquotes and emphasis), rewrites repository links to generated pages and
copies the stylesheet into the output directory.

Usage:
    python3 site/build.py [--out DIR]

The default output is site/dist.  site/build.sh is the usual entry point.
"""

from __future__ import annotations

import argparse
import html
import posixpath
import re
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path

SITE_DIR = Path(__file__).resolve().parent
REPO_ROOT = SITE_DIR.parent
DEFAULT_OUT = SITE_DIR / "dist"

# Primary pages: the README is the landing page, then one page per concept
# and guide.  The order here is the order shown in the sidebar and matches the
# index tables in the Markdown.
CONCEPT_ORDER = [
    "README.md",
    "overview.md",
    "graph-and-loops.md",
    "sessions-and-switching.md",
    "providers-and-failover.md",
    "work-to-production.md",
]
GUIDE_ORDER = [
    "README.md",
    "install.md",
    "quickstart.md",
    "jobs.md",
    "operating.md",
    "sessions.md",
    "multi-node.md",
    "schedules.md",
]

# Additional repository Markdown that is referenced from the primary pages.
# Building these keeps every internal link working without sending readers to
# a code host.
REFERENCE_PAGES = [
    ("docs/security.md", "reference/security.html", "Reference", "Security model"),
    ("docs/troubleshooting.md", "reference/troubleshooting.html", "Reference", "Troubleshooting"),
    ("docs/design/README.md", "reference/design/index.html", "Reference", "Design spec"),
    ("docs/design/job-map.md", "reference/design/job-map.html", "Reference", "Job map"),
    ("docs/design/loops.md", "reference/design/loops.html", "Reference", "Loop spec"),
    ("CONTRIBUTING.md", "reference/contributing.html", "Project", "Contributing"),
    ("SECURITY.md", "reference/security-policy.html", "Project", "Security policy"),
    ("framework/README.md", "reference/framework.html", "Project", "Framework"),
    ("AGENTS.md", "reference/agents.html", "Project", "Agent rules"),
]

# Plain-text repository files linked from the docs.  They get a small source
# page so links keep resolving from file:// and local HTTP previews alike.
TEXT_PAGES = [
    ("LICENSE", "reference/files/license.html", "License", "MIT license"),
    ("rust-toolchain.toml", "reference/files/rust-toolchain.html", "rust-toolchain.toml", "Pinned Rust toolchain"),
    (".github/workflows/ci.yml", "reference/files/ci-workflow.html", "CI workflow", "GitHub Actions checks"),
]

NAV_LABELS = {
    "docs/concepts/README.md": "Concepts index",
    "docs/guides/README.md": "Guides index",
    "docs/design/README.md": "Design spec",
    "docs/design/job-map.md": "Job map",
    "docs/design/loops.md": "Loop spec",
}

EXTERNAL_SCHEMES = ("http://", "https://", "//", "mailto:", "tel:", "data:", "javascript:")


class BuildError(RuntimeError):
    """A source document could not be turned into a working page."""


@dataclass
class Page:
    source: str
    route: str
    title: str
    section: str
    nav_label: str
    description: str
    kind: str = "doc"  # doc | file
    show_in_nav: bool = False
    is_home: bool = False


def slugify(text: str) -> str:
    """Turn heading text into a GitHub-style fragment id."""
    text = re.sub(r"`([^`]*)`", r"\1", text)
    text = re.sub(r"!\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"<[^>]+>", "", text)
    text = html.unescape(text).strip().lower()
    text = "".join(ch if ch.isalnum() or ch in " -_" else "" for ch in text)
    text = re.sub(r"\s+", "-", text)
    text = re.sub(r"-{2,}", "-", text).strip("-")
    return text or "section"


def plain_text(markdown: str) -> str:
    """Rough inline-Markdown stripper used for titles and alt text."""
    text = re.sub(r"!\[([^\]]*)\]\([^)]*\)", r"\1", markdown)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"<[^>]+>", "", text)
    text = text.replace("**", "").replace("__", "").replace("`", "")
    text = re.sub(r"(?<!\w)[*_]|[*_](?!\w)", "", text)
    return re.sub(r"\s+", " ", text).strip()


def extract_title(markdown: str, fallback: str) -> str:
    for line in markdown.splitlines():
        match = re.match(r"^ {0,3}#\s+(.+?)\s*#*\s*$", line)
        if match:
            title = plain_text(match.group(1))
            if title:
                return title
    return fallback


def extract_description(markdown: str, fallback: str) -> str:
    lines = markdown.splitlines()
    seen_heading = False
    paragraph: list[str] = []
    for line in lines:
        stripped = line.strip()
        if not seen_heading:
            if re.match(r"^ {0,3}#\s+", line):
                seen_heading = True
            continue
        if not stripped:
            if paragraph:
                break
            continue
        if stripped.startswith(">"):
            if paragraph:
                break
            continue
        if re.match(r"^ {0,3}#{1,6}\s", line) or re.match(r"^ {0,3}(```|~~~)", line):
            if paragraph:
                break
            continue
        if re.match(r"^ {0,3}([-*+]|\d+[.)])\s+", line) or "|" in line:
            if paragraph:
                break
            continue
        paragraph.append(stripped)
    if paragraph:
        text = plain_text(" ".join(paragraph))
        if text:
            if len(text) > 180:
                return text[:180].rsplit(" ", 1)[0].rstrip(" ,.;:") + "…"
            return text
    return fallback


def relative_url(from_route: str, to_route: str) -> str:
    return posixpath.relpath(to_route, posixpath.dirname(from_route) or ".")


class MarkdownRenderer:
    """A deliberately small Markdown renderer for the site's source pages."""

    def __init__(self, site: "Site", page: Page):
        self.site = site
        self.page = page
        self._slugs: dict[str, int] = {}

    def render(self, markdown: str) -> str:
        lines = markdown.replace("\r\n", "\n").replace("\r", "\n").split("\n")
        return self.render_blocks(lines)

    def heading_id(self, text: str) -> str:
        base = slugify(text)
        count = self._slugs.get(base, 0)
        self._slugs[base] = count + 1
        return base if count == 0 else f"{base}-{count}"

    def render_blocks(self, lines: list[str]) -> str:
        out: list[str] = []
        i = 0
        count = len(lines)
        while i < count:
            line = lines[i]
            if not line.strip():
                i += 1
                continue

            fence = re.match(r"^( {0,3})(`{3,}|~{3,})(.*)$", line)
            if fence:
                indent = len(fence.group(1))
                marker = fence.group(2)
                info = fence.group(3).strip()
                code: list[str] = []
                i += 1
                while i < count:
                    closing = re.match(
                        r"^ {0,3}" + re.escape(marker[0]) + r"{" + str(len(marker)) + r",}\s*$",
                        lines[i],
                    )
                    if closing:
                        i += 1
                        break
                    row = lines[i]
                    remove = min(indent, len(row) - len(row.lstrip(" ")))
                    code.append(row[remove:])
                    i += 1
                language = re.sub(r"[^A-Za-z0-9_+-]", "", info.split()[0]) if info else ""
                class_attr = f' class="language-{language}"' if language else ""
                out.append(
                    f"<pre><code{class_attr}>{html.escape(chr(10).join(code), quote=False)}</code></pre>"
                )
                continue

            heading = re.match(r"^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$", line)
            if heading:
                level = len(heading.group(1))
                text = heading.group(2).strip()
                slug = self.heading_id(text)
                out.append(
                    f'<h{level} id="{html.escape(slug, quote=True)}">{self.inline(text)}</h{level}>'
                )
                i += 1
                continue

            if re.match(r"^ {0,3}([-*_])(?:\s*\1){2,}\s*$", line):
                out.append("<hr>")
                i += 1
                continue

            if re.match(r"^ {0,3}>", line):
                quote: list[str] = []
                while i < count:
                    quoted = re.match(r"^ {0,3}> ?(.*)$", lines[i])
                    if quoted:
                        quote.append(quoted.group(1))
                        i += 1
                        continue
                    if not lines[i].strip():
                        look = i + 1
                        while look < count and not lines[look].strip():
                            look += 1
                        if look < count and re.match(r"^ {0,3}>", lines[look]):
                            quote.append("")
                            i += 1
                            continue
                    break
                out.append("<blockquote>\n" + self.render_blocks(quote) + "\n</blockquote>")
                continue

            if "|" in line and i + 1 < count and self._is_table_separator(lines[i + 1]):
                out.append(self._render_table(lines, i))
                i = self._table_end(lines, i)
                continue

            list_match = re.match(r"^( *)([-+*]|\d+[.)])\s+(.*)$", line)
            if list_match:
                rendered, i = self._render_list(
                    lines, i, len(list_match.group(1)), list_match.group(2)[0].isdigit()
                )
                out.append(rendered)
                continue

            if line.startswith("    "):
                code: list[str] = []
                while i < count and (lines[i].startswith("    ") or not lines[i].strip()):
                    code.append(lines[i][4:] if lines[i].startswith("    ") else "")
                    i += 1
                while code and not code[-1].strip():
                    code.pop()
                out.append(f"<pre><code>{html.escape(chr(10).join(code), quote=False)}</code></pre>")
                continue

            paragraph = [line.lstrip()]
            i += 1
            while i < count and not self._starts_block(lines, i):
                paragraph.append(lines[i].lstrip())
                i += 1
            out.append(f"<p>{self.inline(chr(10).join(paragraph))}</p>")

        return "\n".join(out)

    def _starts_block(self, lines: list[str], i: int) -> bool:
        line = lines[i]
        if not line.strip():
            return True
        if re.match(r"^ {0,3}(#{1,6})\s+", line):
            return True
        if re.match(r"^( {0,3})(`{3,}|~{3,})", line):
            return True
        if re.match(r"^ {0,3}>", line):
            return True
        if re.match(r"^ {0,3}([-*_])(?:\s*\1){2,}\s*$", line):
            return True
        if re.match(r"^( *)([-+*]|\d+[.)])\s+", line):
            return True
        if line.startswith("    "):
            return True
        if "|" in line and i + 1 < len(lines) and self._is_table_separator(lines[i + 1]):
            return True
        return False

    @staticmethod
    def _is_table_separator(line: str) -> bool:
        return bool(re.match(r"^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)+\|?\s*$", line))

    @staticmethod
    def _split_row(line: str) -> list[str]:
        row = line.strip()
        if row.startswith("|"):
            row = row[1:]
        if row.endswith("|") and not row.endswith(r"\|"):
            row = row[:-1]
        cells: list[str] = []
        current: list[str] = []
        i = 0
        code_marker = 0
        while i < len(row):
            char = row[i]
            if char == "\\" and i + 1 < len(row) and row[i + 1] == "|":
                current.append("|")
                i += 2
                continue
            if char == "`":
                run = 1
                while i + run < len(row) and row[i + run] == "`":
                    run += 1
                current.append("`" * run)
                if code_marker == 0:
                    code_marker = run
                elif code_marker == run:
                    code_marker = 0
                i += run
                continue
            if char == "|" and code_marker == 0:
                cells.append("".join(current).strip())
                current = []
                i += 1
                continue
            current.append(char)
            i += 1
        cells.append("".join(current).strip())
        return cells

    @staticmethod
    def _cell_alignment(cell: str) -> str:
        value = cell.strip()
        left = value.startswith(":")
        right = value.endswith(":")
        if left and right:
            return "center"
        if right:
            return "right"
        if left:
            return "left"
        return ""

    def _render_table(self, lines: list[str], start: int) -> str:
        headers = self._split_row(lines[start])
        alignments = [self._cell_alignment(cell) for cell in self._split_row(lines[start + 1])]
        rows: list[list[str]] = []
        i = start + 2
        while i < len(lines) and lines[i].strip() and "|" in lines[i]:
            if re.match(r"^ {0,3}(#{1,6})\s+", lines[i]) or re.match(r"^( {0,3})(`{3,}|~{3,})", lines[i]):
                break
            rows.append(self._split_row(lines[i]))
            i += 1

        columns = max([len(headers), len(alignments), *(len(row) for row in rows)] or [1])

        def pad(cells: list[str]) -> list[str]:
            return cells + [""] * (columns - len(cells))

        headers = pad(headers)
        alignments = pad(alignments)
        parts = ['<div class="table-wrap">', "<table>", "<thead>", "<tr>"]
        for index, cell in enumerate(headers):
            style = f' style="text-align: {alignments[index]}"' if alignments[index] else ""
            parts.append(f"<th{style}>{self.inline(cell)}</th>")
        parts.extend(["</tr>", "</thead>", "<tbody>"])
        for row in rows:
            parts.append("<tr>")
            for index, cell in enumerate(pad(row)):
                style = f' style="text-align: {alignments[index]}"' if alignments[index] else ""
                parts.append(f"<td{style}>{self.inline(cell)}</td>")
            parts.append("</tr>")
        parts.extend(["</tbody>", "</table>", "</div>"])
        return "\n".join(parts)

    def _table_end(self, lines: list[str], start: int) -> int:
        i = start + 2
        while i < len(lines) and lines[i].strip() and "|" in lines[i]:
            if re.match(r"^ {0,3}(#{1,6})\s+", lines[i]) or re.match(r"^( {0,3})(`{3,}|~{3,})", lines[i]):
                break
            i += 1
        return i

    def _render_list(self, lines: list[str], start: int, base_indent: int, ordered: bool) -> tuple[str, int]:
        items: list[list[str]] = []
        start_number = 1
        if ordered:
            first = re.match(r"^ *(\d+)[.)]\s+", lines[start])
            if first:
                start_number = int(first.group(1))

        i = start
        count = len(lines)
        while i < count:
            if not lines[i].strip():
                look = i
                while look < count and not lines[look].strip():
                    look += 1
                if look < count:
                    probe = re.match(r"^( *)([-+*]|\d+[.)])\s+(.*)$", lines[look])
                    if (
                        probe
                        and len(probe.group(1)) == base_indent
                        and probe.group(2)[0].isdigit() == ordered
                    ):
                        i = look
                        continue
                break

            match = re.match(r"^( *)([-+*]|\d+[.)])\s+(.*)$", lines[i])
            if (
                not match
                or len(match.group(1)) != base_indent
                or match.group(2)[0].isdigit() != ordered
            ):
                break

            item_lines = [match.group(3)]
            content_column = match.start(3)
            i += 1
            while i < count:
                if not lines[i].strip():
                    look = i + 1
                    while look < count and not lines[look].strip():
                        look += 1
                    if look < count:
                        probe = re.match(r"^( *)([-+*]|\d+[.)])\s+(.*)$", lines[look])
                        if (
                            probe
                            and len(probe.group(1)) == base_indent
                            and probe.group(2)[0].isdigit() == ordered
                        ):
                            break
                        next_indent = len(lines[look]) - len(lines[look].lstrip(" "))
                        if next_indent > base_indent:
                            item_lines.append("")
                            i += 1
                            continue
                    break

                probe = re.match(r"^( *)([-+*]|\d+[.)])\s+(.*)$", lines[i])
                if probe:
                    indent = len(probe.group(1))
                    if indent == base_indent:
                        break
                    if indent < base_indent:
                        break
                    item_lines.append(lines[i][base_indent:])
                    i += 1
                    continue

                indent = len(lines[i]) - len(lines[i].lstrip(" "))
                if indent > base_indent:
                    item_lines.append(lines[i][min(indent, content_column) :])
                    i += 1
                    continue
                if self._starts_block(lines, i):
                    break
                item_lines.append(lines[i])
                i += 1

            items.append(item_lines)

        tag = "ol" if ordered else "ul"
        attributes = f' start="{start_number}"' if ordered and start_number != 1 else ""
        parts = [f"<{tag}{attributes}>"]
        for item_lines in items:
            inner = self.render_blocks(item_lines)
            if inner.startswith("<p>") and inner.endswith("</p>") and inner.count("<p>") == 1:
                inner = inner[3:-4]
            parts.append(f"<li>{inner}</li>")
        parts.append(f"</{tag}>")
        return "\n".join(parts), i

    def inline(self, text: str) -> str:
        text = text.replace("\x00", "")
        text = re.sub(r" {2,}\n", "\x00", text)
        out: list[str] = []
        i = 0
        length = len(text)
        while i < length:
            char = text[i]
            if char == "\x00":
                out.append("<br>")
                i += 1
                continue

            if char == "\\" and i + 1 < length and text[i + 1] in r"\`*_{}[]()#+-.!|>":
                out.append(html.escape(text[i + 1], quote=False))
                i += 2
                continue

            if char == "`":
                run = 1
                while i + run < length and text[i + run] == "`":
                    run += 1
                closing = text.find("`" * run, i + run)
                if closing != -1:
                    code = text[i + run : closing]
                    if len(code) >= 2 and code.startswith(" ") and code.endswith(" ") and code.strip():
                        code = code[1:-1]
                    out.append(f"<code>{html.escape(code, quote=False)}</code>")
                    i = closing + run
                    continue
                out.append("`")
                i += 1
                continue

            if char == "!" and i + 1 < length and text[i + 1] == "[":
                parsed = self._parse_link(text, i + 1)
                if parsed:
                    label, destination, end = parsed
                    href = self.site.resolve_href(destination, self.page)
                    out.append(
                        f'<img src="{html.escape(href, quote=True)}" '
                        f'alt="{html.escape(plain_text(label), quote=True)}">'
                    )
                    i = end
                    continue

            if char == "[":
                parsed = self._parse_link(text, i)
                if parsed:
                    label, destination, end = parsed
                    href = self.site.resolve_href(destination, self.page)
                    out.append(
                        f'<a href="{html.escape(href, quote=True)}">{self.inline(label)}</a>'
                    )
                    i = end
                    continue

            if char == "<":
                autolink = re.match(r"<(https?://[^ >]+|mailto:[^ >]+)>", text[i:])
                if autolink:
                    url = autolink.group(1)
                    out.append(
                        f'<a href="{html.escape(url, quote=True)}">{html.escape(url, quote=False)}</a>'
                    )
                    i += autolink.end()
                    continue
                out.append("&lt;")
                i += 1
                continue

            if text.startswith("**", i) or text.startswith("__", i):
                marker = text[i : i + 2]
                closing = text.find(marker, i + 2)
                if closing != -1:
                    out.append(f"<strong>{self.inline(text[i + 2 : closing])}</strong>")
                    i = closing + 2
                    continue

            if text.startswith("~~", i):
                closing = text.find("~~", i + 2)
                if closing != -1:
                    out.append(f"<del>{self.inline(text[i + 2 : closing])}</del>")
                    i = closing + 2
                    continue

            if char in "*_":
                closing = text.find(char, i + 1)
                if closing != -1 and closing > i + 1:
                    if (
                        char == "_"
                        and i > 0
                        and text[i - 1].isalnum()
                        and closing + 1 < length
                        and text[closing + 1].isalnum()
                    ):
                        out.append("_")
                        i += 1
                        continue
                    out.append(f"<em>{self.inline(text[i + 1 : closing])}</em>")
                    i = closing + 1
                    continue

            if char == "\n":
                out.append(" ")
                i += 1
                continue

            out.append(html.escape(char, quote=False))
            i += 1

        return "".join(out)

    @staticmethod
    def _parse_link(text: str, start: int) -> tuple[str, str, int] | None:
        depth = 0
        i = start
        while i < len(text):
            char = text[i]
            if char == "\\":
                i += 2
                continue
            if char == "[":
                depth += 1
            elif char == "]":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        if i >= len(text) or i + 1 >= len(text) or text[i + 1] != "(":
            return None

        label = text[start + 1 : i]
        j = i + 2
        depth = 1
        while j < len(text):
            char = text[j]
            if char == "\\":
                j += 2
                continue
            if char == "(":
                depth += 1
            elif char == ")":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        if j >= len(text):
            return None

        destination = text[i + 2 : j].strip()
        if destination.startswith("<") and ">" in destination:
            destination = destination[1 : destination.index(">")]
        else:
            title = re.match(r"^(\S+)(?:\s+[\"'].*[\"'])?$", destination)
            if title:
                destination = title.group(1)
        return label, destination, j + 1


class Site:
    def __init__(self, repo_root: Path, site_dir: Path, out_dir: Path):
        self.repo_root = repo_root
        self.site_dir = site_dir
        self.out_dir = out_dir
        self.pages: list[Page] = []
        self.by_source: dict[str, Page] = {}
        self.by_route: dict[str, Page] = {}
        self.nav_pages: list[Page] = []

    # -- registration -----------------------------------------------------

    def add_page(
        self,
        source: str,
        route: str,
        section: str,
        nav_label: str | None = None,
        *,
        kind: str = "doc",
        show_in_nav: bool = False,
        is_home: bool = False,
    ) -> Page:
        path = self.repo_root / source
        if not path.is_file():
            raise BuildError(f"source page is missing: {source}")
        if kind == "doc":
            markdown = path.read_text(encoding="utf-8")
            title = extract_title(markdown, Path(source).stem)
            description = extract_description(markdown, title)
        else:
            title = nav_label or Path(source).name
            description = f"{title} from the VOWP repository."
        if route in self.by_route:
            raise BuildError(f"duplicate output route: {route}")
        page = Page(
            source=source,
            route=route,
            title=title,
            section=section,
            nav_label=nav_label or title,
            description=description,
            kind=kind,
            show_in_nav=show_in_nav,
            is_home=is_home,
        )
        self.pages.append(page)
        self.by_source[source] = page
        self.by_route[route] = page
        if show_in_nav:
            self.nav_pages.append(page)
        return page

    def discover(self) -> None:
        self.add_page(
            "README.md", "index.html", "Home", "Home", show_in_nav=True, is_home=True
        )

        for section, folder, order in (
            ("Concepts", "docs/concepts", CONCEPT_ORDER),
            ("Guides", "docs/guides", GUIDE_ORDER),
        ):
            available = {path.name: path for path in (self.repo_root / folder).glob("*.md")}
            names = [name for name in order if name in available]
            names.extend(sorted(name for name in available if name not in names))
            for name in names:
                source = f"{folder}/{name}"
                route = f"{folder.split('/')[-1]}/" + (
                    "index.html" if name == "README.md" else f"{Path(name).stem}.html"
                )
                self.add_page(
                    source,
                    route,
                    section,
                    NAV_LABELS.get(source),
                    show_in_nav=True,
                )

        for source, route, section, label in REFERENCE_PAGES:
            self.add_page(source, route, section, label, show_in_nav=True)

        for source, route, title, description in TEXT_PAGES:
            page = self.add_page(
                source,
                route,
                "Reference",
                title,
                kind="file",
                show_in_nav=False,
            )
            page.description = description

    # -- link rewriting ---------------------------------------------------

    def _target_file(self, target: str) -> str | None:
        if not target or target == ".":
            target = "README.md"
        path = self.repo_root / target
        if path.is_dir():
            for name in ("README.md", "index.md"):
                candidate = posixpath.join(target, name)
                if (self.repo_root / candidate).is_file():
                    return candidate
            return None
        if path.is_file():
            return target
        return None

    def _ensure_page_for(self, source: str) -> Page:
        existing = self.by_source.get(source)
        if existing:
            return existing
        stem = re.sub(r"[^a-z0-9]+", "-", source.lower()).strip("-")
        if source.lower().endswith(".md"):
            route = self._unique_route(f"reference/source/{stem}.html")
            return self.add_page(source, route, "Reference", kind="doc", show_in_nav=False)
        route = self._unique_route(f"reference/files/{stem}.html")
        return self.add_page(source, route, "Reference", kind="file", show_in_nav=False)

    def _unique_route(self, route: str) -> str:
        if route not in self.by_route:
            return route
        stem = route[:-5] if route.endswith(".html") else route
        counter = 2
        while f"{stem}-{counter}.html" in self.by_route:
            counter += 1
        return f"{stem}-{counter}.html"

    def resolve_href(self, destination: str, page: Page) -> str:
        if not destination:
            return ""
        if destination.startswith("#"):
            return destination
        if destination.lower().startswith(EXTERNAL_SCHEMES):
            return destination

        path_part, separator, fragment = destination.partition("#")
        path_part = path_part.split("?", 1)[0]
        if not path_part:
            return f"#{fragment}" if fragment else ""

        base = posixpath.dirname(page.source)
        target = posixpath.normpath(posixpath.join(base, path_part))
        if target.startswith("../") or target == "..":
            raise BuildError(f"{page.source}: link escapes the repository: {destination}")

        source = self._target_file(target)
        if source is None:
            raise BuildError(f"{page.source}: link target does not exist: {destination}")

        target_page = self._ensure_page_for(source)
        relative = relative_url(page.route, target_page.route)
        if separator and fragment:
            relative += f"#{fragment}"
        return relative

    # -- rendering --------------------------------------------------------

    def _nav_html(self, page: Page) -> str:
        def link(item: Page) -> str:
            href = relative_url(page.route, item.route)
            classes = "nav-link"
            active = item.route == page.route
            current_page_is_nav = any(entry.route == page.route for entry in self.nav_pages)
            if (
                item.route == "reference/design/index.html"
                and page.route.startswith("reference/design/")
                and not current_page_is_nav
            ):
                active = True
            current = ""
            if active:
                classes += " active"
                current = ' aria-current="page"'
            return (
                f'<li><a class="{classes}" href="{html.escape(href, quote=True)}"{current}>'
                f"{html.escape(item.nav_label)}</a></li>"
            )

        groups: list[tuple[str, list[Page]]] = [
            (
                "Concepts",
                [p for p in self.nav_pages if p.section == "Concepts"],
            ),
            (
                "Guides",
                [p for p in self.nav_pages if p.section == "Guides"],
            ),
            (
                "Reference",
                [p for p in self.nav_pages if p.section == "Reference"],
            ),
            (
                "Project",
                [p for p in self.nav_pages if p.section == "Project"],
            ),
        ]

        parts = [
            f'<a class="nav-link nav-home'
            f'{" active" if page.is_home else ""}" href="{html.escape(relative_url(page.route, "index.html"), quote=True)}"'
            + (' aria-current="page"' if page.is_home else "")
            + ">Home</a>"
        ]
        for title, items in groups:
            if not items:
                continue
            parts.append('<div class="nav-section">')
            parts.append(f'<p class="nav-heading">{html.escape(title)}</p>')
            parts.append("<ul>")
            parts.extend(link(item) for item in items)
            parts.append("</ul>")
            parts.append("</div>")
        return "\n".join(parts)

    def _render_template(self, template: str, values: dict[str, str]) -> str:
        def replace(match: re.Match[str]) -> str:
            key = match.group(1)
            if key not in values:
                raise BuildError(f"unfilled template token: {match.group(0)}")
            return values[key]

        return re.sub(r"\{\{([A-Za-z_]+)\}\}", replace, template)

    def build(self) -> int:
        if self.out_dir.resolve() in (self.repo_root.resolve(), self.site_dir.resolve()):
            raise BuildError("refusing to remove the repository or site directory as output")
        if self.out_dir.resolve() == self.out_dir.resolve().parent:
            raise BuildError("refusing to use the filesystem root as output")
        if self.out_dir.exists():
            shutil.rmtree(self.out_dir)
        self.out_dir.mkdir(parents=True)
        shutil.copytree(self.site_dir / "assets", self.out_dir / "assets")

        templates = {
            "base": (self.site_dir / "templates" / "base.html").read_text(encoding="utf-8"),
            "home": (self.site_dir / "templates" / "home.html").read_text(encoding="utf-8"),
        }

        index = 0
        while index < len(self.pages):
            page = self.pages[index]
            index += 1
            source_path = self.repo_root / page.source
            root = "../" * page.route.count("/")

            if page.kind == "file":
                body = (
                    '<article class="doc file-page">\n'
                    f'<p class="file-meta">Source file: <code>{html.escape(page.source)}</code></p>\n'
                    f'<pre class="source-file"><code>{html.escape(source_path.read_text(encoding="utf-8"), quote=False)}</code></pre>\n'
                    "</article>"
                )
            else:
                markdown = source_path.read_text(encoding="utf-8")
                renderer = MarkdownRenderer(self, page)
                if page.is_home:
                    lines = markdown.replace("\r\n", "\n").replace("\r", "\n").split("\n")
                    split = next(
                        (i for i, line in enumerate(lines) if re.match(r"^ {0,3}##\s+", line)),
                        len(lines),
                    )
                    hero = renderer.render_blocks(lines[:split])
                    rest = renderer.render_blocks(lines[split:])
                    quickstart = self.by_source["docs/guides/quickstart.md"]
                    concepts = self.by_source["docs/concepts/overview.md"]
                    body = self._render_template(
                        templates["home"],
                        {
                            "root": root,
                            "hero": hero,
                            "body": rest,
                            "quickstart_label": html.escape(quickstart.title),
                            "concepts_label": html.escape(concepts.title),
                        },
                    )
                else:
                    body = '<article class="doc">\n' + renderer.render(markdown) + "\n</article>"

            document = self._render_template(
                templates["base"],
                {
                    "title": html.escape(page.title if page.is_home else f"{page.title} — VOWP"),
                    "description": html.escape(page.description, quote=True),
                    "root": root,
                    "nav": self._nav_html(page),
                    "content": body,
                    "body_class": "page-home" if page.is_home else "page-doc",
                },
            )
            target = self.out_dir / page.route
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(document + "\n", encoding="utf-8")

        print(f"Built {len(self.pages)} pages into {self.out_dir}")
        return len(self.pages)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Build the VOWP static site.")
    parser.add_argument(
        "--out",
        type=Path,
        default=DEFAULT_OUT,
        help=f"output directory (default: {DEFAULT_OUT})",
    )
    args = parser.parse_args(argv)

    site = Site(REPO_ROOT, SITE_DIR, args.out.resolve())
    try:
        site.discover()
        site.build()
    except BuildError as error:
        print(f"build: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
