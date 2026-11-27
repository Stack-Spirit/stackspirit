#!/usr/bin/env python3
"""Optional Playwright check for responsive layout and interaction.

Requires the Python Playwright package and Chromium:

    python3 -m pip install playwright
    python3 -m playwright install chromium
    python3 apps/site/scripts/verify-layout.py

It starts the dependency-free preview server on a random loopback port, then
checks 390x844 (mobile), 1024x768 (compact desktop) and 1280x800 (desktop):

  * no horizontal document overflow
  * one h1, landmarks, labelled tabs and an accessible switch
  * the four quickstart tabs (mouse and keyboard)
  * the copy button
  * the pixel-shimmer canvases and the scroll-path mascot
  * offline pricing fallback and the off-by-default backups switch
  * a simulated live catalog response (and that it cannot enable backups)
  * the privacy and terms pages at their extensionless routes
  * no requests to any origin except the local site and api.stackspirit.io

Screenshots are written to the system temporary directory as
stackspirit-site-<width>.png.
"""

from __future__ import annotations

import http.server
import json
import mimetypes
import os
import re
import sys
import tempfile
import threading
from pathlib import Path
from urllib.parse import urlparse

from playwright.sync_api import sync_playwright

SITE = Path(__file__).resolve().parents[1]
API_GLOB = "https://api.stackspirit.io/**"
SCREENSHOT_DIR = Path(tempfile.gettempdir())
# Mirrors _headers, minus upgrade-insecure-requests so the local http preview
# can still load its own subresources. This makes the browser pass exercise the
# real script/style/connect policy.
CSP = (
    "default-src 'self'; base-uri 'self'; object-src 'none'; frame-src 'none'; "
    "frame-ancestors 'none'; form-action 'none'; img-src 'self' data:; "
    "font-src 'self'; script-src 'self'; style-src 'self'; "
    "connect-src 'self' https://api.stackspirit.io"
)

LIVE_CATALOG = {
    "plans": [
        {
            "id": "home-8",
            "name": "Home 8",
            "vcpu": 4,
            "ram_gb": 8,
            "disk_gb": 100,
            "disk_kind": "SSD",
            "monthly_eur": 7.11,
            "setup_fee_eur": 8.25,
            "currency": "EUR",
            "provider_spec": "Cloud VPS 4",
        }
    ],
    "backups": {
        "monthly_eur": 4.25,
        "recommended": True,
        "provider": "cloudflare-r2",
        # A public response must never switch the optional add-on on.
        "default_enabled": True,
    },
}


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(SITE), **kwargs)

    def log_message(self, _format, *_args):
        pass

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Security-Policy", CSP)
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "strict-origin-when-cross-origin")
        super().end_headers()


def start_server() -> tuple[http.server.ThreadingHTTPServer, str]:
    mimetypes.add_type("text/javascript", ".mjs")
    httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), QuietHandler)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    return httpd, f"http://127.0.0.1:{httpd.server_address[1]}"


class Checks:
    def __init__(self) -> None:
        self.count = 0
        self.failed = 0

    def ok(self, label: str, condition: bool, detail: str = "") -> None:
        self.count += 1
        if condition:
            print(f"ok   {label}")
        else:
            self.failed += 1
            print(f"FAIL {label}{f': {detail}' if detail else ''}")

    def equal(self, label: str, actual, expected) -> None:
        self.ok(label, actual == expected, f"expected {expected!r}, got {actual!r}")


def rgb(value: str) -> tuple[int, int, int]:
    match = re.search(r"rgba?\(([^)]+)\)", value)
    if not match:
        raise AssertionError(f"unsupported colour {value!r}")
    parts = [float(part.strip()) for part in match.group(1).split(",")]
    return (int(parts[0]), int(parts[1]), int(parts[2]))


def relative_luminance(color: tuple[int, int, int]) -> float:
    channels = []
    for value in color:
        channel = value / 255
        channels.append(channel / 12.92 if channel <= 0.03928 else ((channel + 0.055) / 1.055) ** 2.4)
    red, green, blue = channels
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue


