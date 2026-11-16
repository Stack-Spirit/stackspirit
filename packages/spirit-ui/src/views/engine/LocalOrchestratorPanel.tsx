import { useCallback, useEffect, useState } from "react";

import type { SpiritBridge } from "../../bridge/SpiritBridge";
import { toSpiritError } from "../../bridge/errors";
import type {
  GateSnapshot,
  LocalOrchestratorOverview,
  LocalOrchestratorView,
  LocalSurfaceView,
  SurfacePairResult,
} from "../../bridge/wire";
import { Badge, Button, EmptyState, Modal, Panel } from "../../components";
import { gateStateLabel, gateTone } from "../../domain/connection";
import {
  gateActionEnabled,
  localAgentLabel,
  localOrchestratorReason,
  localSessionStateLabel,
  localSessionStateTone,
} from "../../domain/engine";
import { formatDateTime } from "../../domain/format";

export interface LocalOrchestratorPanelProps {
  bridge: SpiritBridge;
  gates: GateSnapshot | null;
  live: boolean;
  /** Increments when the gate card's enabled "Run locally" action is pressed. */
  createSignal: number;
}

type OrchestratorApp = "opencode" | "claude_code" | "chat";

/**
 * The D6 Engine panel: local orchestrators, surfaces and the Mac path map.
 *
 * Read-only state stays visible while the gate is `Waiting` so the owner can see the
 * exact prerequisite that is missing. Start/stop/resume/rotate/pair actions enable only
 * when `gateActionEnabled` is true; the backend re-checks the gate, the connected agent
 * and the build flag on every call, so this panel is never the authority.
 */
