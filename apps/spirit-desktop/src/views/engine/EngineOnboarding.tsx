import { useState } from "react";

import { Badge, Button, Panel } from "../../components";
import { toSpiritError } from "../../bridge/errors";
import type {
  EngineStatus,
  ExternalHarness,
  ExternalHarnessKind,
  KeyStatus,
  ModelChoice,
} from "../../bridge/wire";
import { engineLabel, engineTone } from "../../domain/connection";
import { selectedModelLabel } from "../../domain/engine";
import { ByokForm } from "./ByokForm";
import { ExternalHarnessPicker } from "./ExternalHarnessPicker";
import { ModelPicker } from "./ModelPicker";

export interface EngineOnboardingProps {
  status: EngineStatus | null;
  models: ModelChoice[];
  keyStatuses: KeyStatus[];
  harnesses: ExternalHarness[];
  scanning: boolean;
  initialStep?: 1 | 2;
  busy: boolean;
  error: string | null;
  advancedOpen: boolean;
  onToggleAdvanced: () => void;
  onRetry: () => void;
  onSelectModel: (id: string) => Promise<void>;
  onSaveKey: (provider: string, secret: string) => Promise<KeyStatus>;
  onVerifyKey: (provider: string) => Promise<KeyStatus>;
  onDeleteKey: (provider: string) => Promise<KeyStatus>;
  onScanHarnesses: () => void;
  onSelectExternal: (kind: ExternalHarnessKind, path: string) => void;
  onUseBundled: () => void;
  onFinish: () => void;
}

type Choice = "included" | "byok" | "external";

