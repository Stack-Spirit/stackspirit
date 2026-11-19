# StackSpirit clusters — pooled compute and Home promotion

Status: core (`ss-54-cluster-core`, `fe3146b`) and desktop UI (`ss-55-cluster-ui`,
`58a37f2`, `04c9fdd`), 2026-11-19.
Audience: the CLI, the desktop UI and the owner.

This document describes the typed cluster model in
`crates/spirit-core/src/cluster/mod.rs`, the exact VOWP commands the core runs, the
desktop UI, and the honest failure modes. The Tauri command adapter is
`apps/spirit-desktop/src-tauri/src/commands/cluster.rs`; the UI is covered in §6.

VOWP remains the source of truth. If this document and
`/home/dev/vowp/docs/design/cli.md` or `crates/vowp-protocol/src/` disagree, the VOWP
source wins.

## 1. One Home, one lease

A cluster has exactly one **Home** server. Home owns the global VOWP store, project
stores, session logs and the supervisor; it is the source of truth. Workers are
outbound VOWP agent nodes and never become a second Home by accident.

The rule is enforced in three places:

- `ClusterConfig::validate` refuses an empty worker list, a worker that is the Home,
  duplicate host profiles, duplicate node names and an invalid backup policy.
- `ClusterService::form` also refuses a worker whose SSH target is the Home target and
  refuses fewer than two saved servers, and preflights every worker before the first
  mutation.
- `ClusterService::promote` refuses unless the caller passes the explicit confirmation
  token, the backup is fresh and the target worker answers. It drains the old Home
  first; if the drain cannot be confirmed, it refuses rather than start a second Home.

A promotion returns the new Home identity but does **not** rewrite the saved host
profiles or silently change the active Home. The caller (later the UI) performs that
switch explicitly.

## 2. Model

```rust
pub struct ClusterConfig {
    pub home: HostProfileId,
    pub workers: Vec<ClusterWorker>,
    pub backup: BackupPolicy,
}

pub struct ClusterWorker {
    pub host: HostProfileId,
    pub node_name: String,              // VOWP slug
    pub role: ClusterWorkerRole,        // Worker | Backup
    pub agent_state: ClusterAgentState, // read state; form normalizes to Enrolled
}

pub struct BackupPolicy {
    pub every: Duration,
    pub keep: u32,
}
```

`HostProfileId` is an alias for `HostId`. The model stores local profile ids, never an
SSH key or a secret.

`ClusterHealth` is a read model:

| Field | Meaning |
|---|---|
| `online_workers` | active, online worker nodes; Home is not counted |
| `home_target_workers` | Home's own `target_workers` |
| `worker_slots` | max slots contributed by the online workers |
| `pooled_target_workers` | Home target plus worker slots |
| `busy_workers` | Home running runs plus worker busy slots |
| `cluster_totals_present` | VOWP reported the additive `cluster` card |
| `totals_source` | `vowp_cluster` or `computed` |
| `hosts` | one safe row for Home and one per configured worker |
| `warnings` | honest degradation notes |

Every host row carries only the local host id, a sanitized label, the role, the VOWP
node name, status/presence, slot counts, mapped project slugs, `last_seen_at` and a
sanitized `last_error`. No absolute path, SSH target, token or pairing code is in the
document.

`BackupPolicy::keep` is the owner's retention expectation. VOWP's `vowp backup` rotates
7 daily and 4 weekly copies itself and has no `--keep` flag, so `keep` is not sent to a
command. Promotion freshness uses `every` (see §5).

## 3. Forming a cluster

`ClusterService::form(&ClusterFormRequest)` needs:

- `config` — Home, workers, roles and backup policy;
- `home_project` — project name and absolute checkout on Home whose `vowp.toml` is
  edited. The name must match the checkout's `[project] name`; it is the
  `--project` selector for the backup schedule.
- `worker_projects` — the absolute path map added on each worker;
- `backup_target` — the destination for `vowp backup --to`. It must be a path or
  rsync target and must not embed a password or token; StackSpirit passes it to
  VOWP and never treats it as a secret store.

All inputs are validated before the first remote command. The service then:

1. loads the saved host profiles and refuses fewer than two;
2. preflights Home and every worker with `vowp --version` over the existing verified
   SSH runner (`BatchMode`, strict host-key checking, app-private `known_hosts`,
   `IdentitiesOnly` when an identity file is configured);
