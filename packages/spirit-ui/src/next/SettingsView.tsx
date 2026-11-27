import { useState } from "react";

import { EngineView } from "../views/engine/EngineView";
import { HostManager } from "../views/hosts/HostManager";
import {
  useConnection,
  useFleetStore,
  useHosts,
  useSelectedHostId,
} from "../state/fleetStore";
import { CloudAccountPanel } from "./CloudAccountPanel";
import { HarnessesPanel } from "./HarnessesPanel";
import { ManagedHomesPanel } from "./ManagedHomesPanel";
import { PageHeader } from "./PageHeader";
import { SettingsPage } from "./SettingsPage";
import { SecretStore } from "./SecretStore";
import { ServerSecurityCard } from "./ServerSecurity";
import type { ThemePreference } from "./themePreference";

export interface SettingsViewProps {
  theme: ThemePreference;
  onTheme: (theme: ThemePreference) => void;
  onOpenRaw?: () => void;
  /** Open the Engine card on mount (the composer's "Set up a model" link). */
  initialAdvanced?: boolean;
}

/**
 * Everyday settings up front; everything else behind "Show advanced settings".
 * Advanced reuses the existing Engine view (models, keys, harness, capability gates)
 * and the host manager, so their logic is unchanged.
 */
export function SettingsView({
  theme,
  onTheme,
  onOpenRaw,
  initialAdvanced = false,
}: SettingsViewProps) {
  const openSessions = useFleetStore((state) => state.openSessions);
  const sessionsAvailable = useFleetStore((state) => Boolean(state.bridge?.sessions));
  const bridge = useFleetStore((state) => state.bridge);
  const connect = useFleetStore((state) => state.connect);
  const connection = useConnection();
  const hosts = useHosts();
  const selectedHostId = useSelectedHostId();
  const connected = connection.state === "connected";
  const [advanced, setAdvanced] = useState(initialAdvanced);

  // The Harnesses panel probes the connected Home VPS over the same SSH path as the rest
  // of the app. Without a connected server it stays local-only and says so.
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

  return (
    <section className="nx-main" aria-label="Settings">
      <PageHeader title="Settings" />
      <div className="nx-settings">
        <div className="nx-settings__inner">
          <div className="nx-settings__card">
            <h2>Appearance</h2>
            <div className="nx-settings__row">
              <span>
                Theme
                <small>System follows your computer's light or dark setting.</small>
              </span>
              <div className="nx-segment" role="group" aria-label="Theme">
                {(["system", "light", "dark"] as const).map((value) => (
                  <button key={value} type="button" aria-pressed={theme === value} onClick={() => onTheme(value)}>
                    {value === "system" ? "System" : value === "light" ? "Light" : "Dark"}
                  </button>
                ))}
              </div>
            </div>
          </div>

          <div className="nx-settings__card">
            <h2>Engine &amp; Models</h2>
            <SettingsPage />
          </div>

          <div className="nx-settings__card" data-testid="settings-cloud">
            <h2>StackSpirit Cloud</h2>
            <CloudAccountPanel />
            <ManagedHomesPanel />
          </div>

          <ServerSecurityCard />

          <div className="nx-settings__card">
            <div className="nx-settings__row">
              <span>
                <strong>Advanced settings</strong>
                <small>Servers, provider keys, external harnesses and capability checks.</small>
              </span>
              <button
                type="button"
                className="nx-btn nx-btn--quiet"
                aria-expanded={advanced}
                onClick={() => setAdvanced((value) => !value)}
              >
                {advanced ? "Hide advanced settings" : "Show advanced settings"}
              </button>
            </div>
            {advanced ? (
              <div className="nx-advanced">
                <div className="nx-settings__row">
                  <span>
                    Sessions and .asif.zip
                    <small>Export and import portable session packages when StackSpirit supports them.</small>
                  </span>
                  <span style={{ display: "flex", gap: 8 }}>
                    <button
                      type="button"
                      className="nx-btn nx-btn--quiet nx-btn--sm"
                      disabled={!sessionsAvailable || !connected}
                      onClick={() => openSessions("list")}
                    >
                      Sessions
                    </button>
                    {onOpenRaw ? (
                      <button
                        type="button"
                        className="nx-btn nx-btn--quiet nx-btn--sm"
                        disabled={!connected}
                        onClick={onOpenRaw}
                      >
                        Raw StackSpirit document
                      </button>
                    ) : null}
                  </span>
                </div>
                <SecretStore />
                <HostManager />
                <EngineView />
                {bridge ? (
                  <div className="nx-settings__card">
                    <HarnessesPanel
                      bridge={bridge}
                      homeConnected={connected}
                      homeLabel={homeLabel}
                      onRequestConnectServer={requestConnectServer}
                    />
                  </div>
                ) : (
                  <div className="nx-settings__card" data-testid="harnesses-unavailable">
                    <h2>Harnesses</h2>
                    <p>
                      The harness registry appears once the app bridge is ready.
                    </p>
                  </div>
                )}
              </div>
            ) : null}
          </div>
        </div>
      </div>
    </section>
  );
}
