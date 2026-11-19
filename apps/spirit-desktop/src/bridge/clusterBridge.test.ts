import { beforeEach, describe, expect, it, vi } from "vitest";

import type {
  BackupPolicyView,
  ClusterConfigView,
  ClusterFormInput,
  ClusterFormedView,
  ClusterPromoteInput,
  ClusterPromoteOutcome,
  ClusterStateView,
} from "@stackspirit/ui";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { TauriBridge } from "./TauriBridge";

const AT = "2026-11-19T10:00:00.000Z";

const CONFIG: ClusterConfigView = {
  home: "host-home",
  workers: [
    { host: "host-worker-a", node_name: "worker-a", role: "backup", agent_state: "enrolled" },
  ],
  backup: { every_secs: 86_400, keep: 7, schedule: "every 1d" },
};

const STATE: ClusterStateView = {
  configured: true,
  config: CONFIG,
  health: null,
  health_error: null,
  promotion: null,
};

const FORMED: ClusterFormedView = {
  config: CONFIG,
  backup_schedule: "cluster-backup",
  offload_config_changed: true,
  formed_at: AT,
};

const OUTCOME: ClusterPromoteOutcome = {
  new_home: "host-worker-a",
  node_name: "worker-a",
  backup_at: AT,
  promoted_at: AT,
};

const POLICY: BackupPolicyView = { every_secs: 86_400, keep: 7, schedule: "every 1d" };

function formInput(): ClusterFormInput {
  return {
    home: "host-home",
    workers: [{ host: "host-worker-a", node_name: "worker-a", role: "backup" }],
    backup: { every_secs: 86_400, keep: 7 },
    home_project: { name: "stackspirit", root: "/srv/code/stackspirit" },
    worker_projects: [],
    backup_target: "backup-host:/srv/vowp-backups",
    confirmed: true,
  };
}

function promoteInput(): ClusterPromoteInput {
  return {
    target: "host-worker-a",
    backup_dir: "/srv/vowp-backups/cluster-backup",
    new_home: null,
    confirmation: "promote-backup-to-home",
  };
}

describe("TauriBridge cluster commands", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("runs the exact five cluster commands with the exact argument shapes", async () => {
    const bridge = new TauriBridge();
    const cluster = bridge.cluster!;
    const form = formInput();
    invoke.mockResolvedValueOnce(FORMED);
    await expect(cluster.form(form)).resolves.toEqual(FORMED);
    expect(invoke).toHaveBeenLastCalledWith("cluster_form", { input: form });

    invoke.mockResolvedValueOnce(STATE);
    await expect(cluster.health()).resolves.toEqual(STATE);
    expect(invoke).toHaveBeenLastCalledWith("cluster_health", undefined);

    const promote = promoteInput();
    invoke.mockResolvedValueOnce(OUTCOME);
    await expect(cluster.promote(promote)).resolves.toEqual(OUTCOME);
    expect(invoke).toHaveBeenLastCalledWith("cluster_promote", { input: promote });

    invoke.mockResolvedValueOnce(POLICY);
    await expect(cluster.backupPolicy()).resolves.toEqual(POLICY);
    expect(invoke).toHaveBeenLastCalledWith("cluster_backup_policy", undefined);

    invoke.mockResolvedValueOnce(CONFIG);
    await expect(cluster.removeWorker("host-worker-a")).resolves.toEqual(CONFIG);
    expect(invoke).toHaveBeenLastCalledWith("cluster_remove_worker", {
      hostId: "host-worker-a",
    });

    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "cluster_form",
      "cluster_health",
      "cluster_promote",
      "cluster_backup_policy",
      "cluster_remove_worker",
    ]);
  });
});
