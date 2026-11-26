"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");

const { targetFor } = require("../lib/targets");
const { cleanEnv, copyPackage } = require("./helpers/package");
const { MEMBERS, binaryFor, buildRelease } = require("./helpers/release");
const { run } = require("./helpers/run");
const { startServer } = require("./helpers/server");

const packageRoot = path.resolve(__dirname, "..");
const { version } = require("../package.json");
const target = targetFor(process.platform, process.arch);
const noTarget = target === null ? "no prebuilt archive for this test platform" : false;

function makePackage() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "stackspirit-npm-test-"));
  const dir = path.join(root, "package");
  fs.mkdirSync(dir);
  copyPackage(packageRoot, dir);
  return { root, dir };
}

function runPostinstall(packageDir, env) {
  return run(process.execPath, [path.join(packageDir, "scripts", "postinstall.js")], {
    cwd: packageDir,
    env,
    timeout: 60_000,
  });
}

test("postinstall installs both CLIs and both daemons", { skip: noTarget }, async () => {
  const { root, dir } = makePackage();
  const release = await startServer(buildRelease({ version, targets: [target] }));
  try {
    const result = await runPostinstall(
      dir,
      cleanEnv({ STACKSPIRIT_RELEASE_BASE_URL: release.url }),
    );
    assert.equal(result.status, 0, result.stderr);
    for (const name of MEMBERS) {
      const binary = path.join(dir, "vendor", name);
      assert.equal(fs.readFileSync(binary, "utf8"), binaryFor(name, version).toString());
      assert.notEqual(fs.statSync(binary).mode & 0o111, 0, `${name} is not executable`);
    }
    assert.match(result.stdout, /verified SHA256 [0-9a-f]{64}/);
    assert.deepEqual(
      release.requests.map((request) => request.pathname).sort(),
      [
        `/v${version}/stackspirit-${version}-${target}.tar.gz`,
        `/v${version}/stackspirit-${version}-${target}.tar.gz.sha256`,
      ],
    );
  } finally {
    await release.close();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("postinstall rejects an archive missing a daemon and installs nothing", { skip: noTarget }, async () => {
  const { root, dir } = makePackage();
  const release = await startServer(
    buildRelease({ version, targets: [target], omitMembers: ["vowp-daemon"] }),
  );
  try {
    const result = await runPostinstall(
      dir,
      cleanEnv({ STACKSPIRIT_RELEASE_BASE_URL: release.url }),
    );
    assert.equal(result.status, 1);
    assert.match(result.stderr, /stackspirit: error: cannot extract .*vowp-daemon/);
    assert.equal(fs.existsSync(path.join(dir, "vendor", "stackspirit")), false);
  } finally {
    await release.close();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("postinstall rejects a wrong checksum and installs nothing", { skip: noTarget }, async () => {
  const { root, dir } = makePackage();
  const release = await startServer(
    buildRelease({ version, targets: [target], corruptTarget: target }),
  );
  try {
    const result = await runPostinstall(
      dir,
      cleanEnv({ STACKSPIRIT_RELEASE_BASE_URL: release.url }),
    );
    assert.equal(result.status, 1);
    assert.match(result.stderr, /SHA256 mismatch/);
    assert.equal(fs.existsSync(path.join(dir, "vendor", "stackspirit")), false);
  } finally {
    await release.close();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("postinstall needs an explicit private release URL", { skip: noTarget }, async () => {
  const { root, dir } = makePackage();
  try {
    const result = await runPostinstall(dir, cleanEnv());
    assert.equal(result.status, 1);
    assert.match(result.stderr, /STACKSPIRIT_RELEASE_BASE_URL is not set/);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("STACKSPIRIT_SKIP_DOWNLOAD=1 makes no network request", async () => {
  const { root, dir } = makePackage();
  const release = await startServer(buildRelease({ version, targets: [target || "x86_64-unknown-linux-gnu"] }));
  try {
    const result = await runPostinstall(
      dir,
      cleanEnv({
        STACKSPIRIT_RELEASE_BASE_URL: release.url,
        STACKSPIRIT_SKIP_DOWNLOAD: "1",
      }),
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal(release.requests.length, 0);
    assert.equal(fs.existsSync(path.join(dir, "vendor")), false);
  } finally {
    await release.close();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("postinstall rejects an insecure release URL", { skip: noTarget }, async () => {
  const { root, dir } = makePackage();
  try {
    const result = await runPostinstall(
      dir,
      cleanEnv({ STACKSPIRIT_RELEASE_BASE_URL: "http://mirror.example/stackspirit" }),
    );
    assert.equal(result.status, 1);
    assert.match(result.stderr, /refusing an insecure release URL/);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
