import { useEffect, useRef, useState, type FormEvent } from "react";

import type { KeyStatus } from "../bridge/wire";
import { toSpiritError } from "../bridge/errors";
import {
  engineProviders,
  modelProviderLabel,
  providerGroups,
  providerLabel,
  type EngineProvider,
} from "../domain/engine";
import { useEngine, useFleetStore } from "../state/fleetStore";
import { Modal } from "../components/Modal";
import { Button } from "../components/Button";

function upsertKey(statuses: KeyStatus[], next: KeyStatus): KeyStatus[] {
  const rest = statuses.filter((status) => status.provider !== next.provider);
  return [...rest, next].sort((left, right) => left.provider.localeCompare(right.provider));
}

/** A missing key is shown as no saved status, not as a "Not saved" chip. */
function savedStatus(statuses: KeyStatus[], provider: string): KeyStatus | null {
  const status = statuses.find((candidate) => candidate.provider === provider);
  return status && status.state !== "missing" ? status : null;
}

function keyTone(state: KeyStatus["state"]): string {
  if (state === "saved") return "nx-chip--server";
  if (state === "invalid") return "nx-chip--warn";
  if (state === "unverified") return "nx-chip--muted";
  return "nx-chip--muted";
}

function keyLabel(state: KeyStatus["state"]): string {
  if (state === "saved") return "Saved";
  if (state === "invalid") return "Invalid key";
  if (state === "unverified") return "Unverified";
  return "Not saved";
}

function providerMeta(provider: EngineProvider): string {
  const models =
    provider.model_count > 0
      ? `${provider.model_count} model${provider.model_count === 1 ? "" : "s"}`
      : null;
  const free =
    provider.free_model_count > 0
      ? `${provider.free_model_count} free`
      : null;
  return [provider.id, free, models].filter(Boolean).join(" · ");
}

/**
 * Settings → Engine & Models.
 *
 * OpenCode is the featured provider because its free models ship with the bundled
 * engine. Every other provider comes from the live engine catalog: connected ones
 * first, then a searchable list of the rest. A key is write-only here: it goes to
 * `bridge.engine.storeKey`, the field is cleared immediately, and no error or
 * rendered value ever includes it.
 */
