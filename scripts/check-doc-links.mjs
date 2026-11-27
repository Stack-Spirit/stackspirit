#!/usr/bin/env node
// Check that every relative Markdown link and image in the tracked docs
// resolves to a file or directory in this checkout.
//
// External URLs are never fetched: the check runs offline in CI. Fenced code
// blocks and inline code spans are blanked before parsing so regex examples
// like `[x](missing.md)` in the design docs are not treated as links.
//
// Usage: node scripts/check-doc-links.mjs [--json]

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = path.resolve(import.meta.dirname, "..");
const json = process.argv.includes("--json");
const skipDirs = new Set([".git", "node_modules", "target"]);

function trackedMarkdownFiles() {
  const output = execFileSync("git", ["ls-files", "-z", "*.md"], {
    cwd: root,
    encoding: "utf8",
  });
  return output
    .split("\0")
    .filter(Boolean)
    .filter((file) => !file.split("/").some((part) => skipDirs.has(part)))
    .sort();
}

// Replace fenced blocks and inline code with spaces, keeping every newline so
// line numbers in the report still match the original file.
function blankCode(text) {
  const lines = text.split(/\r?\n/);
  let fence = null;
  const out = [];

  for (const line of lines) {
    const fenceMatch = line.match(/^ {0,3}(`{3,}|~{3,})(.*)$/);
    if (fence) {
      if (
        fenceMatch &&
        fenceMatch[1][0] === fence.char &&
        fenceMatch[1].length >= fence.length &&
        fenceMatch[2].trim() === ""
      ) {
        fence = null;
      }
      out.push("");
      continue;
    }
    if (fenceMatch) {
      fence = { char: fenceMatch[1][0], length: fenceMatch[1].length };
      out.push("");
      continue;
    }
    out.push(line.replace(/(`+)(.*?)\1/g, (match) => " ".repeat(match.length)));
  }

  return out.join("\n");
}

const linkPattern =
  /!?\[[^\]]*\]\(\s*(<[^>]*>|[^)\s]+)(?:\s+["'][^"']*["'])?\s*\)/g;

function isRelativeTarget(target) {
  if (!target || target.startsWith("#") || target.startsWith("/")) return false;
  if (/^[A-Za-z][A-Za-z0-9+.-]*:/.test(target)) return false;
  return true;
}

function decodePath(value) {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function checkFile(file) {
  const absolute = path.join(root, file);
  const text = blankCode(fs.readFileSync(absolute, "utf8"));
  const missing = [];

  text.split("\n").forEach((line, index) => {
    for (const match of line.matchAll(linkPattern)) {
      let target = match[1].trim();
      if (target.startsWith("<") && target.endsWith(">")) {
        target = target.slice(1, -1).trim();
      }
      if (!isRelativeTarget(target)) continue;

      const pathPart = target.split(/[?#]/, 1)[0];
      if (!pathPart) continue;

      const resolved = path.resolve(
        path.dirname(absolute),
        decodePath(pathPart),
      );
      if (!fs.existsSync(resolved)) {
        missing.push({ file, line: index + 1, target });
      }
    }
  });

  return missing;
}

const files = trackedMarkdownFiles();
const missing = files.flatMap(checkFile);

if (json) {
  process.stdout.write(
    `${JSON.stringify({ scanned: files.length, broken: missing }, null, 2)}\n`,
  );
} else if (missing.length > 0) {
  for (const item of missing) {
    process.stderr.write(`${item.file}:${item.line} ${item.target}\n`);
  }
  process.stderr.write(
    `FAIL  check-doc-links: ${missing.length} broken relative link(s) in ${files.length} files\n`,
  );
} else {
  process.stdout.write(
    `PASS  check-doc-links: ${files.length} files, all relative links resolve\n`,
  );
}

process.exitCode = missing.length > 0 ? 1 : 0;
