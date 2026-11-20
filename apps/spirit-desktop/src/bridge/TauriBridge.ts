import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { SpiritBridge } from "@stackspirit/ui";
import { toSpiritError } from "@stackspirit/ui";
import type {
  ActionResult,
  AgentPairInput,
  AgentPairing,
  AgentPathAddInput,
  AgentPathView,
  AppKeyView,
  AsifExportOptions,
  AsifExportResult,
  AsifImportOptions,
  AsifImportResult,
  BackupPolicyView,
  CapacityTargetView,
  CapacityView,
  ChatEvent,
  ChatLogPage,
  ChatLogQuery,
  ChatPermissionDecision,
  ChatSendInput,
  ChatSendReceipt,
  ChatSessionView,
  ChatStartInput,
  ClusterConfigView,
  ClusterFormInput,
  ClusterFormedView,
  ClusterPromoteInput,
  ClusterPromoteOutcome,
  ClusterStateView,
  ConnectionSnapshot,
  EngineStatus,
  ExternalHarness,
  ExternalHarnessKind,
  FleetView,
  GateSnapshot,
  HostDraft,
  HostKeyPin,
  HostKeyScan,
  HostProfile,
  HostProfilePatch,
  HostProbe,
  InboxItemView,
  JobAction,
  KeyStatus,
  LocalAgentView,
  LocalOrchestratorOverview,
  ModelChoice,
  NewHostProfile,
  OnboardingInstallInput,
  OnboardingInstallOutcome,
  OrchestratorNewInput,
  OrchestratorView,
  RawDocument,
  SecretListView,
  SecretRemoveOutcome,
  SecretSetInput,
  SecretSetOutcome,
  SessionCardView,
  SessionId,
  SetTargetInput,
  StateEvent,
  SurfaceEntryView,
  SurfacePairInput,
  SurfacePairResult,
  SurfaceRevokeResult,
  SurfaceRotateInput,
  SurfaceRotateResult,
} from "@stackspirit/ui";

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toSpiritError(error);
  }
}

/**
 * The only file in the renderer allowed to import `@tauri-apps/api`.
 * Every method is a thin adapter over a `spirit-core` command.
 */
export class TauriBridge implements SpiritBridge {
  readonly kind = "tauri" as const;
  readonly preview = false;

  hosts = {
    list: (): Promise<HostProfile[]> => call("list_hosts"),
    add: (input: NewHostProfile): Promise<HostProfile> => call("add_host", { input }),
    update: (id: string, patch: HostProfilePatch): Promise<HostProfile> =>
      call("update_host", { id, patch }),
    remove: (id: string): Promise<void> => call("remove_host", { id }),
    scanFingerprint: (input: HostDraft): Promise<HostKeyScan> =>
      call("scan_fingerprint", { draft: input }),
    trustFingerprint: (id: string, pin: HostKeyPin): Promise<HostProfile> =>
      call("trust_fingerprint", { id, pin }),
    probe: (input: HostDraft, pin?: HostKeyPin | null): Promise<HostProbe> =>
      call("probe_host", { draft: input, pin: pin ?? null }),
  };

  onboarding = {
    keyStatus: (): Promise<AppKeyView> => call("onboarding_key_status"),
    createKey: (): Promise<AppKeyView> => call("onboarding_create_key"),
    installKey: async (input: OnboardingInstallInput): Promise<OnboardingInstallOutcome> => {
      // The one-time password never leaves the renderer on an invalid request. The
      // guard throws the normal typed error shape; nothing is invoked.
      if (typeof input.password !== "string" || input.password.trim().length === 0) {
        throw toSpiritError({
          code: "invalid_input",
          message: "password: must not be empty",
          hint: "Enter the one-time server password, then retry.",
          retryable: false,
        });
      }
      return call("onboarding_install_key", { input });
    },
    installedHosts: async (): Promise<string[]> => {
      const view = await call<AppKeyView>("onboarding_key_status");
      return view.installed_host_ids ?? [];
    },
  };

