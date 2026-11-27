#!/usr/bin/env node
/**
 * Tiny zero-dependency static server for local preview and the browser checks.
 *
 *   node apps/site/scripts/serve.mjs
 *   PORT=4173 node apps/site/scripts/serve.mjs
 *
 * The Cloudflare Pages deployment serves `apps/site` as the output directory and
 * `apps/site/functions` as the Functions directory (project root = apps/site).
 */

import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SITE_ROOT = resolve(fileURLToPath(new URL("..", import.meta.url)));

const MIME = new Map([
  [".css", "text/css; charset=utf-8"],
  [".html", "text/html; charset=utf-8"],
  [".ico", "image/x-icon"],
  [".js", "text/javascript; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".mjs", "text/javascript; charset=utf-8"],
  [".png", "image/png"],
  [".svg", "image/svg+xml"],
  [".txt", "text/plain; charset=utf-8"],
  [".webmanifest", "application/manifest+json; charset=utf-8"],
  [".xml", "application/xml; charset=utf-8"],
]);

/** Resolve a URL path inside the site, or null when it escapes the root. */
function safePath(urlPath) {
  let decoded;
  try {
    decoded = decodeURIComponent(urlPath);
  } catch {
    return null;
  }
  const candidate = resolve(SITE_ROOT, `.${decoded.startsWith("/") ? decoded : `/${decoded}`}`);
  const rel = relative(SITE_ROOT, candidate);
  if (rel.startsWith("..") || isAbsolute(rel)) return null;
  return candidate;
}

const server = createServer(async (request, response) => {
  const url = new URL(request.url ?? "/", "http://127.0.0.1");
  const isHead = request.method === "HEAD";
  let file = safePath(url.pathname);

  if (url.pathname === "/") {
    file = join(SITE_ROOT, "index.html");
  }
  // Cloudflare Pages Functions are not static assets; the local server mirrors
  // that and never exposes the middleware source.
  if (url.pathname.startsWith("/functions/") || url.pathname === "/functions") {
    file = null;
  }

  let target = null;
  if (file) {
    try {
      const info = await stat(file);
      if (info.isFile()) target = file;
    } catch {
      target = null;
    }
    // Cloudflare Pages serves /privacy from privacy.html. Mirror that
    // extensionless route in the local preview so legal-page smoke checks and
    // shared links behave the same way.
    if (!target && !file.endsWith(".html")) {
      try {
        const info = await stat(`${file}.html`);
        if (info.isFile()) target = `${file}.html`;
      } catch {
        target = null;
      }
    }
  }

  if (!target) {
    target = join(SITE_ROOT, "404.html");
    response.statusCode = 404;
  }

  response.setHeader("Content-Type", MIME.get(extname(target)) ?? "application/octet-stream");
  response.setHeader("Cache-Control", "no-store");
  response.setHeader("X-Content-Type-Options", "nosniff");

  if (isHead) {
    response.end();
    return;
  }

  createReadStream(target)
    .on("error", () => {
      if (!response.headersSent) response.statusCode = 500;
      response.end("Internal server error");
    })
    .pipe(response);
});

const port = Number.parseInt(process.env.PORT ?? "4173", 10);
server.listen(port, "127.0.0.1", () => {
  console.log(`StackSpirit site: http://127.0.0.1:${port}`);
  console.log("Press Ctrl+C to stop.");
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    server.close(() => process.exit(0));
  });
}
