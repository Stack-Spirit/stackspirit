"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const fsp = require("node:fs/promises");
const path = require("node:path");

const { getBuffer } = require("./download");
const { extractFile } = require("./tar");
const { isMusl, targetFor, unsupportedMessage } = require("./targets");

// The private archive has no public default URL: the owner points the install
// at the private release location with STACKSPIRIT_RELEASE_BASE_URL.
const PRODUCT = "stackspirit";
// Every private archive carries both CLIs and both daemons.
const MEMBERS = Object.freeze([
  "stackspirit",
  "vowp",
  "stackspirit-daemon",
  "vowp-daemon",
]);
const MAX_ARCHIVE_BYTES = 256 * 1024 * 1024;
const MAX_CHECKSUM_BYTES = 64 * 1024;
const VERSION_PATTERN = /^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$/;

function readPackage(packageDir) {
  const file = path.join(packageDir, "package.json");
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`cannot read ${file}: ${error.message}`);
  }
}

function releaseBaseUrl(env) {
  const value = env.STACKSPIRIT_RELEASE_BASE_URL;
  if (!value) {
    throw new Error(
      "STACKSPIRIT_RELEASE_BASE_URL is not set; the private archive has no public download URL",
    );
  }
  return value.replace(/\/+$/, "");
}

function hashesEqual(left, right) {
  const a = Buffer.from(left, "hex");
  const b = Buffer.from(right, "hex");
  return a.length === b.length && crypto.timingSafeEqual(a, b);
}

function parseChecksum(text, archiveName) {
  const lines = text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line !== "");
  const hashes = [];
  for (const line of lines) {
    const match = /^([0-9a-fA-F]{64})(?:[ \t]+\*?(.+))?$/.exec(line);
    if (!match) continue;
    const file = match[2] ? path.basename(match[2].trim()) : null;
    if (file !== null && file !== archiveName) continue;
    hashes.push(match[1].toLowerCase());
  }
  if (hashes.length !== 1) {
    throw new Error(
      `invalid checksum file for ${archiveName}: expected one SHA256 line`,
    );
  }
  return hashes[0];
}

/**
 * Download, verify and install the prebuilt `vowp` binary into the package's
 * `vendor/` directory.
 *
 * @param {object} [options]
 * @param {string} [options.packageDir] package root (default: this module's parent)
 * @param {NodeJS.ProcessEnv} [options.env]
 * @param {string} [options.platform]
 * @param {string} [options.arch]
 * @param {(message: string) => void} [options.log]
 * @returns {Promise<object>} a summary with `skipped: true` when the download was skipped
 */
async function install(options = {}) {
  const env = options.env || process.env;
  const platform = options.platform || process.platform;
  const arch = options.arch || process.arch;
  const packageDir = options.packageDir || path.resolve(__dirname, "..");
  const log = options.log || ((message) => process.stdout.write(`stackspirit: ${message}\n`));

  if (env.STACKSPIRIT_SKIP_DOWNLOAD === "1") {
    log("skipping the prebuilt binary download (STACKSPIRIT_SKIP_DOWNLOAD=1)");
    return { skipped: true };
  }

  if (platform === "linux" && isMusl(platform)) {
    throw new Error(unsupportedMessage(platform, arch, { musl: true }));
  }

  const target = targetFor(platform, arch);
  if (!target) {
    throw new Error(unsupportedMessage(platform, arch));
  }

  const pkg = readPackage(packageDir);
  const version = options.version || pkg.version;
  if (typeof version !== "string" || !VERSION_PATTERN.test(version)) {
    throw new Error(`invalid package version: ${String(version)}`);
  }

  const base = releaseBaseUrl(env);
  const archiveName = `${PRODUCT}-${version}-${target}.tar.gz`;
  const archiveUrl = `${base}/v${version}/${archiveName}`;
  const checksumUrl = `${archiveUrl}.sha256`;
  const archiveRoot = `${PRODUCT}-${version}-${target}`;

  log(`downloading ${checksumUrl}`);
  const checksumText = (
    await getBuffer(checksumUrl, MAX_CHECKSUM_BYTES)
  ).toString("utf8");
  const expected = parseChecksum(checksumText, archiveName);

  log(`downloading ${archiveUrl}`);
  const archive = await getBuffer(archiveUrl, MAX_ARCHIVE_BYTES);
  const actual = crypto.createHash("sha256").update(archive).digest("hex");
  if (!hashesEqual(actual, expected)) {
    throw new Error(
      `SHA256 mismatch for ${archiveName} (expected ${expected}, got ${actual})`,
    );
  }
  log(`verified SHA256 ${actual}`);

  // An archive missing any CLI or daemon is rejected before anything is written.
  const files = new Map();
  for (const member of MEMBERS) {
    try {
      files.set(member, extractFile(archive, `${archiveRoot}/${member}`));
    } catch (error) {
      throw new Error(`cannot extract ${archiveName}: ${error.message}`);
    }
  }
  const vendorDir = path.join(packageDir, "vendor");
  await fsp.mkdir(vendorDir, { recursive: true });
  for (const member of MEMBERS) {
    const binaryPath = path.join(vendorDir, member);
    const temporary = path.join(
      vendorDir,
      `.${member}-${process.pid}-${Date.now().toString(36)}`,
    );
    try {
      await fsp.writeFile(temporary, files.get(member).data, { mode: 0o755 });
      await fsp.chmod(temporary, 0o755);
      await fsp.rename(temporary, binaryPath);
    } catch (error) {
      await fsp.rm(temporary, { force: true }).catch(() => {});
      throw error;
    }
    await fsp.chmod(binaryPath, 0o755);
    log(`installed ${binaryPath}`);
  }
  const binaryPath = path.join(vendorDir, "stackspirit");

  return { skipped: false, version, target, sha256: actual, binary: binaryPath };
}

module.exports = {
  MEMBERS,
  install,
  parseChecksum,
  readPackage,
  releaseBaseUrl,
};
