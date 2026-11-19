#!/usr/bin/env node
// Orchestrator chat owner verification: static checks and the §9 checklist.
//
// This implements docs/ORCHESTRATOR-CHAT.md §8.7 and §9. It is deliberately inert:
//
//   * --check only reads repository files and runs the pinned sidecar's `--version`.
//     It never starts a server, sends a model turn, opens a socket or downloads a
//     sidecar. The version probe gets a private temporary HOME/XDG tree and a minimal
//     environment with no provider or server credential.
//   * --checklist prints owner-run steps and redacted evidence placeholders. It does
//     not execute them.
//   * the server half is printed as BLOCKED on VOWP 0.5 `vowp orchestrator endpoint`;
//     the app-owned SSH forward is not approved (decision A). It is never run by this
//     script.
//   * no `.env` file, provider key, token or keychain entry is read; no VOWP command is
//     invoked; no VOWP session or surface is created.
//
// The sidecar check fails closed: when the pinned binary is absent, --check exits 1 and
// says exactly what the owner must fetch. The script itself never downloads it.
//
// Usage:
//   node scripts/verify-orchestrator-chat.mjs --check
//   node scripts/verify-orchestrator-chat.mjs --checklist
//   node scripts/verify-orchestrator-chat.mjs --help
//
// Exit codes:
//   0  static checks passed, or the checklist was printed
//   1  a required static check failed or was blocked (including a missing sidecar)
//   2  usage error

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  accessSync,
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
} from "node:fs";
import { constants as fsConstants } from "node:fs";
import { hostname as osHostname, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const MANIFEST_PATH = join(REPO_ROOT, "engine", "manifest.json");
const BINARIES_DIR = join(
  REPO_ROOT,
  "apps",
  "spirit-desktop",
  "src-tauri",
  "binaries",
);
const LOCK_PATH = join(BINARIES_DIR, "engine-lock.json");
const LOOPBACK_HOST = "127.0.0.1";
const HEALTH_PATH = "/global/health";
// The view job named in this brief. Its report is worktree-local and gitignored;
// absence is a warning, never a claimed pass.
const VIEW_REPORT_PATH = join(
  REPO_ROOT,
  ".vowp",
  "jobs",
  "ss-46-chat-view",
  "report.md",
);

const USAGE = `StackSpirit orchestrator chat verification (docs/ORCHESTRATOR-CHAT.md §8.7/§9)

usage:
  node scripts/verify-orchestrator-chat.mjs --check
      Verify the static preconditions: the pinned sidecar is present, its version and
      checksum match the lock, the bundled engine never resolves opencode from PATH, the
      private XDG tree policy is present, and the expected Settings/engine diagnostics
      exist. This never starts a model or downloads a sidecar. It exits 1 if the pinned
      sidecar is absent.

  node scripts/verify-orchestrator-chat.mjs --checklist
      Print the owner-run §9.1 local setup steps, the §9.1b local global/project steps
      and the §9.2 server steps with exact commands and redacted evidence placeholders.
      The server section is blocked on VOWP 0.5's "vowp orchestrator endpoint"; the
      app-owned SSH forward is not approved (decision A) and must not be run yet.

  node scripts/verify-orchestrator-chat.mjs --help

No provider key, secret, VOWP command or model turn is used by either mode.`;

// ---------------------------------------------------------------------------
// Redaction
// ---------------------------------------------------------------------------

const redactions = [];

function addRedaction(from, to) {
  if (typeof from === "string" && from.length > 0) {
    redactions.push([from, to]);
  }
}

addRedaction(REPO_ROOT, "<REPO_ROOT>");
if (process.env.HOME) addRedaction(process.env.HOME, "<HOME>");
if (process.env.USERPROFILE) addRedaction(process.env.USERPROFILE, "<HOME>");

const HOSTNAME = osHostname();
const HOSTNAME_PATTERN =
  HOSTNAME && HOSTNAME.length >= 3
    ? new RegExp(`(?<![A-Za-z0-9])${escapeRegExp(HOSTNAME)}(?![A-Za-z0-9])`, "gi")
    : null;

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function redact(value) {
  let text = String(value);
  const pairs = [...redactions].sort(
    (left, right) => right[0].length - left[0].length,
  );
  for (const [from, to] of pairs) {
    text = text.split(from).join(to);
  }
  if (HOSTNAME_PATTERN) {
    text = text.replace(HOSTNAME_PATTERN, "<HOSTNAME>");
  }
  // Any remaining absolute-looking path is machine-specific. Keep only placeholders in
  // output and in the report.
  text = text.replace(/(?<![A-Za-z0-9._@+-])(?:\/[A-Za-z0-9._@+-]+){2,}/g, "<PATH>");
  text = text.replace(/\b[A-Za-z]:\\[^\s"']+/g, "<PATH>");
  text = text.replace(/\bses_[A-Za-z0-9]+\b/g, "<SESSION_ID>");
  return text;
}

function safeError(error) {
  if (error instanceof Error) return redact(error.message || error.name).slice(0, 300);
  return redact(String(error)).slice(0, 300);
}

// ---------------------------------------------------------------------------
// Small repository helpers
// ---------------------------------------------------------------------------

function hostTriple() {
  const arch = { arm64: "aarch64", x64: "x86_64" }[process.arch];
  if (!arch) return null;
  if (process.platform === "darwin") return `${arch}-apple-darwin`;
  if (process.platform === "linux") return `${arch}-unknown-linux-gnu`;
  if (process.platform === "win32") return `${arch}-pc-windows-msvc`;
  return null;
}

function binaryName(triple) {
  return `opencode-${triple}${triple.includes("windows") ? ".exe" : ""}`;
}

function repoFile(relativePath) {
  return join(REPO_ROOT, relativePath);
}

function readRepoText(relativePath) {
  try {
    return readFileSync(repoFile(relativePath), "utf8");
  } catch {
    return null;
  }
}

function stripTestModules(text) {
  const marker = text.indexOf("\n#[cfg(test)]");
  return marker >= 0 ? text.slice(0, marker) : text;
}

// Returns the brace-balanced body of the first `fn <name>(` definition that has a body.
// Trait declarations and other body-less signatures are skipped, so a check can assert a
// call's presence and a forbidden call's absence on one path without pinning a whole line.
function functionBody(text, name) {
  const pattern = new RegExp(`\\bfn\\s+${escapeRegExp(name)}\\s*[<(]`, "g");
  let match;
  while ((match = pattern.exec(text)) !== null) {
    const braceStart = text.indexOf("{", match.index);
    if (braceStart < 0) return null;
    const semicolon = text.indexOf(";", match.index);
    if (semicolon >= 0 && semicolon < braceStart) continue;
    let depth = 0;
    for (let index = braceStart; index < text.length; index += 1) {
      const character = text[index];
      if (character === "{") {
        depth += 1;
      } else if (character === "}") {
        depth -= 1;
        if (depth === 0) return text.slice(braceStart, index + 1);
      }
    }
    return null;
  }
  return null;
}

function sha256File(path) {
  const hash = createHash("sha256");
  hash.update(readFileSync(path));
  return hash.digest("hex");
}

function isExecutableFile(path) {
  try {
    const stat = statSync(path);
    if (!stat.isFile()) return false;
    if (process.platform !== "win32") accessSync(path, fsConstants.X_OK);
    return true;
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// Check result collection
// ---------------------------------------------------------------------------

const results = [];

function record(status, label, detail = "") {
  results.push({ status, label, detail });
}

function pass(label, detail = "") {
  record("pass", label, detail);
}

function warn(label, detail = "") {
  record("warn", label, detail);
}

function blocked(label, detail = "") {
  record("blocked", label, detail);
}

function fail(label, detail = "") {
  record("fail", label, detail);
}

// Checks a list of source files for required and forbidden tokens. A missing file is a
// failure; test modules are excluded when a check targets production launch code so a
// test helper cannot be mistaken for runtime behavior.
function checkSourcePolicy(label, requirements, detail = "") {
  const problems = [];
  for (const requirement of requirements) {
    const text = readRepoText(requirement.file);
    if (text === null) {
      problems.push(`${requirement.file} is missing`);
      continue;
    }
    const haystack = requirement.ignoreTests ? stripTestModules(text) : text;
    for (const token of requirement.required ?? []) {
      if (!haystack.includes(token)) {
        problems.push(`${requirement.file} is missing ${JSON.stringify(token)}`);
      }
    }
    for (const token of requirement.forbidden ?? []) {
      if (haystack.includes(token)) {
        problems.push(`${requirement.file} contains forbidden ${JSON.stringify(token)}`);
      }
    }
    for (const pattern of requirement.forbiddenPatterns ?? []) {
      if (pattern.test(haystack)) {
        problems.push(`${requirement.file} matches forbidden ${pattern}`);
      }
    }
    for (const definition of requirement.functions ?? []) {
      const body = functionBody(haystack, definition.name);
      if (body === null) {
        problems.push(`${requirement.file} has no ${definition.name}() body`);
        continue;
      }
      for (const token of definition.required ?? []) {
        if (!body.includes(token)) {
          problems.push(
            `${requirement.file} ${definition.name}() is missing ${JSON.stringify(token)}`,
          );
        }
      }
      for (const token of definition.forbidden ?? []) {
        if (body.includes(token)) {
          problems.push(
            `${requirement.file} ${definition.name}() contains forbidden ${JSON.stringify(token)}`,
          );
        }
      }
    }
  }
  if (problems.length > 0) {
    fail(label, problems.join("; "));
    return false;
  }
  pass(label, detail);
  return true;
}

// ---------------------------------------------------------------------------
// Manifest, lock, sidecar
// ---------------------------------------------------------------------------

function loadManifest(triple) {
  if (!existsSync(MANIFEST_PATH)) {
    fail("engine manifest", "engine/manifest.json is missing");
    return null;
  }
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(MANIFEST_PATH, "utf8"));
  } catch {
    fail("engine manifest", "engine/manifest.json is not valid JSON");
    return null;
  }
  const problems = [];
  if (manifest.schema !== "stackspirit-engine-manifest/1") {
    problems.push("unexpected schema");
  }
  if (manifest.product !== "opencode") problems.push("product is not opencode");
  if (!/^\d+\.\d+\.\d+$/.test(manifest.version ?? "")) {
    problems.push("version is not an exact semver release");
  }
  if (manifest.serve?.loopback_host !== LOOPBACK_HOST) {
    problems.push(`serve.loopback_host is not ${LOOPBACK_HOST}`);
  }
  if (manifest.serve?.health_path !== HEALTH_PATH) {
    problems.push(`serve.health_path is not ${HEALTH_PATH}`);
  }
  const serveArgs = manifest.serve?.args;
  if (
    !Array.isArray(serveArgs) ||
    serveArgs.length !== 3 ||
    serveArgs[0] !== "serve" ||
    serveArgs[1] !== "--hostname" ||
    serveArgs[2] !== LOOPBACK_HOST
  ) {
    problems.push("serve.args is not exactly serve --hostname 127.0.0.1");
  }
  if (triple) {
    const target = manifest.targets?.[triple];
    if (!target) {
      problems.push(`no pinned target for ${triple}`);
    } else if (!/^[a-f0-9]{64}$/.test(target.sha256 ?? "")) {
      problems.push(`target ${triple} has no pinned SHA-256`);
    }
  } else {
    problems.push("this host has no supported target triple");
  }
  if (problems.length > 0) {
    fail("engine manifest", problems.join("; "));
    return null;
  }
  pass(
    "engine manifest",
    `${manifest.product} ${manifest.version} pinned for ${triple}; loopback ${LOOPBACK_HOST}${HEALTH_PATH}`,
  );
  return manifest;
}

function loadLock(manifest, triple) {
  if (!manifest) {
    blocked("engine lock", "not checked because the engine manifest is unavailable");
    return null;
  }
  if (!existsSync(LOCK_PATH)) {
    blocked(
      "engine lock",
      "apps/spirit-desktop/src-tauri/binaries/engine-lock.json is absent; run node scripts/fetch-engine.mjs",
    );
    return null;
  }
  let lock;
  try {
    lock = JSON.parse(readFileSync(LOCK_PATH, "utf8"));
  } catch {
    fail("engine lock", "engine-lock.json is not valid JSON");
    return null;
  }
  const problems = [];
  if (lock.schema !== "stackspirit-engine-lock/1") problems.push("unexpected schema");
  const target = manifest.targets?.[triple];
  const entry = lock.targets?.[triple];
  if (!entry) {
    problems.push(`no lock entry for ${triple}`);
  } else {
    if (entry.version !== manifest.version) {
      problems.push(`lock version ${entry.version} does not match manifest ${manifest.version}`);
    }
    if (entry.asset !== target?.asset) {
      problems.push("lock asset does not match manifest");
    }
    if (entry.archive_sha256 !== target?.sha256) {
      problems.push("lock archive SHA-256 does not match manifest");
    }
    if (!/^[a-f0-9]{64}$/.test(entry.binary_sha256 ?? "")) {
      problems.push("lock binary SHA-256 is missing");
    }
  }
  if (problems.length > 0) {
    fail("engine lock", problems.join("; "));
    return null;
  }
  pass("engine lock", `version ${manifest.version}; archive and binary digests recorded`);
  return lock;
}

function checkSidecarPresent(triple) {
  if (!triple) {
    blocked("pinned sidecar", "cannot resolve a target triple for this host");
    return null;
  }
  const binary = join(BINARIES_DIR, binaryName(triple));
  if (!existsSync(binary)) {
    blocked(
      "pinned sidecar",
      `absent at apps/spirit-desktop/src-tauri/binaries/${binaryName(triple)}; run node scripts/fetch-engine.mjs --target ${triple} (this script never downloads it)`,
    );
    return null;
  }
  if (!isExecutableFile(binary)) {
    fail(
      "pinned sidecar",
      `apps/spirit-desktop/src-tauri/binaries/${binaryName(triple)} is not an executable file`,
    );
    return null;
  }
  pass(
    "pinned sidecar",
    `${binaryName(triple)} is present and executable; PATH is not used`,
  );
  return binary;
}

function checkSidecarChecksum(binary, lock, triple) {
  if (!binary) {
    blocked("sidecar checksum", "not checked because the pinned sidecar is absent");
    return;
  }
  if (!lock || !triple) {
    blocked("sidecar checksum", "not checked because engine-lock.json is unavailable");
    return;
  }
  const expected = lock.targets?.[triple]?.binary_sha256;
  if (!expected) {
    fail("sidecar checksum", `engine-lock.json has no binary digest for ${triple}`);
    return;
  }
  let actual;
  try {
    actual = sha256File(binary);
  } catch (error) {
    fail("sidecar checksum", `could not hash the sidecar: ${safeError(error)}`);
    return;
  }
  if (actual !== expected) {
    fail(
      "sidecar checksum",
      `the extracted binary digest does not match engine-lock.json for ${triple}`,
    );
    return;
  }
  pass("sidecar checksum", `binary SHA-256 matches engine-lock.json for ${triple}`);
}

function privateEnv(root) {
  const home = join(root, "home");
  const env = {
    // The executable is always spawned by its absolute pinned path. PATH is passed only
    // for the child's own tooling; it is never used to look up `opencode`.
    PATH: process.env.PATH ?? "",
    HOME: home,
    USERPROFILE: home,
    XDG_DATA_HOME: join(root, "data"),
    XDG_STATE_HOME: join(root, "state"),
    XDG_CONFIG_HOME: join(root, "config"),
    XDG_CACHE_HOME: join(root, "cache"),
    XDG_RUNTIME_DIR: join(root, "run"),
    TMPDIR: join(root, "tmp"),
    TEMP: join(root, "tmp"),
    TMP: join(root, "tmp"),
    APPDATA: join(root, "config"),
    LOCALAPPDATA: join(root, "data"),
    OPENCODE_DISABLE_MODELS_FETCH: "1",
    OPENCODE_DISABLE_AUTOUPDATE: "1",
    NO_COLOR: "1",
  };
  if (process.env.SystemRoot) env.SystemRoot = process.env.SystemRoot;
  if (process.env.ComSpec) env.ComSpec = process.env.ComSpec;
  if (process.env.PATHEXT) env.PATHEXT = process.env.PATHEXT;
  if (process.env.LANG) env.LANG = process.env.LANG;
  return env;
}

function checkSidecarVersion(binary, manifest) {
  if (!binary) {
    blocked("pinned sidecar version", "not checked because the pinned sidecar is absent");
    return;
  }
  if (!manifest) {
    blocked("pinned sidecar version", "not checked because the engine manifest is unavailable");
    return;
  }
  let root;
  try {
    root = mkdtempSync(join(tmpdir(), "stackspirit-chat-check-"));
    if (process.platform !== "win32") {
      try {
        chmodSync(root, 0o700);
      } catch {
        // mkdtemp already creates a private directory on POSIX.
      }
    }
    for (const name of [
      "home",
      "data",
      "state",
      "config",
      "cache",
      "run",
      "tmp",
    ]) {
      mkdirSync(join(root, name), { recursive: true, mode: 0o700 });
    }
    const probe = spawnSync(binary, ["--version"], {
      cwd: root,
      env: privateEnv(root),
      encoding: "utf8",
      timeout: 15_000,
      maxBuffer: 64 * 1024,
    });
    if (probe.error) {
      fail("pinned sidecar version", `could not run the sidecar: ${safeError(probe.error)}`);
      return;
    }
    if (probe.status !== 0) {
      fail(
        "pinned sidecar version",
        `--version exited ${probe.status ?? "timeout"}: ${redact(
          `${probe.stdout ?? ""}${probe.stderr ?? ""}`.trim().split(/\r?\n/)[0] ?? "",
        )}`,
      );
      return;
    }
    const output = `${probe.stdout ?? ""}${probe.stderr ?? ""}`.trim();
    const tokens = output.split(/[\s()]+/).filter(Boolean);
    const expected = manifest.version;
    if (!tokens.includes(expected) && !tokens.includes(`v${expected}`)) {
      const firstLine = redact(output.split(/\r?\n/)[0] ?? "");
      fail("pinned sidecar version", `expected ${expected}; got ${firstLine}`);
      return;
    }
    pass("pinned sidecar version", `--version reports ${expected}`);
  } catch (error) {
    fail("pinned sidecar version", `version probe failed: ${safeError(error)}`);
  } finally {
    if (root) rmSync(root, { recursive: true, force: true });
  }
}

// ---------------------------------------------------------------------------
// Static policy checks
// ---------------------------------------------------------------------------

function checkEndpointResolver() {
  checkSourcePolicy(
    "local endpoint resolver",
    [
      {
        file: "crates/spirit-core/src/engine/chat/endpoint.rs",
        ignoreTests: true,
        required: [
          "pub struct VowpEndpointResolver",
          'Command::new("vowp")',
          '.args(["agent", "orchestrator", "endpoint", name, "--json"])',
          "tokio::process::Command::env_clear(&mut command)",
          "tokio::time::timeout(ENDPOINT_TIMEOUT, child.wait_with_output())",
          "Some(2) => not_configured(",
          "no `agent orchestrator endpoint` command",
          "has no loopback endpoint; start or resume it in VOWP",
          "StackSpirit chat only speaks opencode",
          "reported a different home session than the attached log",
        ],
        forbidden: [
          'Command::new("sh")',
          'Command::new("bash")',
          'Command::new("/bin/sh")',
          'Command::new("cmd")',
        ],
        functions: [
          {
            name: "run",
            required: [
              'Command::new("vowp")',
              '.args(["agent", "orchestrator", "endpoint", name, "--json"])',
              "env_clear(&mut command)",
              "tokio::time::timeout(ENDPOINT_TIMEOUT",
              "map_err(|_| not_configured(",
              "the installed VOWP agent command failed",
              "the installed VOWP agent command timed out",
            ],
            forbidden: ['"sh"', '"/bin/sh"', '"bash"', '"-c"'],
          },
          {
            name: "not_configured",
            required: ['"chat_not_configured:'],
          },
          {
            name: "gated",
            required: ['"chat_gated:'],
          },
        ],
      },
    ],
    "the Local endpoint is read with argv only, a bounded timeout and typed chat_not_configured/chat_gated refusals; an absent subcommand is chat_not_configured",
  );
}

function checkBundledResolution() {
  checkSourcePolicy(
    "bundled engine resolution",
    [
      {
        file: "crates/spirit-core/src/engine/process.rs",
        ignoreTests: true,
        required: [
          "pub struct EngineBinaryLocator",
          "pub fn for_current_exe",
          "names.push(sidecar_file_name(triple))",
          "for directory in &self.directories",
          "let binary = self.resolve_binary()?;",
          "Command::new(&binary)",
        ],
        forbidden: [
          'Command::new("opencode")',
          'Command::new("opencode.exe")',
          'env::var("PATH")',
          'env::var_os("PATH")',
        ],
      },
      {
        file: "scripts/fetch-engine.mjs",
        required: [
          "releases/download/v${manifest.version}/${target.asset}",
          "digest !== target.sha256",
          "engine-lock.json",
        ],
      },
      {
        file: "scripts/verify-engine-checksum.sh",
        required: ["verify_checksum", "engine-lock.json", "--self-test"],
      },
    ],
    "the bundled sidecar is resolved next to the app executable, never from PATH; the fetch is pinned and checksum-verified",
  );
}

function checkSingleEngine() {
  checkSourcePolicy(
    "single engine and chat reuse",
    [
      {
        file: "apps/spirit-desktop/src-tauri/src/state.rs",
        required: [
          "ProcessEngineRuntime::for_current_exe()",
          ".with_private_state(app_data_dir.clone())",
          "LocalChatProvider::new(",
          "DesktopChatEngine::new(engine.clone())",
          "setup_workspace",
        ],
        forbidden: ["ProcessEngineRuntime::spawn(", "reserve_loopback_port("],
      },
      {
        file: "crates/spirit-core/src/engine/chat/local.rs",
        ignoreTests: true,
        required: ["pub trait ChatEngine", "async fn ensure_ready"],
        forbidden: [
          "Command::new(",
          "ProcessEngineRuntime::spawn(",
          "reserve_loopback_port(",
        ],
        functions: [
          {
            name: "ensure_engine",
            required: [
              "EngineChoice::External",
              "engine_external_error()",
              "self.engine.ensure_ready().await",
            ],
          },
          {
            name: "start_setup",
            required: [
              "ensure_engine().await",
              "ChatEndpoint::engine(port, &self.inner.workspace)",
              "create_native_session(&endpoint).await?",
            ],
            forbidden: ["resolve_local_endpoint"],
          },
        ],
      },
      {
        file: "apps/spirit-desktop/src-tauri/src/commands/chat.rs",
        required: ["state.chat()"],
        forbidden: [
          "Command::new(",
          "reserve_loopback_port(",
          "ProcessEngineRuntime::spawn(",
        ],
      },
    ],
    "setup uses the one bundled supervisor; Local global/project only attaches and never starts a second engine",
  );
}

function checkLocalOrchestrator() {
  checkSourcePolicy(
    "local orchestrator attach",
    [
      {
        file: "crates/spirit-core/src/engine/chat/local.rs",
        ignoreTests: true,
        required: [
          "resolve_local_endpoint(name, existing.as_deref())",
          "discover_native_session(&endpoint, name).await?",
          "entry_updated(entry)",
        ],
        functions: [
          {
            name: "start",
            required: [
              "self.start_setup(input).await",
              "self.start_orchestrator(input).await",
            ],
            forbidden: [
              "create_native_session",
              "ensure_engine",
              "ChatEndpoint::engine",
              '"POST"',
            ],
          },
          {
            name: "orchestrator_name",
            required: [
              'ChatSessionId::Global => Some("global")',
              "ChatSessionId::Project(name) => Some(name)",
            ],
          },
          {
            name: "start_orchestrator",
            required: [
              "if !input.resumes()",
              "resolve_local_endpoint(name, existing.as_deref())",
              ".open_orchestrator(name, &input.session, resolved)",
            ],
            forbidden: [
              "create_native_session",
              "ensure_engine",
              "ChatEndpoint::engine",
              '"POST"',
            ],
          },
          {
            name: "open_orchestrator",
            required: [
              "Some(native) => native",
              "discover_native_session(&endpoint, name).await?",
              "get_native_session(&endpoint, &native).await",
            ],
            forbidden: [
              "create_native_session",
              "ensure_engine",
              "ChatEndpoint::engine",
              '"POST"',
            ],
          },
          {
            name: "attach_orchestrator",
            required: [
              "resolve_local_endpoint(name, existing.as_deref())",
              "open_orchestrator(name, session, resolved)",
            ],
            forbidden: [
              "create_native_session",
              "ensure_engine",
              "ChatEndpoint::engine",
              '"POST"',
            ],
          },
          {
            name: "discover_native_session",
            required: ['"/api/session"'],
            forbidden: ['"POST"', "create_native_session"],
          },
          {
            name: "parse_discovered_session",
            required: [
              "entry_updated(entry)",
              "newest.as_ref().is_none_or(|(best, _, _)| updated > *best)",
            ],
          },
          {
            name: "entry_updated",
            required: ['.get("time")', '.get("updated")'],
          },
        ],
      },
    ],
    "Local global/project resolves the VOWP endpoint, continues the newest host session when no native id is reported, and never creates a native session or starts the bundled engine",
  );
}

function checkPrivateTree() {
  checkSourcePolicy(
    "private XDG tree policy",
    [
      {
        file: "crates/spirit-core/src/engine/process.rs",
        required: [
          "pub struct PrivateEngineTree",
          "pub fn prepare(app_data: &Path)",
          'engine.join("data")',
          'engine.join("state")',
          'engine.join("config")',
          'workspaces.join("setup")',
          'workspaces.join("global")',
          'workspaces.join("project")',
          '"XDG_DATA_HOME", &tree.data',
          '"XDG_STATE_HOME", &tree.state',
          '"XDG_CONFIG_HOME", &tree.config',
          "command.current_dir(&tree.setup_workspace)",
          "Permissions::from_mode(0o700)",
          "Permissions::from_mode(0o600)",
          'env_remove(&mut command, "OPENCODE_SERVER_PASSWORD")',
          'env_remove(&mut command, "OPENCODE_SERVER_USERNAME")',
        ],
      },
      {
        file: "apps/spirit-desktop/src-tauri/src/state.rs",
        required: [
          ".with_private_state(app_data_dir.clone())",
          "PrivateEngineTree::prepare(&app_data_dir)?.setup_workspace",
        ],
      },
    ],
    "engine data/state/config and workspaces are app-private 0700; setup AGENTS.md is 0600; server credentials are removed",
  );
}

function checkSettingsDiagnostics() {
  checkSourcePolicy(
    "Settings and engine diagnostics",
    [
      {
        file: "packages/spirit-ui/src/next/SettingsView.tsx",
        required: [
          "<EngineView />",
          "Show advanced settings",
          "StackSpirit Engine",
          "Used for the setup assistant and local sessions",
        ],
      },
      {
        file: "packages/spirit-ui/src/views/engine/EngineStatusCard.tsx",
        required: [
          'data-testid="engine-status-card"',
          "Loopback port",
          "status.binary_path",
          "Advanced details",
          "Engine details",
          "View logs",
          "Restart engine",
        ],
      },
      {
        file: "apps/spirit-desktop/src-tauri/src/commands/engine.rs",
        required: [
          "pub async fn engine_status",
          "pub async fn scan_external",
          "pub async fn select_external",
          "pub async fn use_bundled",
          "pub async fn restart_engine",
        ],
      },
      {
        file: "crates/spirit-core/src/engine/runtime.rs",
        required: [
          "pub struct EngineRuntimeInfo",
          "pub port: u16",
          "pub version: Option<String>",
          "pub binary_path: Option<std::path::PathBuf>",
        ],
      },
      {
        file: "packages/spirit-ui/src/views/engine/EngineView.tsx",
        required: [
          'data-testid="engine-logs"',
          'data-testid="engine-logs-empty"',
          "ExternalHarnessPicker",
          "scanExternal",
          "selectExternal",
          "useBundled",
        ],
      },
    ],
    "Advanced engine details expose the loopback port and binary path; logs, restart and the external harness picker are present",
  );
}

function checkChatDiagnostics() {
  checkSourcePolicy(
    "chat location, state and error diagnostics",
    [
      {
        file: "packages/spirit-ui/src/next/ChatView.tsx",
        required: [
          'data-testid="session-location"',
          'data-testid="session-state"',
          'data-testid="chat-error"',
          "data-code={error.code}",
          "data-gate=",
          "Local · this Mac",
          "Server · ",
          "Not connected",
          'data-testid="preview-tag"',
          "Reconnect",
          "Stop",
        ],
      },
      {
        file: "packages/spirit-ui/src/next/conversations.ts",
        required: [
          "chat_unavailable:",
          "engine_not_ready:",
          "host_disconnected:",
          "stop_unconfirmed:",
          "chat_gated:",
          '!bridge.preview && event.message.origin === "preview"',
        ],
      },
      {
        file: "crates/spirit-core/src/engine/chat/local.rs",
        required: [
          "engine_external_error",
          "engine_not_ready_error",
          "chat_gated_error",
          "stop_unconfirmed_error",
        ],
      },
    ],
    "the chip, state, typed errors and preview suppression are wired for the owner scan",
  );
}

function checkServerGate() {
  checkSourcePolicy(
    "server chat remains gated",
    [
      {
        file: "crates/spirit-core/src/engine/chat/local.rs",
        ignoreTests: true,
        functions: [
          {
            name: "validate_start",
            required: [
              "(ChatSessionId::Setup, ChatSessionLocation::Server) => Err(chat_gated_error(",
              "(ChatSessionId::Global | ChatSessionId::Project(_), ChatSessionLocation::Server) => {",
              "this transport does not open SSH",
            ],
            forbidden: [
              "(ChatSessionId::Global | ChatSessionId::Project(_), ChatSessionLocation::Server) => Ok(",
            ],
          },
          {
            name: "chat_gated_error",
            required: ['format!("chat_gated: {detail}")'],
          },
        ],
      },
      {
        file: "apps/spirit-desktop/src-tauri/src/commands/chat.rs",
        required: ["`global` and `project:<name>` stay gated"],
      },
    ],
    "server global/project return the typed chat_gated gate (decision A); the setup assistant is local-only",
  );
}

function checkViewTests() {
  checkSourcePolicy(
    "view job test presence",
    [
      {
        file: "packages/spirit-ui/src/test/next/nextShell.test.tsx",
        required: [
          "sends setup chat through bridge.chat",
          "chat_unavailable",
          "never renders a preview reply",
          "disables the composer while streaming",
          "Stop to bridge.chat",
          "shows the exact VOWP gate",
        ],
      },
      {
        file: "apps/spirit-desktop/e2e/next-shell.spec.ts",
        required: [
          "setup assistant sends through chat",
          "server chat shows the honest VOWP gate",
        ],
      },
    ],
    "setup, preview honesty, stop, error/gate and e2e coverage are present; passing is not claimed here",
  );
}

function checkViewReport() {
  if (!existsSync(VIEW_REPORT_PATH)) {
    warn(
      "view job report",
      "ss-46-chat-view report.md is not present in this worktree; its gate results stay unverified",
    );
    return;
  }
  let status = "unknown";
  try {
    const text = readFileSync(VIEW_REPORT_PATH, "utf8");
    const frontMatter = text.match(/^---\r?\n([\s\S]*?)\r?\n---/);
    const statusMatch = frontMatter?.[1]?.match(/^status:\s*(\S+)/m);
    if (statusMatch) status = statusMatch[1];
  } catch {
    // A missing or unreadable report is handled as a warning, not a claim.
  }
  if (status === "done") {
    pass("view job report", "ss-46-chat-view reports status: done; read it for the gate evidence");
  } else {
    warn(
      "view job report",
      `ss-46-chat-view status is ${status}; its gate results stay unverified`,
    );
  }
}

function checkServerTransport() {
  const serverTransport = existsSync(
    repoFile(join("crates", "spirit-core", "src", "engine", "chat", "server.rs")),
  );
  const sshForward = existsSync(
    repoFile(join("crates", "spirit-core", "src", "ssh", "forward.rs")),
  );
  const chatModule = readRepoText(
    join("crates", "spirit-core", "src", "engine", "chat", "mod.rs"),
  ) ?? "";
  const sshModule = readRepoText(join("crates", "spirit-core", "src", "ssh", "mod.rs")) ?? "";
  const wired =
    /pub\s+mod\s+server\b/.test(chatModule) || /pub\s+mod\s+forward\b/.test(sshModule);
  if (serverTransport || sshForward || wired) {
    fail(
      "server transport",
      "server/forward files or module wiring are present; the server half must stay gated until a new owner decision",
    );
    return;
  }
  pass(
    "server transport",
    "no server or SSH-forward transport in this tree; the server half stays gated",
  );
}

function checkContract() {
  checkSourcePolicy(
    "contract references",
    [
      {
        file: "docs/ORCHESTRATOR-CHAT.md",
        required: [
          "### 8.7 `chat-owner-verification`",
          "### 9.1 Local setup assistant",
          "### 9.2 Server chat",
          "### 9.4 Fail criteria",
          "vowp orchestrator endpoint",
          "vowp agent orchestrator endpoint",
          "not approved",
          "decision **A**",
        ],
        forbidden: ["owner-approved"],
      },
    ],
    "the checklist follows the pinned contract sections and decision A: the app-owned SSH forward is not approved",
  );
}

// ---------------------------------------------------------------------------
// --check
// ---------------------------------------------------------------------------

function printCheckHeader(triple, manifest) {
  console.log("StackSpirit orchestrator chat verification — static check");
  console.log("docs/ORCHESTRATOR-CHAT.md §8.7/§9; read-only; no model turn");
  console.log("");
  console.log(`Repository: <REPO_ROOT>`);
  console.log(`Host triple: ${triple ?? "<unsupported>"}`);
  console.log(
    `Manifest pin: ${manifest ? `${manifest.product} ${manifest.version}` : "<unavailable>"}`,
  );
  console.log("");
}

function printResults() {
  const labelWidth = Math.max(...results.map((item) => item.label.length), 1);
  for (const item of results) {
    const tag = `[${item.status.toUpperCase()}]`.padEnd(10);
    const label = item.label.padEnd(labelWidth);
    console.log(`${tag} ${label} ${redact(item.detail)}`.trimEnd());
  }
}

function printSummary() {
  const counts = { pass: 0, warn: 0, blocked: 0, fail: 0 };
  for (const item of results) counts[item.status] += 1;
  const failed = counts.fail + counts.blocked;
  console.log("");
  console.log(
    `Summary: ${counts.pass} pass, ${counts.fail} fail, ${counts.blocked} blocked, ${counts.warn} warn`,
  );
  if (failed > 0) {
    console.log(
      "Result: FAIL (fail closed; a blocked check is not a pass and no model turn was attempted)",
    );
  } else {
    console.log("Result: PASS (static preconditions only; owner verification still required)");
  }
  console.log(
    "Owner checklist: node scripts/verify-orchestrator-chat.mjs --checklist",
  );
  console.log("");
  console.log(
    "No model turn, network download, VOWP command, secret read or surface pairing was performed.",
  );
  console.log("Server half: BLOCKED on VOWP 0.5 `vowp orchestrator endpoint`;");
  console.log(
    "the app-owned SSH forward is not approved (decision A). This check did not run a server turn.",
  );
  return failed > 0 ? 1 : 0;
}

function runCheck() {
  const triple = hostTriple();
  const manifest = loadManifest(triple);
  printCheckHeader(triple, manifest);

  const lock = loadLock(manifest, triple);
  const binary = checkSidecarPresent(triple);
  checkSidecarChecksum(binary, lock, triple);
  checkSidecarVersion(binary, manifest);

  checkBundledResolution();
  checkEndpointResolver();
  checkSingleEngine();
  checkLocalOrchestrator();
  checkPrivateTree();
  checkSettingsDiagnostics();
  checkChatDiagnostics();
  checkServerGate();
  checkViewTests();
  checkViewReport();
  checkServerTransport();
  checkContract();

  printResults();
  return printSummary();
}

// ---------------------------------------------------------------------------
// --checklist
// ---------------------------------------------------------------------------

const CHECKLIST = String.raw`StackSpirit orchestrator chat owner checklist
docs/ORCHESTRATOR-CHAT.md §8.7/§9

This is an OWNER-RUN checklist. Nothing below was executed by this script; it only
prints the steps and the evidence placeholders. The worker did not start a model,
download a sidecar, read a secret, create a VOWP session or pair a surface.

Before starting:
  * Use only free included models. A3 records the required one-turn check; A4 uses the
    same free model for the stop test. Do not run a paid model.
  * Never paste a provider key, token, .env value, host, path, port, session id or SSH
    identity into the report. Replace every value with an angle-bracket placeholder.
  * Do not create a VOWP session or pair a surface unless the owner explicitly asks for
    that verification.
  * Run the static check first. It fails closed when the pinned sidecar is absent:
        node scripts/verify-orchestrator-chat.mjs --check
  * If the sidecar is absent, fetching it is owner-run and is never done by this script:
        TARGET_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
        node scripts/fetch-engine.mjs --target "$TARGET_TRIPLE"
        bash scripts/verify-engine-checksum.sh
        node scripts/verify-orchestrator-chat.mjs --check

=== A. Local setup assistant (§9.1) — OWNER-RUN ===

A1. Build and run the desktop app with the pinned sidecar present.
    Command:
      (cd apps/spirit-desktop && npx tauri dev)
    Prerequisite:
      node scripts/verify-orchestrator-chat.mjs --check
    Expected: the app opens with the real Tauri bridge (not MockBridge); the static check
    reports the pinned sidecar version and checksum, or the exact missing-sidecar reason.
    Evidence:
      build: <DEBUG_OR_RELEASE_BUILD>
      sidecar: <TARGET_TRIPLE> <VERSION>
      static check: <PASS_OR_BLOCKED_WITH_REASON>

A2. Open the setup assistant from the welcome page. Confirm one bundled engine child only,
    health is green, and the chip is Local · this Mac.
    Read-only process check:
      ps -Ao pid=,ppid=,command= | grep -E 'opencode-[^ ]+ serve --hostname 127\.0\.0\.1' | grep -v grep
    Expected: exactly one child, spawned by the app, with an absolute bundled-sidecar
    name and only loopback binds. A PATH opencode or a second serve is a FAIL.
    Do not paste the real command line or PID into the report.
    Evidence:
      process count: <1>
      binary: <BUNDLED_SIDECAR_NAME>
      chip: Local · this Mac
      health: <GREEN_OR_ENGINE_VERSION>

A3. Send one short message using a free included model.
    Prompt (copy exactly):
      Reply with exactly: STACKSPIRIT-CHAT-OK
    Expected: the reply is model-origin, is not labelled Preview, and either appears
    progressively or shows the honest "reply appears when the turn completes" note.
    Record only what you observed. Do not describe the reply as token-by-token unless
    message.part.delta was actually observed.
    Evidence:
      selected model: <FREE_INCLUDED_MODEL_LABEL>
      user text: STACKSPIRIT-CHAT-OK
      reply: <ASSISTANT_REPLY>
      origin: model (preview must be absent)
      progressive text: <YES_NO>
      non-streaming fallback note: <YES_NO_OR_NA>
      message.part.delta: <EMITTED_NOT_OBSERVED>
      message.part.updated payload: <REDACTED_KEYS_ONLY_OR_NOT_OBSERVED>
      session.idle ordering: <OBSERVED_ORDER_OR_NOT_OBSERVED>
      messageID preserved: <YES_NO_OR_NOT_OBSERVED>

A4. Press Stop during a long turn.
    Prompt (copy exactly):
      Count slowly from 1 to 50, one number per line.
    Expected: state Stopped, one assistant message, no duplicate; either a clean abort
    or the honest stop_unconfirmed note. Do not retry the same turn.
    Evidence:
      stop observed: <YES_NO>
      state: <STOPPED_OR_STOP_UNCONFIRMED>
      duplicate assistant messages: <0>

A5. Reload or restart the app.
    Command:
      (cd apps/spirit-desktop && npx tauri dev)
    Expected: the setup session reattaches, the log is intact, and there are no duplicate
    user or assistant messages. The native session id stays core-only.
    Evidence:
      messages before/after: <N_BEFORE>/<N_AFTER>
      duplicates: <0>
      chip: Local · this Mac

A6. Scan the DOM, console, network and visible error text.
    Open DevTools in the desktop app. In the console, copy the live DOM:
      copy(document.documentElement.outerHTML)
    Search the DOM, console, network panel and error text for all of:
      127.0.0.1
      localhost
      :<PORT> (replace <PORT> with the chat port seen in Settings; never paste it)
      Authorization
      Basic
      Bearer
      provider-token prefixes (sk-, pk_, ghp_, xox)
      -----BEGIN
      AKIA
      <SERVER_CREDENTIAL_ENV_NAME> (the engine server credential variable)
      OPENCODE_AUTH_CONTENT
      opencode-
      binaries
      /Users/
      /home/
      C:\
      engine/data
      engine/state
      engine/config
      workspaces
      <NATIVE_SESSION_ID> (any id with the native session-id prefix)
    Expected: the chat panel, message text, console, network panel and Tauri errors
    contain none of them. Only Settings → Advanced → Engine → Advanced details may show
    the loopback port and bundled binary path; that is an existing engine diagnostic.
    Evidence:
      DOM matches: <0>
      console matches: <0>
      network matches: <0>
      error-text matches: <0>
      advanced-engine diagnostic: <LOOPBACK_PORT_AND_BINARY_ONLY>

A7. Restart the engine while chat is open, then stop it and try a turn.
    UI: Settings → Advanced → Engine → Restart engine.
    Expected: chat shows a brief Connecting state, reattaches, and does not duplicate the
    conversation. Then stop the one bundled child and try a turn:
      kill <BUNDLED_CHILD_PID>    # PID from A2; never paste the real PID
    Expected: chat shows engine_not_ready, not a fake or preview reply. Restart from
    Settings afterwards.
    Evidence:
      restart: <REATTACHED_NO_DUPLICATES>
      stop: <ENGINE_NOT_READY>
      fallback reply: <NONE>

A8. Select an external harness in Settings and reopen setup chat.
    UI: Settings → Advanced → Engine → Existing harness → Scan → select
    <EXTERNAL_HARNESS>. Reopen the setup assistant.
    Expected: setup chat shows engine_external; it does not silently start the bundled
    engine and does not show a fake reply. Choose "Use bundled engine" to restore.
    Evidence:
      external selection: <EXTERNAL_HARNESS_LABEL>
      setup chat error: <ENGINE_EXTERNAL>
      bundled engine restored: <YES_NO>

=== A-Local. Local global/project (§9.1b) — OWNER-RUN ON A MAC ===

Run this block only on the Mac that hosts the VOWP local agent, with VOWP's
"vowp agent orchestrator endpoint <name> --json" command available. This is the live
Local global/project flow added by ss-61/ss-62. It never creates a VOWP session, pairs a
surface or runs a paid model. Use one free included model turn.

L1. Confirm VOWP can resolve the local orchestrator endpoint (command presence only):
      vowp agent orchestrator endpoint <ORCHESTRATOR_NAME> --json
    Expected: a JSON endpoint document. Do not paste its URL, port, auth, VOWP session id
    or native id. A missing subcommand or a command failure must show chat_not_configured
    in StackSpirit, not a local fallback.

L2. Open Local "global" in StackSpirit.
    UI: choose the Local orchestrator named "global".
    Expected: the chip is Local · this Mac; the existing orchestrator conversation loads;
    the composer is enabled only after ready. No new conversation is created.
    Evidence:
      chip: Local · this Mac
      existing conversation loaded: <YES_NO>
      state: <READY_OR_CONNECTING>
      messages shown: <N>

L3. Capture the native session id before the turn from the same endpoint read:
      vowp agent orchestrator endpoint global --json
    Read only the native session id; never paste any other field. If VOWP reports no
    native id, the app continues the newest host session instead of creating one.
    Evidence:
      native id reported: <YES_NO>
      native id before: <REDACTED_NATIVE_ID>

L4. Send one short message with the free included model.
    Prompt (copy exactly):
      Reply with exactly: STACKSPIRIT-CHAT-OK
    Expected: a model-origin reply, not Preview; the existing conversation continues in
    place. The app creates no native session.
    Evidence:
      selected model: <FREE_INCLUDED_MODEL_LABEL>
      reply: <ASSISTANT_REPLY>
      origin: model (preview must be absent)
      progressive text: <YES_NO>

L5. Read the same endpoint again after the turn and compare only the native id:
      vowp agent orchestrator endpoint global --json
    Expected: the native id is byte-for-byte equal to L3. Open the host's session list in
    the orchestrator UI and confirm no new session appeared for this turn.
    Evidence:
      native id after: <REDACTED_NATIVE_ID>
      same as before: <YES>
      new host session in list: <NO>

L6. Repeat L2-L5 for one project orchestrator "project:<PROJECT_NAME>".
    Expected: Local "project:<PROJECT_NAME>" resolves the endpoint name "<PROJECT_NAME>"
    (not the literal prefix), continues the same conversation, and creates no host
    session.
    Evidence:
      project: <PROJECT_NAME>
      existing conversation loaded: <YES_NO>
      native id same before/after: <YES>
      new host session in list: <NO>

L7. Scan the chat DOM, console, app logs and Tauri error text for these markers:
      <LOOPBACK_URL> (any loopback URL, including its scheme)
      <LOOPBACK_PORT> (any chat port)
      Authorization
      Basic
      Bearer
      <NATIVE_SESSION_ID> (any native session id)
      <VOWP_SESSION_ID> (any VOWP session id)
      -----BEGIN
      AKIA
      sk-
      OPENCODE_AUTH_CONTENT
      <SERVER_CREDENTIAL_ENV_NAME>
      /Users/
      /home/
      C:\
      engine/data
      engine/state
      engine/config
      workspaces
    Expected: none appear in the chat panel, message text, console, app logs, network
    panel or Tauri errors. Only Settings → Advanced → Engine → Advanced details may show
    the loopback port and bundled binary path.
    Evidence:
      DOM matches: <0>
      console matches: <0>
      logs matches: <0>
      network matches: <0>
      error-text matches: <0>

=== B. Server chat (§9.2) — BLOCKED, DO NOT RUN YET ===

Blocker: decision A (2026-11-19, ss-51) did not approve the app-owned SSH forward in
§3.3, and VOWP 0.5's server-side "vowp orchestrator endpoint" has not shipped. The
contract records VOWP 0.4.0, whose endpoint surface is the local agent read used by
§9.1b. The server half therefore stays gated: the app must show the typed chat_gated
error and must not run a server turn. Do not create a VOWP session or pair a surface for
this checklist. The server rows stay BLOCKED in the evidence table; they are not
failures.

When both gates open, run only this sequence:

B1. Open the global orchestrator. Expected: chip Server · <HOST_LABEL>; the composer is
    enabled only after ready.

B2. Send one short message. Then read the server log (redacted):
      vowp --json session show <VOWP_SESSION_ID> --events 20
    Confirm the same user and assistant turn is in the VOWP log. Never paste the session
    id or any path.

B3. Disconnect the host. Expected: host_disconnected, composer disabled, and no local
    fallback or preview reply. Reconnect and confirm reattach/backfill without duplicates.

B4. Confirm the VOWP endpoint/forward is absent from the DOM, console and Tauri errors;
    confirm the remote port, key path and token are absent.

B5. If the server runs a v2 OpenCode, expect protocol_unsupported and no partial chat.

=== C. Local global/project move (§9.3 step 4) — NOT RUN ===

The live Local global/project flow is §9.1b above. The "vowp orchestrator move" step in
doc §9.3 step 4 is a separate switch test; this checklist does not run it and must not
claim it verified. When the owner runs it, confirm the same VOWP session continues, the
chip follows the actual new location, and no duplicate messages appear.

=== D. Fail criteria (§9.4) — any one is a FAIL ===

- A preview or canned reply appears with bridge.preview === false.
- A token, endpoint URL, SSH key path or remote path appears in the DOM, console, a log
  or an error.
- A second bundled opencode process starts for chat.
- A local-only session is created under global or project.
- A disconnect silently falls back to local.
- A server turn is not visible in the VOWP log when the endpoint claims a mirrored
  session.
- The chip says Local when the transport is Server, or vice versa.

=== Evidence template (copy into the owner report) ===

The ss-46-chat-view report was absent in this worktree, so its gate results are
unverified. Do not turn a NOT RUN row into PASS from the source code or unit tests alone.

| Step | Result (PASS/FAIL/BLOCKED/NOT RUN) | Redacted evidence |
|---|---|---|
| A1 | NOT RUN | build <...>; sidecar <...>; static check <...> |
| A2 | NOT RUN | one child; chip Local · this Mac; health <...> |
| A3 | NOT RUN | model <...>; reply <...>; origin model; deltas <...> |
| A4 | NOT RUN | state <...>; duplicate count 0 |
| A5 | NOT RUN | messages <before>/<after>; duplicates 0 |
| A6 | NOT RUN | DOM/console/network/error matches 0; advanced engine port+binary only |
| A7 | NOT RUN | reattached without duplicates; engine_not_ready; no fallback |
| A8 | NOT RUN | engine_external; bundled restored |
| L1 | NOT RUN | local endpoint command present; no fallback |
| L2 | NOT RUN | chip Local · this Mac; existing conversation loaded; no new conversation |
| L3 | NOT RUN | native id reported <...>; before <REDACTED_NATIVE_ID> |
| L4 | NOT RUN | model <...>; reply <...>; origin model; no native session created |
| L5 | NOT RUN | native id after <REDACTED_NATIVE_ID>; same as before; no new host session |
| L6 | NOT RUN | project <...>; same native id before/after; no new host session |
| L7 | NOT RUN | DOM/console/logs/network/error matches 0; advanced engine port+binary only |
| B1 | BLOCKED | decision A; VOWP 0.5 endpoint required |
| B2 | BLOCKED | server log read not run |
| B3 | BLOCKED | disconnect check not run |
| B4 | BLOCKED | endpoint/forward leak scan not run |
| B5 | BLOCKED | v1/v2 check not run |
| C1 | NOT RUN | local move switch (doc §9.3 step 4) not run |

Do not mark a row PASS unless the expected result was observed in the real desktop build.
A MockBridge or VITE_SPIRIT_MOCK=1 observation is not owner verification.`;

function runChecklist() {
  console.log(CHECKLIST);
  return 0;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function usage(message) {
  if (message) console.error(redact(message));
  console.error(USAGE);
  process.exit(2);
}

function parseMode(argv) {
  let mode = null;
  for (const argument of argv) {
    if (argument === "--check" || argument === "--checklist") {
      if (mode) usage("choose exactly one of --check and --checklist");
      mode = argument === "--check" ? "check" : "checklist";
    } else if (argument === "--help" || argument === "-h") {
      console.log(USAGE);
      process.exit(0);
    } else {
      usage(`unknown argument: ${argument}`);
    }
  }
  if (!mode) usage("choose --check or --checklist");
  return mode;
}

const mode = parseMode(process.argv.slice(2));
process.exitCode = mode === "check" ? runCheck() : runChecklist();