export function LocalOrchestratorPanel({
  bridge,
  gates,
  live,
  createSignal,
}: LocalOrchestratorPanelProps) {
  const local = bridge.localOrchestrators;
  const enabled = gates ? gateActionEnabled(gates, "local_orchestrator") : false;
  const gateNode =
    gates?.local_orchestrator.state === "ready" ? gates.local_orchestrator.node : null;
  const reason = localOrchestratorReason(gates);

  const [overview, setOverview] = useState<LocalOrchestratorOverview | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(createSignal > 0);
  const [pairOpen, setPairOpen] = useState(false);
  const [createNode, setCreateNode] = useState(gateNode ?? "");
  const [createApp, setCreateApp] = useState<OrchestratorApp>("opencode");
  const [createProject, setCreateProject] = useState("");
  const [createName, setCreateName] = useState("");
  const [createForce, setCreateForce] = useState(false);
  const [pairSession, setPairSession] = useState("");
  const [pairNode, setPairNode] = useState(gateNode ?? "");
  const [pairKind, setPairKind] = useState<"opencode" | "chat">("opencode");
  const [pairApp, setPairApp] = useState("opencode");
  const [pairing, setPairing] = useState<SurfacePairResult | null>(null);

  const refresh = useCallback(async () => {
    if (!local || !live) return;
    setLoading(true);
    try {
      setOverview(await local.overview());
      setError(null);
    } catch (readError) {
      setError(toSpiritError(readError).message);
    } finally {
      setLoading(false);
    }
  }, [live, local]);

  useEffect(() => {
    if (!live) return;
    const handle = window.setTimeout(() => {
      void refresh();
    }, 0);
    return () => window.clearTimeout(handle);
  }, [live, refresh, gates?.probed_at]);

  const act = useCallback(
    async (action: () => Promise<unknown>): Promise<boolean> => {
      setBusy(true);
      setError(null);
      try {
        await action();
        await refresh();
        return true;
      } catch (actionError) {
        setError(toSpiritError(actionError).message);
        return false;
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

  const openCreate = () => {
    setCreateNode(gateNode ?? "");
    setCreateApp("opencode");
    setCreateProject("");
    setCreateName("");
    setCreateForce(false);
    setPairing(null);
    setCreateOpen(true);
  };

  const openPair = () => {
    setPairSession("");
    setPairNode(gateNode ?? "");
    setPairKind("opencode");
    setPairApp("opencode");
    setPairing(null);
    setPairOpen(true);
  };

  const submitCreate = async () => {
    if (!local) return;
    const created = await act(() =>
      local.orchestratorNew({
        node: createNode.trim(),
        app: createApp,
        project: createProject.trim() || null,
        name: createName.trim() || null,
        force: createForce,
      }),
    );
    if (created) {
      setCreateOpen(false);
    }
  };

  const submitPair = async () => {
    if (!local) return;
    setBusy(true);
    setError(null);
    try {
      const result = await local.surfacePair({
        session: pairSession.trim(),
        node: pairNode.trim(),
        kind: pairKind,
        app: pairApp.trim() || null,
      });
      setPairing(result);
      await refresh();
    } catch (pairError) {
      setError(toSpiritError(pairError).message);
    } finally {
      setBusy(false);
    }
  };

  const pairThisMac = async () => {
    if (!local || !pairing) return;
    const paired = await act(() =>
      local.agentPair({
        code: pairing.code,
        surface: pairing.pairing.surface,
        session: pairing.pairing.session,
        app: pairing.pairing.app,
      }),
    );
    if (paired) {
      setPairing(null);
      setPairOpen(false);
    }
  };

  const stop = (row: LocalOrchestratorView) => {
    if (!local) return;
    void act(() => local.orchestratorStop(row.name));
  };

  const resume = (row: LocalOrchestratorView) => {
    if (!local) return;
    void act(() => local.orchestratorResume(row.name, row.project, row.node));
  };

  const rotate = (row: LocalSurfaceView) => {
    if (!local) return;
    void act(() => local.surfaceRotate({ surface: row.surface_id, node: row.node }));
  };

  const revoke = (row: LocalSurfaceView) => {
    if (!local) return;
    void act(() => local.surfaceRevoke(row.surface_id));
  };

  const actionsDisabled = !enabled || !local || busy;
  const cleanupDisabled = !local || busy || !live;

  return (
    <Panel
      title="Local orchestrators"
      actions={
        <Button
          variant="ghost"
          size="sm"
          data-testid="local-orchestrator-refresh"
          onClick={() => void refresh()}
          disabled={!local || !live || loading}
        >
          Refresh
        </Button>
      }
    >
      <div className="local-orchestrators" data-testid="local-orchestrator-panel">
        <div className="engine-actions">
          <Badge tone={gates ? gateTone(gates.local_orchestrator.state) : "unknown"}>
            {gates ? gateStateLabel(gates.local_orchestrator.state) : "Not probed"}
          </Badge>
          <Badge tone={gates?.local_agent.state === "connected" ? "ok" : "warn"}>
            {gates ? localAgentLabel(gates.local_agent) : "Local agent unknown"}
          </Badge>
          <span className="muted small">
            Node{" "}
            <span className="mono">{gateNode ?? overview?.node?.name ?? "unknown"}</span>
          </span>
          <span className="muted small">
            Protocol{" "}
            <span className="mono">
              {gates?.local_orchestrator.state === "ready"
                ? gates.local_orchestrator.agent_protocol
                : (overview?.node?.protocol ?? "unknown")}
            </span>
          </span>
          {overview?.node?.agent_version ? (
            <span className="muted small">
              Agent version <span className="mono">{overview.node.agent_version}</span>
            </span>
          ) : null}
        </div>

        {!enabled ? (
          <p className="gate-reason" data-testid="local-orchestrator-reason">
            {reason}
          </p>
        ) : (
          <p className="muted small">
            VOWP is ready and the local agent is connected. Every action re-checks the
            gate, the agent and the path map on the backend.
          </p>
        )}

        {!local ? (
          <p className="muted small" data-testid="local-orchestrator-unavailable">
            Local orchestrator actions are not available in this bridge. This view stays
            read-only.
          </p>
        ) : null}

        <div className="gate-actions">
          <Button
            variant="secondary"
            size="sm"
            data-testid="local-orchestrator-create"
            disabled={actionsDisabled}
            title={enabled ? undefined : reason}
            onClick={openCreate}
          >
            Create orchestrator
          </Button>
          <Button
            variant="secondary"
            size="sm"
            data-testid="local-orchestrator-pair"
            disabled={actionsDisabled}
            title={enabled ? undefined : reason}
            onClick={openPair}
          >
            Pair surface
          </Button>
        </div>

        {error ? (
          <p className="notice notice--danger" role="alert" data-testid="local-orchestrator-error">
            {error}
          </p>
        ) : null}

        {!live ? (
          <p className="muted small">
            Connect to a VOWP home to read local surfaces and orchestrators.
          </p>
        ) : null}

        {loading && !overview ? <p className="muted small">Reading local state…</p> : null}

        {overview && live && local ? (
          <>
            <section className="local-orchestrators__section">
              <h3 className="panel__title">Mac path map</h3>
              {overview.paths_error ? (
                <p className="muted small" data-testid="local-paths-error">
                  {overview.paths_error}
                </p>
              ) : null}
              {overview.paths.length === 0 ? (
                <p className="muted small">
                  No project path map is configured on this Mac. Map one with{" "}
                  <span className="mono">vowp agent paths add</span> before creating a
                  project-bound orchestrator.
                </p>
              ) : (
                <ul className="local-orchestrators__list" aria-label="Mac path maps">
                  {overview.paths.map((path) => (
                    <li key={path.project} className="local-orchestrators__row">
                      <span className="mono">{path.path_ref}</span>
                      <Badge tone={path.valid ? "ok" : "danger"}>
                        {path.valid ? "Valid" : "Invalid"}
                      </Badge>
                      <span className="muted small">
                        {path.worktree_root_configured
                          ? "Worktree root configured"
                          : "Project root only"}
                        {path.allow.length > 0 ? ` · allow ${path.allow.join(", ")}` : ""}
                      </span>
                      {path.invalid_reason ? (
                        <span className="muted small">{path.invalid_reason}</span>
                      ) : null}
                    </li>
                  ))}
                </ul>
              )}
            </section>

            <section className="local-orchestrators__section">
              <h3 className="panel__title">Orchestrators</h3>
              {overview.orchestrators.length === 0 ? (
                <EmptyState
                  icon="◇"
                  title="No local orchestrators"
                  body="Create one when VOWP is ready and the local agent is connected."
                />
              ) : (
                <ul className="local-orchestrators__list" aria-label="Local orchestrators">
                  {overview.orchestrators.map((row) => (
                    <li
                      key={row.name}
                      className="local-orchestrators__row"
                      data-testid={`local-orchestrator-row-${row.name}`}
                    >
                      <div className="local-orchestrators__row-head">
                        <span className="mono">{row.name}</span>
                        <Badge tone={localSessionStateTone(row.session_state)}>
                          {localSessionStateLabel(row.session_state)}
                        </Badge>
                        <Badge tone={row.state === "running" ? "ok" : "unknown"}>
                          {row.state}
                        </Badge>
                      </div>
                      <div className="local-orchestrators__meta">
                        <span>
                          Node <span className="mono">{row.node ?? "unknown"}</span>
                        </span>
                        <span>
                          Protocol <span className="mono">{row.protocol ?? "unknown"}</span>
                        </span>
                        <span>
                          Agent <span className="mono">{row.agent_status}</span>
                        </span>
                        <span>
                          Lease{" "}
                          <span className="mono">{row.lease_holder ?? "none"}</span>
                        </span>
                      </div>
                      <div className="gate-actions">
                        <Button
                          variant="ghost"
                          size="sm"
                          data-testid={`local-orchestrator-stop-${row.name}`}
                          disabled={cleanupDisabled}
                          onClick={() => stop(row)}
                        >
                          Stop
                        </Button>
                        <Button
                          variant="secondary"
                          size="sm"
                          data-testid={`local-orchestrator-resume-${row.name}`}
                          disabled={actionsDisabled}
                          title={enabled ? undefined : reason}
                          onClick={() => resume(row)}
                        >
                          Resume
                        </Button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </section>

            <section className="local-orchestrators__section">
              <h3 className="panel__title">Surfaces</h3>
              {overview.surfaces.length === 0 ? (
                <p className="muted small">No home surface is paired with this node.</p>
              ) : (
                <ul className="local-orchestrators__list" aria-label="Local surfaces">
                  {overview.surfaces.map((row) => (
                    <li
                      key={row.surface_id}
                      className="local-orchestrators__row"
                      data-testid={`local-surface-row-${row.surface_id}`}
                    >
                      <div className="local-orchestrators__row-head">
                        <span className="mono">{row.surface_id}</span>
                        <Badge tone={localSessionStateTone(row.session_state)}>
                          {localSessionStateLabel(row.session_state)}
                        </Badge>
                        <Badge tone={row.can_write ? "ok" : "unknown"}>
                          {row.can_write ? "Writable" : "Read-only"}
                        </Badge>
                        {row.rotation_pending ? <Badge tone="warn">Rotation queued</Badge> : null}
                        {row.expired ? <Badge tone="danger">Expired</Badge> : null}
                      </div>
                      <div className="local-orchestrators__meta">
                        <span>
                          Node <span className="mono">{row.node}</span>
                        </span>
                        <span>
                          Protocol <span className="mono">{row.protocol ?? "unknown"}</span>
                        </span>
                        <span>
                          Agent <span className="mono">{row.agent_status}</span>
                        </span>
                        <span>
                          Lease{" "}
                          <span className="mono">{row.lease_holder ?? "none"}</span>
                        </span>
                        <span>Expires {formatDateTime(row.expires_at)}</span>
                      </div>
                      <div className="gate-actions">
                        <Button
                          variant="secondary"
                          size="sm"
                          data-testid={`local-surface-rotate-${row.surface_id}`}
                          disabled={
                            actionsDisabled ||
                            row.state !== "attached" ||
                            row.expired ||
                            row.rotation_pending
                          }
                          title={
                            enabled
                              ? row.state !== "attached" || row.expired || row.rotation_pending
                                ? "Only an attached, unexpired surface without a queued rotation can rotate."
                                : undefined
                              : reason
                          }
                          onClick={() => rotate(row)}
                        >
                          Rotate token
                        </Button>
                        <Button
                          variant="danger"
                          size="sm"
                          data-testid={`local-surface-revoke-${row.surface_id}`}
                          disabled={cleanupDisabled}
                          onClick={() => revoke(row)}
                        >
                          Revoke
                        </Button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          </>
        ) : null}
      </div>

      <Modal
        open={createOpen}
        title="Create local orchestrator"
        onClose={() => setCreateOpen(false)}
        footer={
          <>
            <Button variant="ghost" size="sm" onClick={() => setCreateOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              size="sm"
              data-testid="local-orchestrator-create-submit"
              disabled={!enabled || busy || !createNode.trim()}
              onClick={() => void submitCreate()}
            >
              Create orchestrator
            </Button>
          </>
        }
      >
        <div className="local-orchestrators__form">
          <label className="field">
            <span>Node</span>
            <input
              value={createNode}
              onChange={(event) => setCreateNode(event.target.value)}
              aria-label="Node"
              autoComplete="off"
            />
          </label>
          <label className="field">
            <span>App</span>
            <select
              value={createApp}
              onChange={(event) => setCreateApp(event.target.value as OrchestratorApp)}
              aria-label="App"
            >
              <option value="opencode">opencode</option>
              <option value="claude_code">Claude Code</option>
              <option value="chat">chat</option>
            </select>
          </label>
          <label className="field">
            <span>Project (optional)</span>
            <input
              value={createProject}
              onChange={(event) => setCreateProject(event.target.value)}
              aria-label="Project"
              autoComplete="off"
            />
          </label>
          <label className="field">
            <span>Name (optional)</span>
            <input
              value={createName}
              onChange={(event) => setCreateName(event.target.value)}
              aria-label="Name"
              autoComplete="off"
            />
          </label>
          <label className="local-orchestrators__check">
            <input
              type="checkbox"
              checked={createForce}
              onChange={(event) => setCreateForce(event.target.checked)}
            />
            <span>Replace an orchestrator with the same name</span>
          </label>
          <p className="muted small">
            The backend re-checks the gate, the connected agent, the validated node and the
            project path map before VOWP materializes anything.
          </p>
        </div>
      </Modal>

      <Modal
        open={pairOpen}
        title="Pair a local surface"
        onClose={() => setPairOpen(false)}
        footer={
          <>
            <Button variant="ghost" size="sm" onClick={() => setPairOpen(false)}>
              Close
            </Button>
            <Button
              variant="primary"
              size="sm"
              data-testid="local-surface-pair-submit"
              disabled={!enabled || busy || !pairSession.trim() || !pairNode.trim()}
              onClick={() => void submitPair()}
            >
              Create pairing
            </Button>
          </>
        }
      >
        <div className="local-orchestrators__form">
          <label className="field">
            <span>Session id</span>
            <input
              value={pairSession}
              onChange={(event) => setPairSession(event.target.value)}
              aria-label="Session id"
              autoComplete="off"
            />
          </label>
          <label className="field">
            <span>Node</span>
            <input
              value={pairNode}
              onChange={(event) => setPairNode(event.target.value)}
              aria-label="Pair node"
              autoComplete="off"
            />
          </label>
          <label className="field">
            <span>Kind</span>
            <select
              value={pairKind}
              onChange={(event) => setPairKind(event.target.value as "opencode" | "chat")}
              aria-label="Kind"
            >
              <option value="opencode">opencode</option>
              <option value="chat">chat</option>
            </select>
          </label>
          <label className="field">
            <span>App</span>
            <input
              value={pairApp}
              onChange={(event) => setPairApp(event.target.value)}
              aria-label="Pair app"
              autoComplete="off"
            />
          </label>

          {pairing ? (
            <div className="local-orchestrators__pairing" data-testid="local-pairing-result">
              <p>
                Surface <span className="mono">{pairing.pairing.surface}</span> is pending.
                Copy the one-time code to the Mac agent; it expires{" "}
                {formatDateTime(pairing.pairing.expires_at)}.
              </p>
              <div className="gate-evidence">
                <span data-testid="local-pairing-code">{pairing.code}</span>
                <span data-testid="local-pairing-command">{pairing.pairing.command}</span>
              </div>
              <Button
                variant="secondary"
                size="sm"
                data-testid="local-pairing-agent"
                disabled={busy || !enabled}
                onClick={() => void pairThisMac()}
              >
                Pair this Mac
              </Button>
              <p className="muted small">
                The code is shown once for this pairing. StackSpirit never logs or stores
                it.
              </p>
            </div>
          ) : (
            <p className="muted small">
              VOWP prints the pairing code once. The Mac agent redeems it locally with
              <span className="mono"> vowp agent pair --code-file -</span>.
            </p>
          )}
        </div>
      </Modal>
    </Panel>
  );
}
