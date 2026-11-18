import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent } from "react";

import { errorTitle, toSpiritError, type SpiritError } from "../bridge/errors";
import type { HostDraft, HostKeyPin, HostKeyScan } from "../bridge/wire";
import { useEngine, useFleet, useFleetStore } from "../state/fleetStore";
import {
  useConversations,
  useSessionLocation,
  type ChatMessage,
  type SessionId,
} from "./conversations";
import { PageHeader } from "./PageHeader";
import { SpiritMark } from "./SpiritMark";
import { openInbox, projectSummaries } from "./summary";

export interface ChatViewProps {
  session: SessionId;
  /** Label of the connected server, used in the location chip. */
  serverLabel: string | null;
  onOpenWorkers?: (project: string) => void;
}

function sessionTitle(session: SessionId): string {
  if (session === "setup") return "Setup assistant";
  if (session === "global") return "Global orchestrator";
  return session.slice("project:".length);
}

export function ChatView({ session, serverLabel, onOpenWorkers }: ChatViewProps) {
  const messages = useConversations((state) => state.messages[session]) ?? [];
  const send = useConversations((state) => state.send);
  const setLocation = useConversations((state) => state.setLocation);
  const location = useSessionLocation(session);
  const engine = useEngine();
  const endRef = useRef<HTMLDivElement>(null);
  const project = session.startsWith("project:") ? session.slice("project:".length) : null;

  useEffect(() => {
    endRef.current?.scrollIntoView?.({ block: "end" });
  }, [messages.length]);

  const model = engine?.selected_model?.label ?? engine?.selected_model?.id ?? "StackSpirit Engine";
  const fixedLocal = session === "setup";

  return (
    <section className="nx-main" aria-label={sessionTitle(session)}>
      <PageHeader
        title={sessionTitle(session)}
        actions={
          <>
            {fixedLocal ? null : (
              <button
                type="button"
                className="nx-btn nx-btn--quiet nx-btn--sm"
                onClick={() => setLocation(session, location === "local" ? "server" : "local")}
              >
                ⇄ {location === "local" ? "Run on server" : "Run on this Mac"}
              </button>
            )}
            {project && onOpenWorkers ? (
              <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={() => onOpenWorkers(project)}>
                Workers
              </button>
            ) : null}
          </>
        }
      >
        <span
          className={`nx-chip ${location === "local" ? "nx-chip--local" : "nx-chip--server"}`}
          data-testid="session-location"
          title={
            location === "local"
              ? "Runs on this Mac with local context. The session log stays on the server."
              : "Runs on the server. Works while this Mac is closed."
          }
        >
          <span aria-hidden="true">{location === "local" ? "▣" : "◉"}</span>
          {location === "local" ? "Local · this Mac" : `Server · ${serverLabel ?? "server"}`}
        </span>
      </PageHeader>

      <div className="nx-chat">
        {messages.length === 0 ? (
          <EmptySession session={session} onPick={(text) => send(session, text)} />
        ) : (
          <div className="nx-chat__inner" aria-live="polite">
            {messages.map((message) => (
              <Message key={message.id} session={session} message={message} />
            ))}
            <div ref={endRef} />
          </div>
        )}
      </div>

      <Composer
        placeholder={
          session === "setup"
            ? "Tell me about your server, or ask anything"
            : `Message the ${session === "global" ? "global" : "project"} orchestrator`
        }
        model={model}
        onSend={(text) => send(session, text)}
      />
    </section>
  );
}

