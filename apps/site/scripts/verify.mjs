#!/usr/bin/env node
/**
 * Zero-dependency verification for the StackSpirit marketing site.
 *
 *   node apps/site/scripts/verify.mjs
 *
 * Covers the brief's static contract: required sections and CTAs, the four
 * quickstart tabs, the pricing fallback and off-by-default backup switch, the
 * host redirect middleware, tracker-free assets, security headers, accessible
 * markup basics, and a local HTTP smoke test.
 *
 * The optional visual/responsive browser pass lives in verify-layout.py.
 */

import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { readFile, stat } from "node:fs/promises";
import { createServer as createNetServer } from "node:net";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const SITE_ROOT = fileURLToPath(new URL("..", import.meta.url));
const failures = [];
let checks = 0;

async function check(name, fn) {
  checks += 1;
  try {
    await fn();
    console.log(`ok   ${name}`);
  } catch (error) {
    failures.push({ name, error });
    console.error(`FAIL ${name}`);
    console.error(`     ${error instanceof Error ? error.message : String(error)}`);
  }
}

function read(...parts) {
  return readFile(join(SITE_ROOT, ...parts), "utf8");
}

async function exists(...parts) {
  try {
    await stat(join(SITE_ROOT, ...parts));
    return true;
  } catch {
    return false;
  }
}

function decodeEntities(text) {
  return text
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&apos;/g, "'")
    .replace(/&nbsp;/g, " ")
    .replace(/&rsquo;/g, "’")
    .replace(/&lsquo;/g, "‘")
    .replace(/&ldquo;/g, "“")
    .replace(/&rdquo;/g, "”")
    .replace(/&middot;/g, "·");
}

function textOf(html) {
  return decodeEntities(
    html
      .replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, " ")
      .replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, " ")
      .replace(/<[^>]+>/g, " "),
  )
    .replace(/\s+/g, " ")
    .trim();
}

function normalized(html) {
  return textOf(html);
}

function attributeValues(html, attribute) {
  return [...html.matchAll(new RegExp(`\\b${attribute}="([^"]*)"`, "gi"))].map(
    (match) => match[1],
  );
}

function assertIncludes(haystack, needle, label) {
  assert.ok(haystack.includes(needle), `${label ?? "text"} should include ${JSON.stringify(needle)}`);
}

function assertNotIncludes(haystack, needle, label) {
  assert.ok(!haystack.includes(needle), `${label ?? "text"} must not include ${JSON.stringify(needle)}`);
}

function pngSize(buffer) {
  assert.equal(buffer.subarray(0, 8).toString("hex"), "89504e470d0a1a0a", "expected a PNG");
  return {
    width: buffer.readUInt32BE(16),
    height: buffer.readUInt32BE(20),
  };
}

async function freePort() {
  const server = createNetServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  assert.ok(address && typeof address === "object", "expected a TCP address");
  const { port } = address;
  server.close();
  await once(server, "close");
  return port;
}

async function startPreviewServer() {
  const port = await freePort();
  const child = spawn(process.execPath, [join(SITE_ROOT, "scripts", "serve.mjs")], {
    env: { ...process.env, PORT: String(port) },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    stdout += chunk;
  });
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error(`preview server did not start. stderr: ${stderr}`));
    }, 5_000);
    child.stdout.on("data", () => {
      if (stdout.includes(`127.0.0.1:${port}`)) {
        clearTimeout(timer);
        resolve();
      }
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`preview server exited with ${code}. stderr: ${stderr}`));
    });
  });
  return {
    base: `http://127.0.0.1:${port}`,
    async stop() {
      child.kill("SIGTERM");
      await once(child, "exit");
    },
  };
}

const indexHtml = await read("index.html");
const text = normalized(indexHtml);
const pricingData = await import(
  pathToFileURL(join(SITE_ROOT, "pricing-data.mjs")).href
);
const middlewareSource = await read("functions", "_middleware.js");
const middleware = await import(
  `data:text/javascript;base64,${Buffer.from(middlewareSource).toString("base64")}`
);

/* ---------- Files and deployment shape ---------- */

