import type {
  ClusterAgentState,
  ClusterHealthView,
  ClusterHostHealthView,
  ClusterWorkerRole,
  HostProfile,
} from "../bridge/wire";

/**
 * The exact phrase the owner types to promote a backup to Home. This mirrors
 * `spirit_core::cluster::PROMOTE_CONFIRMATION_TOKEN`; the host re-checks it before
 * draining anything, so the renderer copy is only the first gate.
 */
export const PROMOTE_CONFIRMATION = "promote-backup-to-home";

/** One worker row as the form keeps it while it is being edited. */
export interface ClusterWorkerDraft {
  hostId: string;
  nodeName: string;
  role: ClusterWorkerRole;
}

/**
 * The editable cluster form. Paths live only in this draft: they are sent once with
 * the form and never stored in the fleet store or the host's cluster record.
 */
export interface ClusterFormDraft {
  homeId: string;
  workers: ClusterWorkerDraft[];
  /** Optional absolute checkout per worker, keyed by host id. */
  workerRoots: Record<string, string>;
  everySecs: number;
  keep: number;
  projectName: string;
  projectRoot: string;
  backupTarget: string;
  confirmed: boolean;
}

export interface ClusterFormErrors {
  home?: string;
  workers?: string;
  nodeNames: Record<string, string>;
  workerRoots: Record<string, string>;
  every?: string;
  keep?: string;
  projectName?: string;
  projectRoot?: string;
  backupTarget?: string;
}

const NODE_NAME = /^[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?$/;
const PROJECT_NAME = NODE_NAME;

/** Formats a whole number of seconds the way `BackupPolicy::schedule_spec` does. */
export function formatInterval(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "not set";
  if (seconds % 86_400 === 0) return `${seconds / 86_400}d`;
  if (seconds % 3_600 === 0) return `${seconds / 3_600}h`;
  if (seconds % 60 === 0) return `${seconds / 60}m`;
  return `${seconds}s`;
}

/** A deterministic UTC timestamp for read-only cluster rows. */
export function formatTimestamp(timestamp: string): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return timestamp;
  return `${date.toISOString().slice(0, 16).replace("T", " ")} UTC`;
}

/** A VOWP node-name suggestion from a saved host label. */
export function suggestNodeName(label: string, fallback = "worker"): string {
  const slug = label
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64)
    .replace(/-+$/g, "");
  return slug.length > 0 && /^[a-z0-9]/.test(slug) ? slug : fallback;
}

/** A unique version of `base` against names already used in the form. */
export function uniqueNodeName(base: string, used: Iterable<string>): string {
  const taken = new Set(used);
  if (!taken.has(base)) return base;
  for (let index = 2; index < 1000; index += 1) {
    const candidate = `${base.slice(0, 60)}-${index}`;
    if (!taken.has(candidate)) return candidate;
  }
  return base;
}

/** True when a target looks like it embeds a password or token. */
export function backupTargetEmbedsCredentials(target: string): boolean {
  const value = target.trim();
  if (value.includes("://")) return true;
  const at = value.indexOf("@");
  if (at <= 0) return false;
  return value.slice(0, at).includes(":");
}

/** True when a value contains a C0 control character or DEL. */
function hasControlCharacters(value: string): boolean {
  return [...value].some((char) => {
    const code = char.charCodeAt(0);
    return code < 0x20 || code === 0x7f;
  });
}

/** Mirrors the core's absolute-POSIX-path rule for the form's remote paths. */
export function isAbsoluteRemotePath(path: string): boolean {
  const value = path.trim();
  return (
    value.startsWith("/") &&
    value.length > 1 &&
    !value.endsWith("/") &&
    !value.includes("//") &&
    !hasControlCharacters(value) &&
    !value.split("/").some((part) => part === "." || part === "..")
  );
}

