import { create } from "zustand";

import { toSpiritError } from "../bridge/errors";
import type { SpiritBridge } from "../bridge/SpiritBridge";
import type {
  ChatCloseReason,
  ChatEvent,
  ChatLogPage,
  ChatMessage,
  ChatMessageOrigin,
  ChatPanelTarget,
  ChatPermissionDecision,
  ChatSessionView,
  ProjectRequest,
  SessionId,
  SessionLocation,
} from "../bridge/wire";
import type { PasswordAuthOutcome, SpiritErrorDto, Timestamp } from "../bridge/wire";
import { useFleetStore } from "../state/fleetStore";
import { passwordAuthSummary } from "./passwordAuth";
import { useShellUi } from "./shellUi";

/**
 * Renderer-side orchestrator chat cache and router (`docs/ORCHESTRATOR-CHAT.md` §4.6).
 *
 * `bridge.chat` is the only source of conversation state. This module never invents an
 * assistant reply: the real path renders model text from `ChatEvent`s or a typed error
 * and gate. A preview bridge may still supply labelled `origin: "preview"` messages.
 *
 * The app-owned secure-server card is a `ChatMessageView` overlay with
 * `origin: "app"`; it is not model output and never carries a secret.
 */

export type {
  ChatMessage,
  ChatPanelTarget,
  ProjectRequest,
  SessionId,
  SessionLocation,
};

/** A card the app renders inside the conversation instead of plain text. */
export type ChatCard =
  /**
   * Asks for server details in app-owned fields. The password never enters the chat;
   * only the host name is recorded when a scan starts.
   */
  | { kind: "secure-server" }
  /**
   * Confirms a verified app-key install. Deliberately only the host and the
   * fingerprint: never the user, the port, or anything typed into the card.
   */
  | { kind: "secure-received"; host: string; fingerprint: string };

/**
 * The renderer's message: transport text plus app-owned cards.
 *
 * `created_at` and `origin` are optional only so an app-owned card can be seeded
 * without transport metadata. Every message that comes from `bridge.chat` has both.
 */
export interface ChatMessageView extends Omit<ChatMessage, "created_at" | "origin"> {
  /** Canonical RFC 3339 UTC string with millis, when the transport supplied one. */
  created_at?: Timestamp;
  /** Who produced the message; app-owned cards default to `app` when rendered. */
  origin?: ChatMessageOrigin;
  card?: ChatCard;
  /**
   * True only on a user bubble rendered from a `chat.send` receipt that no native
   * event or log message has confirmed yet. Renderer-only: never sent over the bridge.
   */
  optimistic?: boolean;
}

/** The verified facts a successful onboarding may put in the conversation. */
export interface SecureServerReceipt {
  host: string;
  /** The confirmed host-key fingerprint (`SHA256:`), not the app key's. */
  fingerprint: string;
  /** The optional password-login step; rendered in plain language, never secret. */
  password_auth: PasswordAuthOutcome;
}

type OpeningPromise = Promise<ChatSessionView | null>;

interface ConversationState {
  messages: Partial<Record<SessionId, ChatMessageView[]>>;
  views: Partial<Record<SessionId, ChatSessionView>>;
  /** True when the newest log page may have dropped messages. */
  truncated: Partial<Record<SessionId, boolean>>;
  /** Opaque cursor for older messages; `null`/absent means no older page. */
  earlier: Partial<Record<SessionId, string | null>>;
  loadingEarlier: Partial<Record<SessionId, boolean>>;
  /** Permission ids whose reply is in flight, so a card cannot be answered twice. */
  permissionBusy: Partial<Record<SessionId, string | null>>;
  /** The last failed permission reply per session. It never changes the turn state. */
  permissionError: Partial<Record<SessionId, SpiritErrorDto | null>>;
  /** Per-session operation guard; prevents StrictMode double opens. */
  opening: Partial<Record<SessionId, OpeningPromise | undefined>>;
  /** The last applied event revision, for gap detection. */
  lastRevision: number;
  /**
   * @deprecated The old label-only location map. The header reads
   * `ChatSessionView.location`; this field is never read or written by the shell.
   * It is kept only so the out-of-owns onboarding-card fixture still typechecks.
   */
  location?: Partial<Record<SessionId, SessionLocation>>;

