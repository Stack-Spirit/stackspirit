#!/usr/bin/env node
// Session takeover owner verification: static repository checks and the §7.5 checklist.
//
// This is the takeover counterpart to scripts/verify-orchestrator-chat.mjs. It is
// deliberately inert:
//
//   * --check reads repository files only. It never imports node:child_process,
//     spawns vowp/the app/an engine/a shell, opens a socket, or reads .env, keychain,
//     ~/.vowp*, app data, tokens or server profiles.
//   * --checklist prints the fixed §7.5 owner checklist and evidence placeholders. It
//     does not execute them.
//   * --help prints this usage block and the same inertness policy.
//
// Passing --check is not owner evidence. It only proves the shipped pieces are present
// in this checkout. It does not make takeover live on a node-hosted deployment; today's
// live Local chat stays read-only.
//
// Usage:
//   node scripts/verify-session-takeover.mjs --check
//   node scripts/verify-session-takeover.mjs --checklist
//   node scripts/verify-session-takeover.mjs --help
//
// Exit codes:
//   0  every static check passed, or the checklist/help was printed
//   1  a required repository file or symbol is missing
//   2  usage error

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const USAGE = `StackSpirit session takeover verification (docs/SESSION-TAKEOVER.md §7.5)

usage:
  node scripts/verify-session-takeover.mjs --check
      Check that the shipped writer core, hermetic fake-vowp tests and fixtures, Tauri
      commands and registration, bridge wire types and optional methods, gated ChatView
      action and takeover e2e spec are present in this checkout. Then print the fixed
      §7.5 owner checklist. It reads repository files only.

  node scripts/verify-session-takeover.mjs --checklist
      Print the owner-run §7.5 checklist and redacted evidence placeholders. Nothing is
      executed.

  node scripts/verify-session-takeover.mjs --help

Inertness: this script never takes a lease, invokes VOWP, starts the app or an engine,
opens a socket, runs a shell or reads .env, keychain, ~/.vowp*, app data, tokens or
server profiles. Passing --check is not owner evidence and does not make takeover live
on a node-hosted deployment.`;

// ---------------------------------------------------------------------------
// Repository reads and result collection
// ---------------------------------------------------------------------------

const results = [];

function readRepoText(relativePath) {
  try {
    return readFileSync(join(REPO_ROOT, relativePath), "utf8");
  } catch {
    return null;
  }
}

function pass(label, detail) {
  results.push({ ok: true, label, detail });
}

function fail(label, detail) {
  results.push({ ok: false, label, detail });
}

// Checks one or more repository files for required plain-text symbols. A missing file or
// symbol is a failure that names the exact file, so --check fails closed with a precise
// list instead of implying the shipped piece exists.
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
    fail(label, problems.join("; "));
    return false;
  }
  pass(label, `${checked} file${checked === 1 ? "" : "s"} checked`);
  return true;
}

// ---------------------------------------------------------------------------
// Static checks for the real shipped takeover pieces
// ---------------------------------------------------------------------------

