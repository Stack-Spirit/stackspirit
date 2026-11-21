#!/usr/bin/env node
// One-command, fail-closed VPS verification of the BYOK / OpenRouter flow against the
// real pinned `opencode` sidecar, the real desktop binary and a real browser, all under
// Xvfb. It never reads, prints or writes a real key: the only key anywhere is a
// synthetic string inside the test files.
//
//   node scripts/verify-byok-xvfb.mjs
//
// Steps (any failure stops the run with a non-zero exit):
//   1. node scripts/fetch-engine.mjs
//   2. scripts/cargo-gate.sh test -p spirit-core --features keychain --test byok_live_sidecar
//   3. real Tauri desktop binary boot smoke under `xvfb-run -a` (isolated HOME/XDG)
//   4. xvfb-run -a npx playwright test e2e/xvfb-byok-live.spec.ts
//
// Environment:
//   BYOK_REQUIRE_OS_KEYCHAIN=1  fail (instead of skipping) when the kernel keyring that
//                               the real OsKeychain needs is unavailable, for example in
//                               a seccomp-restricted container. Default: skip and say so.
//   BYOK_SMOKE_SECONDS=6        how long the desktop binary is left running.
//   BYOK_ONLY=boot              run one step only (fetch | rust | boot | e2e); debugging.
import { spawn, spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DESKTOP = join(ROOT, "apps/spirit-desktop");
const SMOKE_SECONDS = Number(process.env.BYOK_SMOKE_SECONDS ?? "6");
const FORBIDDEN = /panic|SQLITE_BUSY|database is locked|unhandled|uncaught|segmentation fault|fatal/i;
// The synthetic test key is assembled here only to scan for it; it is not a credential.
const TEST_KEY = ["sk", "or", "v1", "test-dummy-not-a-real-key-00000000"].join("-");

const summary = [];
const record = (step, status, detail = "") => {
  summary.push({ step, status, detail });
  console.log(`\n[${status}] ${step}${detail ? ` — ${detail}` : ""}`);
};

class StepFailure extends Error {}
const fail = (message) => {
  throw new StepFailure(message);
};

/** Runs a command, streaming and capturing its output. Resolves with the combined text. */
function run(command, args, options = {}) {
  return new Promise((resolveRun, reject) => {
    const child = spawn(command, args, {
      cwd: options.cwd ?? ROOT,
      env: options.env ?? process.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let output = "";
    const forward = (stream, target) =>
      stream.on("data", (chunk) => {
        output += chunk;
        if (options.quiet !== true) target.write(chunk);
      });
    forward(child.stdout, process.stdout);
    forward(child.stderr, process.stderr);
    child.on("error", reject);
    child.on("close", (code) =>
      code === 0
        ? resolveRun(output)
        : reject(new StepFailure(`${command} ${args.join(" ")} exited with ${code}`)),
    );
  });
}

function filesUnder(directory) {
  const found = [];
  if (!existsSync(directory)) return found;
  for (const name of readdirSync(directory)) {
    const path = join(directory, name);
    const info = statSync(path, { throwIfNoEntry: false });
    if (!info) continue;
    if (info.isDirectory()) found.push(...filesUnder(path));
    else if (info.isFile()) found.push(path);
  }
  return found;
}

/** Counts lines, forbidden-pattern hits and key leaks across `opencode.log` files. */
function auditLogs(root) {
  const logs = filesUnder(root).filter((path) => path.endsWith("opencode.log"));
  let lines = 0;
  let errors = 0;
  let leaks = 0;
  for (const path of logs) {
    const text = readFileSync(path, "utf8");
    for (const line of text.split("\n")) {
      if (line.trim() === "") continue;
      lines += 1;
      if (FORBIDDEN.test(line)) errors += 1;
    }
  }
  const needle = Buffer.from(TEST_KEY);
  for (const path of filesUnder(root)) {
    if (readFileSync(path).includes(needle)) leaks += 1;
  }
  return { files: logs.length, lines, errors, leaks };
}

/** Pids of live processes (owned by us) whose environment mentions `marker`. */
function processesWithEnv(marker) {
  const pids = [];
  for (const entry of readdirSync("/proc")) {
    if (!/^\d+$/.test(entry)) continue;
    try {
      const environ = readFileSync(`/proc/${entry}/environ`, "utf8");
      if (environ.includes(marker)) pids.push(Number(entry));
    } catch {
      // not ours, or already gone
    }
  }
  return pids;
}

function isOpencode(pid) {
  try {
    return readFileSync(`/proc/${pid}/cmdline`, "utf8").includes("opencode");
  } catch {
    return false;
  }
}

function pidAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

// ───────────────────────── step 1: engine ─────────────────────────

async function fetchEngine() {
  await run("node", ["scripts/fetch-engine.mjs"]);
  record("fetch pinned opencode sidecar", "ok", "checksum-verified");
}

// ───────────────────────── step 2: live Rust test ─────────────────────────

async function rustLiveTest() {
  // The test re-execs itself through a tiny helper that joins a fresh session keyring,
  // so the real keyutils backend is usable even when the container starts us in a
  // read-only keyring. Require it on the x86_64 Linux VPS this runner targets; other
  // hosts (and an explicit BYOK_ALLOW_KEYCHAIN_SKIP=1) may still report a skip.
  const env = { ...process.env };
  if (
    process.platform === "linux" &&
    process.arch === "x64" &&
    env.BYOK_ALLOW_KEYCHAIN_SKIP !== "1"
  ) {
    env.BYOK_REQUIRE_OS_KEYCHAIN = "1";
  }
  const output = await run(
    "scripts/cargo-gate.sh",
    [
      "test",
      "-p",
      "spirit-core",
      "--features",
      "keychain",
      "--test",
      "byok_live_sidecar",
      "--",
      "--nocapture",
      "--test-threads=1",
    ],
    { env },
  );
  const stats = [...output.matchAll(/\[([\w-]+)\] final opencode\.log: lines=(\d+) errors=(\d+) leaks=(\d+)/g)];
  if (stats.length === 0) fail("the live Rust test printed no opencode.log statistics");
  for (const [, label, lines, errors, leaks] of stats) {
    if (Number(errors) !== 0 || Number(leaks) !== 0) {
      fail(`[${label}] opencode.log errors=${errors} leaks=${leaks}`);
    }
    record(
      `rust live sidecar flow (${label})`,
      "ok",
      `opencode.log lines=${lines} errors=${errors} leaks=${leaks}`,
    );
  }
  if (/skipping: platform keychain unavailable/.test(output)) {
    record(
      "rust live sidecar flow (os-keychain)",
      "skipped",
      "kernel keyring (keyutils) unavailable in this environment; set BYOK_REQUIRE_OS_KEYCHAIN=1 to fail instead",
    );
  }
  if (!/test result: ok\. 3 passed/.test(output)) fail("expected all 3 live tests to pass");
}

// ───────────────────────── step 3: desktop boot smoke ─────────────────────────

async function desktopBootSmoke() {
  // The webview must load the embedded real UI (TauriBridge), not the Vite dev server,
  // so build the production frontend without the preview MockBridge first.
  const uiEnv = { ...process.env };
  delete uiEnv.VITE_SPIRIT_MOCK;
  await run("npm", ["run", "build", "-w", "@stackspirit/ui"], { env: uiEnv, quiet: true });
  await run("npm", ["run", "build", "-w", "@stackspirit/desktop"], { env: uiEnv, quiet: true });
  await run("scripts/cargo-gate.sh", ["build", "-p", "spirit-desktop", "--features", "custom-protocol"]);
  const binary = join(ROOT, "target/debug/spirit-desktop");
  if (!existsSync(binary)) fail(`desktop binary missing at ${binary}`);

  const home = mkdtempSync(join(tmpdir(), "stackspirit-xvfb-boot-"));
  const marker = home; // unique per run; also the isolated HOME of the child tree
  for (const name of ["data", "config", "state", "cache"]) mkdirSync(join(home, name));
  const pidFile = join(home, "app.pid");
  const env = {
    PATH: process.env.PATH,
    HOME: home,
    XDG_DATA_HOME: join(home, "data"),
    XDG_CONFIG_HOME: join(home, "config"),
    XDG_STATE_HOME: join(home, "state"),
    XDG_CACHE_HOME: join(home, "cache"),
    // Headless WebKitGTK inside containers.
    WEBKIT_DISABLE_COMPOSITING_MODE: "1",
    WEBKIT_DISABLE_DMABUF_RENDERER: "1",
    LIBGL_ALWAYS_SOFTWARE: "1",
    GDK_BACKEND: "x11",
    NO_AT_BRIDGE: "1",
  };
  const wrapper = join(home, "launch.sh");
  writeFileSync(wrapper, `#!/bin/sh\necho $$ > "${pidFile}"\nexec "${binary}"\n`, { mode: 0o700 });

  let stderr = "";
  const xvfb = spawn("xvfb-run", ["-a", wrapper], { env, stdio: ["ignore", "pipe", "pipe"] });
  xvfb.stdout.on("data", (chunk) => (stderr += chunk));
  xvfb.stderr.on("data", (chunk) => (stderr += chunk));
  const exited = new Promise((resolveExit) => xvfb.on("exit", (code, signal) => resolveExit({ code, signal })));

  try {
    // Wait for the app pid, then let it run.
    for (let i = 0; i < 50 && !existsSync(pidFile); i += 1) await delay(100);
    if (!existsSync(pidFile)) fail("desktop binary never launched under xvfb-run");
    const appPid = Number(readFileSync(pidFile, "utf8").trim());
    await Promise.race([delay(SMOKE_SECONDS * 1000), exited]);
    if (!pidAlive(appPid)) {
      fail(`desktop binary exited during the ${SMOKE_SECONDS}s boot window:\n${stderr.slice(-2000)}`);
    }
    const sidecars = processesWithEnv(home).filter((pid) => isOpencode(pid));
    process.kill(appPid, "SIGTERM");
    const outcome = await Promise.race([exited, delay(15_000).then(() => "timeout")]);
    if (outcome === "timeout") {
      process.kill(appPid, "SIGKILL");
      fail("desktop binary ignored SIGTERM for 15s");
    }
    if (pidAlive(appPid)) fail("desktop binary still alive after SIGTERM");

    // No orphan opencode sidecar may remain in the isolated environment. A sidecar that
    // is still shutting down gets a short grace period before it counts as an orphan.
    let orphans = processesWithEnv(home).filter((pid) => isOpencode(pid));
    for (let i = 0; i < 40 && orphans.length > 0; i += 1) {
      await delay(250);
      orphans = processesWithEnv(home).filter((pid) => isOpencode(pid));
    }
    for (const pid of orphans) process.kill(pid, "SIGKILL");
    if (orphans.length > 0) fail(`orphan opencode sidecar left behind (${orphans.length})`);

    const audit = auditLogs(home);
    if (audit.errors !== 0 || audit.leaks !== 0) {
      fail(`desktop opencode.log errors=${audit.errors} leaks=${audit.leaks}`);
    }
    if (/panicked at|fatal runtime error/i.test(stderr)) fail("desktop binary logged a panic");
    record(
      "desktop boot smoke under xvfb-run",
      "ok",
      `booted ${SMOKE_SECONDS}s, SIGTERM clean, no orphan opencode, sidecar seen during boot=${sidecars.length > 0}, ` +
        `opencode.log files=${audit.files} lines=${audit.lines} errors=${audit.errors} leaks=${audit.leaks}`,
    );
  } finally {
    if (xvfb.exitCode === null) xvfb.kill("SIGKILL");
    rmSync(home, { recursive: true, force: true });
  }
}

// ───────────────────────── step 4: Playwright ─────────────────────────

async function playwrightLive() {
  const output = await run(
    "xvfb-run",
    ["-a", "npx", "playwright", "test", "e2e/xvfb-byok-live.spec.ts", "--reporter=list"],
    { cwd: DESKTOP },
  );
  const stats = [...output.matchAll(/opencode\.log \((\w+)\): lines=(\d+) errors=(\d+) leaks=(\d+)/g)];
  if (stats.length === 0) fail("the Playwright spec printed no opencode.log statistics");
  for (const [, theme, lines, errors, leaks] of stats) {
    if (Number(errors) !== 0 || Number(leaks) !== 0) {
      fail(`[${theme}] opencode.log errors=${errors} leaks=${leaks}`);
    }
    record(`playwright live sidecar e2e (${theme})`, "ok", `opencode.log lines=${lines} errors=${errors} leaks=${leaks}`);
  }
  if (!/2 passed/.test(output)) fail("expected both theme runs to pass");
}

// ───────────────────────── main ─────────────────────────

let failure = null;
try {
  const only = process.env.BYOK_ONLY; // debugging aid: fetch | rust | boot | e2e
  const wanted = (name) => only === undefined || only === name;
  await fetchEngine();
  if (wanted("rust")) await rustLiveTest();
  if (wanted("boot")) await desktopBootSmoke();
  if (wanted("e2e")) await playwrightLive();
} catch (error) {
  failure = error;
  record("verification", "FAILED", error instanceof Error ? error.message.split("\n")[0] : String(error));
}

console.log("\n──────── BYOK Xvfb verification summary ────────");
for (const { step, status, detail } of summary) {
  console.log(`${status.padEnd(7)} ${step}${detail ? `\n          ${detail}` : ""}`);
}
if (failure) {
  console.error(failure instanceof Error ? failure.message : failure);
  process.exit(1);
}
console.log("\nAll BYOK Xvfb checks passed (no key leaked, no sidecar error).");
