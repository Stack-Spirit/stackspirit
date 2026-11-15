import {
  MockBridge,
  type ConnectionSnapshot,
  type DaemonHealth,
  type FleetView,
  type InboxItemView,
  type JobDetailView,
  type MockScenario,
  type SpiritBridge,
  type StateEvent,
  type WebBridgeStatus,
  type WebFleetView,
} from "@stackspirit/ui";

import {
  CAPACITY_DETAIL_UNAVAILABLE,
  JOB_DETAIL_UNAVAILABLE,
  ORCHESTRATORS_UNAVAILABLE,
  PROJECT_STATE_UNAVAILABLE,
  RAW_DOCUMENT_UNAVAILABLE,
  SESSION_SURFACES_UNAVAILABLE,
  SUPERVISOR_STATE_UNAVAILABLE,
  WRITES_DISABLED,
} from "../copy";
import { webError } from "./errors";
import { WEB_READ_ONLY_CAPABILITIES, type WebFleetApi } from "./HttpSseBridge";

function isoNow(): string {
  return new Date().toISOString();
}

function desktopOnly(message: string) {
  return webError("desktop_only", message);
}

/**
 * Preview adapter around the shared `MockBridge`.
 *
 * The shared mock has a full desktop `FleetView` (capacity and supervisor included). The Web
 * shell must not show those as live daemon facts, so this adapter keeps the board, lanes,
 * running cards, and inbox and replaces capacity/supervisor with the documented unavailable
 * notices. The persistent Preview data badge is supplied by `preview: true`.
 */
export class WebMockBridge implements SpiritBridge {
  readonly kind = "mock" as const;
  readonly preview = true;
  readonly capabilities = WEB_READ_ONLY_CAPABILITIES;

  private readonly inner: MockBridge;
  private readonly snapshotListeners = new Set<(snapshot: WebFleetView) => void>();
  private readonly unsubscribers: Array<() => void> = [];
  private startPromise: Promise<void> | null = null;
  private disposed = false;

  constructor(scenario: MockScenario = "fresh") {
    this.inner = new MockBridge(scenario);
    this.unsubscribers.push(
      this.inner.connection.subscribe((event) => this.onInnerEvent(event)),
    );
  }

  connectionApi: SpiritBridge["connection"] = {
    connect: async (): Promise<ConnectionSnapshot> => {
      throw desktopOnly(
        "The Web preview speaks to one daemon; there is no SSH connection to establish.",
      );
    },
    disconnect: async (): Promise<void> => {
      throw desktopOnly("The Web preview has no connect/disconnect control.");
    },
    status: async (): Promise<ConnectionSnapshot> => this.inner.connection.status(),
    subscribe: (listener: (event: StateEvent) => void): (() => void) =>
      this.inner.connection.subscribe(listener),
  };

  fleetApi: WebFleetApi = {
    get: async (): Promise<null> => null,
    refresh: async (): Promise<never> => {
      throw webError(
        "no_route",
        "The Web preview uses the honest Web snapshot, not the desktop FleetView.",
        "Use fleet.snapshot(true).",
      );
    },
    capacity: async (): Promise<null> => null,
    job: async (id: string): Promise<JobDetailView> => {
      const job = this.inner.fleet.job;
      if (!job) {
        throw webError("daemon_incompatible", JOB_DETAIL_UNAVAILABLE);
      }
      return job(id);
    },
    jobAction: async (): Promise<never> => {
      throw webError("writes_disabled", WRITES_DISABLED);
    },
    inbox: async (all: boolean): Promise<InboxItemView[]> => this.inner.fleet.inbox(all),
    answerInbox: async (): Promise<never> => {
      throw webError("writes_disabled", WRITES_DISABLED);
    },
    setTarget: async (): Promise<never> => {
      throw webError("no_route", "The Web preview has no capacity target route.");
    },
    rawDocument: async (): Promise<null> => null,
    snapshot: async (refresh = false): Promise<WebFleetView | null> => this.snapshot(refresh),
    watch: (listener: (snapshot: WebFleetView) => void): (() => void) => {
      if (this.disposed) return () => undefined;
      this.snapshotListeners.add(listener);
      if (this.current) {
        queueMicrotask(() => {
          if (!this.disposed && this.current) listener(this.current);
        });
      }
      void this.ensureStarted().catch(() => undefined);
      return () => this.snapshotListeners.delete(listener);
    },
    webStatus: (): WebBridgeStatus => ({
      polling_only: false,
      next_retry_at: null,
      last_frame_at: this.current?.fetched_at ?? null,
      heartbeat_at: this.current?.daemon.heartbeat_at ?? null,
      stream_state: this.disposed ? "closed" : "open",
    }),
  };

