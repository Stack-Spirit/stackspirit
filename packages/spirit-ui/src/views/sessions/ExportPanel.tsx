import { useState } from "react";

import type { SpiritBridge } from "../../bridge";
import { toSpiritError, type SpiritError } from "../../bridge/errors";
import type { AsifExportResult, SessionCardView } from "../../bridge/wire";
import { Badge, Button } from "../../components";
import { executorLabel } from "../../domain/executor";
import { formatExactBytes } from "../../domain/format";
import {
  DEFAULT_ASIF_LOSS_POLICY,
  asifErrorCopy,
  asifLossPolicyOptions,
  sessionStateLabel,
  sessionStateTone,
  sessionUpdated,
  type AsifLossPolicyChoice,
} from "../../domain/session";
import { AsifLossList, AsifUnsupported } from "./AsifReport";
import { LossPolicyFields } from "./LossPolicyFields";

export interface ExportPanelProps {
  bridge: SpiritBridge | null;
  session: SessionCardView;
  snapshotAt: string | null;
  onBack: () => void;
}

/**
 * One export flow. The bridge opens the native save dialog, so a `null` result means
 * the user cancelled and nothing was written. The full declared-loss list is shown after
 * VOWP writes the summary; no summary is claimed before VOWP produces one.
 */
export function ExportPanel({ bridge, session, snapshotAt, onBack }: ExportPanelProps) {
  const [policy, setPolicy] = useState<AsifLossPolicyChoice>(DEFAULT_ASIF_LOSS_POLICY);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<AsifExportResult | null>(null);
  const [error, setError] = useState<SpiritError | null>(null);
  const [cancelled, setCancelled] = useState(false);
  const updated = sessionUpdated(session, snapshotAt);

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
      const next = await api.exportAsif(session.id, asifLossPolicyOptions(policy));
      if (next === null) setCancelled(true);
      else setResult(next);
    } catch (caught) {
      setError(toSpiritError(caught));
    } finally {
      setBusy(false);
    }
  }

  const failure = error ? asifErrorCopy(error, "export") : null;

  return (
    <div className="asif-flow" data-testid="export-panel">
      <Button variant="ghost" size="sm" onClick={onBack} disabled={busy}>
        ← Back to sessions
      </Button>

      <div className="asif-flow__session">
        <div className="asif-flow__title">
          <h3 className="section-title">Export {session.title}</h3>
          <Badge tone={sessionStateTone(session.state)}>
            {sessionStateLabel(session.state)}
          </Badge>
        </div>
        <dl className="asif-meta">
          <div>
            <dt>Session</dt>
            <dd className="mono" data-testid="export-session-id">
              {session.id}
            </dd>
          </div>
          <div>
            <dt>Project</dt>
            <dd>{session.project ?? "No project"}</dd>
          </div>
          <div>
            <dt>Executor</dt>
            <dd>{executorLabel(session.executor)}</dd>
          </div>
          <div>
            <dt>Updated</dt>
            <dd title={updated.absolute}>{updated.relative}</dd>
          </div>
        </dl>
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
            direction="export"
            policy={policy}
            disabled={busy}
            onChange={setPolicy}
          />
          <p className="muted small">
            The native save dialog chooses the local file. VOWP writes a private package on
            the VPS and StackSpirit copies it over SSH after a size check.
          </p>
          <div className="inline-actions">
            <Button type="submit" variant="primary" loading={busy}>
              Export .asif.zip
            </Button>
            <Button variant="ghost" onClick={onBack} disabled={busy}>
              Cancel
            </Button>
          </div>
        </form>
      ) : null}

      {failure ? (
        <div className="asif-error" role="alert" data-testid="export-error">
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
        <p className="notice" role="status" data-testid="export-cancelled">
          Export cancelled. Nothing was saved.
        </p>
      ) : null}

      {result ? (
        <div className="asif-result" role="status" aria-live="polite" data-testid="export-result">
          <div className="asif-result__head">
            <Badge tone="ok">Exported</Badge>
            <span className="muted small">Local package ready</span>
          </div>
          <dl className="asif-meta">
            <div>
              <dt>Local file</dt>
              <dd className="mono" data-testid="export-file-name">
                {result.file_name}
              </dd>
            </div>
            <div>
              <dt>Size</dt>
              <dd>{formatExactBytes(result.bytes)}</dd>
            </div>
            <div>
              <dt>Session</dt>
              <dd className="mono">{result.session_id}</dd>
            </div>
            <div>
              <dt>Declared losses</dt>
              <dd>{result.loss_count}</dd>
            </div>
          </dl>
          <p className="muted small">
            Only the local file name is shown here. The remote path never reaches this
            window.
          </p>
          <AsifLossList
            direction="export"
            lossCount={result.loss_count}
            losses={result.losses}
          />
          <AsifUnsupported direction="export" unsupported={result.unsupported} />
          <p className="notice notice--warn">
            VOWP declared the losses above. The package is not an exact copy of the
            session, and StackSpirit never presents it as one.
          </p>
        </div>
      ) : null}
    </div>
  );
}
