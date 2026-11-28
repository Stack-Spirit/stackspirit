#!/usr/bin/env node
// Harness chat owner verification: static repository checks and the §7.5 checklist.
//
// Counterpart to scripts/verify-session-takeover.mjs. It is deliberately inert:
//
//   * --check reads repository files only. It never imports node:child_process,
//     spawns a harness, vowp, the app or a shell, opens a socket, or reads .env,
//     keychain, app data, tokens or a real harness binary.
//   * --checklist prints the fixed §7.5 owner checklist. It does not execute it.
//   * --help prints this usage block and the same inertness policy.
//
// Passing --check is not owner evidence. It only proves the shipped pieces are present
// in this checkout. No real ACP turn is claimed. The default permission policy denies
// tool requests until the owner configures one. There is no ChatView harness chip or
// permission card yet.
//
// Usage:
//   node scripts/verify-harness-chat.mjs --check
//   node scripts/verify-harness-chat.mjs --checklist
//   node scripts/verify-harness-chat.mjs --help
//
// Exit codes:
//   0  every static check passed, or the checklist/help was printed
//   1  a required repository file or symbol is missing
//   2  usage error

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const POLICY = `Passing --check is not owner evidence. No real ACP turn is claimed. The default
permission policy denies tool requests until the owner configures one.`;

const USAGE = `StackSpirit harness chat verification (docs/HARNESS-CHAT.md §7.5)

usage:
  node scripts/verify-harness-chat.mjs --check
      Check that the shipped selection/gate modules, HarnessChatProvider and ACP adapter,
      chat_argv manifest field and built-in goose/cursor-cli argv, desktop wiring, the
      Settings Chat transport and custom-argv form, and the core/UI tests are present.
      Then print the fixed §7.5 owner checklist. It reads repository files only.

  node scripts/verify-harness-chat.mjs --checklist
      Print the owner-run §7.5 checklist. Nothing is executed.

  node scripts/verify-harness-chat.mjs --help

Inertness: this script never spawns a harness, VOWP, the app or a shell, opens a socket
or reads .env, keychain, app data, tokens or a real harness binary.
${POLICY}`;

const results = [];

function readRepoText(relativePath) {
  try {
    return readFileSync(join(REPO_ROOT, relativePath), "utf8");
  } catch {
    return null;
  }
}

// A missing file or symbol fails with the exact file, so --check fails closed.
function checkFiles(label, specs) {
  const problems = [];
  let checked = 0;
  for (const spec of specs) {
    const text = readRepoText(spec.file);
    if (text === null) {
      problems.push(`${spec.file} is missing`);
      continue;
    }
    checked += 1;
    for (const symbol of spec.required ?? []) {
      if (!text.includes(symbol)) {
        problems.push(`${spec.file} is missing ${JSON.stringify(symbol)}`);
      }
    }
  }
  if (problems.length > 0) {
    results.push({ ok: false, label, detail: problems.join("; ") });
  } else {
    results.push({ ok: true, label, detail: `${checked} file${checked === 1 ? "" : "s"} checked` });
  }
}

const CHAT = "crates/spirit-core/src/engine/chat";
const MANIFEST = "crates/spirit-core/src/harnesses/manifest.rs";
const STATE = "apps/spirit-desktop/src-tauri/src/state.rs";
const PANEL = "packages/spirit-ui/src/next/HarnessesPanel.tsx";

