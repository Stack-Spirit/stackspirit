import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent, type ReactNode, type RefObject } from "react";

import { errorTitle, toSpiritError, type SpiritError } from "../bridge/errors";
import type {
  ChatModelView,
  ChatOwnershipView,
  ChatPermissionDecision,
  ChatPermissionPrompt,
  ChatRetryStatus,
  ChatSessionState,
  ChatUsageView,
  ChatToolActivity,
  ChatToolStatus,
  ChatWriterState,
  ChatWriterView,
  FleetView,
  HostDraft,
  HostKeyPin,
  HostKeyScan,
  HostProfile,
  OrchestratorMoveTarget,
  OrchestratorView,
  SessionOwnerView,
} from "../bridge/wire";
import { modelProviderLabel } from "../domain/engine";
import { useFleet, useFleetStore } from "../state/fleetStore";
import { ComposerBar } from "./ComposerBar";
import { useShellUi } from "./shellUi";
import { tokenTotal } from "./ContextMeter";
import {
  useConversations,
  type ChatMessageView,
  type SessionId,
} from "./conversations";
import { EmptyHero } from "./EmptyHero";
import { PageHeader } from "./PageHeader";
import { TurnLoader } from "./SpiritLoader";
import { activeRunningWorkers, openInbox, projectSummaries } from "./summary";

export interface ChatViewProps {
  session: SessionId;
  /** Label of the connected server, used in the location chip. */
  serverLabel: string | null;
  /** A saved profile that may be shown read-only in the setup chat without connecting. */
  savedServer?: HostProfile | null;
  onOpenWorkers?: (project: string) => void;
  /** Opens Settings → Engine for the honest zero-model state. */
  onOpenEngineSettings?: () => void;
}

function sessionTitle(session: SessionId): string {
  if (session === "setup") return "Setup assistant";
  if (session === "global") return "Global orchestrator";
  return session.slice("project:".length);
}

function stateLabel(state: ChatSessionState, retrying: boolean): string {
  if (retrying) return "Retrying…";
  switch (state) {
    case "idle":
      return "Not started";
    case "connecting":
      return "Connecting…";
    case "streaming":
      return "Replying…";
    case "ready":
      return "Ready";
    case "stopped":
      return "Stopped";
    case "error":
      return "Needs attention";
  }
}

function isGateError(code: string | undefined, message: string | null | undefined): boolean {
  if (code === "gate") return true;
  if (/^chat_(gated|unavailable|not_configured|not_mirrored|unsupported_app)$/.test(code ?? "")) {
    return true;
  }
  return /chat_(gated|unavailable|not_configured|not_mirrored|unsupported_app)/.test(
    message ?? "",
  );
}

function isNotConfiguredError(code: string | undefined, message: string | null | undefined) {
  return code === "chat_not_configured" || /chat_not_configured/.test(message ?? "");
}

/**
 * The calm one-click card for a project orchestrator that has no local record yet. It is
 * never a provider error: the primary action starts the orchestrator through the bridge.
 */
function StartOrchestratorCard({
  project,
  starting,
  onStart,
  onOpenEngineSettings,
}: {
  project: string;
  starting: boolean;
  onStart: () => void;
  onOpenEngineSettings?: () => void;
}) {
  const bridge = useFleetStore((state) => state.bridge);
  const local = bridge?.localOrchestrators;
  const agentStatus = local?.agentStatus;
  const orchestratorList = local?.orchestratorList;
  const orchestratorResume = local?.orchestratorResume;
  const [agentEnrolled, setEnrolled] = useState<boolean | null>(null);
  const [existing, setExisting] = useState<OrchestratorView | null>(null);
  const [resuming, setResuming] = useState(false);
  const [resumeError, setResumeError] = useState<string | null>(null);
  const [homeBusy, setHomeBusy] = useState(false);
  const enrolled = agentStatus ? agentEnrolled : false;

  useEffect(() => {
    if (!agentStatus) return;
    let cancelled = false;
    void agentStatus()
      .then((result) => {
        if (!cancelled) setEnrolled(result.state === "connected");
      })
      .catch(() => {
        if (!cancelled) setEnrolled(false);
      });
    return () => {
      cancelled = true;
    };
  }, [agentStatus]);

  useEffect(() => {
    if (!orchestratorList) return;
    let cancelled = false;
    void orchestratorList()
      .then((rows) => {
        if (!cancelled) setExisting(resolveProjectOrchestrator(rows, project));
      })
      .catch(() => {
        if (!cancelled) setExisting(null);
      });
    return () => {
      cancelled = true;
    };
  }, [orchestratorList, project]);

  const state =
    existing === null
      ? "not created"
      : existing.state === "running" || existing.state === "starting"
        ? "running"
        : "stopped";
  // A saved orchestrator is resumed, never duplicated; only a missing one is created.
  const canResume = existing !== null && state !== "running" && Boolean(orchestratorResume);
  const busy = starting || resuming;

  async function resume() {
    if (!existing || !orchestratorResume || busy) return;
    setResuming(true);
    setResumeError(null);
    try {
      const card = await orchestratorResume(existing.name, existing.project, existing.node);
      setExisting(card);
      onStart();
    } catch (caught) {
      setResumeError(toSpiritError(caught).message);
    } finally {
      setResuming(false);
    }
  }

  // The local agent is not running: offer to start/resume the orchestrator on the home.
  const canUseHome =
    enrolled === false && existing !== null && existing.node == null && state !== "running";

  async function resumeOnHome() {
    if (!existing || !orchestratorResume || busy || homeBusy) return;
    setHomeBusy(true);
    setResumeError(null);
    try {
      const card = await orchestratorResume(existing.name, existing.project, null);
      setExisting(card);
      onStart();
    } catch (caught) {
      setResumeError(toSpiritError(caught).message);
    } finally {
      setHomeBusy(false);
    }
  }

  const label = canResume
    ? `Resume ${project} orchestrator`
    : existing === null
      ? `Create & Start ${project} orchestrator`
      : `Start ${project} orchestrator`;

  return (
    <div
      className="nx-chat__error nx-chat__start"
      role="region"
      aria-label={`Start ${project} orchestrator`}
      data-testid="start-orchestrator"
    >
      <strong>{project} isn’t running on this Mac yet</strong>
      <span>
        {project} doesn’t have a local orchestrator running on this Mac yet.{" "}
        {canResume
          ? "Its saved orchestrator is stopped; resume it to chat right away."
          : "Start one and you can chat right away with the models you already connected."}
      </span>
      <span className="nx-chat__start-state" data-testid="project-orchestrator-state" data-state={state}>
        Orchestrator: {state}
        {existing ? ` (${existing.name})` : ""}
      </span>
      {resumeError ? (
        <span role="alert" data-testid="project-orchestrator-resume-error">
          {resumeError}
        </span>
      ) : null}
      <div className="nx-chat__error-actions">
        <button
          type="button"
          className="nx-btn nx-btn--sm"
          onClick={canResume ? () => void resume() : onStart}
          disabled={busy}
          aria-busy={busy}
          data-testid="start-orchestrator-button"
        >
          {busy ? (canResume ? "Resuming…" : "Starting…") : label}
        </button>
        {enrolled === false ? (
          <span data-testid="local-agent-not-running">
            The local agent isn’t running on this Mac.
            {canUseHome ? " Start this orchestrator on your connected home instead." : ""}
          </span>
        ) : null}
        {canUseHome ? (
          <button
            type="button"
            className="nx-btn nx-btn--sm"
            onClick={() => void resumeOnHome()}
            disabled={busy || homeBusy}
            data-testid="start-orchestrator-home-button"
          >
            {homeBusy ? "Starting…" : `Resume ${project} on home`}
          </button>
        ) : null}
        {enrolled === false && onOpenEngineSettings ? (
          <button
            type="button"
            className="nx-btn nx-btn--quiet nx-btn--sm"
            onClick={onOpenEngineSettings}
          >
            Use OpenCode models locally
          </button>
        ) : null}
      </div>
    </div>
  );
}

function composerPlaceholder(
  session: SessionId,
  state: ChatSessionState,
  missingModel: boolean,
  retrying: boolean,
): string {
  if (missingModel) return "Choose a model in Settings → Engine";
  if (retrying) return "Waiting for the provider to retry… Stop retrying to switch model";
  switch (state) {
    case "connecting":
      return session === "setup"
        ? "Starting the setup assistant…"
        : "Connecting to the orchestrator…";
    case "streaming":
      return "Waiting for the reply… Stop to cancel";
    case "stopped":
      return "Stopped. Start to continue.";
    case "error":
      return "Resolve the chat error to continue.";
    case "idle":
      return "Start the conversation to continue.";
    case "ready":
      return session === "setup"
        ? "Tell me about your server, or ask anything"
        : `Message the ${session === "global" ? "global" : "project"} orchestrator`;
  }
}

function retryTitle(retry: ChatRetryStatus): string {
  const title = retry.action?.title.trim();
  if (title) return title;
  if (retry.action?.reason === "free_tier_limit") return "Free limit reached";
  return "The provider is retrying";
}

function retryMessage(retry: ChatRetryStatus): string {
  const actionMessage = retry.action?.message.trim();
  if (actionMessage) return actionMessage;
  const message = retry.message.trim();
  if (message) return message;
  return "OpenCode paused this turn before its next attempt.";
}

