import { useEffect, useRef, useState } from "react";

import { CloudAccountPanel, Drawer, useCloudAccount } from "@stackspirit/ui";

export interface WebAccountMenuProps {
  /**
   * Show a quiet Sign in link while signed out. The local daemon cockpit uses
   * this; the account-first landing already has its own sign-in form.
   */
  showSignIn?: boolean;
  /**
   * Open the Managed Homes plans in place. When omitted, the menu links to the
   * Always-On Homes route, where the same plans live.
   */
  onOpenManagedHomes?: () => void;
}

/**
 * The persistent StackSpirit Account button for the top-right of the Web app.
 *
 * It renders for a signed-in account regardless of how many Always-On Machines
 * are enrolled, so the signed-in onboarding screen and the live cockpit both
 * expose Account details, Managed Homes and Sign out. The menu never shows or
 * stores an account token; sign-out goes through the shared cloud store.
 */
export function WebAccountMenu({
  showSignIn = false,
  onOpenManagedHomes,
}: WebAccountMenuProps) {
  const phase = useCloudAccount((state) => state.phase);
  const account = useCloudAccount((state) => state.account);
  const signOut = useCloudAccount((state) => state.signOut);

  const [open, setOpen] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const firstItemRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    firstItemRef.current?.focus();
    function onPointerDown(event: PointerEvent) {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpen(false);
        buttonRef.current?.focus();
        return;
      }
      if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
      const items = Array.from(
        rootRef.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [],
      );
      if (items.length === 0) return;
      event.preventDefault();
      const current = items.indexOf(document.activeElement as HTMLElement);
      const step = event.key === "ArrowDown" ? 1 : -1;
      const next = (current + step + items.length) % items.length;
      items[next]?.focus();
    }
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  if (phase !== "signed_in") {
    if (!showSignIn) return null;
    return (
      <a className="web-account__signin" href="/cloud" data-testid="web-account-sign-in">
        Sign in
      </a>
    );
  }

  const email = account?.email ?? "Signed in";
  const name = account?.name?.trim() || null;

  function handleSignOut(): void {
    setOpen(false);
    setDetailsOpen(false);
    signOut();
  }

  return (
    <div className="web-account" ref={rootRef} data-testid="web-account">
      <button
        ref={buttonRef}
        type="button"
        className="web-account__button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={`Account menu for ${email}`}
        onClick={() => setOpen((value) => !value)}
        data-testid="web-account-button"
      >
        <span className="web-account__avatar" aria-hidden="true">
          {email.slice(0, 1).toUpperCase()}
        </span>
        <span className="web-account__identity">
          <span className="web-account__caption">Account</span>
          <span className="web-account__email" data-testid="web-account-email">
            {email}
          </span>
        </span>
        <span className="web-account__chevron" aria-hidden="true">
          {open ? "▴" : "▾"}
        </span>
      </button>

      {open ? (
        <div className="web-account__menu" role="menu" aria-label="Account" data-testid="web-account-menu">
          <div className="web-account__who" role="presentation">
            <strong>{name ?? email}</strong>
            {name ? <span>{email}</span> : null}
          </div>
          <button
            ref={firstItemRef}
            type="button"
            role="menuitem"
            className="web-account__item"
            onClick={() => {
              setOpen(false);
              setDetailsOpen(true);
            }}
            data-testid="web-account-details"
          >
            Account details
          </button>
          {onOpenManagedHomes ? (
            <button
              type="button"
              role="menuitem"
              className="web-account__item"
              onClick={() => {
                setOpen(false);
                onOpenManagedHomes();
              }}
              data-testid="web-account-managed-homes"
            >
              Managed Homes
            </button>
          ) : (
            <a
              role="menuitem"
              className="web-account__item"
              href="/cloud#managed-homes"
              onClick={() => setOpen(false)}
              data-testid="web-account-managed-homes"
            >
              Managed Homes
            </a>
          )}
          <button
            type="button"
            role="menuitem"
            className="web-account__item web-account__item--danger"
            onClick={handleSignOut}
            data-testid="web-account-sign-out"
          >
            Sign out
          </button>
        </div>
      ) : null}

      <Drawer
        open={detailsOpen}
        title="Account details"
        onClose={() => setDetailsOpen(false)}
      >
        <div className="view-stack">
          <p className="muted small">
            Signed in as <strong>{email}</strong>. The account token stays in this
            browser&apos;s memory and is never shown here.
          </p>
          <CloudAccountPanel />
          <a className="btn btn--secondary btn--sm" href="/cloud">
            Open Always-On Homes
          </a>
        </div>
      </Drawer>
    </div>
  );
}
