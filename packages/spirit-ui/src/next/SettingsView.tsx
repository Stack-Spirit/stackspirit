import { useState } from "react";

import { EngineView } from "../views/engine/EngineView";
import { HostManager } from "../views/hosts/HostManager";
import { useEngine } from "../state/fleetStore";
import { PageHeader } from "./PageHeader";
import type { ThemePreference } from "./themePreference";

export interface SettingsViewProps {
  theme: ThemePreference;
  onTheme: (theme: ThemePreference) => void;
}

/**
 * Everyday settings up front; everything else behind "Show advanced settings".
 * Advanced reuses the existing Engine view (models, keys, harness, capability gates)
 * and the host manager, so their logic is unchanged.
 */
export function SettingsView({ theme, onTheme }: SettingsViewProps) {
  const engine = useEngine();
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
