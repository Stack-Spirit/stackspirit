import { useState } from "react";

import { CloudAccountPanel } from "./CloudAccountPanel";
import { ManagedHomesPanel } from "./ManagedHomesPanel";

/**
 * Settings → Account.
 *
 * The account surface is optional: local and SSH servers work without it. The panel
 * stays first and the Managed Homes catalog (EUR and USD) is one click away, so the
 * owner can see the price catalog without leaving Settings.
 */
export function AccountSettings() {
  const [planLinkNotice, setPlanLinkNotice] = useState<string | null>(null);

  function openManagedHomes() {
    const panel = document.getElementById("settings-managed-homes");
    if (!panel) {
      setPlanLinkNotice("Managed Homes appears below once the account panel is ready.");
      return;
    }
    setPlanLinkNotice(null);
    panel.scrollIntoView?.({ behavior: "smooth", block: "start" });
    panel.focus({ preventScroll: true });
  }

  return (
    <div className="nx-settings-section" data-testid="settings-account">
      <header className="nx-settings-section__head">
        <h3>Account</h3>
        <p>
          Sign in to keep Always-On Homes in sync across this Mac and the Web app. Local
          and SSH servers keep working with no account.
        </p>
      </header>

      <section className="nx-settings__card" aria-labelledby="settings-account-heading">
        <div className="nx-block__head">
          <div>
            <h4 id="settings-account-heading">StackSpirit Account</h4>
            <p className="nx-form__hint">
              Sign in or create an account on the same secure device-code page. StackSpirit
              never asks for your password here.
            </p>
          </div>
          <button
            type="button"
            className="nx-btn nx-btn--quiet nx-btn--sm"
            onClick={openManagedHomes}
            data-testid="account-managed-homes-link"
          >
            Managed Homes (EUR &amp; USD)
          </button>
        </div>
        <CloudAccountPanel />
        <p className="nx-form__hint">
          New to StackSpirit? Create an account on the same secure sign-in page; the
          one-time device code works for both signing in and signing up. Local mode needs
          no account.
        </p>
        {planLinkNotice ? (
          <p className="nx-form__hint" role="status">
            {planLinkNotice}
          </p>
        ) : null}
      </section>

      <section
        className="nx-settings__card"
        id="settings-managed-homes"
        aria-labelledby="settings-managed-homes-heading"
        tabIndex={-1}
        data-testid="settings-managed-homes"
      >
        <div className="nx-block__head">
          <div>
            <h4 id="settings-managed-homes-heading">Managed Homes</h4>
            <p className="nx-form__hint">
              Turnkey Always-On Homes at Contabo's regular rate plus 20%. Prices are
              published in EUR with a USD view when the catalog reports one.
            </p>
          </div>
          <span className="nx-chip nx-chip--muted">EUR &amp; USD</span>
        </div>
        <ManagedHomesPanel />
      </section>
    </div>
  );
}
