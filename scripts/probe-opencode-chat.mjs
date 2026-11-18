#!/usr/bin/env node
// Hermetic probe for the pinned StackSpirit Engine's chat HTTP/SSE surface.
//
// This implements §2.4 of docs/ORCHESTRATOR-CHAT.md and records the §2.5 facts without
// starting a model turn. It is deliberately read-only with respect to the owner's
// OpenCode state:
//
//   * the executable is resolved only at
//     apps/spirit-desktop/src-tauri/binaries/opencode-<TARGET_TRIPLE>; PATH is never
//     consulted and `opencode` is never spawned by name;
//   * the child gets a private 0700 HOME/XDG tree under the OS temp directory, and every
//     path in the tree is removed on every exit path;
//   * only loopback requests are made, to 127.0.0.1 and the ephemeral port the child was
//     given;
//   * no provider credential is read or passed, no provider auth variable is inherited,
//     and no message turn is sent (POST /session/{id}/message is never called);
//   * stdout contains only redacted facts: paths become placeholders, session ids become
//     <SESSION_ID>, ports become <PORT>, and the host name is removed.
//
// If the pinned sidecar is absent, the script prints exactly one line:
//     pinned sidecar absent; probe skipped
// and exits 0. It never downloads the sidecar.
//
// Exit codes:
//   0  probe completed, or the pinned sidecar is absent (nothing to probe)
//   1  probe failed closed (version mismatch, engine error, invalid manifest or environment)