function retryDetail(retry: ChatRetryStatus): string | null {
  if (retry.action?.reason === "free_tier_limit") {
    return "The included free model reached its usage limit. Switch to another free model, connect a provider, or stop the retry.";
  }
  if (retry.action?.provider) {
    return `The ${retry.action.provider} provider asked OpenCode to wait before trying again.`;
  }
  return null;
}

function retryWaitLabel(retry: ChatRetryStatus, now: number): string {
  const attempt = retry.attempt > 0 ? `Attempt ${retry.attempt} · ` : "";
  if (retry.next == null) return `${attempt}OpenCode is retrying automatically.`;
  const seconds = Math.max(0, Math.ceil((retry.next - now) / 1000));
  if (seconds === 0) return `${attempt}Retrying now…`;
  if (seconds <= 120) return `${attempt}Retrying in ${seconds}s…`;
  const at = new Date(retry.next);
  const time = Number.isNaN(at.getTime())
    ? "later"
    : at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return `${attempt}OpenCode will retry at ${time}.`;
}

/** The holder name StackSpirit itself uses when it takes a session over. */
const LOCAL_HOLDER = "stackspirit";

/**
 * The orchestrator that serves a chat session. Global (`project === null`) selects the
 * project-less card; a project prefers its `chat` card, then falls back to the first
 * card for that project.
 */
export function selectProjectOrchestrator(
  orchestrators: readonly OrchestratorView[],
  project: string | null,
): OrchestratorView | null {
  const matches = orchestrators.filter((row) => (row.project ?? null) === project);
  if (project === null) return matches[0] ?? null;
  return matches.find((row) => row.app === "chat") ?? matches[0] ?? null;
}

/**
 * Discovery for the project view's Start/Resume card, matching the core's
 * `select_project_orchestrator`: a running orchestrator for the project, then the one named
 * for the project, then any stopped project-bound card (e.g. `local-<project>`).
 */
export function resolveProjectOrchestrator(
  orchestrators: readonly OrchestratorView[],
  project: string,
): OrchestratorView | null {
  const matches = orchestrators.filter(
    (row) => row.name === project || row.project === project,
  );
  // Mirrors core: a real (non-chat) card outranks a chat stub, then live, then canonical.
  const rank = (row: OrchestratorView) =>
    (row.app === "chat" ? 4 : 0) +
    (row.state === "running" || row.state === "starting" ? 0 : 2) +
    (row.name === project ? 0 : 1);
  return matches.reduce<OrchestratorView | null>(
    (best, row) => (best === null || rank(row) < rank(best) ? row : best),
    null,
  );
}

/**
 * The structured owner of this orchestrator's VOWP session when another surface holds
 * it. Returns null when there is no owner, the owner is StackSpirit, or the session
 * cannot be resolved from the last snapshot.
 */
function foreignOwner(
  fleet: FleetView | null,
  session: SessionId,
): { sessionId: string; owner: SessionOwnerView } | null {
  if (!fleet || session === "setup") return null;
  const project = session.startsWith("project:")
    ? session.slice("project:".length)
    : null;
  const orchestrator = selectProjectOrchestrator(fleet.orchestrators, project);
  if (!orchestrator) return null;
  const owner = fleet.session_owners?.find(
    (row) => row.session_id === orchestrator.session_id,
  );
  if (!owner) return null;
  if (owner.holder.trim().toLowerCase() === LOCAL_HOLDER) return null;
  return { sessionId: owner.session_id, owner };
}

function activityLabel(activity: string): string {
  switch (activity) {
    case "idle":
      return "Idle";
    case "running":
      return "Running";
    case "waiting":
      return "Waiting";
    default:
      return activity
        ? activity.charAt(0).toUpperCase() + activity.slice(1)
        : "Unknown";
  }
}

/** The saved label usually names the node already ("OpenCode · macbook"); do not repeat it. */
function whereLabel(owner: SessionOwnerView): string {
  return owner.label.toLowerCase().includes(owner.node.toLowerCase())
    ? owner.label
    : `${owner.label} · ${owner.node}`;
}

/**
 * A human name for the surface kind VOWP reports on a chat session owner.
 *
 * Known VOWP kinds get their product spelling; an unknown kind is title-cased so a
 * newer VOWP still shows something readable instead of hiding the owner.
 */
function chatOwnerKindLabel(kind: string): string {
  switch (kind.trim().toLowerCase()) {
    case "chat":
    case "cli":
      return "CLI";
    case "opencode":
      return "OpenCode";
    case "claude-code":
    case "claude_code":
    case "claudecode":
      return "Claude Code";
    case "desktop-ui":
    case "desktop_ui":
    case "stackspirit":
      return "StackSpirit";
    case "agent":
      return "Agent";
    case "mcp":
      return "MCP";
    case "mirror":
      return "Mirror";
    case "api":
      return "API";
    case "unknown":
    case "":
      return "Unknown owner";
    default: {
      const words = kind.trim().replace(/[_-]+/g, " ").split(/\s+/).filter(Boolean);
      return words.length > 0
        ? words.map((word) => word.charAt(0).toUpperCase() + word.slice(1)).join(" ")
        : "Unknown owner";
    }
  }
}

/**
 * The header chip text: kind first, then only the label/holder/node values that add
 * information. A saved label often repeats the kind ("OpenCode · macbook"), so it
 * becomes the base instead of being prefixed a second time.
 */
function chatOwnerChipLabel(owner: ChatOwnershipView): string {
  const kind = chatOwnerKindLabel(owner.kind);
  const label = owner.label?.trim() ?? "";
  const holder = owner.holder?.trim() ?? "";
  const node = owner.node?.trim() ?? "";
  const rawKind = owner.kind.trim().replace(/[_-]+/g, " ").toLowerCase();
  const labelNamesKind =
    label.length > 0 &&
    [kind.toLowerCase(), rawKind].some(
      (term) => term.length > 0 && label.toLowerCase().includes(term),
    );
  const parts: string[] = labelNamesKind ? [label] : [kind, ...(label ? [label] : [])];
  const add = (value: string) => {
    if (!value) return;
    if (parts.join(" · ").toLowerCase().includes(value.toLowerCase())) return;
    parts.push(value);
  };
  add(holder);
  add(node);
  return parts.join(" · ");
}

/** The name the read-only notice uses for the writer. */
function chatOwnerWriter(owner: ChatOwnershipView | null): string {
  const label = owner?.label?.trim();
  if (label) return label;
  const holder = owner?.holder?.trim();
  if (holder) return holder;
  return owner ? chatOwnerKindLabel(owner.kind) : "Another window";
}

function chatOwnerTitle(owner: ChatOwnershipView): string {
  const activity = owner.activity?.trim();
  return `Session owner: ${chatOwnerChipLabel(owner)}${
    activity ? ` · ${activityLabel(activity)}` : ""
  }`;
}

/** The composer placeholder when the transport says this window is read-only. */
function readOnlyPlaceholder(owner: ChatOwnershipView | null): string {
  return owner
    ? `Read-only · ${chatOwnerWriter(owner)} is writing here`
    : "Read-only · another window holds this session";
}

/**
 * The calm read-only notice above the composer. It names the writer and explains the
 * cooperative release path; it deliberately offers no takeover action.
 */
function ReadOnlyNotice({ owner }: { owner: ChatOwnershipView | null }) {
  const titleId = useId();
  return (
    <section
      className="nx-readonly"
      aria-labelledby={titleId}
      data-testid="readonly-notice"
    >
      <p className="nx-readonly__title" id={titleId}>
        <strong>{chatOwnerWriter(owner)}</strong> is writing in this session. This window is
        read-only.
      </p>
      <p className="nx-readonly__body">
        Close the other window or end the CLI session to release it. This window becomes
        writable again when the session is free.
      </p>
    </section>
  );
}

/**
 * The safe reason a writer state cannot offer Take over. It never repeats raw VOWP
 * text, a command line, a path or an identifier.
 */
function writerReasonCopy(writer: ChatWriterView): string {
  switch (writer.reason) {
    case "node_hosted":
      return "Take over isn't available because this orchestrator runs on a node. VOWP can only take over a home-hosted orchestrator, so this window stays read-only.";
    case "vowp_too_old":
      return "Take over needs a newer VOWP that supports session takeover. This window stays read-only.";
    case "not_home":
      return "Take over needs StackSpirit on the same computer as the VOWP home; this chat is not configured for takeover here. This window stays read-only.";
    case "orchestrator_stopped":
      return "The orchestrator is stopped. Resume it, then try Take over again. This window stays read-only.";
    case "attach_refused":
      return "The last takeover did not go through. Nothing changed in this window.";
    case "attach_failed":
      return "The last takeover attempt failed. Nothing changed in this window.";
    case "lease_lost":
      return "Another writer took this session over. This window is read-only.";
    case "session_mismatch":
      return "This chat no longer matches the orchestrator session, so takeover is unavailable. This window stays read-only.";
    default:
      if (writer.state === "lost") {
        return "Another writer took this session over. This window is read-only.";
      }
      if (writer.state === "available") {
        return "This session is not ready for takeover yet. This window stays read-only.";
      }
      return "Take over isn't available for this session. This window stays read-only.";
  }
}

