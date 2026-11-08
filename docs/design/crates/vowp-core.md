# crate `vowp-core`

Orchestration library: universal sessions (capsule, switch), the supervisor loops L1–L8 and merge queue, gates, ledger/cost, schedules (L9), promotion.
Depends on `vowp-protocol`, `vowp-store`, `vowp-graph`, `vowp-tmux`, `vowp-adapters`. Behaviour is specified in `loops.md`, `sessions.md`, `adapters.md`; this file is
**module layout, ownership, public API and tests**. Dependencies: see `conventions.md` §3. No `unsafe`, no tokio.

## 1. Modules and ownership

n01 creates `src/lib.rs` declaring `session, loops, gates, ledger, schedules, promote` (each `src/<m>/mod.rs`). Those stubs are not enough: modules share a few
foundation items, so a small first job **n07a core-foundation** (before n07/n08/n10) owns `src/lib.rs`, `src/error.rs`, `src/deps.rs`, `src/requests.rs`, and may edit each `src/<m>/mod.rs`
only to add the `pub mod` lines listed below as empty files with a `//!` comment. After n07a the files `lib.rs`, `error.rs`, `deps.rs`, `requests.rs` are frozen.

```
crates/vowp-core/src/
  lib.rs          n07a  pub mod error, deps, requests, session, loops, gates, ledger, schedules, promote; re-exports
  error.rs        n07a  CoreError
  deps.rs         n07a  Deps, CommandRunner, SystemRunner, GitOps, SystemGit, AdapterRegistry, Rng
  requests.rs     n07a  Request enum + (de)serialisation for CLI→supervisor kv requests
  session/        n07   mod.rs service.rs capsule.rs seed.rs switch.rs recovery.rs bundle.rs importer.rs(optional)
  gates/          n08a  mod.rs runner.rs report.rs cmd.rs owns.rs pushed.rs review.rs deploy.rs secret_scan.rs
  ledger/         n08b  mod.rs record.rs price.rs board.rs report.rs
  schedules/      n08b  mod.rs spec.rs engine.rs
  loops/          n08c  mod.rs supervisor.rs reconcile.rs l2_worker.rs l3_verify.rs merge_queue.rs dispatch.rs runner.rs   (L1, L2, L3, merge queue, dispatch)
                  n08d  l4_planner.rs l5_keeper.rs l6_failover.rs l7_inbox.rs l8_watchdog.rs digest.rs prompts.rs  (+ the `pub mod` lines for them in loops/mod.rs; n08d needs n08c)
  promote/        n10   mod.rs guards.rs bank.rs promoter.rs releases_log.rs
tests/            each job owns tests named after its module: session_*.rs (n07), gates_*.rs (n08a), ledger_*.rs schedules_*.rs (n08b), loops_*.rs drills_*.rs (n08c/n08d), promote_*.rs (n10)
tests/fixtures/   shared recorded fixtures are copied from vowp-adapters fixtures by each job that needs them (no cross-crate path reads)
```

## 2. Foundation (n07a): `error`, `deps`, `requests`