3. reads Home's `<home_project>/vowp.toml` with `cat`;
4. for each worker:
   - on Home: `vowp [--home <dir>] --json node enroll <node>`
   - on the worker: `vowp --json agent enroll --target <user@home> --code-file -`,
     with the one-time code written to stdin as one line;
   - on the worker: `vowp --json agent service install --now`
   - on the worker, for every mapping:
     `vowp --json agent paths add --project <root> --name <project>
     [--worktree-root <root>] [--allow <rel>]...`
5. writes Home's `vowp.toml` back with `tee` only when the `[offload]` values changed;
6. registers the backup schedule on Home:
   `vowp [--home <dir>] --project <project> --json schedule add
   "every <interval>" command --name cluster-backup -- vowp [--home <dir>] backup
   --to <backup_target>`.

The `[offload]` editor changes only three keys and preserves every other line,
including unrelated `[offload.projects.*]` and `[offload.nodes.*]` tables:

```toml
[offload]
enabled = true
default = "any"
result_transport = "bundle"
```

The handoff calls the placement setting `default_placement`. VOWP's real TOML key is
`default`; VOWP rejects unknown keys inside `[offload]`, so StackSpirit writes
`default = "any"` and not a second alias.

`default = "any"` is the fleet default. If the project already carries an explicit
`[offload.projects.<name>] placement = "home"`, StackSpirit preserves it because it is
an owner policy; jobs for that project stay Home-only until the owner changes the
project placement. The editor does not rewrite unrelated project policy tables.

`max_remote_runs` is also owner policy and is preserved. VOWP's documented default is
`2`; a cluster that should run more remote jobs at once needs the owner to raise that
cap separately. StackSpirit does not silently replace an owner-set cap.

`vowp agent enroll --target` is passed as one `user@host` argv element. It does not
carry an SSH port; a Home profile with a non-default port is refused with a hint to
use port 22 or an SSH config alias on every worker.

### Pairing-code handling

The code returned by `vowp node enroll --json` is wrapped in `SecretString`
immediately. It is written only to the `vowp agent enroll` child's stdin with
`--code-file -`. It never enters:

- an argv element (including the remote command string);
- `Debug` output for the request, result or service;
- a typed error or hint;
- a log line;
- the config file or a temp file.

`check_cluster_output` deliberately keeps only the command label, exit status and a
typed code from a failed command; remote stdout and stderr are never copied into an
error because they can contain a pty transcript.

### Failure modes while forming

| Failure | Behaviour |
|---|---|
| Fewer than two saved servers | Refused before any command. |
| Worker is Home, duplicate host or duplicate node name | Invalid input before any command. |
| Worker profile points at the Home SSH target | Invalid input before any command. |
| Worker SSH key login fails | Worker preflight returns `ssh_auth`; no enrollment or config write has run. |
| VOWP missing on Home or a worker | Preflight returns `vowp_missing`; no mutation. |
| `node enroll` conflict (node already exists) | `conflict`; previous workers stay enrolled and Home's config is unchanged. |
| `agent enroll` fails on a worker | Home config is still unchanged. The worker must be able to `ssh <user>@<home>` with its own key and trust the Home host key; fix that and revoke the pending node before a retry. |
| `agent service install` unsupported | `refused`; the pending node must be revoked before a retry. |
| `tee` fails after enrollment | Workers are enrolled but pooled compute stays off; re-running form needs the enrollment conflict resolved first. |
| `schedule add` conflicts with an existing `cluster-backup` | The config is enabled but the backup schedule was not replaced; remove the old schedule and re-register. |

There is no hidden retry and no rollback that could delete a worker's credentials. A
partial form is visible and must be completed or cleaned up explicitly.

## 4. Health

`ClusterService::health` reads four documents over SSH from Home:

1. `vowp [--home <dir>] --json fleet status`
2. `vowp [--home <dir>] --json node ls`
3. `vowp [--home <dir>] --json capacity report`
4. `vowp [--home <dir>] --json offload status`

It always includes the `--json` flag. The connection is the same verified runner used
by the rest of the core; no new daemon or port is opened.

Mapping:

