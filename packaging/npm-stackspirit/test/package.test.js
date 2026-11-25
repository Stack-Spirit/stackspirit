"use strict";

const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");

const packageRoot = path.resolve(__dirname, "..");
const pkg = require("../package.json");

test("package manifest is the private stackspirit package", () => {
  assert.equal(pkg.name, "stackspirit");
  assert.notEqual(pkg.name, "stackspirit-engine");
  assert.equal(pkg.private, true, "a private variant must not be publishable by accident");
  assert.deepEqual(pkg.bin, {
    stackspirit: "bin/stackspirit.js",
    vowp: "bin/vowp.js",
    "stackspirit-daemon": "bin/stackspirit-daemon.js",
    "vowp-daemon": "bin/vowp-daemon.js",
  });
  assert.equal(pkg.license, "MIT");
  assert.equal(pkg.dependencies, undefined);
  assert.equal(pkg.scripts.postinstall, "node scripts/postinstall.js");
  // Version tracks the public npm package and the workspace version.
  const publicPkg = require("../../npm/package.json");
  assert.equal(pkg.version, publicPkg.version);
});

test("npm pack contains only the runtime files", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "stackspirit-npm-pack-"));
  try {
    const userconfig = path.join(root, "npmrc");
    const cache = path.join(root, "cache");
    fs.writeFileSync(userconfig, "");
    fs.mkdirSync(cache);

    const npm = process.platform === "win32" ? "npm.cmd" : "npm";
    const result = spawnSync(npm, ["pack", "--dry-run", "--json"], {
      cwd: packageRoot,
      encoding: "utf8",
      timeout: 120_000,
      env: {
        ...process.env,
        npm_config_userconfig: userconfig,
        npm_config_cache: cache,
        npm_config_audit: "false",
        npm_config_fund: "false",
        npm_config_update_notifier: "false",
      },
    });
    assert.equal(result.status, 0, result.stderr);
    const report = JSON.parse(result.stdout);
    const files = report[0].files.map((file) => file.path).sort();
    assert.deepEqual(files, [
      "LICENSE",
      "README.md",
      "bin/stackspirit-daemon.js",
      "bin/stackspirit.js",
      "bin/vowp-daemon.js",
      "bin/vowp.js",
      "lib/download.js",
      "lib/install.js",
      "lib/launch.js",
      "lib/tar.js",
      "lib/targets.js",
      "package.json",
      "scripts/postinstall.js",
    ]);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
