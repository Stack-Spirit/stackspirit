import { useState } from "react";

import type { SpiritBridge } from "../../bridge";
import { toSpiritError, type SpiritError } from "../../bridge/errors";
import type { AsifImportResult } from "../../bridge/wire";
import { Badge, Button } from "../../components";
import {
  DEFAULT_ASIF_LOSS_POLICY,
  asifErrorCopy,
  asifLossPolicyOptions,
  asifPackageLimitCopy,
  sessionStateLabel,
  sessionStateTone,
  type AsifLossPolicyChoice,
} from "../../domain/session";
import { AsifLossList, AsifUnsupported, AsifWarnings } from "./AsifReport";
import { LossPolicyFields } from "./LossPolicyFields";

export interface ImportPanelProps {
  bridge: SpiritBridge | null;
  onBack: () => void;
}

/**
 * One import flow. The bridge opens the native open dialog and validates the package
 * size before upload. A successful import is an idle session: this panel never exposes
 * a run, start, continue, or resume action.
 */
export function ImportPanel({ bridge, onBack }: ImportPanelProps) {
  const [policy, setPolicy] = useState<AsifLossPolicyChoice>(DEFAULT_ASIF_LOSS_POLICY);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<AsifImportResult | null>(null);
  const [error, setError] = useState<SpiritError | null>(null);
  const [cancelled, setCancelled] = useState(false);

  async function submit(): Promise<void> {
    if (busy) return;
    const api = bridge?.sessions;
    if (!api) {
      setError(
        toSpiritError({
          code: "gate",
          message: "ASIF sessions are not available in this build.",
          hint: "Use a desktop build with the D5 ASIF bridge.",
          retryable: false,
        }),
      );
      return;
    }
    setBusy(true);
    setResult(null);
    setError(null);
    setCancelled(false);
    try {
      const next = await api.importAsif(asifLossPolicyOptions(policy));
      if (next === null) setCancelled(true);
      else setResult(next);
    } catch (caught) {
      setError(toSpiritError(caught));
    } finally {
      setBusy(false);
    }
  }

  const failure = error ? asifErrorCopy(error, "import") : null;

  return (
    <div className="asif-flow" data-testid="import-panel">
      <Button variant="ghost" size="sm" onClick={onBack} disabled={busy}>
        ← Back to sessions
      </Button>

      <div>
        <h3 className="section-title">Import .asif.zip</h3>
        <p className="muted small">
          The host opens the native file picker, checks the local package size before the
          transfer, and sends it only to the connected VOWP home. The upload is never
          logged or indexed by StackSpirit.
        </p>
        <p className="muted small" data-testid="import-size-limit">
          {asifPackageLimitCopy()}
        </p>
      </div>

      {result === null ? (
        <form
          className="asif-flow__form"
          onSubmit={(event) => {
            event.preventDefault();
            void submit();
          }}
        >
          <LossPolicyFields
            direction="import"
            policy={policy}
            disabled={busy}
            onChange={setPolicy}
          />
          <p className="notice">
            Import always creates a new session. The new session is idle; StackSpirit
            never runs a turn, writes a capsule, creates a lease, or dispatches a job
            after import.
          </p>
          <div className="inline-actions">
            <Button type="submit" variant="primary" loading={busy}>
              Import .asif.zip
            </Button>
            <Button variant="ghost" onClick={onBack} disabled={busy}>
              Cancel
            </Button>
          </div>
        </form>
      ) : null}

      {failure ? (
        <div className="asif-error" role="alert" data-testid="import-error">
          <div className="asif-error__head">
            <Badge tone="danger">{failure.title}</Badge>
            <span className="mono small">{failure.code}</span>
          </div>
          <p className="small">{failure.detail}</p>
          <p className="muted small">{failure.next}</p>
          {failure.code === "refused" && policy === "strict" ? (
            <Button
              variant="secondary"
              size="sm"
              onClick={() => setPolicy("allow-loss")}
            >
              Choose allow-loss
            </Button>
          ) : null}
        </div>
      ) : null}

      {cancelled ? (
        <p className="notice" role="status" data-testid="import-cancelled">
          Import cancelled. Nothing was imported.
        </p>
      ) : null}

      {result ? (
        <div className="asif-result" role="status" aria-live="polite" data-testid="import-result">
          <div className="asif-result__head">
            <Badge tone="ok">Imported</Badge>
            <span className="muted small">New idle session</span>
          </div>
          <dl className="asif-meta">
            <div>
              <dt>New session</dt>
              <dd className="mono" data-testid="import-session-id">
                {result.session_id}
              </dd>
            </div>
            <div>
              <dt>State</dt>
              <dd data-testid="import-state">
                <Badge tone={sessionStateTone(result.state)}>
                  {sessionStateLabel(result.state)}
                </Badge>
              </dd>
            </div>
            <div>
              <dt>Declared losses</dt>
              <dd>{result.loss_count}</dd>
            </div>
          </dl>
          <p className="notice" data-testid="import-idle-note">
            The imported session is idle. StackSpirit did not run a turn, write a capsule,
            create a lease, or dispatch a job. There is no run or continue action here.
            The session appears in the list after the next VOWP snapshot.
          </p>
          <AsifLossList
            direction="import"
            lossCount={result.loss_count}
            losses={result.losses}
          />
          <AsifWarnings warnings={result.warnings} />
          <AsifUnsupported direction="import" unsupported={result.unsupported} />
          <p className="muted small">
            Only the new session id and the safe summary are shown. The uploaded file name
            and any staging path are never echoed back to this window.
          </p>
        </div>
      ) : null}
    </div>
  );
}
