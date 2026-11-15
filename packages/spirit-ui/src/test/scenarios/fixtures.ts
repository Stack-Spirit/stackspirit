import type {
  CapacityView,
  EngineStatus,
  FleetView,
  GateSnapshot,
  HostSampleView,
  JobCardView,
  JobLanes,
  RawDocument,
  WorkerCardView,
} from "../../bridge/wire";
import { parseExecutor } from "../../domain/executor";

export const NOW = "2026-11-14T12:00:00.000Z";
export const GIB = 1024 ** 3;

export interface ScenarioFixture {
  description: string;
  fleet: FleetView;
  engine?: EngineStatus | null;
  gates?: GateSnapshot | null;
  raw?: RawDocument | null;
}

function hostSample(overrides: Partial<HostSampleView> = {}): HostSampleView {
  return {
    at: NOW,
    interval_secs: 15,
    mem_total_bytes: 16 * GIB,
    mem_available_bytes: 12.4 * GIB,
    swap_used_bytes: 0,
    load1: 2.1,
    load5: 1.8,
    load15: 1.6,
    cpu_count: 8,
    cpu_quota_cores: null,
    disk_free_home_bytes: 42.1 * GIB,
    disk_free_worktrees_bytes: 38.7 * GIB,
    pressure_cpu_avg10: 4.2,
    pressure_mem_avg10: 1.1,
    pressure_io_avg10: 0.8,
    source: "proc",
    ...overrides,
  };
}

function capacity(overrides: Partial<CapacityView> = {}): CapacityView {
  const host = overrides.host === undefined ? hostSample() : overrides.host;
  const available = host?.mem_available_bytes ?? 0;
  const reserve = 2 * GIB;
  const floor = 5 * GIB;
  const free = host?.disk_free_home_bytes ?? 0;
  return {
    generated_at: NOW,
    measured_at: host?.at ?? null,
    report_loaded: true,
    host,
    running: { runs: 3, ram_p90_bytes: 1.1 * GIB, cpu_p90_cores: 2.4 },
    target: {
      mode: "fixed",
      target_workers: 8,
      advised_workers: 8,
      is_override: false,
      set_by: "owner",
      set_at: "2026-11-14T09:30:00.000Z",
      reason: "steady state",
    },
    advised_workers: 8,
    brake: { active: false, reasons: [], since: null },
    reserve_ram_bytes: reserve,
    min_disk_bytes: floor,
    disk: {
      free_home_bytes: free,
      free_worktrees_bytes: host?.disk_free_worktrees_bytes ?? null,
      floor_bytes: floor,
      source: "capacity_report",
      state: free < floor ? "critical" : free < floor * 1.25 ? "warn" : "ok",
    },
    ram: {
      available_bytes: available,
      reserve_bytes: reserve,
      running_p90_bytes: 1.1 * GIB,
      per_worker_p90_bytes: null,
      per_worker_source: null,
      per_worker_samples: null,
      estimated_extra_slots: null,
      state: available < reserve ? "critical" : available < reserve * 2 ? "warn" : "ok",
    },
    cpu: {
      load1: host?.load1 ?? 0,
      cpu_count: host?.cpu_count ?? 0,
      load_ratio: host && host.cpu_count > 0 ? host.load1 / host.cpu_count : 0,
      pressure_avg10: host?.pressure_cpu_avg10 ?? null,
      state: "ok",
    },
    estimates: [
      {
        project: null,
        job_kind: "build",
        harness: "opencode",
        model: null,
        build_mode: null,
        samples: 12,
        ram_p50_bytes: 1 * GIB,
        ram_p90_bytes: 1.2 * GIB,
        cpu_p50_cores: 1.1,
        cpu_p90_cores: 2.4,
        duration_p50_ms: 420_000,
        duration_p90_ms: 910_000,
        source: "pooled",
      },
    ],
    waiting: [],
    limits: [],
    provider_caps: {},
    reload_pending: false,
    ...overrides,
  };
}

function workerCard(overrides: Partial<WorkerCardView> = {}): WorkerCardView {
  return {
    job_id: "ss-2-desktop-shell",
    project: "vowp",
    kind: "build",
    role: "worker",
    state: "active",
    stalled: false,
    executor: parseExecutor("opencode:deepseek/deepseek-flash#max"),
    node: "vps",
    age_secs: 134,
    last_event_age_secs: 3,
    session_id: "01JCF5WQ8ZK3M2V7N4P6R8T0AB",
    ...overrides,
  };
}