await check("required site files exist", async () => {
  for (const file of [
    "index.html",
    "styles.css",
    "app.mjs",
    "pricing-data.mjs",
    "404.html",
    "favicon.ico",
    "robots.txt",
    "sitemap.xml",
    "_headers",
    "README.md",
    "functions/_middleware.js",
    "assets/stackspirit-mark-128.png",
    "assets/icon-16.png",
    "assets/icon-32.png",
    "assets/icon-32.svg",
    "assets/apple-touch-icon.png",
    "assets/og-image.png",
    "scripts/serve.mjs",
    "scripts/verify-layout.py",
    "tools/build-assets.py",
  ]) {
    assert.ok(await exists(file), `missing ${file}`);
  }
});

await check("the middleware is a Cloudflare Pages function", async () => {
  assert.equal(typeof middleware.onRequest, "function");
  assertIncludes(middlewareSource, "export async function onRequest(context)");
  assertIncludes(middlewareSource, "Response.redirect(target.toString(), 301)");
});

await check("old hosts permanently redirect to the apex", async () => {
  const cases = [
    ["https://www.stackspirit.io/", "https://stackspirit.io/"],
    ["https://www.stackspirit.io./quickstart", "https://stackspirit.io/quickstart"],
    ["https://www.stackspirit.io/quickstart?x=1", "https://stackspirit.io/quickstart?x=1"],
    ["https://stackspirit-site.pages.dev/", "https://stackspirit.io/"],
    ["https://stackspirit-site.pages.dev/faq?ref=old", "https://stackspirit.io/faq?ref=old"],
    // A protocol-relative path must stay on the canonical host.
    ["https://www.stackspirit.io//evil.example/x", "https://stackspirit.io//evil.example/x"],
  ];
  for (const [url, expected] of cases) {
    let nextCalled = false;
    const response = await middleware.onRequest({
      request: new Request(url),
      next: () => {
        nextCalled = true;
        return new Response("next");
      },
    });
    assert.equal(response.status, 301, `${url} should redirect with 301`);
    assert.equal(response.headers.get("location"), expected, `${url} location`);
    assert.equal(nextCalled, false, `${url} must not call next()`);
  }
});

await check("official hosts and preview deployments are not redirected", async () => {
  for (const url of [
    "https://stackspirit.io/",
    "https://app.stackspirit.io/cloud",
    "https://api.stackspirit.io/v1/plans",
    "https://mcp.stackspirit.io/",
    "https://abc123.stackspirit-site.pages.dev/",
  ]) {
    let nextCalled = false;
    const response = await middleware.onRequest({
      request: new Request(url),
      next: () => {
        nextCalled = true;
        return new Response("next");
      },
    });
    assert.equal(response.status, 200, `${url} should pass through`);
    assert.equal(nextCalled, true, `${url} should call next()`);
  }
});

/* ---------- Hero, CTAs, quickstart ---------- */

await check("hero has the exact promise", async () => {
  const match = indexHtml.match(/<h1\b[^>]*>([\s\S]*?)<\/h1>/i);
  assert.ok(match, "missing h1");
  assert.equal(
    textOf(match[1]),
    "An always-on machine as a home for your AI coding agents",
  );
  assertIncludes(text, "Free & MIT open source");
  assertIncludes(text, "optional turnkey homes");
});

await check("primary CTAs point at the Web App and GitHub", async () => {
  const hrefs = attributeValues(indexHtml, "href");
  assert.ok(
    hrefs.filter((href) => href === "https://app.stackspirit.io").length >= 3,
    "expected several direct app.stackspirit.io CTAs",
  );
  assert.ok(
    hrefs.filter((href) => href === "https://github.com/Stack-Spirit/stackspirit").length >= 3,
    "expected several GitHub CTAs",
  );
  assert.ok(
    hrefs.includes("https://app.stackspirit.io/cloud"),
    "expected the managed-home CTA",
  );
});

await check("quickstart has the four required install tabs", async () => {
  assert.equal((indexHtml.match(/role="tab"/g) ?? []).length, 4);
  assert.equal((indexHtml.match(/role="tabpanel"/g) ?? []).length, 4);
  for (const id of ["tab-npm", "tab-uvx", "tab-mcp", "tab-desktop"]) {
    assertIncludes(indexHtml, `id="${id}"`, `tab ${id}`);
  }
  for (const panel of ["panel-npm", "panel-uvx", "panel-mcp", "panel-desktop"]) {
    assertIncludes(indexHtml, `id="${panel}"`, `panel ${panel}`);
  }
  assert.equal((indexHtml.match(/aria-selected="true"/g) ?? []).length, 1);
  assert.equal((indexHtml.match(/aria-selected="false"/g) ?? []).length, 3);
  const panelTags = indexHtml.match(/<div\b[^>]*class="tab-panel"[^>]*>/g) ?? [];
  assert.equal(panelTags.length, 4);
  assert.equal(panelTags.filter((tag) => /\bhidden\b/.test(tag)).length, 3);
});

