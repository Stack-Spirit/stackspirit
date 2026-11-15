import { useId } from "react";

import {
  asifLossPolicyHint,
  asifLossPolicyTitle,
  asifLossPolicyWarning,
  type AsifDirection,
  type AsifLossPolicyChoice,
} from "../../domain/session";

export interface LossPolicyFieldsProps {
  direction: AsifDirection;
  policy: AsifLossPolicyChoice;
  disabled?: boolean;
  onChange: (policy: AsifLossPolicyChoice) => void;
}

/**
 * The only loss-policy controls. Strict and allow-loss are mutually exclusive; there
 * is no force or allow-unsupported switch.
 */
export function LossPolicyFields({
  direction,
  policy,
  disabled = false,
  onChange,
}: LossPolicyFieldsProps) {
  const allowHintId = useId();
  const strictHintId = useId();
  return (
    <div className="loss-policy">
      <fieldset className="loss-policy__group" disabled={disabled}>
        <legend className="loss-policy__legend">Loss policy (advanced)</legend>
        <div className="loss-policy__option">
          <input
            id={`${direction}-allow-loss`}
            type="radio"
            name="asif-loss-policy"
            value="allow-loss"
            checked={policy === "allow-loss"}
            aria-describedby={allowHintId}
            data-testid={`${direction}-policy-allow-loss`}
            onChange={() => onChange("allow-loss")}
          />
          <div>
            <label htmlFor={`${direction}-allow-loss`}>
              {asifLossPolicyTitle("allow-loss")}
            </label>
            <p id={allowHintId} className="muted small">
              {asifLossPolicyHint("allow-loss", direction)}
            </p>
          </div>
        </div>
        <div className="loss-policy__option">
          <input
            id={`${direction}-strict`}
            type="radio"
            name="asif-loss-policy"
            value="strict"
            checked={policy === "strict"}
            aria-describedby={strictHintId}
            data-testid={`${direction}-policy-strict`}
            onChange={() => onChange("strict")}
          />
          <div>
            <label htmlFor={`${direction}-strict`}>{asifLossPolicyTitle("strict")}</label>
            <p id={strictHintId} className="muted small">
              {asifLossPolicyHint("strict", direction)}
            </p>
          </div>
        </div>
      </fieldset>
      <p
        className={policy === "strict" ? "notice notice--warn" : "notice"}
        role="status"
        data-testid={`${direction}-policy-warning`}
      >
        {asifLossPolicyWarning(policy, direction)}
      </p>
    </div>
  );
}
