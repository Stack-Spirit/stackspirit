import { useCallback, useEffect, useRef, useState } from "react";

import { Badge, Button, EmptyState, Modal, Panel } from "../../components";
import type {
  EngineStatus,
  ExternalHarness,
  ExternalHarnessKind,
  GateSnapshot,
  KeyStatus,
  ModelChoice,
} from "../../bridge/wire";
import { toSpiritError } from "../../bridge/errors";
import {
  engineLogs,
  gatesProbed,
  includedModels,
  needsEngineOnboarding,
  providerOptions,
} from "../../domain/engine";
import {
  useConnection,
  useEngine,
  useFleetStore,
  useGates,
} from "../../state/fleetStore";
import { ByokForm } from "./ByokForm";
import { EngineOnboarding } from "./EngineOnboarding";
import { EngineStatusCard } from "./EngineStatusCard";
import { ExternalHarnessPicker } from "./ExternalHarnessPicker";
import { GateCard } from "./GateCard";
import { GateDrawer } from "./GateDrawer";
import { ModelPicker } from "./ModelPicker";
import "./engine.css";

type GateDetails = "local" | "asif" | null;

function upsertKey(statuses: KeyStatus[], next: KeyStatus): KeyStatus[] {
  const rest = statuses.filter((status) => status.provider !== next.provider);
  return [...rest, next].sort((left, right) => left.provider.localeCompare(right.provider));
}

