import { Badge, Button } from "../../components";
import type { GateSnapshot } from "../../bridge/wire";
import { gateStateLabel, gateTone } from "../../domain/connection";
import {
  localAgentLabel,
  gateActionEnabled,
  gateActionReason,
} from "../../domain/engine";

export interface GateCardProps {
  kind: "local" | "asif";
  gates: GateSnapshot;
  onDetails: () => void;
}

function localDetail(gates: GateSnapshot): string | null {
  const gate = gates.local_orchestrator;
  if (gate.state === "waiting_for_vowp_phase3") {
    return `Waiting for VOWP Phase 2/3 · detected ${gate.detected_version ?? "unknown"}`;
  }
  if (gate.state === "incompatible") {
    return `Detected ${gate.detected}; required ${gate.required}.`;
  }
  if (gate.state === "unreachable") return gate.detail;
  if (gate.state === "ready") {
    return `VOWP node ${gate.node ?? "unknown"} · ${gate.agent_protocol}`;
  }
  return null;
}

function asifDetail(gates: GateSnapshot): string | null {
  const gate = gates.asif;
  if (gate.state === "waiting_for_vowp_asif") {
    return `Waiting for VOWP ASIF · detected ${gate.detected_version ?? "unknown"}`;
  }
  if (gate.state === "incompatible") {
    return `Detected ${gate.detected}; required ${gate.required}.`;
  }
  if (gate.state === "unreachable") return gate.detail;
  if (gate.state === "ready") {
    return `export ${gate.export ? "proven" : "not proven"} · import ${
      gate.import ? "proven" : "not proven"
    }`;
  }
  return null;
}

export function GateCard({ kind, gates, onDetails }: GateCardProps) {
  const local = gates.local_orchestrator;
  const asif = gates.asif;
  const gate = kind === "local" ? local : asif;
  const detail = kind === "local" ? localDetail(gates) : asifDetail(gates);
  const missing =
    kind === "local"
      ? local.state === "waiting_for_vowp_phase3"
        ? local.missing
        : []
      : asif.state === "waiting_for_vowp_asif"
        ? asif.missing
        : [];

  const localEnabled = gateActionEnabled(gates, "local_orchestrator");
  const exportEnabled = gateActionEnabled(gates, "asif_export");
  const importEnabled = gateActionEnabled(gates, "asif_import");

  return (
    <div className="gate-card" data-testid={`gate-card-${kind}`}>
      <div className="gate-card__head">
        <span>{kind === "local" ? "Local orchestrator" : ".asif.zip sessions"}</span>
        <Badge tone={gateTone(gate.state)}>{gateStateLabel(gate.state)}</Badge>
      </div>
      <p className="muted small">
        {kind === "local"
          ? "Run global and project orchestrators on this Mac."
          : "Download and import portable session packages."}
      </p>
      {detail ? <p className="muted small">{detail}</p> : null}
      {missing.length > 0 ? (
        <p className="muted small">
          Missing: <span className="mono">{missing.join(" · ")}</span>
        </p>
      ) : null}
      {kind === "local" ? (
        <div className="engine-actions">
          <span className="muted small">Local VOWP agent:</span>
          <Badge tone={gates.local_agent.state === "connected" ? "ok" : "warn"}>
            {localAgentLabel(gates.local_agent)}
          </Badge>
        </div>
      ) : null}

      <div className="gate-actions">
        {kind === "local" ? (
          <>
            <Button
              variant="secondary"
              size="sm"
              disabled={!localEnabled}
              title={localEnabled ? undefined : gateActionReason(gates, "local_orchestrator")}
            >
              Run locally
            </Button>
            <Button variant="ghost" size="sm" onClick={onDetails}>
              Details
            </Button>
          </>
        ) : (
          <>
            <Button
              variant="secondary"
              size="sm"
              disabled={!exportEnabled}
              title={exportEnabled ? undefined : gateActionReason(gates, "asif_export")}
            >
              Export .asif.zip
            </Button>
            <Button
              variant="secondary"
              size="sm"
              disabled={!importEnabled}
              title={importEnabled ? undefined : gateActionReason(gates, "asif_import")}
            >
              Import .asif.zip
            </Button>
            <Button variant="ghost" size="sm" onClick={onDetails}>
              Details
            </Button>
          </>
        )}
      </div>
      {kind === "local" && !localEnabled ? (
        <p className="gate-reason">{gateActionReason(gates, "local_orchestrator")}</p>
      ) : null}
      {kind === "asif" && !exportEnabled ? (
        <p className="gate-reason">{gateActionReason(gates, "asif_export")}</p>
      ) : null}
      {kind === "asif" && !importEnabled ? (
        <p className="gate-reason">{gateActionReason(gates, "asif_import")}</p>
      ) : null}
    </div>
  );
}