await check("quickstart commands are exact", async () => {
  assertIncludes(text, "npx -y stackspirit");
  assertIncludes(text, "uvx stackspirit");
  assertIncludes(text, "npx -y stackspirit mcp");
  assertIncludes(text, "Download the desktop app");
  for (const command of ["npx -y stackspirit", "uvx stackspirit", "npx -y stackspirit mcp"]) {
    assertIncludes(indexHtml, `data-copy="${command}"`, `copy button for ${command}`);
  }
  assertIncludes(indexHtml, "<noscript>", "no-JS command fallback");
});

/* ---------- Harnesses ---------- */

await check("harness compatibility names every required harness", async () => {
  for (const harness of [
    "Claude Code",
    "Codex",
    "OpenCode",
    "Antigravity",
    "T3 Code",
    "Any MCP or CLI harness",
  ]) {
    assertIncludes(text, harness, `harness ${harness}`);
  }
  assertIncludes(text, "not affiliated with or endorsed by them");
});

/* ---------- Pricing ---------- */

await check("fallback catalog is the published strategy data", () => {
  const catalog = pricingData.FALLBACK_CATALOG;
  assert.equal(catalog.plans.length, 5);
  assert.deepEqual(
    catalog.plans.map((plan) => plan.id),
    ["home-8", "home-12", "home-24", "home-48", "home-64"],
  );
  const home8 = catalog.plans[0];
  assert.equal(home8.name, "Home 8");
  assert.equal(home8.monthly_eur, 6.6);
  assert.equal(home8.setup_fee_eur, 7.8);
  assert.equal(home8.vcpu, 4);
  assert.equal(home8.ram_gb, 8);
  assert.equal(home8.disk_gb, 100);
  const home64 = catalog.plans.at(-1);
  assert.equal(home64.name, "Home 64");
  assert.equal(home64.monthly_eur, 44);
  assert.equal(home64.setup_fee_eur, 0);
  assert.equal(home64.vcpu, 16);
  assert.equal(home64.ram_gb, 64);
  assert.equal(home64.disk_gb, 500);
  assert.equal(catalog.backups.monthly_eur, 3.5);
  assert.equal(catalog.backups.recommended, true);
  assert.equal(catalog.backups.provider, "cloudflare-r2");
});

await check("price formatting matches the brief", () => {
  assert.equal(pricingData.PLANS_URL, "https://api.stackspirit.io/v1/plans");
  assert.equal(pricingData.formatEuro(6.6), "€6.60");
  assert.equal(pricingData.formatEuro(7.8), "€7.80");
  assert.equal(pricingData.formatEuro(9), "€9");
  assert.equal(pricingData.formatEuro(44), "€44");
  assert.equal(pricingData.formatEuro(null), "Price unavailable");
  assert.equal(pricingData.formatMonthly(6.6), "€6.60/mo");
  assert.equal(pricingData.formatMonthly(44), "€44/mo");
  assert.equal(pricingData.setupFeeLabel(pricingData.FALLBACK_CATALOG.plans[0]), "+ €7.80 one-time setup");
  assert.equal(pricingData.setupFeeLabel(pricingData.FALLBACK_CATALOG.plans[1]), null);
  assert.equal(
    pricingData.planSpecs(pricingData.FALLBACK_CATALOG.plans[0]),
    "4 vCPU · 8 GB RAM · 100 GB SSD",
  );
  assert.equal(
    pricingData.backupPriceLabel(pricingData.FALLBACK_CATALOG.backups),
    "€3.50/mo if enabled.",
  );
});

await check("the static fallback cards match the catalog", async () => {
  const cards = indexHtml.match(/<li class="plan-card"[\s\S]*?<\/li>/g) ?? [];
  assert.equal(cards.length, 5, "expected five static plan cards");
  for (const plan of pricingData.FALLBACK_CATALOG.plans) {
    const cardText = textOf(cards.find((card) => card.includes(`data-plan="${plan.id}"`)) ?? "");
    assertIncludes(cardText, plan.name, `${plan.id} name`);
    assertIncludes(cardText, pricingData.formatEuro(plan.monthly_eur), `${plan.id} price`);
    assertIncludes(cardText, pricingData.planSpecs(plan), `${plan.id} specs`);
    assertIncludes(cardText, plan.provider_spec, `${plan.id} provider spec`);
    const setup = pricingData.setupFeeLabel(plan);
    assertIncludes(cardText, setup ?? "No setup fee", `${plan.id} setup fee`);
  }
  // The brief asks for these exact published values.
  assertIncludes(text, "€6.60");
  assertIncludes(text, "€7.80");
  assertIncludes(text, "€44");
});