export function EngineView() {
  const bridge = useFleetStore((state) => state.bridge);
  const status = useEngine();
  const gates = useGates();
  const connection = useConnection();
  const refreshEngine = useFleetStore((state) => state.refreshEngine);
  const probeGates = useFleetStore((state) => state.probeGates);

  const [models, setModels] = useState<ModelChoice[]>([]);
  const [keyStatuses, setKeyStatuses] = useState<KeyStatus[]>([]);
  const [harnesses, setHarnesses] = useState<ExternalHarness[]>([]);
  const [scanning, setScanning] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [setupFinished, setSetupFinished] = useState(false);
  const [setupStarted, setSetupStarted] = useState(false);
  const [reopenSetup, setReopenSetup] = useState(false);
  const [onboardingComplete, setOnboardingComplete] = useState(false);
  const [initialStep, setInitialStep] = useState<1 | 2>(1);
  const [gateDetails, setGateDetails] = useState<GateDetails>(null);
  const [logsOpen, setLogsOpen] = useState(false);
  const autoProbedHost = useRef<string | null>(null);
  const liveConnection =
    connection.state === "connected" || connection.state === "stale";
  // A gate snapshot belongs to the connection that requested it (VOWP-CONTRACT
  // §11.5). The scope records the host and connection generation so a snapshot from
  // an earlier connection is never displayed for a new one.
  const [gateScope, setGateScope] = useState<{
    hostId: string;
    since: string | null;
    gates: GateSnapshot;
  } | null>(() =>
    liveConnection && connection.host_id && gates
      ? { hostId: connection.host_id, since: connection.since, gates }
      : null,
  );

  const loadModels = useCallback(
    async (current: EngineStatus | null) => {
      if (!bridge || !current || current.state.state !== "ready") {
        setModels([]);
        return;
      }
      try {
        setModels(await bridge.engine.models());
      } catch (modelError) {
        setError(toSpiritError(modelError).message);
      }
    },
    [bridge],
  );

  useEffect(() => {
    const handle = window.setTimeout(() => {
      void loadModels(status);
    }, 0);
    return () => window.clearTimeout(handle);
  }, [loadModels, status]);

  useEffect(() => {
    if (!bridge) return;
    let active = true;
    void bridge.engine
      .keyStatus()
      .then((statuses) => {
        if (active) setKeyStatuses(statuses);
      })
      .catch((keyError: unknown) => {
        if (active) setError(toSpiritError(keyError).message);
      });
    return () => {
      active = false;
    };
  }, [bridge]);

  const scopeProbe = useCallback(async () => {
    const current = useFleetStore.getState().connection;
    if (current.state !== "connected" && current.state !== "stale") return;
    if (!current.host_id) return;
    const hostId = current.host_id;
    const since = current.since;
    const before = useFleetStore.getState().gates;
    await probeGates();
    const next = useFleetStore.getState().connection;
    if (
      (next.state !== "connected" && next.state !== "stale") ||
      next.host_id !== hostId ||
      next.since !== since
    ) {
      return;
    }
    const latest = useFleetStore.getState().gates;
    // A failed probe leaves the store untouched. Only a new snapshot from this
    // connection generation may replace the scope; never reuse the previous host's.
    if (latest && latest !== before && gatesProbed(latest)) {
      setGateScope({ hostId, since, gates: latest });
    }
  }, [probeGates]);

  useEffect(() => {
    if (!bridge) return;
    if (connection.state !== "connected" || !connection.host_id) return;
    const key = `${connection.host_id}:${connection.since ?? ""}`;
    if (autoProbedHost.current === key) return;
    autoProbedHost.current = key;
    // Only a snapshot already scoped to this connection counts as cached. A snapshot
    // from the previous connection never suppresses the probe for a new host.
    if (
      gateScope &&
      gateScope.hostId === connection.host_id &&
      gateScope.since === connection.since &&
      gatesProbed(gateScope.gates)
    ) {
      return;
    }
    const handle = window.setTimeout(() => {
      void scopeProbe();
    }, 0);
    return () => window.clearTimeout(handle);
  }, [
    bridge,
    connection.host_id,
    connection.since,
    connection.state,
    gateScope,
    scopeProbe,
  ]);

  const handleProbe = useCallback(() => {
    if (!liveConnection || !connection.host_id) return;
    void scopeProbe();
  }, [connection.host_id, liveConnection, scopeProbe]);

  const visibleGates =
    gateScope &&
    liveConnection &&
    gateScope.hostId === connection.host_id &&
    gateScope.since === connection.since
      ? gateScope.gates
      : null;

  const loadHarnesses = useCallback(async () => {
    if (!bridge) return;
    setScanning(true);
    setError(null);
    try {
      setHarnesses(await bridge.engine.scanExternal());
    } catch (scanError) {
      setError(toSpiritError(scanError).message);
    } finally {
      setScanning(false);
    }
  }, [bridge]);

  const handleRestart = useCallback(async () => {
    if (!bridge) return;
    setBusy(true);
    setError(null);
    try {
      await bridge.engine.restart();
      await refreshEngine();
      await loadModels(useFleetStore.getState().engine);
    } catch (restartError) {
      setError(toSpiritError(restartError).message);
    } finally {
      setBusy(false);
    }
  }, [bridge, loadModels, refreshEngine]);

  const handleSelectModel = useCallback(
    async (id: string) => {
      if (!bridge) throw new Error("Engine bridge is unavailable.");
      setSetupStarted(true);
      const next = await bridge.engine.selectModel(id);
      await refreshEngine();
      await loadModels(next);
    },
    [bridge, loadModels, refreshEngine],
  );

  const handleSaveKey = useCallback(
    async (provider: string, secret: string) => {
      if (!bridge) throw new Error("Engine bridge is unavailable.");
      const saved = await bridge.engine.storeKey(provider, secret);
      setKeyStatuses((current) => upsertKey(current, saved));
      return saved;
    },
    [bridge],
  );

  const handleVerifyKey = useCallback(
    async (provider: string) => {
      if (!bridge) throw new Error("Engine bridge is unavailable.");
      const checked = await bridge.engine.verifyKey(provider);
      setKeyStatuses((current) => upsertKey(current, checked));
      return checked;
    },
    [bridge],
  );

  const handleDeleteKey = useCallback(
    async (provider: string) => {
      if (!bridge) throw new Error("Engine bridge is unavailable.");
      const deleted = await bridge.engine.deleteKey(provider);
      setKeyStatuses((current) => upsertKey(current, deleted));
      return deleted;
    },
    [bridge],
  );

  const handleSelectExternal = useCallback(
    async (kind: ExternalHarnessKind, path: string) => {
      if (!bridge) return;
      setSetupStarted(true);
      setBusy(true);
      setError(null);
      try {
        await bridge.engine.selectExternal(kind, path);
        await refreshEngine();
        setModels([]);
      } catch (externalError) {
        setError(toSpiritError(externalError).message);
      } finally {
        setBusy(false);
      }
    },
    [bridge, refreshEngine],
  );

  const handleUseBundled = useCallback(async () => {
    if (!bridge) return;
    setBusy(true);
    setError(null);
    try {
      await bridge.engine.useBundled();
      await refreshEngine();
    } catch (bundledError) {
      setError(toSpiritError(bundledError).message);
    } finally {
      setBusy(false);
    }
  }, [bridge, refreshEngine]);

  const setupRequired = needsEngineOnboarding(status, onboardingComplete);
  const showSetup =
    !setupFinished && (reopenSetup || setupStarted || setupRequired);

  if (!bridge || !status) {
    return (
      <div className="view-stack engine-view" data-testid="engine-view">
        <Panel title="StackSpirit Engine">
          <EmptyState
            icon="◇"
            title="Engine status unavailable"
            body="The local engine boundary did not answer. Restart StackSpirit and retry."
            actionLabel="Refresh status"
            onAction={() => void refreshEngine()}
          />
        </Panel>
      </div>
    );
  }

  const external = status.choice.type === "external";
  const available = includedModels(models);
  const logs = engineLogs(status);
  const probeDisabledReason =
    liveConnection && connection.host_id
      ? null
      : "Connect to a VOWP home before probing capability gates.";

  return (
    <div className="view-stack engine-view" data-testid="engine-view">
      {error ? <p className="notice notice--danger">{error}</p> : null}

      {showSetup ? (
        <EngineOnboarding
          status={status}
          models={models}
          keyStatuses={keyStatuses}
          harnesses={harnesses}
          scanning={scanning}
          initialStep={initialStep}
          busy={busy}
          error={error}
          advancedOpen={advancedOpen}
          onToggleAdvanced={() => setAdvancedOpen((open) => !open)}
          onRetry={() => void handleRestart()}
          onSelectModel={handleSelectModel}
          onSaveKey={handleSaveKey}
          onVerifyKey={handleVerifyKey}
          onDeleteKey={handleDeleteKey}
          onScanHarnesses={() => void loadHarnesses()}
          onSelectExternal={(kind, path) => void handleSelectExternal(kind, path)}
          onUseBundled={() => void handleUseBundled()}
          onFinish={() => {
            setOnboardingComplete(true);
            setSetupFinished(true);
            setSetupStarted(false);
            setReopenSetup(false);
            setInitialStep(1);
            void refreshEngine();
          }}
        />
      ) : (
        <>
          <EngineStatusCard
            status={status}
            busy={busy}
            advancedOpen={advancedOpen}
            onToggleAdvanced={() => setAdvancedOpen((open) => !open)}
            onRestart={() => void handleRestart()}
            onChangeModel={() => {
              setInitialStep(2);
              setSetupFinished(false);
              setSetupStarted(true);
              setReopenSetup(true);
            }}
            onUseBundled={() => void handleUseBundled()}
            onDetails={() => setAdvancedOpen(true)}
            onViewLogs={() => setLogsOpen(true)}
          />

          <div className="engine-columns">
            <Panel
              title="Included models"
              actions={<Badge tone={external ? "unknown" : "ok"}>{available.length} verified</Badge>}
            >
              {external ? (
                <p className="muted small">
                  Included models apply only to the bundled StackSpirit Engine. The
                  external harness keeps its own model access.
                </p>
              ) : (
                <ModelPicker
                  models={available}
                  selectedId={status.selected_model?.id ?? null}
                  onSelect={(id) => {
                    void handleSelectModel(id).catch((selectionError: unknown) =>
                      setError(toSpiritError(selectionError).message),
                    );
                  }}
                  disabled={busy}
                />
              )}
            </Panel>

            <Panel title="Provider key">
              {external ? (
                <p className="muted small">
                  Provider keys apply to the bundled StackSpirit Engine. The external
                  harness keeps its own login and configuration.
                </p>
              ) : (
                <ByokForm
                  providers={providerOptions(available, keyStatuses)}
                  statuses={keyStatuses}
                  onSave={handleSaveKey}
                  onVerify={handleVerifyKey}
                  onDelete={handleDeleteKey}
                />
              )}
            </Panel>
          </div>

          <Panel title="Existing harness">
            <ExternalHarnessPicker
              harnesses={harnesses}
              scanning={scanning}
              selected={external}
              error={error}
              onScan={() => void loadHarnesses()}
              onSelect={(kind, path) => void handleSelectExternal(kind, path)}
              onUseBundled={() => void handleUseBundled()}
            />
          </Panel>

          <Panel
            title="Capability gates"
            actions={
              <Button
                variant="secondary"
                size="sm"
                onClick={handleProbe}
                disabled={probeDisabledReason !== null}
                title={probeDisabledReason ?? undefined}
              >
                Re-probe
              </Button>
            }
          >
            {visibleGates ? (
              <div className="gate-grid">
                <GateCard
                  kind="local"
                  gates={visibleGates}
                  onDetails={() => setGateDetails("local")}
                />
                <GateCard
                  kind="asif"
                  gates={visibleGates}
                  onDetails={() => setGateDetails("asif")}
                />
              </div>
            ) : (
              <EmptyState
                icon="◇"
                title="Capability gates not probed"
                body="Connect to the VOWP home and re-probe. A gate is never Ready from a version string alone."
                actionLabel="Probe gates"
                onAction={handleProbe}
                actionDisabled={probeDisabledReason !== null}
              />
            )}
            {probeDisabledReason ? (
              <p className="gate-reason">{probeDisabledReason}</p>
            ) : null}
            <p className="muted small">
              A Ready gate still says not implemented in this build while its StackSpirit
              action flag is false. D5 and D6 remain gated until VOWP proves the capability.
            </p>
          </Panel>

          <Panel title="About" data-testid="engine-about">
            <p className="muted small">
              StackSpirit Engine runs on opencode {status.version ?? "1.19.5"} (MIT). The
              bundled sidecar is fetched at build time and verified against
              engine/manifest.json. See THIRD_PARTY_NOTICES.md for the license text.
            </p>
            <p className="muted small">
              StackSpirit is MIT-licensed. Engine logs are capped at 200 redacted lines and
              never contain a provider key.
            </p>
          </Panel>
        </>
      )}

      <GateDrawer
        kind={gateDetails ?? "local"}
        gates={visibleGates}
        open={gateDetails !== null}
        onClose={() => setGateDetails(null)}
      />

      <Modal
        open={logsOpen}
        title="StackSpirit Engine logs"
        onClose={() => setLogsOpen(false)}
      >
        <p className="muted small">
          Last {logs.length} redacted lines (maximum 200). Provider keys and full prompts
          are never written here.
        </p>
        {logs.length > 0 ? (
          <div className="gate-evidence engine-logs" data-testid="engine-logs">
            {logs.map((line, index) => (
              <span key={`${index}:${line}`}>{line}</span>
            ))}
          </div>
        ) : (
          <p className="muted small" data-testid="engine-logs-empty">
            No engine log lines have been captured yet.
          </p>
        )}
      </Modal>
    </div>
  );
}
