import { useState } from "react";

import { toSpiritError } from "../bridge/errors";
import type { FleetView, SessionOwnerView } from "../bridge/wire";
import { useFleetStore } from "../state/fleetStore";
import type { SessionId } from "./conversations";

/** The holder name StackSpirit itself uses when it takes a session over. */
const LOCAL_HOLDER = "stackspirit";

/**
 * The structured owner of this orchestrator's VOWP session when another surface holds
 * it. Returns null when there is no owner, the owner is StackSpirit, or the session
 * cannot be resolved from the last snapshot.
 */
export function foreignOwner(
  fleet: FleetView | null,
  session: SessionId,
): { sessionId: string; owner: SessionOwnerView } | null {
  if (!fleet || session === "setup") return null;
  const project = session.startsWith("project:")
    ? session.slice("project:".length)
    : null;
  const orchestrator = fleet.orchestrators.find(
    (row) => (row.project ?? null) === project,
  );
  if (!orchestrator) return null;
  const card = fleet.sessions.find((row) => row.id === orchestrator.session_id);
  const owner = card?.owner;
  if (!card || !owner) return null;
  if (owner.holder.trim().toLowerCase() === LOCAL_HOLDER) return null;
  return { sessionId: card.id, owner };
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
export function OwnershipBanner({ session }: { session: SessionId }) {
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
  const canAct = Boolean(takeover) && !fleet?.stale && !unsupported;
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
          "Take over isn't available with this VOWP yet. This session stays read-only.",
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
        {!confirming ? (
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
