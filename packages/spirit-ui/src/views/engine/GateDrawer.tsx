import { Badge, Drawer } from "../../components";
import type { GateSnapshot } from "../../bridge/wire";
import { gateStateLabel, gateTone } from "../../domain/connection";
import { formatDateTime } from "../../domain/format";
import {
  gateActionEnabled,
  gateActionReason,
  gateMissingMilestones,
  localAgentLabel,
} from "../../domain/engine";
import { asifGateBadge } from "../../domain/session";

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

export function GateDrawer({ kind, gates, open, onClose }: GateDrawerProps) {
  const title = kind === "local" ? "Local orchestrator" : "ASIF sessions";
  const gate = gates
    ? kind === "local"
      ? gates.local_orchestrator
      : gates.asif
    : null;
  const state = gate?.state ?? "unknown";
  const evidence = gates ? evidenceFor(gates, kind) : [];
  const missing = gates ? gateMissingMilestones(gates)[kind] : [];
  const detected =
    gate && "detected_version" in gate ? gate.detected_version : null;
  const asifBadge = asifGateBadge(gates);
  const badge =
    kind === "asif"
      ? asifBadge
      : { label: gateStateLabel(state), tone: gateTone(state) };

  return (
    <Drawer open={open} title={`${title} details`} onClose={onClose}>
      <div className="engine-section" data-testid="gate-drawer">
        <div className="engine-actions">
          <Badge tone={badge.tone}>{badge.label}</Badge>
          {detected ? <span className="muted small">VOWP {detected}</span> : null}
          {gates ? (
            <span className="muted small">probed {formatDateTime(gates.probed_at)}</span>
          ) : null}
        </div>

        <div>
          <h3 className="panel__title">Missing VOWP milestones</h3>
          {missing.length > 0 ? (
            <ul className="gate-required" aria-label="Missing VOWP milestones">
              {missing.map((item) => (
                <li key={item}>{item}</li>
              ))}
            </ul>
          ) : (
            <p className="muted small">
              The probe did not report a missing milestone for this capability.
            </p>
          )}
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

        {kind === "asif" ? (
          <div>
            <h3 className="panel__title">Export and import availability</h3>
            <ul className="gate-availability" aria-label="ASIF action availability">
              {(["asif_export", "asif_import"] as const).map((action) => {
                const enabled = gates ? gateActionEnabled(gates, action) : false;
                return (
                  <li key={action} className="gate-availability__row">
                    <Badge tone={enabled ? "ok" : "warn"}>
                      {enabled ? "Enabled" : "Disabled"}
                    </Badge>
                    <span>{action === "asif_export" ? "Export .asif.zip" : "Import .asif.zip"}</span>
                    <span className="muted small">
                      {enabled
                        ? "The probe proves this direction and the D5 build flag is on."
                        : gates
                          ? gateActionReason(gates, action)
                          : "Probe the VOWP home to check this capability."}
                    </span>
                  </li>
                );
              })}
            </ul>
            <p className="muted small">
              A Ready gate can prove one direction only. The disabled direction keeps its
              exact reason; it is never presented as ready.
            </p>
          </div>
        ) : null}

        <div>
          <h3 className="panel__title">StackSpirit action</h3>
          <p className="muted small">
            {kind === "local"
              ? gates && gateActionEnabled(gates, "local_orchestrator")
                ? "Enabled in this build."
                : gates?.local_orchestrator.state === "ready" &&
                    gates.local_agent.state === "connected"
                  ? "Not implemented in this build. The action stays disabled even when VOWP is ready."
                  : "Disabled until the local-orchestrator gate is Ready and the local VOWP agent is connected."
              : gates?.features.asif_actions
                ? "Enabled in this build."
                : "Not implemented in this build. Export and import stay disabled even when VOWP is ready."}
          </p>
          <p className="muted small">
            Evidence contains command names, exit codes and versions only. It never
            contains a provider key, a path or a response body.
          </p>
          {kind === "asif" ? (
            <p className="muted small">
              The renderer receives only a local file name, byte count, declared losses,
              warnings and unsupported operations. It never receives package bytes, a
              remote path, a token or a provider key.
            </p>
          ) : null}
        </div>
      </div>
    </Drawer>
  );
}
