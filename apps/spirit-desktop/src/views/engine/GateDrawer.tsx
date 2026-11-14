import { Badge, Drawer } from "../../components";
import type { GateSnapshot } from "../../bridge/wire";
import { gateStateLabel, gateTone } from "../../domain/connection";
import { formatDateTime } from "../../domain/format";
import {
  ASIF_MILESTONES,
  PHASE3_MILESTONES,
  localAgentLabel,
} from "../../domain/engine";

export interface GateDrawerProps {
  kind: "local" | "asif";
  gates: GateSnapshot | null;
  open: boolean;
  onClose: () => void;
}

function evidenceFor(gates: GateSnapshot, kind: "local" | "asif"): string[] {
  if (kind === "local") {
    const gate = gates.local_orchestrator;
    return "evidence" in gate ? gate.evidence : [];
  }
  const gate = gates.asif;
  return "evidence" in gate ? gate.evidence : [];
}

function missingFor(gates: GateSnapshot, kind: "local" | "asif"): string[] {
  const required = kind === "local" ? [...PHASE3_MILESTONES] : [...ASIF_MILESTONES];
  const probed =
    kind === "local"
      ? gates.local_orchestrator.state === "waiting_for_vowp_phase3"
        ? gates.local_orchestrator.missing
        : []
      : gates.asif.state === "waiting_for_vowp_asif"
        ? gates.asif.missing
        : [];
  return [...new Set([...probed, ...required])];
}

export function GateDrawer({ kind, gates, open, onClose }: GateDrawerProps) {
  const title = kind === "local" ? "Local orchestrator" : "ASIF sessions";
  const gate = gates
    ? kind === "local"
      ? gates.local_orchestrator
      : gates.asif
    : null;
  const state = gate?.state ?? "unknown";
  const evidence = gates ? evidenceFor(gates, kind) : [];
  const missing = gates ? missingFor(gates, kind) : [];
  const detected =
    gate && "detected_version" in gate ? gate.detected_version : null;

  return (
    <Drawer open={open} title={`${title} details`} onClose={onClose}>
      <div className="engine-section" data-testid="gate-drawer">
        <div className="engine-actions">
          <Badge tone={gateTone(state)}>{gateStateLabel(state)}</Badge>
          {detected ? <span className="muted small">VOWP {detected}</span> : null}
          {gates ? (
            <span className="muted small">probed {formatDateTime(gates.probed_at)}</span>
          ) : null}
        </div>

        <div>
          <h3 className="panel__title">Missing or required milestones</h3>
          <ul className="gate-required" aria-label="Required VOWP milestones">
            {missing.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        </div>

        <div>
          <h3 className="panel__title">Probe evidence</h3>
          {evidence.length > 0 ? (
            <div className="gate-evidence" aria-label="Gate probe evidence">
              {evidence.map((line) => (
                <span key={line}>{line}</span>
              ))}
            </div>
          ) : (
            <p className="muted small">No probe has run for this host.</p>
          )}
        </div>

        {kind === "local" && gates ? (
          <div>
            <h3 className="panel__title">Local VOWP agent</h3>
            <p className="muted small">
              {localAgentLabel(gates.local_agent)}
              {gates.local_agent.protocol ? ` · ${gates.local_agent.protocol}` : ""}
            </p>
            {gates.local_agent.last_error ? (
              <p className="muted small">{gates.local_agent.last_error}</p>
            ) : null}
          </div>
        ) : null}

        <div>
          <h3 className="panel__title">StackSpirit action</h3>
          <p className="muted small">
            {kind === "local"
              ? gates?.features.local_orchestrator_actions
                ? "Enabled in this build."
                : "Not implemented in this build. The action stays disabled even when VOWP is ready."
              : gates?.features.asif_actions
                ? "Enabled in this build."
                : "Not implemented in this build. Export and import stay disabled even when VOWP is ready."}
          </p>
          <p className="muted small">
            Evidence contains command names, exit codes and versions only. It never
            contains a provider key, a path or a response body.
          </p>
        </div>
      </div>
    </Drawer>
  );
}