function jobCard(overrides: Partial<JobCardView> = {}): JobCardView {
  return {
    job_id: "ss-5-fleet-inbox",
    project: "vowp",
    title: "Build the visual fleet canvas",
    kind: "build",
    role: "worker",
    state: "ready",
    priority: 50,
    age_secs: 420,
    reason: null,
    detail: null,
    gate: null,
    gate_at: null,
    gate_evidence: null,
    executor: null,
    session_id: null,
    ...overrides,
  };
}

function lanes(overrides: Partial<JobLanes> = {}): JobLanes {
  return {
    running: [],
    ready: [],
    review_fix: [],
    parked_blocked: [],
    done: [],
    counts: {
      running: 0,
      ready: 0,
      review_fix: 0,
      parked_blocked: 0,
      done: 0,
    },
    queue_truncated: false,
    queue_shown: 0,
    queue_cap: 200,
    ...overrides,
  };
}

function project(
  name: string,
  overrides: Partial<FleetView["projects"][number]> = {},
): FleetView["projects"][number] {
  return {
    name,
    state: "active",
    paused_reason: "",
    jobs_by_state: {},
    cost_today_usd: 1.37,
    ...overrides,
  };
}

function fleet(overrides: Partial<FleetView> = {}): FleetView {
  return {
    revision: 1,
    host_id: "01MOCKHOST000000000000000001",
    fetched_at: NOW,
    generated_at: NOW,
    source: "remote",
    stale: false,
    home: "vps",
    supervisor: {
      running: true,
      pid: 4812,
      paused: false,
      last_tick_at: "2026-11-14T11:59:58.000Z",
      last_tick_age_secs: 2,
      tick_secs: 60,
      fail_streak: 0,
      version: "0.4.0",
    },
    projects: [],
    capacity: capacity(),
    running: [],
    lanes: lanes(),
    inbox: [],
    sessions: [],
    orchestrators: [],
    latency: { last_ms: 42, ewma_ms: 42, p50_ms: 42, p95_ms: 42, samples: 5 },
    ...overrides,
  };
}

const threeRunning = [
  workerCard(),
  workerCard({
    job_id: "ss-3-fleet-canvas",
    executor: parseExecutor("claude-code:model-a@medium"),
    age_secs: 420,
    last_event_age_secs: 22,
    session_id: "01JCF5WQ8ZK3M2V7N4P6R8T0AC",
  }),
  workerCard({
    job_id: "ss-4-engine-onboarding",
    executor: null,
    age_secs: 60,
    last_event_age_secs: 4,
    session_id: "01JCF5WQ8ZK3M2V7N4P6R8T0AD",
  }),
];

const readyCards = [
  jobCard(),
  jobCard({
    job_id: "ss-6-capacity",
    title: "Capacity and storage rail",
    age_secs: 300,
  }),
  jobCard({
    job_id: "ss-7-engine-models",
    title: "Engine model catalog",
    state: "pending",
    age_secs: 120,
  }),
];

function freshSingle(): ScenarioFixture {
  const running = threeRunning;
  const review = jobCard({
    job_id: "ss-9-gate-check",
    title: "Gate check",
    state: "reviewing",
    gate: "secret_scan",
    gate_at: "2026-11-14T11:20:00.000Z",
    gate_evidence: "example evidence line, redacted",
    age_secs: 420,
  });
  const parked = jobCard({
    job_id: "ss-8-blocked-review",
    title: null,
    kind: "unknown",
    role: "unknown",
    state: "parked",
    priority: 0,
    age_secs: 0,
    reason: "question",
    detail: "Waiting for the owner to choose.",
  });
  return {
    description: "3/8 worker slots, 3 running, 3 ready, 1 reviewing, no brake",
    fleet: fleet({
      running,
      projects: [
        project("vowp", {
          jobs_by_state: {
            running: 3,
            ready: 3,
            reviewing: 1,
            parked: 1,
            done: 37,
          },
        }),
      ],
      lanes: lanes({
        running: running.map((card) =>
          jobCard({
            ...card,
            state: card.stalled ? "stalled" : "active",
            title: null,
            priority: 0,
          }),
        ),
        ready: readyCards,
        review_fix: [review],
        parked_blocked: [parked],
        counts: { running: 3, ready: 3, review_fix: 1, parked_blocked: 1, done: 37 },
        queue_shown: 4,
      }),
      inbox: [
        {
          id: "01JCF5WQ8ZK3M2V7N4P6R8T0AC",
          project: "vowp",
          job_id: "ss-4-engine-onboarding",
          kind: "question",
          title: "Choose the included model set",
          body: "Which included models should the first release guarantee?",
          options: ["Keep the generated catalog", "Pin a minimum set"],
          state: "open",
          answer: null,
          answered_by: null,
          created_at: "2026-11-14T11:42:00.000Z",
          answered_at: null,
          age_secs: 600,
        },
      ],
    }),
  };
}