/** A typed bridge failure rendered as calm copy; the raw message and identifiers never appear. */
function writerErrorCopy(error: SpiritError): string {
  const typed = error.code === "gate" ? error.message : "";
  if (error.code === "chat_takeover_conflict" || /chat_takeover_conflict/.test(typed)) {
    return "Someone started writing while you were deciding. Nothing changed here; you can take over again if you still want to.";
  }
  if (error.code === "chat_takeover_unsupported" || /chat_takeover_unsupported/.test(typed)) {
    return "This VOWP does not support session takeover. This window stays read-only.";
  }
  if (error.code === "chat_takeover_unavailable" || /chat_takeover_unavailable/.test(typed)) {
    return "Session takeover isn't available right now. This window stays read-only.";
  }
  if (error.code === "chat_writer_lost" || /chat_writer_lost/.test(typed)) {
    return "Another writer took this session over. This window is read-only.";
  }
  switch (error.code) {
    case "preview_unavailable":
      return "Session takeover needs the desktop host. Preview mode never makes this window the writer.";
    case "chat_read_only":
      return "This window is read-only.";
    case "gate":
      return "This chat is app-owned, so takeover does not apply here.";
    default:
      return "The session writer action did not go through. Nothing changed in this session.";
  }
}

/** A live holder needs the replace confirmation; a stale row is replaced without it. */
function ownerLeaseIsLive(owner: ChatOwnershipView | null): boolean {
  if (!owner) return false;
  const state = owner.state?.trim().toLowerCase();
  return state !== "stale" && state !== "expired" && state !== "released";
}

/** The held writer chip names StackSpirit and keeps any safe node the owner already reports. */
function writerChipLabel(owner: ChatOwnershipView | null): string {
  const label = owner?.label?.trim();
  if (label && label.toLowerCase().includes("stackspirit")) return label;
  return "StackSpirit";
}

function describeTakeOver(owner: ChatOwnershipView | null): ReactNode {
  if (!owner) {
    return (
      <>
        <p>No one is writing in this session. Taking over makes this window the writer.</p>
        <p>A window that connects to the orchestrator directly may keep working until it notices.</p>
      </>
    );
  }
  const stale = !ownerLeaseIsLive(owner);
  const activity = owner.activity?.trim();
  return (
    <>
      <p>
        {stale
          ? `The previous writer left. Taking over lets this window write again.`
          : `${chatOwnerWriter(owner)} is writing in this session.${activity ? ` (${activityLabel(activity)})` : ""} Taking over makes the other app read-only.`}
      </p>
      {!stale && activity === "running" ? (
        <p>A turn is running. It may be interrupted after a few seconds.</p>
      ) : null}
      <p>A window that connects to the orchestrator directly may keep working until it notices.</p>
    </>
  );
}

/**
 * The last point where the owner can cancel a takeover or release. It is a real modal
 * for keyboard and screen-reader users: Cancel takes focus, Tab stays inside, Escape
 * cancels and focus returns to the control that opened it.
 */
