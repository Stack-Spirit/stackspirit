import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const TAURI_BRIDGE = fileURLToPath(new URL("./TauriBridge.ts", import.meta.url));
const TAURI_SRC = fileURLToPath(new URL("../../src-tauri/src/", import.meta.url));

function rustFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return rustFiles(path);
    return entry.isFile() && entry.name.endsWith(".rs") ? [path] : [];
  });
}

/** Every Rust function exposed by `#[tauri::command]`, keyed by its command name. */
function exposedCommands(): Map<string, string> {
  const commands = new Map<string, string>();
  for (const file of rustFiles(TAURI_SRC)) {
    const source = readFileSync(file, "utf8");
    const attribute =
      /#\[tauri::command(?:\(([^)]*)\))?\]\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)/g;
    for (const match of source.matchAll(attribute)) {
      const [, options = "", functionName = ""] = match;
      const rename = /rename\s*=\s*"([^"]+)"/.exec(options)?.[1];
      commands.set(functionName, rename ?? functionName);
    }
  }
  return commands;
}

/** The functions listed in `tauri::generate_handler![...]`, by exposed command name. */
function registeredCommands(): Set<string> {
  const lib = readFileSync(join(TAURI_SRC, "lib.rs"), "utf8");
  const handler = /generate_handler!\[([\s\S]*?)\]/.exec(lib)?.[1];
  if (!handler) throw new Error("lib.rs has no generate_handler! block");

  const exposed = exposedCommands();
  const registered = new Set<string>();
  for (const raw of handler.split(",")) {
    const entry = raw.trim();
    if (!entry) continue;
    const functionName = entry.split("::").pop() ?? entry;
    // A handler entry normally names a local command function. Mapping through the
    // attribute keeps `#[tauri::command(rename = "...")]` adapters honest.
    registered.add(exposed.get(functionName) ?? functionName);
  }
  return registered;
}

/** Every literal command name the desktop renderer invokes through `call()`. */
function bridgeCommands(): Set<string> {
  const source = readFileSync(TAURI_BRIDGE, "utf8");
  const calls = /call(?:<[^>]*>)?\(\s*"([a-z0-9_]+)"/g;
  return new Set([...source.matchAll(calls)].map((match) => match[1] ?? ""));
}

/**
 * The renderer may only invoke commands the Tauri host registers. This guards every
 * Settings surface at once, including the harness registry and CLI/MCP connect, where a
 * missing entry only fails at runtime inside the desktop app.
 */
describe("TauriBridge command contract", () => {
  it("invokes only commands registered by generate_handler!", () => {
    const bridge = bridgeCommands();
    const registered = registeredCommands();

    expect(bridge.size).toBeGreaterThan(50);
    expect(registered.size).toBeGreaterThan(50);
    expect([...bridge].filter((command) => !registered.has(command))).toEqual([]);
  });

  it("keeps the Settings harness and gate commands registered under their bridge names", () => {
    const registered = registeredCommands();
    for (const command of [
      "harnesses_list",
      "harnesses_save_custom",
      "harnesses_delete_custom",
      "harnesses_probe",
      "harnesses_connect_start",
      "engine_status",
      "engine_models",
      "key_status",
      "gates_get",
      "probe_gates",
    ]) {
      expect(registered.has(command), command).toBe(true);
    }
  });
});
