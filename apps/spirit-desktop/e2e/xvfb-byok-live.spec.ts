import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
} from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import {
  E2E_LOCK,
  E2E_TIMEOUT,
  startViteServer,
  stopViteServer,
  type ViteServer,
} from "./support";

/**
 * Real `opencode` 1.19.5 sidecar + real Chromium (run under `xvfb-run -a` by
 * `scripts/verify-byok-xvfb.mjs`). Nothing about the engine is mocked: this spec spawns
 * the pinned sidecar fetched by `node scripts/fetch-engine.mjs` in an isolated temp XDG
 * tree, keeps the (synthetic, non-secret) provider key only in this Node process's
 * memory, hands it to the sidecar through the ephemeral `OPENCODE_AUTH_CONTENT`
 * environment exactly like the desktop host, and bridges `SettingsPage` and `ComposerBar`
 * to the sidecar's real `/provider`, `/config/providers` and verification responses.
 *
 * The preview `MockBridge` still supplies everything that is not the engine (hosts,
 * sessions, chat transcripts), the way `meadow-bridge-flows.spec.ts` wraps it.
 */
test.describe.configure({ mode: "serial", lock: E2E_LOCK, timeout: E2E_TIMEOUT });
test.use({ contextOptions: { reducedMotion: "reduce" } });

const PORT = 1431;
const BASE_URL = `http://127.0.0.1:${PORT}`;
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];
const THEMES = ["light", "dark"] as const;
const PINNED_VERSION = "1.19.5";
const PROVIDER = "openrouter";

/** A clearly synthetic key, assembled at runtime. It is not a credential. */
const TEST_KEY = ["sk", "or", "v1", "test-dummy-not-a-real-key-00000000"].join("-");

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");

// ───────────────────────── the live sidecar harness ─────────────────────────

interface Json {
  [key: string]: unknown;
}

function sidecarPath(): string {
  const triple =
    process.arch === "arm64"
      ? process.platform === "darwin"
        ? "aarch64-apple-darwin"
        : "aarch64-unknown-linux-gnu"
      : process.platform === "darwin"
        ? "x86_64-apple-darwin"
        : "x86_64-unknown-linux-gnu";
  const path = join(
    REPO_ROOT,
    "apps/spirit-desktop/src-tauri/binaries",
    `opencode-${triple}`,
  );
  if (!existsSync(path)) {
    throw new Error(
      `pinned sidecar missing at ${path}: run \`node scripts/fetch-engine.mjs\` first`,
    );
  }
  return path;
}

async function freePort(): Promise<number> {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address() as { port: number };
      server.close(() => resolvePort(port));
    });
  });
}

async function getJson(
  port: number,
  path: string,
  init?: RequestInit,
  timeoutMs = 30_000,
): Promise<Json> {
  const response = await fetch(`http://127.0.0.1:${port}${path}`, {
    ...init,
    signal: AbortSignal.timeout(timeoutMs),
  });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(`${init?.method ?? "GET"} ${path} -> ${response.status}`);
  }
  return text ? (JSON.parse(text) as Json) : {};
}

function childEnv(root: string, auth: string | null): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {
    PATH: process.env.PATH,
    HOME: join(root, "home"),
    XDG_DATA_HOME: join(root, "data"),
    XDG_STATE_HOME: join(root, "state"),
    XDG_CONFIG_HOME: join(root, "config"),
    OPENCODE_DISABLE_PROJECT_CONFIG: "1",
  };
  for (const name of [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "LANG",
  ]) {
    if (process.env[name]) env[name] = process.env[name];
  }
  if (auth) env.OPENCODE_AUTH_CONTENT = auth;
  return env;
}

class SidecarProcess {
  readonly output: string[] = [];
  child: ChildProcess | null = null;
  port = 0;

  constructor(
    readonly root: string,
    private readonly auth: string | null,
  ) {
    for (const directory of ["home", "data", "state", "config", "workspace"]) {
      mkdirSync(join(root, directory), { recursive: true });
    }
  }

