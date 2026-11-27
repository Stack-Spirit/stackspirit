import { useEffect, useRef, useState, type FormEvent } from "react";

import {
  Button,
  CloudAccountPanel,
  asCloudError,
  useCloudAccount,
  type CloudError,
  type CloudMachine,
} from "@stackspirit/ui";

import type { WebAuthClient } from "../auth/client";
import { consumeAuthTokenFragment } from "../auth/session";

export interface WebAuthPanelProps {
  /** The Web account client; null keeps the honest build-unavailable copy. */
  client: WebAuthClient | null;
  /** Opens the Managed Homes plan catalogue from the second onboarding card. */
  onOpenPlans?: () => void;
  /**
   * On the landing route there is no relay cockpit to mount, so a signed-in
   * account with machines links to the Always-On Homes route instead of offering
   * a Connect button that would have nowhere to appear.
   */
  homesHref?: string;
}

type AuthMode = "sign_in" | "sign_up";
type AuthStep = "credentials" | "verify";

function firstMachineId(machines: CloudMachine[]): string | null {
  return machines.find((machine) => machine.status === "online")?.id ?? machines[0]?.id ?? null;
}

function applySignedIn(
  account: Awaited<ReturnType<WebAuthClient["account"]>>,
  machines: CloudMachine[],
  machinesError: CloudError | null,
): void {
  const selected = firstMachineId(machines);
  useCloudAccount.setState({
    phase: "signed_in",
    account,
    machines,
    machinesStatus: machinesError ? "error" : "ready",
    machinesError,
    selectedMachineId: selected,
    provisioning: machines.find((machine) => machine.id === selected)?.provisioning ?? null,
    provisioningStatus: "idle",
    provisioningError: null,
    error: null,
    notice: null,
  });
}

async function readAccount(client: WebAuthClient): Promise<{
  account: Awaited<ReturnType<WebAuthClient["account"]>>;
  machines: CloudMachine[];
  machinesError: CloudError | null;
}> {
  const account = await client.account();
  try {
    return { account, machines: await client.listMachines(), machinesError: null };
  } catch (error) {
    return { account, machines: [], machinesError: asCloudError(error) };
  }
}

function errorCopy(error: unknown): { message: string; hint: string | null } {
  const mapped = asCloudError(error);
  return { message: mapped.message, hint: mapped.hint };
}

/**
 * The account-first StackSpirit sign-in card for the Web/PWA landing and `/cloud`
 * screens.
 *
 * Google and GitHub are one GET away; email sign-up asks for the six-digit code
 * from the API and email sign-in is a password login. A signed-in account with no
 * enrolled machines gets the two-choice onboarding: link an existing VPS or open
 * the Managed Homes plans. The token only lives in `auth/session.ts` memory.
 */
