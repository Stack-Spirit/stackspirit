import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
} from "react";

import type { SpiritBridge } from "../bridge/SpiritBridge";
import type {
  EngineProvider,
  EngineStatus,
  KeyStatus,
  ModelChoice,
} from "../bridge/wire";
import { toSpiritError } from "../bridge/errors";
import { Button } from "../components/Button";
import { Modal } from "../components/Modal";
import { providerLabel } from "../domain/engine";
import { useEngine, useFleetStore } from "../state/fleetStore";
import { useConversations } from "./conversations";

interface ProviderRow {
  id: string;
  label: string;
  /** The engine's own connected fact, or a key in this Mac's keychain. */
  connected: boolean;
  modelCount: number;
  freeModelCount: number;
}

/**
 * The running engine's provider catalog. A stopped engine reports none, and the panel
 * says so instead of inventing rows.
 */
function catalogFromStatus(status: EngineStatus | null): EngineProvider[] {
  const raw = status?.providers;
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((entry) => {
    if (!entry || typeof entry.id !== "string" || !entry.id.trim()) return [];
    const id = entry.id.trim();
    return [
      {
        ...entry,
        id,
        label:
          typeof entry.label === "string" && entry.label.trim()
            ? entry.label.trim()
            : providerLabel(id),
      },
    ];
  });
}

async function refreshSetupModels(bridge: SpiritBridge): Promise<void> {
  // The chat adapter caches its usable models; re-attaching the setup session makes it
  // re-read them, so the composer picker reflects a key that was just saved or removed.
  const chat = bridge.chat;
  if (!chat || !useConversations.getState().views.setup) return;
  try {
    const view = await chat.attach("setup");
    useConversations.setState((state) => {
      const current = state.views.setup;
      if (!current) return {};
      return { views: { ...state.views, setup: { ...current, ...view } } };
    });
  } catch {
    // The next chat attach reads the list anyway; never fail a key change on it.
  }
}

function upsertKey(statuses: KeyStatus[], next: KeyStatus): KeyStatus[] {
  const rest = statuses.filter((status) => status.provider !== next.provider);
  return [...rest, next].sort((left, right) =>
    left.provider.localeCompare(right.provider),
  );
}

function savedStatus(
  statuses: KeyStatus[],
  provider: string,
): KeyStatus | null {
  const status = statuses.find((candidate) => candidate.provider === provider);
  return status && status.state !== "missing" ? status : null;
}

function keyTone(status: KeyStatus): string {
  if (status.state === "saved" && status.last_checked_at !== null) {
    return "nx-chip--server";
  }
  if (status.state === "invalid") return "nx-chip--warn";
  return "nx-chip--muted";
}