await check("managed homes state no free trial and show the backup add-on", async () => {
  assertIncludes(text, "No free trial");
  assertIncludes(text, "Off-Site Encrypted R2 Backups");
  assertIncludes(text, "Recommended");
  assertIncludes(text, "off by default");
  assertIncludes(text, "€3.50");
  assertIncludes(indexHtml, 'role="switch"');
  assertIncludes(indexHtml, 'for="backups"');
});

await check("the backup switch is off by default", async () => {
  const tags = indexHtml.match(/<input\b[^>]*id="backups"[^>]*>/g) ?? [];
  assert.equal(tags.length, 1, "expected one backups input");
  assertNotIncludes(tags[0], "checked", "backups switch");
  assert.ok(tags[0].includes('type="checkbox"'), "expected a checkbox");
  assert.ok(tags[0].includes('role="switch"'), "expected switch semantics");
  assert.ok(
    tags[0].includes('aria-describedby="backups-help"'),
    "expected the help association",
  );
});

await check("catalog parsing is strict and ignores server-side defaults", () => {
  const apiPayload = {
    plans: [
      {
        id: "home-8",
        name: "Home 8",
        vcpu: 4,
        ram_gb: 8,
        disk_gb: 100,
        disk_kind: "SSD",
        monthly_eur: 6.6,
        setup_fee_eur: 7.8,
        currency: "EUR",
        provider_spec: "Cloud VPS 4",
      },
    ],
    backups: {
      monthly_eur: 3.5,
      recommended: true,
      provider: "cloudflare-r2",
      // A public catalog must never be able to switch the add-on on.
      default_enabled: true,
    },
  };
  const parsed = pricingData.parseCatalog(apiPayload);
  assert.equal(parsed.plans.length, 1);
  assert.equal(parsed.backups.recommended, true);
  assert.equal(parsed.backups.default_enabled, undefined);

  const blankOptional = pricingData.parseCatalog({
    ...apiPayload,
    plans: [{ ...apiPayload.plans[0], disk_kind: "", provider_spec: "  " }],
  });
  assert.equal(blankOptional.plans[0].disk_kind, null);
  assert.equal(blankOptional.plans[0].provider_spec, null);

  for (const bad of [
    {},
    { plans: [], backups: apiPayload.backups },
    { plans: apiPayload.plans },
    { plans: [{ ...apiPayload.plans[0], currency: "USD" }], backups: apiPayload.backups },
    { plans: [{ ...apiPayload.plans[0], monthly_eur: -1 }], backups: apiPayload.backups },
    { plans: [{ ...apiPayload.plans[0], vcpu: 0 }], backups: apiPayload.backups },
    { plans: [apiPayload.plans[0], apiPayload.plans[0]], backups: apiPayload.backups },
    {
      plans: apiPayload.plans,
      backups: { monthly_eur: 3.5, recommended: "yes", provider: "r2" },
    },
  ]) {
    assert.throws(() => pricingData.parseCatalog(bad));
  }
});

/* ---------- Semantic and accessibility basics ---------- */

await check("HTML has one h1, landmarks and a skip link", async () => {
  assert.equal((indexHtml.match(/<h1\b/g) ?? []).length, 1);
  assert.ok((indexHtml.match(/<h2\b/g) ?? []).length >= 8);
  assertIncludes(indexHtml, '<main id="main"');
  assertIncludes(indexHtml, '<a class="skip-link" href="#main">Skip to content</a>');
  assertIncludes(indexHtml, '<header class="site-header"');
  assertIncludes(indexHtml, '<footer class="site-footer"');
  assertIncludes(indexHtml, '<nav class="site-nav" aria-label="Primary">');
  assertIncludes(indexHtml, 'lang="en"');
  assertIncludes(indexHtml, 'charset="utf-8"');
  assertIncludes(indexHtml, 'name="viewport"');
  assertIncludes(indexHtml, 'width=device-width');
});

