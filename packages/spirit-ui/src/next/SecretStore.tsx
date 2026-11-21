import { useCallback, useEffect, useId, useState, type FormEvent } from "react";

import { errorTitle, toSpiritError, type SpiritError } from "../bridge/errors";
import type { SecretEntryView, SecretListView } from "../bridge/wire";
import { formatDateTime } from "../domain/format";
import { useConnection, useFleetStore, useHosts, useSelectedHostId } from "../state/fleetStore";

/**
 * The VOWP secret-name grammar. Core validates the same expression; this copy only
 * enables or disables the form before a round trip.
 */
const SECRET_NAME = /^[A-Z][A-Z0-9_]{0,63}$/;

/**
 * App-session acknowledgement, keyed by the bridge instance. This survives closing and
 * reopening Settings for the same app run, but is never written to localStorage and
 * disappears when the app (or the bridge) is recreated.
 */
const acknowledgedSessions = new WeakSet<object>();
const storedSessions = new WeakSet<object>();

interface BlockedSet {
  message: string;
  hint: string | null;
}

interface RowFailure {
  name: string;
  error: SpiritError;
}

/**
 * A host or bridge may report that this build cannot store secrets. Core's normal
 * `ss-34` path drives VOWP's hidden prompt, so this is a defensive fallback: only a
 * typed "not supported" error changes the form state; an ordinary failure stays an
 * ordinary error and leaves the form usable.
 */
function unsupportedSet(error: SpiritError): BlockedSet | null {
  if (
    error.code !== "gate" &&
    error.code !== "vowp_incompatible" &&
    error.code !== "not_supported"
  ) {
    return null;
  }
  if (!/\b(not supported|unsupported|cannot store)\b/i.test(error.message)) return null;
  return { message: error.message, hint: error.hint };
}

/**
 * Settings card over VOWP's server secret vault.
 *
 * The value is write-only. It lives in this form until submit, is cleared from React
 * state before the first await, and is never rendered, logged or interpolated into an
 * error. The card only ever receives names, the last four characters and timestamps.
 */