```rust
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)] Store(#[from] vowp_store::StoreError),
    #[error(transparent)] Graph(#[from] vowp_graph::GraphError),
    #[error(transparent)] Protocol(#[from] vowp_protocol::ProtocolError),
    #[error(transparent)] Adapter(#[from] vowp_protocol::AdapterError),
    #[error(transparent)] Tmux(#[from] vowp_tmux::TmuxError),
    #[error("io {path}: {source}")]            Io { path: String, source: std::io::Error },
    #[error("git {args:?}: exit {code:?}: {stderr}")] Git { args: Vec<String>, code: Option<i32>, stderr: String },
    #[error("command failed: {0}")]            Command(String),
    #[error("capsule invalid after repair: {0}")] CapsuleInvalid(String),
    #[error("switch aborted: {0}")]            SwitchAborted(String),
    #[error("provider limited: {0:?}")]        Limited(vowp_protocol::LimitEvent),
    #[error("refused: {0}")]                   Refused(String),   // cwd allow-list, guard failures that must not be worked around
    #[error("guard failed: {name}: {detail}")] Guard { name: String, detail: String },
    #[error("not found: {0}")]                 NotFound(String),
    #[error("invalid: {0}")]                   Invalid(String),
    #[error("cancelled")]                      Cancelled,
}
impl CoreError { pub fn kind(&self) -> vowp_protocol::ErrorKind; }
impl From<CoreError> for vowp_protocol::ServiceError {}   // Limited→ServiceError::Limited, Cancelled→Cancelled, NotFound→NotFound, rest→Failed(msg)

/// Everything a loop or service needs from the outside world, injected so tests use fakes. Cheap to clone (Arcs).
#[derive(Clone)]
pub struct Deps {
    pub paths: vowp_store::NodePaths,
    pub config: std::sync::Arc<vowp_protocol::VowpConfig>,
    pub node: vowp_protocol::NodeName,
    pub clock: std::sync::Arc<dyn vowp_protocol::Clock>,
    pub log: std::sync::Arc<dyn vowp_protocol::Log>,
    pub adapters: std::sync::Arc<AdapterRegistry>,
    pub windows: std::sync::Arc<dyn vowp_tmux::WindowManager>,
    pub git: std::sync::Arc<dyn GitOps>,
    pub cmd: std::sync::Arc<dyn CommandRunner>,
    pub rng: std::sync::Arc<dyn Rng>,
}
impl Deps { pub fn open_store(&self) -> Result<vowp_store::Store, CoreError>; }   // each thread opens its own connection

pub trait Rng: Send + Sync { fn jitter_secs(&self, max: u64) -> u64; }          // SystemRng (time-seeded xorshift) / FixedRng(n) in tests

/// Maps HarnessKind (+ `[harnesses.*]` config) to a ready adapter. Fakes: `AdapterRegistry::with(kind, Arc<dyn Adapter>)`.
pub struct AdapterRegistry { /* BTreeMap<String, Arc<dyn Adapter>> keyed by "claude-code" | "opencode" | "cli:<name>" */ }
impl AdapterRegistry {
    pub fn new() -> Self;
    pub fn from_config(cfg: &vowp_protocol::VowpConfig) -> Result<Self, CoreError>;   // builds ClaudeCodeAdapter/OpencodeAdapter/CliAdapter from [harnesses.*], tz_offset_minutes
    pub fn with(self, key: &str, adapter: std::sync::Arc<dyn vowp_protocol::Adapter>) -> Self;
    pub fn get(&self, exec: &vowp_protocol::Executor) -> Option<std::sync::Arc<dyn vowp_protocol::Adapter>>;
}

#[derive(Debug, Clone)]
pub struct CommandSpec { pub shell: Option<String> /* run via sh -c */, pub argv: Vec<String> /* used when shell is None */,
                         pub cwd: Option<std::path::PathBuf>, pub env: Vec<(String, String)>, pub timeout: std::time::Duration }
#[derive(Debug, Clone)]
pub struct CommandOutput { pub exit_code: Option<i32>, pub timed_out: bool, pub output: Vec<u8> /* stdout+stderr interleaved, capped at 4 MiB */,
                           pub duration: std::time::Duration }
impl CommandOutput { pub fn tail_lines(&self, n: usize) -> String; pub fn text(&self) -> String }
pub trait CommandRunner: Send + Sync { fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, CoreError>; }
pub struct SystemRunner;   // std::process::Command, process group, kills group on timeout (adapters::proc::signal_group is reused via vowp_adapters::proc)

pub trait GitOps: Send + Sync {
    fn worktree_add(&self, repo: &Path, path: &Path, branch: &str, base: &str) -> Result<(), CoreError>;   // existing worktree/branch reused (idempotent)
    fn worktree_remove(&self, repo: &Path, path: &Path) -> Result<(), CoreError>;
    fn branch_delete(&self, repo: &Path, branch: &str) -> Result<(), CoreError>;
    fn fetch(&self, repo: &Path, remote: &str) -> Result<(), CoreError>;
    fn diff_names(&self, repo: &Path, base: &str, head: &str) -> Result<Vec<String>, CoreError>;  // `git diff --name-only base...head`
    fn diff_text(&self, repo: &Path, base: &str, head: &str) -> Result<String, CoreError>;
    fn commits_between(&self, repo: &Path, base: &str, head: &str) -> Result<Vec<String>, CoreError>;   // short shas
    fn head_sha(&self, repo: &Path, rev: &str) -> Result<String, CoreError>;
    fn ls_remote(&self, repo: &Path, remote: &str, branch: &str) -> Result<String, CoreError>;      // raw output
    fn status_clean(&self, repo: &Path) -> Result<bool, CoreError>;
    fn rebase(&self, worktree: &Path, onto: &str) -> Result<RebaseResult, CoreError>;
    fn rebase_abort(&self, worktree: &Path) -> Result<(), CoreError>;
    fn merge_ff_only(&self, repo: &Path, branch: &str) -> Result<(), CoreError>;
    fn push(&self, repo: &Path, remote: &str, branch: &str) -> Result<(), CoreError>;
    fn last_commit_time(&self, repo: &Path, rev: &str) -> Result<Option<Timestamp>, CoreError>;     // for stall "no new commits"
}
pub enum RebaseResult { Clean, Conflict { files: Vec<String> } }
pub struct SystemGit { /* CommandRunner */ }   // runs real `git`; tests use real git in temp repos (bare origin + clone)
```

