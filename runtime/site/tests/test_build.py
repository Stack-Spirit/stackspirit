"""Tests for the static site generator.

The test builds the site into a temporary directory with the real generator,
then checks the generated files rather than trusting the build's own output.
"""

from __future__ import annotations

import html.parser
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from urllib.parse import unquote, urlsplit

SITE_DIR = Path(__file__).resolve().parents[1]
REPO_ROOT = SITE_DIR.parent
BUILD_SCRIPT = SITE_DIR / "build.py"

EXTERNAL_PREFIXES = (
    "http://",
    "https://",
    "//",
    "mailto:",
    "tel:",
    "data:",
    "javascript:",
)


class PageParser(html.parser.HTMLParser):
    """Collect anchor targets and asset references from one generated page."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.anchors: list[str] = []
        self.assets: list[tuple[str, str, str]] = []
        self.hrefs: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = dict(attrs)
        if tag == "a" and values.get("href") is not None:
            self.anchors.append(values["href"] or "")
        if tag in {"link", "script", "img"}:
            for attribute in ("href", "src"):
                value = values.get(attribute)
                if value is not None:
                    self.assets.append((tag, attribute, value))
        href = values.get("href")
        if href is not None:
            self.hrefs.append(href)


def expected_routes() -> set[str]:
    routes = {"index.html"}
    for folder, output in (("docs/concepts", "concepts"), ("docs/guides", "guides")):
        for source in sorted((REPO_ROOT / folder).glob("*.md")):
            name = "index.html" if source.name == "README.md" else f"{source.stem}.html"
            routes.add(f"{output}/{name}")
    routes.update(
        {
            "reference/security.html",
            "reference/troubleshooting.html",
            "reference/design/index.html",
            "reference/design/job-map.html",
            "reference/design/loops.html",
            "reference/contributing.html",
            "reference/security-policy.html",
            "reference/framework.html",
            "reference/agents.html",
            "reference/files/license.html",
            "reference/files/rust-toolchain.html",
            "reference/files/ci-workflow.html",
        }
    )
    return routes


class SiteBuildTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls._temp = tempfile.TemporaryDirectory(prefix="vowp-site-test-")
        cls.out = Path(cls._temp.name) / "dist"
        result = subprocess.run(
            [sys.executable, str(BUILD_SCRIPT), "--out", str(cls.out)],
            cwd=REPO_ROOT,
            text=True,
            capture_output=True,
            timeout=120,
        )
        if result.returncode != 0:
            raise AssertionError(
                f"build failed with exit {result.returncode}\n"
                f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
            )
        cls.build_stdout = result.stdout

    @classmethod
    def tearDownClass(cls) -> None:
        cls._temp.cleanup()

    def html_files(self) -> list[Path]:
        return sorted(self.out.rglob("*.html"))

    def test_every_source_page_generated_html(self) -> None:
        for route in sorted(expected_routes()):
            with self.subTest(route=route):
                path = self.out / route
                self.assertTrue(path.is_file(), f"missing generated page: {route}")
                self.assertGreater(path.stat().st_size, 0, f"empty generated page: {route}")

    def test_style_and_landing_exist(self) -> None:
        style = self.out / "assets" / "style.css"
        self.assertTrue(style.is_file())
        self.assertTrue((self.out / "index.html").is_file())
        css = style.read_text(encoding="utf-8")
        self.assertIn("prefers-color-scheme: dark", css)
        self.assertNotIn("@import", css)
        self.assertNotIn("http://", css)
        self.assertNotIn("https://", css)

    def test_no_external_asset_urls(self) -> None:
        for path in self.html_files():
            parser = PageParser()
            parser.feed(path.read_text(encoding="utf-8"))
            for tag, attribute, value in parser.assets:
                with self.subTest(page=path.relative_to(self.out), tag=tag):
                    self.assertFalse(
                        value.lower().startswith(("http://", "https://", "//")),
                        f"{path.relative_to(self.out)} uses external asset URL: "
                        f"<{tag} {attribute}={value!r}>",
                    )

    def test_internal_links_resolve(self) -> None:
        out_root = self.out.resolve()
        for path in self.html_files():
            parser = PageParser()
            parser.feed(path.read_text(encoding="utf-8"))
            for href in parser.hrefs:
                if not href or href.startswith("#"):
                    continue
                if href.lower().startswith(EXTERNAL_PREFIXES):
                    continue
                with self.subTest(page=path.relative_to(self.out), href=href):
                    self.assertFalse(
                        href.startswith("/"),
                        f"root-absolute link does not work from file://: {href}",
                    )
                    target_part = urlsplit(href).path
                    if not target_part:
                        continue
                    target = (path.parent / unquote(target_part)).resolve()
                    self.assertTrue(
                        target.is_file(),
                        f"{path.relative_to(self.out)}: {href} -> missing {target}",
                    )
                    try:
                        target.relative_to(out_root)
                    except ValueError:
                        self.fail(
                            f"{path.relative_to(self.out)}: {href} escapes the generated site"
                        )

    def test_landing_has_project_and_quickstart(self) -> None:
        landing = (self.out / "index.html").read_text(encoding="utf-8")
        self.assertIn("VOWP", landing)
        self.assertIn("Virtual Open Work-to-Production", landing)
        self.assertIn("cargo build --workspace", landing)
        self.assertIn("cargo run -p vowp-cli --bin vowp -- --version", landing)
        self.assertIn("<pre><code", landing)
        self.assertIn("guides/quickstart.html", landing)
        self.assertNotIn("<script", landing)

    def test_sidebar_links_to_concepts_and_guides(self) -> None:
        landing = (self.out / "index.html").read_text(encoding="utf-8")
        self.assertIn('class="sidebar"', landing)
        self.assertIn("concepts/index.html", landing)
        self.assertIn("guides/index.html", landing)
        self.assertIn("concepts/graph-and-loops.html", landing)
        self.assertIn("guides/quickstart.html", landing)


if __name__ == "__main__":
    unittest.main()