export function SecretStore() {
  const bridge = useFleetStore((state) => state.bridge);
  const connection = useConnection();
  const hosts = useHosts();
  const selectedHostId = useSelectedHostId();
  const secrets = bridge?.secrets ?? null;
  const connected = connection.state === "connected";
  const serverLabel =
    hosts.find((host) => host.id === (connection.host_id ?? selectedHostId))?.label ??
    "the connected server";

  const [entries, setEntries] = useState<SecretListView | null>(null);
  const [entriesHostId, setEntriesHostId] = useState<string | null>(null);
  const [listError, setListError] = useState<SpiritError | null>(null);
  const [errorHostId, setErrorHostId] = useState<string | null>(null);
  const [listing, setListing] = useState(false);
  const [acknowledged, setAcknowledged] = useState(
    () => bridge !== null && acknowledgedSessions.has(bridge),
  );
  const [firstSaveDone, setFirstSaveDone] = useState(
    () => bridge !== null && storedSessions.has(bridge),
  );
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<{ name: string; last4: string } | null>(null);
  const [setError, setSetError] = useState<SpiritError | null>(null);
  const [blockedSet, setBlockedSet] = useState<BlockedSet | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [rowFailure, setRowFailure] = useState<RowFailure | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const ids = { name: useId(), value: useId() };

  const hostId = connection.host_id;

  useEffect(() => {
    if (!secrets || !connected) return;
    let disposed = false;
    void secrets.list().then(
      (next) => {
        if (disposed) return;
        setEntries(next);
        setEntriesHostId(hostId);
        setListError(null);
        setErrorHostId(null);
      },
      (caught: unknown) => {
        if (disposed) return;
        setListError(toSpiritError(caught));
        setErrorHostId(hostId);
      },
    );
    return () => {
      disposed = true;
    };
  }, [secrets, connected, hostId]);

  /** Re-reads the stored metadata and reports whether the refresh succeeded. */
  const refreshList = useCallback(async (): Promise<boolean> => {
    if (!secrets || !connected) return false;
    setListing(true);
    try {
      setEntries(await secrets.list());
      setEntriesHostId(hostId);
      setListError(null);
      setErrorHostId(null);
      return true;
    } catch (caught) {
      setListError(toSpiritError(caught));
      setErrorHostId(hostId);
      return false;
    } finally {
      setListing(false);
    }
  }, [secrets, connected, hostId]);

  const missingSet: BlockedSet | null =
    secrets && typeof secrets.set !== "function"
      ? {
          message: "This StackSpirit build does not expose an action that stores server secrets.",
          hint: null,
        }
      : null;
  const blockedReason = blockedSet ?? missingSet;
  const visibleEntries =
    connected && secrets && entriesHostId === hostId ? entries : null;
  const visibleListError =
    connected && secrets && errorHostId === hostId ? listError : null;
  const initialLoading =
    connected && secrets && visibleEntries === null && visibleListError === null;
  const nameValid = SECRET_NAME.test(name);
  const formDisabled = !connected || !secrets || blockedReason !== null || saving;
  const saveDisabled =
    formDisabled || !acknowledged || !nameValid || value === "";
  const revokeDisabled = !connected || !secrets || removing !== null;

  async function storeSecret(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!secrets || !connected || saving || blockedReason || !acknowledged) return;
    if (!nameValid || value === "") return;
    if (typeof secrets.set !== "function") {
      setBlockedSet({
        message: "This StackSpirit build does not expose an action that stores server secrets.",
        hint: null,
      });
      return;
    }

    // Read the value once, then clear it from React state before the first await.
    // From here on no render, snapshot, log or error can contain it.
    const secretName = name;
    const secretValue = value;
    setValue("");
    setSetError(null);
    setSaved(null);
    setNotice(null);
    setSaving(true);
    try {
      const outcome = await secrets.set({ name: secretName, value: secretValue });
      setFirstSaveDone(true);
      setAcknowledged(true);
      if (bridge) {
        acknowledgedSessions.add(bridge);
        storedSessions.add(bridge);
      }
      setName("");
      setSaved({ name: outcome.name, last4: outcome.last4 });
      await refreshList();
    } catch (caught) {
      const error = toSpiritError(caught);
      const unsupported = unsupportedSet(error);
      if (unsupported) setBlockedSet(unsupported);
      else setSetError(error);
    } finally {
      setSaving(false);
    }
  }

  async function revoke(secretName: string) {
    if (!secrets || !connected || removing) return;
    setRemoving(secretName);
    setRowFailure(null);
    setNotice(null);
    try {
      const outcome = await secrets.remove(secretName);
      setConfirming(null);
      if (outcome.removed) {
        setNotice(`Removed ${secretName}.`);
      } else {
        setNotice(`VOWP did not report ${secretName} as removed. The list was refreshed.`);
      }
      await refreshList();
    } catch (caught) {
      const error = toSpiritError(caught);
      if (error.code === "not_found") {
        setConfirming(null);
        const refreshed = await refreshList();
        setNotice(
          refreshed
            ? `${secretName} was already gone. The list was refreshed.`
            : `${secretName} was already gone. Refreshing the list failed; retry.`,
        );
      } else {
        // Keep the row in place. Only a fresh list read may remove a row.
        setRowFailure({ name: secretName, error });
      }
    } finally {
      setRemoving(null);
    }
  }

  return (
    <div className="nx-settings__card nx-secrets" data-testid="server-secrets">
      <h2>Server secrets</h2>
      <div className="nx-secrets__server">
        <span className="nx-chip nx-chip--server">
          ◇ {connected ? serverLabel : "No server connected"}
        </span>
      </div>

      {!connected ? (
        <p className="nx-secrets__disconnected">Connect a server to manage its secrets</p>
      ) : null}
      {connected && !secrets ? (
        <p className="nx-secrets__disconnected">
          This StackSpirit build does not expose the server secret store.
        </p>
      ) : null}

      {connected && secrets ? (
        <div className="nx-secrets__list-wrap">
          <div className="nx-secrets__list-head">
            <h3>Stored secrets</h3>
            <button
              type="button"
              className="nx-btn nx-btn--quiet nx-btn--sm"
              onClick={() => void refreshList()}
              disabled={listing || initialLoading || removing !== null}
            >
              Refresh
            </button>
          </div>

          {initialLoading ? (
            <div className="nx-secrets__progress" role="status">
              Loading stored secrets…
            </div>
          ) : null}

          {visibleListError ? (
            <div className="nx-secrets__error" role="alert" data-testid="secret-list-error">
              <strong>{errorTitle(visibleListError)}</strong>
              <span>{visibleListError.message}</span>
              {visibleListError.hint ? <span>{visibleListError.hint}</span> : null}
              <button
                type="button"
                className="nx-btn nx-btn--quiet nx-btn--sm"
                onClick={() => void refreshList()}
                disabled={listing}
              >
                Retry
              </button>
            </div>
          ) : null}

          {visibleEntries && visibleEntries.secrets.length === 0 ? (
            <p className="nx-secrets__empty">
              No secrets are stored on {serverLabel} yet.
            </p>
          ) : null}

          {visibleEntries && visibleEntries.secrets.length > 0 ? (
            <ul className="nx-secrets__list" aria-label="Stored server secrets">
              {visibleEntries.secrets.map((entry: SecretEntryView) => {
                const confirmingThis = confirming === entry.name;
                const failure =
                  rowFailure?.name === entry.name ? rowFailure.error : null;
                return (
                  <li
                    key={entry.name}
                    className="nx-secrets__row"
                    data-testid={`secret-row-${entry.name}`}
                  >
                    <div className="nx-secrets__row-main">
                      <span className="nx-secrets__name">{entry.name}</span>
                      <span className="nx-secrets__last4">•••• {entry.last4}</span>
                      <span className="nx-secrets__updated">
                        Updated {formatDateTime(entry.updated_at)}
                      </span>
                    </div>
                    <div className="nx-secrets__actions">
                      {confirmingThis ? (
                        <div
                          className="nx-secrets__confirm"
                          role="group"
                          aria-label={`Confirm revoke ${entry.name}`}
                        >
                          <span className="nx-secrets__confirm-title">
                            Revoke {entry.name}?
                          </span>
                          <button
                            type="button"
                            className="nx-btn nx-btn--quiet nx-btn--sm"
                            onClick={() => setConfirming(null)}
                            disabled={removing === entry.name}
                          >
                            Cancel
                          </button>
                          <button
                            type="button"
                            className="nx-btn nx-btn--sm nx-secrets__danger"
                            onClick={() => void revoke(entry.name)}
                            disabled={revokeDisabled}
                          >
                            {removing === entry.name ? "Revoking…" : "Revoke"}
                          </button>
                        </div>
                      ) : (
                        <button
                          type="button"
                          className="nx-btn nx-btn--quiet nx-btn--sm"
                          onClick={() => {
                            setConfirming(entry.name);
                            setRowFailure(null);
                          }}
                          disabled={revokeDisabled}
                        >
                          Revoke
                        </button>
                      )}
                    </div>
                    {failure ? (
                      <div
                        className="nx-secrets__row-error"
                        role="alert"
                        data-testid={`secret-row-error-${entry.name}`}
                      >
                        <strong>{errorTitle(failure)}</strong>
                        <span>{failure.message}</span>
                        {failure.hint ? <span>{failure.hint}</span> : null}
                      </div>
                    ) : null}
                  </li>
                );
              })}
            </ul>
          ) : null}

          {visibleEntries && visibleEntries.storage === null ? (
            <p className="nx-secrets__storage-note" data-testid="vault-mode-missing">
              VOWP did not report a storage mode
            </p>
          ) : null}
          {visibleEntries && visibleEntries.storage !== null ? (
            <VaultModeBadge mode={visibleEntries.storage} />
          ) : null}
        </div>
      ) : null}

      {connected && secrets && !firstSaveDone && blockedReason === null ? (
        <div className="nx-secrets__warning" data-testid="secret-first-warning">
          <div className="nx-secrets__warning-head">
            <span aria-hidden="true">🔒</span>
            <strong>Before your first server secret</strong>
          </div>
          <p>
            This key will be stored <strong>on {serverLabel} in VOWP&rsquo;s vault</strong>,
            not on this Mac. The assistant only sees secret names and metadata (name, last
            four characters and timestamps), never the value.
          </p>
          <label className="nx-secrets__ack">
            <input
              type="checkbox"
              checked={acknowledged}
              onChange={(event) => {
                const checked = event.target.checked;
                setAcknowledged(checked);
                if (bridge) {
                  if (checked) acknowledgedSessions.add(bridge);
                  else acknowledgedSessions.delete(bridge);
                }
              }}
            />
            I understand this stores the key on the server
          </label>
        </div>
      ) : null}

      {blockedReason ? (
        <div
          className="nx-secrets__unavailable"
          role="status"
          data-testid="secret-set-unavailable"
        >
          <strong>Storing secrets is not supported</strong>
          <span>{blockedReason.message}</span>
          {blockedReason.hint ? <span>{blockedReason.hint}</span> : null}
          <span>You can still list and revoke stored secrets.</span>
        </div>
      ) : null}

      <form
        className="nx-secrets__form"
        onSubmit={storeSecret}
        aria-label="Store a server secret"
        aria-busy={saving}
        autoComplete="off"
      >
        <div className="nx-secrets__form-row">
          <label className="nx-field" htmlFor={ids.name}>
            Secret name
            <input
              id={ids.name}
              value={name}
              onChange={(event) => setName(event.target.value)}
              pattern="[A-Z][A-Z0-9_]{0,63}"
              title="Start with an uppercase letter; then uppercase letters, digits or underscore (up to 64 characters)."
              aria-invalid={name !== "" && !nameValid}
              disabled={formDisabled}
              autoComplete="off"
              spellCheck={false}
              required
            />
          </label>
          <label className="nx-field" htmlFor={ids.value}>
            Secret value
            <input
              id={ids.value}
              type="password"
              value={value}
              onChange={(event) => setValue(event.target.value)}
              disabled={formDisabled}
              autoComplete="new-password"
              spellCheck={false}
              required
            />
          </label>
        </div>
        <p className="nx-secrets__form-note">
          Uppercase letters, digits and <code>_</code> only, starting with a letter. The
          value is cleared from this form before the request starts and is never shown to
          the assistant.
        </p>
        {setError ? (
          <div className="nx-secrets__error" role="alert" data-testid="secret-set-error">
            <strong>{errorTitle(setError)}</strong>
            <span>{setError.message}</span>
            {setError.hint ? <span>{setError.hint}</span> : null}
          </div>
        ) : null}
        {saved ? (
          <div className="nx-secrets__notice" role="status" data-testid="secret-stored">
            stored {saved.name}, ends in …{saved.last4}
          </div>
        ) : null}
        {notice ? (
          <div className="nx-secrets__notice" role="status" data-testid="secret-notice">
            {notice}
          </div>
        ) : null}
        <div className="nx-secrets__form-actions">
          <button type="submit" className="nx-btn" disabled={saveDisabled}>
            {saving ? "Saving…" : "Save"}
          </button>
        </div>
      </form>
    </div>
  );
}

const KNOWN_VAULT_MODES: Record<string, { label: string; note: string }> = {
  basic: {
    label: "Basic",
    note: "Files under the runtime user, mode 0600, not encrypted at rest.",
  },
  vault_user: {
    label: "Vault user",
    note: "Stored in the vault user's home.",
  },
  encrypted: {
    label: "Encrypted",
    note: "Encrypted at rest with age.",
  },
};

/** Labels the storage mode VOWP reported without inventing semantics for unknown tokens. */
function VaultModeBadge({ mode }: { mode: string }) {
  const known = Object.prototype.hasOwnProperty.call(KNOWN_VAULT_MODES, mode)
    ? KNOWN_VAULT_MODES[mode]
    : undefined;
  return (
    <p className="nx-secrets__storage-note" data-testid="vault-mode-note">
      <span className="nx-secrets__mode-badge" data-testid="vault-mode" data-mode={mode}>
        {known ? known.label : mode}
      </span>{" "}
      {known ? known.note : `VOWP reported this storage mode: ${mode}.`}
    </p>
  );
}