  async start(): Promise<void> {
    this.port = await freePort();
    const child = spawn(
      sidecarPath(),
      ["serve", "--hostname", "127.0.0.1", "--port", String(this.port)],
      {
        cwd: join(this.root, "workspace"),
        env: childEnv(this.root, this.auth),
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    for (const stream of [child.stdout, child.stderr]) {
      stream?.on("data", (chunk: Buffer) => this.output.push(chunk.toString()));
    }
    this.child = child;
    for (let attempt = 0; attempt < 100; attempt += 1) {
      if (child.exitCode !== null) {
        throw new Error(`sidecar exited early (${child.exitCode})`);
      }
      try {
        const health = await getJson(this.port, "/global/health", undefined, 2_000);
        if (health.healthy === true) {
          expect(health.version).toBe(PINNED_VERSION);
          return;
        }
      } catch {
        // not listening yet
      }
      await delay(150);
    }
    throw new Error("sidecar never became healthy");
  }

  async stop(): Promise<void> {
    const child = this.child;
    this.child = null;
    if (!child || child.exitCode !== null) return;
    const exited = new Promise<void>((done) => child.once("exit", () => done()));
    child.kill("SIGTERM");
    await Promise.race([exited, delay(4_000)]);
    if (child.exitCode === null) {
      child.kill("SIGKILL");
      await exited;
    }
  }

  get pid(): number | undefined {
    return this.child?.pid;
  }
}

function authDocument(keys: Map<string, StoredKey>): string | null {
  if (keys.size === 0) return null;
  const document: Json = {};
  for (const [provider, key] of keys) {
    document[provider] = { type: "api", key: key.secret };
  }
  return JSON.stringify(document);
}

interface StoredKey {
  secret: string;
  state: "saved" | "invalid" | "unverified";
  checkedAt: string | null;
}

interface ModelRow {
  id: string;
  provider: string;
  label: string;
  free: boolean;
  context: number | null;
  cost: number;
  reasoning: boolean;
  textOnly: boolean;
}

function modelsOf(document: Json): ModelRow[] {
  const rows: ModelRow[] = [];
  for (const provider of (document.providers as Json[]) ?? []) {
    const id = String(provider.id);
    for (const [key, raw] of Object.entries((provider.models as Json) ?? {})) {
      const model = raw as Json;
      const cost = Number((model.cost as Json | undefined)?.input ?? 0);
      const modalities = model.modalities as Json | undefined;
      const input = (modalities?.input as string[] | undefined) ?? ["text"];
      const output = (modalities?.output as string[] | undefined) ?? ["text"];
      rows.push({
        id: `${id}/${key}`,
        provider: id,
        label: String(model.name ?? key),
        free: id === "opencode" && cost === 0,
        context:
          typeof (model.limit as Json | undefined)?.context === "number"
            ? ((model.limit as Json).context as number)
            : null,
        cost,
        reasoning: model.reasoning === true,
        textOnly:
          model.status !== "deprecated" &&
          input.includes("text") &&
          output.length === 1 &&
          output[0] === "text",
      });
    }
  }
  return rows;
}

/** Same classification as `classify_message_response` in spirit-core. */
function classify(body: unknown): "saved" | "invalid" | "unverified" {
  const findError = (value: unknown): unknown => {
    if (Array.isArray(value)) {
      for (const item of value) {
        const found = findError(item);
        if (found !== undefined) return found;
      }
    } else if (value && typeof value === "object") {
      const map = value as Json;
      if ("error" in map) return map.error;
      for (const item of Object.values(map)) {
        const found = findError(item);
        if (found !== undefined) return found;
      }
    }
    return undefined;
  };
  const statusOf = (value: unknown): number | null => {
    if (Array.isArray(value)) {
      for (const item of value) {
        const found = statusOf(item);
        if (found !== null) return found;
      }
    } else if (value && typeof value === "object") {
      const map = value as Json;
      for (const name of ["statusCode", "status_code", "status", "code"]) {
        const code = map[name];
        if (typeof code === "number" && code >= 100 && code <= 599) return code;
      }
      for (const item of Object.values(map)) {
        const found = statusOf(item);
        if (found !== null) return found;
      }
    }
    return null;
  };
  const error = findError(body);
  if (error === undefined || error === null) return "saved";
  const status = statusOf(error);
  return status === 401 || status === 403 ? "invalid" : "unverified";
}

/** The Node-side stand-in for the desktop host's engine commands. */
class LiveEngineHost {
  readonly root = mkdtempSync(join(tmpdir(), "stackspirit-xvfb-byok-"));
  readonly keys = new Map<string, StoredKey>();
  readonly probeOutput: string[] = [];
  sidecar: SidecarProcess;
  startedAt = new Date().toISOString();
  private readonly pidHistory: number[] = [];
  private readonly probeRoots: string[] = [];

  constructor() {
    this.sidecar = this.newSidecar();
  }

  private newSidecar(): SidecarProcess {
    return new SidecarProcess(join(this.root, "engine"), authDocument(this.keys));
  }

  get dataTree(): string {
    return join(this.root, "engine");
  }

  async boot(): Promise<void> {
    await this.sidecar.start();
    this.recordPid();
  }

  private recordPid(): void {
    if (this.sidecar.pid !== undefined) this.pidHistory.push(this.sidecar.pid);
  }

  pids(): number[] {
    return [...this.pidHistory];
  }

  /** A key change restarts the bundled sidecar so it reads the new auth document. */
  async restart(): Promise<void> {
    const previousOutput = this.sidecar.output;
    await this.sidecar.stop();
    this.sidecar = this.newSidecar();
    this.sidecar.output.push(...previousOutput);
    this.startedAt = new Date().toISOString();
    await this.sidecar.start();
    this.recordPid();
  }

  async shutdown(): Promise<void> {
    await this.sidecar.stop();
  }

  cleanup(): void {
    rmSync(this.root, { recursive: true, force: true });
    for (const probe of this.probeRoots) rmSync(probe, { recursive: true, force: true });
  }

  async models(): Promise<ModelRow[]> {
    return modelsOf(await getJson(this.sidecar.port, "/config/providers"));
  }

  async providers(): Promise<Json[]> {
    const document = await getJson(this.sidecar.port, "/provider");
    const connected = new Set((document.connected as string[]) ?? []);
    const configured = modelsOf(await getJson(this.sidecar.port, "/config/providers"));
    return ((document.all as Json[]) ?? []).map((provider) => {
      const id = String(provider.id);
      const models = Object.keys((provider.models as Json) ?? {}).length;
      const live = configured.filter((model) => model.provider === id);
      return {
        id,
        label: String(provider.name ?? id),
        connected: connected.has(id),
        source: connected.has(id) ? String(provider.source ?? "api") : undefined,
        model_count: live.length > 0 ? live.length : models,
        free_model_count: live.filter((model) => model.free).length,
      };
    });
  }

  /** Models chat can use now: free runtime models plus connected keyed providers. */
  async chatModels(): Promise<Json[]> {
    return (await this.models())
      .filter((model) => model.free || this.keys.has(model.provider))
      .sort(
        (left, right) =>
          Number(!left.free) - Number(!right.free) ||
          left.provider.localeCompare(right.provider) ||
          left.label.localeCompare(right.label),
      )
      .map((model) => ({
        id: model.id,
        label: model.label,
        provider: model.provider,
        tier: model.free ? "included" : "byok",
        context_tokens: model.context,
      }));
  }

  keyStatuses(): Json[] {
    return [...this.keys].map(([provider, key]) => ({
      provider,
      state: key.state,
      last_checked_at: key.checkedAt,
    }));
  }

  async storeKey(provider: string, secret: string): Promise<Json> {
    this.keys.set(provider, { secret, state: "saved", checkedAt: null });
    await this.restart();
    return { provider, state: "saved", last_checked_at: null };
  }

  async deleteKey(provider: string): Promise<Json> {
    this.keys.delete(provider);
    await this.restart();
    return { provider, state: "missing", last_checked_at: new Date().toISOString() };
  }

  /**
   * Verification mirrors `verify_with_secret`: a throwaway sidecar with its own XDG
   * tree receives the key through `OPENCODE_AUTH_CONTENT`, makes one minimal model call
   * and is stopped. The saved key is never removed, whatever the outcome.
   */
  async verifyKey(provider: string): Promise<Json> {
    const key = this.keys.get(provider);
    if (!key) {
      return { provider, state: "missing", last_checked_at: new Date().toISOString() };
    }
    const probeRoot = mkdtempSync(join(tmpdir(), "stackspirit-xvfb-probe-"));
    this.probeRoots.push(probeRoot);
    const probe = new SidecarProcess(
      probeRoot,
      JSON.stringify({ [provider]: { type: "api", key: key.secret } }),
    );
    let state: "saved" | "invalid" | "unverified" = "unverified";
    try {
      await probe.start();
      const candidates = modelsOf(await getJson(probe.port, "/config/providers"))
        .filter((model) => model.provider === provider && model.textOnly)
        .sort((a, b) => Number(a.reasoning) - Number(b.reasoning) || a.cost - b.cost)
        .slice(0, 3);
      for (const model of candidates) {
        const session = await getJson(probe.port, "/session", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: "{}",
        });
        const response = await fetch(
          `http://127.0.0.1:${probe.port}/session/${String(session.id)}/message`,
          {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({
              model: {
                providerID: provider,
                modelID: model.id.slice(provider.length + 1),
              },
              parts: [{ type: "text", text: "Reply with OK." }],
            }),
            signal: AbortSignal.timeout(25_000),
          },
        ).catch(() => null);
        if (!response) break;
        if (response.status === 401 || response.status === 403) {
          state = "invalid";
          break;
        }
        if (response.status === 400 || response.status === 404) continue;
        if (!response.ok) break;
        state = classify(await response.json().catch(() => null));
        break;
      }
    } catch {
      state = "unverified";
    } finally {
      this.probeOutput.push(...probe.output);
      await probe.stop();
    }
    key.state = state;
    key.checkedAt = new Date().toISOString();
    return { provider, state, last_checked_at: key.checkedAt };
  }

  /** Audit `opencode.log`, sidecar output and every file in the private tree. */
  audit(): { lines: number; errors: number; leaks: number; problems: string[] } {
    const logPath = join(this.dataTree, "data/opencode/log/opencode.log");
    expect(existsSync(logPath), `opencode.log missing at ${logPath}`).toBe(true);
    const log = readFileSync(logPath, "utf8");
    expect(log.trim().length, "opencode.log is empty").toBeGreaterThan(0);

    const forbidden =
      /panic|SQLITE_BUSY|database is locked|unhandled|uncaught|segmentation fault|fatal/i;
    const output = [...this.sidecar.output, ...this.probeOutput]
      .join("")
      .split("\n");
    const problems = [...log.split("\n"), ...output].filter((line) => forbidden.test(line));

    let leaks = log.includes(TEST_KEY) ? 1 : 0;
    leaks += output.filter((line) => line.includes(TEST_KEY)).length;
    const needle = Buffer.from(TEST_KEY);
    const walk = (directory: string): void => {
      for (const name of readdirSync(directory)) {
        const path = join(directory, name);
        const info = statSync(path);
        if (info.isDirectory()) walk(path);
        else if (info.isFile() && readFileSync(path).includes(needle)) leaks += 1;
      }
    };
    walk(this.dataTree);
    expect(
      existsSync(join(this.dataTree, "data/opencode/auth.json")),
      "plaintext auth.json written",
    ).toBe(false);
    return {
      lines: log.split("\n").filter(Boolean).length,
      errors: problems.length,
      leaks,
      problems,
    };
  }
}

// ───────────────────────── the bridge wrapper ─────────────────────────

/**
 * Replaces only the desktop app's `bridge/index.ts`. Every engine and model call goes
 * to the live sidecar through `window.__liveEngine`, which the test exposes from Node.
 */
const BRIDGE_WRAPPER = `
import { MockBridge } from "/@id/@stackspirit/ui";

const live = (op, arg) => window.__liveEngine(op, arg ?? null);
const clone = (value) => JSON.parse(JSON.stringify(value));

export async function createBridge() {
  const bridge = new MockBridge("empty");
  const engine = bridge.engine;

  const status = async () => {
    const info = await live("status");
    const models = await live("chatModels");
    const first = models.find((model) => model.tier === "included") ?? null;
    return {
      choice: { type: "bundled" },
      state: { state: "ready" },
      version: info.version,
      binary_path: null,
      port: info.port,
      selected_model: first && {
        id: first.id,
        label: first.label,
        provider: first.provider,
        access: "included",
        context_tokens: first.context_tokens ?? null,
        notes: "",
        verified_at: info.startedAt,
      },
      included_models_available: models.some((model) => model.tier === "included"),
      started_at: info.startedAt,
      last_error: null,
      onboarding_complete: true,
      providers: info.providers,
    };
  };

  // A key change restarts the sidecar; republish engine status and the chat models so
  // every mounted view updates without a reload, like the desktop host does.
  const publish = async () => {
    const engineStatus = await status();
    bridge.chatModels = await live("chatModels");
    bridge.emit({ kind: "engine", revision: bridge.nextRevision(), engine: clone(engineStatus) });
    return engineStatus;
  };

  engine.status = status;
  engine.models = async () => [];
  engine.restart = publish;
  engine.keyStatus = async () => live("keyStatus");
  engine.storeKey = async (provider, secret) => {
    const result = await live("storeKey", { provider, secret });
    await publish();
    return result;
  };
  engine.verifyKey = async (provider) => {
    const result = await live("verifyKey", { provider });
    await publish();
    return result;
  };
  engine.deleteKey = async (provider) => {
    const result = await live("deleteKey", { provider });
    await publish();
    return result;
  };

  // The chat session lists the live sidecar's usable models on every start or attach.
  const chat = bridge.chat;
  const refresh = async () => {
    bridge.chatModels = await live("chatModels");
  };
  const start = chat.start;
  const attach = chat.attach;
  chat.start = async (input) => {
    await refresh();
    return start(input);
  };
  chat.attach = async (session) => {
    await refresh();
    return attach(session);
  };
  await refresh();
  return bridge;
}
`;

async function installLiveBridge(page: Page, host: LiveEngineHost) {
  await page.exposeFunction("__liveEngine", async (op: string, arg: Json | null) => {
    switch (op) {
      case "status":
        return {
          port: host.sidecar.port,
          version: PINNED_VERSION,
          startedAt: host.startedAt,
          providers: await host.providers(),
        };
      case "chatModels":
        return host.chatModels();
      case "keyStatus":
        return host.keyStatuses();
      case "storeKey":
        return host.storeKey(String(arg?.provider), String(arg?.secret));
      case "verifyKey":
        return host.verifyKey(String(arg?.provider));
      case "deleteKey":
        return host.deleteKey(String(arg?.provider));
      default:
        throw new Error(`unknown live engine op: ${op}`);
    }
  });
  await page.route(/^https?:\/\/[^/]+\/src\/bridge\/index\.ts(?:\?.*)?$/, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/javascript",
      headers: { "cache-control": "no-store" },
      body: BRIDGE_WRAPPER,
    });
  });
}