/** An honest chip: only a key the engine accepted reads as Verified. */
function keyLabel(status: KeyStatus): string {
  if (status.state === "saved") {
    return status.last_checked_at !== null ? "Verified" : "Saved";
  }
  if (status.state === "invalid") return "Invalid key";
  if (status.state === "unverified") return "Unverified";
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
/**
 * The badge above the provider list. An explicit selection wins; otherwise the first
 * verified usable model (an included free model first) is the effective default.
 * "No model" appears only when the runtime reports zero usable models.
 */
export function defaultModelLabel(
  selected: { label?: string; id: string } | null,
  models: ModelChoice[],
): string {
  if (selected) return `Default · ${selected.label ?? selected.id}`;
  const fallback =
    models.find((model) => model.access === "included") ?? models[0];
  return fallback ? `Default · ${fallback.label || fallback.id}` : "No model";
}

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

  const counts = new Map<string, { total: number; free: number }>();
  for (const model of models) {
    const count = counts.get(model.provider) ?? { total: 0, free: 0 };
    count.total += 1;
    if (model.access === "included") count.free += 1;
    counts.set(model.provider, count);
  }

  const keyed = new Set(
    keys
      .filter((status) => status.state !== "missing")
      .map((status) => status.provider),
  );
  const rows: ProviderRow[] = catalogFromStatus(engine).map((provider) => {
    const count = counts.get(provider.id);
    return {
      id: provider.id,
      label: provider.label,
      connected: provider.connected === true || keyed.has(provider.id),
      modelCount: Math.max(provider.model_count ?? 0, count?.total ?? 0),
      freeModelCount: Math.max(
        provider.free_model_count ?? 0,
        count?.free ?? 0,
      ),
    };
  });
  const connected = (provider: string): boolean =>
    provider === "opencode" ||
    keyed.has(provider) ||
    rows.some((row) => row.id === provider && row.connected);
  const catalogReady = rows.length > 0;
  const defaultModelBadge = defaultModelLabel(
    engine?.selected_model ?? null,
    models,
  );

  const featured: ProviderRow = rows.find(
    (provider) => provider.id === "opencode",
  ) ?? {
    id: "opencode",
    label: "OpenCode",
    connected: true,
    modelCount: 0,
    freeModelCount: 0,
  };
  const opencodeGo =
    rows.find((provider) => provider.id === "opencode-go") ?? null;

  const needle = query.trim().toLowerCase();
  const searchable = rows.filter(
    (provider) =>
      provider.id !== "opencode" &&
      provider.id !== "opencode-go" &&
      (needle === "" ||
        provider.id.toLowerCase().includes(needle) ||
        provider.label.toLowerCase().includes(needle)),
  );
  const connectedRows = searchable.filter((provider) => provider.connected);
  const availableRows = searchable.filter((provider) => !provider.connected);

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
      await refreshSetupModels(bridge);
      setEditing(null);
      if (verified.state === "saved") {
        setNotice(
          `${providerLabel(provider)} connected and verified. Its models are ready in the composer.`,
        );
      } else if (verified.state === "invalid") {
        // The key stays saved so the owner can verify, replace or remove it in place.
        setNotice(
          `${providerLabel(provider)} rejected this key. It is still saved; verify it again, replace it or remove it.`,
        );
      } else {
        setNotice(
          `${providerLabel(provider)} connected. The key is saved, but the engine could not verify it yet; use Verify key to retry.`,
        );
      }
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
      if (checked.state === "missing") {
        setError(
          `${providerLabel(provider)} key was not found in this Mac's private provider-key file. Enter it again to reconnect.`,
        );
      } else {
        setKeys((current) => upsertKey(current, checked));
        setNotice(
          checked.state === "saved"
            ? `${providerLabel(provider)} key verified.`
            : checked.state === "invalid"
              ? `${providerLabel(provider)} rejected this key. Replace or remove it.`
              : `${providerLabel(provider)} is saved, but the engine could not confirm it yet (network, rate limit or model access). The key was kept; try Verify key again.`,
        );
      }
      await refreshEngine();
      await reload();
      await refreshSetupModels(bridge);
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
      // The core's disconnect path already restarts a live bundled engine; refresh the
      // shell, this panel and the composer's model list from it.
      try {
        await refreshEngine();
        await reload();
        await refreshSetupModels(bridge);
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
            OpenCode free models are included. Connect a provider key to unlock
            its models in the composer.
          </small>
        </span>
        <span className="nx-chip nx-chip--muted">{defaultModelBadge}</span>
      </div>

      <div
        className="nx-provider nx-provider--featured"
        data-testid="provider-opencode"
      >
        <div className="nx-provider__head">
          <div className="nx-provider__title">
            <h3>{featured.label}</h3>
            <span className="nx-chip nx-chip--server">
              Free models included
            </span>
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
          Use the included free models on this Mac. Add an OpenCode API key for
          higher limits, or connect OpenCode Go with a Go/Zen account key.
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
              {connected("opencode-go")
                ? "Manage OpenCode Go"
                : "Connect OpenCode Go"}
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
              <span className={`nx-chip ${keyTone(status)}`}>
                {label}: {keyLabel(status)}
              </span>
              <button
                type="button"
                className="nx-btn nx-btn--quiet nx-btn--sm"
                onClick={() => void verify(provider)}
                disabled={busy === provider}
                aria-label={`Verify ${label} key`}
              >
                {busy === provider ? "Verifying…" : "Verify key"}
              </button>
              <button
                type="button"
                className="nx-btn nx-btn--quiet nx-btn--sm"
                onClick={() => startEditing(provider)}
                disabled={busy === provider}
                aria-label={`Replace ${label} key`}
              >
                Replace key
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

      <div
        className="nx-provider__section"
        aria-labelledby="nx-connected-providers"
      >
        <div className="nx-block__head">
          <h3 id="nx-connected-providers">Connected providers</h3>
          <span className="nx-provider__meta">
            {connectedRows.length === 0
              ? "No others yet"
              : `${connectedRows.length} connected`}
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
            No other providers are connected yet. OpenCode free models are
            always available.
          </p>
        )}
      </div>

      <div
        className="nx-provider__section"
        aria-labelledby="nx-connect-provider"
      >
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
            disabled={!catalogReady}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        {!catalogReady ? (
          <p className="nx-provider__empty">
            The StackSpirit Engine is stopped. Start it to load the live
            provider catalog.
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
          This removes only the{" "}
          {pendingRemove ? providerLabel(pendingRemove) : ""} key from this
          Mac&apos;s private key file and refreshes the engine model list. It does not
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
  const connected = provider.connected;
  return (
    <li className="nx-provider-row" data-testid={`provider-${provider.id}`}>
      <div className="nx-provider-row__head">
        <div className="nx-provider-row__name">
          <strong>{provider.label}</strong>
          <span
            className={
              connected ? "nx-chip nx-chip--server" : "nx-chip nx-chip--muted"
            }
          >
            {connected ? "Connected" : "Available"}
          </span>
          {status ? (
            <span className={`nx-chip ${keyTone(status)}`}>
              {keyLabel(status)}
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
                {busy ? "Verifying…" : "Verify key"}
              </button>
              <button
                type="button"
                className="nx-btn nx-btn--quiet nx-btn--sm"
                onClick={editing ? onCancel : onConnect}
                disabled={busy}
                aria-expanded={editing}
                aria-label={`Replace ${provider.label} key`}
              >
                Replace key
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
          Stored in a private file on this Mac; never sent to your server.
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
