#!/usr/bin/env node
// Fetch and verify the pinned StackSpirit Engine sidecar.
//
// The bundled choice never resolves `opencode` from PATH. This script downloads one
// release archive, verifies the pinned SHA-256, extracts the single binary into
// apps/spirit-desktop/src-tauri/binaries/opencode-<target-triple>, and records the
// extracted binary digest in a generated lock file next to it. It refuses to overwrite
// a different binary without --force.
//
// Usage:
//   node scripts/fetch-engine.mjs                  # current platform
//   node scripts/fetch-engine.mjs --target <triple>
//   node scripts/fetch-engine.mjs --all
//   node scripts/fetch-engine.mjs --force
//   node scripts/fetch-engine.mjs --dry-run
//
// Requirements: Node 20+, `tar` for tar.gz assets and `unzip` for zip assets.

import { createHash } from "node:crypto";
import {
  chmodSync,
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { pipeline } from "node:stream/promises";
import { Readable } from "node:stream";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const manifestPath = join(root, "engine", "manifest.json");
const binariesDir = join(root, "apps", "spirit-desktop", "src-tauri", "binaries");
const lockPath = join(binariesDir, "engine-lock.json");

function usage(message) {
  if (message) console.error(message);
  console.error(
    "usage: node scripts/fetch-engine.mjs [--target <triple>] [--all] [--force] [--dry-run]",
  );
  process.exit(2);
}

function parseArgs(argv) {
  const options = { target: null, all: false, force: false, dryRun: false };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--target") {
      options.target = argv[index + 1] ?? null;
      index += 1;
      if (!options.target) usage("--target requires a Rust target triple");
    } else if (arg === "--all") {
      options.all = true;
    } else if (arg === "--force") {
      options.force = true;
    } else if (arg === "--dry-run") {
      options.dryRun = true;
    } else if (arg === "--help" || arg === "-h") {
      usage();
    } else {
      usage(`unknown argument: ${arg}`);
    }
  }
  if (options.all && options.target) usage("--all and --target are mutually exclusive");
  return options;
}

function hostTriple() {
  const arch = process.arch === "arm64" ? "aarch64" : process.arch;
  if (process.platform === "darwin") return `${arch}-apple-darwin`;
  if (process.platform === "linux") return `${arch}-unknown-linux-gnu`;
  if (process.platform === "win32") return `${arch}-pc-windows-msvc`;
  return null;
}

function binaryName(triple) {
  return triple.includes("windows") ? "opencode.exe" : "opencode";
}

function bundledBinaryPath(triple) {
  return join(binariesDir, `opencode-${triple}${triple.includes("windows") ? ".exe" : ""}`);
}

function sha256File(path) {
  const hash = createHash("sha256");
  hash.update(readFileSync(path));
  return hash.digest("hex");
}

async function download(url, destination) {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok || !response.body) {
    throw new Error(`download failed: HTTP ${response.status} for ${url}`);
  }
  await pipeline(Readable.fromWeb(response.body), createWriteStream(destination, { mode: 0o600 }));
}

function extract(archive, format, directory) {
  if (format === "tar.gz") {
    const result = spawnSync("tar", ["-xzf", archive, "-C", directory], { stdio: "inherit" });
    if (result.status !== 0) throw new Error("tar failed to extract the engine archive");
    return;
  }
  if (format === "zip") {
    const result = spawnSync("unzip", ["-q", archive, "-d", directory], { stdio: "inherit" });
    if (result.status !== 0) throw new Error("unzip failed to extract the engine archive");
    return;
  }
  throw new Error(`unsupported archive format: ${format}`);
}

function findExtractedBinary(directory, expectedName) {
  const stack = [directory];
  const found = [];
  while (stack.length > 0) {
    const current = stack.pop();
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const path = join(current, entry.name);
      if (entry.isDirectory()) stack.push(path);
      else if (entry.isFile() && entry.name === expectedName) found.push(path);
    }
  }
  return found.sort()[0] ?? null;
}

function verifyVersion(binary, expected) {
  const probe = spawnSync(binary, ["--version"], {
    encoding: "utf8",
    timeout: 15_000,
    maxBuffer: 64 * 1024,
  });
  if (probe.status !== 0) {
    return `could not run ${basename(binary)} --version (exit ${probe.status ?? "timeout"})`;
  }
  const text = `${probe.stdout ?? ""}${probe.stderr ?? ""}`;
  if (!text.includes(expected)) {
    return `version mismatch: expected ${expected}, got ${text.trim().split("\n")[0]}`;
  }
  return null;
}