function EmptySession({ session, onPick }: { session: SessionId; onPick: (text: string) => void }) {
  const fleet = useFleet();
  let title: string;
  let body: string;
  let suggestions: string[];

  if (session === "setup") {
    title = "Let's connect your always-on server";
    body =
      "A VPS from a provider, or a computer at home you can reach over SSH. I'll ask for what I need; anything secret goes into a secure box I can't read.";
    suggestions = ["I have a new VPS", "I want to use a computer at home", "What is an always-on server?"];
  } else if (session === "global") {
    const projects = fleet?.projects.length ?? 0;
    const running = fleet?.running.length ?? 0;
    const inbox = openInbox(fleet).length;
    title = "Global orchestrator";
    body = `${running} worker${running === 1 ? "" : "s"} running across ${projects} project${
      projects === 1 ? "" : "s"
    }${inbox ? ` · ${inbox} waiting in your inbox` : ""}.`;
    suggestions = ["What needs my attention?", "Start a new project", "How busy is the server?"];
  } else {
    const name = session.slice("project:".length);
    const summary = projectSummaries(fleet).find((item) => item.name === name);
    title = name;
    body = summary
      ? `${summary.running} running · ${summary.ready} ready · ${summary.review} in review${
          summary.inbox ? ` · ${summary.inbox} in inbox` : ""
        }.`
      : "This project's orchestrator.";
    suggestions = ["What's running right now?", "Plan the next job", "Show what's blocked"];
  }

  return (
    <div className="nx-empty-chat">
      <SpiritMark size={64} />
      <h2>{title}</h2>
      <p>{body}</p>
      <div className="nx-suggest">
        {suggestions.map((text) => (
          <button key={text} type="button" onClick={() => onPick(text)}>
            {text}
          </button>
        ))}
      </div>
    </div>
  );
}

function Message({ session, message }: { session: SessionId; message: ChatMessage }) {
  if (message.role === "user") {
    return (
      <div className="nx-msg nx-msg--user">
        {message.text.map((paragraph, index) => (
          <p key={index}>{paragraph}</p>
        ))}
      </div>
    );
  }
  const received = message.card?.kind === "secure-received";
  const card = <MessageCard session={session} message={message} />;
  return (
    <div className="nx-msg">
      <div className="nx-msg__who">
        <SpiritMark size={20} />
        StackSpirit
      </div>
      {received ? card : null}
      {message.text.map((paragraph, index) => (
        <p key={index} className={received ? "nx-secure__outcome" : undefined}>
          {paragraph}
        </p>
      ))}
      {received ? null : card}
    </div>
  );
}

function MessageCard({ session, message }: { session: SessionId; message: ChatMessage }) {
  const card = message.card;
  if (!card) return null;
  if (card.kind === "secure-server") {
    return <SecureServerCard session={session} messageId={message.id} />;
  }
  return (
    <div className="nx-secure nx-secure--done" data-testid="secure-received">
      <div className="nx-secure__head">
        <span className="nx-secure__lock" aria-hidden="true">
          ✓
        </span>
        Key installed for {card.host}
      </div>
      <div className="nx-secure__verified">
        <span>Verified fingerprint</span>
        <code>{card.fingerprint}</code>
      </div>
      <div className="nx-secure__note">
        <span aria-hidden="true">🔒</span>
        <span>
          StackSpirit confirmed a fresh key-only login before this message. The assistant saw
          the server address and this fingerprint; it never saw the one-time password, and the
          password is not stored.
        </span>
      </div>
    </div>
  );
}

type SecureStage = "form" | "scanning" | "confirm" | "installing";

interface SecureFailure {
  error: SpiritError;
  /** Which step failed, so the retry action matches it. */
  action: "scan" | "install";
}

/**
 * App-owned fields for server details and the one-time-password install.
 *
 * Only the host name and the verified fingerprint enter the conversation. StackSpirit
 * scans first, shows every offered host key, and installs only after the owner confirms
 * a fingerprint. The password stays in React state until that confirmation, is cleared
 * before the install call, and is never rendered, logged, or put in an error.
 */