  secrets = {
    list: (): Promise<SecretListView> => call("secrets_list"),
    set: async (input: SecretSetInput): Promise<SecretSetOutcome> => {
      // The write-only value never leaves the renderer on an invalid request. The guard
      // throws the normal typed error shape; nothing is invoked and the value is not
      // included in the error.
      if (typeof input.name !== "string" || input.name.trim().length === 0) {
        throw toSpiritError({
          code: "invalid_input",
          message: "name: must not be empty",
          hint: "Use an uppercase name such as OPENAI_API_KEY.",
          retryable: false,
        });
      }
      if (typeof input.value !== "string" || input.value.trim().length === 0) {
        throw toSpiritError({
          code: "invalid_input",
          message: "value: must not be empty",
          hint: "Enter the secret value, then retry.",
          retryable: false,
        });
      }
      return call("secrets_set", { input });
    },
    remove: async (name: string): Promise<SecretRemoveOutcome> => {
      if (typeof name !== "string" || name.trim().length === 0) {
        throw toSpiritError({
          code: "invalid_input",
          message: "name: must not be empty",
          hint: "Choose a stored secret to remove.",
          retryable: false,
        });
      }
      return call("secrets_remove", { name });
    },
  };

  cluster = {
    form: (input: ClusterFormInput): Promise<ClusterFormedView> =>
      call("cluster_form", { input }),
    health: (): Promise<ClusterStateView> => call("cluster_health"),
    promote: (input: ClusterPromoteInput): Promise<ClusterPromoteOutcome> =>
      call("cluster_promote", { input }),
    backupPolicy: (): Promise<BackupPolicyView | null> => call("cluster_backup_policy"),
    removeWorker: (hostId: string): Promise<ClusterConfigView> =>
      call("cluster_remove_worker", { hostId }),
  };

  connection = {
    connect: (hostId: string): Promise<ConnectionSnapshot> => call("connect_host", { hostId }),
    disconnect: async (hostId: string): Promise<void> => {
      await call<ConnectionSnapshot>("disconnect_host", { hostId });
    },
    status: (): Promise<ConnectionSnapshot> => call("connection_status"),
    subscribe: (listener: (event: StateEvent) => void): (() => void) => {
      let disposed = false;
      let unlisten: UnlistenFn | undefined;
      void listen<StateEvent>("spirit://state", (event) => listener(event.payload))
        .then((stop) => {
          if (disposed) stop();
          else unlisten = stop;
        })
        .catch(() => {
          // The window can close while the subscription is still starting.
        });
      return () => {
        disposed = true;
        unlisten?.();
      };
    },
  };

  fleet = {
    get: (): Promise<FleetView | null> => call("fleet_get"),
    refresh: (): Promise<FleetView> => call("fleet_refresh"),
    capacity: async (): Promise<CapacityView | null> => {
      const fleet = await call<FleetView | null>("fleet_get");
      return fleet?.capacity ?? null;
    },
    jobAction: (
      jobId: string,
      action: JobAction,
      reason?: string | null,
    ): Promise<ActionResult> => call("job_action", { jobId, action, reason: reason ?? null }),
    inbox: (all: boolean): Promise<InboxItemView[]> => call("fleet_inbox", { all }),
    answerInbox: (id: string, answer: string): Promise<InboxItemView> =>
      call("answer_inbox", { id, answer }),
    setTarget: (input: SetTargetInput): Promise<CapacityTargetView> =>
      call("set_target", { input }),
    rawDocument: (): Promise<RawDocument | null> => call("fleet_raw_document"),
  };

  engine = {
    status: (): Promise<EngineStatus> => call("engine_status"),
    models: (): Promise<ModelChoice[]> => call("engine_models"),
    selectModel: (id: string): Promise<EngineStatus> => call("select_model", { modelId: id }),
    storeKey: (provider: string, secret: string): Promise<KeyStatus> =>
      call("store_key", { provider, secret }),
    verifyKey: (provider: string): Promise<KeyStatus> => call("verify_key", { provider }),
    deleteKey: (provider: string): Promise<KeyStatus> => call("delete_key", { provider }),
    keyStatus: (): Promise<KeyStatus[]> => call("key_status"),
    scanExternal: (): Promise<ExternalHarness[]> => call("scan_external"),
    selectExternal: (kind: ExternalHarnessKind, path: string): Promise<EngineStatus> =>
      call("select_external", { kind, path }),
    useBundled: (): Promise<EngineStatus> => call("use_bundled"),
    restart: (): Promise<EngineStatus> => call("restart_engine"),
  };