def contrast_ratio(foreground: tuple[int, int, int], background: tuple[int, int, int]) -> float:
    a = relative_luminance(foreground)
    b = relative_luminance(background)
    lighter, darker = max(a, b), min(a, b)
    return (lighter + 0.05) / (darker + 0.05)


def computed(page, selector: str) -> dict[str, str]:
    return page.eval_on_selector(
        selector,
        """el => {
          const s = getComputedStyle(el);
          return { color: s.color, background: s.backgroundColor };
        }""",
    )


def launch_browser(playwright):
    """Use the packaged browser, or a newer cached Chromium when versions differ.

    The worker image can carry a Chromium build from a newer Playwright release
    than the Python package. The CDP surface used here is compatible, so an
    explicit executable path keeps the optional check usable offline.
    """
    try:
        return playwright.chromium.launch()
    except Exception as default_error:
        candidates = []
        configured = os.environ.get("PLAYWRIGHT_CHROMIUM")
        if configured:
            candidates.append(Path(configured))
        cache = Path.home() / ".cache" / "ms-playwright"
        if cache.is_dir():
            candidates.extend(sorted(cache.glob("chromium-*/chrome-linux64/chrome"), reverse=True))
            candidates.extend(sorted(cache.glob("chromium-*/chrome-linux/chrome"), reverse=True))
        for candidate in candidates:
            if candidate.is_file():
                try:
                    return playwright.chromium.launch(executable_path=str(candidate))
                except Exception:
                    continue
        raise default_error


def open_page(browser, base: str, width: int, height: int):
    context = browser.new_context(
        viewport={"width": width, "height": height},
        permissions=["clipboard-read", "clipboard-write"],
    )
    page = context.new_page()
    requests: list[str] = []
    errors: list[str] = []

    page.add_init_script(
        """
        Object.defineProperty(navigator, "clipboard", {
          configurable: true,
          value: { writeText: async (text) => { window.__copiedText = text; } },
        });
        """
    )
    page.on("request", lambda request: requests.append(request.url))
    page.on("pageerror", lambda error: errors.append(str(error)))
    page.on(
        "console",
        lambda message: errors.append(message.text)
        if "Content Security Policy" in message.text or "Refused to" in message.text
        else None,
    )
    page.route(API_GLOB, lambda route: route.abort())
    page.goto(f"{base}/", wait_until="domcontentloaded")
    return context, page, requests, errors