  daemon: NonNullable<SpiritBridge["daemon"]> = {
    health: async (): Promise<DaemonHealth> => {
      const snapshot = await this.snapshot(false);
      if (!snapshot) {
        throw webError("daemon_unreachable", "The preview bridge has no snapshot.");
      }
      return snapshot.daemon;
    },
  };

  hosts: SpiritBridge["hosts"] = {
    list: async (): Promise<never> => {
      throw desktopOnly("Host profiles are desktop-only.");
    },
    add: async (): Promise<never> => {
      throw desktopOnly("Host profiles are desktop-only.");
    },
    update: async (): Promise<never> => {
      throw desktopOnly("Host profiles are desktop-only.");
    },
    remove: async (): Promise<never> => {
      throw desktopOnly("Host profiles are desktop-only.");
    },
    scanFingerprint: async (): Promise<never> => {
      throw desktopOnly("SSH host-key scanning is desktop-only.");
    },
    trustFingerprint: async (): Promise<never> => {
      throw desktopOnly("SSH host-key trust is desktop-only.");
    },
    probe: async (): Promise<never> => {
      throw desktopOnly("SSH probes are desktop-only.");
    },
  };

  engine: SpiritBridge["engine"] = {
    status: async (): Promise<never> => {
      throw desktopOnly("The StackSpirit Engine is desktop-only.");
    },
    models: async (): Promise<never> => {
      throw desktopOnly("Models and provider keys are desktop-only.");
    },
    selectModel: async (): Promise<never> => {
      throw desktopOnly("Models and provider keys are desktop-only.");
    },
    storeKey: async (): Promise<never> => {
      throw desktopOnly("Provider keys never enter the browser.");
    },
    verifyKey: async (): Promise<never> => {
      throw desktopOnly("Provider keys never enter the browser.");
    },
    deleteKey: async (): Promise<never> => {
      throw desktopOnly("Provider keys never enter the browser.");
    },
    keyStatus: async (): Promise<never> => {
      throw desktopOnly("Provider keys never enter the browser.");
    },
    scanExternal: async (): Promise<never> => {
      throw desktopOnly("External harness discovery is desktop-only.");
    },
    selectExternal: async (): Promise<never> => {
      throw desktopOnly("External harness selection is desktop-only.");
    },
    useBundled: async (): Promise<never> => {
      throw desktopOnly("Engine choice is desktop-only.");
    },
    restart: async (): Promise<never> => {
      throw desktopOnly("Engine supervision is desktop-only.");
    },
  };

  gates: SpiritBridge["gates"] = {
    get: async (): Promise<never> => {
      throw desktopOnly("Capability gates are desktop-only.");
    },
    probe: async (): Promise<never> => {
      throw desktopOnly("Capability gates are desktop-only.");
    },
  };

  connection = this.connectionApi;
  fleet = this.fleetApi;

