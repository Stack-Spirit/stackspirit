/**
 * Fixed copy from `docs/WEB.md` §3.7. These strings are part of the contract; tests assert
 * them verbatim. Do not paraphrase them in a view.
 */
export const CAPACITY_DETAIL_UNAVAILABLE =
  "Capacity detail unavailable — this daemon has no capacity/storage read route (GET /api/v1/capacity).";

export const CAPACITY_DETAIL_EXPLAIN =
  "The board reports job counts, configured worker slots, active limits, cost, and the inbox count. It does not report disk, RAM, CPU pressure, the worker target, or the brake state, so StackSpirit shows no values instead of zeroes or a guessed target.";

export const SUPERVISOR_STATE_UNAVAILABLE =
  "Supervisor state unavailable — this daemon exposes only the last heartbeat, not running, pid, paused, or fail-streak state.";

export const PROJECT_STATE_UNAVAILABLE =
  "Project state unavailable — the daemon does not report active, paused, or archived.";

export const HOSTS_DESKTOP_ONLY =
  "Host profiles and SSH keys live in the desktop app. This Web build talks to one daemon and cannot add, connect, or switch hosts.";

export const ENGINE_DESKTOP_ONLY =
  "The StackSpirit Engine runs beside the desktop app. This Web build cannot start the engine, manage provider keys, or switch models.";

export const GATES_DESKTOP_ONLY =
  "Capability gates are probed over SSH from the desktop app. This Web build cannot probe them.";

export const READ_ONLY_NOTICE =
  "Read-only Web build. Job and inbox actions require the local write proxy.";

export const GATE_FAILURES_UNAVAILABLE =
  "Gate failure detail is unavailable — the daemon has no job-gate read route.";

export const SESSION_SURFACES_UNAVAILABLE =
  "Session surfaces are unavailable — the daemon does not expose active surface leases or last-event times.";

export const ORCHESTRATORS_UNAVAILABLE =
  "Orchestrators are unavailable — the daemon has no orchestrator read route.";

export const RAW_DOCUMENT_UNAVAILABLE =
  "Raw VOWP documents are unavailable — the daemon serves board and list documents, not the fleet-status or capacity-report documents.";

export const SAME_ORIGIN_REQUIRED =
  "This daemon has no CORS support. Serve this app and /api/v1 from the same origin through the reverse proxy.";

export const JOB_DETAIL_UNAVAILABLE =
  "Job detail is unavailable — the daemon does not serve GET /api/v1/jobs/{id}, /runs, or /gates.";

export const JOB_PAGE_TRUNCATED =
  "This daemon paginated the job list; only the returned page is shown. The board counts remain exact.";

export const WRITES_DISABLED =
  "Writes are disabled on this daemon. The local write proxy is not configured.";
