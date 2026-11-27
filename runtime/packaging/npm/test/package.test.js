"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const f = require("./helpers/fixture.js");

const rootDir = path.join(f.NPM_DIR, "stackspirit");
const rootManifest = JSON.parse(fs.readFileSync(path.join(rootDir, "package.json"), "utf8"));
const platform = (key) => JSON.parse(fs.readFileSync(path.join(f.NPM_DIR, "platforms", key, "package.json"), "utf8"));

test("root_package_has_no_install_script", () => {
  for (const manifest of [rootManifest, ...f.KEYS.map(platform)]) {
    for (const hook of ["preinstall", "install", "postinstall", "prepare"]) {
      assert.equal(manifest.scripts?.[hook], undefined, `${manifest.name} has ${hook}`);
    }
  }
  assert.equal(rootManifest.private, undefined);
  assert.equal(rootManifest.name, "stackspirit");
  assert.equal(rootManifest.publishConfig.access, "public");
});

test("root package has no downloader, release base URL or vendored archive logic", () => {
  const files = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else files.push(full);
    }
  };
  walk(rootDir);
  assert.ok(!files.some((file) => /postinstall|download|tar\.js|targets\.js/.test(path.basename(file))));
  for (const file of files.filter((name) => /\.(js|json)$/.test(name))) {
    const text = fs.readFileSync(file, "utf8");
    assert.doesNotMatch(text, /STACKSPIRIT_RELEASE_BASE_URL|https?\.get|require\(["'](node:)?https?["']\)|sha256/i, file);
  }
  assert.ok(!fs.existsSync(path.join(f.NPM_DIR, "stackspirit-engine")));
});

test("root package graph matches the design", () => {
  assert.deepEqual(Object.keys(rootManifest.bin).sort(), [...f.BINARIES].sort());
  for (const [name, target] of Object.entries(rootManifest.bin)) {
    assert.equal(target, `bin/${name}.js`);
    const shim = fs.readFileSync(path.join(rootDir, target), "utf8").trim().split("\n");
    assert.match(shim.at(-1), new RegExp(`launch\\("${name}"\\)`));
    assert.ok(fs.statSync(path.join(rootDir, target)).mode & 0o100, `${target} must be executable`);
  }
  assert.deepEqual(
    Object.keys(rootManifest.optionalDependencies).sort(),
    f.KEYS.map((key) => `@stackspirit/${key}`).sort(),
  );
  for (const version of Object.values(rootManifest.optionalDependencies)) assert.equal(version, rootManifest.version);
  assert.deepEqual(rootManifest.files, ["bin/", "lib/", "README.md", "LICENSE"]);
});

test("package manifests describe the real repository path", () => {
  assert.equal(rootManifest.repository.url, "git+https://github.com/Stack-Spirit/stackspirit.git");
  assert.equal(rootManifest.repository.directory, "runtime/packaging/npm/stackspirit");
  for (const key of f.KEYS) {
    const manifest = platform(key);
    assert.equal(manifest.repository.url, rootManifest.repository.url);
    assert.equal(manifest.repository.directory, `runtime/packaging/npm/platforms/${key}`);
  }
});

test("platform_packages_are_os_and_cpu_scoped", () => {
  const seen = new Set();
  for (const key of f.KEYS) {
    const manifest = platform(key);
    const [os, cpu] = key.split("-");
    assert.equal(manifest.name, `@stackspirit/${key}`);
    assert.equal(manifest.version, rootManifest.version);
    assert.deepEqual(manifest.os, [os]);
    assert.deepEqual(manifest.cpu, [cpu]);
    assert.deepEqual(manifest.libc, os === "linux" ? ["glibc"] : undefined);
    assert.equal(manifest.dependencies, undefined);
    assert.equal(manifest.scripts, undefined);
    assert.deepEqual(manifest.files, ["bin/", "LICENSE"]);
    assert.ok(!seen.has(`${os}/${cpu}`), "two packages match one platform");
    seen.add(`${os}/${cpu}`);
  }
});

test("packed_tarballs_contain_only_runtime_files", () => {
  const built = f.buildPackages();
  assert.equal(built.result.status, 0, built.result.stderr);
  const names = fs.readdirSync(built.out).sort();
  assert.equal(names.filter((name) => name.endsWith(".tgz")).length, 5);
  assert.ok(names.includes("SHA256SUMS"));
  const sums = fs.readFileSync(path.join(built.out, "SHA256SUMS"), "utf8");
  assert.equal(sums.trim().split("\n").length, 5);

  const listing = (file) => f.run("tar", ["-tvzf", path.join(built.out, file)]).stdout.trim().split("\n");
  for (const key of f.KEYS) {
    const lines = listing(`stackspirit-${key}-${f.VERSION}.tgz`);
    const members = lines.map((line) => line.split(/\s+/).at(-1)).sort();
    assert.deepEqual(members, [
      "package/LICENSE",
      "package/bin/stackspirit",
      "package/bin/stackspirit-daemon",
      "package/bin/vowp",
      "package/bin/vowp-daemon",
      "package/package.json",
    ]);
    for (const line of lines.filter((entry) => entry.includes("package/bin/"))) {
      assert.match(line, /^-rwxr-xr-x /, `binary mode: ${line}`);
    }
  }
  const root = listing(`stackspirit-${f.VERSION}.tgz`).map((line) => line.split(/\s+/).at(-1)).sort();
  assert.deepEqual(root, [
    "package/LICENSE",
    "package/README.md",
    "package/bin/stackspirit-daemon.js",
    "package/bin/stackspirit.js",
    "package/bin/vowp-daemon.js",
    "package/bin/vowp.js",
    "package/lib/launch.js",
    "package/package.json",
  ]);
});

test("npm pack of the root package lists no test, source or archive files", () => {
  const result = f.run("npm", ["pack", "--dry-run", "--json", "--ignore-scripts", rootDir]);
  assert.equal(result.status, 0, result.stderr);
  const [pack] = JSON.parse(result.stdout);
  const paths = pack.files.map((file) => file.path).sort();
  assert.ok(paths.every((p) => /^(bin\/|lib\/|README\.md$|LICENSE$|package\.json$)/.test(p)), paths.join());
  assert.ok(!paths.some((p) => /test|\.tgz|\.tar/.test(p)));
});

function buildWith(mutate, { version = f.VERSION } = {}) {
  const work = f.tmp("refuse");
  const args = ["--version", version, "--out", path.join(work, "out")];
  for (const key of f.KEYS) {
    const dir = f.writeFakeBinaries(path.join(work, key));
    mutate(key, dir);
    args.push("--target", `${key}=${dir}`);
  }
  return f.run("bash", [f.BUILD, ...args]);
}

test("build script refuses a missing binary", () => {
  const result = buildWith((key, dir) => key === "darwin-x64" && fs.rmSync(path.join(dir, "vowp-daemon")));
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /missing vowp-daemon/);
});