import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { constants as fsConstants } from "node:fs";
import {
  accessSync,
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  statSync,
} from "node:fs";
import { createServer } from "node:net";
import { hostname as osHostname, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const MANIFEST_PATH = join(REPO_ROOT, "engine", "manifest.json");
const BINARIES_DIR = join(REPO_ROOT, "apps", "spirit-desktop", "src-tauri", "binaries");
const LOOPBACK = "127.0.0.1";
const HEALTH_INTERVAL_MS = 100;
const HEALTH_ATTEMPTS = 100;
const HEALTH_BUDGET_MS = 12_000;
const SSE_WINDOW_MS = 3_000;
const LOG_LINE_LIMIT = 40;
const LOG_CHARS_PER_LINE = 1_000;

const state = {
  probeRoot: null,
  child: null,
  cleaned: false,
  redactions: [],
  sessionIds: new Map(),
  ports: new Set(),
  logLines: [],
};

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// ---------------------------------------------------------------------------
// Redaction
// ---------------------------------------------------------------------------

function addRedaction(from, to) {
  if (typeof from === "string" && from.length > 0) {
    state.redactions.push([from, to]);
  }
}

// Static redactions are installed before any file or process is touched, so even an
// early manifest/read error cannot print a real repository or home path.
addRedaction(REPO_ROOT, "<REPO_ROOT>");
if (process.env.HOME) addRedaction(process.env.HOME, "<HOME>");
if (process.env.USERPROFILE) addRedaction(process.env.USERPROFILE, "<HOME>");
const HOSTNAME = osHostname();
const HOSTNAME_PATTERN =
  HOSTNAME && HOSTNAME.length >= 3
    ? new RegExp(`(?<![A-Za-z0-9])${escapeRegExp(HOSTNAME)}(?![A-Za-z0-9])`, "gi")
    : null;

function addSessionId(id, placeholder = "<SESSION_ID>") {
  if (typeof id === "string" && id.length > 0) {
    state.sessionIds.set(id, placeholder);
  }
}

function redactString(value) {
  let text = String(value);
  const pairs = [...state.redactions].sort((left, right) => right[0].length - left[0].length);
  for (const [from, to] of pairs) {
    text = text.split(from).join(to);
  }
  const ids = [...state.sessionIds.entries()].sort(
    (left, right) => right[0].length - left[0].length,
  );
  for (const [id, placeholder] of ids) {
    text = text.split(id).join(placeholder);
  }
  if (HOSTNAME_PATTERN) {
    text = text.replace(HOSTNAME_PATTERN, "<HOSTNAME>");
  }
  for (const port of state.ports) {
    text = text.replace(new RegExp(`\\b${port}\\b`, "g"), "<PORT>");
  }
  text = text.replace(
    /https?:\/\/(?:127\.0\.0\.1|localhost):\d+/gi,
    "http://127.0.0.1:<PORT>",
  );
  text = text.replace(/\b127\.0\.0\.1:\d+\b/g, "127.0.0.1:<PORT>");
  text = text.replace(/\b(?:localhost|\[::1\]):\d+\b/g, "localhost:<PORT>");
  text = text.replace(/\bses_[A-Za-z0-9]+\b/g, "<SESSION_ID>");
  return text;
}

function redactDeep(value) {
  if (typeof value === "string") return redactString(value);
  if (Array.isArray(value)) return value.map(redactDeep);
  if (value && typeof value === "object") {
    const result = {};
    for (const [key, item] of Object.entries(value)) {
      result[key] = redactDeep(item);
    }
    return result;
  }
  return value;
}

function redactDiagnostic(value) {
  let text = redactString(value);
  // Errors and engine output can name arbitrary absolute paths (caches, certificates,
  // temporary files). Strip any remaining absolute-looking path.
  text = text.replace(/(?:\/[A-Za-z0-9._@+-]+){2,}/g, "<PATH>");
  text = text.replace(/\b[A-Za-z]:\\[^\s"']+/g, "<PATH>");
  return text.slice(0, 300);
}

function safeError(error) {
  if (error instanceof Error) {
    return redactDiagnostic(error.message || error.name);
  }
  return redactDiagnostic(String(error));
}

// ---------------------------------------------------------------------------
// Cleanup
// ---------------------------------------------------------------------------

function cleanupSync() {
  if (state.cleaned) return;
  state.cleaned = true;
  if (state.child) {
    try {
      state.child.kill("SIGKILL");
    } catch {
      // The child may have exited between the check and the kill.
    }
    state.child = null;
  }
  if (state.probeRoot) {
    try {
      rmSync(state.probeRoot, { recursive: true, force: true });
    } catch {
      // Cleanup is best effort on the exit path; the tree is private and temporary.
    }
  }
}

for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(signal, () => {
    cleanupSync();
    process.exit(1);
  });
}
process.on("exit", cleanupSync);

// ---------------------------------------------------------------------------
// Host, manifest and private tree
// ---------------------------------------------------------------------------

function hostTriple() {
  const arch = { arm64: "aarch64", x64: "x86_64" }[process.arch];
  if (!arch) return null;
  if (process.platform === "darwin") return `${arch}-apple-darwin`;
  if (process.platform === "linux") return `${arch}-unknown-linux-gnu`;
  if (process.platform === "win32") return `${arch}-pc-windows-msvc`;
  return null;
}

function binaryFileName(triple) {
  return `opencode-${triple}${triple.includes("windows") ? ".exe" : ""}`;
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

function ensurePrivateDir(path) {
  mkdirSync(path, { recursive: true, mode: 0o700 });
  if (process.platform !== "win32") {
    try {
      chmodSync(path, 0o700);
    } catch {
      // A restrictive umask already produced a private directory.
    }
  }
}

function privateEnv(root) {
  const home = join(root, "home");
  const env = {
    // PATH is passed through for the child's own tooling only. The executable is always
    // spawned by its absolute pinned path; this is not a PATH lookup for `opencode`.
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

function buildPrivateTree() {
  const root = mkdtempSync(join(tmpdir(), "stackspirit-chat-probe-"));
  try {
    chmodSync(root, 0o700);
  } catch {
    // mkdtemp already creates 0700 on POSIX.
  }
  state.probeRoot = root;
  addRedaction(root, "<PROBE_ROOT>");
  for (const directory of [
    "home",
    "data",
    "state",
    "config",
    "cache",
    "run",
    "tmp",
    join("workspace", "base"),
    join("workspace", "selected"),
    join("workspace", "query"),
  ]) {
    ensurePrivateDir(join(root, directory));
  }
  return {
    root,
    home: join(root, "home"),
    base: join(root, "workspace", "base"),
    selected: join(root, "workspace", "selected"),
    query: join(root, "workspace", "query"),
    env: privateEnv(root),
  };
}

// ---------------------------------------------------------------------------
// HTTP helpers. Every request is loopback-only.
// ---------------------------------------------------------------------------

function loopbackUrl(port, requestPath) {
  return `http://${LOOPBACK}:${port}${requestPath}`;
}

async function httpRequest(port, method, requestPath, options = {}) {
  const headers = { accept: "application/json", ...(options.headers ?? {}) };
  let body;
  if (options.json !== undefined) {
    headers["content-type"] = "application/json";
    body = JSON.stringify(options.json);
  }
  const timeoutMs = options.timeoutMs ?? 3_000;
  try {
    const response = await fetch(loopbackUrl(port, requestPath), {
      method,
      headers,
      body,
      redirect: "error",
      signal: AbortSignal.timeout(timeoutMs),
    });
    const text = await response.text();
    let json;
    try {
      json = JSON.parse(text);
    } catch {
      json = undefined;
    }
    return {
      status: response.status,
      ok: response.ok,
      contentType: response.headers.get("content-type"),
      bytes: Buffer.byteLength(text, "utf8"),
      text,
      json,
    };
  } catch (error) {
    const timedOut = error instanceof Error && error.name === "TimeoutError";
    return {
      status: null,
      ok: false,
      contentType: null,
      bytes: 0,
      text: "",
      json: undefined,
      error: timedOut ? "timeout" : safeError(error),
    };
  }
}

function delay(ms) {
  return new Promise((resolveDelay) => setTimeout(resolveDelay, ms));
}

async function pollHealth(port) {
  let last = null;
  const started = Date.now();
  let attempts = 0;
  for (let attempt = 1; attempt <= HEALTH_ATTEMPTS; attempt += 1) {
    const remaining = HEALTH_BUDGET_MS - (Date.now() - started);
    if (remaining <= 0) break;
    attempts = attempt;
    const response = await httpRequest(port, "GET", "/global/health", {
      timeoutMs: Math.min(3_000, remaining),
    });
    last = response;
    if (response.json && response.json.healthy === true) {
      return {
        ok: true,
        attempts: attempt,
        status: response.status,
        contentType: response.contentType,
        healthy: true,
        version: typeof response.json.version === "string" ? response.json.version : null,
        bodyKeys: Object.keys(response.json).sort(),
      };
    }
    if (attempt < HEALTH_ATTEMPTS) await delay(HEALTH_INTERVAL_MS);
  }
  return {
    ok: false,
    attempts,
    status: last?.status ?? null,
    contentType: last?.contentType ?? null,
    error: last?.error ?? null,
    bodyKeys:
      last?.json && typeof last.json === "object" ? Object.keys(last.json).sort() : [],
  };
}

// ---------------------------------------------------------------------------
// Child process
// ---------------------------------------------------------------------------

function recordLogChunk(chunk) {
  const text = chunk.toString("utf8");
  for (const line of text.split(/\r?\n/)) {
    if (line.length === 0) continue;
    state.logLines.push(line.slice(0, LOG_CHARS_PER_LINE));
  }
  if (state.logLines.length > LOG_LINE_LIMIT) {
    state.logLines = state.logLines.slice(-LOG_LINE_LIMIT);
  }
}

function startEngine(binary, port, privateTree) {
  const child = spawn(
    binary,
    ["serve", "--hostname", LOOPBACK, "--port", String(port)],
    {
      cwd: privateTree.base,
      env: privateTree.env,
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    },
  );
  state.child = child;
  state.logLines = [];
  child.stdout?.on("data", recordLogChunk);
  child.stderr?.on("data", recordLogChunk);
  child.on("error", (error) => {
    state.logLines.push(`spawn: ${safeError(error)}`);
    if (state.logLines.length > LOG_LINE_LIMIT) {
      state.logLines = state.logLines.slice(-LOG_LINE_LIMIT);
    }
  });
  return child;
}

async function stopEngine() {
  const child = state.child;
  state.child = null;
  if (!child) return { stopped: true, forced: false };
  if (child.exitCode !== null) return { stopped: true, forced: false };
  let forced = false;
  // Attach the exit listener before signalling so a fast exit cannot be missed.
  const exitedPromise = once(child, "exit").then(() => true);
  try {
    child.kill("SIGTERM");
  } catch {
    forced = true;
  }
  const exited = await Promise.race([exitedPromise, delay(2_000).then(() => false)]);
  if (!exited) {
    forced = true;
    try {
      child.kill("SIGKILL");
    } catch {
      // Already gone.
    }
    await Promise.race([once(child, "exit").then(() => true), delay(1_000)]);
  }
  return { stopped: true, forced };
}

function redactedLogTail(limit = 3) {
  return state.logLines.slice(-limit).map(redactDiagnostic);
}

// ---------------------------------------------------------------------------
// Shape descriptions (no values that can carry ids, paths or credentials)
// ---------------------------------------------------------------------------

function describeKeys(value, limit = 40) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return [];
  return Object.keys(value).sort().slice(0, limit);
}

function describeArray(value) {
  if (!Array.isArray(value)) {
    if (value && typeof value === "object") {
      return { kind: "object", keys: describeKeys(value) };
    }
    return { kind: value === null ? "null" : typeof value };
  }
  if (value.length === 0) {
    return { kind: "array", length: 0, itemShape: "unknown (empty)" };
  }
  const first = value[0];
  if (!first || typeof first !== "object" || Array.isArray(first)) {
    return { kind: "array", length: value.length, itemShape: typeof first };
  }
  const keys = describeKeys(first);
  return {
    kind: "array",
    length: value.length,
    itemKeys: keys,
    itemHasInfoAndParts: keys.includes("info") && keys.includes("parts"),
    itemLooksLikeSession: keys.includes("directory") && keys.includes("projectID"),
    itemLooksLikeMessage:
      keys.includes("role") ||
      keys.includes("parts") ||
      (keys.includes("id") && keys.includes("sessionID")),
  };
}

function describeSession(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return { kind: value === null ? "null" : typeof value };
  }
  const keys = describeKeys(value);
  return {
    kind: "object",
    keys,
    hasId: typeof value.id === "string",
    hasSlug: typeof value.slug === "string",
    hasProjectId: typeof value.projectID === "string",
    hasDirectory: typeof value.directory === "string",
    hasTitle: typeof value.title === "string",
    hasVersion: typeof value.version === "string",
    timeKeys: describeKeys(value.time),
  };
}

function normalizedPath(value) {
  if (typeof value !== "string" || value.length === 0) return null;
  let normalized;
  try {
    normalized = realpathSync.native(value);
  } catch {
    normalized = resolve(value);
  }
  return process.platform === "win32" ? normalized.toLowerCase() : normalized;
}

function classifyDirectory(observed, candidates) {
  const normalized = normalizedPath(observed);
  if (!normalized) return "missing";
  for (const [label, candidate] of Object.entries(candidates)) {
    if (normalizedPath(candidate) === normalized) return label;
  }
  return "other";
}

function describeSse(raw) {
  const events = [];
  let commentLines = 0;
  let dataLines = 0;
  let firstEventField = null;
  const blocks = raw.split(/\r?\n\r?\n/);
  for (const block of blocks) {
    if (block.trim().length === 0) continue;
    const lines = block.split(/\r?\n/);
    let eventName = null;
    const data = [];
    for (const line of lines) {
      if (line.startsWith(":")) {
        commentLines += 1;
        continue;
      }
      const colon = line.indexOf(":");
      const field = colon === -1 ? line : line.slice(0, colon);
      let fieldValue = colon === -1 ? "" : line.slice(colon + 1);
      if (fieldValue.startsWith(" ")) fieldValue = fieldValue.slice(1);
      if (field === "event") eventName = fieldValue;
      if (field === "data") data.push(fieldValue);
    }
    if (eventName && firstEventField === null) firstEventField = eventName;
    const dataText = data.join("\n");
    if (dataText.length === 0) continue;
    dataLines += data.length;
    let parsed;
    try {
      parsed = JSON.parse(dataText);
    } catch {
      parsed = undefined;
    }
    const type =
      parsed && typeof parsed === "object" && typeof parsed.type === "string"
        ? parsed.type
        : null;
    events.push({
      event: eventName,
      type,
      envelope:
        parsed && typeof parsed === "object" && !Array.isArray(parsed)
          ? {
              keys: describeKeys(parsed, 10),
              hasType: Object.prototype.hasOwnProperty.call(parsed, "type"),
              hasProperties: Object.prototype.hasOwnProperty.call(parsed, "properties"),
            }
          : null,
    });
  }
  const names = [
    ...new Set(events.map((event) => event.type ?? event.event).filter(Boolean)),
  ];
  return {
    frameCount: events.length,
    dataLines,
    commentLines,
    firstEvent: events[0]?.type ?? events[0]?.event ?? firstEventField,
    uniqueEvents: names.slice(0, 12),
    firstEnvelope: events[0]?.envelope ?? null,
  };
}

async function probeSse(port) {
  const controller = new AbortController();
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    controller.abort();
  }, SSE_WINDOW_MS);
  const started = Date.now();
  let firstByteMs = null;
  let bytes = 0;
  let status = null;
  let contentType = null;
  let raw = "";
  let closedByServer = false;
  let error = null;
  try {
    const response = await fetch(loopbackUrl(port, "/event"), {
      headers: { accept: "text/event-stream", "cache-control": "no-cache" },
      redirect: "error",
      signal: controller.signal,
    });
    status = response.status;
    contentType = response.headers.get("content-type");
    if (!response.body) {
      closedByServer = true;
    } else {
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      while (true) {
        let chunk;
        try {
          chunk = await reader.read();
        } catch (readError) {
          if (readError instanceof Error && readError.name === "AbortError") break;
          throw readError;
        }
        if (chunk.done) {
          closedByServer = true;
          break;
        }
        if (firstByteMs === null) firstByteMs = Date.now() - started;
        bytes += chunk.value.byteLength;
        if (raw.length < 512 * 1024) {
          raw += decoder.decode(chunk.value, { stream: true });
        }
      }
    }
  } catch (fetchError) {
    if (!(fetchError instanceof Error && fetchError.name === "AbortError")) {
      error = safeError(fetchError);
    }
  } finally {
    clearTimeout(timer);
    controller.abort();
  }
  const parsed = describeSse(raw);
  return {
    status,
    connected: status !== null,
    contentType,
    firstByteMs,
    bytes,
    windowMs: SSE_WINDOW_MS,
    timedOut,
    closedByServer,
    error,
    ...parsed,
    hasServerConnected: parsed.uniqueEvents.includes("server.connected"),
    heartbeatIntervalMs:
      parsed.commentLines > 0
        ? "unknown: comment(s) observed, a single 3 s window cannot establish the interval"
        : "unknown: no comment/heartbeat observed in the 3 s window",
  };
}

function describeOpenApi(document, bytes) {
  if (!document || typeof document !== "object" || Array.isArray(document)) {
    return { present: true, parsed: false, bytes };
  }
  const paths =
    document.paths && typeof document.paths === "object" ? Object.keys(document.paths) : [];
  const relevant = paths.filter((path) =>
    /session|event|global\/health|\/doc|provider|config/i.test(path),
  );
  const pathMethods = {};
  for (const path of relevant.sort()) {
    pathMethods[path] = describeKeys(document.paths[path], 12);
  }
  const schemas =
    document.components && typeof document.components === "object"
      ? describeKeys(document.components.schemas, 200)
      : [];
  const schemaNames = schemas
    .filter((name) => /session|message|part|event|abort|directory/i.test(name))
    .slice(0, 40);
  let mentions = null;
  if (bytes <= 5_000_000) {
    try {
      const text = JSON.stringify(document);
      mentions = {
        messagePartUpdated: text.includes("message.part.updated"),
        sessionIdle: text.includes("session.idle"),
        serverConnected: text.includes("server.connected"),
        messageIdField: text.includes("messageID"),
        deltaField: text.includes("delta"),
        propertiesField: text.includes("properties"),
        directoryHeader: text.toLowerCase().includes("x-opencode-directory"),
      };
    } catch {
      mentions = null;
    }
  }
  return {
    present: true,
    parsed: true,
    bytes,
    openapi:
      typeof document.openapi === "string"
        ? document.openapi
        : typeof document.swagger === "string"
          ? document.swagger
          : null,
    pathCount: paths.length,
    relevantPaths: pathMethods,
    schemaNames,
    mentions,
  };
}

function countTree(root) {
  if (!existsSync(root)) return { exists: false, files: 0, directories: 0 };
  let files = 0;
  let directories = 0;
  const stack = [{ directory: root, depth: 0 }];
  while (stack.length > 0) {
    const current = stack.pop();
    if (current.depth > 8) continue;
    let entries;
    try {
      entries = readdirSync(current.directory, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (files + directories > 2_000) break;
      if (entry.isDirectory()) {
        directories += 1;
        stack.push({ directory: join(current.directory, entry.name), depth: current.depth + 1 });
      } else if (entry.isFile()) {
        files += 1;
      }
    }
  }
  return { exists: true, files, directories };
}

// ---------------------------------------------------------------------------
// Port reservation (mirrors reserve_loopback_port: bind loopback with port 0, read, close)
// ---------------------------------------------------------------------------

function tryReservePort(requestedPort = 0) {
  return new Promise((resolvePort, rejectPort) => {
    const server = createServer();
    server.unref();
    server.on("error", rejectPort);
    server.listen(requestedPort, LOOPBACK, () => {
      const address = server.address();
      const port = address && typeof address === "object" ? address.port : null;
      server.close(() => {
        if (typeof port === "number" && port > 0) {
          state.ports.add(port);
          resolvePort(port);
        } else rejectPort(new Error("could not read the reserved loopback port"));
      });
    });
  });
}

async function reservePortPreferring(previousPort) {
  if (previousPort) {
    try {
      const port = await tryReservePort(previousPort);
      return { port, reused: true };
    } catch {
      // The old port was taken in the tiny window after the child exited.
    }
  }
  return { port: await tryReservePort(0), reused: false };
}

// ---------------------------------------------------------------------------
// Probe steps
// ---------------------------------------------------------------------------

function readManifest() {
  const manifest = JSON.parse(readFileSync(MANIFEST_PATH, "utf8"));
  if (manifest.schema !== "stackspirit-engine-manifest/1") {
    throw new Error(`unsupported engine manifest schema: ${manifest.schema}`);
  }
  if (manifest.product !== "opencode") {
    throw new Error(`unexpected engine product: ${manifest.product}`);
  }
  if (!/^\d+\.\d+\.\d+$/.test(manifest.version ?? "")) {
    throw new Error("engine manifest version must be an exact semver release");
  }
  if (manifest.serve?.loopback_host !== LOOPBACK) {
    throw new Error("engine manifest no longer pins the loopback host");
  }
  if (manifest.serve?.health_path !== "/global/health") {
    throw new Error("engine manifest no longer pins the health path");
  }
  const serveArgs = manifest.serve?.args;
  if (
    !Array.isArray(serveArgs) ||
    serveArgs.length !== 3 ||
    serveArgs[0] !== "serve" ||
    serveArgs[1] !== "--hostname" ||
    serveArgs[2] !== LOOPBACK
  ) {
    throw new Error("engine manifest serve arguments drifted from serve --hostname 127.0.0.1");
  }
  return manifest;
}

function probeVersion(binary, env, cwd, expectedVersion) {
  const probe = spawnSync(binary, ["--version"], {
    cwd,
    env,
    encoding: "utf8",
    timeout: 15_000,
    maxBuffer: 64 * 1024,
  });
  const output = `${probe.stdout ?? ""}${probe.stderr ?? ""}`.trim();
  const firstLine = redactDiagnostic(output.split(/\r?\n/)[0] ?? "");
  if (probe.error) {
    return { ok: false, reason: `could not run the pinned sidecar: ${safeError(probe.error)}` };
  }
  if (probe.status !== 0) {
    return {
      ok: false,
      reason: `pinned sidecar --version exited ${probe.status ?? "timeout"}: ${firstLine}`,
    };
  }
  const versionToken = output.split(/[\s()]+/).filter(Boolean);
  const matchesVersion =
    versionToken.includes(expectedVersion) || versionToken.includes(`v${expectedVersion}`);
  if (!matchesVersion) {
    return {
      ok: false,
      reason: `pinned sidecar version mismatch; expected ${expectedVersion}, got ${firstLine}`,
    };
  }
  return { ok: true, version: expectedVersion };
}

function probeServeHelp(binary, env, cwd) {
  const probe = spawnSync(binary, ["serve", "--help"], {
    cwd,
    env,
    encoding: "utf8",
    timeout: 15_000,
    maxBuffer: 256 * 1024,
  });
  const text = `${probe.stdout ?? ""}\n${probe.stderr ?? ""}`;
  const flags = [
    ...new Set(
      text.match(/--[a-zA-Z0-9][a-zA-Z0-9-]*/g)?.filter((flag) => flag.length > 2) ?? [],
    ),
  ].sort();
  const authFlags = flags.filter((flag) => /auth|password|token|secret/i.test(flag));
  return {
    exitStatus: probe.error ? null : probe.status,
    error: probe.error ? safeError(probe.error) : null,
    flags,
    authFlags,
    hasAuthOrPasswordFlag: authFlags.length > 0,
    mentionsAuthText: /auth|password|token|secret/i.test(text),
    hasHostnameFlag: flags.includes("--hostname"),
    hasPortFlag: flags.includes("--port"),
    mentionsServe: /(^|\s)serve(\s|$)/.test(text),
    helpBytes: Buffer.byteLength(text, "utf8"),
  };
}

async function createSession(port, title, extra = {}) {
  const response = await httpRequest(port, "POST", extra.path ?? "/session", {
    json: { title },
    headers: extra.headers,
    timeoutMs: 3_000,
  });
  const session =
    response.json &&
    typeof response.json === "object" &&
    !Array.isArray(response.json)
      ? response.json
      : null;
  const id = session && typeof session.id === "string" ? session.id : null;
  if (id && extra.idPlaceholder) addSessionId(id, extra.idPlaceholder);
  return {
    status: response.status,
    ok: response.ok,
    contentType: response.contentType,
    created: response.ok && id !== null,
    id,
    shape: describeSession(session),
    error: response.error ?? null,
  };
}

async function probeSessions(port, privateTree) {
  const base = await createSession(port, "probe", { idPlaceholder: "<SESSION_ID_BASE>" });
  const header = await createSession(port, "probe-directory", {
    idPlaceholder: "<SESSION_ID_HEADER>",
    headers: { "x-opencode-directory": privateTree.selected },
  });
  const query = await createSession(port, "probe-query", {
    idPlaceholder: "<SESSION_ID_QUERY>",
    path: `/session?directory=${encodeURIComponent(privateTree.query)}`,
  });

  const candidates = {
    base: privateTree.base,
    selected: privateTree.selected,
    query: privateTree.query,
    home: privateTree.home,
  };
  // The session summaries intentionally do not carry the raw directory. Re-read each
  // session with the same selector used to create it, then classify the observed
  // directory against the three private workspaces. The raw path never reaches output.
  const baseDetail = base.id
    ? await httpRequest(port, "GET", `/session/${encodeURIComponent(base.id)}`, {
        timeoutMs: 3_000,
      })
    : null;
  const headerDetail = header.id
    ? await httpRequest(port, "GET", `/session/${encodeURIComponent(header.id)}`, {
        timeoutMs: 3_000,
        headers: { "x-opencode-directory": privateTree.selected },
      })
    : null;
  const queryDetail = query.id
    ? await httpRequest(
        port,
        "GET",
        `/session/${encodeURIComponent(query.id)}?directory=${encodeURIComponent(privateTree.query)}`,
        { timeoutMs: 3_000 },
      )
    : null;
  const directoryOf = (response) =>
    response?.json && typeof response.json === "object" && !Array.isArray(response.json)
      ? response.json.directory
      : undefined;

  const observed = {
    base: base.created ? classifyDirectory(directoryOf(baseDetail), candidates) : "not-created",
    header: header.created ? classifyDirectory(directoryOf(headerDetail), candidates) : "not-created",
    query: query.created ? classifyDirectory(directoryOf(queryDetail), candidates) : "not-created",
  };

  const list = await httpRequest(port, "GET", "/session", { timeoutMs: 3_000 });
  const listedIds = Array.isArray(list.json)
    ? list.json.map((item) => (item && typeof item.id === "string" ? item.id : null)).filter(Boolean)
    : [];
  const createdIds = [base.id, header.id, query.id].filter(Boolean);

  return {
    createDefault: {
      status: base.status,
      ok: base.ok,
      contentType: base.contentType,
      created: base.created,
      shape: base.shape,
      detailStatus: baseDetail?.status ?? null,
      defaultDirectoryClass: observed.base,
      defaultDirectoryMatchesCwd: observed.base === "base",
      error: base.error,
    },
    createWithDirectoryHeader: {
      status: header.status,
      ok: header.ok,
      contentType: header.contentType,
      created: header.created,
      shape: header.shape,
      detailStatus: headerDetail?.status ?? null,
      directoryClass: observed.header,
      headerHonored: observed.header === "selected",
      error: header.error,
    },
    createWithDirectoryQuery: {
      status: query.status,
      ok: query.ok,
      contentType: query.contentType,
      created: query.created,
      shape: query.shape,
      detailStatus: queryDetail?.status ?? null,
      directoryClass: observed.query,
      queryHonored: observed.query === "query",
      error: query.error,
    },
    list: {
      status: list.status,
      ok: list.ok,
      contentType: list.contentType,
      ...describeArray(list.json),
      createdCount: createdIds.length,
      listedCount: listedIds.length,
      listsEveryCreatedSession: createdIds.every((id) => listedIds.includes(id)),
      error: list.error ?? null,
    },
    baseSessionId: base.id,
    createdIds,
    directoryClasses: observed,
    sessionHandles: [
      base.id
        ? { label: "default", id: base.id, detailPath: `/session/${encodeURIComponent(base.id)}` }
        : null,
      header.id
        ? {
            label: "header",
            id: header.id,
            detailPath: `/session/${encodeURIComponent(header.id)}`,
            detailHeaders: { "x-opencode-directory": privateTree.selected },
          }
        : null,
      query.id
        ? {
            label: "query",
            id: query.id,
            detailPath: `/session/${encodeURIComponent(query.id)}?directory=${encodeURIComponent(privateTree.query)}`,
          }
        : null,
    ].filter(Boolean),
  };
}

async function probeMessageLog(port, sessionId) {
  if (!sessionId) return { skipped: "no session was created" };
  const response = await httpRequest(
    port,
    "GET",
    `/session/${encodeURIComponent(sessionId)}/message`,
    { timeoutMs: 3_000 },
  );
  return {
    status: response.status,
    ok: response.ok,
    contentType: response.contentType,
    ...describeArray(response.json),
    error: response.error ?? null,
  };
}

function describeAbort(response) {
  const keys = describeKeys(response.json);
  let bodyKind = "empty";
  if (response.error) bodyKind = "request-error";
  else if (typeof response.json === "boolean") bodyKind = `boolean:${response.json}`;
  else if (Array.isArray(response.json)) bodyKind = "array";
  else if (response.json !== undefined) bodyKind = "object";
  else if (response.text.trim().length > 0) bodyKind = "non-json";
  return {
    status: response.status,
    ok: response.ok,
    contentType: response.contentType,
    bodyKind,
    bodyKeys: keys,
    error: response.error ?? null,
  };
}

async function probeAbort(port, sessionId) {
  if (!sessionId) return { skipped: "no session was created" };
  const response = await httpRequest(
    port,
    "POST",
    `/session/${encodeURIComponent(sessionId)}/abort`,
    { timeoutMs: 3_000 },
  );
  const after = await httpRequest(
    port,
    "GET",
    `/session/${encodeURIComponent(sessionId)}`,
    { timeoutMs: 3_000 },
  );
  return {
    ...describeAbort(response),
    idleAbortAccepted:
      response.error === undefined &&
      response.status !== null &&
      response.status >= 200 &&
      response.status < 300,
    idleAbortDidNotCauseServerError:
      response.error === undefined && response.status !== null && response.status < 500,
    sessionStillExistsAfterAbort: after.ok,
    followUpStatus: after.status,
  };
}

async function probeRestart(binary, privateTree, previousPort, sessionHandles) {
  const stopped = await stopEngine();
  const reservation = await reservePortPreferring(previousPort);
  const child = startEngine(binary, reservation.port, privateTree);
  const health = await pollHealth(reservation.port);
  if (!health.ok) {
    return {
      stopped,
      portReused: reservation.reused,
      health,
      logTail: redactedLogTail(),
      childStarted: child.pid !== undefined,
      list: { skipped: "engine did not become healthy" },
      details: { skipped: "engine did not become healthy" },
    };
  }
  const list = await httpRequest(reservation.port, "GET", "/session", { timeoutMs: 3_000 });
  const listedIds = Array.isArray(list.json)
    ? list.json.map((item) => (item && typeof item.id === "string" ? item.id : null)).filter(Boolean)
    : [];
  const detailStatuses = {};
  for (const handle of sessionHandles) {
    const response = await httpRequest(reservation.port, "GET", handle.detailPath, {
      timeoutMs: 3_000,
      headers: handle.detailHeaders,
    });
    detailStatuses[handle.label ?? "session"] = response.status;
  }
  return {
    stopped,
    portReused: reservation.reused,
    health,
    logTail: [],
    childStarted: child.pid !== undefined,
    list: {
      status: list.status,
      ok: list.ok,
      contentType: list.contentType,
      ...describeArray(list.json),
      listedCount: listedIds.length,
      listContainsEveryCreatedSession: sessionHandles.every((handle) =>
        listedIds.includes(handle.id),
      ),
      error: list.error ?? null,
    },
    details: {
      attempted: sessionHandles.length,
      detailStatuses,
      everyCreatedSessionRetrievable: Object.values(detailStatuses).every(
        (status) => typeof status === "number" && status >= 200 && status < 300,
      ),
    },
    port: reservation.port,
  };
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function probe() {
  const manifest = readManifest();
  const triple = hostTriple();
  if (!triple) {
    throw new Error(`no pinned engine target for ${process.platform}/${process.arch}`);
  }
  if (!manifest.targets?.[triple]) {
    throw new Error(`engine manifest does not pin the host target ${triple}`);
  }
  const fileName = binaryFileName(triple);
  const binaryPath = join(BINARIES_DIR, fileName);
  const expectedRelativePath = `apps/spirit-desktop/src-tauri/binaries/${fileName}`;

  if (!existsSync(binaryPath)) {
    console.log("pinned sidecar absent; probe skipped");
    return 0;
  }
  if (!isExecutableFile(binaryPath)) {
    throw new Error(
      `pinned sidecar exists at ${expectedRelativePath} but is not an executable file; failed closed`,
    );
  }

  const privateTree = buildPrivateTree();

  const version = probeVersion(binaryPath, privateTree.env, privateTree.base, manifest.version);
  if (!version.ok) throw new Error(version.reason);

  const help = probeServeHelp(binaryPath, privateTree.env, privateTree.base);

  const firstPort = await tryReservePort(0);
  startEngine(binaryPath, firstPort, privateTree);
  const health = await pollHealth(firstPort);
  if (!health.ok) {
    const detail = health.error ? `: ${health.error}` : "";
    const tail = redactedLogTail();
    const suffix = tail.length > 0 ? `; last output: ${tail.join(" | ")}` : "";
    throw new Error(
      `pinned sidecar did not become healthy on loopback after ${health.attempts} attempts${detail}${suffix}`,
    );
  }

  const docResponse = await httpRequest(firstPort, "GET", "/doc", { timeoutMs: 10_000 });
  const openapi = docResponse.ok
    ? describeOpenApi(docResponse.json, docResponse.bytes)
    : {
        present: false,
        status: docResponse.status,
        contentType: docResponse.contentType,
        error: docResponse.error ?? null,
      };

  const sessions = await probeSessions(firstPort, privateTree);
  const messageLog = await probeMessageLog(firstPort, sessions.baseSessionId);
  const sse = await probeSse(firstPort);
  const abort = await probeAbort(firstPort, sessions.baseSessionId);

  const treeBeforeRestart = {
    home: countTree(join(privateTree.root, "home")),
    data: countTree(join(privateTree.root, "data")),
    state: countTree(join(privateTree.root, "state")),
    config: countTree(join(privateTree.root, "config")),
  };

  const restart = await probeRestart(
    binaryPath,
    privateTree,
    firstPort,
    sessions.sessionHandles,
  );

  const stopped = await stopEngine();
  const treeBeforeCleanup = {
    home: countTree(join(privateTree.root, "home")),
    data: countTree(join(privateTree.root, "data")),
    state: countTree(join(privateTree.root, "state")),
    config: countTree(join(privateTree.root, "config")),
  };
  const treeExistedBeforeCleanup = existsSync(privateTree.root);
  cleanupSync();
  const treeRemoved = !existsSync(privateTree.root);
  const cleanupOk = treeRemoved;

  const facts = {
    schema: "stackspirit-chat-probe/1",
    contract: "docs/ORCHESTRATOR-CHAT.md §2.4 and §2.5",
    pinned: {
      product: manifest.product,
      version: manifest.version,
      hostTriple: triple,
      binary: `<REPO_ROOT>/${expectedRelativePath}`,
      versionOutputMatches: version.ok,
    },
    isolation: {
      home: "<PROBE_ROOT>/home",
      xdgDataHome: "<PROBE_ROOT>/data",
      xdgStateHome: "<PROBE_ROOT>/state",
      xdgConfigHome: "<PROBE_ROOT>/config",
      xdgCacheHome: "<PROBE_ROOT>/cache",
      modelsFetchDisabled: privateTree.env.OPENCODE_DISABLE_MODELS_FETCH === "1",
      autoupdateDisabled: privateTree.env.OPENCODE_DISABLE_AUTOUPDATE === "1",
      loopbackOnly: true,
      providerAuthInherited: false,
      ownerOpenCodePathsUsed: false,
    },
    serveHelp: help,
    health: {
      ...health,
      endpoint: `GET http://127.0.0.1:<PORT>/global/health`,
    },
    openapi,
    sessions: {
      createDefault: sessions.createDefault,
      createWithDirectoryHeader: sessions.createWithDirectoryHeader,
      createWithDirectoryQuery: sessions.createWithDirectoryQuery,
      list: sessions.list,
    },
    messageLog,
    sse,
    abort,
    restart: {
      ...restart,
      endpoint: `GET http://127.0.0.1:<PORT>/session`,
    },
    privateTree: {
      beforeRestart: treeBeforeRestart,
      afterRestart: treeBeforeCleanup,
      existedBeforeCleanup: treeExistedBeforeCleanup,
      removedOnCleanup: treeRemoved,
      engineStopped: stopped.stopped,
      engineForcedKill: stopped.forced,
    },
    skippedCommands: [
      "POST /session/{id}/message — skipped by design: no model turn, no provider key",
      "DELETE /session/{id} — not used: the probe never deletes a session",
      "node scripts/fetch-engine.mjs — never run: the probe never downloads the sidecar",
    ],
    unresolved: [
      "§2.5.8 message.part.updated payload (needs a model turn or /doc evidence; no turn is allowed)",
      "§2.5.9 POST message response shape, info.error and messageID preservation (needs a model turn)",
    ],
  };
  // The port itself never reaches stdout; the restart block only carries it internally.
  delete facts.restart.port;

  console.log("pinned sidecar present; probe completed (redacted facts follow)");
  console.log(JSON.stringify(redactDeep(facts), null, 2));
  if (!cleanupOk) {
    console.error("probe failed closed: the private probe tree could not be removed");
    return 1;
  }
  return 0;
}

probe()
  .then((code) => {
    cleanupSync();
    process.exitCode = code;
  })
  .catch((error) => {
    cleanupSync();
    console.error(`probe failed closed: ${safeError(error)}`);
    if (state.probeRoot && existsSync(state.probeRoot)) {
      console.error("probe failed closed: the private probe tree could not be removed");
    }
    process.exitCode = 1;
  });
