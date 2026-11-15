import { Badge, Button, Sparkline, Tooltip } from "../components";
import { formatAge, formatClock, latencyLabel } from "../domain/format";
import {
  connectionLabel,
  connectionTone,
  engineLabel,
  engineTone,
  gateStateLabel,
  gateTone,
  supervisorLabel,
  supervisorTone,
} from "../domain/connection";
import {
  useConnection,
  useEngine,
  useFleet,
  useFleetStore,
  useGates,
  useHosts,
  usePreview,
  useProjectFilter,
  useSelectedHostId,
} from "../state/fleetStore";

export type AppView = "fleet" | "engine" | "hosts";

export interface TopBarProps {
  onNavigate: (view: AppView) => void;
  onOpenInbox: () => void;
  onOpenCapacity: () => void;
}

export function TopBar({ onNavigate, onOpenInbox, onOpenCapacity }: TopBarProps) {
  const connection = useConnection();
  const fleet = useFleet();
  const engine = useEngine();
  const gates = useGates();
  const hosts = useHosts();
  const selectedHostId = useSelectedHostId();
  const projectFilter = useProjectFilter();
  const preview = usePreview();
  const connect = useFleetStore((state) => state.connect);
  const disconnect = useFleetStore((state) => state.disconnect);
  const setProject = useFleetStore((state) => state.setProject);
  const latencyHistory = useFleetStore((state) => state.latencyHistory);

  const activeHost =
    hosts.find((host) => host.id === connection.host_id) ??
    hosts.find((host) => host.id === selectedHostId) ??
    null;
  const tone = connectionTone(connection, fleet);
  const label = connectionLabel(connection, fleet);
  const latency = connection.latency;
  const inboxCount = fleet?.inbox.filter((item) => item.state === "open").length ?? 0;
  const transitions = connection.state === "connecting" || connection.state === "disconnecting";
  const connected = connection.state === "connected" || connection.state === "stale";
  const canConnect = Boolean(activeHost) && !transitions;
  const localGate = gates?.local_orchestrator.state ?? "unknown";
  const asifGate = gates?.asif.state ?? "unknown";

  const announcement =
    connection.state === "connected"
      ? `Connected to ${activeHost?.label ?? "host"}${fleet?.stale ? ", data is stale" : ""}.`
      : connection.state === "failed"
        ? `Connection failed${connection.error ? `: ${connection.error.message}` : ""}.`
        : connection.state === "disconnected"
          ? "Disconnected from the host."
          : `${label}${activeHost ? ` ${activeHost.label}` : ""}.`;

  function onConnectionClick() {
    if (!activeHost) {
      onNavigate("hosts");
      return;
    }
    if (connected || transitions) void disconnect(activeHost.id);
    else void connect(activeHost.id);
  }

  return (
    <header className="topbar">
      <div className="topbar__brand">
        <span className="topbar__mark" aria-hidden="true">
          ✦
        </span>
        <span className="topbar__wordmark">StackSpirit</span>
        {preview ? (
          <Badge
            tone="warn"
            uppercase
            className="preview-badge"
            title="Mock data. Nothing here comes from a real VPS."
          >
            Preview data
          </Badge>
        ) : null}
      </div>

      <div className="topbar__cluster topbar__cluster--host">
        <span
          className={`status-dot status-dot--${tone}`}
          data-testid="host-status"
          data-tone={tone}
          aria-hidden="true"
        />
        <div className="topbar__host-text">
          <strong>{activeHost?.label ?? "Not connected"}</strong>
          <span
            className="muted mono small"
            title={
              activeHost
                ? `${activeHost.ssh.user}@${activeHost.ssh.host}:${activeHost.ssh.port}`
                : "Add a VPS to begin"
            }
          >
            {activeHost
              ? `${activeHost.ssh.user}@${activeHost.ssh.host}:${activeHost.ssh.port}`
              : "Add a VPS to begin"}
          </span>
        </div>
        <span className="muted small">{label}</span>
        {connected && latency ? (
          <span className="topbar__latency">
            <Tooltip
              content={`last ${latencyLabel(latency.last_ms)} · p50 ${latencyLabel(
                latency.p50_ms,
              )} · p95 ${latencyLabel(latency.p95_ms)} · ${
                latency.samples
              } samples · updated ${formatClock(fleet?.fetched_at)}`}
            >
              <span className="mono">{latencyLabel(latency.last_ms)}</span>
              <Sparkline
                samples={latencyHistory}
                p50={latency.p50_ms}
                p95={latency.p95_ms}
                label="Connection latency"
              />
            </Tooltip>
          </span>
        ) : null}
      </div>

      <Button
        variant={connected || transitions ? "secondary" : "primary"}
        size="sm"
        className="topbar__connect"
        data-testid="topbar-connect"
        onClick={onConnectionClick}
        disabled={!transitions && !canConnect && !activeHost}
        title={activeHost ? undefined : "Add a host first"}
      >
        {transitions ? "Cancel" : connected ? "Disconnect" : "Connect"}
      </Button>

      <Badge tone={supervisorTone(fleet)} className="topbar__supervisor">
        {supervisorLabel(fleet)}
      </Badge>

      <label className="topbar__project">
        <span className="sr-only">Project</span>
        <select
          className="select"
          value={projectFilter ?? ""}
          onChange={(event) => setProject(event.target.value === "" ? null : event.target.value)}
          disabled={!fleet}
          aria-label="Project"
        >
          <option value="">All projects</option>
          {(fleet?.projects ?? []).map((project) => (
            <option key={project.name} value={project.name}>
              {project.name}
            </option>
          ))}
        </select>
      </label>

      <Button
        variant="ghost"
        size="sm"
        className="topbar__inbox"
        onClick={onOpenInbox}
        aria-label={`Inbox, ${inboxCount} open`}
      >
        <span aria-hidden="true">◔</span> {inboxCount}
      </Button>

      <Button
        variant="ghost"
        size="sm"
        className="topbar__engine"
        onClick={() => onNavigate("engine")}
        aria-label={engineLabel(engine)}
      >
        <Badge tone={engineTone(engine)}>{engineLabel(engine)}</Badge>
      </Button>

      <div className="topbar__gates" aria-label="Capability gates">
        <Badge
          tone={gateTone(localGate)}
          title={`Local orchestrator gate: ${gateStateLabel(localGate)}`}
        >
          Local · {gateStateLabel(localGate)}
        </Badge>
        <Badge tone={gateTone(asifGate)} title={`ASIF gate: ${gateStateLabel(asifGate)}`}>
          ASIF · {gateStateLabel(asifGate)}
        </Badge>
      </div>

      <Button
        variant="ghost"
        size="sm"
        className="topbar__capacity"
        onClick={onOpenCapacity}
        aria-label="Open capacity rail"
      >
        Capacity
      </Button>

      {fleet ? (
        <span className="sr-only">
          Snapshot from {formatAge(fleet.supervisor.last_tick_age_secs)} ago.
        </span>
      ) : null}
      <p className="sr-only" role="status" aria-live="polite">
        {announcement}
      </p>
    </header>
  );
}