### `requests.rs` — CLI→supervisor requests (decision D2)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    RetryJob { job: JobId, reset_attempts: bool }, ParkJob { job: JobId, reason: String }, CancelJob { job: JobId },
    NudgeJob { job: JobId, text: Option<String> }, RestartJob { job: JobId }, ReplanJob { job: JobId }, FailJob { job: JobId, reason: String },   // keeper commands (loops.md §7)
    AnswerInbox { id: InboxId }, PauseLoop { name: String }, ResumeLoop { name: String },
    SwitchSession { request: SwitchRequest },        // not used by v0.1 CLI (switch runs inline); reserved
    Wake,
}
pub fn push(store: &Store, req: &Request) -> Result<RequestId, CoreError>;      // thin wrapper over Store::push_request
pub fn drain(store: &Store) -> Result<Vec<(RequestId, Request)>, CoreError>;     // take_requests + decode; undecodable rows are logged & dropped
pub fn apply_direct(deps: &Deps, store: &Store, req: &Request) -> Result<(), CoreError>;   // used by the CLI when no supervisor runs
```

## 3. `session` (n07) — spec `sessions.md`

```rust
pub struct SessionService { /* Deps, session-level locks map */ }
impl SessionService {
    pub fn new(deps: Deps) -> Self;
    pub fn recover(&self, store: &Store) -> Result<RecoveryReport, CoreError>;      // startup: resolve `switches` rows left in 'started', close dangling runs (sessions.md §5.8)
}
impl vowp_protocol::SessionOps for SessionService { /* create_session, run_turn, switch, write_capsule */ }
```

`run_turn` algorithm (numbered, normative):
1. Check `cwd` against the allow-list (`Refused` otherwise). Resolve adapter via `AdapterRegistry::get`; missing → `ServiceError::Failed("harness not installed")`.
2. Open the session's `SessionLog` (lockfile; a second concurrent writer → `Conflict`). Append `turn.started` (turn = last turn + 1) and, first time on this executor, `session.executor` (attach) with `native_session_id` from `session_natives` if resuming the same harness; if the session has no native session on this harness and a capsule exists, the prompt is `seed_prompt(capsule, tail_turns, prompt)`; else `prompt`.
3. Append the `user.message`; insert the `runs` row (`started_at=now`, `purpose`, `profile`, `rung`, `window`); `Adapter::start(StartInput{ resume, prompt, cwd, env: configured env_pass values, … })`.
4. Loop on `run.events()` with `recv_timeout(200 ms)`: append each event through `SessionLog::append` (blob spill >8192 B), update `sessions.last_seq`/`updated_at` and `session_events` index every 20 events and at the end; poll `cancel` → `interrupt()`.
5. `wait()` → `RunSummary`; append `turn.ended` (stop_reason from summary), `cost` event (if cost known) and `limit` event (if any), `session.executor` detach; `sync()`.
6. Finish the run row (`finish_run`), upsert/clear `limits`, write the ledger rows (`ledger::record_run_cost`, price-table fallback when no receipt), return `TurnResult`. A `Limit/Credits/Auth/Context/Rate` outcome is **returned**, not raised: callers (loops) decide.

```rust
pub mod capsule {
    pub struct CapsuleWriter { /* Deps */ }
    impl CapsuleWriter {
        pub fn new(deps: Deps) -> Self;
        /// Compacts input, runs the capsule executor, validates, repairs once, stores file + row + `capsule` event. sessions.md §3.
        pub fn write(&self, session: &SessionId, trigger: CapsuleTrigger) -> Result<Capsule, CoreError>;
        pub fn build_prompt(&self, last: Option<&Capsule>, events: &[EventEnvelope], from: &ExecutorRef) -> String;   // pure, golden-tested
        pub fn parse_draft(&self, text: &str) -> Result<CapsuleDraft, CoreError>;    // tolerates ``` fences and leading prose; first JSON object
    }
}
pub mod seed { pub fn seed_prompt(capsule: &Capsule, tail: &[EventEnvelope], tail_turns: u32, next_input: &str) -> String; }   // sessions.md §4
pub mod switch { pub fn run_switch(deps: &Deps, req: &SwitchRequest, cancel: &CancelToken) -> Result<Switch, CoreError>; }        // sessions.md §5
pub mod bundle {
    pub const BUNDLE_FORMAT: &str = "vowp-bundle-v1";
    pub struct Bundle { /* fields per sessions.md §7 */ }
    pub fn export(deps: &Deps, session: &SessionId, tail: usize) -> Result<Bundle, CoreError>;
    pub fn to_json(b: &Bundle) -> Result<String, CoreError>;  pub fn from_json(s: &str) -> Result<Bundle, CoreError>;  // verifies checksum + format
    pub fn import(deps: &Deps, b: &Bundle, executor: Option<ExecutorRef>) -> Result<SessionId, CoreError>;                // child session
}
pub mod importer { /* optional (nice-to-have): ClaudeTranscriptImporter::scan_once(dir) -> ImportReport, read-only, incremental by byte offset */ }
```

Tests (hermetic: temp `NodePaths`, `FakeClock`, `AdapterRegistry::with("opencode", FakeAdapter)` where `FakeAdapter` replays a fixture through the real `Adapter` trait — defined in `crates/vowp-core/tests/support/fake_adapter.rs`, built by n07a, copied by later jobs):
`run_turn_appends_events_and_finishes_run`, `run_turn_resumes_native_session_same_harness`, `run_turn_seeds_capsule_on_new_harness`, `run_turn_cwd_outside_allow_list_refused`, `run_turn_cancel_interrupts_adapter`,
`run_turn_limit_outcome_returned_not_raised_and_limits_row_written`, `run_turn_large_tool_output_spilled_to_blob`, `second_writer_on_same_session_conflicts`,
`capsule_prompt_golden`, `capsule_parse_draft_with_fences_and_prose`, `capsule_write_valid_first_try`, `capsule_write_repairs_once_then_ok`, `capsule_write_twice_invalid_keeps_previous_and_logs_error_event`,
`capsule_debounce_120s_except_manual_switch_limit_export`, `seed_prompt_golden_with_two_turn_tail`, `seed_prompt_never_contains_tool_calls_as_native`,
`switch_happy_path_events_rows_and_lineage`, `switch_target_probe_fails_stays_on_source`, `switch_drain_timeout_interrupts_after_5s_fakeclock`, `switch_seed_rejected_retries_capsule_only_then_aborts`,
`recover_started_switch_never_attached_is_aborted`, `recover_started_switch_attached_is_completed`, `bundle_export_import_roundtrip_child_session`, `bundle_checksum_mismatch_rejected`, `bundle_unknown_format_rejected`, `bundle_inlines_small_blobs_only`,
`failover_roundtrip_claude_to_opencode_and_back_preserves_capsule_content` (end-to-end with two fake adapters).

## 4. `gates` (n08a) — spec `loops.md` §4

```rust
pub struct GateCtx<'a> { pub deps: &'a Deps, pub job: &'a Job, pub run_id: Option<RunId>, pub worktree: &'a Path, pub repo: &'a Path,
                         pub store: &'a Store, pub attempt: u32 }
