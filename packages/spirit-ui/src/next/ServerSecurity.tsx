import { useEffect, useId, useMemo, useRef, useState, type FormEvent } from "react";

import { errorTitle, toSpiritError, type SpiritError } from "../bridge/errors";
import type { HostDraft, HostProfile, PasswordAuthOutcome } from "../bridge/wire";
import { useFleetStore, useHosts, usePreview } from "../state/fleetStore";
import { passwordAuthSettingsSummary } from "./passwordAuth";

/** Turns a saved profile back into the draft the onboarding install expects. */
function draftFor(profile: HostProfile): HostDraft {
  return {
    label: profile.label,
    ssh: profile.ssh,
    // The host resolves the app key itself; never send the profile's path back.
    identity_file: null,
    vowp_home: profile.vowp_home,
    default_project: profile.default_project,
  };
}

/**
 * Settings entry for retrying `PasswordAuthentication no` on a server whose saved
 * profile uses the app-owned key.
 *
 * The current `spirit-core` install API needs one more password login to prove the
 * key and apply the sshd drop-in, so the card asks for it when password login is
 * still on. The password is cleared from React state before the call starts, is
 * used once in the host process, and is never stored or rendered. The outcome is
 * reported exactly as `disabled`, `not_supported` or `failed`.
 */
export function ServerSecurityCard() {
  const bridge = useFleetStore((state) => state.bridge);
  const loadHosts = useFleetStore((state) => state.loadHosts);
  const preview = usePreview();
  const hosts = useHosts();
  const onboarding = bridge?.onboarding ?? null;
  const [installed, setInstalled] = useState<string[] | null>(null);
  const [hostId, setHostId] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<PasswordAuthOutcome | null>(null);
  const [failure, setFailure] = useState<SpiritError | null>(null);
  const passwordRef = useRef<HTMLInputElement>(null);
  const ids = { host: useId(), password: useId() };
  const lookupInstalledHosts = onboarding?.installedHosts;

  useEffect(() => {
    if (!lookupInstalledHosts || preview) return;
    let disposed = false;
    void lookupInstalledHosts().then(
      (value) => {
        if (!disposed) setInstalled(value);
      },
      () => {
        if (!disposed) setInstalled([]);
      },
    );
    return () => {
      disposed = true;
    };
  }, [lookupInstalledHosts, preview]);

  // Without the optional host lookup (older bridge or a focused test), every saved
  // host is offered. The host command still refuses anything that is not its key.
  const eligible = useMemo(() => {
    if (!onboarding || preview) return [];
    const ids = lookupInstalledHosts ? installed : hosts.map((host) => host.id);
    return ids ? hosts.filter((host) => ids.includes(host.id)) : [];
  }, [hosts, installed, lookupInstalledHosts, onboarding, preview]);

  const selected = eligible.find((host) => host.id === hostId) ?? eligible[0] ?? null;
  if (!onboarding || preview || !selected) return null;

  async function retryPasswordAuth(event: FormEvent) {
    event.preventDefault();
    if (!onboarding || !selected || password === "" || busy) return;
    // Read the one-time secret, then clear it from React state before the first
    // await. From here on no render, snapshot, or error can contain it.
    const oneTimePassword = password;
    setPassword("");
    setOutcome(null);
    setFailure(null);
    setBusy(true);
    try {
      const result = await onboarding.installKey({
        draft: draftFor(selected),
        host_key: selected.host_key,
        password: oneTimePassword,
        disable_password_auth: true,
      });
      if (!result.installed) {
        setFailure({
          code: "ssh_auth",
          message: "The key was written, but a fresh key-only login did not verify.",
          hint: "StackSpirit only counts a verified key-only login as success. Retry when the server is reachable.",
          retryable: true,
        });
      } else {
        setOutcome(result.password_auth);
        await loadHosts();
      }
    } catch (caught) {
      setFailure(toSpiritError(caught));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="nx-settings__card" data-testid="server-security">
      <h2>Password login</h2>
      <div className="nx-settings__row">
        <span>
          Turn off password login
          <small>
            Ask sshd to accept only keys on a server whose saved profile already uses the
            StackSpirit key. Use this after StackSpirit reported that password login is
            still on.
          </small>
        </span>
      </div>
      <form className="nx-security" onSubmit={retryPasswordAuth} aria-busy={busy} autoComplete="off">
        <label className="nx-field" htmlFor={ids.host}>
          Server
          <select
            id={ids.host}
            value={selected.id}
            disabled={busy}
            onChange={(event) => {
              setHostId(event.target.value);
              setPassword("");
              setOutcome(null);
              setFailure(null);
            }}
          >
            {eligible.map((host) => (
              <option key={host.id} value={host.id}>
                {host.label} · {host.ssh.user}@{host.ssh.host}
              </option>
            ))}
          </select>
        </label>
        <label className="nx-field" htmlFor={ids.password}>
          One-time server password
          <input
            ref={passwordRef}
            id={ids.password}
            type="password"
            value={password}
            disabled={busy}
            onChange={(event) => setPassword(event.target.value)}
            autoComplete="off"
          />
        </label>
        <div className="nx-secure__note">
          <span aria-hidden="true">🔒</span>
          <span>
            The password is used once to re-check the key and apply the sshd change, then
            forgotten. It is cleared before the call, never stored, and never shown to the
            assistant.
          </span>
        </div>
        {busy ? (
          <div className="nx-secure__progress" role="status">
            Re-checking the key and applying the sshd change…
          </div>
        ) : null}
        {failure ? (
          <div className="nx-secure__error" role="alert" data-testid="password-auth-error">
            <strong>{errorTitle(failure)}</strong>
            <span>{failure.message}</span>
            {failure.hint ? <span>{failure.hint}</span> : null}
            <span>The one-time password was cleared and is not stored. Enter it again to retry.</span>
          </div>
        ) : null}
        {outcome ? (
          <div
            className={`nx-secure__outcome nx-security__result nx-security__result--${outcome.state}`}
            role="status"
            data-testid="password-auth-outcome"
          >
            {passwordAuthSettingsSummary(outcome)}
          </div>
        ) : null}
        <div className="nx-secure__actions">
          <button type="submit" className="nx-btn" disabled={password === "" || busy}>
            {busy ? "Turning off…" : "Turn off password login"}
          </button>
        </div>
      </form>
    </div>
  );
}
