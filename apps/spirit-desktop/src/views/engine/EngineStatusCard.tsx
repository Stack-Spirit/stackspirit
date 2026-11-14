import { Badge, Button, Panel } from "../../components";
import type { EngineStatus } from "../../bridge/wire";
import { engineLabel, engineTone } from "../../domain/connection";
import { formatClock } from "../../domain/format";
import { selectedModelLabel } from "../../domain/engine";

export interface EngineStatusCardProps {
  status: EngineStatus;
  busy?: boolean;
  onRestart: () => void;
  onChangeModel: () => void;
  onUseBundled: () => void;
  onDetails: () => void;
  advancedOpen: boolean;
  onToggleAdvanced: () => void;
}

export function EngineStatusCard({
  status,
  busy = false,
  onRestart,
  onChangeModel,
  onUseBundled,
  onDetails,
  advancedOpen,
  onToggleAdvanced,
}: EngineStatusCardProps) {
  const external = status.choice.type === "external";
  const stateDetail =
    status.state.state === "degraded" || status.state.state === "failed"
      ? status.state.detail
      : null;

  return (
    <Panel
      title="StackSpirit Engine"
      data-testid="engine-status-card"
      actions={
        <div className="engine-actions">
          <Badge tone={engineTone(status)}>{engineLabel(status)}</Badge>
          {external ? <Badge tone="info">External</Badge> : null}
        </div>
      }
    >
      <div className="engine-card">
        <div className="engine-status-grid">
          <div>
            <span className="muted small">Version</span>
            <p className="mono">{status.version ?? "unknown"}</p>
          </div>
          <div>
            <span className="muted small">Loopback port</span>
            <p className="mono">{status.port ?? "—"}</p>
          </div>
          <div>
            <span className="muted small">Started</span>
            <p>{status.started_at ? formatClock(status.started_at) : "—"}</p>
          </div>
          <div>
            <span className="muted small">Selected model</span>
            <p>{selectedModelLabel(status)}</p>
          </div>
        </div>

        {status.choice.type === "external" ? (
          <p className="muted small">
            Using an existing{" "}
            {status.choice.kind === "claude-code" ? "Claude Code" : "harness"} installation
            at <span className="mono">{status.choice.path}</span>. Its login and model access
            stay with that installation.
          </p>
        ) : null}

        {stateDetail ? <p className="notice notice--danger">{stateDetail}</p> : null}
        {status.last_error && status.last_error !== stateDetail ? (
          <p className="notice notice--danger">{status.last_error}</p>
        ) : null}

        <div className="engine-actions">
          <Button
            variant="secondary"
            size="sm"
            onClick={onRestart}
            disabled={busy || external}
            title={
              external
                ? "The external harness starts from its own installation."
                : "Restart the StackSpirit Engine."
            }
          >
            Restart engine
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={onChangeModel}
            disabled={busy || external}
          >
            Change model
          </Button>
          {external ? (
            <Button variant="primary" size="sm" onClick={onUseBundled} disabled={busy}>
              Use bundled engine
            </Button>
          ) : null}
          <Button variant="ghost" size="sm" onClick={onDetails} disabled={busy}>
            Engine details
          </Button>
        </div>
        {external ? (
          <p className="gate-reason">
            The external harness is not supervised by StackSpirit; Restart and model
            selection belong to that installation.
          </p>
        ) : null}

        <p className="muted small">
          Engine runs on this Mac only. Your VPS never receives a provider key.
        </p>

        <div className="engine-advanced">
          <Button variant="ghost" size="sm" onClick={onToggleAdvanced}>
            {advancedOpen ? "Hide advanced details" : "Advanced details"}
          </Button>
          {advancedOpen ? (
            <div className="engine-advanced__body">
              <span>
                Engine implementation: opencode {status.version ?? "unknown version"}
              </span>
              <span className="mono">{status.binary_path ?? "binary path unavailable"}</span>
              <span>StackSpirit Engine runs on opencode (MIT); see THIRD_PARTY_NOTICES.md.</span>
              <span>Capability gate actions remain disabled until their VOWP phase ships.</span>
            </div>
          ) : null}
        </div>
      </div>
    </Panel>
  );
}