function SecureServerCard({ session, messageId }: { session: SessionId; messageId: string }) {
  const bridge = useFleetStore((state) => state.bridge);
  const connect = useFleetStore((state) => state.connect);
  const loadHosts = useFleetStore((state) => state.loadHosts);
  const selectHost = useFleetStore((state) => state.selectHost);
  const noteScan = useConversations((state) => state.noteSecureServerScan);
  const record = useConversations((state) => state.recordSecureServer);
  const [host, setHost] = useState("");
  const [user, setUser] = useState("root");
  const [port, setPort] = useState("22");
  const [password, setPassword] = useState("");
  const [disablePasswords, setDisablePasswords] = useState(true);
  const [stage, setStage] = useState<SecureStage>("form");
  const [scan, setScan] = useState<HostKeyScan | null>(null);
  const [selectedSha, setSelectedSha] = useState<string | null>(null);
  const [verified, setVerified] = useState(false);
  const [confirmedPin, setConfirmedPin] = useState<HostKeyPin | null>(null);
  const [failure, setFailure] = useState<SecureFailure | null>(null);
  const passwordRef = useRef<HTMLInputElement>(null);
  // Synchronous guard: a fast double click must not start two scans or two installs
  // before React has rendered the disabled state.
  const operation = useRef(false);
  const ids = {
    host: useId(),
    user: useId(),
    port: useId(),
    password: useId(),
    fingerprint: useId(),
    keyGroup: useId(),
  };

  const busy = stage === "scanning" || stage === "installing";
  const editing = stage === "form";
  const onboarding = bridge?.onboarding ?? null;
  const portNumber = Number(port);
  const portValid = Number.isInteger(portNumber) && portNumber >= 1 && portNumber <= 65535;
  const ready = host.trim() !== "" && user.trim() !== "" && password !== "" && portValid && onboarding !== null;
  const name = host.trim();
  const draft: HostDraft = {
    // There is no separate label field on this card; the host is the label, capped at
    // the core's 64-character limit so a long DNS name cannot fail validation.
    label: name.slice(0, 64),
    ssh: { user: user.trim(), host: name, port: portNumber },
    identity_file: null,
    vowp_home: null,
    default_project: null,
  };

  function resetScan() {
    setScan(null);
    setSelectedSha(null);
    setVerified(false);
  }

  async function scanHost() {
    if (!bridge || !ready || busy || operation.current) return;
    operation.current = true;
    const firstScan = scan === null;
    setFailure(null);
    resetScan();
    setStage("scanning");
    // Only the host name enters the transcript. The password stays in the form.
    if (firstScan) noteScan(session, messageId, name);
    try {
      const result = await bridge.hosts.scanFingerprint(draft);
      const preferred = result.keys.find((key) => key.preferred) ?? result.keys[0] ?? null;
      setScan(result);
      setSelectedSha(preferred?.sha256 ?? null);
      setStage("confirm");
    } catch (caught) {
      setFailure({ error: toSpiritError(caught), action: "scan" });
      setStage("form");
    } finally {
      operation.current = false;
    }
  }

  async function install() {
    if (!onboarding || !scan || !selectedSha || !verified || password === "" || busy || operation.current) {
      return;
    }
    const key = scan.keys.find((candidate) => candidate.sha256 === selectedSha);
    if (!key) return;
    operation.current = true;
    // Reuse the first confirmation time while the fingerprint is unchanged. The host
    // saves a pinned profile before each install and refuses a pin it considers
    // different, so a retry must not silently change `pinned_at` for the same key.
    const pin: HostKeyPin = {
      algorithm: key.algorithm,
      sha256: key.sha256,
      pinned_at:
        confirmedPin &&
        confirmedPin.algorithm === key.algorithm &&
        confirmedPin.sha256 === key.sha256
          ? confirmedPin.pinned_at
          : scan.scanned_at,
    };
    setConfirmedPin(pin);
    // Read the one-time secret, then clear it from React state before the first await.
    // From here on no render, snapshot, or error can contain it.
    const oneTimePassword = password;
    setPassword("");
    setFailure(null);
    setStage("installing");
    try {
      await onboarding.createKey();
      const outcome = await onboarding.installKey({
        draft,
        host_key: pin,
        password: oneTimePassword,
        disable_password_auth: disablePasswords,
      });
      if (outcome.installed !== true) {
        const notVerified: SpiritError = {
          code: "ssh_auth",
          message: "The key was written, but a fresh key-only login did not verify.",
          hint: "StackSpirit only counts a verified key-only login as success. Retry the install when the server is reachable.",
          retryable: true,
        };
        throw notVerified;
      }
      record(session, messageId, {
        host: name,
        // The fingerprint the owner confirmed is the host key's, not the app key's.
        fingerprint: pin.sha256,
        password_auth: outcome.password_auth,
      });
      // The install saved `outcome.profile` before it touched the server. Refresh the
      // store so that profile is present, select it, and connect without another
      // manual step. The store reports a connection failure in the error bar; the
      // profile stays saved so Connect can be retried.
      await loadHosts();
      selectHost(outcome.profile.id);
      await connect(outcome.profile.id);
    } catch (caught) {
      setFailure({ error: toSpiritError(caught), action: "install" });
      resetScan();
      setStage("form");
    } finally {
      operation.current = false;
    }
  }

  function onSubmit(event: FormEvent) {
    event.preventDefault();
    if (stage !== "form") return;
    void scanHost();
  }

  function retry() {
    if (!failure || busy) return;
    if (failure.action === "scan") {
      void scanHost();
      return;
    }
    setFailure(null);
    passwordRef.current?.focus();
  }

  const showDetails = stage === "scanning" || stage === "confirm";

  return (
    <form
      className="nx-secure"
      onSubmit={onSubmit}
      aria-label="Secure server details"
      aria-busy={busy}
      autoComplete="off"
    >
      <div className="nx-secure__head">
        <span className="nx-secure__lock" aria-hidden="true">
          🔒
        </span>
        Secure server details
      </div>
      <div className="nx-secure__grid">
        <label className="nx-field" htmlFor={ids.host}>
          Server address
          <input
            id={ids.host}
            value={host}
            disabled={!editing}
            onChange={(event) => {
              setConfirmedPin(null);
              setHost(event.target.value);
            }}
            placeholder="203.0.113.10 or vps.example.com"
            spellCheck={false}
            autoComplete="off"
          />
        </label>
        <label className="nx-field" htmlFor={ids.user}>
          User
          <input
            id={ids.user}
            value={user}
            disabled={!editing}
            onChange={(event) => {
              setConfirmedPin(null);
              setUser(event.target.value);
            }}
            spellCheck={false}
            autoComplete="off"
          />
        </label>
        <label className="nx-field" htmlFor={ids.port}>
          Port
          <input
            id={ids.port}
            value={port}
            inputMode="numeric"
            disabled={!editing}
            onChange={(event) => {
              setConfirmedPin(null);
              setPort(event.target.value.replace(/\D/g, ""));
            }}
            autoComplete="off"
          />
        </label>
      </div>
      {editing ? (
        <label className="nx-field" htmlFor={ids.password}>
          One-time password from your provider
          <input
            ref={passwordRef}
            id={ids.password}
            type="password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            autoComplete="off"
          />
        </label>
      ) : null}
      {onboarding === null ? (
        <div className="nx-secure__note" role="status">
          <span aria-hidden="true">ⓘ</span>
          <span>One-time-password setup needs the StackSpirit desktop host.</span>
        </div>
      ) : null}
      <label className="nx-secure__verify">
        <input
          type="checkbox"
          checked={disablePasswords}
          disabled={busy}
          onChange={(event) => setDisablePasswords(event.target.checked)}
        />
        Turn off password login on the server once the key works (recommended)
      </label>

      {showDetails ? (
        <div className="nx-secure__confirm" role="group" aria-label="Confirm the server fingerprint">
          <div className="nx-secure__confirm-head">
            <span className="nx-secure__confirm-title">Server details received for {name}</span>
            <span className="nx-secure__confirm-sub">
              {stage === "scanning"
                ? `Reading the host keys from ${name}… Nothing is installed yet.`
                : "Nothing has been installed yet. Compare one of these fingerprints with your provider's console or the server's own key, then confirm it."}
            </span>
          </div>
          {stage === "scanning" ? (
            <div className="nx-secure__progress" role="status">
              Scanning host keys…
            </div>
          ) : null}
          {scan && stage === "confirm" ? (
            <>
              <ul className="nx-secure__keys" aria-label="Offered host keys">
                {scan.keys.map((key) => {
                  const keyId = `${ids.keyGroup}-${key.sha256}`;
                  return (
                    <li key={key.sha256}>
                      <label
                        className="nx-secure__key"
                        htmlFor={keyId}
                        data-selected={selectedSha === key.sha256}
                      >
                        <input
                          id={keyId}
                          type="radio"
                          name={ids.keyGroup}
                          checked={selectedSha === key.sha256}
                          onChange={() => setSelectedSha(key.sha256)}
                        />
                        <span className="nx-secure__key-body">
                          <span className="nx-secure__key-meta">
                            <span>{key.algorithm}</span>
                            {key.preferred ? (
                              <span className="nx-chip nx-chip--tiny nx-chip--server">preferred</span>
                            ) : null}
                          </span>
                          <code className="nx-secure__key-fingerprint">{key.sha256}</code>
                        </span>
                      </label>
                    </li>
                  );
                })}
              </ul>
              <label className="nx-secure__verify" htmlFor={ids.fingerprint}>
                <input
                  id={ids.fingerprint}
                  type="checkbox"
                  checked={verified}
                  onChange={(event) => setVerified(event.target.checked)}
                />
                <span>
                  I verified this fingerprint
                  <small>
                    Only this key is trusted. If the server's key changes later, StackSpirit
                    refuses to connect.
                  </small>
                </span>
              </label>
            </>
          ) : null}
        </div>
      ) : null}

      {stage === "installing" ? (
        <div className="nx-secure__progress" role="status">
          Installing the StackSpirit key and checking a key-only login…
        </div>
      ) : null}

      {failure ? (
        <div className="nx-secure__error" role="alert" data-testid="secure-error">
          <strong>
            {failure.error.code === "preview_unavailable"
              ? "Preview mode cannot install a key"
              : errorTitle(failure.error)}
          </strong>
          <span>{failure.error.message}</span>
          {failure.error.hint ? <span>{failure.error.hint}</span> : null}
          {failure.action === "install" ? (
            <span>
              The one-time password was cleared and is not stored. Enter it again to retry.
            </span>
          ) : null}
          <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={retry}>
            {failure.action === "scan" ? "Retry scan" : "Enter the password again"}
          </button>
        </div>
      ) : null}

      <div className="nx-secure__note">
        <span aria-hidden="true">🔒</span>
        <span>
          The assistant never sees this password. StackSpirit reads the server's host key,
          shows you the fingerprint to confirm, then logs in once to install an app-owned
          key and forgets the password. Only a verified key-only login counts as success.
        </span>
      </div>
      <div className="nx-secure__actions">
        {stage === "confirm" ? (
          <>
            <button type="button" className="nx-btn nx-btn--quiet" onClick={() => void scanHost()} disabled={busy}>
              Scan again
            </button>
            <button
              type="button"
              className="nx-btn"
              onClick={() => void install()}
              disabled={!verified || !selectedSha || password === "" || busy}
            >
              Install key
            </button>
          </>
        ) : (
          <button type="submit" className="nx-btn" disabled={!ready || busy}>
            {stage === "scanning" ? "Scanning…" : stage === "installing" ? "Installing…" : "Connect securely"}
          </button>
        )}
      </div>
    </form>
  );
}

function Composer({
  placeholder,
  model,
  onSend,
}: {
  placeholder: string;
  model: string;
  onSend: (text: string) => void;
}) {
  const [text, setText] = useState("");
  const id = useId();

  function submit() {
    if (!text.trim()) return;
    onSend(text);
    setText("");
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      submit();
    }
  }

  return (
    <form
      className="nx-composer"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <label htmlFor={id} className="nx-sr">
        Message
      </label>
      <textarea
        id={id}
        rows={1}
        value={text}
        placeholder={placeholder}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={onKeyDown}
      />
      <div className="nx-composer__bar">
        <span className="nx-chip" title="Runs on the bundled StackSpirit Engine">
          ◇ {model}
        </span>
        <span className="nx-composer__spacer" />
        <button type="submit" className="nx-send" aria-label="Send" disabled={!text.trim()}>
          ↑
        </button>
      </div>
    </form>
  );
}