pub struct GateRunner { /* Deps */ }
impl GateRunner {
    pub fn new(deps: Deps) -> Self;
    pub fn evaluate(&self, ctx: &GateCtx, index: i32, gate: &Gate) -> Result<GateResult, CoreError>;   // never panics on bad input: failures are `passed=false` + evidence
    /// All gates of the job in order, stopping at the first failure except `review` (evaluated separately by L3). Persists each result (`insert_gate_result`).
    pub fn evaluate_all(&self, ctx: &GateCtx) -> Result<GatesOutcome, CoreError>;
    /// Repo-wide check for the merge queue (index -1): project check_cmds + secret_scan on the rebased tree.
    pub fn evaluate_merge_checks(&self, ctx: &GateCtx) -> Result<Vec<GateResult>, CoreError>;
}
pub struct GatesOutcome { pub results: Vec<GateResult>, pub all_passed_before_review: bool, pub first_failure: Option<usize>, pub needs_review: bool }
pub mod secret_scan {
    pub struct Rule { pub name: &'static str, pub regex: &'static str }
    pub fn rules() -> &'static [Rule];                                   // names: aws_access_key, github_pat, github_token, private_key_block, slack_token, openai_style_key, supabase_secret
    pub fn scan_text(path: &str, text: &str) -> Vec<SecretHit>;           // hits carry rule NAME + line only
    pub fn scan_diff(diff: &str) -> Vec<SecretHit>;                       // only added lines (`+`), path from `+++ b/…`
}
pub fn parse_report_file(path: &Path) -> Result<Report, CoreError>;      // thin wrapper, for gates + loops
```
Tests: `report_gate_passes_with_required_fields`, `report_gate_fails_missing_field_lists_names`, `report_gate_missing_file_fails_with_evidence`, `report_gate_status_partial_fails_done_requirement` (a `report` gate requires `status: done` for `done`; `partial|blocked` never advance a job), `cmd_gate_exit_zero_passes`, `cmd_gate_wrong_exit_fails_with_tail`, `cmd_gate_expect_pattern`, `cmd_gate_timeout_kills_group_and_fails` (sleep 30 with timeout 1 s), `cmd_gate_tail_is_last_200_lines`,
`owns_respected_pass_and_fail_with_real_git`, `owns_respected_allows_always_allowed_cargo_lock`, `pushed_gate_against_local_bare_remote`, `pushed_gate_fails_when_branch_absent`,
`review_gate_reads_verifier_report_verdict`, `review_gate_pending_when_no_verify_job_yet`, `secret_scan_detects_each_rule_and_never_stores_value`, `secret_scan_only_added_lines`, `deploy_gate_runs_command_then_probe`, `deploy_gate_probe_mismatch_fails`,
`file_exists_only_gate_rejected_by_validate` (anti-pattern ban), `evaluate_all_stops_at_first_failure_and_persists_results`, `evaluate_merge_checks_uses_project_check_cmds`.

## 5. `ledger` (n08b)

```rust
pub struct Ledger { /* Deps */ }
impl Ledger {
    pub fn new(deps: Deps) -> Self;
    pub fn record(&self, store: &Store, entry: LedgerEntry) -> Result<i64, CoreError>;
    pub fn record_dispatch(&self, store: &Store, job: &Job, run: &Run) -> Result<(), CoreError>;
    /// One `cost` row per run: receipt cost from the summary, else `price::estimate`, else `unknown`. For claude-code runs
    /// `detail_json = {"billed":false,"basis":"list"}` (subscription; adapters.md §6).
    pub fn record_run_cost(&self, store: &Store, run: &Run, summary: &RunSummary) -> Result<Cost, CoreError>;
    pub fn record_gate(&self, store: &Store, r: &GateResult) -> Result<(), CoreError>;
    pub fn record_state(&self, store: &Store, job: &JobId, from: JobState, to: JobState, why: &str) -> Result<(), CoreError>;
}
pub mod price {
    pub fn estimate(cfg: &VowpConfig, exec: &Executor, tokens: &Tokens) -> Cost;   // PriceTable if `[providers.p.prices."model"]` present (model key = text after '/' for opencode), else Cost::unknown()
}
pub struct BoardSnapshot { pub project: ProjectName, pub at: Timestamp, pub jobs_by_state: BTreeMap<JobState, u32>, pub running: Vec<BoardRun>,
    pub slots_busy: u32, pub slots_total: u32, pub planner_executor: Option<Executor>, pub keeper_executor: Option<Executor>, pub limits: Vec<Limit>,
    pub cost_today: Cost, pub inbox_open: u32, pub last_checkpoint: Option<Timestamp> }
