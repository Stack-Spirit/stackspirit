import { useState } from "react";

import { EngineView } from "../views/engine/EngineView";
import { HostManager } from "../views/hosts/HostManager";
import { useConnection, useFleetStore } from "../state/fleetStore";
import { EngineModelsSettings } from "./EngineModelsSettings";
import { PageHeader } from "./PageHeader";
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
  const connected = useConnection().state === "connected";
  const [advanced, setAdvanced] = useState(initialAdvanced);

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

          <EngineModelsSettings />

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
                    <small>Export and import portable session packages when VOWP supports them.</small>
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
                        Raw VOWP document
                      </button>
                    ) : null}
                  </span>
                </div>
                <SecretStore />
                <HostManager />
                <EngineView />
              </div>
            ) : null}
          </div>
        </div>
      </div>
    </section>
  );
}