function idle(): ScenarioFixture {
  const cap = capacity({
    running: { runs: 0, ram_p90_bytes: 0, cpu_p90_cores: 0 },
    target: {
      mode: "auto",
      target_workers: 4,
      advised_workers: 4,
      is_override: false,
      set_by: "vowp",
      set_at: NOW,
      reason: "idle",
    },
    advised_workers: 4,
  });
  return {
    description: "supervisor up, 0 running, 0 queue, target 4, advised 4",
    fleet: fleet({
      capacity: cap,
      projects: [project("vowp", { jobs_by_state: {} })],
    }),
  };
}

function busy(): ScenarioFixture {
  const running = Array.from({ length: 8 }, (_, index) =>
    workerCard({
      job_id: `ss-${10 + index}-busy`,
      age_secs: 100 + index * 30,
      last_event_age_secs: 2 + index,
    }),
  );
  const queue = Array.from({ length: 12 }, (_, index) =>
    jobCard({
      job_id: `ss-${30 + index}-queued`,
      title: `Queued job ${index + 1}`,
      age_secs: 200 + index,
    }),
  );
  const blocked = Array.from({ length: 3 }, (_, index) =>
    jobCard({
      job_id: `ss-${50 + index}-blocked`,
      title: null,
      state: "failed",
      reason: "gate",
      detail: "A gate failed.",
      age_secs: 0,
    }),
  );
  const cap = capacity({
    running: { runs: 8, ram_p90_bytes: 1.1 * GIB, cpu_p90_cores: 2.4 },
    limits: [
      {
        provider: "openai",
        harness: "opencode",
        kind: "quota",
        resets_at: "2026-11-14T13:00:00.000Z",
        detail: "Provider quota resets at the top of the hour.",
        since: "2026-11-14T11:50:00.000Z",
        last_probe_at: null,
        probe_fail_streak: 0,
        concurrency_cap: 2,
        cap_until: "2026-11-14T13:00:00.000Z",
      },
    ],
    waiting: [
      {
        project: "vowp",
        job: "ss-30-queued",
        code: "ram",
        detail: "RAM 1.2 GB short",
        since: "2026-11-14T11:57:00.000Z",
      },
    ],
    provider_caps: { openai: 2 },
  });
  return {
    description: "8/8 running, 12 queue, 3 blocked, provider limit",
    fleet: fleet({
      capacity: cap,
      running,
      projects: [
        project("vowp", {
          jobs_by_state: { running: 8, ready: 12, failed: 3, done: 5 },
        }),
      ],
      lanes: lanes({
        running: running.map((card) =>
          jobCard({ ...card, state: "active", title: null, priority: 0 }),
        ),
        ready: queue,
        parked_blocked: blocked,
        counts: { running: 8, ready: 12, review_fix: 0, parked_blocked: 3, done: 5 },
        queue_shown: 15,
      }),
    }),
  };
}

function storageLow(): ScenarioFixture {
  const cap = capacity({
    host: hostSample({ disk_free_home_bytes: 3 * GIB, disk_free_worktrees_bytes: null }),
    brake: {
      active: true,
      reasons: [{ code: "disk_low", detail: "Home disk is below the reserve floor." }],
      since: "2026-11-14T11:58:00.000Z",
    },
    waiting: [
      {
        project: "vowp",
        job: "ss-5-fleet-inbox",
        code: "disk",
        detail: "Home disk is 2.0 GiB below the floor",
        since: "2026-11-14T11:58:00.000Z",
      },
    ],
  });
  return {
    description: "disk_free_home_bytes < min_disk_bytes, disk_low brake",
    fleet: fleet({ capacity: cap, projects: [project("vowp", { jobs_by_state: {} })] }),
  };
}

