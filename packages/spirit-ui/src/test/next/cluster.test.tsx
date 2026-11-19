import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SpiritBridge } from "../../bridge/SpiritBridge";
import { MockBridge } from "../../bridge/MockBridge";
import type {
  ClusterAgentState,
  ClusterConfigView,
  ClusterFormInput,
  ClusterFormedView,
  ClusterHealthView,
  ClusterHostHealthView,
  ClusterPromoteInput,
  ClusterPromoteOutcome,
  ClusterStateView,
  HostProfile,
} from "../../bridge/wire";
import { ClusterSection } from "../../next/ClusterSection";
import { FormClusterDialog } from "../../next/FormClusterDialog";
import { PromoteBackupDialog } from "../../next/PromoteBackupDialog";
import { Sidebar } from "../../next/Sidebar";
import { resetClusterStore, useClusterStore } from "../../next/clusterStore";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const AT = "2026-11-18T11:00:00.000Z";
const BACKUP_AT = "2026-11-18T11:30:00.000Z";

function host(id: string, label: string, address: string): HostProfile {
  return {
    id,
    label,
    ssh: { user: "owner", host: address, port: 22 },
    identity_file: null,
    vowp_home: null,
    default_project: null,
    host_key: {
      algorithm: "ssh-ed25519",
      sha256: "SHA256:clusterTestHost000000000000000000000000",
      pinned_at: AT,
    },
    created_at: AT,
    updated_at: AT,
    last_connected_at: null,
  };
}

const HOSTS = [
  host("host-home", "home-vps", "203.0.113.10"),
  host("host-worker-a", "worker-a", "203.0.113.11"),
  host("host-worker-b", "worker-b", "203.0.113.12"),
];

function row(
  hostId: string,
  role: ClusterHostHealthView["role"],
  nodeName: string | null,
  agentState: ClusterAgentState,
  slotsBusy: number,
  slotsMax: number,
): ClusterHostHealthView {
  return {
    host: hostId,
    label: role === "home" ? "Home" : `worker ${nodeName ?? hostId}`,
    role,
    node_name: nodeName,
    agent_state: agentState,
    status: agentState === "enrolled" ? "active" : "unknown",
    presence: agentState === "enrolled" ? "online" : "offline",
    registered: agentState !== "missing",
    slots_busy: slotsBusy,
    slots_max: slotsMax,
    mapped_projects: [],
    last_seen_at: AT,
    last_error: null,
  };
}

function config(workers: ClusterConfigView["workers"]): ClusterConfigView {
  return {
    home: "host-home",
    workers,
    backup: { every_secs: 86_400, keep: 7, schedule: "every 1d" },
  };
}

const TWO_WORKERS = config([
  { host: "host-worker-a", node_name: "worker-a", role: "backup", agent_state: "enrolled" },
  { host: "host-worker-b", node_name: "worker-b", role: "worker", agent_state: "enrolled" },
]);

function health(overrides: Partial<ClusterHealthView> = {}): ClusterHealthView {
  return {
    home_node: "home-node",
    online_workers: 2,
    home_target_workers: 4,
    worker_slots: 8,
    pooled_target_workers: 12,
    busy_workers: 3,
    cluster_totals_present: false,
    totals_source: "computed",
    hosts: [
      row("host-home", "home", "home-node", "enrolled", 3, 4),
      row("host-worker-a", "backup", "worker-a", "enrolled", 0, 4),
      row("host-worker-b", "worker", "worker-b", "enrolled", 0, 4),
    ],
    warnings: [],
    checked_at: AT,
    ...overrides,
  };
}

function clusterState(withHealth: ClusterHealthView | null, promotion: ClusterStateView["promotion"] = null): ClusterStateView {
  return {
    configured: true,
    config: TWO_WORKERS,
    health: withHealth,
    health_error: null,
    promotion,
  };
}