- `home_target_workers` ← `capacity report . target.target_workers`
- Home busy runs ← `capacity report . running.runs`
- Home node name ← `fleet status . snapshot.home`
- Home online ← `fleet status . snapshot.supervisor.running`
- Home mapped projects ← `fleet status . snapshot.projects[].name`
- Worker status/presence/slots/mapped/last error ← `offload status . nodes[]`
- Worker registration ← `node ls[]`

When VOWP 0.5 reports the additive `cluster` card, in either `fleet status` or
`capacity report`, the core uses it:

- `online_workers` ← `cluster.online_worker_nodes`
- `pooled_target_workers` ← `cluster.target_workers`
- `busy_workers` ← `cluster.busy_workers`
- `worker_slots` ← `cluster.target_workers - home_target_workers`
- `totals_source` = `vowp_cluster`, `cluster_totals_present` = `true`

When the card is absent (VOWP 0.4 or a home with no cluster card), the core computes:

- `online_workers` = configured workers with `status=active` and `presence=online`
- `worker_slots` = sum of those online rows' `slots_max` (an offline row's last-known slots are shown only in its per-host row)
- `pooled_target_workers` = Home target plus online worker slots
- `busy_workers` = Home running runs plus online worker `slots_busy`
- `totals_source` = `computed`, `cluster_totals_present` = `false`

The absence is not hidden: `warnings` contains
`VOWP did not report pooled cluster totals; the pooled numbers are computed from Home
and the visible worker nodes`.

A worker that is offline, unregistered or missing from `offload status` gets an
explicit `agent_state` of `offline`, `missing` or `unknown`; its slots are not counted
as online capacity. A worker whose node name equals Home's node name produces a
warning because VOWP cannot store two node records with the same name.

### Privacy

`health` never returns:

- an absolute path or `~` path (paths in `last_error` and labels are scrubbed to
  `[path]`);
- an SSH target or username;
- a pairing code, surface token, API key or secret name;
- raw command stdout or stderr.

The test `health_maps_pooled_totals_and_degrades_when_cluster_is_absent` asserts the
serialized document contains none of these.

## 5. Promote a backup to Home

`ClusterService::promote(&PromoteRequest)` is the explicit failover action. It is
gated by the caller's confirmation token, `promote-backup-to-home`
(`PROMOTE_CONFIRMATION_TOKEN`). A missing or different token is refused before any
command.

The request also names:

- `target` — one configured worker that holds the backup and will become Home;
- `backup_dir` — absolute backup directory on that worker;
- `backup_at` — optional known timestamp; when absent the core reads the directory
  mtime with `stat -c %Y`;
- `new_home` — fresh home directory on the target; defaults to the target profile's
  `vowp_home`.

Before mutating anything, the core checks:

1. the confirmation token;
2. the target is a configured worker;
3. `backup_at` is no older than `backup.every` (a stale backup is refused);
4. the target answers `vowp --version`;
5. `stat -c %Y <backup_dir>` proves the restore source exists; when `backup_at` was
   omitted, its mtime is also the age checked for staleness.

Then, in order:

1. on the old Home: `vowp [--home <dir>] down --drain` — admits no new work and lets
   in-flight runs and gates finish;
2. on the target: `vowp backup --restore <backup_dir> --to-home <new_home>`;
3. on the target: `vowp --home <new_home> up`;
4. on the target: `vowp --home <new_home> --json fleet status` to read the restored
   `snapshot.home`.

The outcome is the new Home host id, node name, home directory, backup timestamp and
promotion timestamp. Secrets are never restored: VOWP excludes `secrets/` from every
backup, so the owner must re-enter server secret values with `vowp secret set` after a
promotion.

### Failure modes while promoting

| Failure | Behaviour |
|---|---|
| No/wrong confirmation token | Refused before any command. |
| Target is not a configured worker | Invalid input before any command. |
| Backup is older than `backup.every` | Refused before any command. |
| Target unreachable | Preflight fails; the old Home is not drained. |
| Old Home cannot be drained | Refused. Starting the target anyway would risk two live Homes and break the one-home/one-lease rule. |
| Restore or `vowp up` fails | The old Home is already drained. The operator must inspect the target, fix it and run the recovery path explicitly; StackSpirit does not auto-start the old Home. |
| Promotion mid-run | `--drain` waits for active runs and gate tasks to reach zero before the old supervisor exits. The drain timeout is 30 minutes; if it expires the promotion fails and the old Home remains the only Home. |
| `new_home` is the target's live home | VOWP refuses to restore over the home serving the command; choose a fresh directory. |