function memoryPressure(): ScenarioFixture {
  const cap = capacity({
    host: hostSample({
      mem_available_bytes: 1.4 * GIB,
      swap_used_bytes: 2 * GIB,
      pressure_mem_avg10: 28.4,
    }),
    brake: {
      active: true,
      reasons: [
        { code: "memory_pressure", detail: "Memory pressure stayed above the safe threshold." },
        { code: "swap_growth", detail: "Swap usage is growing." },
      ],
      since: "2026-11-14T11:50:00.000Z",
    },
    waiting: [
      {
        project: "vowp",
        job: "ss-3-fleet-canvas",
        code: "brake",
        detail: "Memory pressure brake is active",
        since: "2026-11-14T11:52:00.000Z",
      },
    ],
  });
  return {
    description: "available < reserve, memory-pressure brake, high swap",
    fleet: fleet({ capacity: cap, projects: [project("vowp", { jobs_by_state: {} })] }),
  };
}

function providerLimit(): ScenarioFixture {
  const cap = capacity({
    limits: [
      {
        provider: "deepseek",
        harness: "opencode",
        kind: "quota",
        resets_at: "2026-11-14T13:00:00.000Z",
        detail: "Quota resets in one hour.",
        since: "2026-11-14T11:40:00.000Z",
        last_probe_at: null,
        probe_fail_streak: 2,
        concurrency_cap: 2,
        cap_until: "2026-11-14T13:00:00.000Z",
      },
    ],
    waiting: [
      {
        project: "vowp",
        job: "ss-7-engine-models",
        code: "provider",
        detail: "deepseek concurrency cap is 2",
        since: "2026-11-14T11:45:00.000Z",
      },
    ],
    provider_caps: { deepseek: 2 },
  });
  return {
    description: "quota limit with reset, provider waiting reason",
    fleet: fleet({ capacity: cap, projects: [project("vowp", { jobs_by_state: {} })] }),
  };
}

function supervisorDown(): ScenarioFixture {
  return {
    description: "running false, old last tick, stale snapshot",
    fleet: fleet({
      stale: true,
      supervisor: {
        running: false,
        pid: null,
        paused: false,
        last_tick_at: "2026-11-14T11:45:00.000Z",
        last_tick_age_secs: 900,
        tick_secs: 60,
        fail_streak: 7,
        version: "0.4.0",
      },
      projects: [project("vowp", { jobs_by_state: { running: 1, ready: 2 } })],
    }),
  };
}

function staleCache(): ScenarioFixture {
  return {
    description: 'source "cache", stale true',
    fleet: fleet({
      source: "cache",
      stale: true,
      projects: [project("vowp", { jobs_by_state: { running: 2, ready: 1 } })],
    }),
  };
}

function multiProject(): ScenarioFixture {
  const running = [
    workerCard({ job_id: "ss-2-desktop-shell", project: "vowp", age_secs: 134 }),
    workerCard({
      job_id: "nn-4-score",
      project: "northnoir",
      age_secs: 310,
      executor: parseExecutor("claude-code:model-a@medium"),
    }),
    workerCard({
      job_id: "asif-2-export",
      project: "vowp-asif",
      age_secs: 75,
      executor: null,
    }),
  ];
  return {
    description: "vowp + northnoir + vowp-asif, project filtering",
    fleet: fleet({
      running,
      capacity: capacity({ running: { runs: 3, ram_p90_bytes: 1.1 * GIB, cpu_p90_cores: 2.4 } }),
      projects: [
        project("vowp", { jobs_by_state: { running: 1, ready: 2, done: 12 } }),
        project("northnoir", { jobs_by_state: { running: 1, ready: 1, done: 4 } }),
        project("vowp-asif", {
          state: "paused",
          paused_reason: "Owner paused the project.",
          jobs_by_state: { running: 1, ready: 1 },
        }),
      ],
      lanes: lanes({
        running: running.map((card) =>
          jobCard({
            ...card,
            state: "active",
            title: null,
            priority: 0,
          }),
        ),
        ready: [
          jobCard({ job_id: "ss-20-queue", project: "vowp" }),
          jobCard({ job_id: "ss-21-queue", project: "vowp" }),
          jobCard({ job_id: "nn-5-queue", project: "northnoir" }),
          jobCard({ job_id: "asif-3-queue", project: "vowp-asif" }),
        ],
        counts: { running: 3, ready: 4, review_fix: 0, parked_blocked: 0, done: 16 },
        queue_shown: 4,
      }),
    }),
  };
}