const FORMED: ClusterFormedView = {
  config: TWO_WORKERS,
  backup_schedule: "cluster-backup",
  offload_config_changed: true,
  formed_at: AT,
};

const OUTCOME: ClusterPromoteOutcome = {
  new_home: "host-worker-a",
  node_name: "worker-a",
  backup_at: BACKUP_AT,
  promoted_at: AT,
};

interface ClusterOverrides {
  form?: (input: ClusterFormInput) => Promise<ClusterFormedView>;
  health?: () => Promise<ClusterStateView>;
  promote?: (input: ClusterPromoteInput) => Promise<ClusterPromoteOutcome>;
  removeWorker?: (hostId: string) => Promise<ClusterConfigView>;
}

function fakeBridge(overrides: ClusterOverrides = {}): SpiritBridge {
  return {
    kind: "tauri",
    preview: false,
    hosts: { list: async () => HOSTS },
    cluster: {
      form:
        overrides.form ??
        (async () => FORMED),
      health:
        overrides.health ??
        (async () => clusterState(health())),
      promote:
        overrides.promote ??
        (async () => OUTCOME),
      backupPolicy: async () => TWO_WORKERS.backup,
      removeWorker:
        overrides.removeWorker ??
        (async () => config([TWO_WORKERS.workers[0]!])),
    },
  } as unknown as SpiritBridge;
}

function seed(cluster: ClusterStateView | null, bridge: SpiritBridge = fakeBridge()) {
  resetFleetStore();
  resetClusterStore();
  useFleetStore.setState({
    bridge,
    status: "ready",
    preview: false,
    connection: {
      state: "connected",
      host_id: HOSTS[0]!.id,
      since: AT,
      latency: null,
      last_ok_at: AT,
      error: null,
    },
    hosts: HOSTS,
    hostsLoaded: true,
    selectedHostId: HOSTS[0]!.id,
  });
  useClusterStore.setState({
    cluster,
    clusterStatus: cluster ? "ready" : "idle",
    clusterError: null,
  });
  return bridge;
}

function sidebar() {
  return (
    <Sidebar
      view={{ kind: "server" }}
      onNavigate={() => undefined}
      onAddServer={() => undefined}
      onAddProject={() => undefined}
      onFormCluster={() => undefined}
      theme="system"
      onTheme={() => undefined}
      resizeHandle={{}}
      resizing={false}
      onOpenRaw={() => undefined}
    />
  );
}

async function fillRequiredForm(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByLabelText("Use worker-a as a worker"));
  await user.click(screen.getByLabelText("Use worker-b as a worker"));
  await user.type(screen.getByLabelText("Project name on Home"), "stackspirit");
  await user.type(screen.getByLabelText("Project checkout on Home"), "/srv/code/stackspirit");
  await user.type(screen.getByLabelText("Backup destination"), "backup-host:/srv/vowp-backups");
  await user.click(screen.getByLabelText("Confirm cluster changes"));
}

afterEach(() => {
  resetFleetStore();
  resetClusterStore();
});

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

async function wcagViolations(container: HTMLElement) {
  const results = await axe.run(container, {
    runOnly: { type: "tag", values: WCAG_TAGS },
  });
  return results.violations;
}

