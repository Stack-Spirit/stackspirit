import { useState } from "react";

import type { SessionCardView } from "../../bridge/wire";
import { Badge, Button, Drawer, EmptyState } from "../../components";
import { executorLabel } from "../../domain/executor";
import { formatDateTime } from "../../domain/format";
import {
  asifActionAvailability,
  asifGateBadge,
  asifGateDetail,
  asifGateMissing,
  sessionStateLabel,
  sessionStateTone,
  sessionUpdated,
} from "../../domain/session";
import {
  useConnection,
  useFleet,
  useFleetStore,
  useGates,
  useSessions,
} from "../../state/fleetStore";
import { ExportPanel } from "./ExportPanel";
import { ImportPanel } from "./ImportPanel";
import "./sessions.css";

type SessionFlow =
  | { kind: "list" }
  | { kind: "export"; session: SessionCardView }
  | { kind: "import" };

export interface SessionsDrawerProps {
  open: boolean;
  onClose: () => void;
}

function flowTitle(flow: SessionFlow): string {
  if (flow.kind === "export") return "Export .asif.zip";
  if (flow.kind === "import") return "Import .asif.zip";
  return "Sessions";
}

/**
 * The read-only sessions list and the two D5 ASIF flows.
 *
 * Every action is disabled unless the connection is live, the probe proves that
 * direction, and the build feature flag is on. Disabled actions carry the exact reason
 * in text beside them, never only in a tooltip.
 */
