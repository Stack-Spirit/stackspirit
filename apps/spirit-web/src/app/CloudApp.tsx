import { useEffect, useRef, useState } from "react";

import {
  CloudAccountPanel,
  ManagedHomesPanel,
  useCloudAccount,
  type CloudMachine,
} from "@stackspirit/ui";

import { createRelayBridge } from "../bridge/relayBridge";
import type { HttpSseBridge } from "../bridge/HttpSseBridge";
import type { WebCloudRuntime } from "../cloud/runtime";
import { SpiritMark } from "../shared/next";
import { WebApp } from "./WebApp";

/**
 * The explicit Always-On Homes route (`/cloud`).
 *
 * It is optional and never the boot default. Once the relay channel is secure,
 * the exact same read-only `WebApp` is mounted with a relay-backed bridge, so the
 * page contracts do not fork between local and remote mode.
 */
export function CloudApp({ runtime }: { runtime: WebCloudRuntime }) {
  const phase = useCloudAccount((state) => state.phase);
  const relayStatus = useCloudAccount((state) => state.relayStatus);
  const selectedMachineId = useCloudAccount((state) => state.selectedMachineId);
  const machines = useCloudAccount((state) => state.machines);
  const [bridge, setBridge] = useState<HttpSseBridge | null>(null);
  const [bridgeError, setBridgeError] = useState<string | null>(null);
  const bridgeRef = useRef<HttpSseBridge | null>(null);

  useEffect(() => {
    let active = true;
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
        bridgeRef.current.dispose();
        bridgeRef.current = null;
        setBridge(null);
        setBridgeError(
          status.error?.message ?? "The relay connection was lost before the cockpit loaded.",
        );
      }
    };
    const unsubscribe = useCloudAccount.subscribe((state) => apply(state.relayStatus));
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
  return (
    <main className="web-boot web-cloud" id="main-canvas" data-testid="cloud-app">
      <div className="web-brand">
        <SpiritMark size={36} />
        <div>
          <strong>StackSpirit</strong>
          <span>Always-On Homes · optional account</span>
        </div>
      </div>

      <section className="panel" aria-label="Cloud Relay">
        <header className="panel__header">
          <h1 className="panel__title">Get an Always-On Home</h1>
        </header>
        <div className="panel__body web-cloud__body">
          <p className="muted">
            This page is optional. The local Web cockpit needs no account and no cloud
            connection. Sign in here only to reach one of your enrolled Always-On Homes
            through <span className="mono">relay.stackspirit.io</span>.
          </p>

          <CloudAccountPanel />
          <ManagedHomesPanel autoLoad />

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
          {phase === "local" ? (
            <p className="muted small">
              Nothing above is required.{" "}
              <a className="nx-link" href="/">
                Continue with a local daemon
              </a>
              .
            </p>
          ) : null}
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