describe("cluster pooled health", () => {
  it("shows the pooled numbers from ClusterHealth in the Server section and sidebar", async () => {
    seed(clusterState(health()));
    const user = userEvent.setup();
    render(
      <>
        {sidebar()}
        <ClusterSection onFormCluster={() => undefined} onPromote={() => undefined} />
      </>,
    );

    const pooled = screen.getByTestId("cluster-pooled");
    expect(pooled).toHaveTextContent("3 of 12 pooled workers busy");
    expect(pooled).toHaveTextContent("Home 4 + 2 online workers (8 slots) = 12 workers");
    expect(pooled).toHaveTextContent("Pooled totals computed from Home");

    const card = screen.getByTestId("server-card");
    expect(card).toHaveAttribute("data-cluster", "true");
    expect(within(card).getByTestId("cluster-badge")).toHaveTextContent("cluster");
    expect(within(card).getByTestId("health-card")).toHaveAccessibleName(
      /cluster 3 of 12 workers busy, 2 workers online/,
    );
    expect(within(card).getByText("3")).toBeInTheDocument();
    expect(within(card).getByText(/\/12 workers busy/)).toBeInTheDocument();

    // The cluster menu action is available with two or more saved servers.
    await user.click(screen.getByRole("button", { name: "Server options" }));
    expect(screen.getByRole("menuitem", { name: /Form a cluster/ })).toBeInTheDocument();
  });

  it("shows the honest reduced total and names the offline worker", async () => {
    seed(
      clusterState(
        health({
          online_workers: 1,
          worker_slots: 4,
          pooled_target_workers: 8,
          busy_workers: 1,
          hosts: [
            row("host-home", "home", "home-node", "enrolled", 1, 4),
            row("host-worker-a", "backup", "worker-a", "enrolled", 0, 4),
            row("host-worker-b", "worker", "worker-b", "offline", 0, 4),
          ],
        }),
      ),
    );
    render(<ClusterSection onFormCluster={() => undefined} onPromote={() => undefined} />);

    const pooled = screen.getByTestId("cluster-pooled");
    expect(pooled).toHaveTextContent("Home 4 + 1 online worker (4 slots) = 8 workers");
    expect(pooled).toHaveTextContent("1 worker offline and not counted: worker-b");
    expect(pooled).not.toHaveTextContent("= 12 workers");
    expect(within(screen.getByTestId("cluster-section")).getByText(/Offline/)).toBeInTheDocument();
  });

  it("does not show a cluster badge while every worker is offline", () => {
    seed(
      clusterState(
        health({
          online_workers: 0,
          worker_slots: 0,
          pooled_target_workers: 4,
          busy_workers: 1,
          hosts: [
            row("host-home", "home", "home-node", "enrolled", 1, 4),
            row("host-worker-a", "backup", "worker-a", "offline", 0, 4),
            row("host-worker-b", "worker", "worker-b", "offline", 0, 4),
          ],
        }),
      ),
    );
    render(sidebar());
    const card = screen.getByTestId("server-card");
    expect(card).not.toHaveAttribute("data-cluster");
    expect(within(card).queryByTestId("cluster-badge")).not.toBeInTheDocument();
  });
});