async function expectNoWcagViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect(
    results.violations.map((violation) => ({
      id: violation.id,
      targets: violation.nodes.map((node) => node.target.join(" ")),
    })),
  ).toEqual([]);
}

const startedAt = Date.now();
function progress(label: string): void {
  console.log(`[+${((Date.now() - startedAt) / 1000).toFixed(1)}s] ${label}`);
}

let vite: ViteServer | null = null;

test.beforeAll(async () => {
  vite = await startViteServer("empty", PORT);
});

test.afterAll(async () => {
  await stopViteServer(vite?.process ?? null);
});

async function optionLabels(menu: Locator): Promise<string[]> {
  return (await menu.getByRole("option").allInnerTexts()).map((text) => text.trim());
}

for (const theme of THEMES) {
  test(`real opencode ${PINNED_VERSION}: add key, verify keeps it, OpenRouter models appear (${theme})`, async ({
    page,
  }) => {
    const host = new LiveEngineHost();
    const pids: number[] = [];
    try {
      await host.boot();
      expect(host.sidecar.pid).toBeTruthy();
      await installLiveBridge(page, host);
      await page.goto(`${BASE_URL}/?scenario=empty&theme=${theme}`);

      progress("step 1");
      // 1. Setup chat: the picker initially offers OpenCode Free models only.
      await page.getByRole("button", { name: /Set up with AI/ }).click();
      const trigger = page.getByTestId("composer-model");
      await expect(trigger).toContainText("OpenCode free");
      await trigger.click();
      const menu = page.getByTestId("model-picker-popover");
      await expect(menu).toBeVisible();
      await expect(menu.getByRole("group", { name: "OpenCode Free" })).toBeVisible();
      await expect(menu.getByRole("group")).toHaveCount(1);
      await expect(menu.getByRole("group", { name: "OpenRouter" })).toHaveCount(0);
      expect((await optionLabels(menu)).length).toBeGreaterThan(0);
      await expectNoWcagViolations(page);

      progress("step 2");
      // 2. Manage models opens Settings → Engine & Models in place.
      await menu.getByTestId("composer-manage-models").click();
      const dialog = page.getByRole("dialog", { name: "Engine & Models" });
      await expect(dialog).toBeVisible();
      const card = dialog.getByTestId("engine-models-settings");

      progress("step 3");
      // 3. Search OpenRouter, paste the synthetic key, save, then Verify key.
      await card.getByRole("searchbox", { name: "Search providers" }).fill("openrouter");
      const row = card.getByTestId("provider-openrouter");
      await expect(row).toContainText("OpenRouter");
      await row.getByRole("button", { name: "Connect OpenRouter" }).click();
      await row.getByLabel("API key for OpenRouter").fill(TEST_KEY);
      await row.getByRole("button", { name: "Save key" }).click();
      // Saving restarts the real sidecar and verifies once; wait for the honest result.
      await expect(card.getByRole("status")).toContainText(/openrouter (rejected|connected)/i, {
        timeout: 90_000,
      });

      progress("step 4");
      // 4. The key stays: OpenRouter is a Connected provider with all three controls.
      const connected = card.getByRole("list", { name: "Connected providers" });
      const connectedRow = connected.getByTestId("provider-openrouter");
      await expect(connectedRow).toContainText("Connected");
      await expect(connectedRow.getByRole("button", { name: "Verify key" })).toBeVisible();
      await connectedRow.getByRole("button", { name: "Verify key" }).click();
      await expect(connectedRow.getByRole("button", { name: "Verify key" })).toBeEnabled({
        timeout: 90_000,
      });
      await expect(card.getByRole("status")).toContainText(
        /rejected this key|still unverified|key verified/,
      );
      // An honest badge: the synthetic key is Invalid (or Unverified offline), never wiped.
      await expect(connectedRow).toContainText(/Invalid key|Unverified/);
      await expect(connectedRow).not.toContainText("Verified");
      await expect(connectedRow.getByRole("button", { name: "Verify key" })).toBeVisible();
      await expect(connectedRow.getByRole("button", { name: "Replace OpenRouter key" })).toBeVisible();
      await expect(connectedRow.getByRole("button", { name: "Remove OpenRouter key" })).toBeVisible();
      await expect(card.getByRole("alert")).toHaveCount(0);
      expect(host.keys.get(PROVIDER)?.secret).toBe(TEST_KEY);
      await expectNoWcagViolations(page);

      progress("step 5");
      // 5. The composer picker lists the live OpenRouter catalog next to OpenCode Free.
      await dialog.getByRole("button", { name: "Close dialog" }).click();
      await trigger.click();
      await expect(menu).toBeVisible();
      await expect(menu.getByRole("group", { name: "OpenCode Free" })).toBeVisible();
      const openrouter = menu.getByRole("group", { name: "OpenRouter" });
      await expect(openrouter).toBeVisible();
      expect((await openrouter.getByRole("option").count())).toBeGreaterThan(100);

      const search = menu.getByTestId("composer-model-search");
      await search.fill("claude");
      await expect(openrouter.getByRole("option").first()).toContainText(/claude/i);
      await search.fill("gemini");
      await expect(openrouter.getByRole("option").first()).toContainText(/gemini/i);
      await expectNoWcagViolations(page);

      await search.fill("claude");
      const choice = openrouter.getByRole("option").first();
      const label = ((await choice.innerText()).split("\n")[0] ?? "").trim();
      expect(label.length).toBeGreaterThan(0);
      // Select with the keyboard: at the 1280x720 test viewport the chat header overlaps
      // the top of the popover, so a pointer click on its first row is intercepted.
      await expect(choice).toHaveClass(/is-active/);
      await page.keyboard.press("Enter");
      await expect(menu).toHaveCount(0);
      await expect(trigger).toContainText("BYOK");
      await expect(trigger).toContainText(label);
      await expectNoWcagViolations(page);

      progress("step 6");
      // 6. Inspect the live sidecar log: no crash, no SQLite lock, no key anywhere.
      const audit = host.audit();
      console.log(
        `opencode.log (${theme}): lines=${audit.lines} errors=${audit.errors} leaks=${audit.leaks}`,
      );
      expect(audit.problems).toEqual([]);
      expect(audit.leaks).toBe(0);
    } finally {
      // Every sidecar generation (one per key change) must be gone after shutdown.
      pids.push(...host.pids());
      await host.shutdown();
      host.cleanup();
    }
    for (const pid of pids) {
      expect(() => process.kill(pid, 0), `orphan sidecar ${pid}`).toThrow();
    }
  });
}
