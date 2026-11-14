import { useId, type ReactNode } from "react";

import { Badge } from "../../components";
import type { ModelChoice } from "../../bridge/wire";
import { formatDateTime } from "../../domain/format";
import { modelContextLabel, modelNotes, modelProviderLabel } from "../../domain/engine";

export interface ModelPickerProps {
  models: ModelChoice[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  disabled?: boolean;
  emptyAction?: ReactNode;
  emptyMessage?: string;
}

export function ModelPicker({
  models,
  selectedId,
  onSelect,
  disabled = false,
  emptyAction,
  emptyMessage = "This engine build has no included models available. Add your own provider key to continue.",
}: ModelPickerProps) {
  const group = useId();

  if (models.length === 0) {
    return (
      <div className="engine-section">
        <p className="muted small">{emptyMessage}</p>
        {emptyAction}
      </div>
    );
  }

  return (
    <div className="engine-model-list" role="radiogroup" aria-label="Included models">
      {models.map((model) => {
        const checked = model.id === selectedId;
        return (
          <label
            key={model.id}
            className={`engine-model${checked ? " engine-model--selected" : ""}`}
            data-testid={`model-${model.id}`}
          >
            <span className="engine-model__main">
              <span>
                <input
                  type="radio"
                  name={group}
                  value={model.id}
                  checked={checked}
                  disabled={disabled}
                  onChange={() => onSelect(model.id)}
                  aria-label={`${model.label}, ${modelProviderLabel(model.provider)}`}
                />{" "}
                <strong>{model.label}</strong>
              </span>
              <span className="engine-model__meta">
                <span>{modelProviderLabel(model.provider)}</span>
                <span>{modelContextLabel(model.context_tokens)}</span>
                <Badge tone="ok">Included · no key</Badge>
                <span>
                  {model.verified_at
                    ? `verified ${formatDateTime(model.verified_at)}`
                    : "not verified"}
                </span>
              </span>
              <span className="engine-model__notes">
                {modelNotes(model.provider, model.notes)}
              </span>
            </span>
          </label>
        );
      })}
    </div>
  );
}