Promotion never rewrites the local host store or the active Home selection. The caller
must switch explicitly after reviewing the returned identity.

## 6. Desktop UI

The desktop half is the optional `SpiritBridge.cluster` namespace (`form`, `health`,
`promote`, `backupPolicy`, `removeWorker`) backed by the five Tauri commands in
`apps/spirit-desktop/src-tauri/src/commands/cluster.rs`. Meadow uses:

- `packages/spirit-ui/src/next/FormClusterDialog.tsx` — the Form a cluster flow;
- `packages/spirit-ui/src/next/ClusterSection.tsx` — the Server page cluster block;
- `packages/spirit-ui/src/next/PromoteBackupDialog.tsx` — the failover confirmation;
- `packages/spirit-ui/src/next/cluster.ts` — form validation, formatting and the
  confirmation phrase;
- `packages/spirit-ui/src/next/clusterStore.ts` — cluster state.

Cluster state is not part of `fleetStore`. `clusterStore` reads the bridge and host list
from `fleetStore` so the shell has one bridge instance, and keeps the saved cluster beside
the fleet snapshot: a failed health read drops the last pooled health instead of dimming
the connected fleet view. The saved record is the app-private `cluster.json` and holds no
path, backup destination or credential.

### Form a cluster

With fewer than two saved servers the Form a cluster entry point is not offered and the
single-server layout is unchanged. With two or more, the Server card `⋯` menu and the
Server page cluster block offer **Form a cluster**. The dialog collects one Home plus one
or more Worker/Backup servers, a VOWP node name and role per worker, the optional
per-worker project checkout, the Home project name and checkout, the backup interval and
retention, and the `vowp backup --to` destination, then requires an explicit confirmation
before the bridge is called. Client-side validation mirrors the core rules (one Home,
unique node names, absolute paths, no credentials in the backup target) and keeps typed
failures inline so a single worker can be fixed without retyping the form. Only the safe shape is saved; the
paths and destination exist only in the request that uses them. A successful form reports
the enrolled Home/workers, whether pooled compute was enabled, and the backup schedule,
then `clusterStore` re-reads `ClusterHealth`; the dialog never copies a pooled total from
its own placeholder fields.

Removing a worker changes only StackSpirit's saved cluster. It does not revoke the VOWP
agent credentials or remove Home's backup schedule.

### Pooled health

The sidebar Server card shows the `cluster` badge, pooled **Load** and
`busy / pooled target` workers only when `ClusterHealth.online_workers > 0`. If every
worker is offline, the card stays the single-server card and shows no pooled number. The
Server page cluster block lists Home and each configured worker with its honest
`agent_state`, VOWP node name, slot counts, mapped project count and sanitized last error,
plus any `warnings`, the saved backup policy, and the promote action on backup workers.
It never computes or caches a pooled total of its own.

Which numbers come from where:

- **Read from VOWP on Home:** the Home node name, Home target and running count, worker
  status/presence/slots/mapped projects/`last_seen_at`/sanitized error, node registration,
  and the additive `cluster` card's worker and slot totals when a Home reports one.
- **Computed when the card is absent:** the values and warning described in §4. The UI
  labels them `Pooled totals computed from Home and the visible worker nodes.` and
  `totals_source` stays `computed`; it does not present them as VOWP totals.
- **From the saved policy:** the backup interval and `keep`. `freshness ≤ every` is the
  promotion bound, not an observed backup time.
- **From a completed promotion:** the returned `backup_at` for that one promotion, shown
  in the outcome and the last-promotion line. `ClusterHealth` itself has no backup
  timestamp.

### Promote backup to Home

**Promote backup to Home** opens a confirmation dialog: choose the backup worker, the
absolute backup directory, an optional fresh VOWP home, and type the exact phrase
`promote-backup-to-home`. The host re-checks the phrase and all of §5's preconditions
before anything is drained. The dialog warns that Home must answer the drain; a refusal
keeps the saved cluster untouched. Success names the restored Home and node, the backup
timestamp and the promotion time, offers **Connect to** the new Home, and states that the
active server was not switched and that VOWP backups exclude secrets.

### Preview mode

