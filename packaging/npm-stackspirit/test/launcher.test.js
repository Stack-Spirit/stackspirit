"use strict";

const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");

const packageRoot = path.resolve(__dirname, "..");
const NAMES = ["stackspirit", "vowp", "stackspirit-daemon", "vowp-daemon"];

function makeLauncher() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "stackspirit-npm-launcher-"));
  fs.mkdirSync(path.join(root, "vendor"));
  fs.cpSync(path.join(packageRoot, "bin"), path.join(root, "bin"), { recursive: true });
  fs.cpSync(path.join(packageRoot, "lib"), path.join(root, "lib"), { recursive: true });
  return root;
}

test("each launcher runs its own binary and passes arguments and the exit code", () => {
  const root = makeLauncher();
  try {
    for (const name of NAMES) {
      const binary = path.join(root, "vendor", name);
      fs.writeFileSync(binary, `#!/bin/sh\nprintf '${name}:%s\\n' "$*"\nexit 7\n`);
      fs.chmodSync(binary, 0o755);
      const result = spawnSync(
        process.execPath,
        [path.join(root, "bin", `${name}.js`), "one", "two"],
        { encoding: "utf8", timeout: 30_000 },
      );
      assert.equal(result.status, 7, name);
      assert.equal(result.stdout, `${name}:one two\n`);
    }
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("launcher reports a missing binary with a rebuild hint", () => {
  const root = makeLauncher();
  try {
    const result = spawnSync(process.execPath, [path.join(root, "bin", "vowp.js")], {
      encoding: "utf8",
      timeout: 30_000,
    });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /prebuilt vowp binary is not installed/);
    assert.match(result.stderr, /npm rebuild -g stackspirit/);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