def check_page(page, checks: Checks, width: int, requests: list[str], errors: list[str], base: str):
    page.wait_for_selector("#plans-list .plan-card")
    page.wait_for_function(
        "() => document.getElementById('plans-source').textContent.includes('Live catalog unavailable')"
    )

    metrics = page.evaluate(
        """() => ({
          scrollWidth: document.documentElement.scrollWidth,
          innerWidth: window.innerWidth,
        })"""
    )
    checks.ok(
        f"{width}px no horizontal overflow",
        metrics["scrollWidth"] <= metrics["innerWidth"] + 1,
        json.dumps(metrics),
    )

    checks.equal(f"{width}px one h1", page.locator("h1").count(), 1)
    checks.equal(
        f"{width}px hero promise",
        page.get_by_role("heading", level=1).inner_text().strip(),
        "Manage the always-on computers your coding agents work on.",
    )
    checks.equal(f"{width}px primary nav", page.get_by_role("navigation", name="Primary").count(), 1)
    checks.equal(f"{width}px main landmark", page.get_by_role("main").count(), 1)
    checks.equal(f"{width}px switch count", page.get_by_role("switch").count(), 1)
    checks.ok(
        f"{width}px switch accessible name",
        page.get_by_role(
            "switch", name="Enable off-site encrypted R2 backups", exact=True
        ).count()
        == 1,
    )
    checks.equal(f"{width}px images with alt", page.locator("img:not([alt])").count(), 0)
    checks.equal(
        f"{width}px pixel shimmer canvases",
        page.locator("canvas[data-shimmer]").count(),
        7,
    )

    nav_links = page.eval_on_selector_all(
        ".site-nav a",
        """els => els.map(el => {
          const box = el.getBoundingClientRect();
          return { text: el.textContent.trim(), top: Math.round(box.top), right: Math.round(box.right) };
        })""",
    )
    checks.equal(f"{width}px nav link count", len(nav_links), 6)
    checks.ok(
        f"{width}px nav links fit on screen",
        all(link["right"] <= width + 1 for link in nav_links),
        str(nav_links),
    )
    checks.ok(
        f"{width}px nav links use at most two rows",
        len({link["top"] for link in nav_links}) <= 2,
        str(nav_links),
    )

    levels = page.eval_on_selector_all(
        "h1, h2, h3", "els => els.map(el => Number(el.tagName.slice(1)))"
    )
    jumps = [b - a for a, b in zip(levels, levels[1:]) if b - a > 1]
    checks.ok(f"{width}px heading order", not jumps, f"unexpected jump(s) {jumps}")

    # The skip link is the first tab stop and moves focus into main.
    page.keyboard.press("Tab")
    checks.ok(
        f"{width}px skip link is first",
        page.evaluate("document.activeElement?.classList.contains('skip-link') === true"),
    )
    page.keyboard.press("Enter")
    page.wait_for_timeout(100)
    checks.ok(
        f"{width}px skip link targets main",
        page.evaluate("document.activeElement?.id === 'main'"),
    )

    # Quickstart tabs: mouse, then the APG arrow-key pattern.
    page.get_by_role("tab", name="PyPI", exact=True).click()
    checks.ok(f"{width}px PyPI tab shows its panel", page.locator("#panel-uvx").is_visible())
    checks.ok(f"{width}px npm panel hides", page.locator("#panel-npm").is_hidden())
    checks.ok(
        f"{width}px PyPI command visible",
        "uvx stackspirit" in page.locator("#panel-uvx").inner_text(),
    )
    page.get_by_role("tab", name="npm", exact=True).focus()
    page.keyboard.press("ArrowRight")
    checks.equal(
        f"{width}px arrow-key tabs",
        page.get_by_role("tab", name="PyPI", exact=True).get_attribute("aria-selected"),
        "true",
    )
    page.get_by_role("tab", name="Desktop", exact=True).click()
    checks.ok(
        f"{width}px desktop tab",
        "Download the desktop app" in page.locator("#panel-desktop").inner_text(),
    )

    # Copy button uses the stubbed clipboard above.
    page.get_by_role("tab", name="npm", exact=True).click()
    page.get_by_role("button", name="Copy command: npx -y stackspirit").click()
    page.wait_for_function(
        "() => document.getElementById('copy-status').textContent.includes('Copied')"
    )
    checks.equal(
        f"{width}px copy command",
        page.evaluate("window.__copiedText"),
        "npx -y stackspirit",
    )

    # Pricing fallback.
    checks.equal(f"{width}px static plan count", page.locator("#plans-list .plan-card").count(), 5)
    plans_text = page.locator("#plans-list").inner_text()
    for expected in ["Home 8", "€6.60", "€7.80", "Home 64", "€44"]:
        checks.ok(f"{width}px fallback price {expected}", expected in plans_text)

    switch = page.locator("#backups")
    checks.ok(f"{width}px backups off by default", not switch.is_checked())
    checks.ok(
        f"{width}px backups recommended",
        page.locator("#backup-recommended").is_visible(),
    )
    switch.check()
    page.wait_for_function(
        "() => document.getElementById('backups-status').textContent.includes('Backups selected')"
    )
    checks.ok(
        f"{width}px backups status",
        "€3.50" in page.locator("#backups-status").inner_text(),
    )
    switch.uncheck()

    # Contrast on the primary surfaces. The page background is measured, not assumed.
    page_background = computed(page, "body")["background"]
    for selector in [".lede", ".feature p", ".plan-card__spec", ".footer-col a"]:
        foreground = rgb(computed(page, selector)["color"])
        ratio = contrast_ratio(foreground, rgb(page_background))
        checks.ok(
            f"{width}px contrast {selector}",
            ratio >= 4.5,
            f"{ratio:.2f}:1 on the page background",
        )
    for selector in [".btn--primary", ".btn--sprout", ".plan-card__cta"]:
        style = computed(page, selector)
        background = style["background"]
        if background in ("rgba(0, 0, 0, 0)", "transparent"):
            background = page_background
        ratio = contrast_ratio(rgb(style["color"]), rgb(background))
        checks.ok(
            f"{width}px contrast {selector}",
            ratio >= 4.5,
            f"{ratio:.2f}:1",
        )

    # External requests: only the local site and the public plans endpoint.
    allowed = {urlparse(base).netloc, "api.stackspirit.io"}
    unexpected = [
        url for url in requests if urlparse(url).netloc not in allowed and urlparse(url).netloc
    ]
    checks.ok(f"{width}px no external requests", not unexpected, str(unexpected[:5]))

    checks.ok(f"{width}px no page errors", not errors, "; ".join(errors[:3]))

    if width >= 1280:
        checks.ok(
            f"{width}px scroll path visible",
            page.locator("#scroll-path").is_visible(),
        )
        page.evaluate("window.scrollTo(0, document.documentElement.scrollHeight * 0.55)")
        page.wait_for_timeout(250)
        checks.ok(
            f"{width}px mascot follows the scroll path",
            page.eval_on_selector(
                "#scroll-path-mascot", "el => el.style.transform.includes('translate3d')"
            ),
        )
    else:
        checks.ok(
            f"{width}px scroll path stays out of small layouts",
            page.locator("#scroll-path").is_hidden(),
        )

    # Return to the top so the saved screenshot shows the hero and header.
    page.evaluate("window.scrollTo(0, 0)")
    page.wait_for_timeout(100)
    screenshot = SCREENSHOT_DIR / f"stackspirit-site-{width}.png"
    page.screenshot(path=str(screenshot), full_page=False)
    print(f"     screenshot: {screenshot}")