function runStaticChecks() {
  checkFiles("core writer manager", [
    {
      file: "crates/spirit-core/src/engine/chat/writer.rs",
      required: [
        "pub struct ChatWriter",
        "pub const WATCH_INTERVAL",
        "pub async fn probe",
        "pub async fn take_over",
        "pub async fn release",
        "pub async fn check_lease",
        "pub fn start_watcher",
      ],
    },
    {
      file: "crates/spirit-core/src/engine/chat/model.rs",
      required: [
        "pub struct ChatWriterView",
        "pub enum ChatWriterState",
        "pub enum ChatWriterReason",
        "pub writer: Option<ChatWriterView>",
      ],
    },
  ]);

  checkFiles("chat writer tests", [
    {
      file: "crates/spirit-core/tests/chat_writer.rs",
      required: [
        "tests/fixtures/fake-vowp/vowp",
        "fn canary",
        "probe_available_uses_the_home_fallback_and_never_spawns_an_attach",
        "take_over_free_attach_validates_reports_and_releases",
        "the_watcher_keeps_a_live_lease_and_flips_a_replaced_one",
      ],
    },
    {
      file: "crates/spirit-core/tests/chat_writer_lifecycle.rs",
      required: [
        "tests/fixtures/fake-vowp/vowp",
        "fn canary",
        "sleep_past_the_stale_window_is_lost_and_a_new_attach_uses_a_new_surface",
      ],
    },
  ]);

  checkFiles("hermetic fake vowp fixture", [
    {
      file: "crates/spirit-core/tests/fixtures/fake-vowp/vowp",
      required: [
        "attach-grant",
        "attach-conflict",
        "attach-node",
        "attach-hang",
        "attach-crash",
        "attach-old",
        "endpoint-owner",
        "endpoint-node",
      ],
    },
    {
      file: "crates/spirit-core/tests/fixtures/fake-vowp/README.md",
      required: ["attach-grant", "endpoint-owner", "canary token"],
    },
  ]);

  checkFiles("Tauri writer commands", [
    {
      file: "apps/spirit-desktop/src-tauri/src/commands/chat.rs",
      required: ["pub async fn chat_take_over", "pub async fn chat_release_writer"],
    },
    {
      file: "apps/spirit-desktop/src-tauri/src/lib.rs",
      required: [
        "commands::chat::chat_take_over,",
        "commands::chat::chat_release_writer,",
      ],
    },
  ]);

  checkFiles("bridge wire types and optional methods", [
    {
      file: "packages/spirit-ui/src/bridge/wire.ts",
      required: [
        "export type ChatWriterState",
        "export type ChatWriterReason",
        "export interface ChatWriterView",
        "writer?: ChatWriterView",
      ],
    },
    {
      file: "packages/spirit-ui/src/bridge/SpiritBridge.ts",
      required: [
        "takeOver?(session: SessionId, takeover?: boolean): Promise<ChatSessionView>;",
        "releaseWriter?(session: SessionId): Promise<ChatSessionView>;",
      ],
    },
    {
      file: "apps/spirit-desktop/src/bridge/TauriBridge.ts",
      required: [
        'call("chat_take_over", { session, takeover })',
        'call("chat_release_writer", { session })',
      ],
    },
  ]);

  checkFiles("gated Take over action and read-only reason", [
    {
      file: "packages/spirit-ui/src/next/ChatView.tsx",
      required: [
        "function WriterControl",
        'data-testid="writer-take-over"',
        "Take over",
        'state === "available" && canOfferTakeOver',
        'writer.reason !== "node_hosted"',
        "function writerReasonCopy",
        'case "node_hosted":',
        'data-testid="writer-reason"',
        "This window stays read-only.",
      ],
    },
  ]);

  checkFiles("takeover e2e spec", [
    {
      file: "apps/spirit-desktop/e2e/chat-takeover.spec.ts",
      required: [
        "Take over this session?",
        'getByTestId("readonly-notice")',
        "a lost lease shows the read-only lost view and no release action",
      ],
    },
  ]);
}

// ---------------------------------------------------------------------------
// --check
// ---------------------------------------------------------------------------

function printResults() {
  for (const item of results) {
    console.log(`[${item.ok ? "PASS" : "FAIL"}] ${item.label}: ${item.detail}`);
  }
}

function runCheck() {
  console.log("StackSpirit session takeover verification - static check");
  console.log("docs/SESSION-TAKEOVER.md §7.5; repository-only; no real takeover");
  console.log("");
  runStaticChecks();
  printResults();
  const failed = results.filter((item) => !item.ok);
  console.log("");
  if (failed.length > 0) {
    console.log(`Result: FAIL (${failed.length} of ${results.length} checks)`);
    console.log("Missing:");
    for (const item of failed) {
      console.log(`  - ${item.label}: ${item.detail}`);
    }
    console.log("");
    console.log(
      "No takeover, VOWP command, app, engine, network, socket, .env, keychain or app-data read was performed.",
    );
    console.log("");
    console.log(CHECKLIST);
    return 1;
  }
  console.log(`Result: PASS (${results.length} static repository checks)`);
  console.log("");
  console.log(CHECKLIST);
  return 0;
}

