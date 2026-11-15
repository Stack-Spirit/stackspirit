import { useState } from "react";

import type { TargetMode } from "../../bridge/wire";
import { toSpiritError } from "../../bridge/errors";
import { Badge, Button, Modal, Panel } from "../../components";
import {
  targetAdvice,
  targetModeLabel,
  targetSourceTitle,
} from "../../domain/capacity";
import { formatDateTime } from "../../domain/format";
import {
  useCapacity,
  useFleetStore,
  useMutationsAllowed,
} from "../../state/fleetStore";

const MAX_TARGET = 64;

function clampTarget(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(MAX_TARGET, Math.max(0, Math.trunc(value)));
}

export function TargetAdvisor() {
  const capacity = useCapacity();
  const mutationsAllowed = useMutationsAllowed();
  const bridge = useFleetStore((state) => state.bridge);
  const refresh = useFleetStore((state) => state.refresh);

  const current = capacity?.target.target_workers ?? 0;
  const advised =
    capacity?.advised_workers ?? capacity?.target.advised_workers ?? null;
  const override = capacity?.target.is_override ?? false;
  const advice = targetAdvice(current, advised, override);

  const [draftOverride, setDraftOverride] = useState<string | null>(null);
  const [modeOverride, setModeOverride] = useState<TargetMode | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [overrideTarget, setOverrideTarget] = useState<number | null>(null);
  const [overrideReason, setOverrideReason] = useState(
    "Owner raised the worker target above VOWP's advice.",
  );
  const draft = draftOverride ?? String(current);
  const mode =
    modeOverride ?? (capacity?.target.mode === "auto" ? "auto" : "fixed");

  if (!capacity) {
    return (
      <Panel title="Target advisor" data-testid="target-advisor">
        <p className="muted small">Capacity detail unavailable — retrying.</p>
      </Panel>
    );
  }

  const disabled = !mutationsAllowed || busy;

  async function applyTarget(
    next: number,
    nextMode: TargetMode,
    useOverride: boolean,
    reason: string,
  ): Promise<void> {
    if (!bridge) {
      setError("The bridge is not initialized.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await bridge.fleet.setTarget({
        target: next,
        mode: nextMode,
        override: useOverride,
        reason: reason.trim().slice(0, 200),
      });
      // A capacity write always refreshes; the rail never keeps a local optimistic target.
      await refresh();
      setOverrideTarget(null);
      setDraftOverride(null);
      setModeOverride(null);
    } catch (caught) {
      const spirit = toSpiritError(caught);
      setError(spirit.hint ? `${spirit.message} ${spirit.hint}` : spirit.message);
    } finally {
      setBusy(false);
    }
  }

  function commit(nextValue: number): void {
    if (busy) return;
    const next = clampTarget(nextValue);
    setDraftOverride(String(next));
    if (next === current) return;
    if (advised !== null && next > advised) {
      setOverrideTarget(next);
      return;
    }
    void applyTarget(next, mode, false, "Adjusted the worker target from Fleet Canvas.");
  }

  function commitDraft(): void {
    const parsed = Number.parseInt(draft, 10);
    if (Number.isNaN(parsed)) {
      setDraftOverride(null);
      return;
    }
    commit(parsed);
  }

  function changeMode(nextMode: TargetMode): void {
    if (busy) return;
    setModeOverride(nextMode);
    if (nextMode === mode) return;
    void applyTarget(
      current,
      nextMode,
      override,
      "Changed the worker target mode from Fleet Canvas.",
    );
  }

  function cancelOverride(): void {
    setOverrideTarget(null);
    setDraftOverride(null);
  }

  return (
    <Panel
      title="Target advisor"
      actions={
        <Badge tone={override ? "warn" : "unknown"}>
          {override ? "Owner override" : targetModeLabel(capacity.target.mode)}
        </Badge>
      }
      className="target-advisor"
      data-testid="target-advisor"
    >
      <div className="target-advisor__headline">
        <div>
          <span className="eyebrow">Worker target</span>
          <strong>
            {current} worker{current === 1 ? "" : "s"}
          </strong>
        </div>
        <label className="target-advisor__mode">
          <span className="field__label">Mode</span>
          <select
            className="select"
            value={mode}
            aria-label="Target mode"
            disabled={disabled}
            onChange={(event) => changeMode(event.target.value === "auto" ? "auto" : "fixed")}
          >
            <option value="auto">Auto</option>
            <option value="fixed">Fixed</option>
          </select>
        </label>
      </div>

      <div className="target-advisor__advice">
        <span className="muted small">
          {advised === null ? "VOWP has no advice yet" : `VOWP advises ${advised}`}
        </span>
        <Badge tone={advice.tone}>{advice.label}</Badge>
      </div>

      <div className="target-advisor__stepper">
        <Button
          variant="secondary"
          size="sm"
          aria-label="Decrease worker target"
          disabled={disabled || current <= 0}
          onClick={() => commit(current - 1)}
        >
          −
        </Button>
        <label className="target-advisor__value">
          <span className="sr-only">Worker target</span>
          <input
            className="input"
            type="number"
            min={0}
            max={MAX_TARGET}
            step={1}
            value={draft}
            aria-label="Worker target"
            disabled={disabled}
            onChange={(event) => setDraftOverride(event.target.value)}
            onBlur={commitDraft}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                commitDraft();
              }
            }}
          />
        </label>
        <Button
          variant="secondary"
          size="sm"
          aria-label="Increase worker target"
          disabled={disabled || current >= MAX_TARGET}
          onClick={() => commit(current + 1)}
        >
          +
        </Button>
        <Button
          variant="primary"
          size="sm"
          disabled={!advice.canApply || disabled}
          title={advice.canApply ? undefined : "VOWP has no advice yet."}
          onClick={() => {
            if (advised !== null) {
              commit(advised);
            }
          }}
          data-testid="apply-advised"
        >
          {advice.applyLabel}
        </Button>
      </div>

      <p className="muted small" title={targetSourceTitle(capacity.target)}>
        Set by {capacity.target.set_by} · {formatDateTime(capacity.target.set_at)}.
      </p>
      <p className="muted small">
        VOWP still applies brakes and provider limits. A higher target is not a
        guarantee that runs start.
      </p>
      {!mutationsAllowed ? (
        <p className="muted small" data-testid="target-stale-reason">
          Refresh before changing the worker target.
        </p>
      ) : null}
      {error ? (
        <p className="notice notice--danger" role="alert">
          {error}
        </p>
      ) : null}

      <Modal
        open={overrideTarget !== null}
        title="Confirm owner override"
        onClose={cancelOverride}
        footer={
          <div className="inline-actions">
            <Button variant="ghost" size="sm" onClick={cancelOverride}>
              Keep {current}
            </Button>
            <Button
              variant="primary"
              size="sm"
              disabled={
                busy || overrideTarget === null || overrideReason.trim().length === 0
              }
              onClick={() => {
                if (overrideTarget !== null) {
                  void applyTarget(
                    overrideTarget,
                    mode,
                    true,
                    overrideReason.trim() || "Owner override from Fleet Canvas.",
                  );
                }
              }}
              data-testid="confirm-override"
            >
              Set {overrideTarget ?? current} above advice
            </Button>
          </div>
        }
      >
        <p className="small">
          {overrideTarget ?? current} is above VOWP&apos;s advised{" "}
          {advised ?? "unknown"} workers. VOWP requires an explicit owner override; it can
          still refuse on disk, RAM, CPU or provider limits.
        </p>
        <label className="field target-advisor__reason">
          <span className="field__label">Reason</span>
          <input
            className="input"
            value={overrideReason}
            maxLength={200}
            onChange={(event) => setOverrideReason(event.target.value)}
          />
          <span className="field__hint">1–200 characters. Never include a secret.</span>
        </label>
      </Modal>
    </Panel>
  );
}
