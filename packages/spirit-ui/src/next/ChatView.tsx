import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent } from "react";

import { useEngine, useFleet } from "../state/fleetStore";
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
  return (
    <div className="nx-msg">
      <div className="nx-msg__who">
        <SpiritMark size={20} />
        StackSpirit
      </div>
      {message.text.map((paragraph, index) => (
        <p key={index}>{paragraph}</p>
      ))}
      {message.card?.kind === "secure-server" ? (
        <SecureServerCard session={session} messageId={message.id} />
      ) : null}
      {message.card?.kind === "secure-received" ? (
        <div className="nx-secure nx-secure--done">
          <div className="nx-secure__head">
            <span className="nx-secure__lock" aria-hidden="true">
              ✓
            </span>
            Server details received for {message.card.host}
          </div>
          <div className="nx-secure__note">
            <span aria-hidden="true">🔒</span>
            The assistant saw: address provided · password provided. The password itself was used once
            and is not stored.
          </div>
        </div>
      ) : null}
    </div>
  );
}

/**
 * App-owned fields for server details. The values stay in this component: only the
 * host name reaches the conversation, and the password is cleared on submit. In the
 * real app the submit goes to the bridge, which installs a key over SSH.
 */
function SecureServerCard({ session, messageId }: { session: SessionId; messageId: string }) {
  const submit = useConversations((state) => state.submitSecureServer);
  const [host, setHost] = useState("");
  const [user, setUser] = useState("root");
  const [port, setPort] = useState("22");
  const [password, setPassword] = useState("");
  const [disablePasswords, setDisablePasswords] = useState(true);
  const ids = { host: useId(), user: useId(), port: useId(), password: useId() };
  const ready = host.trim() !== "" && user.trim() !== "" && password !== "";

  function onSubmit(event: FormEvent) {
    event.preventDefault();
    if (!ready) return;
    const name = host.trim();
    setPassword("");
    submit(session, messageId, name);
  }

  return (
    <form className="nx-secure" onSubmit={onSubmit} aria-label="Secure server details" autoComplete="off">
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
            onChange={(event) => setHost(event.target.value)}
            placeholder="203.0.113.10 or vps.example.com"
            spellCheck={false}
          />
        </label>
        <label className="nx-field" htmlFor={ids.user}>
          User
          <input id={ids.user} value={user} onChange={(event) => setUser(event.target.value)} spellCheck={false} />
        </label>
        <label className="nx-field" htmlFor={ids.port}>
          Port
          <input
            id={ids.port}
            value={port}
            inputMode="numeric"
            onChange={(event) => setPort(event.target.value.replace(/\D/g, ""))}
          />
        </label>
      </div>
      <label className="nx-field" htmlFor={ids.password}>
        One-time password from your provider
        <input
          id={ids.password}
          type="password"
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          autoComplete="off"
        />
      </label>
      <label style={{ display: "flex", gap: 8, alignItems: "center", fontSize: 13 }}>
        <input
          type="checkbox"
          checked={disablePasswords}
          onChange={(event) => setDisablePasswords(event.target.checked)}
        />
        Turn off password login on the server once the key works (recommended)
      </label>
      <div className="nx-secure__note">
        <span aria-hidden="true">🔒</span>
        <span>
          The assistant never sees what you type here. StackSpirit logs in once, installs a fresh SSH key,
          asks you to confirm the server's fingerprint, and forgets the password.
        </span>
      </div>
      <div className="nx-secure__actions">
        <button type="submit" className="nx-btn" disabled={!ready}>
          Connect securely
        </button>
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
