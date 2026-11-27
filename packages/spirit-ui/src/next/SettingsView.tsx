import { useState, type KeyboardEvent } from "react";

import {
  useConnection,
  useFleetStore,
  useHosts,
  useSelectedHostId,
} from "../state/fleetStore";
import { AccountSettings } from "./AccountSettings";
import { CliMcpConnect } from "./CliMcpConnect";
import { GeneralSettings } from "./GeneralSettings";
import { HarnessesPanel } from "./HarnessesPanel";
import { ServerMachinesSettings } from "./ServerMachinesSettings";
import {
  SETTINGS_SECTIONS,
  type SettingsSectionId,
} from "./settingsSections";
import type { ThemePreference } from "./themePreference";

export interface SettingsViewProps {
  theme: ThemePreference;
  onTheme: (theme: ThemePreference) => void;
  onOpenRaw?: () => void;
  /**
   * Legacy alias: opens the Harnesses rail item (OpenCode providers) on mount, which is
   * where the composer's "Set up a model" link now lands.
   */
  initialAdvanced?: boolean;
  /** Rail item to select on mount; used by the sidebar, account and composer links. */
  initialSection?: SettingsSectionId;
}

/**
 * The Settings surface: a fixed-width left navigation rail and one scrollable content
 * pane, laid out like the centered popups in a modern agent harness. The component owns
 * only the rail and the five StackSpirit categories; the portal, backdrop and focus trap
 * live in `SettingsModal`.
 *
 * Existing panels are reused inside the categories rather than rewritten: the account
 * store, Managed Homes catalog, host manager, server secrets, engine/gates view and the
 * Universal Harness Registry keep their tested behaviour.
 */
export function SettingsView({
  theme,
  onTheme,
  onOpenRaw,
  initialAdvanced = false,
  initialSection,
}: SettingsViewProps) {
  const [section, setSection] = useState<SettingsSectionId>(
    initialSection ?? (initialAdvanced ? "harnesses" : "general"),
  );

  const bridge = useFleetStore((state) => state.bridge);
  const connect = useFleetStore((state) => state.connect);
  const connection = useConnection();
  const hosts = useHosts();
  const selectedHostId = useSelectedHostId();
  const connected = connection.state === "connected";
  const connectedHost = hosts.find((host) => host.id === connection.host_id) ?? null;
  const fallbackHost =
    hosts.find((host) => host.id === selectedHostId) ?? hosts[0] ?? null;
  const homeLabel = (connectedHost ?? fallbackHost)?.label ?? "Home VPS";
  const requestConnectServer =
    !connected && fallbackHost
      ? () => {
          void connect(fallbackHost.id);
        }
      : undefined;

  function focusSection(id: SettingsSectionId) {
    setSection(id);
    document.getElementById(`settings-tab-${id}`)?.focus();
  }

  function onRailKeyDown(event: KeyboardEvent<HTMLElement>) {
    const index = SETTINGS_SECTIONS.findIndex((item) => item.id === section);
    if (event.key === "ArrowDown") {
      event.preventDefault();
      focusSection(SETTINGS_SECTIONS[(index + 1) % SETTINGS_SECTIONS.length]!.id);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      focusSection(
        SETTINGS_SECTIONS[
          (index - 1 + SETTINGS_SECTIONS.length) % SETTINGS_SECTIONS.length
        ]!.id,
      );
    } else if (event.key === "Home") {
      event.preventDefault();
      focusSection(SETTINGS_SECTIONS[0]!.id);
    } else if (event.key === "End") {
      event.preventDefault();
      focusSection(SETTINGS_SECTIONS[SETTINGS_SECTIONS.length - 1]!.id);
    }
  }

  const active = SETTINGS_SECTIONS.find((item) => item.id === section) ?? SETTINGS_SECTIONS[0];

  return (
    <div className="nx-settings-surface" data-testid="settings-view">
      <nav className="nx-settings-rail" aria-label="Settings sections">
        <div
          className="nx-settings-rail__tabs"
          role="tablist"
          aria-orientation="vertical"
          onKeyDown={onRailKeyDown}
        >
          {SETTINGS_SECTIONS.map((item) => (
            <button
              key={item.id}
              type="button"
              role="tab"
              id={`settings-tab-${item.id}`}
              aria-selected={section === item.id}
              aria-controls={`settings-pane-${item.id}`}
              tabIndex={section === item.id ? 0 : -1}
              className="nx-settings-rail__tab"
              onClick={() => setSection(item.id)}
            >
              <span>{item.label}</span>
            </button>
          ))}
        </div>
        <p className="nx-settings-rail__hint" aria-hidden="true">
          {active.hint}
        </p>
      </nav>

      <div
        className="nx-settings-pane"
        id={`settings-pane-${section}`}
        role="tabpanel"
        aria-labelledby={`settings-tab-${section}`}
        tabIndex={0}
        data-testid={`settings-pane-${section}`}
      >
        {section === "general" ? (
          <GeneralSettings theme={theme} onTheme={onTheme} onOpenRaw={onOpenRaw} />
        ) : null}
        {section === "account" ? <AccountSettings /> : null}
        {section === "server" ? (
          <ServerMachinesSettings onOpenAccount={() => setSection("account")} />
        ) : null}
        {section === "harnesses" ? (
          bridge ? (
            <div className="nx-settings-section" data-testid="settings-harnesses">
              <header className="nx-settings-section__head">
                <h3>Harnesses</h3>
                <p>
                  OpenCode is the default overarching harness. Connect the others on this
                  Mac or the Home VPS; embedded chat still runs on OpenCode.
                </p>
              </header>
              <HarnessesPanel
                bridge={bridge}
                homeConnected={connected}
                homeLabel={homeLabel}
                onRequestConnectServer={requestConnectServer}
              />
            </div>
          ) : (
            <div className="nx-settings__card" data-testid="harnesses-unavailable">
              <h3>Harnesses</h3>
              <p>The harness registry appears once the app bridge is ready.</p>
            </div>
          )
        ) : null}
        {section === "cli-mcp" ? (
          bridge ? (
            <CliMcpConnect
              bridge={bridge}
              homeConnected={connected}
              homeLabel={homeLabel}
              onRequestConnectServer={requestConnectServer}
              onOpenAccount={() => setSection("account")}
            />
          ) : (
            <div className="nx-settings__card" data-testid="cli-mcp-unavailable">
              <h3>CLI &amp; MCP Connect</h3>
              <p>The connect panel appears once the app bridge is ready.</p>
            </div>
          )
        ) : null}
      </div>
    </div>
  );
}
