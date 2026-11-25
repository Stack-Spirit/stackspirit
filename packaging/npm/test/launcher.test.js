"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const f = require("./helpers/fixture.js");

const launch = require(path.join(f.NPM_DIR, "stackspirit", "lib", "launch.js"));
const supported = Object.hasOwn(launch.PACKAGES, f.HOST_KEY) && !(process.platform === "linux" && launch.isMusl());
const hostTest = supported ? test : test.skip;

let built;
let installed;
function setup() {
  if (!built) {
    built = f.buildPackages();
    assert.equal(built.result.status, 0, built.result.stderr);
  }
  return built;
}
function project() {
  installed = f.installProject({ out: setup().out });
  assert.equal(installed.install.status, 0, installed.install.stderr);
  return installed.project;
}
const npx = (cwd, args) => f.run("npx", ["--no-install", ...args], { cwd });

test("launcher maps exactly the four supported platforms", () => {
  assert.deepEqual(Object.keys(launch.PACKAGES).sort(), [...f.KEYS].sort());
  for (const [key, pkg] of Object.entries(launch.PACKAGES)) assert.equal(pkg, `@stackspirit/${key}`);
});

test("unsupported platforms get an explicit message", () => {
  assert.throws(() => launch.resolveBinary("stackspirit", { platform: "win32", arch: "x64" }), /Windows.*WSL/);
  assert.throws(
    () => launch.resolveBinary("stackspirit", { platform: "linux", arch: "x64", musl: true }),
    /musl.*source/,
  );
  assert.throws(() => launch.resolveBinary("stackspirit", { platform: "freebsd", arch: "x64" }), /unsupported platform/);
});

hostTest("launcher_resolves_the_platform_package", () => {
  const cwd = project();
  const result = npx(cwd, ["stackspirit", "--version"]);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), `${f.HOST_KEY}:stackspirit [--version]`);
});

hostTest("launcher_runs_all_four_command_names", () => {
  const cwd = project();
  for (const name of f.BINARIES) {
    const result = npx(cwd, [name, "a"]);
    assert.equal(result.stdout.trim(), `${f.HOST_KEY}:${name} [a]`, result.stderr);
  }
});

hostTest("launcher_passes_arguments_and_exit_code", () => {
  const cwd = project();
  const result = npx(cwd, ["stackspirit", "--exit7", "two words", "--flag=1"]);
  assert.equal(result.status, 7);
  assert.equal(result.stdout.trim(), `${f.HOST_KEY}:stackspirit [--exit7] [two words] [--flag=1]`);
});

hostTest("npx_style_install_runs_the_fake_binary", () => {
  const result = npx(project(), ["stackspirit", "--version"]);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /stackspirit \[--version\]/);
});

hostTest("npx_style_install_runs_mcp_with_arguments", () => {
  const result = npx(project(), ["stackspirit", "mcp"]);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), `${f.HOST_KEY}:stackspirit [mcp]`);
});

hostTest("launcher_reports_missing_optional_dependency", () => {
  const cwd = project();
  fs.rmSync(path.join(cwd, "node_modules", "@stackspirit"), { recursive: true });
  const result = npx(cwd, ["stackspirit", "--version"]);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /optional platform package @stackspirit\/.* is not installed/);
  assert.match(result.stderr, /without --omit=optional/);
  assert.doesNotMatch(result.stderr, /\n\s+at /, "no stack trace");
  assert.equal(result.stdout, "");
});

hostTest("launcher_refuses_a_version_mismatch", () => {
  const cwd = project();
  const manifest = path.join(cwd, "node_modules", "@stackspirit", f.HOST_KEY, "package.json");
  const json = JSON.parse(fs.readFileSync(manifest, "utf8"));
  json.version = "0.5.1";
  fs.writeFileSync(manifest, JSON.stringify(json));
  const result = npx(cwd, ["stackspirit", "--version"]);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /version mismatch: stackspirit 0\.6\.0 needs .* found 0\.5\.1/);
  assert.equal(result.stdout, "", "a mixed-version binary must not run");
});

hostTest("launcher_reports_a_platform_package_without_the_binary", () => {
  const cwd = project();
  fs.rmSync(path.join(cwd, "node_modules", "@stackspirit", f.HOST_KEY, "bin", "vowp-daemon"));
  const result = npx(cwd, ["vowp-daemon"]);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /does not contain bin\/vowp-daemon/);
});
