import { useId } from "react";

export interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  stateText?: string;
  disabled?: boolean;
}

export function Toggle({ checked, onChange, label, stateText, disabled = false }: ToggleProps) {
  const id = useId();
  return (
    <span className="toggle-field">
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        className="toggle"
        onClick={() => onChange(!checked)}
      >
        <span className="toggle__thumb" aria-hidden="true" />
      </button>
      <label htmlFor={id} className="toggle__label">
        {label}
      </label>
      <span className="toggle__state">{stateText ?? (checked ? "On" : "Off")}</span>
    </span>
  );
}
