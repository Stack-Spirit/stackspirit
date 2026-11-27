import { useEffect, useMemo, useRef, useState } from "react";

import { toSpiritError, type SpiritError } from "../bridge/errors";
import type { SpiritBridge } from "../bridge/SpiritBridge";
import type { HarnessManifest, HarnessTarget } from "../bridge/wire";

export interface CliMcpConnectProps {
  bridge: SpiritBridge;
  /** True when a saved Home server is connected and can receive harness sign-ins. */
  homeConnected: boolean;
  homeLabel: string;
  /** Optional shortcut when no Home server is connected. */
  onRequestConnectServer?: () => void;
  /** Opens the Account rail item for StackSpirit Account/relay setup. */
  onOpenAccount?: () => void;
}

type ToolState = "idle" | "starting" | "started" | "failed";

function targetLabel(target: HarnessTarget, homeLabel: string): string {
  return target === "local" ? "This Mac" : homeLabel;
}

function slug(value: string): string {
  const clean = value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return clean || "home";
}

/**
 * The MCP server entry is a local stdio registration. It deliberately contains no
 * token, password, key or SSH argument: the CLI resolves the saved Home through the
 * desktop host. The Home name is only used to give the entry a readable label.
 */
export function mcpConfigSnippet(target: HarnessTarget, homeLabel: string): string {
  const name = target === "home_vps" ? `stackspirit-${slug(homeLabel)}` : "stackspirit";
  return JSON.stringify(
    {
      mcpServers: {
        [name]: {
          command: "stackspirit",
          args: ["mcp"],
        },
      },
    },
    null,
    2,
  );
}

function statusLabel(state: ToolState): string {
  switch (state) {
    case "starting":
      return "Starting…";
    case "started":
      return "Sign-in started";
    case "failed":
      return "Needs attention";
    default:
      return "Not connected";
  }
}

/**
 * Settings → CLI & MCP Connect.
 *
 * The panel has one target switch for both targets; every action is explicit. External
 * harness sign-ins go through the real desktop `SpiritBridge.harnesses.connectStart`
 * surface, so a Home sign-in can use the host-owned loopback forward. The StackSpirit
 * CLI install command and the MCP JSON snippet are copy-only and contain no secret; the
 * panel never opens an SSH tunnel or invents a connected state.
 */