  /** Ensure the chat session is attached and load its newest log page. */
  open: (session: SessionId, project: string | null, location?: SessionLocation) => Promise<void>;
  /** Start a fresh local setup conversation (`resume: false`); never used for global/project. */
  restart: (session: SessionId) => Promise<void>;
  /** Accept one user turn through `bridge.chat`; assistant text arrives as events. */
  send: (session: SessionId, text: string) => Promise<void>;
  /** Abort the current turn through `bridge.chat`; never stops an engine or host. */
  stop: (session: SessionId) => Promise<void>;
  /** Switch the setup assistant's model inline; an orchestrator-owned session rejects. */
  setModel: (session: SessionId, modelId: string) => Promise<void>;
  /**
   * Answers one pending permission card. The returned view replaces the session view, so
   * the card disappears immediately; a failure keeps the card and records the error.
   */
  replyPermission: (
    session: SessionId,
    permissionId: string,
    decision: ChatPermissionDecision,
  ) => Promise<void>;
  /** Page older messages into the transcript using the opaque cursor. */
  loadEarlier: (session: SessionId) => Promise<void>;
  /** Preview-only project creation demo; a real bridge shows the VOWP gate. */
  requestProject: (request: ProjectRequest) => void;
  /**
   * Shows the app-owned new-server secure card in the setup chat because the owner
   * explicitly asked to add a server. A saved profile never seeds it on its own.
   */
  requestSecureServerCard: () => void;
  /** Notes a host-key scan on the card message. Only the host name, which is not a secret. */
  noteSecureServerScan: (session: SessionId, messageId: string, host: string) => void;
  /** Records a verified key-only install; the secure card turns into a receipt. */
  recordSecureServer: (session: SessionId, messageId: string, receipt: SecureServerReceipt) => void;
}

const MAX_MESSAGE_CHARS = 32_000;

let activeBridge: SpiritBridge | null = null;
let activeUnsubscribe: (() => void) | null = null;
let counter = 0;
/** Preview message ids a real bridge tried to render; their deltas are ignored too. */
const hiddenPreviewIds = new Set<string>();
const backfilling = new Set<SessionId>();

function createInitialState(): Pick<
  ConversationState,
  | "messages"
  | "views"
  | "truncated"
  | "earlier"
  | "loadingEarlier"
  | "permissionBusy"
  | "permissionError"
  | "opening"
  | "lastRevision"
> {
  return {
    messages: {},
    views: {},
    truncated: {},
    earlier: {},
    loadingEarlier: {},
    permissionBusy: {},
    permissionError: {},
    opening: {},
    lastRevision: 0,
  };
}

function nowIso(): Timestamp {
  return new Date().toISOString();
}

function nextLocalId(prefix: "app" | "preview" | "user"): string {
  counter += 1;
  return `${prefix}:ui-${counter}`;
}

/** Strips C0 control characters except tab and newline; keeps text plain. */
function cleanText(value: string): string {
  // eslint-disable-next-line no-control-regex -- control characters are exactly what this strips
  return value.replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, "");
}

function normalizeText(text: string[]): string[] {
  const out: string[] = [];
  let remaining = MAX_MESSAGE_CHARS;
  for (const paragraph of text) {
    if (remaining <= 0) break;
    const clean = cleanText(typeof paragraph === "string" ? paragraph : "").slice(0, remaining);
    out.push(clean);
    remaining -= clean.length;
  }
  return out.length > 0 ? out : [""];
}

function normalizeMessage(message: ChatMessage): ChatMessageView {
  return { ...message, text: normalizeText(message.text) };
}

function baseView(session: SessionId): ChatSessionView {
  return {
    session,
    location: "local",
    state: "idle",
    attached: false,
    streaming_supported: false,
    model: null,
    started_at: null,
    last_event_at: null,
    error: null,
  };
}

function chatUnavailableError(): SpiritErrorDto {
  return {
    code: "gate",
    message: "chat_unavailable: Chat is not available in this client.",
    hint: "Chat needs the StackSpirit desktop app; this client stays read-only.",
    retryable: false,
  };
}