function readLock() {
  if (!existsSync(lockPath)) return { schema: "stackspirit-engine-lock/1", targets: {} };
  try {
    const parsed = JSON.parse(readFileSync(lockPath, "utf8"));
    return parsed && typeof parsed === "object" ? parsed : { targets: {} };
  } catch {
    return { schema: "stackspirit-engine-lock/1", targets: {} };
  }
}

function writeLock(lock) {
  writeFileSync(lockPath, `${JSON.stringify(lock, null, 2)}\n`, { mode: 0o600 });
}

async function fetchTarget(manifest, triple, target, options, lock) {
  const destination = bundledBinaryPath(triple);
  if (existsSync(destination) && !options.force) {
    const digest = sha256File(destination);
    const recorded = lock.targets?.[triple]?.sha256;
    if (recorded === digest) {
      console.log(`engine ${triple}: already present and verified`);
      return;
    }
    throw new Error(
      `engine ${triple}: ${destination} exists with an unexpected digest; pass --force to replace it`,
    );
  }

  const url = `https://github.com/anomalyco/opencode/releases/download/v${manifest.version}/${target.asset}`;
  console.log(`engine ${triple}: downloading ${url}`);
  if (options.dryRun) {
    console.log(`engine ${triple}: dry run, archive sha256 ${target.sha256}`);
    return;
  }

  const work = mkdtempSync(join(tmpdir(), "stackspirit-engine-"));
  const archive = join(work, target.asset);
  const extractDir = join(work, "extract");
  mkdirSync(extractDir, { recursive: true });
  try {
    await download(url, archive);
    const digest = sha256File(archive);
    if (digest !== target.sha256) {
      throw new Error(
        `engine ${triple}: checksum mismatch (expected ${target.sha256}, got ${digest})`,
      );
    }
    const size = statSync(archive).size;
    if (typeof target.size === "number" && size !== target.size) {
      throw new Error(`engine ${triple}: size mismatch (expected ${target.size}, got ${size})`);
    }
    extract(archive, target.archive, extractDir);
    const expected = binaryName(triple);
    const extracted = findExtractedBinary(extractDir, expected);
    if (!extracted) throw new Error(`engine ${triple}: archive did not contain ${expected}`);

    mkdirSync(binariesDir, { recursive: true });
    const staged = `${destination}.partial`;
    writeFileSync(staged, readFileSync(extracted), { mode: 0o755 });
    chmodSync(staged, 0o755);
    const binaryDigest = sha256File(staged);
    if (triple === hostTriple()) {
      const versionProblem = verifyVersion(staged, manifest.version);
      if (versionProblem) {
        rmSync(staged, { force: true });
        throw new Error(`engine ${triple}: ${versionProblem}`);
      }
    }
    rmSync(destination, { force: true });
    writeFileSync(destination, readFileSync(staged), { mode: 0o755 });
    chmodSync(destination, 0o755);
    rmSync(staged, { force: true });
    lock.targets = lock.targets ?? {};
    lock.targets[triple] = {
      asset: target.asset,
      archive_sha256: target.sha256,
      binary_sha256: binaryDigest,
      version: manifest.version,
    };
    writeLock(lock);
    console.log(`engine ${triple}: ${destination} (sha256 ${binaryDigest})`);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  if (manifest.schema !== "stackspirit-engine-manifest/1") {
    throw new Error(`unsupported engine manifest schema: ${manifest.schema}`);
  }
  if (!/^\d+\.\d+\.\d+$/.test(manifest.version ?? "")) {
    throw new Error("engine manifest version must be an exact semver release");
  }
  const targets = Object.keys(manifest.targets ?? {});
  const selected = options.all
    ? targets
    : [options.target ?? hostTriple()].filter(Boolean);
  if (selected.length === 0) usage("no target triple for this host; pass --target");
  const lock = readLock();
  for (const triple of selected) {
    const target = manifest.targets?.[triple];
    if (!target) usage(`target ${triple} is not pinned in engine/manifest.json`);
    if (target.sha256?.length !== 64) {
      throw new Error(`engine ${triple}: manifest is missing a pinned SHA-256`);
    }
    await fetchTarget(manifest, triple, target, options, lock);
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
