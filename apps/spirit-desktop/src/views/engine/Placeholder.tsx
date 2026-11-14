import { Badge, Button, EmptyState, Panel } from "../../components";
import { engineLabel, engineTone, gateStateLabel, gateTone } from "../../domain/connection";
import { useEngine, useFleetStore, useGates } from "../../state/fleetStore";

export function EnginePlaceholder() {
  const engine = useEngine();
  const gates = useGates();
  const refreshEngine = useFleetStore((state) => state.refreshEngine);
  const probeGates = useFleetStore((state) => state.probeGates);

  const local = gates?.local_orchestrator ?? { state: "unknown" as const };
  const asif = gates?.asif ?? { state: "unknown" as const };

  return (
    <div className="view-stack" data-testid="engine-placeholder">
      <Panel
        title="StackSpirit Engine"
        actions={
          <Button variant="secondary" size="sm" onClick={() => void refreshEngine()}>
            Refresh status
          </Button>
        }
      >
        {!engine ? (
          <EmptyState
            icon="◇"
            title="Engine status unavailable"
            body="The local engine boundary did not answer. Restart StackSpirit and retry."
            actionLabel="Refresh status"
            onAction={() => void refreshEngine()}
          />
        ) : (
          <div className="engine-card">
            <div className="engine-card__head">
              <Badge tone={engineTone(engine)}>{engineLabel(engine)}</Badge>
              <Badge tone="unknown">{engine.choice.type}</Badge>
            </div>
            <dl className="metric-grid">
              <div>
                <dt>Version</dt>
                <dd className="mono">{engine.version ?? "unknown"}</dd>
              </div>
              <div>
                <dt>Loopback port</dt>
                <dd className="mono">{engine.port ?? "—"}</dd>
              </div>
              <div>
                <dt>Included models</dt>
                <dd>{engine.included_models_available ? "verified" : "not verified"}</dd>
              </div>
              <div>
                <dt>Selected model</dt>
                <dd>{engine.selected_model?.label ?? "none"}</dd>
              </div>
            </dl>
            {engine.last_error ? <p className="notice notice--danger">{engine.last_error}</p> : null}
          </div>
        )}
        <p className="muted small">
          Engine onboarding, included models, BYOK key storage and the external harness
          picker arrive with the Engine Onboarding job (D4). No provider key is ever sent
          to the VPS; this view only shows provider and model names.
        </p>
      </Panel>

      <Panel
        title="Capability gates"
        actions={
          <Button variant="secondary" size="sm" onClick={() => void probeGates()}>
            Probe gates
          </Button>
        }
      >
        <div className="gate-grid">
          <div className="gate-card">
            <div className="gate-card__head">
              <span>Local orchestrator</span>
              <Badge tone={gateTone(local.state)}>{gateStateLabel(local.state)}</Badge>
            </div>
            {local.state === "waiting_for_vowp_phase3" ? (
              <ul className="reason-list">
                {local.missing.map((item) => (
                  <li key={item} className="mono small">
                    {item}
                  </li>
                ))}
              </ul>
            ) : local.state === "unreachable" ? (
              <p className="muted small">{local.detail}</p>
            ) : local.state === "incompatible" ? (
              <p className="muted small">
                detected {local.detected}; required {local.required}
              </p>
            ) : null}
          </div>

          <div className="gate-card">
            <div className="gate-card__head">
              <span>ASIF export / import</span>
              <Badge tone={gateTone(asif.state)}>{gateStateLabel(asif.state)}</Badge>
            </div>
            {asif.state === "waiting_for_vowp_asif" ? (
              <ul className="reason-list">
                {asif.missing.map((item) => (
                  <li key={item} className="mono small">
                    {item}
                  </li>
                ))}
              </ul>
            ) : asif.state === "ready" ? (
              <p className="muted small">
                export {asif.export ? "ready" : "not proven"} · import{" "}
                {asif.import ? "ready" : "not proven"}
              </p>
            ) : asif.state === "unreachable" ? (
              <p className="muted small">{asif.detail}</p>
            ) : null}
          </div>
        </div>
        <p className="muted small">
          A gate is never Ready from a version string alone. D5 and D6 remain gated until
          VOWP proves the capability.
        </p>
      </Panel>
    </div>
  );
}
