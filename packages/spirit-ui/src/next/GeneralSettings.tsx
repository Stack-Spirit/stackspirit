import {
  useConnection,
  useFleetStore,
  useHosts,
  useSelectedHostId,
} from "../state/fleetStore";
import type { ThemePreference } from "./themePreference";

export interface GeneralSettingsProps {
  theme: ThemePreference;
  onTheme: (theme: ThemePreference) => void;
  onOpenRaw?: () => void;
}

const THEME_OPTIONS: Array<{ value: ThemePreference; label: string }> = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

/**
 * Settings → General. Appearance is limited to facts the shell actually applies
 * (the Meadow theme switch); workspace defaults point at the saved server the app
 * already remembers. Session export and the raw document stay here because they are
 * data controls, not machine setup.
 */
export function GeneralSettings({ theme, onTheme, onOpenRaw }: GeneralSettingsProps) {
  const hosts = useHosts();
  const connection = useConnection();
  const selectedHostId = useSelectedHostId();
  const selectHost = useFleetStore((state) => state.selectHost);
  const openSessions = useFleetStore((state) => state.openSessions);
  const sessionsAvailable = useFleetStore((state) => Boolean(state.bridge?.sessions));
  const connected = connection.state === "connected" || connection.state === "stale";
  const activeHost =
    hosts.find((host) => host.id === connection.host_id) ??
    hosts.find((host) => host.id === selectedHostId) ??
    hosts[0] ??
    null;

  return (
    <div className="nx-settings-section" data-testid="settings-general">
      <header className="nx-settings-section__head">
        <h3>General</h3>
        <p>Theme, the shell's everyday appearance and the workspace StackSpirit opens.</p>
      </header>

      <section className="nx-settings__card" aria-labelledby="settings-appearance">
        <h4 id="settings-appearance">Appearance</h4>
        <div className="nx-settings__row">
          <span>
            Theme
            <small>System follows your computer's light or dark setting.</small>
          </span>
          <div className="nx-segment" role="group" aria-label="Theme">
            {THEME_OPTIONS.map((option) => (
              <button
                key={option.value}
                type="button"
                aria-pressed={theme === option.value}
                onClick={() => onTheme(option.value)}
              >
                {option.label}
              </button>
            ))}
          </div>
        </div>
        <div className="nx-settings__row">
          <span>
            Interface
            <small>Meadow is the calm, chat-first shell built for long sessions.</small>
          </span>
          <span className="nx-chip nx-chip--server">Meadow</span>
        </div>
      </section>

      <section className="nx-settings__card" aria-labelledby="settings-workspace">
        <h4 id="settings-workspace">Workspace defaults</h4>
        <label className="nx-settings__row" htmlFor="settings-default-server">
          <span>
            Default server
            <small>The saved server the sidebar and Server page open first.</small>
          </span>
          <select
            id="settings-default-server"
            className="nx-select"
            value={activeHost?.id ?? ""}
            disabled={hosts.length === 0}
            onChange={(event) => selectHost(event.target.value)}
          >
            {hosts.length === 0 ? <option value="">No saved server</option> : null}
            {hosts.map((host) => (
              <option key={host.id} value={host.id}>
                {host.label}
              </option>
            ))}
          </select>
        </label>
        <div className="nx-settings__row">
          <span>
            New project chats
            <small>
              A project chat is created on this Mac when its local agent is connected;
              otherwise it stays on Home.
            </small>
          </span>
          <span className="nx-chip nx-chip--local">Local first</span>
        </div>
        <div className="nx-settings__row">
          <span>
            Global chat
            <small>Global sessions stay on Local until Home chat transport is approved.</small>
          </span>
          <span className="nx-chip nx-chip--local">Local · this Mac</span>
        </div>
      </section>

      <section className="nx-settings__card" aria-labelledby="settings-data">
        <h4 id="settings-data">Sessions &amp; data</h4>
        <div className="nx-settings__row">
          <span>
            Portable sessions
            <small>Export and import .asif.zip session packages for a connected Home.</small>
          </span>
          <span className="nx-settings__actions">
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
      </section>
    </div>
  );
}
