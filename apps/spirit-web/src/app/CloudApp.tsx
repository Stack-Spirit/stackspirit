import { useEffect, useRef, useState } from "react";

import { ManagedHomesPanel, useCloudAccount, type CloudMachine } from "@stackspirit/ui";

import { createRelayBridge } from "../bridge/relayBridge";
import type { HttpSseBridge } from "../bridge/HttpSseBridge";
import type { WebCloudRuntime } from "../cloud/runtime";
import { SpiritMark } from "../shared/next";
import { WebApp } from "./WebApp";
import { WebAuthPanel } from "./WebAuthPanel";

/**
 * The explicit Always-On Homes route (`/cloud`).
 *
 * It is optional and never the boot default. Once the relay channel is secure,
 * the exact same read-only `WebApp` is mounted with a relay-backed bridge, so the
 * page contracts do not fork between local and remote mode.
 */
export function CloudApp({ runtime }: { runtime: WebCloudRuntime }) {
  const relayStatus = useCloudAccount((state) => state.relayStatus);
  const selectedMachineId = useCloudAccount((state) => state.selectedMachineId);
  const machines = useCloudAccount((state) => state.machines);
  const [bridge, setBridge] = useState<HttpSseBridge | null>(null);
  const [bridgeError, setBridgeError] = useState<string | null>(null);
  const [plansOpen, setPlansOpen] = useState(false);
  const [localOpen, setLocalOpen] = useState(false);
  const bridgeRef = useRef<HttpSseBridge | null>(null);

  useEffect(() => {
    let active = true;
    const teardown = (message: string) => {
      bridgeRef.current?.dispose();
      bridgeRef.current = null;
      setBridge(null);
      setBridgeError(message);
    };
    const apply = (status: ReturnType<typeof useCloudAccount.getState>["relayStatus"]) => {
      if (!active) return;
      if (status?.state === "secure" && !bridgeRef.current) {
        try {
          const next = createRelayBridge(runtime.relay);
          bridgeRef.current = next;
          setBridge(next);
          setBridgeError(null);
        } catch (error) {
          setBridgeError(
            typeof error === "object" && error !== null && "message" in error
              ? String((error as { message: unknown }).message)
              : "The relay-backed cockpit could not start.",
          );
        }
        return;
      }
      if (status?.state === "failed" && bridgeRef.current) {
        teardown(
          status.error?.message ?? "The relay connection was lost before the cockpit loaded.",
        );
      }
    };
    const unsubscribe = useCloudAccount.subscribe((state) => {
      apply(state.relayStatus);
      // Signing out closes the relay in the shared store. Drop the cockpit
      // bridge too, so the page returns to the signed-out Always-On Homes view
      // instead of leaving a dead relay view mounted.
      if (state.phase !== "signed_in" && bridgeRef.current) {
        teardown(
          "Signed out. The encrypted relay cockpit is closed; local server mode is unchanged.",
        );
      }
    });
    // The store is an external system: create the bridge from its subscription,
    // never from a render or a synchronous effect body.
    queueMicrotask(() => apply(useCloudAccount.getState().relayStatus));
    return () => {
      active = false;
      unsubscribe();
      bridgeRef.current?.dispose();
      bridgeRef.current = null;
      runtime.dispose();
    };
  }, [runtime]);

  if (bridge) {
    return (
      <div className="web-cloud web-cloud--live" data-testid="cloud-live">
        <div className="web-cloud__bar" role="status">
          <span>
            <span className="status-dot status-dot--ok" aria-hidden="true" />
            Cloud Relay · {machineName(machines, selectedMachineId)}
          </span>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            data-testid="cloud-live-disconnect"
            onClick={() => {
              bridge.dispose();
              bridgeRef.current = null;
              setBridge(null);
              runtime.relay.disconnect();
            }}
          >
            Disconnect
          </button>
        </div>
        <WebApp bridge={bridge} />
      </div>
    );
  }

  const secureMachine = machines.find((machine) => machine.id === selectedMachineId) ?? null;

  function openPlans(): void {
    setPlansOpen(true);
    window.setTimeout(() => {
      document.getElementById("managed-homes")?.scrollIntoView?.({ block: "start" });
    }, 0);
  }

  return (
    <main className="web-boot web-cloud" id="main-canvas" data-testid="cloud-app">
      <div className="web-brand">
        <SpiritMark size={36} />
        <div>
          <strong>StackSpirit</strong>
          <span>Always-On Homes · encrypted relay</span>
        </div>
      </div>

      <section className="panel" aria-label="StackSpirit Account">
        <header className="panel__header">
          <h1 className="panel__title">Sign in to StackSpirit</h1>
        </header>
        <div className="panel__body web-cloud__body">
          <p className="muted">
            One account reaches every Always-On Machine you enroll through the encrypted
            relay. Sign in with Google, GitHub or email; or keep using a local daemon below.
            Nothing on this page is required for local server mode.
          </p>

          <WebAuthPanel client={runtime.auth} onOpenPlans={openPlans} />

          <section
            className="web-landing__plans"
            id="managed-homes"
            aria-label="Managed Homes plans"
            data-testid="cloud-managed-homes"
          >
            <ManagedHomesPanel autoLoad={plansOpen} />
          </section>

          {relayStatus?.state === "secure" ? (
            <p className="notice" role="status" data-testid="cloud-opening">
              Secure relay ready. Opening the encrypted cockpit for{" "}
              {secureMachine?.name ?? "your Always-On Home"}…
            </p>
          ) : null}
          {bridgeError ? (
            <p className="notice notice--danger" role="alert" data-testid="cloud-bridge-error">
              {bridgeError}
            </p>
          ) : null}

          <div className="web-local">
            <button
              type="button"
              className="web-local__toggle"
              aria-expanded={localOpen}
              onClick={() => setLocalOpen((open) => !open)}
              data-testid="local-daemon-toggle"
            >
              <span>Self-hosting? Connect to a local daemon without an account</span>
              <span className="web-local__chevron" aria-hidden="true">
                {localOpen ? "−" : "+"}
              </span>
            </button>
            {localOpen ? (
              <p className="muted small" data-testid="local-daemon-details">
                The local Web cockpit needs no account and no cloud connection.{" "}
                <a className="nx-link" href="/">
                  Continue with a local daemon
                </a>
                . Account sign-in is only for reaching an enrolled Always-On Machine through{" "}
                <span className="mono">relay.stackspirit.io</span>.
              </p>
            ) : null}
          </div>
        </div>
      </section>
    </main>
  );
}

function machineName(
  machines: CloudMachine[],
  machineId: string | null,
): string {
  if (!machineId) return "Always-On Home";
  return machines.find((machine) => machine.id === machineId)?.name ?? "Always-On Home";
}