await check("ARIA references and HTML ids are valid", async () => {
  const ids = new Set(attributeValues(indexHtml, "id"));
  assert.equal(ids.size, attributeValues(indexHtml, "id").length, "duplicate id");
  for (const attribute of ["aria-labelledby", "aria-describedby", "aria-controls"]) {
    for (const value of attributeValues(indexHtml, attribute)) {
      for (const id of value.split(/\s+/).filter(Boolean)) {
        assert.ok(ids.has(id), `${attribute} points at missing #${id}`);
      }
    }
  }
  for (const href of attributeValues(indexHtml, "href")) {
    if (href.startsWith("#")) {
      assert.ok(ids.has(href.slice(1)), `internal link points at missing ${href}`);
    }
  }
});

await check("images, buttons and new-tab links are accessible", async () => {
  for (const tag of indexHtml.match(/<img\b[^>]*>/g) ?? []) {
    assertIncludes(tag, "alt=", "img alt");
    const src = tag.match(/\bsrc="([^"]+)"/)?.[1] ?? "";
    assert.ok(src.startsWith("/assets/"), `image ${src} must be local`);
  }
  for (const tag of indexHtml.match(/<button\b[^>]*>/g) ?? []) {
    assertIncludes(tag, 'type="button"', "button type");
  }
  for (const tag of indexHtml.match(/<a\b[^>]*>/g) ?? []) {
    if (tag.includes('target="_blank"')) {
      assert.ok(
        /rel="[^"]*noopener/.test(tag),
        `target=_blank link lacks rel=noopener: ${tag}`,
      );
    }
  }
  assert.equal((indexHtml.match(/role="switch"/g) ?? []).length, 1);
  assert.equal((indexHtml.match(/<details\b/g) ?? []).length, 8);
});

await check("the page declares exactly one external module script", async () => {
  const scripts = indexHtml.match(/<script\b[^>]*>/g) ?? [];
  assert.equal(scripts.length, 1, "expected one script tag");
  assertIncludes(scripts[0], 'type="module"');
  assertIncludes(scripts[0], 'src="/app.mjs"');
  assertNotIncludes(scripts[0], "http", "script src");
  assertNotIncludes(indexHtml, "<iframe");
});

/* ---------- Tracker-free and client-side safety ---------- */

