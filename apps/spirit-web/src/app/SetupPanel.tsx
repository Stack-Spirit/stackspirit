import { useState, type FormEvent } from "react";

import { Button, Panel } from "@stackspirit/ui";

import { SAME_ORIGIN_REQUIRED } from "../copy";
import { parseBaseUrl } from "../bridge/baseUrl";
import { setupRedirectUrl } from "../bridge/create";
import { SpiritMark } from "../shared/next";
import type { SpiritErrorDto } from "@stackspirit/ui";

export function SetupPanel({ error }: { error: SpiritErrorDto | null }) {
  const [value, setValue] = useState(() =>
    typeof window === "undefined" ? "" : window.location.origin,
  );
  const [localError, setLocalError] = useState<string | null>(error?.message ?? null);

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const parsed = parseBaseUrl(value);
    if (!parsed.baseUrl) {
      setLocalError(parsed.error?.message ?? "Enter a valid daemon address.");
      return;
    }
    if (new URL(parsed.baseUrl).origin !== window.location.origin) {
      setLocalError(SAME_ORIGIN_REQUIRED);
      return;
    }
    window.location.assign(
      setupRedirectUrl(parsed.baseUrl, {
        search: window.location.search,
        pathname: window.location.pathname,
        hash: window.location.hash,
        origin: window.location.origin,
      }),
    );
  }

  return (
    <main className="web-boot" id="main-canvas">
      <div className="web-brand">
        <SpiritMark size={36} />
        <div>
          <strong>StackSpirit</strong>
          <span>Web cockpit · read-only</span>
        </div>
      </div>
      <Panel title="Connect to the VOWP daemon" tone="warn">
        <p>
          This Web build talks to one loopback daemon through a same-origin reverse proxy or
          the Vite development proxy. There is no fixed port and no mock fallback.
        </p>
        <form className="web-setup-form" onSubmit={submit}>
          <label className="field">
            <span className="field__label">Daemon address</span>
            <input
              className="input"
              value={value}
              onChange={(event) => setValue(event.target.value)}
              placeholder="http://127.0.0.1:5174 (the dev-server origin)"
              autoComplete="off"
              spellCheck={false}
            />
            <span className="field__hint">
              An absolute http(s) URL. Same origin as this page; no userinfo, query, or
              fragment.
            </span>
          </label>
          <Button type="submit" variant="primary">
            Use this address
          </Button>
        </form>
        {localError ? (
          <p className="notice notice--danger" role="alert" data-testid="setup-error">
            {localError}
          </p>
        ) : null}
        <p className="muted small">
          Set <span className="mono">VITE_VOWP_DAEMON_URL</span> at build time, or open this
          page with <span className="mono">?daemon=</span> and the same-origin proxy origin.
          The address is stored in session storage and removed from the address bar.
        </p>
      </Panel>
    </main>
  );
}
