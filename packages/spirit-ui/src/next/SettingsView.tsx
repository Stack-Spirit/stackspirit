import { useState } from "react";

import { EngineView } from "../views/engine/EngineView";
import { HostManager } from "../views/hosts/HostManager";
import { useConnection, useEngine, useFleetStore } from "../state/fleetStore";
import { PageHeader } from "./PageHeader";
import { SecretStore } from "./SecretStore";
import { ServerSecurityCard } from "./ServerSecurity";
import type { ThemePreference } from "./themePreference";

export interface SettingsViewProps {
  theme: ThemePreference;
  onTheme: (theme: ThemePreference) => void;
  onOpenRaw?: () => void;
}

/**
 * Everyday settings up front; everything else behind "Show advanced settings".
 * Advanced reuses the existing Engine view (models, keys, harness, capability gates)
 * and the host manager, so their logic is unchanged.
 */
export function SettingsView({ theme, onTheme, onOpenRaw }: SettingsViewProps) {
  const engine = useEngine();
  const openSessions = useFleetStore((state) => state.openSessions);
  const sessionsAvailable = useFleetStore((state) => Boolean(state.bridge?.sessions));
  const connected = useConnection().state === "connected";
  const [advanced, setAdvanced] = useState(false);
  const model = engine?.selected_model?.label ?? engine?.selected_model?.id ?? "No model selected";

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
            <h2>AI model</h2>
            <div className="nx-settings__row">
              <span>
                StackSpirit Engine
                <small>Used for the setup assistant and local sessions.</small>
              </span>
              <span className="nx-chip nx-chip--server">◇ {model}</span>
            </div>
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