function closedError(reason: ChatCloseReason): SpiritErrorDto {
  switch (reason) {
    case "engine_stopped":
      return {
        code: "engine",
        message: "engine_not_ready: The StackSpirit Engine stopped.",
        hint: "Restart it in Settings → Engine, then reconnect.",
        retryable: true,
      };
    case "host_disconnected":
      return {
        code: "remote",
        message: "host_disconnected: The server is not connected.",
        hint: "Reconnect the server, then retry.",
        retryable: true,
      };
    case "session_lost":
      return {
        code: "not_found",
        message: "session_lost: The orchestrator's app session was recreated.",
        hint: "Start a new conversation here; the server log stays in Sessions.",
        retryable: false,
      };
    case "failed":
      return {
        code: "engine",
        message: "chat_failed: The turn failed.",
        hint: "Retry the turn.",
        retryable: true,
      };
    case "stopped":
    default:
      return {
        code: "conflict",
        message: "stop_unconfirmed: The turn may still be running on the orchestrator.",
        hint: "Check the orchestrator before starting another turn.",
        retryable: false,
      };
  }
}

function currentBridge(): SpiritBridge | null {
  return activeBridge ?? useFleetStore.getState().bridge;
}

function projectFor(session: SessionId): string | null {
  return session.startsWith("project:") ? session.slice("project:".length) : null;
}

function setView(session: SessionId, view: ChatSessionView): void {
  useConversations.setState((state) => ({
    views: { ...state.views, [session]: view },
  }));
}

function patchView(session: SessionId, patch: Partial<ChatSessionView>): void {
  useConversations.setState((state) => {
    const current = state.views[session] ?? baseView(session);
    return {
      views: { ...state.views, [session]: { ...current, ...patch } },
    };
  });
}

function sameTurn(left: ChatMessageView, right: ChatMessageView): boolean {
  if (left.role !== right.role || left.origin !== right.origin) return false;
  if (left.text.join("\n\n") !== right.text.join("\n\n")) return false;
  if (!left.created_at || !right.created_at) return false;
  const leftAt = Date.parse(left.created_at);
  const rightAt = Date.parse(right.created_at);
  return (
    Number.isFinite(leftAt) &&
    Number.isFinite(rightAt) &&
    Math.abs(leftAt - rightAt) <= 5_000
  );
}

/** Drops the renderer-only optimistic marker after native truth arrives. */
function withoutOptimistic(message: ChatMessageView): ChatMessageView {
  const next = { ...message };
  delete next.optimistic;
  return next;
}

/**
 * Upserts by id and by the renderer-only `optimistic` marker. An exact id match always
 * merges, with the incoming fields winning and the marker cleared. A `send` receipt
 * with a different id merges into a same-turn native message and keeps the native id;
 * a native event or log message with a different id replaces the same-turn optimistic
 * bubble instead of appending a second one. Two native or two optimistic messages are
 * never merged, so repeated identical user turns stay separate.
 */
export function upsertMessage(
  list: ChatMessageView[],
  message: ChatMessageView,
): ChatMessageView[] {
  const exact = list.findIndex((candidate) => candidate.id === message.id);
  if (exact >= 0) {
    const existing = list[exact];
    if (!existing) return list;
    const next = list.slice();
    next[exact] = withoutOptimistic({ ...existing, ...message });
    return next;
  }
  if (message.optimistic === true) {
    const native = list.findIndex(
      (candidate) => candidate.optimistic !== true && sameTurn(candidate, message),
    );
    if (native >= 0) {
      const existing = list[native];
      if (!existing) return [...list, { ...message, optimistic: true }];
      const next = list.slice();
      next[native] = withoutOptimistic({ ...existing, ...message, id: existing.id });
      return next;
    }
    return [...list, { ...message, optimistic: true }];
  }
  const optimistic = list.findIndex(
    (candidate) => candidate.optimistic === true && sameTurn(candidate, message),
  );
  if (optimistic >= 0) {
    const existing = list[optimistic];
    if (!existing) return [...list, message];
    const next = list.slice();
    next[optimistic] = withoutOptimistic({ ...existing, ...message });
    return next;
  }
  return [...list, message];
}

function mergePage(
  session: SessionId,
  page: ChatLogPage,
  mode: "merge" | "prepend",
  previewAllowed: boolean,
): void {
  const incoming = page.messages
    .map(normalizeMessage)
    .filter((message) => previewAllowed || message.origin !== "preview");
  useConversations.setState((state) => {
    const current = state.messages[session] ?? [];
    let next = current;
    if (mode === "prepend") {
      const ids = new Set(current.map((message) => message.id));
      next = [...incoming.filter((message) => !ids.has(message.id)), ...current];
    } else {
      for (const message of incoming) next = upsertMessage(next, message);
    }
    return {
      messages: { ...state.messages, [session]: next },
      ...(mode === "merge"
        ? { truncated: { ...state.truncated, [session]: page.truncated } }
        : {}),
      earlier: { ...state.earlier, [session]: page.next },
    };
  });
}