def check_legal(browser, base: str, checks: Checks) -> None:
    """The legal pages load as static documents with no script or tracking."""
    context = browser.new_context(viewport={"width": 1280, "height": 800})
    page = context.new_page()
    requests: list[str] = []
    errors: list[str] = []
    page.on("request", lambda request: requests.append(request.url))
    page.on("pageerror", lambda error: errors.append(str(error)))

    for path, heading, contact in [
        (
            "privacy.html",
            "The short version: your machine is the default.",
            "syver@stackspirit.io",
        ),
        (
            "terms.html",
            "The software is free. The managed machine is a paid service.",
            "syver@stackspirit.io",
        ),
    ]:
        page.goto(f"{base}/{path}", wait_until="domcontentloaded")
        checks.equal(f"{path} one h1", page.locator("h1").count(), 1)
        checks.equal(f"{path} title", page.locator("h1").inner_text().strip(), heading)
        checks.ok(
            f"{path} contact link",
            page.get_by_role("link", name=contact).count() >= 1,
        )
        checks.equal(f"{path} no scripts", page.locator("script").count(), 0)
        metrics = page.evaluate(
            """() => ({
              scrollWidth: document.documentElement.scrollWidth,
              innerWidth: window.innerWidth,
            })"""
        )
        checks.ok(
            f"{path} no horizontal overflow",
            metrics["scrollWidth"] <= metrics["innerWidth"] + 1,
            json.dumps(metrics),
        )

    allowed = {urlparse(base).netloc}
    unexpected = [
        url for url in requests if urlparse(url).netloc not in allowed and urlparse(url).netloc
    ]
    checks.ok("legal pages no external requests", not unexpected, str(unexpected[:5]))
    checks.ok("legal pages no page errors", not errors, "; ".join(errors[:3]))
    context.close()