function queueCap(): ScenarioFixture {
  const queue = Array.from({ length: 200 }, (_, index) =>
    jobCard({
      job_id: `ss-${100 + index}-queued`,
      title: `Queued job ${index + 1}`,
      age_secs: 200 + index,
    }),
  );
  return {
    description: "queue length 200, truncation label",
    fleet: fleet({
      projects: [project("vowp", { jobs_by_state: { ready: 200 } })],
      lanes: lanes({
        ready: queue,
        counts: { running: 0, ready: 200, review_fix: 0, parked_blocked: 0, done: 0 },
        queue_truncated: true,
        queue_shown: 200,
      }),
    }),
  };
}

function unknownStates(): ScenarioFixture {
  return {
    description: "new job state / harness value becomes Unknown",
    fleet: fleet({
      projects: [
        project("vowp", {
          jobs_by_state: { quarantined: 1, ready: 1 },
        }),
      ],
      lanes: lanes({
        ready: [jobCard({ job_id: "ss-40-ready" })],
        parked_blocked: [
          jobCard({
            job_id: "ss-41-quarantined",
            title: "New state from a future VOWP",
            state: "quarantined",
            kind: "future-kind",
            reason: "unknown_state",
            detail: "VOWP used a state this desktop does not know.",
          }),
        ],
        counts: { running: 0, ready: 1, review_fix: 0, parked_blocked: 1, done: 0 },
        queue_shown: 2,
      }),
    }),
  };
}

function noAdvice(): ScenarioFixture {
  return {
    description: "advised null, editor disabled advice action",
    fleet: fleet({
      capacity: capacity({
        advised_workers: null,
        target: {
          mode: "fixed",
          target_workers: 6,
          advised_workers: null,
          is_override: false,
          set_by: "owner",
          set_at: NOW,
          reason: "no advice yet",
        },
      }),
      projects: [project("vowp", { jobs_by_state: {} })],
    }),
  };
}

function diskDisagreement(): ScenarioFixture {
  return {
    description: "snapshot vs capacity report disk values; report wins with source label",
    fleet: fleet({
      capacity: capacity({
        host: hostSample({ disk_free_home_bytes: 18 * GIB }),
        disk: {
          free_home_bytes: 18 * GIB,
          free_worktrees_bytes: 18 * GIB,
          floor_bytes: 6 * GIB,
          source: "capacity_report",
          state: "ok",
        },
      }),
      projects: [project("vowp", { jobs_by_state: {} })],
    }),
  };
}

const includedEngine: EngineStatus = {
  choice: { type: "bundled" },
  state: { state: "ready" },
  version: "0.1.0",
  binary_path: "/Applications/StackSpirit Engine/opencode",
  port: 43117,
  selected_model: {
    id: "deepseek/deepseek-flash",
    label: "DeepSeek V4.1 Flash",
    provider: "deepseek",
    access: "included",
    context_tokens: 128_000,
    notes: "Included.",
    verified_at: NOW,
  },
  included_models_available: true,
  started_at: NOW,
  last_error: null,
};

const emptyEngine: EngineStatus = {
  ...includedEngine,
  selected_model: null,
  included_models_available: false,
  state: { state: "stopped" },
  version: null,
  binary_path: null,
  port: null,
  started_at: null,
};

const externalEngine: EngineStatus = {
  ...emptyEngine,
  choice: { type: "external", kind: "opencode", path: "/opt/homebrew/bin/opencode" },
  version: "1.2.3",
  binary_path: "/opt/homebrew/bin/opencode",
};

function gateSnapshot(
  local: GateSnapshot["local_orchestrator"],
  asif: GateSnapshot["asif"],
): GateSnapshot {
  return {
    probed_at: NOW,
    local_orchestrator: local,
    asif,
    local_agent: {
      state: "not_installed",
      node: null,
      protocol: null,
      last_heartbeat: null,
      last_error: null,
    },
    features: { asif_actions: false, local_orchestrator_actions: false },
  };
}

const ready = gateSnapshot(
  {
    state: "ready",
    agent_protocol: "vowp-agent/1",
    node: "macbook",
    capabilities: ["node_card", "offload_status", "surface_start"],
    evidence: ["synthetic capability evidence"],
  },
  {
    state: "ready",
    export: true,
    import: true,
    detected_version: "0.4.0",
    evidence: ["synthetic export/import evidence"],
  },
);

