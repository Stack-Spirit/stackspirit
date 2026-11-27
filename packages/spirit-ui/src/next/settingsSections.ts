/**
 * The left navigation rail inside the Settings modal.
 *
 * The order is deliberate: everyday appearance first, then account and machines,
 * with harnesses and the CLI/MCP wiring last. Keep labels short enough to fit the
 * fixed rail at the smallest desktop width.
 */
export const SETTINGS_SECTIONS = [
  { id: "general", label: "General", hint: "Theme, appearance and workspace defaults" },
  { id: "account", label: "Account", hint: "Sign in, synced Homes and managed plans" },
  { id: "server", label: "Server & Machines", hint: "Home server, relay, machines and backups" },
  { id: "harnesses", label: "Harnesses", hint: "OpenCode plus one-click external harnesses" },
  { id: "cli-mcp", label: "CLI & MCP Connect", hint: "CLI, MCP and external tools on both targets" },
] as const;

export type SettingsSectionId = (typeof SETTINGS_SECTIONS)[number]["id"];

export function isSettingsSection(value: string): value is SettingsSectionId {
  return SETTINGS_SECTIONS.some((section) => section.id === value);
}
