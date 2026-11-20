import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";

import type { EngineStatus, KeyStatus, ModelChoice } from "../bridge/wire";
import { toSpiritError } from "../bridge/errors";
import { Button } from "../components/Button";
import { Modal } from "../components/Modal";
import { useEngine, useFleetStore } from "../state/fleetStore";

/**
 * The connect dialog's provider rows. The engine's `/provider` document is the live
 * catalog; the fallback list keeps the panel useful before the engine starts. Only
 * ids and labels are shown — never a key, endpoint or provider configuration.
 */
const FALLBACK_PROVIDERS: ReadonlyArray<{ id: string; label: string }> = [
  { id: "opencode", label: "OpenCode" },
  { id: "opencode-go", label: "OpenCode Go" },
  { id: "anthropic", label: "Anthropic" },
  { id: "openai", label: "OpenAI" },
  { id: "google", label: "Google" },
  { id: "openrouter", label: "OpenRouter" },
  { id: "deepseek", label: "DeepSeek" },
  { id: "xai", label: "xAI" },
  { id: "groq", label: "Groq" },
  { id: "mistral", label: "Mistral" },
  { id: "ollama", label: "Ollama" },
];

const KNOWN_LABELS: Record<string, string> = Object.fromEntries(
  FALLBACK_PROVIDERS.map((provider) => [provider.id, provider.label]),
);

interface ProviderRow {
  id: string;
  label: string;
  modelCount: number;
  freeModelCount: number;
}

function providerLabel(id: string): string {
  const known = KNOWN_LABELS[id];
  if (known) return known;
  const words = id.replaceAll(/[_-]+/g, " ").split(/\s+/).filter(Boolean);
  return (
    words
      .map((word) => `${word.charAt(0).toUpperCase()}${word.slice(1)}`)
      .join(" ") || id
  );
}

/** Reads the engine-supplied provider rows structurally; older bridges omit them. */
function catalogFromStatus(status: EngineStatus | null): Array<{ id: string; label: string }> {
  const raw = (status as { providers?: unknown } | null)?.providers;
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((entry) => {
    if (!entry || typeof entry !== "object") return [];
    const { id, label } = entry as { id?: unknown; label?: unknown };
    if (typeof id !== "string" || !id.trim()) return [];
    return [
      {
        id: id.trim(),
        label: typeof label === "string" && label.trim() ? label.trim() : providerLabel(id),
      },
    ];
  });
}

function upsertKey(statuses: KeyStatus[], next: KeyStatus): KeyStatus[] {
  const rest = statuses.filter((status) => status.provider !== next.provider);
  return [...rest, next].sort((left, right) => left.provider.localeCompare(right.provider));
}

function savedStatus(statuses: KeyStatus[], provider: string): KeyStatus | null {
  const status = statuses.find((candidate) => candidate.provider === provider);
  return status && status.state !== "missing" ? status : null;
}

function keyTone(state: KeyStatus["state"]): string {
  if (state === "saved") return "nx-chip--server";
  if (state === "invalid") return "nx-chip--warn";
  return "nx-chip--muted";
}

function keyLabel(state: KeyStatus["state"]): string {
  if (state === "saved") return "Saved";
  if (state === "invalid") return "Invalid key";
  if (state === "unverified") return "Unverified";
  return "Not saved";
}

function providerMeta(provider: ProviderRow): string {
  const models =
    provider.modelCount > 0
      ? `${provider.modelCount} model${provider.modelCount === 1 ? "" : "s"}`
      : null;
  const free =
    provider.freeModelCount > 0 ? `${provider.freeModelCount} free` : null;
  return [provider.id, free, models].filter(Boolean).join(" · ");
}

/**
 * Settings → Engine & Models.
 *
 * OpenCode is featured because its free models ship with the bundled engine. Every
 * other provider is grouped by whether the running engine (or the keychain) reports
 * it as connected, then shown as a searchable catalog. A key is write-only: it goes
 * to `bridge.engine.storeKey`, the field is cleared immediately, and no notice or
 * error ever includes it.
 */