export function EngineOnboarding({
  status,
  models,
  keyStatuses,
  harnesses,
  scanning,
  initialStep = 1,
  busy,
  error,
  advancedOpen,
  onToggleAdvanced,
  onRetry,
  onSelectModel,
  onSaveKey,
  onVerifyKey,
  onDeleteKey,
  onScanHarnesses,
  onSelectExternal,
  onUseBundled,
  onFinish,
}: EngineOnboardingProps) {
  const [step, setStep] = useState<1 | 2 | 3>(initialStep);
  const [choice, setChoice] = useState<Choice>("included");
  const [selectedId, setSelectedId] = useState<string>(
    status?.selected_model?.id ?? models[0]?.id ?? "",
  );
  const [continuing, setContinuing] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const effectiveSelectedId = selectedId || models[0]?.id || "";

  const ready = status?.state.state === "ready";
  const externalSelected = status?.choice.type === "external";
  const savedKeys = keyStatuses.filter((key) => key.state !== "missing");
  const canContinue =
    choice === "included"
      ? Boolean(effectiveSelectedId) && ready
      : choice === "byok"
        ? savedKeys.length > 0
        : externalSelected;

  const stateDetail =
    status?.state.state === "degraded" || status?.state.state === "failed"
      ? status.state.detail
      : null;

  async function continueFromModels() {
    setLocalError(null);
    if (choice === "included") {
      setContinuing(true);
      try {
        await onSelectModel(effectiveSelectedId);
        setStep(3);
      } catch (selectionError) {
        setLocalError(toSpiritError(selectionError).message);
      } finally {
        setContinuing(false);
      }
      return;
    }
    setStep(3);
  }

  return (
    <Panel
      title="StackSpirit Engine"
      data-testid="engine-onboarding"
      actions={
        <Badge tone={engineTone(status)}>
          {status ? engineLabel(status) : "Checking engine"}
        </Badge>
      }
    >
      <div className="engine-section">
        <ol className="engine-steps" aria-label="Engine setup steps">
          {[
            { number: 1, label: "Engine check" },
            { number: 2, label: "Choose models" },
            { number: 3, label: "Finish" },
          ].map((item) => (
            <li
              key={item.number}
              className={`engine-step${step === item.number ? " engine-step--active" : ""}${
                step > item.number ? " engine-step--done" : ""
              }`}
              aria-current={step === item.number ? "step" : undefined}
            >
              <span className="engine-step__index">Step {item.number}</span>
              <span>{item.label}</span>
            </li>
          ))}
        </ol>

        {step === 1 ? (
          <div className="engine-section" data-testid="engine-step-check">
            <h3 className="panel__title">StackSpirit Engine check</h3>
            <p className="muted small">
              StackSpirit verifies the bundled engine, starts it on a loopback port and
              waits for health. Nothing is installed and no VPS change is made.
            </p>
            {ready ? (
              <p>
                StackSpirit Engine found · version {status?.version ?? "unknown"} ·{" "}
                <span className="mono">127.0.0.1:{status?.port ?? "—"}</span>
              </p>
            ) : stateDetail ? (
              <div className="notice notice--danger">
                <p>{stateDetail}</p>
                <p className="muted small">
                  Reinstall StackSpirit to restore the bundled engine. If you already have
                  a harness, choose an existing installation on the next step.
                </p>
              </div>
            ) : status?.state.state === "stopped" ||
              status?.state.state === "not_configured" ? (
              <p className="muted small">
                The bundled engine is stopped. Retry the engine check to start it on a
                loopback port.
              </p>
            ) : (
              <p className="muted small">Checking the bundled engine and waiting for health…</p>
            )}
            {error ? <p className="engine-error">{error}</p> : null}
            <div className="engine-actions">
              <Button variant="secondary" size="sm" onClick={onRetry} loading={busy}>
                Retry engine check
              </Button>
              {!ready ? (
                <Button variant="ghost" size="sm" onClick={onToggleAdvanced}>
                  Repair instructions
                </Button>
              ) : null}
              <Button variant="primary" size="sm" onClick={() => setStep(2)}>
                Continue
              </Button>
            </div>
            {!ready ? (
              <p className="gate-reason">
                Included models stay unavailable until the engine is healthy; you can still
                use your own provider key or an existing harness.
              </p>
            ) : null}
          </div>
        ) : null}

        {step === 2 ? (
          <div className="engine-section" data-testid="engine-step-models">
            <h3 className="panel__title">Choose models</h3>
            <div className="engine-choice" role="radiogroup" aria-label="Model source">
              <label
                className={`engine-choice__option${
                  choice === "included" ? " engine-choice__option--selected" : ""
                }`}
              >
                <input
                  type="radio"
                  name="model-source"
                  checked={choice === "included"}
                  onChange={() => setChoice("included")}
                />
                <span className="engine-choice__option-body">
                  <strong>Included models (recommended)</strong>
                  <span className="muted small">
                    Included with StackSpirit Engine; no key required. Availability is
                    checked when the engine starts.
                  </span>
                </span>
              </label>
              {choice === "included" ? (
                <div className="engine-section">
                  <ModelPicker
                    models={models}
                    selectedId={effectiveSelectedId}
                    onSelect={setSelectedId}
                    disabled={busy}
                    emptyMessage={
                      ready
                        ? undefined
                        : "The engine is not ready, so included models cannot be verified yet. Retry the engine check or use another source."
                    }
                    emptyAction={
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => setChoice("byok")}
                      >
                        Add your own provider key
                      </Button>
                    }
                  />
                </div>
              ) : null}

              <label
                className={`engine-choice__option${
                  choice === "byok" ? " engine-choice__option--selected" : ""
                }`}
              >
                <input
                  type="radio"
                  name="model-source"
                  checked={choice === "byok"}
                  onChange={() => setChoice("byok")}
                />
                <span className="engine-choice__option-body">
                  <strong>Use my own provider key</strong>
                  <span className="muted small">
                    The key stays in this Mac's keychain and is never sent to your VPS.
                  </span>
                </span>
              </label>
              {choice === "byok" ? (
                <ByokForm
                  providers={[
                    { id: "deepseek", label: "DeepSeek" },
                    { id: "openai", label: "OpenAI" },
                    { id: "anthropic", label: "Anthropic" },
                    { id: "google", label: "Google" },
                    { id: "openrouter", label: "OpenRouter" },
                    { id: "xai", label: "xAI" },
                    { id: "ollama", label: "Ollama" },
                  ]}
                  statuses={keyStatuses}
                  onSave={onSaveKey}
                  onVerify={onVerifyKey}
                  onDelete={onDeleteKey}
                />
              ) : null}

              <label
                className={`engine-choice__option${
                  choice === "external" ? " engine-choice__option--selected" : ""
                }`}
              >
                <input
                  type="radio"
                  name="model-source"
                  checked={choice === "external"}
                  onChange={() => setChoice("external")}
                />
                <span className="engine-choice__option-body">
                  <strong>Use an existing harness</strong>
                  <span className="muted small">
                    Keep the login and model access of an installation you already have.
                  </span>
                </span>
              </label>
              {choice === "external" ? (
                <ExternalHarnessPicker
                  harnesses={harnesses}
                  scanning={scanning}
                  selected={externalSelected}
                  error={error}
                  onScan={onScanHarnesses}
                  onSelect={onSelectExternal}
                  onUseBundled={onUseBundled}
                />
              ) : null}
            </div>

            {localError ? <p className="engine-error">{localError}</p> : null}
            {!canContinue ? (
              <p className="gate-reason">
                {choice === "included"
                  ? "Select an included model to continue."
                  : choice === "byok"
                    ? "Save at least one provider key to continue."
                    : "Scan for a harness and select one to continue."}
              </p>
            ) : null}
            <div className="engine-actions">
              <Button variant="ghost" size="sm" onClick={() => setStep(1)}>
                Back
              </Button>
              <Button
                variant="primary"
                size="sm"
                onClick={() => void continueFromModels()}
                loading={continuing}
                disabled={!canContinue || busy}
              >
                Continue
              </Button>
            </div>
          </div>
        ) : null}

        {step === 3 ? (
          <div className="engine-section" data-testid="engine-step-finish">
            <h3 className="panel__title">Finish</h3>
            <p>
              {choice === "included"
                ? `Model selected · ${selectedModelLabel(status)}`
                : choice === "byok"
                  ? `Provider key saved · ${savedKeys
                      .map((key) => key.provider)
                      .join(", ")}`
                  : "External harness selected"}
            </p>
            <p className="muted small">
              The selection is saved locally. It does not change the VPS; provider and model
              names only cross the SSH boundary later, when a VOWP profile or local
              orchestrator is configured.
            </p>
            <div className="engine-actions">
              <Button variant="ghost" size="sm" onClick={() => setStep(2)}>
                Back
              </Button>
              <Button variant="primary" size="sm" onClick={onFinish}>
                Finish setup
              </Button>
            </div>
          </div>
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
                Engine implementation: opencode {status?.version ?? "unknown version"}
              </span>
              {status?.binary_path ? (
                <span className="mono">{status.binary_path}</span>
              ) : null}
              <span>StackSpirit Engine runs on opencode (MIT); see THIRD_PARTY_NOTICES.md.</span>
              <span>Nothing here is sent to the VPS.</span>
            </div>
          ) : null}
        </div>
      </div>
    </Panel>
  );
}