  gates = {
    get: (): Promise<GateSnapshot> => call("gates_get"),
    probe: (): Promise<GateSnapshot> => call("probe_gates"),
  };

  chat = {
    start: (input: ChatStartInput): Promise<ChatSessionView> =>
      call("chat_start", { input }),
    attach: (session: SessionId): Promise<ChatSessionView> =>
      call("chat_attach", { session }),
    send: async (input: ChatSendInput): Promise<ChatSendReceipt> => {
      // Blank text never leaves the renderer: the guard throws the normal typed error
      // shape and nothing is invoked.
      if (typeof input.text !== "string" || input.text.trim().length === 0) {
        throw toSpiritError({
          code: "invalid_input",
          message: "invalid_text: enter a message before sending",
          hint: "Type a message, then retry.",
          retryable: false,
        });
      }
      return call("chat_send", { input });
    },
    stop: (session: SessionId): Promise<ChatSessionView> =>
      call("chat_stop", { session }),
    setModel: (session: SessionId, modelId: string): Promise<ChatSessionView> =>
      call("chat_set_model", { session, modelId }),
    replyPermission: (
      session: SessionId,
      permissionId: string,
      decision: ChatPermissionDecision,
    ): Promise<ChatSessionView> =>
      call("chat_reply_permission", { session, permissionId, decision }),
    log: (session: SessionId, query?: ChatLogQuery): Promise<ChatLogPage> =>
      call("chat_log", { session, query: query ?? null }),
    subscribe: (listener: (event: ChatEvent) => void): (() => void) => {
      let disposed = false;
      let unlisten: UnlistenFn | undefined;
      void listen<ChatEvent>("spirit://chat", (event) => listener(event.payload))
        .then((stop) => {
          if (disposed) stop();
          else unlisten = stop;
        })
        .catch(() => {
          // The window can close while the subscription is still starting.
        });
      return () => {
        if (disposed) return;
        disposed = true;
        unlisten?.();
      };
    },
  };

  sessions = {
    list: async (): Promise<SessionCardView[]> => {
      const fleet = await call<FleetView | null>("fleet_get");
      return fleet?.sessions ?? [];
    },
    exportAsif: (
      sessionId: string,
      options: AsifExportOptions,
    ): Promise<AsifExportResult | null> =>
      call("asif_export", { sessionId, options }),
    importAsif: (options: AsifImportOptions): Promise<AsifImportResult | null> =>
      call("asif_import", { options }),
  };

  localOrchestrators = {
    overview: (): Promise<LocalOrchestratorOverview> =>
      call("local_orchestrator_overview"),
    agentStatus: (): Promise<LocalAgentView> => call("local_agent_status"),
    agentPair: (input: AgentPairInput): Promise<AgentPairing> =>
      call("local_agent_pair", { input }),
    agentPaths: (): Promise<AgentPathView[]> => call("local_agent_paths"),
    agentPathAdd: (input: AgentPathAddInput): Promise<AgentPathView> =>
      call("local_agent_path_add", { input }),
    agentPathRemove: async (project: string): Promise<void> => {
      await call<void>("local_agent_path_remove", { project });
    },
    surfacePair: (input: SurfacePairInput): Promise<SurfacePairResult> =>
      call("local_surface_pair", { input }),
    surfaceList: (node?: string | null): Promise<SurfaceEntryView[]> =>
      call("local_surface_list", { node: node ?? null }),
    surfaceRotate: (input: SurfaceRotateInput): Promise<SurfaceRotateResult> =>
      call("local_surface_rotate", { input }),
    surfaceRevoke: (surface: string): Promise<SurfaceRevokeResult> =>
      call("local_surface_revoke", { surface }),
    orchestratorNew: (input: OrchestratorNewInput): Promise<OrchestratorView> =>
      call("local_orchestrator_new", { input }),
    orchestratorList: (): Promise<OrchestratorView[]> => call("local_orchestrator_list"),
    orchestratorStop: (name: string): Promise<OrchestratorView> =>
      call("local_orchestrator_stop", { name }),
    orchestratorResume: (
      name: string,
      project?: string | null,
      node?: string | null,
    ): Promise<OrchestratorView> =>
      call("local_orchestrator_resume", {
        name,
        project: project ?? null,
        node: node ?? null,
      }),
  };
}