pub struct BoardRun { pub job: JobId, pub executor: Option<Executor>, pub age_secs: u64, pub last_event_age_secs: Option<u64> }
impl BoardSnapshot { pub fn collect(store: &Store, cfg: &VowpConfig, now: Timestamp) -> Result<Self, CoreError>; pub fn render_text(&self, color: bool) -> String; }
pub mod report {
    pub fn night_report(store: &Store, cfg: &VowpConfig, now: Timestamp) -> Result<String, CoreError>;   // Markdown: built, failed+why, cost by receipts, limit/failover counts, switches, inbox
    pub fn checkpoint(store: &Store, cfg: &VowpConfig, now: Timestamp) -> Result<String, CoreError>;      // short; written to .vowp/checkpoints/<ts>.md by L4
}
```
Tests: `record_run_cost_receipt_vs_price_table_vs_unknown`, `price_estimate_math_per_million`, `claude_cost_marked_not_billed`, `ledger_rows_are_append_only_via_store_trigger`, `board_snapshot_counts_and_render_golden`, `night_report_lists_limits_and_switches_golden`, `cost_totals_never_count_unknown_as_zero`.

## 6. `schedules` (n08b) — spec `loops.md` §11

```rust
pub struct ScheduleSpec { /* parsed Cron(cron::Schedule + offset) | Interval(Duration) */ }
impl ScheduleSpec {
    pub fn parse(kind: ScheduleSpecKind, spec: &str, tz: &str) -> Result<Self, CoreError>;     // cron: 5 fields -> prefix "0 " seconds field for the `cron` crate
    pub fn next_after(&self, after: Timestamp) -> Option<Timestamp>;
}
pub struct ScheduleEngine { /* Deps */ }
impl ScheduleEngine {
    pub fn new(deps: Deps) -> Self;
    pub fn sync_from_config(&self, store: &Store, cfg: &VowpConfig) -> Result<(), CoreError>;       // upsert [[schedules]] rows by name; compute next_run_at
    pub fn catch_up(&self, store: &Store, now: Timestamp) -> Result<Vec<DueRun>, CoreError>;        // startup: missed runs per missed_policy (run_once → one DueRun, skip → none) and re-arm
    pub fn tick(&self, store: &Store, now: Timestamp) -> Result<Vec<DueRun>, CoreError>;            // due rows; advances next_run_at BEFORE firing (idempotency key = schedule id + due time)
    pub fn fire(&self, store: &Store, due: &DueRun) -> Result<FireOutcome, CoreError>;               // JobTemplate -> insert job (suffix -yyyymmddhhmm); Loop -> push Request::Wake + loops.next_run_at=now; Command -> CommandRunner
}
pub struct DueRun { pub schedule: Schedule, pub due_at: Timestamp }
pub enum FireOutcome { JobCreated(JobId), LoopTriggered(String), CommandRan { exit_code: Option<i32> }, Skipped(String) }
```
Tests (all with `FakeClock`): `cron_five_field_next_runs_golden`, `cron_weekday_window`, `interval_every_30m`, `fixed_offset_tz_shifts_next_run`, `bad_cron_rejected`, `tick_fires_due_and_advances_next_before_fire`, `double_tick_same_instant_fires_once`, `catch_up_run_once_after_three_missed_fires_once`, `catch_up_skip_fires_none_but_rearms`, `job_template_creates_job_with_suffix`, `command_action_runs_with_timeout`, `paused_schedule_never_fires`, `proptest_next_after_is_strictly_greater_and_matches_cron`.

## 7. `loops` (n08c, n08d) — spec `loops.md`

```rust
pub struct Supervisor { /* Deps, project, Store, RunnerPool, GateRunner, Ledger, ScheduleEngine, SessionService, state */ }
impl Supervisor {
    pub fn new(deps: Deps, project: ProjectName) -> Result<Self, CoreError>;
    /// Startup reconciliation (loops.md §1 step 1): orphaned runs → crash, dead windows, `switches` recovery, schedule catch-up, loop rows created from defaults+config.
    pub fn startup(&mut self) -> Result<ReconcileReport, CoreError>;
    /// One full L1 tick, ordered steps 0–13 of loops.md §1. A failing step is logged and counted; the tick continues.
    pub fn tick(&mut self) -> Result<TickReport, CoreError>;
    /// startup() then tick() every `tick_secs` or on wake, until `cancel`; on exit interrupts runner threads and marks runs `stalled`.
    pub fn run(&mut self, cancel: &CancelToken) -> Result<(), CoreError>;
    pub fn wake_handle(&self) -> WakeHandle;     // Sender<()> clone for runner threads
}
pub struct TickReport { pub at: Timestamp, pub steps: Vec<StepReport>, pub dispatched: Vec<JobId>, pub transitions: Vec<(JobId, JobState, JobState)>, pub merged: Vec<JobId> }
pub struct StepReport { pub name: &'static str, pub ok: bool, pub detail: String }
pub struct ReconcileReport { pub orphaned_runs: u32, pub switches_recovered: u32, pub schedules_caught_up: u32, pub windows_recreated: u32 }

/// Runner threads: one per active run; each opens its own Store; results are written to the DB then `Wake`.
pub struct RunnerPool { /* JoinHandles + CancelTokens by RunId */ }
impl RunnerPool { pub fn spawn(&mut self, job: Job, choice: vowp_graph::DispatchChoice, deps: Deps, wake: WakeHandle) -> Result<RunId, CoreError>; pub fn cancel(&self, run: &RunId); pub fn cancel_all(&self); pub fn active(&self) -> usize; }

pub mod l2 { pub fn start_prompt(job: &Job, brief: &str) -> String; pub fn continue_prompt(job: &Job) -> String; pub fn fix_prompt(job: &Job, findings: &[String]) -> String; }   // text templates: loops.md §3 (golden-tested)
pub mod l3 { pub fn verify_prompt(job: &Job, brief: &str, evidence: &[GateResult], diff_summary: &str) -> String; pub fn parse_verdict(report: &Report) -> Result<(Verdict, Vec<String>), CoreError>; }
pub mod merge_queue { pub struct MergeQueue { /* Deps, GateRunner */ } impl MergeQueue { pub fn step(&self, store: &Store) -> Result<MergeStep, CoreError>; } pub enum MergeStep { Idle, Merged(JobId), Conflict { job: JobId, files: Vec<String> }, ChecksFailed { job: JobId }, InProgress } }
pub mod l6 {   // failover (n08d)
    pub struct Failover { /* Deps */ }
    impl Failover {
        pub fn on_run_outcome(&self, store: &Store, run: &Run, summary: &RunSummary) -> Result<FailoverAction, CoreError>;   // writes `limits`, inbox, ledger; pure decision otherwise
        pub fn probe_due(&self, store: &Store, now: Timestamp) -> Result<Vec<String /*provider*/>, CoreError>;
        pub fn run_probe(&self, store: &Store, provider: &str) -> Result<ProbeVerdict, CoreError>;                              // one tiny turn via the adapter
    }
    pub enum FailoverAction { None, Backoff { until: Timestamp }, NextRung { rung: usize }, ParkProvider { provider: String }, ParkJob { reason: ParkedReason }, FreshSessionWithCapsule, ProbeBack { provider: String } }
}
pub mod l7 { pub fn open_inbox(store: &Store, deps: &Deps, item: NewInboxItem) -> Result<InboxId, CoreError>; pub fn on_answer(store: &Store, id: &InboxId) -> Result<(), CoreError>; }   // writes inbox/<id>.md copy too
pub mod l8 { pub fn write_heartbeat(deps: &Deps) -> Result<(), CoreError>; pub fn heartbeat_age(paths: &NodePaths, now: Timestamp) -> Option<std::time::Duration>; pub const SYSTEMD_UNIT: &str; pub const CRON_CHECK_SH: &str; }  // text templates installed by n13
pub mod digest { pub fn planner_digest(store: &Store, since: Timestamp, max_chars: usize) -> Result<String, CoreError>; pub fn keeper_digest(…) -> Result<String, CoreError>; }
```
Named tests live in `loops.md` §13 (drills) plus: `tick_step_failure_does_not_abort_tick`, `startup_marks_orphaned_runs_crash_and_continues_job`, `dispatch_respects_ownership_locks_and_slots`, `dispatch_skips_limited_provider_and_uses_next_rung`, `stall_judge_triggers_stop_and_continue`, `hang_before_first_output_restart_up_to_3`, `continue_twice_then_keeper`, `verify_fail_creates_fix_job_same_branch_max_3_then_escalates`, `merge_queue_happy_path_ff_push_to_local_bare_origin`, `merge_queue_conflict_creates_merge_job`, `merge_queue_cargo_lock_regenerated_not_merged`, `merge_queue_check_failure_returns_job_to_fixing`, `failover_drill_*`, `credits_park_all_jobs_of_provider_and_inbox`, `rate_backoff_lowers_concurrency_cap`, `auth_parks_with_inbox`, `three_unknown_failures_mark_unavailable_15min`, `inbox_answer_moves_parked_job_to_ready`, `planner_one_turn_at_a_time`, `keeper_covers_planner_when_provider_limited_and_hands_back`, `repeat_guard_trips_on_third_identical_action`, `board_and_heartbeat_written_each_tick`, `requests_drained_and_applied`, `paused_loop_is_skipped`.

## 8. `promote` (n10) — nice-to-have

```rust
pub struct Promoter { /* Deps, GateRunner */ }
impl Promoter {
    pub fn new(deps: Deps) -> Self;
    /// plan §10: guards → serving bank → diff candidate vs serving → approval → deploy → probe → record. Returns the release in its final state for this call
    /// (`Approved`→`Live`, or stopped at `Guarded`/`Banked` awaiting an inbox approval, or `Refused`/`Failed`). Re-entrant: called again after the approval is answered.
    pub fn promote(&self, store: &Store, project: &ProjectName, env: &str, change: &str, opts: PromoteOpts) -> Result<Release, CoreError>;
    pub fn rollback(&self, store: &Store, release: &ReleaseId, opts: PromoteOpts) -> Result<Release, CoreError>;      // runs the banked rollback_cmd
    pub fn releases(&self, store: &Store, project: &ProjectName, env: Option<&str>, limit: u32) -> Result<Vec<Release>, CoreError>;
}
pub struct PromoteOpts { pub dry_run: bool, pub yes: bool /* only honoured when env.approval == policy */, pub approved_by: Option<String> }
pub mod guards {
    pub const BUILTIN: &[&str] = &["clean_tree", "head_equals_remote", "build", "no_active_jobs", "upload_set"];
    pub fn run_guard(deps: &Deps, env: &EnvConfig, name: &str, checkout: &Path) -> Result<GuardResult, CoreError>;   // names with ":cmd" suffix run a custom script: "no_active_jobs:./scripts/guard.sh"
}
pub mod bank { pub fn bank_serving(deps: &Deps, env: &EnvConfig, target: &str, to: &Path) -> Result<BankedServing, CoreError>; pub struct BankedServing { pub path: PathBuf, pub commit: Option<String>, pub rollback_cmd: String } }
```
Rules: (1) a failing guard ⇒ `Release.state = Refused`, ledger `promote`, exit 5 at the CLI; **no retry, no workaround** (plan §10, §16.5). (2) the diff between candidate and the banked serving copy must equal the files of the intended change; otherwise `Refused` ("live is not always git"). (3) the `upload_set` guard runs the project's upload-set check (e.g. `.vercelignore` listing) via `env.probe`-style custom guard: it exists to catch "a local build is not the deploy build". (4) approval `human` ⇒ inbox `approval` item, state `Banked` until answered; the supervisor calls `promote` again on answer. (5) `releases.log` line appended on every terminal state: `<ts> <env> <release id> <state> <from>-><to> rollback: <cmd>`.
Tests (temp git repos; deploy/bank/probe are fake shell scripts writing to temp files): `guards_clean_tree_dirty_fails`, `guards_head_equals_remote_behind_fails`, `guard_failure_marks_refused_and_never_deploys`, `serving_bank_runs_before_deploy_order_asserted_via_script_log`, `diff_unexpected_files_refused`, `human_approval_creates_inbox_and_waits`, `answer_approve_then_promote_goes_live_and_records_rollback_cmd`, `policy_approval_with_yes_deploys`, `yes_flag_never_bypasses_human_approval`, `probe_mismatch_marks_failed_with_rollback_available`, `rollback_runs_banked_command_and_links_releases`, `releases_log_lines_appended`, `dry_run_runs_guards_only`.

## 9. Out of scope (v0.2)
HTTP+SSE daemon, ACP, cross-node live sync, IANA timezone schedules, notification hooks (ntfy), automatic price-table download.