def check_no_javascript(browser, base: str, checks: Checks) -> None:
    """The static HTML must carry the fallback pricing even without JS."""
    context = browser.new_context(viewport={"width": 390, "height": 844}, java_script_enabled=False)
    page = context.new_page()
    page.goto(f"{base}/", wait_until="domcontentloaded")

    checks.equal("no-JS one h1", page.locator("h1").count(), 1)
    checks.equal("no-JS static plan count", page.locator("#plans-list .plan-card").count(), 5)
    plans_text = page.locator("#plans-list").inner_text()
    for expected in ["Home 8", "€6.60", "€7.80", "Home 64", "€44"]:
        checks.ok(f"no-JS fallback price {expected}", expected in plans_text)
    checks.ok("no-JS source note hidden", page.locator("#plans-source").is_hidden())
    checks.ok("no-JS copy buttons hidden", page.locator(".copy-button").first.is_hidden())
    checks.ok("no-JS backups off", not page.locator("#backups").is_checked())
    checks.ok("no-JS npm panel visible", page.locator("#panel-npm").is_visible())
    checks.ok("no-JS other tab panels hidden", page.locator("#panel-uvx").is_hidden())
    checks.ok(
        "no-JS command fallback",
        "uvx stackspirit" in page.locator(".install-card noscript").inner_text(),
    )
    context.close()


def check_live_catalog(browser, base: str, checks: Checks) -> None:
    context = browser.new_context(viewport={"width": 1280, "height": 800})
    page = context.new_page()
    errors: list[str] = []
    requests: list[str] = []
    page.on("pageerror", lambda error: errors.append(str(error)))
    page.on(
        "console",
        lambda message: errors.append(message.text)
        if "Content Security Policy" in message.text or "Refused to" in message.text
        else None,
    )
    page.on("request", lambda request: requests.append(request.url))

    def fulfill(route):
        route.fulfill(
            status=200,
            content_type="application/json",
            headers={"Access-Control-Allow-Origin": "*"},
            body=json.dumps(LIVE_CATALOG),
        )

    page.route(API_GLOB, fulfill)
    page.goto(f"{base}/", wait_until="domcontentloaded")
    page.wait_for_function(
        "() => document.getElementById('pricing')?.dataset.source === 'live'",
        timeout=7_000,
    )

    checks.equal("live catalog card count", page.locator("#plans-list .plan-card").count(), 1)
    checks.ok(
        "live catalog price rendered",
        "€7.11" in page.locator("#plans-list").inner_text(),
    )
    checks.ok(
        "live catalog source note",
        "Live prices" in page.locator("#plans-source").inner_text(),
    )
    checks.ok(
        "live catalog backup price",
        "€4.25" in page.locator("#backups-help").inner_text(),
    )
    checks.ok(
        "live catalog cannot enable backups",
        not page.locator("#backups").is_checked(),
    )
    checks.ok("live catalog no page errors", not errors, "; ".join(errors[:3]))

    allowed = {urlparse(base).netloc, "api.stackspirit.io"}
    unexpected = [url for url in requests if urlparse(url).netloc not in allowed and urlparse(url).netloc]
    checks.ok("live catalog no external requests", not unexpected, str(unexpected[:5]))
    context.close()


def main() -> int:
    httpd, base = start_server()
    checks = Checks()
    try:
        with sync_playwright() as playwright:
            browser = launch_browser(playwright)
            try:
                for width, height in [(390, 844), (1024, 768), (1280, 800)]:
                    context, page, requests, errors = open_page(browser, base, width, height)
                    try:
                        check_page(page, checks, width, requests, errors, base)
                    finally:
                        context.close()
                check_no_javascript(browser, base, checks)
                check_legal(browser, base, checks)
                check_live_catalog(browser, base, checks)
            finally:
                browser.close()
    finally:
        httpd.shutdown()
        httpd.server_close()

    if checks.failed:
        print(f"\n{checks.failed} of {checks.count} checks failed.")
        return 1
    print(f"\nAll {checks.count} browser checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