function appendDelta(text: string[], delta: string): string[] {
  const clean = cleanText(delta);
  if (!clean) return text;
  const current = text.length > 0 ? (text[text.length - 1] ?? "") : "";
  const merged = current + clean;
  const next = [...text.slice(0, -1), ...merged.split(/\n{2,}/)];
  const capped: string[] = [];
  let remaining = MAX_MESSAGE_CHARS;
  for (const paragraph of next) {
    if (remaining <= 0) break;
    const slice = paragraph.slice(0, remaining);
    capped.push(slice);
    remaining -= slice.length;
  }
  return capped.length > 0 ? capped : [""];
}

async function backfill(bridge: SpiritBridge, session: SessionId): Promise<void> {
  const chat = bridge.chat;
  if (!chat || backfilling.has(session)) return;
  backfilling.add(session);
  try {
    const page = await chat.log(session);
    mergePage(session, page, "merge", bridge.preview);
  } catch {
    // Keep the live stream; the next attach/retry reads the log again.
  } finally {
    backfilling.delete(session);
  }
}

function applyChatEvent(bridge: SpiritBridge, event: ChatEvent): void {
  const state = useConversations.getState();
  const session = event.kind === "session" ? event.session.session : event.session;
  const gap = state.lastRevision !== 0 && event.revision > state.lastRevision + 1;
  if (event.revision > state.lastRevision) {
    useConversations.setState({ lastRevision: event.revision });
  }

  switch (event.kind) {
    case "session":
      setView(session, { ...event.session });
      break;
    case "message": {
      if (!bridge.preview && event.message.origin === "preview") {
        // A real transport must never deliver preview text. Drop the message and any
        // deltas for it rather than render a canned reply with preview === false.
        hiddenPreviewIds.add(event.message.id);
        break;
      }
      const message = normalizeMessage(event.message);
      useConversations.setState((current) => ({
        messages: {
          ...current.messages,
          [session]: upsertMessage(current.messages[session] ?? [], message),
        },
      }));
      if (message.show) useShellUi.getState().openPanel(message.show);
      break;
    }
    case "delta": {
      // Ignore deltas for a preview message a real bridge tried to send. The id prefix
      // check also covers a bridge that skips the shell message event.
      if (!bridge.preview && event.message_id.startsWith("preview:")) break;
      if (hiddenPreviewIds.has(event.message_id)) break;
      useConversations.setState((current) => {
        const list = current.messages[session] ?? [];
        const index = list.findIndex((message) => message.id === event.message_id);
        if (index < 0) {
          const shell: ChatMessageView = {
            id: event.message_id,
            role: "assistant",
            text: appendDelta([], event.text),
            created_at: nowIso(),
            origin: bridge.preview ? "preview" : "model",
            streaming: true,
          };
          return { messages: { ...current.messages, [session]: [...list, shell] } };
        }
        const next = list.slice();
        const existing = list[index];
        if (!existing) return { messages: { ...current.messages, [session]: list } };
        next[index] = {
          ...existing,
          text: appendDelta(existing.text, event.text),
          streaming: true,
        };
        return { messages: { ...current.messages, [session]: next } };
      });
      break;
    }
    case "done": {
      useConversations.setState((current) => {
        const list = current.messages[session];
        const view = current.views[session];
        let next = list;
        if (list) {
          const index = list.findIndex((message) => message.id === event.message_id);
          if (index >= 0) {
            const existing = list[index];
            if (!existing) return {};
            const message = { ...existing, streaming: false };
            const empty = message.text.every((paragraph) => paragraph.trim() === "");
            next =
              empty && !message.card && !message.tools?.length
                ? list.filter((_, candidate) => candidate !== index)
                : list.map((candidate, candidateIndex) =>
                    candidateIndex === index ? message : candidate,
                  );
          }
        }
        return {
          messages: list ? { ...current.messages, [session]: next ?? list } : current.messages,
          views: view
            ? {
                ...current.views,
                [session]: {
                  ...view,
                  state: view.error ? "error" : "ready",
                },
              }
            : current.views,
        };
      });
      break;
    }
    case "error":
      patchView(session, { state: "error", error: event.error, attached: true });
      break;
    case "closed": {
      if (event.reason === "stopped") {
        patchView(session, { state: "stopped", attached: false, error: null });
      } else {
        const current = useConversations.getState().views[session];
        patchView(session, {
          state: "error",
          attached: false,
          error: current?.error ?? closedError(event.reason),
        });
      }
      break;
    }
  }

  if (gap) void backfill(bridge, session);
}