Preview has no host and no SSH connection, so `MockBridge` returns typed
`preview_unavailable` for every `cluster` action (`form`, `health`, `promote`,
`backupPolicy`, `removeWorker`), with the message that clusters need the desktop host and
the hint to run the app without `VITE_SPIRIT_MOCK`. The cluster store skips its health read
when `bridge.preview` is true, so preview never renders a cluster section or a pooled
total. The Form a cluster entry point can still open when the preview scenario has two or
more saved servers, but submitting shows the typed error inline via `ClusterErrorAlert`;
no success state, worker row or pooled number is invented.

## 7. Limits / not yet

- **No last-backup timestamp.** `ClusterHealth` has `checked_at` (when health was read) and
  per-host `last_seen_at`, but no backup time. The Server page shows the policy bound
  (`freshness ≤ every`) and the promotion dialog repeats it. An observed last-backup time
  would need a new VOWP read; the UI does not invent one.
- **Sidebar Load is cluster utilization.** With an online worker, the sidebar card's
  **Load** is `busy_workers / pooled_target_workers`, not the single-host
  higher-of-RAM-and-CPU figure. The single-server card keeps the per-host figure when no
  pooled cluster is shown.
- **Promotion needs a reachable old Home.** `promote` runs `vowp down --drain` on Home
  before restoring. If Home cannot be drained, it refuses rather than risk two live Homes
  (§5), so the action cannot currently be used once Home is completely unreachable. This
  is an `ss-54` product gap, not a UI omission.
- **VOWP `0.5` cluster totals are not installed.** The core and UI support the additive
  `cluster` card when a Home reports it, but the installed VOWP (`0.4.0`) does not. The
  pooled numbers are computed from Home and the visible online workers, labelled
  `computed`, with a warning; no `0.5` field is claimed as available.
- **Coverage is component and bridge tests, not a real cluster.** In `VITE_SPIRIT_MOCK=1`
  every cluster action is `preview_unavailable`, so mock e2e can only prove the honest
  gate. A real cluster test needs the desktop host and two servers.

## 8. Tests

`crates/spirit-core/tests/cluster.rs` uses `FakeCommandRunner`, `InMemoryHostStore` and
`FakeClock`. It never opens a socket or touches a real home.

| Test | Evidence |
|---|---|
| `form_with_two_servers_writes_the_expected_commands_and_config` | exact argv, code only on stdin, `[offload]` edit preserves unrelated keys, schedule registration |
| `form_refuses_fewer_than_two_saved_servers_before_any_command` | no command on refusal |
| `form_refuses_home_as_worker_and_duplicate_node_names` | no command on invalid topology |
| `form_refuses_a_worker_whose_ssh_login_fails_before_any_mutation` | `ssh_auth`, no `node enroll` |
| `form_never_puts_the_pairing_code_in_an_error_or_debug_output` | code is stdin-only; error/Debug clean |
| `health_maps_pooled_totals_and_degrades_when_cluster_is_absent` | computed totals, warning, no paths/tokens |
| `health_prefers_vowp_cluster_totals_when_present` | `vowp_cluster` totals |
| `health_marks_an_offline_worker_without_counting_it_as_online` | offline worker is excluded |
| `health_sanitizes_unknown_enum_values_instead_of_leaking_them` | unknown VOWP values become `unknown`; no raw path/token |
| `promote_requires_the_explicit_confirmation_token_before_any_command` | refused, zero commands |
| `promote_refuses_a_stale_backup_before_any_command` | refused, zero commands |
| `promote_refuses_an_unreachable_target_without_draining_home` | target preflight only |
| `promote_reads_the_remote_backup_mtime_when_no_timestamp_is_supplied` | `stat -c %Y` before any mutation |
| `promote_drains_home_restores_the_worker_and_returns_the_new_identity` | exact promote sequence and identity |

The desktop half has component tests in
`packages/spirit-ui/src/test/next/cluster.test.tsx` (form validation, pooled numbers,
offline worker, promotion confirmation) and bridge tests in
`apps/spirit-desktop/src/bridge/clusterBridge.test.ts` (the five command names and argument
shapes). Both use a fake or mock bridge; neither forms a real cluster.

Run:

```sh
cargo test -p spirit-core --test cluster
cargo test -p spirit-core
cargo clippy -p spirit-core --all-targets -- -D warnings
cargo fmt --all -- --check
```