const waiting = gateSnapshot(
  {
    state: "waiting_for_vowp_phase3",
    missing: ["local opencode app host", "surface bridge", "MCP proxy"],
    detected_version: "0.4.0",
    evidence: ["vowp agent surface start --help: exit 2"],
  },
  {
    state: "waiting_for_vowp_asif",
    missing: ["v4-asif-core", "v4-asif-cli"],
    detected_version: "0.4.0",
    evidence: ["session export --format asif: exit 2"],
  },
);

export const SCENARIO_FIXTURES = {
  "fresh-single": freshSingle(),
  idle: idle(),
  busy: busy(),
  "storage-low": storageLow(),
  "memory-pressure": memoryPressure(),
  "provider-limit": providerLimit(),
  "supervisor-down": supervisorDown(),
  "stale-cache": staleCache(),
  "multi-project": multiProject(),
  "queue-cap": queueCap(),
  "unknown-states": unknownStates(),
  "no-advice": noAdvice(),
  "disk-disagreement": diskDisagreement(),
  "engine-included": { ...freshSingle(), engine: includedEngine, description: "included models available" },
  "engine-no-included": {
    ...freshSingle(),
    engine: emptyEngine,
    description: "BYOK empty state",
  },
  "engine-external": {
    ...freshSingle(),
    engine: externalEngine,
    description: "external opencode detected",
  },
  "gate-phase3-waiting": {
    ...freshSingle(),
    gates: gateSnapshot(waiting.local_orchestrator, waiting.asif),
    description: "local orchestrator card waiting with exact missing capabilities",
  },
  "gate-phase3-ready": {
    ...freshSingle(),
    gates: gateSnapshot(
      ready.local_orchestrator,
      waiting.asif,
    ),
    description: "synthetic capability evidence; gate Ready, action disabled by flag",
  },
  "gate-asif-waiting": {
    ...freshSingle(),
    gates: gateSnapshot(waiting.local_orchestrator, waiting.asif),
    description: "ASIF card waiting with v4-asif-* names",
  },
  "gate-asif-ready": {
    ...freshSingle(),
    gates: gateSnapshot(waiting.local_orchestrator, ready.asif),
    description: "synthetic export/import evidence; actions disabled by flag",
  },
} satisfies Record<string, ScenarioFixture>;

export type ScenarioName = keyof typeof SCENARIO_FIXTURES;

export function scenario(name: ScenarioName): ScenarioFixture {
  return structuredClone(SCENARIO_FIXTURES[name]);
}

export const FAKE_SECRET_TOKENS = {
  sk: ["sk", "abcdefgh12345678"].join("_"),
  ghp: ["ghp", "abcdefgh1234567890"].join("_"),
};

export function rawWithWarnings(): RawDocument {
  return {
    status: {
      snapshot: {
        schema: "vowp-snapshot/1",
        version: 1,
        sequence: 4412,
        generated_at: NOW,
        home: "vps",
      },
      source: "remote",
      fetched_at: NOW,
      stale: false,
      authorization: `Bearer ${FAKE_SECRET_TOKENS.sk}`,
    },
    capacity: {
      report: {
        generated_at: NOW,
        target: { mode: "fixed", target_workers: 8 },
        token: FAKE_SECRET_TOKENS.ghp,
      },
    },
    status_warnings: [
      {
        path: "snapshot.projects[0].state",
        code: "unknown_enum",
        detail: 'unknown value "paused_forever"; shown as Unknown',
      },
      {
        path: "snapshot.projects[0].queue",
        code: "missing_optional",
        detail: "optional field absent; using the documented default",
      },
    ],
    capacity_warnings: [
      {
        path: "report.estimates",
        code: "truncated",
        detail: "VOWP capped the estimate list; the view labels the truncation.",
      },
    ],
    fetched_at: NOW,
    generated_at: NOW,
  };
}

export const PLAN_10_2_SCENARIOS: ScenarioName[] = [
  "fresh-single",
  "idle",
  "busy",
  "storage-low",
  "memory-pressure",
  "provider-limit",
  "supervisor-down",
  "stale-cache",
  "multi-project",
  "queue-cap",
  "unknown-states",
  "no-advice",
  "disk-disagreement",
  "engine-included",
  "engine-no-included",
  "engine-external",
  "gate-phase3-waiting",
  "gate-phase3-ready",
  "gate-asif-waiting",
  "gate-asif-ready",
];
