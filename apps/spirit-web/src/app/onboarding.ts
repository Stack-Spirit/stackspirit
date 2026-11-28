/**
 * The agent-first onboarding copy for a signed-in account with no enrolled
 * Always-On Machine.
 *
 * The setup prompt is intentionally plain text: it is copied verbatim into
 * OpenCode, Claude Code, Cursor or Codex. It asks the coding agent to log the
 * CLI in, request the SSH host, install StackSpirit on that host and enroll it
 * through the relay. It never asks for a private key, and the browser never
 * receives one.
 */

export const DESKTOP_RELEASES_URL =
  "https://github.com/Stack-Spirit/stackspirit/releases/latest";

export const AGENT_SETUP_PROMPT =
  "Run npx -y stackspirit login, ask me for the SSH host of my always-on machine, install StackSpirit and run stackspirit relay enroll so it appears under stackspirit machines ls.\n" +
  "Use the one-time ssm_ enrollment token from StackSpirit's Manual terminal setup if relay enroll asks for one. Never ask me for SSH private keys.";