export function EngineModelsSettings() {
  const bridge = useFleetStore((state) => state.bridge);
  const engine = useEngine();
  const refreshEngine = useFleetStore((state) => state.refreshEngine);
  const providers = engineProviders(engine);
  const { opencode, opencodeGo, connected } = providerGroups(providers);
  const [keys, setKeys] = useState<KeyStatus[]>([]);
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [pendingRemove, setPendingRemove] = useState<string | null>(null);

  useEffect(() => {
    if (!bridge?.engine?.keyStatus) return;
    let active = true;
    void bridge.engine
      .keyStatus()
      .then((statuses) => {
        if (active) setKeys(statuses);
      })
      .catch((keyError: unknown) => {
        if (active) setError(toSpiritError(keyError).message);
      });
    return () => {
      active = false;
    };
  }, [bridge, engine]);

  const search = providerGroups(providers, query);

  async function save(provider: string, secret: string): Promise<boolean> {
    if (!bridge) return false;
    setBusy(provider);
    setError(null);
    setNotice(null);
    try {
      await bridge.engine.storeKey(provider, secret);
      const verified = await bridge.engine.verifyKey(provider);
      setKeys((current) => upsertKey(current, verified));
      await refreshEngine();
      setEditing(null);
      setNotice(
        verified.state === "saved"
          ? `${providerLabel(provider)} connected and verified. Its models are ready in the composer.`
          : `${providerLabel(provider)} connected. The engine could not verify the key yet; its models appear only when the engine accepts it.`,
      );
      return true;
    } catch (saveError) {
      setError(toSpiritError(saveError).message);
      return false;
    } finally {
      setBusy(null);
    }
  }

  async function verify(provider: string) {
    if (!bridge) return;
    setBusy(provider);
    setError(null);
    setNotice(null);
    try {
      const checked = await bridge.engine.verifyKey(provider);
      setKeys((current) => upsertKey(current, checked));
      setNotice(
        checked.state === "saved"
          ? `${providerLabel(provider)} key verified.`
          : checked.state === "invalid"
            ? `${providerLabel(provider)} rejected this key. Replace or remove it.`
            : `${providerLabel(provider)} is still unverified.`,
      );
    } catch (verifyError) {
      setError(toSpiritError(verifyError).message);
    } finally {
      setBusy(null);
    }
  }

  async function remove() {
    if (!bridge || !pendingRemove) return;
    const provider = pendingRemove;
    setBusy(provider);
    setError(null);
    setNotice(null);
    try {
      const removed = await bridge.engine.deleteKey(provider);
      setKeys((current) => upsertKey(current, removed));
      await refreshEngine();
      setPendingRemove(null);
      setNotice(`${providerLabel(provider)} key removed. Its models are no longer offered.`);
    } catch (removeError) {
      setError(toSpiritError(removeError).message);
    } finally {
      setBusy(null);
    }
  }

  function startEditing(provider: string) {
    setEditing(provider);
    setError(null);
    setNotice(null);
  }

  return (
    <section
      className="nx-settings__card nx-providers"
      aria-labelledby="nx-engine-models-title"
      data-testid="engine-models-settings"
    >
      <div className="nx-settings__row">
        <span>
          <h2 id="nx-engine-models-title">Engine &amp; Models</h2>
          <small>
            OpenCode free models are included. Connect a provider key to unlock its models in
            the composer.
          </small>
        </span>
        <span className="nx-chip nx-chip--muted">
          {engine?.selected_model ? modelProviderLabel(engine.selected_model.provider) : "No model"}
        </span>
      </div>

      <div className="nx-provider nx-provider--featured" data-testid="provider-opencode">
        <div className="nx-provider__head">
          <div className="nx-provider__title">
            <h3>{opencode?.label ?? "OpenCode"}</h3>
            <span className="nx-chip nx-chip--server">Free models included</span>
          </div>
          <span className="nx-provider__meta">
            {opencode && opencode.free_model_count > 0
              ? `${opencode.free_model_count} free model${opencode.free_model_count === 1 ? "" : "s"} verified at runtime`
              : "Availability is checked when the engine starts"}
          </span>
        </div>
        <p className="nx-provider__copy">
          Use the included free models on this Mac. Add an OpenCode API key for higher limits,
          or connect OpenCode Go with a Go/Zen account key.
        </p>
        <div className="nx-provider__actions">
          <button
            type="button"
            className="nx-btn nx-btn--quiet nx-btn--sm"
            onClick={() => startEditing("opencode")}
            aria-expanded={editing === "opencode"}
          >
            Add OpenCode key
          </button>
          {opencodeGo ? (
            <button
              type="button"
              className="nx-btn nx-btn--quiet nx-btn--sm"
              onClick={() => startEditing("opencode-go")}
              aria-expanded={editing === "opencode-go"}
            >
              {opencodeGo.connected ? "Manage OpenCode Go" : "Connect OpenCode Go"}
            </button>
          ) : null}
          {engine?.selected_model ? (
            <span className="nx-provider__current">
              Current: {engine.selected_model.label}
            </span>
          ) : null}
        </div>
        {editing === "opencode" || editing === "opencode-go" ? (
          <ProviderKeyForm
            provider={editing}
            label={editing === "opencode-go" ? "OpenCode Go" : "OpenCode"}
            busy={busy === editing}
            onSave={save}
            onCancel={() => setEditing(null)}
          />
        ) : null}
      </div>

      <div className="nx-provider__section" aria-labelledby="nx-connected-providers">
        <div className="nx-block__head">
          <h3 id="nx-connected-providers">Connected providers</h3>
          <span className="nx-provider__meta">
            {connected.length === 0 ? "No others yet" : `${connected.length} connected`}
          </span>
        </div>
        {connected.length > 0 ? (
          <ul className="nx-provider__list" aria-label="Connected providers">
            {connected.map((provider) => (
              <ProviderRow
                key={provider.id}
                provider={provider}
                status={savedStatus(keys, provider.id)}
                editing={editing === provider.id}
                busy={busy === provider.id}
                onConnect={() => startEditing(provider.id)}
                onVerify={() => void verify(provider.id)}
                onRemove={() => setPendingRemove(provider.id)}
                onSave={save}
                onCancel={() => setEditing(null)}
              />
            ))}
          </ul>
        ) : (
          <p className="nx-provider__empty">
            No other providers are connected yet. OpenCode free models are always available.
          </p>
        )}
      </div>

      <div className="nx-provider__section" aria-labelledby="nx-connect-provider">
        <div className="nx-block__head">
          <h3 id="nx-connect-provider">Connect a provider</h3>
          <span className="nx-provider__meta">
            {search.available.length} available
          </span>
        </div>
        <label className="nx-field">
          <span className="nx-sr">Search providers</span>
          <input
            className="nx-search"
            type="search"
            value={query}
            placeholder="Search providers"
            aria-label="Search providers"
            disabled={providers.length === 0}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        {providers.length === 0 ? (
          <p className="nx-provider__empty">
            Start the StackSpirit Engine to load the live provider catalog.
          </p>
        ) : search.available.length > 0 ? (
          <ul className="nx-provider__list" aria-label="Available providers">
            {search.available.map((provider) => (
              <ProviderRow
                key={provider.id}
                provider={provider}
                status={savedStatus(keys, provider.id)}
                editing={editing === provider.id}
                busy={busy === provider.id}
                onConnect={() => startEditing(provider.id)}
                onVerify={() => void verify(provider.id)}
                onRemove={() => setPendingRemove(provider.id)}
                onSave={save}
                onCancel={() => setEditing(null)}
              />
            ))}
          </ul>
        ) : (
          <p className="nx-provider__empty">
            {query.trim()
              ? `No provider matches “${query.trim()}”.`
              : "Every provider in this engine is already connected."}
          </p>
        )}
      </div>

      {notice ? (
        <p className="nx-provider__notice" role="status">
          {notice}
        </p>
      ) : null}
      {error ? (
        <p className="nx-provider__error" role="alert">
          {error}
        </p>
      ) : null}

      <Modal
        open={pendingRemove !== null}
        title="Remove provider key?"
        onClose={() => setPendingRemove(null)}
        danger
        footer={
          <>
            <Button
              variant="ghost"
              onClick={() => setPendingRemove(null)}
              disabled={busy !== null}
            >
              Cancel
            </Button>
            <Button
              variant="danger"
              onClick={() => void remove()}
              loading={busy !== null}
            >
              Remove key
            </Button>
          </>
        }
      >
        <p>
          This removes only the {pendingRemove ? providerLabel(pendingRemove) : ""} key from
          this Mac&apos;s keychain and refreshes the engine model list. It does not change a
          VOWP setting or a server secret.
        </p>
      </Modal>
    </section>
  );
}