await check("no tracker, analytics or external font code", async () => {
  const files = [
    "index.html",
    "404.html",
    "styles.css",
    "app.mjs",
    "pricing-data.mjs",
    "functions/_middleware.js",
    "robots.txt",
    "sitemap.xml",
    "_headers",
  ];
  const trackerPattern =
    /(google-analytics|googletagmanager|gtag\(|plausible\.io|posthog|segment\.(?:com|io)|hotjar|clarity\.ms|matomo|facebook\.net|doubleclick\.net|static\.cloudflareinsights\.com|cloudflareinsights)/i;
  for (const file of files) {
    const source = await read(file);
    assert.ok(!trackerPattern.test(source), `${file} contains a tracker reference`);
    assert.ok(!source.includes("@import url(http"), `${file} imports an external stylesheet`);
  }
  assertNotIncludes(indexHtml, "<link rel=\"stylesheet\" href=\"http");
});

await check("client code avoids unsafe HTML injection", async () => {
  const app = await read("app.mjs");
  for (const banned of ["innerHTML", "insertAdjacentHTML", "outerHTML", "eval(", "new Function"]) {
    assertNotIncludes(app, banned, "app.mjs");
  }
  assertIncludes(app, "fetch(PLANS_URL");
  assertIncludes(app, 'credentials: "omit"');
  assertIncludes(app, "parseCatalog(await response.json())");
  assertIncludes(app, "input.checked = false");
  assertNotIncludes(app, "default_enabled");
  assertNotIncludes(app, "document.cookie");
  assertNotIncludes(app, "localStorage");
});

await check("_headers has a strict self-only CSP and no tracker host", async () => {
  const headers = await read("_headers");
  assertIncludes(headers, "Content-Security-Policy:");
  assertIncludes(headers, "default-src 'self'");
  assertIncludes(headers, "script-src 'self'");
  assertIncludes(headers, "style-src 'self'");
  assertIncludes(headers, "connect-src 'self' https://api.stackspirit.io");
  assertIncludes(headers, "frame-ancestors 'none'");
  assertIncludes(headers, "X-Content-Type-Options: nosniff");
  assertIncludes(headers, "Referrer-Policy: strict-origin-when-cross-origin");
  assertIncludes(headers, "Strict-Transport-Security:");
  assertIncludes(headers, "Cache-Control: public, max-age=86400, stale-while-revalidate=604800");
  for (const tracker of ["google", "plausible", "posthog", "hotjar", "facebook"]) {
    assertNotIncludes(headers.toLowerCase(), tracker, "_headers tracker host");
  }
  assertNotIncludes(headers, "script-src 'self' 'unsafe-inline'", "script CSP");
});

/* ---------- SEO and assets ---------- */

await check("SEO basics and sitemap are present", async () => {
  assertIncludes(indexHtml, '<link rel="canonical" href="https://stackspirit.io/"');
  assertIncludes(indexHtml, 'property="og:image" content="https://stackspirit.io/assets/og-image.png"');
  assertIncludes(indexHtml, 'name="twitter:card" content="summary_large_image"');
  const sitemap = await read("sitemap.xml");
  assertIncludes(sitemap, "<loc>https://stackspirit.io/</loc>");
  const robots = await read("robots.txt");
  assertIncludes(robots, "Sitemap: https://stackspirit.io/sitemap.xml");
});

await check("image assets have the expected dimensions", async () => {
  const og = pngSize(await readFile(join(SITE_ROOT, "assets", "og-image.png")));
  assert.deepEqual(og, { width: 1200, height: 630 });
  const touch = pngSize(await readFile(join(SITE_ROOT, "assets", "apple-touch-icon.png")));
  assert.deepEqual(touch, { width: 180, height: 180 });
  const mark = pngSize(await readFile(join(SITE_ROOT, "assets", "stackspirit-mark-128.png")));
  assert.deepEqual(mark, { width: 128, height: 128 });
});

await check("responsive CSS keeps mobile and desktop breakpoints", async () => {
  const css = await read("styles.css");
  assertIncludes(css, "@media (max-width: 1020px)");
  assertIncludes(css, "@media (max-width: 980px)");
  assertIncludes(css, "@media (max-width: 760px)");
  assertIncludes(css, "@media (max-width: 520px)");
  assertIncludes(css, "grid-template-columns: repeat(5, minmax(0, 1fr))");
  assertIncludes(css, "grid-template-columns: 1fr");
  assertIncludes(css, "prefers-reduced-motion: reduce");
  assertIncludes(css, ".has-js .copy-button");
  assertNotIncludes(css, "url(http");
});

await check("JavaScript syntax checks pass", async () => {
  for (const file of ["app.mjs", "pricing-data.mjs", "scripts/serve.mjs", "scripts/verify.mjs"]) {
    execFileSync(process.execPath, ["--check", join(SITE_ROOT, file)], {
      stdio: "pipe",
    });
  }
});

/* ---------- Local server smoke test ---------- */

await check("the local preview server serves the real static contract", async () => {
  const server = await startPreviewServer();
  try {
    const home = await fetch(`${server.base}/`);
    assert.equal(home.status, 200);
    assertIncludes(home.headers.get("content-type") ?? "", "text/html");
    assertIncludes(await home.text(), "An always-on machine as a home");

    const css = await fetch(`${server.base}/styles.css`);
    assert.equal(css.status, 200);
    assertIncludes(css.headers.get("content-type") ?? "", "text/css");

    const app = await fetch(`${server.base}/app.mjs`);
    assert.equal(app.status, 200);
    assertIncludes(app.headers.get("content-type") ?? "", "text/javascript");

    const image = await fetch(`${server.base}/assets/og-image.png`);
    assert.equal(image.status, 200);
    assertIncludes(image.headers.get("content-type") ?? "", "image/png");

    const missing = await fetch(`${server.base}/not-a-real-page`);
    assert.equal(missing.status, 404);
    assertIncludes(textOf(await missing.text()), "This page isn’t home.");

    const middlewareFile = await fetch(`${server.base}/functions/_middleware.js`);
    assert.equal(middlewareFile.status, 404, "functions must not be exposed as static files");

    const malformed = await fetch(`${server.base}/%E0%A4%A`);
    assert.equal(malformed.status, 404, "malformed URLs should not crash the preview server");

    const head = await fetch(`${server.base}/`, { method: "HEAD" });
    assert.equal(head.status, 200);
    assert.equal((await head.text()).length, 0, "HEAD responses have no body");
  } finally {
    await server.stop();
  }
});

/* ---------- Summary ---------- */

if (failures.length > 0) {
  console.error(`\n${failures.length} of ${checks} checks failed.`);
  process.exitCode = 1;
} else {
  console.log(`\nAll ${checks} checks passed.`);
}
