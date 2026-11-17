import { create } from "zustand";

import { useShellUi, type PanelContent } from "./shellUi";

/**
 * Design-preview conversation store for the next shell.
 *
 * Orchestrator chat has no bridge contract yet (VOWP chat is still landing), so this
 * module keeps the shapes the shell needs in one place. When the real transport
 * exists, `SessionId`, `SessionLocation` and `ChatMessage` move into the bridge wire
 * types and `send` calls the bridge; the components only read this store.
 *
 * Nothing here talks to a model. Replies are canned and labelled as preview.
 */

/** Where an orchestrator session runs. The session log always lives on the server. */
export type SessionLocation = "local" | "server";

/** "setup" is the local first-run assistant; "global" and projects are orchestrators. */
export type SessionId = "setup" | "global" | `project:${string}`;

/** A card the app renders inside the conversation instead of plain text. */
export type ChatCard =
  /** Asks for server details in app-owned fields. The values never enter the chat. */
  | { kind: "secure-server" }
  /** Confirms what the assistant was told: only that a value was provided. */
  | { kind: "secure-received"; host: string };

export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  /** Paragraphs. Kept as plain strings so no markup from a model reaches the DOM. */
  text: string[];
  card?: ChatCard;
  /** Something the agent wants to show; the shell opens it in the details panel. */
  show?: PanelContent;
}

/** What the add-project dialog collects. Nothing secret: access uses the server's own Git login. */
export interface ProjectRequest {
  name: string;
  source: { kind: "git"; url: string; branch: string } | { kind: "folder"; path: string };
}

interface ConversationState {
  messages: Partial<Record<SessionId, ChatMessage[]>>;
  location: Partial<Record<SessionId, SessionLocation>>;
  send: (session: SessionId, text: string) => void;
  /** Hands a new project to the global orchestrator (preview until VOWP exposes project creation). */
  requestProject: (request: ProjectRequest) => void;
  submitSecureServer: (session: SessionId, messageId: string, host: string) => void;
  setLocation: (session: SessionId, location: SessionLocation) => void;
}

let counter = 0;
const nextId = () => `m${++counter}`;

/** Sessions start on the server; the setup assistant always runs locally. */
export function defaultLocation(session: SessionId): SessionLocation {
  return session === "setup" ? "local" : "server";
}

function setupReply(): ChatMessage {
  return {
    id: nextId(),
    role: "assistant",
    text: [
      "Let's connect it. I need the server's address and a one-time login from your provider.",
      "Type them in the secure box below. I never see what you type there: StackSpirit uses the password once to install a fresh SSH key on the server, then forgets it.",
    ],
    card: { kind: "secure-server" },
  };
}

/**
 * Canned preview replies. A few questions point at something to show, so the
 * preview demonstrates the details panel opening on an agent's behalf.
 */
function previewReply(session: SessionId, text: string): ChatMessage {
  const where = session === "global" ? "the global orchestrator" : "this project's orchestrator";
  const preview = `Preview reply. In the real app ${where} answers here.`;
  if (/running|working|workers|progress/i.test(text)) {
    return { id: nextId(), role: "assistant", text: ["Here's what is running right now. I opened it on the right.", preview], show: { kind: "overview" } };
  }
  if (/busy|server|capacity|memory|ram|disk|cpu/i.test(text)) {
    return { id: nextId(), role: "assistant", text: ["Here's the server right now. I opened it on the right.", preview], show: { kind: "server" } };
  }
  if (/attention|inbox|question|blocked|waiting/i.test(text)) {
    return { id: nextId(), role: "assistant", text: ["These are waiting for you. I opened your inbox on the right.", preview], show: { kind: "inbox" } };
  }
  return { id: nextId(), role: "assistant", text: [preview] };
}

export const useConversations = create<ConversationState>((set, get) => ({
  messages: {},
  location: {},

  send: (session, text) => {
    const trimmed = text.trim();
    if (!trimmed) return;
    const current = get().messages[session] ?? [];
    const user: ChatMessage = { id: nextId(), role: "user", text: [trimmed] };
    const askedForCard = current.some((message) => message.card?.kind === "secure-server");
    const reply = session === "setup" && !askedForCard ? setupReply() : previewReply(session, trimmed);
    set((state) => ({
      messages: { ...state.messages, [session]: [...current, user, reply] },
    }));
    if (reply.show) useShellUi.getState().openPanel(reply.show);
  },

  requestProject: (request) => {
    const from =
      request.source.kind === "git"
        ? `${request.source.url} (branch ${request.source.branch || "main"})`
        : `the folder ${request.source.path} on the server`;
    const current = get().messages.global ?? [];
    const user: ChatMessage = { id: nextId(), role: "user", text: [`Create the project "${request.name}" from ${from}.`] };
    const reply: ChatMessage = {
      id: nextId(),
      role: "assistant",
      text: [
        `I'll set up "${request.name}": clone or link the code on the server, register it with VOWP, and open its orchestrator.`,
        "Preview: project creation runs here once VOWP's project API is connected.",
      ],
    };
    set((state) => ({ messages: { ...state.messages, global: [...current, user, reply] } }));
  },

  submitSecureServer: (session, messageId, host) => {
    const current = get().messages[session] ?? [];
    const updated = current.map((message) =>
      message.id === messageId ? { ...message, card: { kind: "secure-received" as const, host } } : message,
    );
    const reply: ChatMessage = {
      id: nextId(),
      role: "assistant",
      text: [
        `Thanks. I was told only that a password was provided for ${host}, not the password itself.`,
        "Preview: the real app now installs a new key, shows you the server's fingerprint to confirm, and saves the server. For this demo, use Add server in the sidebar.",
      ],
    };
    set((state) => ({ messages: { ...state.messages, [session]: [...updated, reply] } }));
  },

  setLocation: (session, location) =>
    set((state) => ({ location: { ...state.location, [session]: location } })),
}));

export function useSessionLocation(session: SessionId): SessionLocation {
  return useConversations((state) => state.location[session] ?? defaultLocation(session));
}