function hasSecureServerForm(messages: ChatMessageView[] | undefined): boolean {
  return (messages ?? []).some((message) => message.card?.kind === "secure-server");
}

function secureServerMessage(): ChatMessageView {
  return {
    id: "app:secure-server",
    role: "assistant",
    text: [
      "Let's connect your server. I need the server's address and a one-time login from your provider.",
      "Type them in the secure box below. I never see what you type there: StackSpirit shows you the server's fingerprint to confirm, installs a fresh SSH key, then forgets the password.",
    ],
    created_at: nowIso(),
    origin: "app",
    card: { kind: "secure-server" },
  };
}

/**
 * Drops a secure-server form that was seeded before a profile was saved. A saved
 * server never sees the new-server password card unless the owner asks for it.
 */
function removeSecureServerForm(): void {
  useConversations.setState((state) => {
    const current = state.messages.setup ?? [];
    const next = current.filter((message) => message.card?.kind !== "secure-server");
    if (next.length === current.length) return {};
    return { messages: { ...state.messages, setup: next } };
  });
}

/**
 * A saved profile never shows the new-server password card unasked. The card is
 * seeded only by `requestSecureServerCard` when the owner chooses Connect server,
 * so an empty setup chat opens as a calm hero instead of a password wall.
 */
function pruneSetupCardWhenSaved(): void {
  if (useFleetStore.getState().hosts.length > 0) removeSecureServerForm();
}

async function doOpen(
  bridge: SpiritBridge,
  session: SessionId,
  project: string | null,
  location: SessionLocation | undefined,
  resume: boolean | undefined,
): Promise<ChatSessionView | null> {
  const chat = bridge.chat;
  const desired: SessionLocation = session === "setup" ? "local" : (location ?? "local");
  const current = useConversations.getState().views[session];

  if (!chat) {
    setView(session, {
      ...baseView(session),
      location: desired,
      state: "error",
      attached: false,
      error: chatUnavailableError(),
      last_event_at: nowIso(),
    });
    if (session === "setup") pruneSetupCardWhenSaved();
    return null;
  }

  patchView(session, {
    location: current?.location ?? desired,
    state: "connecting",
    attached: current?.attached ?? false,
    error: null,
    last_event_at: nowIso(),
  });

  try {
    const view = await chat.start({ session, project, location: desired, resume });
    setView(session, { ...view });
    // A fresh setup conversation (`resume: false`) replaces the rendered transcript; the
    // old native log is left intact by the bridge and remains in the engine's private tree.
    if (resume === false && session === "setup") {
      useConversations.setState((state) => ({
        messages: { ...state.messages, [session]: [] },
        truncated: { ...state.truncated, [session]: false },
        earlier: { ...state.earlier, [session]: null },
      }));
    }
    try {
      const page = await chat.log(session);
      mergePage(session, page, "merge", bridge.preview);
    } catch (caught) {
      const error = toSpiritError(caught);
      const after = useConversations.getState().views[session] ?? view;
      setView(session, { ...after, state: "error", error, attached: false });
    }
    if (session === "setup") pruneSetupCardWhenSaved();
    return view;
  } catch (caught) {
    patchView(session, {
      state: "error",
      attached: false,
      error: toSpiritError(caught),
      last_event_at: nowIso(),
    });
    if (session === "setup") pruneSetupCardWhenSaved();
    return null;
  }
}

