import { Badge } from "../../components";
import type { HostProbe, HostProbeStage } from "../../bridge/wire";

const LABELS: Record<string, string> = {
  host_key: "Host key matches",
  ssh: "SSH reachable",
  vowp: "VOWP found",
  snapshot: "Snapshot readable",
  supervisor: "Supervisor running",
};

const ORDER = ["host_key", "ssh", "vowp", "snapshot", "supervisor"];

export function ProbeChecklist({
  probe,
  busy,
}: {
  probe: HostProbe | null;
  busy: boolean;
}) {
  const stages: HostProbeStage[] =
    probe?.stages ??
    ORDER.map((stage) => ({
      stage,
      ok: false,
      detail: "Waiting to run…",
    }));

  return (
    <ol className="probe-list" aria-label="Connection checks" aria-busy={busy}>
      {[...stages]
        .sort((left, right) => ORDER.indexOf(left.stage) - ORDER.indexOf(right.stage))
        .map((stage) => {
          const label = LABELS[stage.stage] ?? stage.stage;
          const state = busy && !probe ? "pending" : stage.ok ? "ok" : "failed";
          return (
            <li
              key={stage.stage}
              className={`probe-list__item probe-list__item--${state}`}
              data-testid={`probe-stage-${stage.stage}`}
            >
              <span className="probe-list__icon" aria-hidden="true">
                {state === "ok" ? "✓" : state === "failed" ? "✕" : "○"}
              </span>
              <span className="probe-list__body">
                <span className="probe-list__label">{label}</span>
                <span className="probe-list__detail mono">{stage.detail}</span>
              </span>
              <Badge tone={state === "ok" ? "ok" : state === "failed" ? "danger" : "unknown"}>
                {state === "ok" ? "Passed" : state === "failed" ? "Failed" : "Waiting"}
              </Badge>
            </li>
          );
        })}
    </ol>
  );
}

/** Checks 1–4 gate Save; the supervisor is informational. */
export function probeAllowsSave(probe: HostProbe | null): boolean {
  if (!probe) return false;
  return ["host_key", "ssh", "vowp", "snapshot"].every(
    (name) => probe.stages.find((stage) => stage.stage === name)?.ok === true,
  );
}

export function failingStage(probe: HostProbe | null): HostProbeStage | null {
  if (!probe) return null;
  return (
    ORDER.map((name) => probe.stages.find((stage) => stage.stage === name)).find(
      (stage) => stage !== undefined && !stage.ok,
    ) ?? null
  );
}