// ---------------------------------------------------------------------------
// --checklist
// ---------------------------------------------------------------------------

const CHECKLIST = `StackSpirit session takeover owner checklist
docs/SESSION-TAKEOVER.md §7.5

This is an OWNER-RUN checklist. This script printed it; it did not run any step, take
a lease, start the app, invoke VOWP, open a socket or read a secret. The §7.5 checks
remain unrun.

Redaction rule: never put a URL, port, token, native session id, surface id or real
hostname in the owner report. Use an angle-bracket placeholder instead.

1. Grant and hold
   Check: on a co-located home, "vowp orchestrator attach <NAME> --holder stackspirit"
   prints one grant and holds until stdin closes.
   Record: grant printed once <YES_NO>; stdin held until EOF <YES_NO>; release marker <YES_NO>.
   Redact: no URL, port, token, native id or real hostname.

2. Lease holder
   Check: while held, "vowp session lease <SESSION>" names the DesktopUi surface.
   Record: surface kind <SURFACE_KIND>; holder name <HOLDER_NAME>; heartbeat observed <YES_NO>.
   Redact: no URL, port, token, native id or real hostname.

3. Conflict without --takeover
   Check: a second writer without --takeover is refused with the live holder's name.
   Record: exit class <CONFLICT>; holder named <YES_NO>; second lease created <NO>.
   Redact: no URL, port, token, native id or real hostname.

4. Replacement and read-only flip
   Check: a second writer with --takeover replaces the lease; the first app flips
   read-only within one watch interval.
   Record: first app read-only within <WATCH_INTERVAL_S>; new holder <HOLDER_NAME>; false
   "turn stopped" claim <NONE>.
   Redact: no URL, port, token, native id or real hostname.

5. Close release
   Check: closing the app releases within seconds and "vowp session lease <SESSION>"
   shows no owner.
   Record: release observed <YES_NO>; lease owner <NONE>; explicit Release path <YES_NO>.
   Redact: no URL, port, token, native id or real hostname.

6. Hard-kill staleness
   Check: kill the app hard; the lease is stale within 60 s and a new attach succeeds.
   Record: stale within 60 s <YES_NO>; new attach succeeds <YES_NO>; clean release claimed <NO>.
   Redact: no URL, port, token, native id or real hostname.

7. Sleep reissue
   Check: sleep past the 60 s stale window; the next check is read-only and a new attach
   gets a new token.
   Record: read-only on resume <YES_NO>; new surface and token <YES_NO>; old token reused <NO>.
   Redact: no URL, port, token, native id or real hostname.

8. Cooperative direct-engine writer
   Check: an OpenCode window keeps writing after the takeover.
   Record: direct writer still writes <YES_NO>; UI claimed enforcement <NO>.
   Redact: no URL, port, token, native id or real hostname.

9. Node-hosted fallback
   Check: a node-hosted orchestrator returns the typed node_hosted state and no button.
   Record: writer state <NODE_HOSTED>; Take over action <ABSENT>; composer <DISABLED>.
   Redact: no URL, port, token, native id or real hostname.

10. Token-leak scan
    Check: no lease token appears in the app log, a snapshot, the DOM or an error card.
    Record: log matches <0>; snapshot matches <0>; DOM matches <0>; error-card matches <0>;
    canary absent <YES>; surface id/URL/port/native id absent <YES>.
    Redact: no URL, port, token, native id or real hostname.

Reminder: passing --check is not owner evidence. It only proves these shipped pieces
are present in this checkout, and it does not make takeover live on a node-hosted
deployment. Today's live Local chat stays read-only until VOWP provides a node-side
attach or the owner chooses a co-located home deployment.`;

function runChecklist() {
  console.log(CHECKLIST);
  return 0;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

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
process.exitCode = mode === "check" ? runCheck() : runChecklist();