export function SessionsDrawer({ open, onClose }: SessionsDrawerProps) {
  const bridge = useFleetStore((state) => state.bridge);
  const probeGates = useFleetStore((state) => state.probeGates);
  const fleet = useFleet();
  const sessions = useSessions();
  const connection = useConnection();
  const gates = useGates();
  const intent = useFleetStore((state) => state.sessionsIntent);
  // AppShell keys this component on `open` and `intent`, so the initializer is the one
  // place that decides which panel a fresh open starts on.
  const [flow, setFlow] = useState<SessionFlow>(() =>
    intent === "import" ? { kind: "import" } : { kind: "list" },
  );

  const bridgeAvailable = Boolean(bridge?.sessions);
  const importAvailability = asifActionAvailability({
    gates,
    action: "asif_import",
    connectionState: connection.state,
    bridgeAvailable,
  });
  const exportAvailability = asifActionAvailability({
    gates,
    action: "asif_export",
    connectionState: connection.state,
    bridgeAvailable,
  });
  const badge = asifGateBadge(gates);
  const missing = asifGateMissing(gates);
  const liveConnection = connection.state === "connected" && Boolean(connection.host_id);
  const probeReason = liveConnection
    ? null
    : connection.state === "stale"
      ? "The VOWP connection is stale. Reconnect before probing capability gates."
      : "Connect to a VOWP home before probing capability gates.";
  return (
    <Drawer open={open} title={flowTitle(flow)} onClose={onClose}>
      <div className="sessions-drawer" data-testid="sessions-drawer">
        {flow.kind === "list" ? (
          <>
            <div className="sessions-gate" data-testid="sessions-gate">
              <div className="sessions-gate__row">
                <span className="muted small">ASIF gate</span>
                <Badge tone={badge.tone}>{badge.label}</Badge>
                <span className="muted small">{asifGateDetail(gates)}</span>
              </div>
              {missing.length > 0 ? (
                <p className="muted small">
                  Missing: <span className="mono">{missing.join(" · ")}</span>
                </p>
              ) : null}
              <div className="sessions-gate__row">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => void probeGates()}
                  disabled={probeReason !== null}
                  title={probeReason ?? undefined}
                >
                  Re-probe
                </Button>
                {probeReason ? <span className="gate-reason">{probeReason}</span> : null}
              </div>
            </div>

            {connection.state === "disconnected" ? (
              <p className="notice notice--warn" role="status" data-testid="sessions-disconnected">
                Not connected to a VOWP home. The last snapshot is shown read-only and
                ASIF actions are disabled until you connect.
              </p>
            ) : null}
            {connection.state === "stale" ? (
              <p className="notice notice--warn" role="status" data-testid="sessions-stale">
                The VOWP connection is stale. ASIF actions are disabled until a fresh
                poll, reconnect, or refresh succeeds.
              </p>
            ) : null}
            {connection.state === "connected" && fleet?.stale ? (
              <p className="notice notice--warn" role="status" data-testid="sessions-stale">
                This snapshot came from {fleet.source} and is marked stale. ASIF actions
                are disabled until a fresh poll succeeds.
              </p>
            ) : null}

            <div className="sessions-toolbar">
              <div>
                <strong>
                  {sessions.length} session{sessions.length === 1 ? "" : "s"}
                </strong>
                <p className="muted small">
                  {fleet
                    ? `Snapshot ${formatDateTime(fleet.fetched_at)} · source ${fleet.source}`
                    : "No snapshot yet."}
                </p>
              </div>
              <div className="sessions-toolbar__actions">
                <Button
                  variant="primary"
                  size="sm"
                  data-testid="sessions-import"
                  onClick={() => setFlow({ kind: "import" })}
                  disabled={!importAvailability.enabled}
                  title={importAvailability.enabled ? undefined : importAvailability.reason}
                >
                  Import .asif.zip
                </Button>
                {!importAvailability.enabled ? (
                  <span className="gate-reason" data-testid="sessions-import-reason">
                    {importAvailability.reason}
                  </span>
                ) : null}
              </div>
            </div>

            {sessions.length === 0 ? (
              <EmptyState
                icon="▤"
                title={
                  connection.state === "disconnected"
                    ? "No session snapshot"
                    : "No sessions in the last snapshot"
                }
                body="VOWP returns the sessions it owns. StackSpirit never creates or starts a session from this drawer."
              />
            ) : (
              <ul className="session-list" aria-label="Sessions">
                {sessions.map((session) => {
                  const updated = sessionUpdated(session, fleet?.fetched_at);
                  return (
                    <li
                      key={session.id}
                      className="session-list__item"
                      data-testid={`session-row-${session.id}`}
                    >
                      <div className="session-list__head">
                        <strong>{session.title}</strong>
                        <Badge tone={sessionStateTone(session.state)}>
                          {sessionStateLabel(session.state)}
                        </Badge>
                      </div>
                      <dl className="session-meta">
                        <div>
                          <dt>ID</dt>
                          <dd className="mono" data-testid={`session-id-${session.id}`}>
                            {session.id}
                          </dd>
                        </div>
                        <div>
                          <dt>Project</dt>
                          <dd>{session.project ?? "No project"}</dd>
                        </div>
                        <div>
                          <dt>Executor</dt>
                          <dd>{executorLabel(session.executor)}</dd>
                        </div>
                        <div>
                          <dt>Updated</dt>
                          <dd title={updated.absolute}>{updated.relative}</dd>
                        </div>
                      </dl>
                      <div className="session-list__actions">
                        <Button
                          variant="secondary"
                          size="sm"
                          data-testid={`session-export-${session.id}`}
                          onClick={() => setFlow({ kind: "export", session })}
                          disabled={!exportAvailability.enabled}
                          title={
                            exportAvailability.enabled
                              ? undefined
                              : exportAvailability.reason
                          }
                        >
                          Export .asif.zip
                        </Button>
                        {!exportAvailability.enabled ? (
                          <span
                            className="gate-reason"
                            data-testid={`session-export-reason-${session.id}`}
                          >
                            {exportAvailability.reason}
                          </span>
                        ) : null}
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}

            <p className="muted small">
              Sessions are read-only here. An imported package becomes an idle session and
              is never run automatically.
            </p>
          </>
        ) : null}

        {flow.kind === "export" ? (
          <ExportPanel
            key={flow.session.id}
            bridge={bridge}
            session={flow.session}
            snapshotAt={fleet?.fetched_at ?? null}
            onBack={() => setFlow({ kind: "list" })}
          />
        ) : null}

        {flow.kind === "import" ? (
          <ImportPanel bridge={bridge} onBack={() => setFlow({ kind: "list" })} />
        ) : null}
      </div>
    </Drawer>
  );
}