test("build script refuses a symlink", () => {
  const result = buildWith((key, dir) => {
    if (key !== "linux-x64") return;
    fs.rmSync(path.join(dir, "vowp"));
    fs.symlinkSync("stackspirit", path.join(dir, "vowp"));
  });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /symlink/);
});

test("build script refuses a mode other than 0755", () => {
  const result = buildWith((key, dir) => key === "linux-arm64" && fs.chmodSync(path.join(dir, "stackspirit"), 0o775));
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /expected 0755/);
});

test("build script refuses a version mismatch", () => {
  const result = buildWith(() => {}, { version: "0.5.1" });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /version mismatch/);
});

test("build script refuses a missing target", () => {
  const result = f.run("bash", [f.BUILD, "--version", f.VERSION, "--out", path.join(f.tmp("x"), "o")]);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /missing --target/);
});

test("build script extracts a release archive and ignores extra members", () => {
  const work = f.tmp("archive");
  const args = ["--version", f.VERSION, "--out", path.join(work, "out")];
  for (const key of f.KEYS) {
    const top = path.join(work, `stackspirit-${f.VERSION}-${key}`);
    f.writeFakeBinaries(top);
    fs.writeFileSync(path.join(top, "README.md"), "x");
    const archive = path.join(work, `${key}.tar.gz`);
    assert.equal(f.run("tar", ["-C", work, "-czf", archive, path.basename(top)]).status, 0);
    args.push("--target", `${key}=${archive}`);
  }
  const result = f.run("bash", [f.BUILD, ...args]);
  assert.equal(result.status, 0, result.stderr);
  const members = f.run("tar", ["-tzf", path.join(work, "out", `stackspirit-linux-x64-${f.VERSION}.tgz`)]).stdout;
  assert.doesNotMatch(members, /README/);
});