function WriterConfirmDialog({
  title,
  confirmLabel,
  children,
  onCancel,
  onConfirm,
}: {
  title: string;
  confirmLabel: string;
  children: ReactNode;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const titleId = useId();
  const bodyId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const previous = document.activeElement;
    cancelRef.current?.focus();
    return () => {
      if (previous instanceof HTMLElement && previous.isConnected) {
        previous.focus();
      }
    };
  }, []);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onCancel();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = dialogRef.current?.querySelectorAll<HTMLElement>(
      'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    );
    if (!focusable || focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (!first || !last) return;
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !dialogRef.current?.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || !dialogRef.current?.contains(active))) {
      event.preventDefault();
      first.focus();
    }
  }

  return (
    <div className="nx-writer-dialog">
      <div
        ref={dialogRef}
        className="nx-writer-dialog__card"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={bodyId}
        onKeyDown={onKeyDown}
        data-testid="writer-dialog"
      >
        <h2 className="nx-writer-dialog__title" id={titleId}>
          {title}
        </h2>
        <div className="nx-writer-dialog__body" id={bodyId}>
          {children}
        </div>
        <div className="nx-writer-dialog__actions">
          <button
            ref={cancelRef}
            type="button"
            className="nx-btn nx-btn--quiet nx-btn--sm"
            onClick={onCancel}
          >
            Cancel
          </button>
          <button type="button" className="nx-btn nx-btn--sm" onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * The separate writer action row. It is deliberately outside the calm read-only notice:
 * the notice never gains a button, and an ineligible session gets reason text only.
 */
function WriterControl({
  writer,
  state,
  ownership,
  canOfferTakeOver,
  canRelease,
  error,
  takeOverRef,
  releaseRef,
  onTakeOver,
  onRelease,
}: {
  writer: ChatWriterView | null;
  state: ChatWriterState | null;
  ownership: ChatOwnershipView | null;
  canOfferTakeOver: boolean;
  canRelease: boolean;
  error: SpiritError | null;
  takeOverRef: RefObject<HTMLButtonElement | null>;
  releaseRef: RefObject<HTMLButtonElement | null>;
  onTakeOver: () => void;
  onRelease: () => void;
}) {
  if (!writer || state === null) return null;

  if (state === "held") {
    return (
      <div
        className="nx-writer nx-writer--held"
        data-testid="writer-control"
        role="group"
        aria-label="Session writer"
      >
        <span className="nx-writer__status" data-testid="writer-status">
          {canRelease
            ? "You are writing here. Release the session to let another app write."
            : "You are writing here."}
        </span>
        {canRelease ? (
          <button
            ref={releaseRef}
            type="button"
            className="nx-btn nx-btn--quiet nx-btn--sm"
            onClick={onRelease}
            data-testid="writer-release"
          >
            Release
          </button>
        ) : null}
        {error ? (
          <p className="nx-writer__error" role="alert" data-testid="writer-error" data-code={error.code}>
            {writerErrorCopy(error)}
          </p>
        ) : null}
      </div>
    );
  }

  if (state === "attaching" || state === "releasing") {
    return (
      <div
        className="nx-writer nx-writer--busy"
        data-testid="writer-control"
        role="status"
        data-state={state}
      >
        <span className="nx-writer__status" data-testid="writer-status">
          {state === "attaching" ? "Taking over the session…" : "Releasing the session…"}
        </span>
      </div>
    );
  }

  const canOffer =
    state === "available" && canOfferTakeOver;
  if (canOffer) {
    return (
      <div
        className="nx-writer"
        data-testid="writer-control"
        role="group"
        aria-label="Session writer"
      >
        <span className="nx-writer__status" data-testid="writer-status">
          {ownership
            ? "This window is read-only. Take over to write here."
            : "No one is writing in this session."}
        </span>
        <button
          ref={takeOverRef}
          type="button"
          className="nx-btn nx-btn--sm"
          onClick={onTakeOver}
          data-testid="writer-take-over"
        >
          Take over
        </button>
        {error ? (
          <p className="nx-writer__error" role="alert" data-testid="writer-error" data-code={error.code}>
            {writerErrorCopy(error)}
          </p>
        ) : null}
      </div>
    );
  }

  return (
    <div
      className="nx-writer nx-writer--unavailable"
      data-testid="writer-control"
      role="status"
    >
      <p className="nx-writer__reason" data-testid="writer-reason">
        {writerReasonCopy(writer)}
      </p>
      {error ? (
        <p className="nx-writer__error" role="alert" data-testid="writer-error" data-code={error.code}>
          {writerErrorCopy(error)}
        </p>
      ) : null}
    </div>
  );
}


function confirmCopy(owner: SessionOwnerView): string {
  if (owner.activity === "running") {
    return `${owner.label} is currently running a turn. Taking over will fence its writes and tell it to stop. Take over anyway?`;
  }
  return `${owner.label} is ${owner.activity === "waiting" ? "waiting on an answer" : "still active"}. Taking over will fence its writes and tell it to stop. Take over anyway?`;
}

function isNotSupported(code: string, message: string): boolean {
  return code === "gate" && /not_supported|chat_gated/.test(message);
}

/** Calm Meadow status bar: who holds this session, what they are doing, and Take over. */
function OwnershipBanner({
  session,
  hideTakeOver = false,
}: {
  session: SessionId;
  /** The chat writer action is visible; do not show a second, different Take over. */
  hideTakeOver?: boolean;
}) {
  const fleet = useFleetStore((state) => state.fleet);
  const bridge = useFleetStore((state) => state.bridge);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [unsupported, setUnsupported] = useState(false);

  const found = foreignOwner(fleet, session);
  if (!found) return null;
  const { sessionId, owner } = found;
  const takeover = bridge?.sessions?.takeoverSession;
  const canAct = Boolean(takeover) && !fleet?.stale && !unsupported && !hideTakeOver;
  const idle = owner.activity === "idle";

  async function run() {
    if (!bridge?.sessions?.takeoverSession || busy) return;
    setBusy(true);
    setError(null);
    try {
      await bridge.sessions.takeoverSession(sessionId);
      setConfirming(false);
      try {
        await bridge.fleet.refresh();
      } catch {
        // The next snapshot shows the new owner; the takeover itself succeeded.
      }
    } catch (caught) {
      const failure = toSpiritError(caught);
      setConfirming(false);
      if (isNotSupported(failure.code, failure.message)) {
        setUnsupported(true);
        setError(
          /chat_gated/.test(failure.message)
            ? "Pair this computer as a local node in Settings to take sessions over. This session stays read-only until then."
            : "Take over isn't available with this StackSpirit yet. This session stays read-only.",
        );
      } else {
        setError(failure.message);
      }
    } finally {
      setBusy(false);
    }
  }

  function onTakeOver() {
    if (idle) void run();
    else setConfirming(true);
  }

  return (
    <div
      className="nx-owner"
      data-testid="ownership-banner"
      role="region"
      aria-label="Session ownership"
    >
      <div className="nx-owner__row">
        <span className="nx-owner__where" data-testid="ownership-where">
          Active on <strong>{whereLabel(owner)}</strong>
        </span>
        <span
          className={`nx-owner__badge nx-owner__badge--${owner.activity}`}
          data-testid="ownership-activity"
        >
          {activityLabel(owner.activity)}
        </span>
        {!confirming && !hideTakeOver ? (
          <button
            type="button"
            className="nx-btn nx-btn--sm"
            onClick={onTakeOver}
            disabled={!canAct || busy}
            title={
              canAct
                ? undefined
                : "Take over is unavailable until a fresh snapshot is loaded."
            }
          >
            {busy ? "Taking over…" : "Take over"}
          </button>
        ) : null}
      </div>
      {confirming ? (
        <div
          className="nx-owner__confirm"
          role="alertdialog"
          aria-label="Confirm take over"
        >
          <p>{confirmCopy(owner)}</p>
          <div className="nx-owner__actions">
            <button
              type="button"
              className="nx-btn nx-btn--sm"
              onClick={() => void run()}
              disabled={busy}
            >
              {busy ? "Taking over…" : "Take over anyway"}
            </button>
            <button
              type="button"
              className="nx-btn nx-btn--quiet nx-btn--sm"
              onClick={() => setConfirming(false)}
              disabled={busy}
            >
              Cancel
            </button>
          </div>
        </div>
      ) : null}
      {error ? (
        <p
          className="nx-owner__error"
          role="alert"
          data-testid="ownership-error"
        >
          {error}
        </p>
      ) : null}
    </div>
  );
}

interface HostTarget {
  key: string;
  label: string;
  target: OrchestratorMoveTarget | null;
  disabled: boolean;
  hint: string | null;
}

function hostLabel(orchestrator: OrchestratorView): string {
  return orchestrator.node == null ? "Home" : `Local · ${orchestrator.node}`;
}

function movedNote(target: OrchestratorMoveTarget): string {
  return `Moved to ${target.kind === "home" ? "Home" : target.node}. Reconnect the chat to use the new host.`;
}

/**
 * The explicit session host control. It only ever shows the host from the refreshed
 * orchestrator card, and it moves nothing until the owner picks a target.
 */
function HostControl({
  session,
  attached,
  onReconnect,
}: {
  session: SessionId;
  attached: boolean;
  onReconnect: () => void;
}) {
  const fleet = useFleetStore((state) => state.fleet);
  const bridge = useFleetStore((state) => state.bridge);
  const [open, setOpen] = useState(false);
  const [agentNode, setAgentNode] = useState<string | null>(null);
  const [agentLoaded, setAgentLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const panelId = useId();

  const project = session.startsWith("project:") ? session.slice("project:".length) : null;
  const orchestrator =
    session === "setup" || !fleet
      ? null
      : selectProjectOrchestrator(fleet.orchestrators, project);
  const local = bridge?.localOrchestrators;
  const move = local?.orchestratorMove;
  const overview = local?.overview;

  useEffect(() => {
    if (!open || !overview) return;
    let cancelled = false;
    void overview()
      .then((result) => {
        if (cancelled) return;
        setAgentNode(
          result.agent.state === "connected" ? (result.agent.node ?? null) : null,
        );
        setAgentLoaded(true);
      })
      .catch(() => {
        if (cancelled) return;
        setAgentNode(null);
        setAgentLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, [open, overview]);

  if (!orchestrator || !local || !move) return null;

  const home: HostTarget = {
    key: "home",
    label: "Home (this server)",
    target: { kind: "home" },
    disabled: orchestrator.node == null,
    hint: orchestrator.node == null ? "Already here" : null,
  };
  const localRow: HostTarget = agentNode
    ? {
        key: "local",
        label: `Local · ${agentNode}`,
        target: { kind: "node", node: agentNode },
        disabled: orchestrator.node === agentNode,
        hint: orchestrator.node === agentNode ? "Already here" : null,
      }
    : {
        key: "local",
        label: "Local · no connected agent",
        target: null,
        disabled: true,
        hint: agentLoaded
          ? "Pair and connect the local agent to run this chat on your Mac."
          : "Checking the local agent…",
      };
  const rows = agentNode ? [localRow, home] : [home, localRow];

  function close() {
    setOpen(false);
    buttonRef.current?.focus();
  }

  async function choose(target: OrchestratorMoveTarget) {
    if (!orchestrator || !move || busy) return;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      await move(orchestrator.name, target);
      setOpen(false);
      try {
        await bridge?.fleet.refresh();
      } catch {
        // The next snapshot shows the new host; the move itself succeeded.
      }
      setNote(movedNote(target));
    } catch (caught) {
      const failure = toSpiritError(caught);
      setError(
        isNotSupported(failure.code, failure.message)
          ? "This StackSpirit cannot move orchestrators yet. The chat stays on its current host."
          : failure.message,
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className="nx-host"
      onKeyDown={(event) => {
        if (event.key === "Escape" && open) {
          event.stopPropagation();
          close();
        }
      }}
    >
      <div className="nx-host__row">
        <span className="nx-host__label">Host</span>
        <button
          ref={buttonRef}
          type="button"
          className="nx-btn nx-btn--quiet nx-btn--sm nx-host__button"
          data-testid="orchestrator-host"
          aria-expanded={open}
          aria-controls={open ? panelId : undefined}
          aria-label={`Session host: ${hostLabel(orchestrator)}`}
          onClick={() => {
            if (!open) setAgentLoaded(false);
            setOpen((value) => !value);
          }}
        >
          {hostLabel(orchestrator)}
        </button>
        {note ? (
          <span className="nx-host__status" role="status" data-testid="orchestrator-host-note">
            {note}
            {attached ? (
              <button
                type="button"
                className="nx-btn nx-btn--sm"
                onClick={onReconnect}
              >
                Reconnect
              </button>
            ) : null}
          </span>
        ) : null}
      </div>
      {open ? (
        <div
          id={panelId}
          className="nx-host__panel"
          role="group"
          aria-label="Choose session host"
          data-testid="orchestrator-host-panel"
        >
          {rows.map((row) => (
            <div className="nx-host__option" key={row.key}>
              <button
                type="button"
                className="nx-btn nx-btn--sm"
                disabled={row.disabled || busy || !row.target}
                onClick={() => row.target && void choose(row.target)}
              >
                {row.label}
              </button>
              {row.hint ? <span className="nx-host__hint">{row.hint}</span> : null}
            </div>
          ))}
        </div>
      ) : null}
      {error ? (
        <p className="nx-host__error" role="alert" data-testid="orchestrator-host-error">
          {error}
        </p>
      ) : null}
    </div>
  );
}

export function ChatView({
  session,
  serverLabel,
  savedServer = null,
  onOpenWorkers,
  onOpenEngineSettings,
}: ChatViewProps) {
  const messages = useConversations((state) => state.messages[session]) ?? [];
  const view = useConversations((state) => state.views[session]) ?? null;
  const send = useConversations((state) => state.send);
  const stop = useConversations((state) => state.stop);
  const setModel = useConversations((state) => state.setModel);
  const replyPermission = useConversations((state) => state.replyPermission);
  const permissionBusy = useConversations((state) => state.permissionBusy[session] ?? null);
  const permissionError = useConversations((state) => state.permissionError[session] ?? null);
  const open = useConversations((state) => state.open);
  const restart = useConversations((state) => state.restart);
  const provision = useConversations((state) => state.provision);
  const loadEarlier = useConversations((state) => state.loadEarlier);
  const requestSecureServerCard = useConversations((state) => state.requestSecureServerCard);
  const hasEarlier = useConversations((state) => Boolean(state.earlier[session]));
  const loadingEarlier = useConversations((state) => state.loadingEarlier[session] ?? false);
  const truncated = useConversations((state) => state.truncated[session] ?? false);
  const endRef = useRef<HTMLDivElement>(null);
  const composerDraft = useShellUi((state) => state.composerDraft);
  const clearComposerDraft = useShellUi((state) => state.clearComposerDraft);
  const fleet = useFleet();
  const bridge = useFleetStore((state) => state.bridge);
  const [newServerRequested, setNewServerRequested] = useState(false);
  const project = session.startsWith("project:") ? session.slice("project:".length) : null;
  const state = view?.state ?? "idle";
  const retry = view?.retry ?? null;
  const [now, setNow] = useState(() => Date.now());
  const [modelPickerRequest, setModelPickerRequest] = useState(0);
  const [stopping, setStopping] = useState(false);
  const [confirmingTakeOver, setConfirmingTakeOver] = useState(false);
  const [confirmingRelease, setConfirmingRelease] = useState(false);
  const [writerBusy, setWriterBusy] = useState<"takeOver" | "release" | null>(null);
  const [writerError, setWriterError] = useState<SpiritError | null>(null);
  const takeOverRef = useRef<HTMLButtonElement>(null);
  const releaseRef = useRef<HTMLButtonElement>(null);
  const focusAfterWriterAction = useRef<"takeOver" | "release" | null>(null);

  useEffect(() => {
    void open(session, project, "local");
  }, [open, session, project]);

  useEffect(() => {
    if (retry?.next == null) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [retry?.next]);

  useEffect(() => {
    endRef.current?.scrollIntoView?.({ block: "end" });
  }, [messages.length, view?.pending_permissions?.length]);

  // Focus moves to the control that replaced the dialog once the action settles. Cancel
  // and Escape are handled by the dialog itself, which restores the trigger.
  useEffect(() => {
    if (writerBusy !== null) return;
    const target = focusAfterWriterAction.current;
    if (!target) return;
    focusAfterWriterAction.current = null;
    if (target === "release" && view?.writer?.state === "held") {
      releaseRef.current?.focus();
    } else if (target === "takeOver" && view?.writer?.state === "available") {
      takeOverRef.current?.focus();
    }
  }, [writerBusy, view?.writer?.state]);

  const isSetup = session === "setup";
  const models: ChatModelView[] | undefined = view?.available_models;
  // The model link is honest: it appears only when the engine reports zero usable
  // models, not when an older bridge simply cannot list them.
  const noModelsReported = isSetup && models !== undefined && models.length === 0;
  // A ready setup session without a model cannot send. The old bridge case still
  // disables send even though there is no list to prove why.
  const missingModel = isSetup && state === "ready" && !view?.model;
  const canChat = state === "ready" && !missingModel;
  const ownership = view?.ownership ?? null;
  const writer = view?.writer ?? null;
  // A writer-aware session is writable only while this app holds the lease. An old
  // bridge omits `writer`, so it keeps the exact `access` behavior it had before.
  const readOnly = writer
    ? writer.state !== "held" || view?.access !== "owner"
    : view?.access === "read_only";
  const canWrite = canChat && !readOnly;
  const writerState: ChatWriterState | null =
    writerBusy === "takeOver"
      ? "attaching"
      : writerBusy === "release"
        ? "releasing"
        : (writer?.state ?? null);
  const takeoverOffered =
    !isSetup &&
    writerBusy === null &&
    Boolean(bridge?.chat?.takeOver) &&
    writer !== null &&
    writer.state === "available" &&
    writer.can_take_over &&
    writer.reason !== "node_hosted" &&
    Boolean(view?.attached);
  const canRelease =
    writer !== null && writer.can_release && Boolean(bridge?.chat?.releaseWriter);
  const writerHeld = writer?.state === "held";
  const writerTransitioning = writerState === "attaching" || writerState === "releasing";
  // A saved profile never shows the new-server password form unless the owner asks
  // for another server from the saved-server card.
  const showSecureServerForm = !savedServer || newServerRequested;
  const visibleMessages = showSecureServerForm
    ? messages
    : messages.filter((message) => message.card?.kind !== "secure-server");
  // The usage object is session-cumulative, so only the latest reply carries it.
  const lastAnswerIndex = visibleMessages.reduce(
    (last, message, index) =>
      message.role === "assistant" && (message.origin === "model" || message.origin === "preview")
        ? index
        : last,
    -1,
  );
  const hasConversation = visibleMessages.some(
    (message) =>
      message.role === "user" ||
      message.origin === "model" ||
      message.origin === "preview",
  );
  const hasReply = visibleMessages.some(
    (message) => message.origin === "model" || message.origin === "preview",
  );
  const pendingPermissions = view?.pending_permissions ?? [];
  const lastVisible = visibleMessages[visibleMessages.length - 1];
  // The completed answer footer owns the tail slot once it renders; no second indicator.
  const lastAnswered =
    lastVisible !== undefined &&
    lastVisible.role !== "user" &&
    !lastVisible.streaming &&
    (lastVisible.origin === "model" || lastVisible.origin === "preview") &&
    lastVisible.text.some((paragraph) => paragraph.trim() !== "");
  const hero = !hasConversation && visibleMessages.length === 0 && pendingPermissions.length === 0;
  const readonlyNotice =
    readOnly &&
    !writerTransitioning &&
    !(takeoverOffered && !ownership) ? (
      <ReadOnlyNotice owner={ownership} />
    ) : null;
  const secureCardOpen = visibleMessages.some(
    (message) => message.card?.kind === "secure-server",
  );
  const copy = heroCopy(session, savedServer, fleet);
  const heroTitle = isSetup ? "StackSpirit" : sessionTitle(session);
  const error = view?.error ?? null;
  const gate = isGateError(error?.code, error?.message);
  const needsStart =
    project !== null && isNotConfiguredError(error?.code, error?.message) && !retry;
  const starting = view?.state === "connecting";
  const canRetry = Boolean(error?.retryable);
  const canRestartSetup =
    isSetup &&
    (error?.code === "not_found" || /session_lost/.test(error?.message ?? ""));
  const canSwitchModel = models !== undefined && models.length > 0;
  const allowModelSwitch =
    !readOnly && canSwitchModel && (Boolean(retry) || state === "error");

  function requestModelPicker() {
    if (canSwitchModel) {
      setModelPickerRequest((value) => value + 1);
      return;
    }
    onOpenEngineSettings?.();
  }

  async function stopRetrying() {
    if (stopping) return;
    setStopping(true);
    try {
      await stop(session);
    } finally {
      setStopping(false);
    }
  }

  async function runTakeOver(replaceHolder: boolean) {
    const action = bridge?.chat?.takeOver;
    if (!action || writerBusy !== null) return;
    setConfirmingTakeOver(false);
    setWriterBusy("takeOver");
    setWriterError(null);
    try {
      const next = await action(session, replaceHolder);
      useConversations.setState((current) => ({
        views: { ...current.views, [session]: next },
      }));
      focusAfterWriterAction.current = "release";
    } catch (caught) {
      setWriterError(toSpiritError(caught));
      focusAfterWriterAction.current = "takeOver";
    } finally {
      setWriterBusy(null);
    }
  }

  async function runRelease() {
    const action = bridge?.chat?.releaseWriter;
    if (!action || writerBusy !== null) return;
    setConfirmingRelease(false);
    setWriterBusy("release");
    setWriterError(null);
    try {
      const next = await action(session);
      useConversations.setState((current) => ({
        views: { ...current.views, [session]: next },
      }));
      focusAfterWriterAction.current = "takeOver";
    } catch (caught) {
      setWriterError(toSpiritError(caught));
      focusAfterWriterAction.current = "release";
    } finally {
      setWriterBusy(null);
    }
  }

  function requestTakeOver() {
    if (writerBusy !== null) return;
    setWriterError(null);
    setConfirmingTakeOver(true);
  }

  function requestRelease() {
    if (!canRelease || writerBusy !== null) return;
    setWriterError(null);
    if (state === "streaming") {
      setConfirmingRelease(true);
      return;
    }
    void runRelease();
  }

  // The live text itself is never aria-live. This single polite region only changes on
  // state transitions, so screen readers do not read every delta.
  const announcement =
    state === "streaming"
      ? retry
        ? `Retrying: ${retryTitle(retry)}. ${retryWaitLabel(retry, now)}`
        : "Reply in progress"
      : state === "connecting"
        ? "Connecting…"
        : state === "stopped"
          ? "Stopped"
          : state === "error"
            ? `Error: ${error?.message ?? "Chat error"}`
            : state === "ready"
              ? hasReply
                ? "Reply complete"
                : "Ready"
              : "";
  const writerAnnouncement =
    writerState === "attaching"
      ? "Taking over the session."
      : writerState === "releasing"
        ? "Releasing the session."
        : writerState === "held"
          ? "You are writing in this session."
          : writerState === "lost"
            ? "Another writer took this session over. This window is read-only."
            : writerError
              ? "The session writer action did not go through."
              : "";

  // The chip is derived from the transport view, never from a requested label.
  // Setup is always local. A gate is not a connected location, even though the view
  // carries the requested hint; show "Not connected" until a real transport attaches.
  const chipLocation = isSetup
    ? view
      ? "local"
      : null
    : view && view.state !== "connecting" && !gate
      ? view.location
      : null;
  const chipLabel =
    chipLocation === "local"
      ? "Local · this Mac"
      : chipLocation === "server"
        ? `Server · ${serverLabel ?? "server"}`
        : isSetup
          ? view
            ? "Local · this Mac"
            : "Starting…"
          : view?.state === "connecting"
            ? "Connecting…"
            : "Not connected";
  const chipTitle =
    chipLocation === "local"
      ? isSetup
        ? "Runs on this Mac; the setup transcript is local to this app."
        : "Runs on this Mac and mirrors turns to the home log."
      : chipLocation === "server"
        ? "Runs on the server. Works while this Mac is closed."
        : "No chat transport is connected.";

  return (
    <section className="nx-main" aria-label={sessionTitle(session)}>
      <PageHeader
        title={sessionTitle(session)}
        actions={
          project && onOpenWorkers ? (
            <button
              type="button"
              className="nx-btn nx-btn--quiet nx-btn--sm"
              onClick={() => onOpenWorkers(project)}
            >
              Workers
            </button>
          ) : null
        }
      >
        <span
          className={`nx-chip ${chipLocation === "local" ? "nx-chip--local" : chipLocation === "server" ? "nx-chip--server" : "nx-chip--muted"}`}
          data-testid="session-location"
          data-state={state}
          title={chipTitle}
        >
          <span aria-hidden="true">
            {chipLocation === "local" ? "▣" : chipLocation === "server" ? "◉" : "○"}
          </span>
          {chipLabel}
        </span>
        {ownership && !writerHeld ? (
          <span
            className="nx-chip nx-chip--owner"
            data-testid="session-ownership"
            title={chatOwnerTitle(ownership)}
          >
            <span aria-hidden="true">✎</span>
            {chatOwnerChipLabel(ownership)}
          </span>
        ) : null}
        {writerHeld ? (
          <span
            className="nx-chip nx-chip--writer"
            data-testid="session-writer"
            title="You are writing here. Release the session to let another app write."
          >
            <span aria-hidden="true">✎</span>
            {writerChipLabel(ownership)}
          </span>
        ) : null}
        {view ? (
          <span
            className={`nx-chat-state nx-chat-state--${state}`}
            data-testid="session-state"
          >
            {stateLabel(state, Boolean(retry))}
          </span>
        ) : null}
      </PageHeader>

      {session !== "setup" ? (
        <HostControl
          session={session}
          attached={view != null}
          onReconnect={() => void restart(session)}
        />
      ) : null}
      {session !== "setup" ? (
        <WriterControl
          writer={writer}
          state={writerState}
          ownership={ownership}
          canOfferTakeOver={takeoverOffered}
          canRelease={canRelease}
          error={writerError}
          takeOverRef={takeOverRef}
          releaseRef={releaseRef}
          onTakeOver={requestTakeOver}
          onRelease={requestRelease}
        />
      ) : null}
      {session !== "setup" ? (
        <OwnershipBanner
          session={session}
          hideTakeOver={takeoverOffered || writerHeld}
        />
      ) : null}
      {confirmingTakeOver ? (
        <WriterConfirmDialog
          title="Take over this session?"
          confirmLabel="Take over"
          onCancel={() => setConfirmingTakeOver(false)}
          onConfirm={() => void runTakeOver(ownerLeaseIsLive(ownership))}
        >
          {describeTakeOver(ownership)}
        </WriterConfirmDialog>
      ) : null}
      {confirmingRelease ? (
        <WriterConfirmDialog
          title="Release this session?"
          confirmLabel="Release"
          onCancel={() => setConfirmingRelease(false)}
          onConfirm={() => void runRelease()}
        >
          <p>The current turn keeps running on the orchestrator. Release this session?</p>
        </WriterConfirmDialog>
      ) : null}

      {/* A stable slot keeps the chat (and the composer inside it) mounted when the
          saved-server card appears after the first turn. */}
      <div className="nx-saved-server-slot">
        {isSetup && savedServer && !hero ? (
          <SavedServerCard
            host={savedServer}
            onAddServer={() => {
              setNewServerRequested(true);
              requestSecureServerCard();
            }}
          />
        ) : null}
      </div>

      <div className={`nx-chat${hero ? " nx-chat--hero" : ""}`}>
        {/* Kept in the tree (hidden in hero mode) so inserting the transcript never
            remounts the composer that follows it. */}
        <div
          className="nx-chat__scroll"
          role="group"
          aria-label="Conversation"
          tabIndex={0}
        >
          {hasEarlier || truncated ? (
            <div className="nx-chat__earlier">
              {hasEarlier ? (
                <button
                  type="button"
                  className="nx-btn nx-btn--quiet nx-btn--sm"
                  onClick={() => void loadEarlier(session)}
                  disabled={loadingEarlier}
                >
                  {loadingEarlier ? "Loading earlier messages…" : "Load earlier messages"}
                </button>
              ) : null}
              {truncated ? (
                <span className="nx-chat__note">
                  Older messages may be missing from this transcript.
                </span>
              ) : null}
            </div>
          ) : null}
          {visibleMessages.length > 0 ? (
            <div className="nx-chat__inner">
              {visibleMessages.map((message, index) => (
                <Message
                  key={message.id}
                  session={session}
                  message={message}
                  model={view?.model ?? null}
                  live={state === "streaming" && index === visibleMessages.length - 1}
                  usage={index === lastAnswerIndex ? (view?.usage ?? null) : null}
                  onRetry={
                    !readOnly &&
                    state === "ready" &&
                    retryText(visibleMessages, index)
                      ? () => void send(session, retryText(visibleMessages, index)!)
                      : null
                  }
                />
              ))}
              {state === "streaming" && !retry && !lastAnswered ? (
                <div className="nx-msg__thinking">
                  <TurnLoader />
                </div>
              ) : null}
              <div ref={endRef} />
            </div>
          ) : (
            <div ref={endRef} />
          )}
        </div>

        {retry ? (
          <RetryNotice
            retry={retry}
            now={now}
            stopping={stopping}
            canSwitchModel={canSwitchModel}
            onSwitchModel={requestModelPicker}
            onConnectProvider={onOpenEngineSettings}
            onStop={() => void stopRetrying()}
          />
        ) : null}

        {readonlyNotice && !hero ? readonlyNotice : null}

        <EmptyHero
          active={hero}
          title={heroTitle}
          lede={copy.lede}
          suggestions={copy.suggestions}
          disabled={!canWrite}
          onPick={(text) => void send(session, text)}
          above={
            <>
              {readonlyNotice && hero ? readonlyNotice : null}
              {isSetup && savedServer ? (
                <SavedServerCard
                  host={savedServer}
                  onAddServer={() => {
                    setNewServerRequested(true);
                    requestSecureServerCard();
                  }}
                />
              ) : null}
            </>
          }
        >
          {pendingPermissions.length > 0 ? (
            <div className="nx-perm-dock" data-testid="permission-dock">
              {pendingPermissions.map((prompt) => (
                <PermissionCard
                  key={prompt.id}
                  prompt={prompt}
                  busy={permissionBusy === prompt.id}
                  locked={readOnly || permissionBusy !== null}
                  error={permissionError?.message ?? null}
                  onReply={(decision) => void replyPermission(session, prompt.id, decision)}
                />
              ))}
            </div>
          ) : null}
          <ComposerBar
            session={session}
            placeholder={
              readOnly
                ? readOnlyPlaceholder(ownership)
                : composerPlaceholder(session, state, missingModel, Boolean(retry))
            }
            model={view?.model ?? null}
            models={models}
            modelMissing={noModelsReported}
            disabled={!canWrite}
            streaming={state === "streaming" && !readOnly}
            sending={state === "connecting"}
            stopped={state === "stopped" || state === "idle"}
            allowModelSwitch={allowModelSwitch}
            openModelPickerRequest={modelPickerRequest}
            hero={hero}
            draft={
              composerDraft && composerDraft.session === session
                ? composerDraft
                : null
            }
            onDraftApplied={clearComposerDraft}
            usage={view?.usage}
            onSend={(text, options) => send(session, text, options)}
            onStop={() => void stop(session)}
            onStart={() => void open(session, project, "local")}
            onSelectModel={
              view?.available_models !== undefined
                ? (modelId) => void setModel(session, modelId)
                : undefined
            }
            onConnectServer={() => {
              setNewServerRequested(true);
              requestSecureServerCard();
            }}
            showConnectServer={isSetup && !secureCardOpen}
            onOpenEngineSettings={onOpenEngineSettings}
          />
        </EmptyHero>
      </div>

      {needsStart && project ? (
        <StartOrchestratorCard
          project={project}
          starting={starting}
          onStart={() => void provision(session)}
          onOpenEngineSettings={onOpenEngineSettings}
        />
      ) : null}

      {error && !retry && !needsStart ? (
        <div
          className={`nx-chat__error${gate ? " nx-chat__error--gate" : ""}`}
          role="alert"
          data-testid="chat-error"
          data-code={error.code}
          data-gate={gate ? "true" : "false"}
        >
          <strong>{gate ? "A capability gate is not ready" : errorTitle(error)}</strong>
          <span>{error.message}</span>
          {error.hint ? <span>{error.hint}</span> : null}
          <div className="nx-chat__error-actions">
            {canSwitchModel ? (
              <button
                type="button"
                className="nx-btn nx-btn--quiet nx-btn--sm"
                onClick={requestModelPicker}
              >
                Switch model
              </button>
            ) : null}
            {onOpenEngineSettings && !gate ? (
              <button
                type="button"
                className="nx-btn nx-btn--quiet nx-btn--sm"
                onClick={onOpenEngineSettings}
              >
                Connect a provider
              </button>
            ) : null}
            {canRetry ? (
              <button
                type="button"
                className="nx-btn nx-btn--quiet nx-btn--sm"
                onClick={() =>
                  void open(session, project, "local")
                }
              >
                Reconnect
              </button>
            ) : null}
            {canRestartSetup ? (
              <button
                type="button"
                className="nx-btn nx-btn--quiet nx-btn--sm"
                onClick={() => void restart("setup")}
              >
                Start a new conversation
              </button>
            ) : null}
          </div>
        </div>
      ) : null}

      <span className="nx-sr" role="status" aria-live="polite">
        {announcement}
      </span>
      <span className="nx-sr" role="status" aria-live="polite">
        {writerAnnouncement}
      </span>
    </section>
  );
}

/** The key file's name only; a profile may hold an absolute path that stays private. */
export function keyFileName(identityFile: string | null): string | null {
  const trimmed = identityFile?.trim();
  if (!trimmed) return null;
  const name = trimmed.split(/[\\/]/).filter(Boolean).pop();
  return name && name.length > 0 ? name : null;
}

/**
 * A saved server shown read-only inside the setup chat. It exists so the owner can
 * recognize the computer without connecting; it never renders a key path, a
 * passphrase or a password. "Add another server" is the explicit ask that unlocks
 * the one-time-password card for a new server.
 */
function SavedServerCard({ host, onAddServer }: { host: HostProfile; onAddServer: () => void }) {
  const keyName = keyFileName(host.identity_file);
  return (
    <aside className="nx-saved-server" aria-label="Saved server" data-testid="saved-server-card">
      <div className="nx-saved-server__head">
        <span className="nx-chip nx-chip--tiny nx-chip--local">Saved</span>
        <span className="nx-saved-server__name">{host.label}</span>
      </div>
      <dl className="nx-saved-server__facts">
        <div>
          <dt>Host</dt>
          <dd>{host.ssh.host}</dd>
        </div>
        <div>
          <dt>User</dt>
          <dd>{host.ssh.user}</dd>
        </div>
        <div>
          <dt>Port</dt>
          <dd>{host.ssh.port}</dd>
        </div>
        {keyName ? (
          <div>
            <dt>Key file</dt>
            <dd>{keyName}</dd>
          </div>
        ) : null}
      </dl>
      <p className="nx-saved-server__note">
        Saved on this Mac. You don&apos;t need to connect to chat here.
      </p>
      <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={onAddServer}>
        Add another server
      </button>
    </aside>
  );
}

/**
 * The short hero line and prompt chips. The setup copy never asks for a password:
 * the secure card appears only after an explicit Connect server action.
 */
function heroCopy(
  session: SessionId,
  savedServer: HostProfile | null,
  fleet: FleetView | null,
): { lede: string; suggestions: string[] } {
  if (session === "setup") {
    if (savedServer) {
      return {
        lede:
          "Chat here on this Mac with the bundled engine's free models; your saved server stays read-only until you connect.",
        suggestions: [
          "What can I do with my server?",
          "How do I add another server?",
          "What is an always-on server?",
        ],
      };
    }
    return {
      lede:
        "Type what you want to do. When you are ready, connect a new server — nothing secret is asked for up front.",
      suggestions: [
        "I have a new VPS",
        "I want to use a computer at home",
        "What is an always-on server?",
      ],
    };
  }
  if (session === "global") {
    const projects = fleet?.projects.length ?? 0;
    const running = activeRunningWorkers(fleet).length;
    const inbox = openInbox(fleet).length;
    return {
      lede: `${running} worker${running === 1 ? "" : "s"} running across ${projects} project${
        projects === 1 ? "" : "s"
      }${inbox ? ` · ${inbox} waiting in your inbox` : ""}.`,
      suggestions: ["What needs my attention?", "Start a new project", "How busy is the server?"],
    };
  }
  const name = session.slice("project:".length);
  const summary = projectSummaries(fleet).find((item) => item.name === name);
  return {
    lede: summary
      ? `${summary.running} running · ${summary.ready} ready · ${summary.review} in review${
          summary.inbox ? ` · ${summary.inbox} in inbox` : ""
        }.`
      : "This project's orchestrator.",
    suggestions: ["What's running right now?", "Plan the next job", "Show what's blocked"],
  };
}

/**
 * A calm, unmistakable inline notice for an OpenCode retry loop.
 *
 * The three recovery actions work without dismissing the notice first: switch model opens
 * the composer picker, connect a provider opens Settings → Engine, and stop retrying
 * aborts the turn and returns the composer to Ready.
 */
function RetryNotice({
  retry,
  now,
  stopping,
  canSwitchModel,
  onSwitchModel,
  onConnectProvider,
  onStop,
}: {
  retry: ChatRetryStatus;
  now: number;
  stopping: boolean;
  canSwitchModel: boolean;
  onSwitchModel: () => void;
  onConnectProvider?: () => void;
  onStop: () => void;
}) {
  const titleId = useId();
  const detail = retryDetail(retry);
  return (
    <section
      className="nx-retry"
      aria-labelledby={titleId}
      data-testid="retry-notice"
      data-reason={retry.action?.reason ?? "retry"}
    >
      <h2 className="nx-retry__title" id={titleId}>
        <span className="nx-retry__icon" aria-hidden="true">
          !
        </span>
        {retryTitle(retry)}
      </h2>
      <p className="nx-retry__body">{retryMessage(retry)}</p>
      {detail ? <p className="nx-retry__detail">{detail}</p> : null}
      <p className="nx-retry__wait">{retryWaitLabel(retry, now)}</p>
      <div className="nx-retry__actions">
        {canSwitchModel ? (
          <button type="button" className="nx-btn nx-btn--sm" onClick={onSwitchModel}>
            Switch model
          </button>
        ) : null}
        {onConnectProvider ? (
          <button
            type="button"
            className="nx-btn nx-btn--quiet nx-btn--sm"
            onClick={onConnectProvider}
          >
            Connect a provider
          </button>
        ) : null}
        <button
          type="button"
          className="nx-btn nx-btn--quiet nx-btn--sm"
          onClick={onStop}
          disabled={stopping}
          aria-busy={stopping}
        >
          {stopping ? "Stopping…" : "Stop retrying"}
        </button>
      </div>
    </section>
  );
}

function compactTokens(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
  if (value >= 1000) return `${(value / 1000).toFixed(1).replace(/\.0$/, "")}K`;
  return String(Math.round(value));
}

/** `MM/DD HH:MM` in local time, or null when the timestamp is absent or invalid. */
export function messageStamp(createdAt: string | undefined): string | null {
  if (!createdAt) return null;
  const at = new Date(createdAt);
  if (Number.isNaN(at.getTime())) return null;
  const two = (n: number) => String(n).padStart(2, "0");
  return `${two(at.getMonth() + 1)}/${two(at.getDate())} ${two(at.getHours())}:${two(at.getMinutes())}`;
}

function CopyIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
      <rect x="5" y="5" width="8" height="8" rx="1.5" fill="none" stroke="currentColor" strokeWidth="1.4" />
      <path d="M3 10.5V4A1.5 1.5 0 0 1 4.5 2.5H11" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M3.5 8.5l3 3 6-7" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function RetryIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M13 8a5 5 0 1 1-1.6-3.7M13 2.5v3h-3" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function TokenIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
      <ellipse cx="8" cy="4" rx="5" ry="2" fill="none" stroke="currentColor" strokeWidth="1.3" />
      <path d="M3 4v8c0 1.1 2.2 2 5 2s5-.9 5-2V4M3 8c0 1.1 2.2 2 5 2s5-.9 5-2" fill="none" stroke="currentColor" strokeWidth="1.3" />
    </svg>
  );
}

/** Copy-to-clipboard with a short "copied" confirmation. */
function useCopy(getText: () => string) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );
  async function copy() {
    try {
      await navigator.clipboard?.writeText(getText());
    } catch {
      return;
    }
    setCopied(true);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(false), 1800);
  }
  return { copied, copy };
}

/** Subtle metadata row beneath a user bubble: `MM/DD HH:MM` and an icon Copy button. */
function UserMeta({ message }: { message: ChatMessageView }) {
  const { copied, copy } = useCopy(() => message.text.join("\n\n"));
  const stamp = messageStamp(message.created_at);
  return (
    <div className="nx-user-meta" data-testid="user-meta">
      {stamp ? (
        <time className="nx-user-meta__time" dateTime={message.created_at} data-testid="user-time">
          {stamp}
        </time>
      ) : null}
      <button
        type="button"
        className="nx-icon-btn"
        onClick={() => void copy()}
        aria-label={copied ? "Copied" : "Copy message"}
        title={copied ? "Copied" : "Copy"}
        data-testid="user-copy"
      >
        {copied ? <CheckIcon /> : <CopyIcon />}
      </button>
    </div>
  );
}

function durationLabel(ms: number): string {
  const seconds = Math.max(1, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}

/**
 * The quiet footer under a finished assistant reply: Copy, Retry, token usage, model and
 * provider, and `MM/DD HH:MM`. Retry appears only on the newest reply of a settled
 * session. Nothing here is invented: absent data is omitted.
 */
/** The user text a reply answers: the nearest user message before it. */
function retryText(messages: ChatMessageView[], index: number): string | null {
  for (let i = index - 1; i >= 0; i--) {
    const m = messages[i]!;
    if (m.role === "user") return m.text.join("\n\n") || null;
  }
  return null;
}

function AnswerFooter({
  message,
  model,
  usage,
  onRetry,
}: {
  message: ChatMessageView;
  model: ChatModelView | null;
  usage: ChatUsageView | null;
  onRetry: (() => void) | null;
}) {
  const { copied, copy } = useCopy(() => message.text.join("\n\n"));
  const total = usage ? tokenTotal(usage) : null;
  const preview = message.origin === "preview";
  const stamp = messageStamp(message.created_at);

  return (
    <div className="nx-msg-footer" data-testid="answer-footer">
      <button
        type="button"
        className="nx-msg-footer__btn"
        onClick={() => void copy()}
        aria-label={copied ? "Copied" : "Copy answer"}
        title={copied ? "Copied" : "Copy"}
        data-testid="answer-copy"
      >
        {copied ? <CheckIcon /> : <CopyIcon />}
        <span>{copied ? "Copied" : "Copy"}</span>
      </button>
      {onRetry ? (
        <button
          type="button"
          className="nx-msg-footer__btn"
          onClick={onRetry}
          data-testid="answer-retry"
        >
          <RetryIcon />
          <span>Retry</span>
        </button>
      ) : null}
      <span className="nx-msg-footer__meta" data-testid="answer-meta">
        {total !== null && total > 0 ? (
          <span className="nx-msg-footer__usage" data-testid="answer-usage">
            <TokenIcon />
            Usage {compactTokens(total)} tok
          </span>
        ) : null}
        {preview ? (
          <span data-testid="preview-tag">Preview</span>
        ) : model ? (
          <span data-testid="answer-model">
            {model.label} · {modelProviderLabel(model.provider)}
          </span>
        ) : null}
        {stamp ? <time dateTime={message.created_at}>{stamp}</time> : null}
      </span>
    </div>
  );
}

/** The completion header (and so the toggle) only exists when the engine reported a duration. */
function completedMs0(message: ChatMessageView): boolean {
  return message.completed_ms != null && message.completed_ms > 0;
}

function Message({
  session,
  message,
  model,
  usage,
  live,
  onRetry,
}: {
  session: SessionId;
  message: ChatMessageView;
  /** True for the reply the running turn is still producing: its tool rows stay visible. */
  live: boolean;
  model: ChatModelView | null;
  usage: ChatUsageView | null;
  onRetry: (() => void) | null;
}) {
  const [stepsOpen, setStepsOpen] = useState(false);
  const toolsId = useId();
  if (message.role === "user") {
    return (
      <div className="nx-user">
        <div className="nx-msg nx-msg--user">
          {message.text.map((paragraph, index) => (
            <p key={index}>{paragraph}</p>
          ))}
        </div>
        <UserMeta message={message} />
      </div>
    );
  }
  const received = message.card?.kind === "secure-received";
  const card = <MessageCard session={session} message={message} />;
  const answered =
    !message.streaming &&
    (message.origin === "model" || message.origin === "preview") &&
    message.text.some((paragraph) => paragraph.trim() !== "");
  const steps = message.tools?.length ?? 0;
  const header: string[] = [];
  // Completed tool steps collapse behind the completion header once the reply is done;
  // while the turn streams, live tool rows stay visible.
  const collapsible = answered && !live && steps > 0 && completedMs0(message);
  // Wall-clock turn duration, present only when the engine reported it.
  const completedMs = message.completed_ms;
  if (answered && completedMs != null && completedMs > 0) {
    header.push(`Completed in ${durationLabel(completedMs)}`);
    if (steps > 0) header.push(`${steps} ${steps === 1 ? "step" : "steps"}`);
  }
  return (
    <div
      className={`nx-msg nx-msg--assistant${message.streaming ? " nx-msg--streaming" : ""}`}
      data-testid="assistant-message"
    >
      {header.length > 0 ? (
        steps > 0 ? (
          <button
            type="button"
            className="nx-msg-header nx-msg-header--toggle"
            data-testid="completion-header"
            aria-expanded={stepsOpen}
            aria-controls={toolsId}
            onClick={() => setStepsOpen((open) => !open)}
          >
            <span>{header.join(" · ")}</span>
            <span className="nx-msg-header__chevron" aria-hidden="true">
              {stepsOpen ? "▾" : "▸"}
            </span>
          </button>
        ) : (
          <div className="nx-msg-header" data-testid="completion-header">
            {header.join(" · ")}
          </div>
        )
      ) : null}
      {received ? card : null}
      {message.tools && message.tools.length > 0 && (!collapsible || stepsOpen) ? (
        <ToolActivity tools={message.tools} id={toolsId} />
      ) : null}
      <div className="nx-msg__body">
        {message.text
          .filter((paragraph) => paragraph.trim() !== "" || !message.tools?.length)
          .map((paragraph, index) => (
            <p key={index} className={received ? "nx-secure__outcome" : undefined}>
              {paragraph}
            </p>
          ))}
      </div>
      {received ? null : card}
      {answered ? (
        <AnswerFooter message={message} model={model} usage={usage} onRetry={onRetry} />
      ) : null}
    </div>
  );
}

const TOOL_STATUS_LABEL: Record<ChatToolStatus, string> = {
  pending: "waiting",
  running: "running",
  completed: "done",
  error: "failed",
};

/** Compact, calm tool-call rows. The text states the status; the dot only reinforces it. */
function ToolActivity({ tools, id }: { tools: ChatToolActivity[]; id?: string }) {
  return (
    <ul id={id} className="nx-tools" aria-label="Tool activity" data-testid="tool-activity">
      {tools.map((tool) => (
        <li
          key={tool.id}
          className={`nx-tool nx-tool--${tool.status}`}
          data-testid="tool-row"
          data-status={tool.status}
        >
          <span className="nx-tool__dot" aria-hidden="true" />
          <span className="nx-tool__name">{tool.name}</span>
          {tool.summary ? <span className="nx-tool__summary">{tool.summary}</span> : null}
          <span className="nx-tool__status">{TOOL_STATUS_LABEL[tool.status]}</span>
        </li>
      ))}
    </ul>
  );
}

function describePermission(permission: string): string {
  switch (permission) {
    case "external_directory":
      return "access a folder outside its workspace";
    case "bash":
      return "run a command";
    case "edit":
    case "write":
      return "change files";
    case "read":
      return "read files";
    case "webfetch":
      return "fetch a web page";
    default:
      return `use “${permission}”`;
  }
}

/**
 * An unmistakable inline card for a permission the engine is waiting on. The turn is
 * paused until an owner click; nothing here approves anything by itself.
 */
function PermissionCard({
  prompt,
  busy,
  locked,
  error,
  onReply,
}: {
  prompt: ChatPermissionPrompt;
  busy: boolean;
  locked: boolean;
  error: string | null;
  onReply: (decision: ChatPermissionDecision) => void;
}) {
  const titleId = useId();
  const cardRef = useRef<HTMLElement>(null);
  // Focus the card when it appears so Enter / Escape work without a click. Only steal
  // focus from the page body or the composer, never from another control the owner chose.
  useEffect(() => {
    const active = document.activeElement;
    if (!active || active === document.body || active.tagName === "TEXTAREA") {
      cardRef.current?.focus({ preventScroll: true });
    }
  }, []);
  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (locked || event.repeat || event.nativeEvent.isComposing) return;
    if (event.key === "Escape") {
      event.preventDefault();
      onReply("reject");
    } else if (event.key === "Enter") {
      if ((event.target as HTMLElement).closest("button, a[href], input, textarea, select")) {
        return;
      }
      event.preventDefault();
      onReply("once");
    }
  };
  return (
    <section
      ref={cardRef}
      tabIndex={-1}
      onKeyDown={onKeyDown}
      className="nx-perm nx-permission-card"
      aria-labelledby={titleId}
      data-testid="permission-card"
      data-permission={prompt.permission}
    >
      <span className="nx-sr" role="alert">
        Permission needed: StackSpirit wants to {describePermission(prompt.permission)}.
      </span>
      <div className="nx-perm__status" aria-hidden="true">
        <span className="nx-perm__dot" />
        Waiting for approval
      </div>
      <h2 className="nx-perm__title" id={titleId}>
        Permission needed
      </h2>
      <p className="nx-perm__what">
        The assistant wants to {describePermission(prompt.permission)}. It is paused until you
        answer.
      </p>
      <div className="nx-perm__detail">
        <span className="nx-perm__kind">{prompt.permission}</span>
        {prompt.patterns.length > 0 ? (
          <ul className="nx-perm__patterns" aria-label="Requested access">
            {prompt.patterns.map((pattern, index) => (
              <li key={index}>
                <code>{pattern}</code>
              </li>
            ))}
          </ul>
        ) : null}
      </div>
      {error ? (
        <p className="nx-perm__error" data-testid="permission-error">
          Could not send your answer: {error} Try again.
        </p>
      ) : null}
      <div className="nx-perm__actions">
        <button
          type="button"
          className="nx-btn nx-btn--quiet nx-btn--sm nx-perm__reject"
          disabled={locked}
          onClick={() => onReply("reject")}
        >
          Reject
        </button>
        <button
          type="button"
          className="nx-btn nx-btn--quiet nx-btn--sm nx-perm__always"
          disabled={locked}
          onClick={() => onReply("always")}
        >
          Always allow
        </button>
        <button
          type="button"
          className="nx-btn nx-btn--sm nx-perm__once"
          disabled={locked}
          aria-busy={busy}
          onClick={() => onReply("once")}
        >
          Allow once
        </button>
      </div>
    </section>
  );
}

function MessageCard({ session, message }: { session: SessionId; message: ChatMessageView }) {
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
