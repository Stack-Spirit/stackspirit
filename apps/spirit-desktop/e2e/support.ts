import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { createConnection } from "node:net";
import { dirname, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const APP_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/**
 * The desktop e2e files drive full cockpit pages and axe. On a busy CI host they
 * are throughput-bound, so let them share one lock instead of competing for the
 * machine with four browsers and four Vite servers. The budget is deliberately
 * larger than the 90 s CLI default because axe scans a real page and can take
 * an order of magnitude longer when the host is loaded.
 */
export const E2E_LOCK = "stackspirit-desktop-e2e";
export const E2E_TIMEOUT = 300_000;

export interface ViteServer {
  readonly process: ChildProcess;
  readonly url: string;
  log(): string;
}

/**
 * Start a MockBridge scenario behind its own Vite dev server.
 *
 * The child is detached on POSIX so npm and Vite share a process group. Killing
 * only npm leaves Vite listening on the port, which makes later runs silently
 * reuse a stale server; `stopViteServer` signals the whole group instead.
 */
export async function startViteServer(
  scenario: string,
  port: number,
): Promise<ViteServer> {
  await assertPortFree(port);

  let output = "";
  const child = spawn(
    process.platform === "win32" ? "npm.cmd" : "npm",
    ["run", "dev", "--", "--port", String(port), "--strictPort"],
    {
      cwd: APP_ROOT,
      env: {
        ...process.env,
        VITE_SPIRIT_MOCK: "1",
        VITE_SPIRIT_MOCK_SCENARIO: scenario,
      },
      stdio: ["ignore", "pipe", "pipe"],
      detached: process.platform !== "win32",
    },
  );
  child.stdout?.on("data", (chunk: Buffer) => {
    output += chunk.toString();
  });
  child.stderr?.on("data", (chunk: Buffer) => {
    output += chunk.toString();
  });

  const url = `http://127.0.0.1:${port}`;
  try {
    await waitForVite(child, url, () => output);
  } catch (error) {
    await stopViteServer(child);
    throw error;
  }
  return { process: child, url, log: () => output };
}

export async function stopViteServer(child: ChildProcess | null): Promise<void> {
  if (!child || child.pid === undefined) return;

  if (process.platform === "win32") {
    // taskkill /T also stops the Vite process npm started.
    spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], {
      stdio: "ignore",
    });
    return;
  }

  signalGroup(child.pid, "SIGTERM");
  await waitForExit(child, 5_000);
  // npm can exit before Vite does; the group still owns the Vite process.
  signalGroup(child.pid, "SIGKILL");
}

function signalGroup(pid: number, signal: NodeJS.Signals): void {
  try {
    process.kill(-pid, signal);
  } catch {
    try {
      process.kill(pid, signal);
    } catch {
      // The process group is already gone.
    }
  }
}

async function waitForExit(child: ChildProcess, timeoutMs: number): Promise<void> {
  if (child.exitCode !== null) return;
  await new Promise<void>((resolveExit) => {
    const timer = setTimeout(resolveExit, timeoutMs);
    child.once("exit", () => {
      clearTimeout(timer);
      resolveExit();
    });
  });
}

function assertPortFree(port: number): Promise<void> {
  return new Promise((resolvePort, reject) => {
    const socket = createConnection({ host: "127.0.0.1", port });
    const fail = (message: string) => {
      socket.destroy();
      reject(new Error(message));
    };
    socket.once("connect", () => {
      fail(
        `Port ${port} is already in use. Stop the stale Vite process before running the e2e suite.`,
      );
    });
    socket.once("error", () => {
      socket.destroy();
      resolvePort();
    });
  });
}

async function waitForVite(
  child: ChildProcess,
  url: string,
  log: () => string,
): Promise<void> {
  const deadline = Date.now() + 120_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      throw new Error(`Vite exited before serving ${url}:\n${log().slice(-4000)}`);
    }
    try {
      const response = await fetch(url);
      if (response.ok) return;
    } catch {
      // The dev server socket is not open yet.
    }
    await delay(200);
  }
  throw new Error(`Vite did not serve ${url}:\n${log().slice(-4000)}`);
}
