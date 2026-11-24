import { useCallback, useEffect, useRef, useState } from "react";

import { toSpiritError } from "../bridge/errors";
import type { SessionEventView, SessionTakeoverResult } from "../bridge/wire";
import { executorProviderLabel, harnessLabel } from "../domain/executor";
import { useFleet, useFleetStore } from "../state/fleetStore";
import { PageHeader } from "./PageHeader";
import { findJob } from "./summary";

/** Poll interval while the worker is still running. */
const WS_CSS = `/* Worker session viewer: compact read-only event timeline. */
.nx-ws__confirm {
  margin-bottom: 12px;
}

.nx-ws__list {
  display: grid;
  gap: 2px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.nx-ws__event {
  display: grid;
  grid-template-columns: 72px 1fr;
  gap: 10px;
  padding: 6px 8px;
  border-left: 3px solid var(--nx-line, currentColor);
  font-size: 13px;
  color: var(--nx-text);
}

.nx-ws__event--assistant {
  border-left-color: var(--nx-local);
}

.nx-ws__event--tool {
  border-left-color: var(--nx-warn);
}

.nx-ws__event--error {
  border-left-color: var(--nx-danger);
}

.nx-ws__time {
  font-variant-numeric: tabular-nums;
  color: var(--nx-text);
  opacity: 0.8;
}

.nx-ws__detail {
  margin: 4px 0 0;
  max-height: 220px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
  font-size: 12px;
}`;

export const SESSION_POLL_MS = 3000;
const MAX_EVENTS = 1000;

export interface WorkerSessionViewProps {
  project: string;
  jobId: string;
  sessionId: string;
  onBack: () => void;
}

function eventTone(event: SessionEventView): string {
  if (event.kind.startsWith("tool.")) return "tool";
  if (event.kind === "assistant.message") return "assistant";
  if (event.kind === "error") return "error";
  if (event.kind === "user.message") return "user";
  return "system";
}

function clock(at: string): string {
  const match = /T(\d{2}:\d{2}:\d{2})/.exec(at);
  return match?.[1] ?? at;
}

/**
 * Read-only live view of what a worker is doing. It only ever calls `sessionTail`, so it
 * cannot corrupt the run; the one write is the explicit, confirmed `Stop & Take over`.
 */
export function WorkerSessionView({ project, jobId, sessionId, onBack }: WorkerSessionViewProps) {
  const bridge = useFleetStore((state) => state.bridge);
  const fleet = useFleet();
  const found = findJob(fleet, project, jobId);
  const running = fleet?.running.find((card) => card.project === project && card.job_id === jobId);
  const executor = running?.executor ?? found?.job.executor ?? null;
  const settledByFleet = !running;

  const [events, setEvents] = useState<SessionEventView[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [tookOver, setTookOver] = useState<SessionTakeoverResult | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const cursor = useRef<number | null>(null);
  const inFlight = useRef(false);

  const live = !settledByFleet && !tookOver;
  const tail = bridge?.fleet.sessionTail;

  const poll = useCallback(async () => {
    if (!tail || inFlight.current) return;
    inFlight.current = true;
    try {
      const page = await tail(project, sessionId, cursor.current);
      cursor.current = page.next_seq;
      setError(null);
      setLoaded(true);
      if (page.events.length > 0) {
        setEvents((current) => [...current, ...page.events].slice(-MAX_EVENTS));
      }
    } catch (caught) {
      setError(toSpiritError(caught).message);
      setLoaded(true);
    } finally {
      inFlight.current = false;
    }
  }, [tail, project, sessionId]);

  // The shell keys this view by session id, so state starts fresh for each session.
  useEffect(() => {
    const first = window.setTimeout(() => void poll(), 0);
    return () => window.clearTimeout(first);
  }, [poll]);

  useEffect(() => {
    if (!live) return;
    const timer = window.setInterval(() => void poll(), SESSION_POLL_MS);
    return () => window.clearInterval(timer);
  }, [live, poll]);

  const takeover = async () => {
    if (!bridge?.sessions?.takeoverSession || busy) return;
    setBusy(true);
    try {
      setTookOver(await bridge.sessions.takeoverSession(sessionId));
      setError(null);
    } catch (caught) {
      setError(toSpiritError(caught).message);
    } finally {
      setBusy(false);
      setConfirming(false);
    }
  };

  const badge = executor
    ? [
        executor.provider ? executorProviderLabel(executor.provider) : null,
        harnessLabel(executor.harness),
        executor.display,
      ]
        .filter(Boolean)
        .join(" · ")
    : "Unknown executor";

  return (
    <section className="nx-main" aria-label={`Session for ${jobId}`} data-testid="worker-session">
      <style>{WS_CSS}</style>
      <PageHeader
        title={jobId}
        sub={project}
        actions={
          <>
            <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={onBack}>
              ← Back to workers
            </button>
            {bridge?.sessions?.takeoverSession ? (
              <button
                type="button"
                className="nx-btn nx-btn--sm"
                onClick={() => setConfirming(true)}
                disabled={busy || tookOver !== null}
              >
                Stop &amp; Take over
              </button>
            ) : null}
          </>
        }
      >
        <span className="nx-chip" data-testid="session-badge">
          {badge}
        </span>
        <span
          className={`nx-chip nx-chip--tone ${live ? "nx-tone-chip--ok" : ""}`}
          role="status"
          data-testid="session-status"
        >
          {tookOver ? "Taken over" : live ? "Live · polling" : "Settled"}
        </span>
      </PageHeader>
      <div className="nx-page">
        <div className="nx-page__inner">
          {confirming ? (
            <div className="nx-owner__confirm nx-ws__confirm" role="alertdialog" aria-label="Confirm take over">
              <p>
                This pauses the autonomous worker, saves a handoff capsule, fences its writes, and hands the
                session to you. Take over?
              </p>
              <div className="nx-owner__actions">
                <button type="button" className="nx-btn nx-btn--sm" onClick={() => void takeover()} disabled={busy}>
                  {busy ? "Taking over…" : "Take over"}
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
          {tookOver ? (
            <div className="nx-secure__note" role="status" data-testid="takeover-result">
              <span>
                <strong>You now own this session.</strong>{" "}
                {tookOver.previous ? `Previous owner: ${tookOver.previous}. ` : ""}Next: open the orchestrator chat
                to steer it, or return to workers. The worker will not write again until ownership is released.
              </span>
            </div>
          ) : null}
          {error ? (
            <p className="nx-owner__error" role="alert" data-testid="session-error">
              {error}
            </p>
          ) : null}
          {!tail ? (
            <p className="nx-panel__empty">This connection cannot read worker sessions.</p>
          ) : !loaded ? (
            <p className="nx-panel__empty">Loading session…</p>
          ) : events.length === 0 && !error ? (
            <p className="nx-panel__empty">No events yet.</p>
          ) : (
            <ol className="nx-ws__list" aria-label="Session events" data-testid="session-events">
              {events.map((event) => (
                <li key={event.seq} className={`nx-ws__event nx-ws__event--${eventTone(event)}`}>
                  <span className="nx-ws__time">{clock(event.at)}</span>
                  <span className="nx-ws__body">
                    <span className="nx-ws__summary">{event.summary}</span>
                    {event.detail && event.detail !== event.summary ? (
                      <details>
                        <summary>Details</summary>
                        <pre className="nx-ws__detail">{event.detail}</pre>
                      </details>
                    ) : null}
                  </span>
                </li>
              ))}
            </ol>
          )}
        </div>
      </div>
    </section>
  );
}