/** Validates the form without touching a host. Empty object means ready to confirm. */
export function validateClusterDraft(
  draft: ClusterFormDraft,
  hosts: HostProfile[],
): ClusterFormErrors {
  const errors: ClusterFormErrors = { nodeNames: {}, workerRoots: {} };

  if (hosts.length < 2) {
    errors.home = "Save at least two servers before forming a cluster.";
  }
  const home = hosts.find((host) => host.id === draft.homeId);
  if (!home) {
    errors.home = "Choose exactly one Home server.";
  }

  if (draft.workers.length === 0) {
    errors.workers = "Choose at least one Worker/Backup server.";
  }
  const seen = new Set<string>();
  for (const worker of draft.workers) {
    if (seen.has(worker.hostId)) {
      errors.workers = "A saved server may appear only once in a cluster.";
      break;
    }
    seen.add(worker.hostId);
    if (worker.hostId === draft.homeId) {
      errors.workers = "The Home server cannot also be a worker.";
    }
  }

  const usedNodes = new Set<string>();
  for (const worker of draft.workers) {
    const node = worker.nodeName.trim();
    if (!NODE_NAME.test(node)) {
      errors.nodeNames[worker.hostId] =
        "Use a lowercase VOWP node name: letters, digits and dashes, 1–64 characters.";
    } else if (usedNodes.has(node)) {
      errors.nodeNames[worker.hostId] = "Node names must be unique inside the cluster.";
    } else {
      usedNodes.add(node);
    }
    const workerRoot = draft.workerRoots[worker.hostId]?.trim() ?? "";
    if (workerRoot.length > 0 && !isAbsoluteRemotePath(workerRoot)) {
      errors.workerRoots[worker.hostId] =
        "Enter an absolute checkout path on this worker, or leave it empty.";
    }
  }

  if (!Number.isInteger(draft.everySecs) || draft.everySecs < 60) {
    errors.every = "VOWP's shortest backup schedule is 60 seconds.";
  }
  if (!Number.isInteger(draft.keep) || draft.keep < 1 || draft.keep > 1000) {
    errors.keep = "Keep between 1 and 1000 copies.";
  }

  if (!PROJECT_NAME.test(draft.projectName.trim())) {
    errors.projectName =
      "Use the lowercase project slug VOWP stores: letters, digits and dashes.";
  }
  if (!isAbsoluteRemotePath(draft.projectRoot)) {
    errors.projectRoot =
      "Enter the project's absolute checkout path on Home, for example /srv/code/project.";
  }
  if (draft.backupTarget.trim().length === 0) {
    errors.backupTarget = "Enter the destination for `vowp backup --to`.";
  } else if (draft.backupTarget.trim().length > 4096) {
    errors.backupTarget = "The backup destination is too long.";
  } else if (draft.backupTarget.trim().startsWith("-")) {
    errors.backupTarget = "The destination must not start with '-'.";
  } else if (hasControlCharacters(draft.backupTarget)) {
    errors.backupTarget = "The destination must not contain control characters.";
  } else if (backupTargetEmbedsCredentials(draft.backupTarget)) {
    errors.backupTarget =
      "Do not put a password or token in the destination; use the server's SSH key.";
  }

  return errors;
}

/** True when the form has no blocking error. */
export function clusterDraftReady(
  draft: ClusterFormDraft,
  errors: ClusterFormErrors,
): boolean {
  return (
    draft.confirmed &&
    draft.homeId.length > 0 &&
    draft.workers.length > 0 &&
    !errors.home &&
    !errors.workers &&
    Object.keys(errors.nodeNames).length === 0 &&
    Object.keys(errors.workerRoots).length === 0 &&
    !errors.every &&
    !errors.keep &&
    !errors.projectName &&
    !errors.projectRoot &&
    !errors.backupTarget
  );
}

/** The pooled numbers the sidebar and Server page show, read from `ClusterHealth`. */
export interface ClusterPooled {
  homeTarget: number;
  onlineWorkers: number;
  workerSlots: number;
  pooled: number;
  busy: number;
  source: ClusterHealthView["totals_source"];
  /** Configured worker rows that are not currently contributing slots. */
  offline: ClusterHostHealthView[];
}

export function clusterPooled(health: ClusterHealthView): ClusterPooled {
  return {
    homeTarget: health.home_target_workers,
    onlineWorkers: health.online_workers,
    workerSlots: health.worker_slots,
    pooled: health.pooled_target_workers,
    busy: health.busy_workers,
    source: health.totals_source,
    offline: health.hosts.filter(
      (host) => host.role !== "home" && host.agent_state !== "enrolled",
    ),
  };
}

/** One short, honest state label for a health row. */
export function agentStateLabel(state: ClusterAgentState): string {
  switch (state) {
    case "enrolled":
      return "Online";
    case "pending":
      return "Pending";
    case "offline":
      return "Offline";
    case "revoked":
      return "Revoked";
    case "missing":
      return "Missing";
    default:
      return "Unknown";
  }
}

/** A saved host's label, with a safe fallback when the profile was deleted. */
export function hostLabel(
  hosts: HostProfile[],
  hostId: string,
  fallback = "unknown server",
): string {
  return hosts.find((host) => host.id === hostId)?.label ?? fallback;
}

/** A health row's saved host label, never the core's generated "worker <node>" label. */
export function healthRowLabel(hosts: HostProfile[], row: ClusterHostHealthView): string {
  return hostLabel(hosts, row.host, row.node_name ?? row.label);
}

/** The restored Home's display label. */
export function promotionLabel(
  hosts: HostProfile[],
  promotion: { new_home: string; node_name: string } | null,
): string | null {
  if (!promotion) return null;
  return hostLabel(hosts, promotion.new_home, promotion.node_name);
}
