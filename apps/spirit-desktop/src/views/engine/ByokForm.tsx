import { useRef, useState, type FormEvent } from "react";

import { Badge, Button, Modal } from "../../components";
import type { KeyStatus } from "../../bridge/wire";
import { toSpiritError } from "../../bridge/errors";
import { providerLabel } from "../../domain/engine";

export interface ByokFormProps {
  providers: Array<{ id: string; label: string }>;
  statuses: KeyStatus[];
  onSave: (provider: string, secret: string) => Promise<KeyStatus>;
  onVerify: (provider: string) => Promise<KeyStatus>;
  onDelete: (provider: string) => Promise<KeyStatus>;
}

function statusTone(state: KeyStatus["state"]): "ok" | "warn" | "danger" | "unknown" {
  switch (state) {
    case "saved":
      return "ok";
    case "unverified":
      return "warn";
    case "invalid":
      return "danger";
    default:
      return "unknown";
  }
}

function statusLabel(state: KeyStatus["state"]): string {
  switch (state) {
    case "saved":
      return "Saved";
    case "unverified":
      return "Unverified";
    case "invalid":
      return "Invalid";
    default:
      return "Not saved";
  }
}

function resultMessage(status: KeyStatus): string {
  switch (status.state) {
    case "saved":
      return "Saved to this Mac's keychain.";
    case "invalid":
      return "Saved, but the engine rejected this key. Replace or delete it.";
    case "unverified":
      return "Saved. The local engine could not verify this key yet.";
    default:
      return "No key is stored for this provider.";
  }
}

export function ByokForm({
  providers,
  statuses,
  onSave,
  onVerify,
  onDelete,
}: ByokFormProps) {
  const keyRef = useRef<HTMLInputElement>(null);
  const [provider, setProvider] = useState(providers[0]?.id ?? "deepseek");
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);
  const current = statuses.find((status) => status.provider === provider);
  const saved = statuses.filter((status) => status.state !== "missing");

  function clearKeyField() {
    if (keyRef.current) keyRef.current.value = "";
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const secret = keyRef.current?.value ?? "";
    if (!secret) {
      setError("Enter a provider key first.");
      return;
    }
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      await onSave(provider, secret);
      clearKeyField();
      try {
        const verified = await onVerify(provider);
        setMessage(resultMessage(verified));
      } catch (verifyError) {
        setMessage(
          `Saved, but verification failed: ${toSpiritError(verifyError).message}`,
        );
      }
    } catch (saveError) {
      setError(toSpiritError(saveError).message);
    } finally {
      clearKeyField();
      setSaving(false);
    }
  }

  async function confirmDelete() {
    if (!pendingDelete) return;
    const providerToDelete = pendingDelete;
    setSaving(true);
    setError(null);
    try {
      const status = await onDelete(providerToDelete);
      setMessage(resultMessage(status));
      setPendingDelete(null);
    } catch (deleteError) {
      setError(toSpiritError(deleteError).message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="engine-section">
      <form className="byok-form" onSubmit={(event) => void submit(event)}>
        <div className="byok-form__row">
          <label className="field">
            <span>Provider</span>
            <select
              className="select"
              value={provider}
              onChange={(event) => {
                setProvider(event.target.value);
                setMessage(null);
                setError(null);
              }}
            >
              {providers.map((option) => (
                <option key={option.id} value={option.id}>
                  {option.label || providerLabel(option.id)}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>API key</span>
            <input
              ref={keyRef}
              className="input"
              type="password"
              name="provider-key"
              autoComplete="new-password"
              spellCheck={false}
              aria-label="API key"
              placeholder="Paste the provider key"
            />
          </label>
          <Button type="submit" variant="primary" size="sm" loading={saving}>
            Save
          </Button>
        </div>
        <div className="engine-actions">
          {current ? (
            <Badge tone={statusTone(current.state)}>{statusLabel(current.state)}</Badge>
          ) : (
            <Badge tone="unknown">Not saved</Badge>
          )}
          <span className="muted small">
            Stored in this Mac's keychain. Never sent to your VPS.
          </span>
        </div>
        {message ? <p className="muted small">{message}</p> : null}
        {error ? <p className="engine-error">{error}</p> : null}
      </form>

      {saved.length > 0 ? (
        <ul className="byok-keys" aria-label="Saved provider keys">
          {saved.map((status) => (
            <li key={status.provider} className="byok-key">
              <span>
                {providerLabel(status.provider)}{" "}
                <Badge tone={statusTone(status.state)}>{statusLabel(status.state)}</Badge>
              </span>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setPendingDelete(status.provider)}
                aria-label={`Delete ${providerLabel(status.provider)} key`}
                disabled={saving}
              >
                Delete key
              </Button>
            </li>
          ))}
        </ul>
      ) : null}

      <p className="muted small">
        Keys never leave this Mac. Only provider and model names cross the SSH boundary, and
        only after a VOWP profile or local orchestrator is configured.
      </p>

      <Modal
        open={pendingDelete !== null}
        title="Delete provider key?"
        onClose={() => setPendingDelete(null)}
        danger
        footer={
          <>
            <Button variant="ghost" onClick={() => setPendingDelete(null)} disabled={saving}>
              Cancel
            </Button>
            <Button
              variant="danger"
              onClick={() => void confirmDelete()}
              loading={saving}
            >
              Delete key
            </Button>
          </>
        }
      >
        <p>
          This removes only the {pendingDelete ? providerLabel(pendingDelete) : ""} key from
          this Mac's keychain. No VOWP setting changes.
        </p>
      </Modal>
    </div>
  );
}