  private current: WebFleetView | null = null;

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const unsubscribe of this.unsubscribers) unsubscribe();
    this.unsubscribers.length = 0;
    this.snapshotListeners.clear();
    this.current = null;
  }

  private async ensureStarted(): Promise<void> {
    if (this.disposed) return;
    if (this.current) return;
    if (!this.startPromise) {
      this.startPromise = this.start().finally(() => {
        this.startPromise = null;
      });
    }
    return this.startPromise;
  }

  private async start(): Promise<void> {
    const status = await this.inner.connection.status();
    if (status.state !== "connected" && status.state !== "stale") {
      const host = (await this.inner.hosts.list())[0];
      if (!host) {
        throw webError(
          "daemon_unreachable",
          "This preview scenario has no host to connect to.",
        );
      }
      await this.inner.connection.connect(host.id);
    }
    const fleet = await this.inner.fleet.get();
    if (!fleet) {
      throw webError("daemon_unreachable", "The preview bridge has no fleet snapshot.");
    }
    this.current = this.mapFleet(fleet);
    this.publish(this.current);
  }

  private async snapshot(refresh: boolean): Promise<WebFleetView | null> {
    await this.ensureStarted();
    if (refresh) {
      const fleet = await this.inner.fleet.refresh();
      this.current = this.mapFleet(fleet);
      this.publish(this.current);
    }
    return this.current;
  }

  private onInnerEvent(event: StateEvent): void {
    if (this.disposed || event.kind !== "fleet") return;
    this.current = this.mapFleet(event.fleet);
    this.publish(this.current);
  }

  private publish(snapshot: WebFleetView): void {
    for (const listener of this.snapshotListeners) {
      listener(snapshot);
    }
  }

  private mapFleet(fleet: FleetView): WebFleetView {
    const project = fleet.projects[0] ?? {
      name: "vowp",
      state: "unknown",
      paused_reason: "",
      jobs_by_state: {},
      cost_today_usd: null,
    };
    const now = isoNow();
    const openInbox = fleet.inbox.filter((item) => item.state === "open").length;
    return {
      revision: fleet.revision,
      host_id: "daemon",
      fetched_at: fleet.fetched_at,
      generated_at: fleet.generated_at,
      source: "preview",
      stale: fleet.stale,
      home: fleet.home,
      project,
      board: {
        slots_busy: fleet.running.length,
        slots_total: fleet.running.length,
        planner: null,
        keeper: null,
        limits: fleet.capacity.limits.map((limit) => ({
          provider: limit.provider,
          kind: limit.kind,
          resets_at: limit.resets_at,
        })),
        cost_today_usd: project.cost_today_usd,
        inbox_open: openInbox,
        last_checkpoint: null,
      },
      capacity_unavailable: {
        code: "no_route",
        route: "/api/v1/capacity",
        detail: CAPACITY_DETAIL_UNAVAILABLE,
      },
      supervisor_unavailable: {
        code: "no_route",
        detail: SUPERVISOR_STATE_UNAVAILABLE,
      },
      running: fleet.running,
      lanes: { ...fleet.lanes, queue_cap: null },
      inbox: fleet.inbox,
      sessions: [],
      orchestrators: [],
      latency: fleet.latency,
      unavailable: [
        {
          feature: "capacity_report",
          code: "no_route",
          route: "/api/v1/capacity",
          detail: CAPACITY_DETAIL_UNAVAILABLE,
        },
        {
          feature: "supervisor_state",
          code: "no_route",
          route: "/api/v1/fleet",
          detail: SUPERVISOR_STATE_UNAVAILABLE,
        },
        {
          feature: "project_state",
          code: "no_route",
          route: "/api/v1/fleet",
          detail: PROJECT_STATE_UNAVAILABLE,
        },
        {
          feature: "active_surfaces",
          code: "no_route",
          route: "/api/v1/fleet",
          detail: SESSION_SURFACES_UNAVAILABLE,
        },
        {
          feature: "orchestrators",
          code: "no_route",
          route: "/api/v1/fleet",
          detail: ORCHESTRATORS_UNAVAILABLE,
        },
        {
          feature: "raw_documents",
          code: "no_route",
          route: null,
          detail: RAW_DOCUMENT_UNAVAILABLE,
        },
      ],
      daemon: {
        origin: "http://mock.invalid",
        version: "0.0.0-preview",
        node: fleet.home,
        project: project.name,
        schema_version: 0,
        heartbeat_at: fleet.fetched_at,
        received_at: now,
      },
    };
  }
}
