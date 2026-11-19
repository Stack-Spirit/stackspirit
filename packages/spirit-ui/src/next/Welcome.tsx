import { SpiritMark } from "./SpiritMark";

export interface WelcomeProps {
  onAddServer: () => void;
  onSetupWithAi: () => void;
}

/** Full-page first run: no server saved on this computer yet. */
export function Welcome({ onAddServer, onSetupWithAi }: WelcomeProps) {
  return (
    <main className="nx-welcome" data-testid="next-welcome">
      <div className="nx-welcome__inner">
        <SpiritMark size={132} label="StackSpirit" />
        <h1 className="nx-welcome__title">No always-on server connected</h1>
        <p className="nx-welcome__lede">
          Connect a VPS or a computer at home that you can reach over SSH. Your agents keep working
          there while this computer sleeps.
        </p>
        <div className="nx-choices">
          <button type="button" className="nx-choice" onClick={onAddServer}>
            <span className="nx-choice__icon" aria-hidden="true">
              ＋
            </span>
            <span className="nx-choice__title">Add server</span>
            <span className="nx-choice__body">
              Enter the address and SSH key yourself. Takes a minute if your key is already on the
              server.
            </span>
          </button>
          <button type="button" className="nx-choice" onClick={onSetupWithAi}>
            <span className="nx-choice__icon" aria-hidden="true">
              ✦
            </span>
            <span className="nx-choice__title">Set up with AI</span>
            <span className="nx-choice__body">
              An assistant walks you through it with a free built-in model. No account or API key
              needed.
            </span>
          </button>
        </div>
        <p className="nx-welcome__fine">
          Passwords are used once and never saved. Secrets are typed into secure fields the
          assistant cannot read.
        </p>
      </div>
    </main>
  );
}

export interface ConnectPromptProps {
  serverLabel: string;
  address: string;
  state: string;
  error: string | null;
  onConnect: () => void;
  onChatWithSetupAssistant: () => void;
}

/** Main area when a server is saved but not connected. */
export function ConnectPrompt({
  serverLabel,
  address,
  state,
  error,
  onConnect,
  onChatWithSetupAssistant,
}: ConnectPromptProps) {
  const connecting = state === "connecting";
  return (
    <section className="nx-main" aria-label="Connect">
      <div className="nx-empty-chat" style={{ alignContent: "center", minHeight: "100%", paddingTop: 0 }}>
        <SpiritMark size={96} />
        <h2>{serverLabel} is not connected</h2>
        <p className="mono" style={{ fontFamily: "var(--font-mono)", fontSize: 13 }}>
          {address}
        </p>
        {error ? (
          <p role="alert" style={{ color: "var(--nx-danger)" }}>
            {error}
          </p>
        ) : null}
        <div className="nx-connect-actions">
          <button
            type="button"
            className="nx-btn"
            style={{ padding: "10px 28px", fontSize: 14 }}
            onClick={onConnect}
            disabled={connecting}
            data-testid="connect-prompt-connect"
          >
            {connecting ? "Connecting…" : "Connect"}
          </button>
          <button
            type="button"
            className="nx-btn nx-btn--quiet"
            onClick={onChatWithSetupAssistant}
            data-testid="connect-prompt-chat"
          >
            Chat with the setup assistant
          </button>
        </div>
        <p className="nx-welcome__fine" style={{ maxWidth: 360 }}>
          The setup assistant chats on this Mac with the bundled engine. No server connection
          needed.
        </p>
      </div>
    </section>
  );
}