function ProviderRow({
  provider,
  status,
  editing,
  busy,
  onConnect,
  onVerify,
  onRemove,
  onSave,
  onCancel,
}: {
  provider: EngineProvider;
  status: KeyStatus | null;
  editing: boolean;
  busy: boolean;
  onConnect: () => void;
  onVerify: () => void;
  onRemove: () => void;
  onSave: (provider: string, secret: string) => Promise<boolean>;
  onCancel: () => void;
}) {
  const connected = provider.connected;
  return (
    <li className="nx-provider-row" data-testid={`provider-${provider.id}`}>
      <div className="nx-provider-row__head">
        <div className="nx-provider-row__name">
          <strong>{provider.label || providerLabel(provider.id)}</strong>
          <span className={connected ? "nx-chip nx-chip--server" : "nx-chip nx-chip--muted"}>
            {connected ? "Connected" : "Available"}
          </span>
          {status ? (
            <span className={`nx-chip ${keyTone(status.state)}`}>
              {keyLabel(status.state)}
            </span>
          ) : null}
        </div>
        <span className="nx-provider-row__meta">{providerMeta(provider)}</span>
        <div className="nx-provider-row__actions">
          {connected ? (
            status ? (
              <>
                <button
                  type="button"
                  className="nx-btn nx-btn--quiet nx-btn--sm"
                  onClick={onVerify}
                  disabled={busy}
                >
                  {busy ? "Checking…" : "Verify key"}
                </button>
                <button
                  type="button"
                  className="nx-btn nx-btn--quiet nx-btn--sm"
                  onClick={onRemove}
                  disabled={busy}
                  aria-label={`Remove ${provider.label} key`}
                >
                  Remove key
                </button>
              </>
            ) : (
              <span className="nx-provider-row__meta">Connected outside the keychain</span>
            )
          ) : (
            <button
              type="button"
              className="nx-btn nx-btn--sm"
              onClick={editing ? onCancel : onConnect}
              aria-expanded={editing}
              aria-label={`Connect ${provider.label}`}
            >
              {editing ? "Cancel" : "Connect"}
            </button>
          )}
        </div>
      </div>
      {editing ? (
        <ProviderKeyForm
          provider={provider.id}
          label={provider.label || providerLabel(provider.id)}
          busy={busy}
          onSave={onSave}
          onCancel={onCancel}
        />
      ) : null}
    </li>
  );
}

function ProviderKeyForm({
  provider,
  label,
  busy,
  onSave,
  onCancel,
}: {
  provider: string;
  label: string;
  busy: boolean;
  onSave: (provider: string, secret: string) => Promise<boolean>;
  onCancel: () => void;
}) {
  const keyRef = useRef<HTMLInputElement>(null);
  const [formError, setFormError] = useState<string | null>(null);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const secret = keyRef.current?.value ?? "";
    if (!secret) {
      setFormError("Enter a provider key first.");
      return;
    }
    setFormError(null);
    try {
      const saved = await onSave(provider, secret);
      if (saved) onCancel();
    } finally {
      if (keyRef.current) keyRef.current.value = "";
    }
  }

  return (
    <form className="nx-provider-form" onSubmit={(event) => void submit(event)}>
      <label className="nx-field">
        <span>API key for {label}</span>
        <input
          ref={keyRef}
          type="password"
          name={`provider-key-${provider}`}
          autoComplete="new-password"
          spellCheck={false}
          aria-label={`API key for ${label}`}
          placeholder="Paste the provider key"
        />
      </label>
      <div className="nx-provider-form__actions">
        <button type="submit" className="nx-btn nx-btn--sm" disabled={busy}>
          {busy ? "Saving…" : "Save key"}
        </button>
        <button
          type="button"
          className="nx-btn nx-btn--quiet nx-btn--sm"
          onClick={onCancel}
          disabled={busy}
        >
          Cancel
        </button>
        <span className="nx-provider__meta">
          Stored in this Mac&apos;s keychain; never sent to your server.
        </span>
      </div>
      {formError ? (
        <p className="nx-provider__error" role="alert">
          {formError}
        </p>
      ) : null}
    </form>
  );
}