function runStaticChecks() {
  checkFiles("selection and gate modules", [
    {
      file: `${CHAT}/selection.rs`,
      required: ["pub struct ChatPreferences", "pub struct JsonChatPreferences", "pub struct ChatHarnessCapture"],
    },
    {
      file: `${CHAT}/harness.rs`,
      required: ["pub enum HarnessChatGate", "pub fn preflight", "pub fn dry_run", "pub fn gate_for_manifest"],
    },
  ]);

  checkFiles("provider, router and ACP adapter", [
    { file: `${CHAT}/adapter.rs`, required: ["pub trait HarnessChatAdapter", "pub struct DenyPermissionPolicy"] },
    { file: `${CHAT}/acp_adapter.rs`, required: ["pub struct AcpChatAdapter"] },
    {
      file: `${CHAT}/router.rs`,
      required: ["pub struct HarnessChatProvider", "pub fn set_adapter", "pub fn clear_adapter", "pub fn subscribe"],
    },
    { file: `${CHAT}/mod.rs`, required: ["pub use acp_adapter::AcpChatAdapter", "pub use router::HarnessChatProvider"] },
  ]);

  checkFiles("chat_argv in the manifest and built-in argv", [
    {
      file: MANIFEST,
      required: [
        "pub chat_argv: Vec<String>",
        "fn validate_chat_argv",
        'id: "goose"',
        'id: "cursor-cli"',
        'chat_argv: &["acp"]',
      ],
    },
    { file: "crates/spirit-core/src/harnesses/toml.rs", required: ["chat_argv"] },
  ]);

  checkFiles("desktop state, router and event forwarding", [
    {
      file: STATE,
      required: [
        "chat_harness: Arc<HarnessChatProvider>",
        "fn install_harness_adapter",
        "AcpChatAdapter::new",
        "fn acp_launch_spec",
        "fn request_harness_chat_shutdown",
      ],
    },
    { file: "apps/spirit-desktop/src-tauri/src/lib.rs", required: ["chat_harness().subscribe()", "chat_harness_settings"] },
    { file: "apps/spirit-desktop/src-tauri/src/commands/chat.rs", required: ["remove_adapter", "install_harness_adapter"] },
  ]);

  checkFiles("Settings Chat transport and custom launch-argv form", [
    {
      file: PANEL,
      required: ['data-testid="harness-chat-transport"', "showsChatArgv", "validateChatArgv", "chat_argv"],
    },
  ]);

  checkFiles("core and UI tests", [
    { file: "crates/spirit-core/tests/harness_chat_selection.rs" },
    { file: "crates/spirit-core/tests/harness_chat_acp.rs" },
    { file: "crates/spirit-core/tests/harness_chat_provider.rs" },
    { file: "crates/spirit-core/tests/harness_chat_router.rs" },
    { file: "crates/spirit-core/tests/harnesses.rs", required: ["fn chat_argv_validation_accepts_acp_and_rejects_unsafe_values"] },
    { file: "packages/spirit-ui/src/test/next/harnessChatTransport.test.tsx" },
    { file: "packages/spirit-ui/src/test/next/harnessCustomChatArgv.test.tsx" },
  ]);
}

const CHECKLIST = `StackSpirit harness chat owner checklist
docs/HARNESS-CHAT.md §7.5

This is an OWNER-RUN checklist. This script printed it; it did not run any step, start a
harness, spawn VOWP or the app, open a socket or read a secret. These checks remain unrun.
${POLICY}
There is no ChatView harness chip or permission card yet; record what the UI shows.

Redaction rule: never put a key, token, URL, port, native session id, real hostname or
absolute path in the owner report. Use an angle-bracket placeholder instead.

1. Install/auth state
   Check: the chosen ACP harness is installed and signed in; capture its exact --version.
   Record: binary found <YES_NO>; auth state <STATE>; version <VERSION>.

2. One real turn
   Check: select the harness as the chat default, start a Setup chat, send one short turn.
   Record: text streamed <YES_NO>; done event once <YES_NO>; other harness processes <NONE>.

3. Permission allow/reject
   Check: trigger a tool request. The default policy denies it; note any configured policy.
   Record: request denied by default <YES_NO>; allow path available <YES_NO>; option ids <REDACTED>.

4. Abort
   Check: abort a long turn.
   Record: turn stopped <YES_NO>; child still alive <YES_NO>.

5. Restart/resume
   Check: quit and reopen the app, then reopen the conversation.
   Record: resumed <YES_NO>; native id shown <NO>.

6. Model and usage
   Check: model list and usage/cost the harness reports.
   Record: models reported <YES_NO>; usage reported <YES_NO>; invented defaults <NONE>.

7. Subscription terms
   Check: the vendor terms for the sign-in used.
   Record: terms allow third-party client use <YES_NO>; ToS note shown <YES_NO>.

8. Process cleanup on macOS and Windows
   Check: after close, abort and app exit, no harness process or child remains.
   Record: macOS clean <YES_NO>; Windows clean <YES_NO>.

9. No secret in argv/logs/DOM
   Check: plant a fake value; scan the process argv, app logs and DOM.
   Record: argv matches <0>; log matches <0>; DOM matches <0>; canary absent <YES>.

Reminder: ${POLICY.replace(/\n/g, " ")} Harness chat stays not live and not claimed
until this checklist passes.`;

function runCheck() {
  console.log("StackSpirit harness chat static check (docs/HARNESS-CHAT.md §7.5)");
  console.log("");
  runStaticChecks();
  for (const item of results) {
    console.log(`${item.ok ? "PASS" : "FAIL"}  ${item.label}: ${item.detail}`);
  }
  const failed = results.filter((item) => !item.ok);
  console.log("");
  if (failed.length > 0) {
    console.log(`Result: FAIL (${failed.length} of ${results.length} checks)`);
    console.log("Missing:");
    for (const item of failed) console.log(`  - ${item.label}: ${item.detail}`);
  } else {
    console.log(`Result: PASS (${results.length} static repository checks)`);
  }
  console.log("");
  console.log(POLICY);
  console.log("No harness, VOWP, app, shell, network, .env, keychain or app-data access was performed.");
  console.log("");
  console.log(CHECKLIST);
  return failed.length > 0 ? 1 : 0;
}

function usage(message) {
  if (message) console.error(message);
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
if (mode === "check") {
  process.exitCode = runCheck();
} else {
  console.log(CHECKLIST);
}