describe("Form a cluster", () => {
  it("requires one worker, unique node names and the explicit confirmation", async () => {
    const form = vi.fn(async () => FORMED);
    seed(null, fakeBridge({ form }));
    const user = userEvent.setup();
    render(<FormClusterDialog hosts={HOSTS} initial={null} onClose={() => undefined} />);

    const submit = screen.getByRole("button", { name: "Form cluster" });
    expect(submit).toBeDisabled();

    await user.click(screen.getByLabelText("Use worker-a as a worker"));
    await user.click(screen.getByLabelText("Use worker-b as a worker"));
    expect(screen.getByLabelText("Node name for worker-a")).toHaveValue("worker-a");
    expect(screen.getByLabelText("Node name for worker-b")).toHaveValue("worker-b");
    await user.type(
      screen.getByLabelText("Project checkout on worker-a"),
      "/srv/code/stackspirit",
    );

    await user.clear(screen.getByLabelText("Node name for worker-b"));
    await user.type(screen.getByLabelText("Node name for worker-b"), "worker-a");
    expect(screen.getByText("Node names must be unique inside the cluster.")).toBeInTheDocument();
    expect(submit).toBeDisabled();

    await user.clear(screen.getByLabelText("Node name for worker-b"));
    await user.type(screen.getByLabelText("Node name for worker-b"), "worker-b");
    await user.type(screen.getByLabelText("Project name on Home"), "stackspirit");
    await user.type(screen.getByLabelText("Project checkout on Home"), "/srv/code/stackspirit");
    await user.type(screen.getByLabelText("Backup destination"), "backup-host:/srv/vowp-backups");
    expect(submit).toBeDisabled();
    await user.click(screen.getByLabelText("Confirm cluster changes"));
    expect(submit).toBeEnabled();

    await user.click(submit);
    await waitFor(() => expect(form).toHaveBeenCalledTimes(1));
    expect(form).toHaveBeenCalledWith({
      home: "host-home",
      workers: [
        { host: "host-worker-a", node_name: "worker-a", role: "worker" },
        { host: "host-worker-b", node_name: "worker-b", role: "worker" },
      ],
      backup: { every_secs: 86_400, keep: 7 },
      home_project: { name: "stackspirit", root: "/srv/code/stackspirit" },
      worker_projects: [
        {
          worker: "host-worker-a",
          project: "stackspirit",
          root: "/srv/code/stackspirit",
          worktree_root: null,
          allow: [],
        },
      ],
      backup_target: "backup-host:/srv/vowp-backups",
      confirmed: true,
    });
    expect(await screen.findByTestId("cluster-formed")).toHaveTextContent(
      "2 Worker/Backup servers are enrolled",
    );
  });

  it("keeps the form and shows a typed failure inline", async () => {
    const form = vi.fn(async () => {
      throw {
        code: "ssh_auth",
        message: "worker worker-a VOWP preflight: SSH authentication failed",
        hint: "Check the worker's key login and retry.",
        retryable: false,
      };
    });
    seed(null, fakeBridge({ form }));
    const user = userEvent.setup();
    render(<FormClusterDialog hosts={HOSTS} initial={null} onClose={() => undefined} />);

    await fillRequiredForm(user);
    await user.click(screen.getByRole("button", { name: "Form cluster" }));

    const error = await screen.findByTestId("cluster-error");
    expect(error).toHaveTextContent("ssh_auth");
    expect(error).toHaveTextContent("SSH authentication failed");
    expect(screen.getByLabelText("Home server")).toHaveValue("host-home");
    expect(screen.getByLabelText("Backup destination")).toHaveValue(
      "backup-host:/srv/vowp-backups",
    );
    expect(screen.getByRole("button", { name: "Form cluster" })).toBeEnabled();
  });
});

