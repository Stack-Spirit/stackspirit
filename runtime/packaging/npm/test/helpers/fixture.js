"use strict";
// Shared offline fixtures: fake binaries, packed tarballs and temp npm projects.

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const NPM_DIR = path.resolve(__dirname, "..", "..");
const ROOT = path.resolve(NPM_DIR, "..", "..");
const BUILD = path.join(ROOT, "scripts", "release", "build-npm-packages.sh");
const BINARIES = ["stackspirit", "vowp", "stackspirit-daemon", "vowp-daemon"];
const KEYS = ["linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64"];
const VERSION = require(path.join(NPM_DIR, "stackspirit", "package.json")).version;
const HOST_KEY = `${process.platform}-${process.arch}`;

function tmp(prefix) {
  return fs.mkdtempSync(path.join(os.tmpdir(), `stackspirit-npm-${prefix}-`));
}

/** Hermetic environment for child processes: no registry, no user config. */
function cleanEnv(extra = {}) {
  const home = tmp("home");
  return {
    PATH: process.env.PATH,
    HOME: home,
    npm_config_cache: path.join(home, "cache"),
    npm_config_userconfig: path.join(home, "npmrc"),
    npm_config_globalconfig: path.join(home, "gnpmrc"),
    npm_config_offline: "true",
    npm_config_audit: "false",
    npm_config_fund: "false",
    npm_config_update_notifier: "false",
    ...extra,
  };
}

/** Writes four fake binaries into `dir`; each prints its name and arguments. */
function writeFakeBinaries(dir, { label = "fake" } = {}) {
  fs.mkdirSync(dir, { recursive: true });
  for (const name of BINARIES) {
    const file = path.join(dir, name);
    fs.writeFileSync(
      file,
      `#!/bin/sh\nprintf '%s' "${label}:${name}"\nfor a in "$@"; do printf ' [%s]' "$a"; done\nprintf '\\n'\n` +
        `if [ "$1" = "--exit7" ]; then exit 7; fi\n`,
    );
    fs.chmodSync(file, 0o755);
  }
  return dir;
}

function run(command, args, options = {}) {
  return spawnSync(command, args, { encoding: "utf8", env: cleanEnv(), ...options });
}

/** Builds the five tarballs from fake binaries; returns `{ out, tarballs }`. */
function buildPackages({ version = VERSION } = {}) {
  const work = tmp("build");
  const out = path.join(work, "out");
  const args = ["--version", version, "--out", out];
  for (const key of KEYS) {
    args.push("--target", `${key}=${writeFakeBinaries(path.join(work, key), { label: key })}`);
  }
  const result = run("bash", [BUILD, ...args]);
  return { work, out, result, root: path.join(out, `stackspirit-${version}.tgz`) };
}

/** Installs the root tarball plus one platform tarball in a temp project, offline. */
function installProject({ out, platformKey = HOST_KEY, extraPlatformVersion } = {}) {
  const project = tmp("project");
  const platformTarball = path.join(out, `stackspirit-${platformKey}-${VERSION}.tgz`);
  const rootTarball = path.join(out, `stackspirit-${VERSION}.tgz`);
  const overrides = { [`@stackspirit/${platformKey}`]: `file:${platformTarball}` };
  fs.writeFileSync(
    path.join(project, "package.json"),
    JSON.stringify({
      name: "temp-project",
      version: "1.0.0",
      private: true,
      dependencies: { stackspirit: `file:${rootTarball}` },
      overrides,
    }),
  );
  const install = run("npm", ["install", "--ignore-scripts", "--offline", "--no-audit", "--no-fund"], {
    cwd: project,
  });
  return { project, install, extraPlatformVersion };
}

module.exports = {
  NPM_DIR, ROOT, BUILD, BINARIES, KEYS, VERSION, HOST_KEY,
  tmp, cleanEnv, run, writeFakeBinaries, buildPackages, installProject,
};