async function runOpen(
  session: SessionId,
  project: string | null,
  location: SessionLocation | undefined,
  resume: boolean | undefined,
): Promise<void> {
  const bridge = currentBridge();
  if (!bridge) {
    setView(session, {
      ...baseView(session),
      state: "error",
      error: chatUnavailableError(),
      last_event_at: nowIso(),
    });
    return;
  }

  const pending = useConversations.getState().opening[session];
  if (pending) {
    await pending;
    return;
  }

  const promise = doOpen(bridge, session, project, location, resume);
  useConversations.setState((state) => ({
    opening: { ...state.opening, [session]: promise },
  }));
  await promise;
  if (useConversations.getState().opening[session] === promise) {
    useConversations.setState((state) => ({
      opening: { ...state.opening, [session]: undefined },
    }));
  }
}

export const useConversations = create<ConversationState>((set, get) => ({
  ...createInitialState(),

  open: (session, project, location) => runOpen(session, project, location, undefined),

  restart: (session) =>
    runOpen(
      session,
      projectFor(session),
      "local",
      false,
    ),

  send: async (session, text) => {
    const trimmed = text.trim();
    if (!trimmed) return;
    const bridge = currentBridge();
    const chat = bridge?.chat;
    const view = get().views[session];
    if (!bridge || !chat) {
      if (view?.state !== "error") {
        patchView(session, {
          state: "error",
          attached: false,
          error: chatUnavailableError(),
          last_event_at: nowIso(),
        });
      }
      return;
    }
    if (!view || view.state !== "ready") return;

    try {
      const receipt = await chat.send({ session, text: trimmed });
      if (!bridge.preview && receipt.message.origin === "preview") return;
      const message: ChatMessageView = {
        ...normalizeMessage(receipt.message),
        optimistic: true,
      };
      useConversations.setState((state) => ({
        messages: {
          ...state.messages,
          [session]: upsertMessage(state.messages[session] ?? [], message),
        },
      }));
    } catch (caught) {
      const error = toSpiritError(caught);
      // A second send is rejected because the first turn is still streaming; keep the
      // streaming view (the composer is already disabled) instead of an error state.
      if (error.code === "conflict" && /chat_busy/.test(error.message)) return;
      patchView(session, {
        state: "error",
        attached: true,
        error,
        last_event_at: nowIso(),
      });
    }
  },

  stop: async (session) => {
    const bridge = currentBridge();
    const chat = bridge?.chat;
    if (!chat) return;
    try {
      const view = await chat.stop(session);
      setView(session, { ...view });
    } catch (caught) {
      patchView(session, {
        state: "error",
        error: toSpiritError(caught),
        last_event_at: nowIso(),
      });
    }
  },

  setModel: async (session, modelId) => {
    const bridge = currentBridge();
    const chat = bridge?.chat;
    if (!chat?.setModel) return;
    try {
      const view = await chat.setModel(session, modelId);
      setView(session, { ...view, error: null });
    } catch (caught) {
      patchView(session, {
        state: "error",
        error: toSpiritError(caught),
        last_event_at: nowIso(),
      });
    }
  },

  replyPermission: async (session, permissionId, decision) => {
    const bridge = currentBridge();
    const chat = bridge?.chat;
    if (!chat?.replyPermission || get().permissionBusy[session]) return;
    set((state) => ({
      permissionBusy: { ...state.permissionBusy, [session]: permissionId },
      permissionError: { ...state.permissionError, [session]: null },
    }));
    try {
      const view = await chat.replyPermission(session, permissionId, decision);
      setView(session, { ...view });
    } catch (caught) {
      set((state) => ({
        permissionError: { ...state.permissionError, [session]: toSpiritError(caught) },
      }));
    } finally {
      set((state) => ({
        permissionBusy: { ...state.permissionBusy, [session]: null },
      }));
    }
  },

  loadEarlier: async (session) => {
    const bridge = currentBridge();
    const chat = bridge?.chat;
    const cursor = get().earlier[session];
    if (!chat || !cursor || get().loadingEarlier[session]) return;
    set((state) => ({
      loadingEarlier: { ...state.loadingEarlier, [session]: true },
    }));
    try {
      const page = await chat.log(session, { cursor, limit: 50 });
      mergePage(session, page, "prepend", bridge.preview);
    } catch {
      // Keep the transcript; the user can retry the button.
    } finally {
      set((state) => ({
        loadingEarlier: { ...state.loadingEarlier, [session]: false },
      }));
    }
  },

  requestProject: (request) => {
    const bridge = currentBridge();
    const session: SessionId = "global";
    // Project creation remains preview until VOWP 0.5's one-step bootstrap. A real
    // bridge never gets a canned project reply.
    if (!bridge?.preview) {
      patchView(session, {
        state: "error",
        attached: false,
        error: {
          code: "gate",
          message: "chat_gated: Project creation needs VOWP 0.5's `vowp project add`.",
          hint: "Create the project in VOWP, then reopen this conversation.",
          retryable: false,
        },
        last_event_at: nowIso(),
      });
      return;
    }

    const from =
      request.source.kind === "git"
        ? `${request.source.url} (branch ${request.source.branch || "main"})`
        : `the folder ${request.source.path} on the server`;
    const user: ChatMessageView = {
      id: nextLocalId("user"),
      role: "user",
      text: [`Create the project "${request.name}" from ${from}.`],
      created_at: nowIso(),
      origin: "user",
    };
    const reply: ChatMessageView = {
      id: nextLocalId("preview"),
      role: "assistant",
      text: [
        `Preview reply. In the real app I'll set up "${request.name}": clone or link the code on the server, register it with VOWP, and open its orchestrator.`,
        "Preview: project creation runs here once VOWP's project API is connected.",
      ],
      created_at: nowIso(),
      origin: "preview",
    };
    set((state) => ({
      messages: {
        ...state.messages,
        [session]: [...(state.messages[session] ?? []), user, reply],
      },
    }));
  },

  requestSecureServerCard: () => {
    const session: SessionId = "setup";
    const card = secureServerMessage();
    set((state) => {
      if (hasSecureServerForm(state.messages[session])) return {};
      return {
        messages: {
          ...state.messages,
          [session]: [card, ...(state.messages[session] ?? [])],
        },
      };
    });
  },

  noteSecureServerScan: (session, messageId, host) => {
    const note = `Checking ${host} for its host key. Nothing is installed until you verify the fingerprint.`;
    set((state) => {
      const updated = (state.messages[session] ?? []).map((message) => {
        if (message.id !== messageId || message.card?.kind !== "secure-server") return message;
        return {
          ...message,
          origin: message.origin ?? "app",
          text: [...message.text.filter((paragraph) => !paragraph.startsWith("Checking ")), note],
        };
      });
      return { messages: { ...state.messages, [session]: updated } };
    });
  },

  recordSecureServer: (session, messageId, receipt) => {
    const current = get().messages[session] ?? [];
    // The form and its scan note are gone once the install succeeded; the receipt is
    // a new app-owned message.
    const withoutCard = current.map((message) =>
      message.id === messageId && message.card?.kind === "secure-server"
        ? {
            ...message,
            origin: message.origin ?? "app",
            card: undefined,
            text: message.text.filter((paragraph) => !paragraph.startsWith("Checking ")),
          }
        : message,
    );
    const reply: ChatMessageView = {
      id: nextLocalId("app"),
      role: "assistant",
      text: [passwordAuthSummary(receipt.password_auth)],
      created_at: nowIso(),
      origin: "app",
      card: {
        kind: "secure-received",
        host: receipt.host,
        fingerprint: receipt.fingerprint,
      },
    };
    set((state) => ({
      messages: { ...state.messages, [session]: [...withoutCard, reply] },
    }));
  },

  // `location` is a deprecated compatibility field; the shell never reads it. Kept in
  // the state type so the out-of-owns onboarding-card fixture still compiles.
  location: {},
}));

/**
 * Subscribes the store to `bridge.chat` once. `NextShell` calls this in the same effect
 * that initializes the fleet store and returns the cleanup.
 */
export function initializeConversations(bridge: SpiritBridge): () => void {
  activeBridge = bridge;
  activeUnsubscribe?.();
  activeUnsubscribe = null;
  const chat = bridge.chat;
  if (!chat) return () => {};

  const unsubscribe = chat.subscribe((event) => applyChatEvent(bridge, event));
  activeUnsubscribe = unsubscribe;
  return () => {
    if (activeUnsubscribe === unsubscribe) {
      unsubscribe();
      activeUnsubscribe = null;
    }
  };
}

/** Clears the cache, saves and subscription. Tests use it; production has one bridge. */
export function resetConversations(): void {
  activeUnsubscribe?.();
  activeUnsubscribe = null;
  activeBridge = null;
  hiddenPreviewIds.clear();
  backfilling.clear();
  useConversations.setState({ ...createInitialState(), location: {} });
}
