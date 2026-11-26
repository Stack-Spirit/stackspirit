"use strict";
// The only executable logic of the `stackspirit` npm package: find the binary in the
// matching platform package and run it. Nothing here downloads anything.

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawn } = require("node:child_process");

const PACKAGES = Object.freeze({
  "linux-x64": "@stackspirit/linux-x64",
  "linux-arm64": "@stackspirit/linux-arm64",
  "darwin-x64": "@stackspirit/darwin-x64",
  "darwin-arm64": "@stackspirit/darwin-arm64",
});

const SOURCE_BUILD =
  "build from source instead: https://github.com/Stack-Spirit/stackspirit#build-from-source";

function isMusl() {
  try {
    const report = process.report && process.report.getReport();
    const header = report && (typeof report === "string" ? JSON.parse(report) : report).header;
    return !(header && header.glibcVersionRuntime);
  } catch {
    return false;
  }
}

function unsupportedMessage(platform, arch, { musl = false } = {}) {
  if (platform === "win32") {
    return `Windows is not supported by the stackspirit npm package; run it inside WSL 2 (a Linux ${arch} install), or ${SOURCE_BUILD}`;
  }
  if (musl) {
    return `Linux with musl libc (for example Alpine) is not supported by the stackspirit npm package; ${SOURCE_BUILD}`;
  }
  return `unsupported platform ${platform}/${arch}; stackspirit ships linux and darwin on x64 and arm64; ${SOURCE_BUILD}`;
}

/**
 * Returns the absolute path of `bin/<name>` in the installed platform package.
 * `options` exists for tests: `platform`, `arch`, `musl` override the host.
 */
function resolveBinary(name, options = {}) {
  const platform = options.platform || process.platform;
  const arch = options.arch || process.arch;
  const key = `${platform}-${arch}`;
  const pkg = PACKAGES[key];
  if (!pkg) throw new Error(unsupportedMessage(platform, arch));
  const musl = options.musl !== undefined ? options.musl : platform === "linux" && isMusl();
  if (platform === "linux" && musl) {
    throw new Error(unsupportedMessage(platform, arch, { musl: true }));
  }
  let manifest;
  try {
    manifest = require.resolve(`${pkg}/package.json`);
  } catch {
    throw new Error(
      `the optional platform package ${pkg} is not installed; ` +
        "run `npm install` again without --omit=optional",
    );
  }
  const root = require("../package.json");
  const platformManifest = require(manifest);
  if (platformManifest.version !== root.version) {
    throw new Error(
      `version mismatch: ${root.name} ${root.version} needs ${pkg} ${root.version}, ` +
        `found ${platformManifest.version}; delete node_modules and reinstall`,
    );
  }
  const binary = path.join(path.dirname(manifest), "bin", name);
  if (!fs.existsSync(binary)) {
    throw new Error(`${pkg} does not contain bin/${name}`);
  }
  return binary;
}

/** Runs `bin/<name>` with this process's arguments and exits with its status. */
function launch(name) {
  let binary;
  try {
    binary = resolveBinary(name);
  } catch (error) {
    process.stderr.write(`${name}: ${error.message}\n`);
    process.exit(1);
  }
  const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });
  const forward = (signal) => () => {
    try {
      child.kill(signal);
    } catch {
      // The child already exited.
    }
  };
  for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
    process.on(signal, forward(signal));
  }
  child.on("error", (error) => {
    process.stderr.write(`${name}: cannot run ${binary}: ${error.message}\n`);
    process.exit(1);
  });
  child.on("exit", (code, signal) => {
    if (signal) process.exit(128 + (os.constants.signals[signal] || 0));
    process.exit(code === null ? 1 : code);
  });
}

module.exports = { PACKAGES, resolveBinary, launch, unsupportedMessage, isMusl };