describe("Promote backup to Home", () => {
  it("requires the exact typed phrase, then shows the restored Home", async () => {
    let resolvePromote: ((outcome: ClusterPromoteOutcome) => void) | undefined;
    const promote = vi.fn(
      (input: ClusterPromoteInput) =>
        new Promise<ClusterPromoteOutcome>((resolve) => {
          void input;
          resolvePromote = resolve;
        }),
    );
    seed(clusterState(health()), fakeBridge({ promote }));
    const onConnect = vi.fn();
    const user = userEvent.setup();
    render(
      <PromoteBackupDialog
        hosts={HOSTS}
        initialTarget="host-worker-a"
        onClose={() => undefined}
        onConnect={onConnect}
      />,
    );

    expect(screen.getByLabelText("Backup source")).toHaveValue("host-worker-a");
    const submit = screen.getByRole("button", { name: "Promote backup" });
    await user.type(
      screen.getByLabelText("Backup directory on the target"),
      "/srv/vowp-backups/cluster-backup",
    );
    expect(submit).toBeDisabled();

    const phrase = screen.getByLabelText(/Type promote-backup-to-home to confirm/);
    await user.type(phrase, "promote-backup");
    expect(screen.getByText("The confirmation phrase does not match exactly.")).toBeInTheDocument();
    expect(submit).toBeDisabled();

    await user.clear(phrase);
    await user.type(phrase, "promote-backup-to-home");
    expect(screen.queryByText("The confirmation phrase does not match exactly.")).not.toBeInTheDocument();
    expect(submit).toBeEnabled();

    await user.click(submit);
    expect(screen.getByRole("button", { name: "Promoting…" })).toBeDisabled();
    expect(promote).toHaveBeenCalledWith({
      target: "host-worker-a",
      backup_dir: "/srv/vowp-backups/cluster-backup",
      new_home: null,
      confirmation: "promote-backup-to-home",
    });

    await act(async () => {
      resolvePromote?.(OUTCOME);
    });
    const outcome = await screen.findByTestId("promote-outcome");
    expect(outcome).toHaveTextContent("VOWP restored the backup and started the supervisor on worker-a");
    expect(outcome).toHaveTextContent("worker-a");
    expect(outcome).toHaveTextContent("2026-11-18 11:30 UTC");
    expect(outcome).toHaveTextContent("The active server has not been switched");

    await user.click(screen.getByRole("button", { name: "Connect to worker-a" }));
    expect(onConnect).toHaveBeenCalledWith("host-worker-a");
  });

  it("shows an honest refusal and keeps the form", async () => {
    const promote = vi.fn(async () => {
      throw {
        code: "refused",
        message: "backup is stale: it is 90000s old and the policy allows 86400s",
        hint: "Run or copy a newer `vowp backup` before promoting it.",
        retryable: false,
      };
    });
    seed(clusterState(health()), fakeBridge({ promote }));
    const user = userEvent.setup();
    render(
      <PromoteBackupDialog
        hosts={HOSTS}
        initialTarget="host-worker-a"
        onClose={() => undefined}
        onConnect={() => undefined}
      />,
    );

    await user.type(
      screen.getByLabelText("Backup directory on the target"),
      "/srv/vowp-backups/cluster-backup",
    );
    await user.type(
      screen.getByLabelText(/Type promote-backup-to-home to confirm/),
      "promote-backup-to-home",
    );
    await user.click(screen.getByRole("button", { name: "Promote backup" }));

    const error = await screen.findByTestId("cluster-error");
    expect(error).toHaveTextContent("refused");
    expect(error).toHaveTextContent("backup is stale");
    expect(error).toHaveTextContent("Run or copy a newer");
    expect(screen.getByLabelText("Backup directory on the target")).toHaveValue(
      "/srv/vowp-backups/cluster-backup",
    );
    expect(screen.getByRole("button", { name: "Promote backup" })).toBeEnabled();
  });
});

describe("preview cluster honesty", () => {
  it("returns a typed preview_unavailable result instead of a fake cluster", async () => {
    const bridge = new MockBridge("fresh");
    await expect(bridge.cluster!.health()).rejects.toMatchObject({
      code: "preview_unavailable",
      retryable: false,
    });
    await expect(bridge.cluster!.backupPolicy()).rejects.toMatchObject({
      code: "preview_unavailable",
    });
    await expect(
      bridge.cluster!.form({
        home: "host-home",
        workers: [],
        backup: { every_secs: 86_400, keep: 7 },
        home_project: { name: "stackspirit", root: "/srv/code/stackspirit" },
        worker_projects: [],
        backup_target: "backup-host:/srv/vowp-backups",
        confirmed: true,
      }),
    ).rejects.toMatchObject({ code: "preview_unavailable" });
  });
});

describe("cluster accessibility", () => {
  it("has no WCAG 2.2 AA axe violations in the cluster section", async () => {
    seed(clusterState(health()));
    const { container } = render(
      <ClusterSection onFormCluster={() => undefined} onPromote={() => undefined} />,
    );
    expect(await wcagViolations(container)).toEqual([]);
  });

  it("has no WCAG 2.2 AA axe violations in the cluster form", async () => {
    seed(null);
    const { container } = render(
      <FormClusterDialog hosts={HOSTS} initial={null} onClose={() => undefined} />,
    );
    // The modal portals to body; scan the dialog rather than the render container.
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]');
    expect(dialog).not.toBeNull();
    expect(await wcagViolations(dialog!)).toEqual([]);
    expect(container.isConnected).toBe(true);
  });
});