export function SettingsPage() {
  const bridge = useFleetStore((state) => state.bridge);
  const engine = useEngine();
  const refreshEngine = useFleetStore((state) => state.refreshEngine);
  const [keys, setKeys] = useState<KeyStatus[]>([]);
  const [models, setModels] = useState<ModelChoice[]>([]);
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [pendingRemove, setPendingRemove] = useState<string | null>(null);

  const engineState = engine?.state.state ?? "unknown";

  const reload = useCallback(async () => {
    if (!bridge) return;
    const [statuses, engineModels] = await Promise.all([
      bridge.engine.keyStatus().catch(() => [] as KeyStatus[]),
      engineState === "ready"
        ? bridge.engine.models().catch(() => [] as ModelChoice[])
        : Promise.resolve([] as ModelChoice[]),
    ]);
    setKeys(statuses);
    setModels(engineModels);
  }, [bridge, engineState]);

  useEffect(() => {
    let active = true;
    void (async () => {
      if (!bridge) return;
      try {
        const [statuses, engineModels] = await Promise.all([
          bridge.engine.keyStatus(),
          engineState === "ready"
            ? bridge.engine.models()
            : Promise.resolve([] as ModelChoice[]),
        ]);
        if (!active) return;
        setKeys(statuses);
        setModels(engineModels);
      } catch (loadError) {
        if (active) setError(toSpiritError(loadError).message);
      }
    })();
    return () => {
      active = false;
    };
  }, [bridge, engineState]);

  const catalog = new Map<string, { id: string; label: string }>();
  for (const provider of FALLBACK_PROVIDERS) catalog.set(provider.id, provider);
  for (const provider of catalogFromStatus(engine)) catalog.set(provider.id, provider);

  const counts = new Map<string, { total: number; free: number }>();
  for (const model of models) {
    const count = counts.get(model.provider) ?? { total: 0, free: 0 };
    count.total += 1;
    if (model.access === "included") count.free += 1;
    counts.set(model.provider, count);
  }

  const keyed = new Set(
    keys.filter((status) => status.state !== "missing").map((status) => status.provider),
  );
  const modeled = new Set(models.map((model) => model.provider));
  const connected = (provider: string): boolean =>
    provider === "opencode" || keyed.has(provider) || modeled.has(provider);

  const rows: ProviderRow[] = [...catalog.values()].map((provider) => {
    const count = counts.get(provider.id);
    return {
      ...provider,
      modelCount: count?.total ?? 0,
      freeModelCount: count?.free ?? 0,
    };
  });
  const featured = rows.find((provider) => provider.id === "opencode") ?? {
    id: "opencode",
    label: "OpenCode",
    modelCount: 0,
    freeModelCount: 0,
  };
  const opencodeGo = rows.find((provider) => provider.id === "opencode-go") ?? null;

  const needle = query.trim().toLowerCase();
  const searchable = rows.filter(
    (provider) =>
      provider.id !== "opencode" &&
      provider.id !== "opencode-go" &&
      (needle === "" ||
        provider.id.toLowerCase().includes(needle) ||
        provider.label.toLowerCase().includes(needle)),
  );
  const connectedRows = searchable.filter((provider) => connected(provider.id));
  const availableRows = searchable.filter((provider) => !connected(provider.id));

  async function save(provider: string, secret: string): Promise<boolean> {
    if (!bridge) return false;
    setBusy(provider);
    setError(null);
    setNotice(null);
    try {
      await bridge.engine.storeKey(provider, secret);
      let verified: KeyStatus;
      try {
        verified = await bridge.engine.verifyKey(provider);
      } catch {
        verified = {
          provider,
          state: "unverified",
          last_checked_at: null,
        };
      }
      setKeys((current) => upsertKey(current, verified));
      await refreshEngine();
      await reload();
      setEditing(null);
      setNotice(
        verified.state === "saved"
          ? `${providerLabel(provider)} connected and verified. Its models are ready in the composer.`
          : `${providerLabel(provider)} connected. The engine could not verify the key yet; its models appear only when the engine accepts it.`,
      );
      return true;
    } catch (saveError) {
      const message = toSpiritError(saveError).message;
      setError(message.replaceAll(secret, "•••"));
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
      setPendingRemove(null);
      setNotice(`${providerLabel(provider)} key removed.`);
      // The desktop's delete path only touches the keychain; a live bundled sidecar
      // keeps its spawn-time auth until it restarts. Refresh it so the removed
      // provider's models disappear from the composer without an app restart. A
      // stopped engine is left stopped.
      const wasLive =
        engineState === "ready" ||
        engineState === "degraded" ||
        engineState === "failed";
      try {
        if (wasLive) await bridge.engine.restart();
        await refreshEngine();
        await reload();
      } catch {
        setNotice(
          `${providerLabel(provider)} key removed. Restart the engine to refresh its model list.`,
        );
      }
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
      className="nx-providers"
      aria-label="Engine & Models"
      data-testid="engine-models-settings"
    >
      <div className="nx-settings__row">
        <span>
          <strong>Providers</strong>
          <small>
            OpenCode free models are included. Connect a provider key to unlock its models
            in the composer.
          </small>
        </span>
        <span className="nx-chip nx-chip--muted">
          {engine?.selected_model
            ? providerLabel(engine.selected_model.provider)
            : "No model"}
        </span>
      </div>

      <div className="nx-provider nx-provider--featured" data-testid="provider-opencode">
        <div className="nx-provider__head">
          <div className="nx-provider__title">
            <h3>{featured.label}</h3>
            <span className="nx-chip nx-chip--server">Free models included</span>
          </div>
          <span className="nx-provider__meta">
            {featured.freeModelCount > 0
              ? `${featured.freeModelCount} free model${
                  featured.freeModelCount === 1 ? "" : "s"
                } verified at runtime`
              : "Availability is checked when the engine starts"}
          </span>
        </div>
        <p className="nx-provider__copy">
          Use the included free models on this Mac. Add an OpenCode API key for higher
          limits, or connect OpenCode Go with a Go/Zen account key.
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
              {connected("opencode-go") ? "Manage OpenCode Go" : "Connect OpenCode Go"}
            </button>
          ) : null}
          {engine?.selected_model ? (
            <span className="nx-provider__current">
              Current: {engine.selected_model.label}
            </span>
          ) : null}
        </div>
        {(["opencode", "opencode-go"] as const).map((provider) => {
          const status = savedStatus(keys, provider);
          if (!status) return null;
          const label = providerLabel(provider);
          return (
            <div className="nx-provider__actions" key={provider}>
              <span className={`nx-chip ${keyTone(status.state)}`}>
                {label}: {keyLabel(status.state)}
              </span>
              <button
                type="button"
                className="nx-btn nx-btn--quiet nx-btn--sm"
                onClick={() => void verify(provider)}
                disabled={busy === provider}
                aria-label={`Verify ${label} key`}
              >
                {busy === provider ? "Checking…" : "Verify key"}
              </button>
              <button
                type="button"
                className="nx-btn nx-btn--quiet nx-btn--sm"
                onClick={() => setPendingRemove(provider)}
                disabled={busy === provider}
                aria-label={`Remove ${label} key`}
              >
                Remove key
              </button>
            </div>
          );
        })}
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
            {connectedRows.length === 0 ? "No others yet" : `${connectedRows.length} connected`}
          </span>
        </div>
        {connectedRows.length > 0 ? (
          <ul className="nx-provider__list" aria-label="Connected providers">
            {connectedRows.map((provider) => (
              <ProviderItem
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
            No other providers are connected yet. OpenCode free models are always
            available.
          </p>
        )}
      </div>

      <div className="nx-provider__section" aria-labelledby="nx-connect-provider">
        <div className="nx-block__head">
          <h3 id="nx-connect-provider">Connect a provider</h3>
          <span className="nx-provider__meta">
            {availableRows.length} available
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
            disabled={rows.length === 0}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        {rows.length === 0 ? (
          <p className="nx-provider__empty">
            Start the StackSpirit Engine to load the live provider catalog.
          </p>
        ) : availableRows.length > 0 ? (
          <ul className="nx-provider__list" aria-label="Available providers">
            {availableRows.map((provider) => (
              <ProviderItem
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
            {needle
              ? connectedRows.length > 0
                ? "Every matching provider is already connected above."
                : `No provider matches “${query.trim()}”.`
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
            <Button variant="danger" onClick={() => void remove()} loading={busy !== null}>
              Remove key
            </Button>
          </>
        }
      >
        <p>
          This removes only the {pendingRemove ? providerLabel(pendingRemove) : ""} key
          from this Mac&apos;s keychain and refreshes the engine model list. It does not
          change a VOWP setting or a server secret.
        </p>
      </Modal>
    </section>
  );
}

function ProviderItem({
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
  provider: ProviderRow;
  status: KeyStatus | null;
  editing: boolean;
  busy: boolean;
  onConnect: () => void;
  onVerify: () => void;
  onRemove: () => void;
  onSave: (provider: string, secret: string) => Promise<boolean>;
  onCancel: () => void;
}) {
  const connected = status !== null || provider.modelCount > 0;
  return (
    <li className="nx-provider-row" data-testid={`provider-${provider.id}`}>
      <div className="nx-provider-row__head">
        <div className="nx-provider-row__name">
          <strong>{provider.label}</strong>
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
          {status ? (
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
          ) : connected ? (
            <span className="nx-provider-row__meta">
              Connected outside the keychain
            </span>
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
          label={provider.label}
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
    const input = keyRef.current;
    const secret = input?.value ?? "";
    if (input) input.value = "";
    if (!secret) {
      setFormError("Enter a provider key first.");
      return;
    }
    setFormError(null);
    try {
      const saved = await onSave(provider, secret);
      if (saved) onCancel();
    } finally {
      if (input) input.value = "";
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