export function WebAuthPanel({ client, onOpenPlans, homesHref }: WebAuthPanelProps) {
  const phase = useCloudAccount((state) => state.phase);
  const account = useCloudAccount((state) => state.account);
  const machines = useCloudAccount((state) => state.machines);
  const machinesStatus = useCloudAccount((state) => state.machinesStatus);
  const machinesError = useCloudAccount((state) => state.machinesError);
  const notice = useCloudAccount((state) => state.notice);
  const storeError = useCloudAccount((state) => state.error);
  const signOut = useCloudAccount((state) => state.signOut);
  const refreshMachines = useCloudAccount((state) => state.refreshMachines);

  // Read (and scrub) the OAuth callback before the first render can show it.
  const [fragment] = useState(() =>
    consumeAuthTokenFragment(window.location, window.history),
  );
  const [mode, setMode] = useState<AuthMode>("sign_in");
  const [step, setStep] = useState<AuthStep>("credentials");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [pendingEmail, setPendingEmail] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(() => {
    if (fragment.sawAuthToken && !fragment.token) {
      return "That sign-in link was incomplete or expired. Sign in again.";
    }
    if (fragment.token && !client) {
      return "StackSpirit Account is not available in this build.";
    }
    return null;
  });
  const [errorHint, setErrorHint] = useState<string | null>(null);
  const [localNotice, setLocalNotice] = useState<string | null>(null);
  const [enrollName, setEnrollName] = useState("");
  const [enrollToken, setEnrollToken] = useState<string | null>(null);
  const [enrollExpires, setEnrollExpires] = useState<string | null>(null);
  const [enrollBusy, setEnrollBusy] = useState(false);
  const [enrollError, setEnrollError] = useState<{ message: string; hint: string | null } | null>(
    null,
  );
  const hydratedToken = useRef<string | null>(null);

  useEffect(() => {
    if (!fragment.token || !client || hydratedToken.current === fragment.token) return;
    hydratedToken.current = fragment.token;
    let active = true;
    void (async () => {
      setBusy(true);
      try {
        const session = await readAccount(client);
        if (!active) return;
        applySignedIn(session.account, session.machines, session.machinesError);
        useCloudAccount.setState({
          notice: "Signed in with your StackSpirit Account. Local mode still works without one.",
        });
      } catch (caught) {
        if (!active) return;
        const copy = errorCopy(caught);
        setError(copy.message);
        setErrorHint(copy.hint);
        client.clearSession();
      } finally {
        if (active) setBusy(false);
      }
    })();
    return () => {
      active = false;
    };
  }, [client, fragment.token]);

  async function finishSignIn(): Promise<void> {
    if (!client) return;
    const session = await readAccount(client);
    applySignedIn(session.account, session.machines, session.machinesError);
    useCloudAccount.setState({
      notice: "Signed in. Your Always-On Machines appear below.",
    });
    setPassword("");
    setCode("");
    setStep("credentials");
  }

  async function submitCredentials(event: FormEvent<HTMLFormElement>): Promise<void> {
    event.preventDefault();
    if (!client) {
      setError("StackSpirit Account is not available in this build.");
      return;
    }
    const address = email.trim();
    if (!address || !address.includes("@")) {
      setError("Enter your email address.");
      setErrorHint(null);
      return;
    }
    if (!password) {
      setError("Enter your password.");
      setErrorHint(null);
      return;
    }
    setBusy(true);
    setError(null);
    setErrorHint(null);
    setLocalNotice(null);
    try {
      if (mode === "sign_up") {
        const created = await client.signUp(address, password);
        if (created.signedIn) {
          await finishSignIn();
        } else {
          setPendingEmail(created.email);
          setStep("verify");
          setLocalNotice(`Enter the 6-digit code sent to ${created.email}.`);
        }
      } else {
        await client.login(address, password);
        await finishSignIn();
      }
    } catch (caught) {
      const copy = errorCopy(caught);
      setError(copy.message);
      setErrorHint(copy.hint);
    } finally {
      setBusy(false);
    }
  }

  async function submitCode(event: FormEvent<HTMLFormElement>): Promise<void> {
    event.preventDefault();
    if (!client) return;
    setBusy(true);
    setError(null);
    setErrorHint(null);
    try {
      const verified = await client.verifyEmail(pendingEmail, code);
      if (!verified.signedIn) {
        await client.login(pendingEmail, password);
      }
      await finishSignIn();
    } catch (caught) {
      const copy = errorCopy(caught);
      setError(copy.message);
      setErrorHint(copy.hint);
    } finally {
      setBusy(false);
    }
  }

  async function createEnrollmentToken(event: FormEvent<HTMLFormElement>): Promise<void> {
    event.preventDefault();
    if (!client) return;
    setEnrollBusy(true);
    setEnrollError(null);
    try {
      const token = await client.createEnrollmentToken(enrollName);
      setEnrollToken(token.token);
      setEnrollExpires(token.expires_at);
    } catch (caught) {
      setEnrollError(errorCopy(caught));
    } finally {
      setEnrollBusy(false);
    }
  }

  function toggleMode(next: AuthMode): void {
    setMode(next);
    setStep("credentials");
    setError(null);
    setErrorHint(null);
    setLocalNotice(null);
    setCode("");
  }

  if (phase === "signed_in") {
    const hasMachines = machines.length > 0;
    return (
      <section className="nx-cloud web-auth" data-testid="web-auth-panel" aria-label="StackSpirit Account">
        {hasMachines && !homesHref ? (
          <CloudAccountPanel />
        ) : (
          <div className="web-auth__account" data-testid="web-auth-account">
            <div className="nx-cloud__title">
              <div>
                <h3>{account?.name ?? account?.email ?? "StackSpirit Account"}</h3>
                <p>
                  {account?.email ?? "Signed in"}
                  <span className="nx-chip nx-chip--server">Signed in</span>
                </p>
              </div>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  setEnrollToken(null);
                  setEnrollExpires(null);
                  setEnrollName("");
                  setEnrollError(null);
                  signOut();
                }}
                data-testid="web-auth-sign-out"
              >
                Sign out
              </Button>
            </div>

            {machinesStatus === "loading" ? (
              <p className="nx-cloud__meta" role="status">
                Reading your Always-On Machines…
              </p>
            ) : machinesStatus === "error" && machinesError ? (
              <p className="nx-cloud__error" role="alert" data-testid="web-machines-error">
                {machinesError.message}
                {machinesError.hint ? <span> {machinesError.hint}</span> : null}{" "}
                <button
                  type="button"
                  className="nx-link nx-link--muted"
                  onClick={() => void refreshMachines()}
                >
                  Retry
                </button>
              </p>
            ) : hasMachines && homesHref ? (
              <div className="web-auth__homes" data-testid="web-auth-homes">
                <p className="nx-cloud__meta">
                  {machines.length} Always-On {machines.length === 1 ? "Machine" : "Machines"}{" "}
                  enrolled. Open the Always-On Homes page to connect through the encrypted
                  relay.
                </p>
                <a className="btn btn--secondary btn--sm" href={homesHref}>
                  Open Always-On Homes
                </a>
              </div>
            ) : (
              <div className="web-onboard" data-testid="web-onboarding">
                <div>
                  <h4>Finish setting up StackSpirit</h4>
                  <p className="nx-cloud__meta">
                    Your account has no enrolled Always-On Machine yet. Pick one of these
                    two paths; local and SSH server mode keeps working either way.
                  </p>
                </div>
                <div className="web-onboard__grid">
                  <article className="web-onboard__card" data-testid="onboard-connect-machine">
                    <h5>Connect your existing Always-On Machine</h5>
                    <p>
                      Link a VPS you already own to this account. StackSpirit creates a
                      one-time enrollment token; the machine keeps its own relay key and
                      any signed-in device can then reach it through the encrypted relay.
                    </p>
                    {enrollToken ? (
                      <div className="web-onboard__token" data-testid="onboard-enrollment">
                        <p>
                          Run this on <strong>{enrollName || "your VPS"}</strong> and paste the
                          token when it asks:
                        </p>
                        <code className="web-onboard__command">
                          stackspirit relay enroll --token-file - --name &quot;
                          {enrollName || "my-vps"}&quot;
                        </code>
                        <p className="nx-cloud__meta">One-time enrollment token:</p>
                        <code className="web-onboard__secret" data-testid="web-enroll-token">
                          {enrollToken}
                        </code>
                        <p className="nx-cloud__meta">
                          Shown once. Do not share it. It expires{" "}
                          {enrollExpires ? new Date(enrollExpires).toLocaleString() : "soon"}.
                        </p>
                        <Button
                          size="sm"
                          variant="secondary"
                          onClick={() => void refreshMachines()}
                          data-testid="onboard-refresh-machines"
                        >
                          I ran it — refresh machines
                        </Button>
                      </div>
                    ) : (
                      <form className="web-onboard__form" onSubmit={(event) => void createEnrollmentToken(event)}>
                        <label className="field">
                          <span className="field__label">Machine name</span>
                          <input
                            className="input"
                            value={enrollName}
                            onChange={(event) => setEnrollName(event.target.value)}
                            placeholder="my-vps"
                            autoComplete="off"
                            spellCheck={false}
                          />
                        </label>
                        <Button
                          type="submit"
                          size="sm"
                          variant="secondary"
                          loading={enrollBusy}
                          data-testid="onboard-create-token"
                        >
                          Create enrollment token
                        </Button>
                      </form>
                    )}
                    {enrollError ? (
                      <p className="nx-cloud__error" role="alert" data-testid="onboard-enroll-error">
                        {enrollError.message}
                        {enrollError.hint ? <span> {enrollError.hint}</span> : null}
                      </p>
                    ) : null}
                  </article>

                  <article className="web-onboard__card" data-testid="onboard-managed-home">
                    <h5>Need an Always-On Machine?</h5>
                    <p>
                      Choose a turnkey managed Contabo machine with StackSpirit installed.
                      Every published tier and the optional encrypted R2 backup add-on are
                      shown with EUR and USD pricing when the API publishes them.
                    </p>
                    <Button
                      size="sm"
                      variant="primary"
                      onClick={onOpenPlans}
                      data-testid="onboard-open-plans"
                    >
                      See Managed Homes plans
                    </Button>
                  </article>
                </div>
              </div>
            )}
          </div>
        )}

        {notice ? (
          <p className="nx-cloud__notice" role="status" data-testid="web-auth-notice">
            {notice}
          </p>
        ) : null}
        {storeError ? (
          <p className="nx-cloud__error" role="alert" data-testid="web-auth-store-error">
            {storeError.message}
            {storeError.hint ? <span> {storeError.hint}</span> : null}
          </p>
        ) : null}
      </section>
    );
  }

  const googleUrl = client?.oauthStartUrl("google") ?? "https://api.stackspirit.io/v1/auth/google/start";
  const githubUrl = client?.oauthStartUrl("github") ?? "https://api.stackspirit.io/v1/auth/github/start";

  return (
    <section className="nx-cloud web-auth" data-testid="web-auth-panel" aria-label="StackSpirit Account">
      <div className="web-auth__head">
        <h2>StackSpirit Account</h2>
        <p>
          One account reaches every Always-On Machine you enroll through the encrypted
          relay. You can also use a local or SSH server without an account.
        </p>
      </div>

      <div className="web-auth__oauth">
        <a
          className="btn btn--secondary btn--block web-auth__oauth-btn"
          href={googleUrl}
          data-testid="auth-google"
        >
          <span className="web-auth__oauth-glyph" aria-hidden="true">
            G
          </span>
          Continue with Google
        </a>
        <a
          className="btn btn--secondary btn--block web-auth__oauth-btn"
          href={githubUrl}
          data-testid="auth-github"
        >
          <span className="web-auth__oauth-glyph" aria-hidden="true">
            ◈
          </span>
          Continue with GitHub
        </a>
      </div>

      <div className="web-auth__divider" role="separator">
        <span>or use email</span>
      </div>

      <div className="nx-segment web-auth__mode" role="group" aria-label="Email sign-in or sign-up">
        <button
          type="button"
          aria-pressed={mode === "sign_in"}
          onClick={() => toggleMode("sign_in")}
          data-testid="auth-mode-sign-in"
        >
          Sign in
        </button>
        <button
          type="button"
          aria-pressed={mode === "sign_up"}
          onClick={() => toggleMode("sign_up")}
          data-testid="auth-mode-sign-up"
        >
          Create account
        </button>
      </div>

      {step === "verify" ? (
        <form className="web-auth__form" onSubmit={(event) => void submitCode(event)}>
          <p className="nx-cloud__meta">
            We sent a 6-digit code to <strong>{pendingEmail}</strong>.
          </p>
          <label className="field">
            <span className="field__label">Verification code</span>
            <input
              className="input web-auth__code"
              value={code}
              onChange={(event) => setCode(event.target.value.replace(/\D/g, "").slice(0, 6))}
              inputMode="numeric"
              autoComplete="one-time-code"
              maxLength={6}
              pattern="[0-9]{6}"
              placeholder="000000"
              aria-label="6-digit verification code"
            />
          </label>
          <div className="web-auth__actions">
            <Button type="submit" variant="primary" loading={busy} data-testid="auth-verify">
              Verify email
            </Button>
            <Button
              type="button"
              variant="ghost"
              onClick={() => {
                setStep("credentials");
                setCode("");
                setError(null);
                setErrorHint(null);
              }}
            >
              Back
            </Button>
          </div>
        </form>
      ) : (
        <form className="web-auth__form" onSubmit={(event) => void submitCredentials(event)}>
          <label className="field">
            <span className="field__label">Email</span>
            <input
              className="input"
              type="email"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              autoComplete="email"
              spellCheck={false}
              placeholder="you@example.com"
            />
          </label>
          <label className="field">
            <span className="field__label">Password</span>
            <input
              className="input"
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              autoComplete={mode === "sign_up" ? "new-password" : "current-password"}
              placeholder={mode === "sign_up" ? "Choose a password" : "Your password"}
            />
          </label>
          <div className="web-auth__actions">
            <Button type="submit" variant="primary" loading={busy} data-testid="auth-submit">
              {mode === "sign_up" ? "Create account" : "Sign in"}
            </Button>
            <span className="nx-cloud__meta">
              {mode === "sign_up"
                ? "We email a 6-digit code before the account is active."
                : "Secure HTTPS only. StackSpirit never asks for your SSH keys."}
            </span>
          </div>
        </form>
      )}

      {error ? (
        <p className="nx-cloud__error" role="alert" data-testid="web-auth-error">
          {error}
          {errorHint ? <span> {errorHint}</span> : null}
        </p>
      ) : null}
      {localNotice ? (
        <p className="nx-cloud__notice" role="status" data-testid="web-auth-step-notice">
          {localNotice}
        </p>
      ) : null}

      {!client ? (
        <p className="nx-banner nx-banner--warn" role="status">
          This build has no StackSpirit Account runtime. Local and SSH server mode still works.
        </p>
      ) : null}
    </section>
  );
}