export function CliMcpConnect({
  bridge,
  homeConnected,
  homeLabel,
  onRequestConnectServer,
  onOpenAccount,
}: CliMcpConnectProps) {
  const surface = bridge.harnesses;
  const desktop = bridge.kind === "tauri" && !bridge.preview;
  const [target, setTarget] = useState<HarnessTarget>("local");
  const [manifests, setManifests] = useState<HarnessManifest[]>([]);
  const [listError, setListError] = useState<SpiritError | null>(null);
  const [toolStates, setToolStates] = useState<Record<string, ToolState>>({});
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<SpiritError | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [copyError, setCopyError] = useState<string | null>(null);
  const copiedTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      if (copiedTimer.current) clearTimeout(copiedTimer.current);
    };
  }, []);

  useEffect(() => {
    if (!surface) return;
    let cancelled = false;
    surface
      .list()
      .then((view) => {
        if (cancelled) return;
        setManifests(
          view.harnesses.filter(
            (manifest) => manifest.builtin && manifest.id !== "opencode",
          ),
        );
        setListError(null);
      })
      .catch((caught: unknown) => {
        if (cancelled) return;
        setListError(toSpiritError(caught));
      });
    return () => {
      cancelled = true;
    };
  }, [surface]);

  const localTargetLabel = "This Mac";
  const homeTargetLabel = homeLabel;
  // A Home that disconnects falls back to Local without a state-writing effect.
  const activeTarget: HarnessTarget = homeConnected ? target : "local";
  const currentTargetLabel = targetLabel(activeTarget, homeLabel);
  const installCommand = "npm install -g stackspirit";
  const snippet = useMemo(
    () => mcpConfigSnippet(activeTarget, homeLabel),
    [activeTarget, homeLabel],
  );

  async function copy(text: string, label: string) {
    setCopyError(null);
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error("This window cannot copy to the clipboard.");
      }
      await navigator.clipboard.writeText(text);
      if (!alive.current) return;
      setCopied(label);
      if (copiedTimer.current) clearTimeout(copiedTimer.current);
      copiedTimer.current = setTimeout(() => {
        if (alive.current) setCopied(null);
      }, 2000);
    } catch (caught) {
      if (!alive.current) return;
      setCopyError(
        caught instanceof Error ? caught.message : "The clipboard is not available.",
      );
    }
  }

  async function connect(manifest: HarnessManifest) {
    if (!surface) return;
    setToolStates((current) => ({ ...current, [manifest.id]: "starting" }));
    setError(null);
    setNotice(null);
    try {
      const started = await surface.connectStart({ id: manifest.id, target: activeTarget });
      if (!alive.current) return;
      setToolStates((current) => ({ ...current, [manifest.id]: "started" }));
      setNotice(
        `${manifest.name} · ${currentTargetLabel}. ${started.message}${
          started.forward_port
            ? ` The desktop host owns the temporary loopback forward on 127.0.0.1:${started.forward_port}; no manual SSH tunnel is needed.`
            : ""
        }`,
      );
    } catch (caught) {
      if (!alive.current) return;
      setToolStates((current) => ({ ...current, [manifest.id]: "failed" }));
      setError(toSpiritError(caught));
    }
  }

  return (
    <div className="nx-settings-section" data-testid="cli-mcp-connect">
      <header className="nx-settings-section__head">
        <h3>CLI &amp; MCP Connect</h3>
        <p>
          Connect the StackSpirit CLI, the MCP server and your external CLI harnesses to
          this Mac or the Always-On Home. The desktop host keeps the connection open, so
          there is no ssh -L to paste.
        </p>
      </header>

      <div className="nx-cli-targets" role="group" aria-label="Connect target">
        <button
          type="button"
          className="nx-cli-target"
          aria-pressed={activeTarget === "local"}
          onClick={() => setTarget("local")}
          data-testid="cli-mcp-target-local"
        >
          <span className="nx-cli-target__name">{localTargetLabel}</span>
          <span className="nx-chip nx-chip--local">Local</span>
          <small>This desktop app runs the connection.</small>
        </button>
        <button
          type="button"
          className="nx-cli-target"
          aria-pressed={activeTarget === "home_vps"}
          disabled={!homeConnected}
          onClick={() => setTarget("home_vps")}
          data-testid="cli-mcp-target-home"
          title={homeConnected ? undefined : "Connect a saved Home VPS first."}
        >
          <span className="nx-cli-target__name">{homeTargetLabel}</span>
          <span className={`nx-chip ${homeConnected ? "nx-chip--server" : "nx-chip--muted"}`}>
            {homeConnected ? "Connected" : "No Home"}
          </span>
          <small>
            {homeConnected
              ? "Encrypted relay and loopback sign-ins are owned by the host."
              : "Connect a saved Home VPS first."}
          </small>
        </button>
      </div>

      {!homeConnected && onRequestConnectServer ? (
        <p className="nx-form__hint">
          Want Home instead of this Mac?{" "}
          <button
            type="button"
            className="nx-link"
            onClick={onRequestConnectServer}
          >
            Connect a Home VPS
          </button>
        </p>
      ) : null}

      <section className="nx-settings__card" aria-labelledby="cli-install-heading">
        <div className="nx-block__head">
          <div>
            <h4 id="cli-install-heading">StackSpirit CLI</h4>
            <p className="nx-form__hint">
              Install once on {currentTargetLabel}. The same binary serves chat, sessions
              and the MCP server.
            </p>
          </div>
          <span className="nx-chip nx-chip--muted">{currentTargetLabel}</span>
        </div>
        <div className="nx-cli-command">
          <code data-testid="cli-install-command">{installCommand}</code>
          <button
            type="button"
            className="nx-btn nx-btn--quiet nx-btn--sm"
            onClick={() => void copy(installCommand, "Install command")}
            data-testid="copy-cli-install"
          >
            {copied === "Install command" ? "Copied" : "Copy install command"}
          </button>
        </div>
        <p className="nx-form__hint">
          The command is shown, never run automatically. It contains no account token or
          key; StackSpirit's own sign-in happens once on the target.
        </p>
      </section>

      <section className="nx-settings__card" aria-labelledby="mcp-connect-heading">
        <div className="nx-block__head">
          <div>
            <h4 id="mcp-connect-heading">MCP server</h4>
            <p className="nx-form__hint">
              Add this entry to any MCP-capable harness on {currentTargetLabel}. The CLI
              resolves the saved machine; no token or SSH argument is included.
            </p>
          </div>
          <span className="nx-chip nx-chip--muted">stdio</span>
        </div>
        <pre className="nx-cli-snippet" data-testid="mcp-snippet">
          <code>{snippet}</code>
        </pre>
        <div className="nx-cli-command__actions">
          <button
            type="button"
            className="nx-btn nx-btn--sm"
            onClick={() => void copy(snippet, "MCP config")}
            data-testid="copy-mcp-config"
          >
            {copied === "MCP config" ? "Copied" : "Copy MCP config"}
          </button>
          {activeTarget === "home_vps" ? (
            <span className="nx-form__hint">
              Home sign-ins use the host-owned loopback forward; no ssh -L command.
            </span>
          ) : null}
        </div>
      </section>

      <section className="nx-settings__card" aria-labelledby="cli-tools-heading">
        <div className="nx-block__head">
          <div>
            <h4 id="cli-tools-heading">External CLI tools</h4>
            <p className="nx-form__hint">
              One-click sign-in for the built-in harnesses on {currentTargetLabel}. The
              embedded chat still runs on OpenCode; these tools stay worker/CLI surfaces.
            </p>
          </div>
          <span className={`nx-chip ${desktop ? "nx-chip--local" : "nx-chip--muted"}`}>
            {desktop ? "Desktop" : "Desktop only"}
          </span>
        </div>

        {!surface ? (
          <p className="nx-form__hint" data-testid="cli-tools-unavailable">
            External CLI sign-in needs the desktop harness registry. This bridge does not
            expose it, so StackSpirit does not pretend to connect a tool here.
          </p>
        ) : listError ? (
          <p className="nx-form__error" role="alert">
            {listError.message}
            {listError.hint ? <span> {listError.hint}</span> : null}
          </p>
        ) : manifests.length === 0 ? (
          <p className="nx-form__hint" role="status">
            Reading the built-in harness list…
          </p>
        ) : (
          <ul className="nx-cli-tools" aria-label="External CLI tools">
            {manifests.map((manifest) => {
              const state = toolStates[manifest.id] ?? "idle";
              return (
                <li
                  className="nx-cli-tool"
                  key={manifest.id}
                  data-testid={`cli-tool-${manifest.id}`}
                >
                  <div className="nx-cli-tool__name">
                    <strong>{manifest.name}</strong>
                    <span className="nx-provider__meta">
                      <code>{manifest.binary}</code> ·{" "}
                      {manifest.supports_embedded_chat ? "Embedded Chat" : "Worker-Only"}
                    </span>
                  </div>
                  <span
                    className={`nx-chip ${
                      state === "started"
                        ? "nx-chip--server"
                        : state === "failed"
                          ? "nx-chip--danger"
                          : state === "starting"
                            ? "nx-chip--warn"
                            : "nx-chip--muted"
                    }`}
                  >
                    {statusLabel(state)}
                  </span>
                  <button
                    type="button"
                    className="nx-btn nx-btn--sm"
                    disabled={!desktop || !surface || state === "starting"}
                    aria-label={`Connect ${manifest.name} on ${currentTargetLabel}`}
                    title={
                      !desktop
                        ? "Connect is available in the desktop app only."
                        : undefined
                    }
                    onClick={() => void connect(manifest)}
                  >
                    {state === "starting" ? "Starting…" : "Connect"}
                  </button>
                </li>
              );
            })}
          </ul>
        )}

        {onOpenAccount ? (
          <p className="nx-form__hint">
            Using the encrypted relay or a hosted Remote MCP endpoint?{" "}
            <button type="button" className="nx-link" onClick={onOpenAccount}>
              Open Account
            </button>
          </p>
        ) : null}
      </section>

      {copied ? (
        <p className="nx-form__hint" role="status" data-testid="cli-mcp-copied">
          {copied} copied.
        </p>
      ) : null}
      {copyError ? (
        <p className="nx-form__error" role="alert">
          {copyError}
        </p>
      ) : null}
      {notice ? (
        <div className="nx-banner" role="status" data-testid="cli-mcp-notice">
          {notice}
        </div>
      ) : null}
      {error ? (
        <p className="nx-form__error nx-form__error--block" role="alert">
          {error.message}
          {error.hint ? <span className="nx-form__hint"> {error.hint}</span> : null}
        </p>
      ) : null}
    </div>
  );
}
