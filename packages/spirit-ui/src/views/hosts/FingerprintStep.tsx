import { useId, useState } from "react";

import { Badge, Button, ErrorState } from "../../components";
import type { SpiritError } from "../../bridge/errors";
import type { HostKeyScan } from "../../bridge/wire";

export interface FingerprintStepProps {
  scan: HostKeyScan | null;
  scanning: boolean;
  scanError: SpiritError | null;
  selectedSha: string | null;
  verified: boolean;
  onSelect: (sha256: string) => void;
  onVerified: (checked: boolean) => void;
  onRescan: () => void;
  onBack?: () => void;
  onTrust: () => void;
  trustLabel?: string;
  busy?: boolean;
}

export function FingerprintStep({
  scan,
  scanning,
  scanError,
  selectedSha,
  verified,
  onSelect,
  onVerified,
  onRescan,
  onBack,
  onTrust,
  trustLabel = "Trust and continue",
  busy = false,
}: FingerprintStepProps) {
  const checkboxId = useId();
  const [copied, setCopied] = useState(false);
  const selected = scan?.keys.find((key) => key.sha256 === selectedSha) ?? null;

  async function copyFingerprint() {
    if (!selected) return;
    try {
      await navigator.clipboard?.writeText(selected.sha256);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  }

  return (
    <div className="fingerprint-step">
      <p className="muted">
        StackSpirit reads every host key the server offers. Compare one out of band
        before trusting it.
      </p>

      {scanning ? (
        <div className="inline-status" role="status" aria-live="polite">
          Scanning host keys with ssh-keyscan…
        </div>
      ) : null}

      {scanError ? (
        <ErrorState error={scanError} onRetry={onRescan} retryLabel="Retry scan" />
      ) : null}

      {!scanning && !scanError && scan ? (
        <>
          <ul className="key-list" aria-label="Offered host keys">
            {scan.keys.map((key) => {
              const id = `key-${key.sha256}`;
              return (
                <li key={key.sha256} className="key-list__item">
                  <label htmlFor={id} className="key-list__label">
                    <input
                      id={id}
                      type="radio"
                      name="host-key"
                      checked={selectedSha === key.sha256}
                      onChange={() => onSelect(key.sha256)}
                    />
                    <span className="key-list__body">
                      <span className="key-list__algorithm mono">{key.algorithm}</span>
                      {key.preferred ? <Badge tone="accent">preferred</Badge> : null}
                      <span className="key-list__fingerprint mono">{key.sha256}</span>
                    </span>
                  </label>
                </li>
              );
            })}
          </ul>

          <div className="fingerprint-step__compare">
            <p>
              Compare this with{" "}
              <code className="mono">ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</code>{" "}
              on your server. StackSpirit will refuse to connect if this key changes.
            </p>
            <div className="fingerprint-step__copy">
              <code className="mono">{selected?.sha256 ?? "Choose a key"}</code>
              <Button variant="ghost" size="sm" onClick={() => void copyFingerprint()}>
                {copied ? "Copied" : "Copy"}
              </Button>
            </div>
          </div>

          <div className="checkbox-row">
            <input
              id={checkboxId}
              type="checkbox"
              checked={verified}
              onChange={(event) => onVerified(event.target.checked)}
            />
            <label htmlFor={checkboxId}>I verified this fingerprint on my server</label>
          </div>

          <div className="fingerprint-step__hint muted">
            Multiple keys are shown separately. StackSpirit never merges different key
            sets silently.
          </div>
        </>
      ) : null}

      <div className="modal__actions">
        {onBack ? (
          <Button variant="secondary" onClick={onBack} disabled={busy}>
            Back
          </Button>
        ) : null}
        <Button variant="ghost" onClick={onRescan} disabled={busy}>
          {scan ? "Scan again" : "Scan host key"}
        </Button>
        <Button
          variant="primary"
          onClick={onTrust}
          disabled={!scan || !selected || !verified || scanning || Boolean(scanError) || busy}
          loading={busy}
        >
          {trustLabel}
        </Button>
      </div>
    </div>
  );
}
