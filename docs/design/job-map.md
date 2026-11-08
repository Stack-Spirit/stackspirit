# job-map.md — the build plan for the planner

Decomposes the seed graph of `plan.md` §16.3 (n00 … n17) into concrete jobs of ~30–90 min each. Ids keep the plan's `nNN` prefix; the seeded umbrella jobs
`n02-protocol`, `n03-store`, `n04-graph`, `n07-capsule-switch`, `n08-supervisor`, `n09-cli`, `n10-promote`, `n12-docs`, `n13-install-ops` are **replaced by the split jobs below**
(`vowp job update` their dependents' `needs` to the last split job; `replanned` the umbrella).
Every job's brief is generated from its entry here plus the spec file/section it names; the brief must say "implement exactly the named API and named tests; do not invent architecture".

## 0. Rules for this map

- **R1 — owned paths are disjoint for jobs that can run in parallel.** Where several jobs share a crate, the *first* job writes `lib.rs` (declaring all modules) and creates every other module file as an
  empty file with only a `//! owned by <job>` doc line; later jobs only edit the files they own. Ownership may overlap only between jobs ordered by `needs` (e.g. n08d edits `loops/mod.rs` after n08c).
- **R2 — default gates** for every `build` job: `report`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test -p <crate>`; `owns_respected`; `secret_scan`; `review`.
  Jobs list only their crate-specific tests below. `Cargo.lock` is always allowed.
- **R3 — acceptance = the named tests** of the cited spec section, implemented under exactly those names, passing, plus the gates. A test is not allowed to be weakened to pass.
- **R4 — a job that finds the spec wrong or ambiguous** writes `questions_for_owner` and `status: partial|blocked`; it does not redesign.
- **R5 — fakes before reality:** jobs that need another crate's API before it is merged depend on it (`needs`); there are no cross-crate stubs except the explicitly allowed `vowp-cli` command stubs (`crates/vowp-cli.md` §1).
- Time estimates are for DeepSeek V4.1 Flash at max effort; verify/fix loops add ~30%.

## 1. Waves (must-haves first; plan §15)

| Wave | Jobs (parallel inside a wave) | Gate to next wave |
|---|---|---|
| W0 | n00 spec (this), n01 skeleton, n11 framework templates | n01 merged |
| W1 | n02a protocol-foundation, n05 tmux | n02a merged |
| W2 | n02b, n02c, n02d (protocol), n06z adapters-shared | all n02* + n06z merged |
| W3 | n03a→(n03b ∥ n03c) store, n04a→n04b graph, n06a, n06b, n06c adapters, n12a docs start | store + graph + adapters merged |
| W4 | n07a core-foundation → n07b → n07c session ; n08a gates ∥ n08b ledger+schedules ; then n08c loops core → n08d failover/planner/keeper | n08d merged |
| W5 | n09a → (n09b ∥ n09c) cli ; n13a install-ops ; n10a promote → n10b | CLI complete |
| W6 | n14 dogfood, n15 fresh-user, n16 site, n12b/c docs+example, fixes | |
| W7 | n17 release-prep | |

Critical path: n01 → n02a → n02c/n02d → n03a → n07a → n07b → n08c → n08d → n09b → n14. Nice-to-haves (n10a/b promote, n16 site, `session watch` importer inside n07c) are scheduled last in their waves and may be dropped without blocking n14.

## 2. Jobs

Format: **id** · kind/role · est · needs · owns — spec slice — acceptance.

### W0

**n00-design-spec** · spec/planner · done by this job · — · `docs/design/**`.

**n01-skeleton** · build · 45 min · — · `Cargo.toml`, `rust-toolchain.toml`, `crates/*/Cargo.toml`, stub `crates/*/src/{lib,main}.rs` (+ the fixed module skeleton in `conventions.md`/brief: adapters 3 modules, core 6 modules), `.github/**`, `LICENSE`, `CONTRIBUTING.md`, `SECURITY.md`, `.gitignore`.
Spec: `conventions.md` §1–3 (edition 2024, `forbid(unsafe_code)`, workspace deps, `testing` feature in adapters). Acceptance: `cargo build/test/fmt --check/clippy -D warnings` green on the stub workspace; CI file valid YAML (fmt, clippy, test on Linux + macOS).

**n11-framework-templates** · docs · 60 min · — · `framework/**`. Spec: plan §12; brief/report front-matter from `types.md` §8; reports must parse with `Report::parse` once n02c exists (a later gate in n15). Acceptance: templates + three skills + `opencode.jsonc`; both harnesses load them (file layout check script in `framework/tests/`).

### W1–W2 — `vowp-protocol` (spec: `types.md`, `crates/vowp-protocol.md`)

**n02a-protocol-foundation** · build · 60 min · n00, n01 · `crates/vowp-protocol/Cargo.toml`, `src/{lib,ids,time,error,log,cost,limit,executor}.rs`, creates empty placeholder files for all other modules (R1), `tests/{ids,time,executor,cost}.rs`.
Slice: types.md §1–4 (ids, Timestamp, Clock/FakeClock, ErrorKind/ProtocolError, Executor/Profile/OnQuota, Cost/Tokens, Limit), `Log`/`Level`/`MemoryLog`, `prelude`. Acceptance: protocol tests `id_*`, `timestamp_*`, `executor_*`, `profile_pick_*`, `cost_*`, `log_*`. Also resolves the `serde_yaml_ng` availability question (conventions §3).

**n02b-protocol-events** · build · 60 min · n02a · `src/event.rs`, `src/capsule.rs`, `tests/{events,capsule}.rs`, fixtures `tests/fixtures/event_*.json`, `capsule_*.json`.
Slice: types.md §5–6 (EventEnvelope, EventType, every payload, `EventBody` parts/from_parts/summary, BlobRef, Capsule/CapsuleDraft validate + `render_markdown` golden). Acceptance: `event_body_roundtrip_every_type`, `envelope_*`, `capsule_*` tests.

**n02c-protocol-domain** · build · 75 min · n02a · `src/{job,gate,report,run,session,inbox,loops,release}.rs`, `tests/{job,gate,report,domain}.rs`, fixtures `job_*.json`, `report_*.md`, `brief_*.md`.
Slice: types.md §7–10 (Job/NewJob/JobPatch/states, Gate/Evidence/default_gates/short forms, Report/Brief front-matter, Run/Worker/Session/Switch/InboxItem/Loop/Schedule/Ledger/Release). Acceptance: exactly the tests of `crates/vowp-protocol.md` §6 for these modules: `gate_short_form_table`, `gate_serde_matches_plan_yaml`, `gate_validate_rejects_bad_regex_and_empty_run`, `default_gates_per_kind`, `evidence_serde_tagged`, `report_parse_good`, `report_parse_bad_cases`, `report_missing_fields`, `brief_roundtrip_and_from_job`, `job_patch_json_roundtrip_with_loop_and_defaults` (+ the domain-row serde tests listed there).

**n02d-protocol-config-traits** · build · 75 min · n02a, (n02b, n02c for compile: `needs` all three) · `src/{config,adapter,services,schema}.rs`, `tests/{config,schema,traits}.rs`, `tests/fixtures/vowp.example.toml` (byte-identical to architecture.md §5.2 — a test asserts it when the repo docs are reachable).
Slice: types.md §11–14 + architecture.md §5.3 validation rules. Acceptance: `config_example_parses`, `config_rejects_unknown_fields_and_bad_values` (covering validation rules 1–8), `config_defaults_are_documented_values`, `service_cancel_token`, `schema_export_covers_listed_types`, `types_md_examples_validate` (names from `crates/vowp-protocol.md` §6), plus `NodeFile`/`with_node_defaults` merge tests.

### W1 — `vowp-tmux` (spec: `crates/vowp-tmux.md`)

**n05-tmux** · build · 75 min · n01 · `crates/vowp-tmux/**` (starts from n01's `Cargo.toml`, removing its `vowp-protocol` dependency). Acceptance: all tests in the crate doc, run on a private socket; `source_has_no_respawn_or_send_keys` safety grep test.

### W2 — adapters shared (spec: `crates/vowp-adapters.md` §1–3, `adapters.md` §1, §5, §9)

**n06z-adapters-shared** · build · 80 min · n02a (+ n02b for `HarnessEvent`/`EventBody`; `needs` n02b, n02d for the `Adapter` trait → needs n02b, n02d) · `crates/vowp-adapters/{Cargo.toml,src/lib.rs,src/classify/**,src/proc/**,src/stream.rs,src/testing.rs,tests/fakes/**,tests/classify.rs,tests/proc.rs}`, empty placeholders for adapter modules if n01 left none.
Acceptance: crate doc §7 "n06z" tests (`classify_*`, `parse_reset_time_*`, `redact_*`, `proc_*` incl. process-group kill, reader threads).

### W3 — adapters (each owns only its module + fixtures + test file)

**n06a-adapter-opencode** · build · 75 min · n06z · `crates/vowp-adapters/src/opencode/**`, `tests/opencode.rs`, `tests/fixtures/opencode_*`. Spec: `adapters.md` §3, crate doc §4.2 + §7 "n06a". Fixtures: sanitised from the real runs named in adapters.md §9. Acceptance: all n06a tests.
**n06b-adapter-claude** · build · 75 min · n06z · `src/claude_code/**`, `tests/claude_code.rs`, `tests/fixtures/claude_*`. Spec: `adapters.md` §2, crate doc §4.1. Includes limit-text parsing via `classify`. Acceptance: all n06b tests.
**n06c-adapter-cli** · build · 75 min · n06z · `src/cli/**`, `tests/cli.rs`, `tests/fixtures/cli_*`. Spec: `adapters.md` §4 (mapping-file format). Acceptance: all n06c tests.

### W3 — `vowp-store` (spec: `crates/vowp-store.md`, `schema.sql`, `sessions.md` §1–2)

**n03a-store-db** · build · 80 min · n02c · `crates/vowp-store/{Cargo.toml,src/{lib,error,paths,db,convert,repo_nodes,repo_jobs,repo_runs}.rs,src/schema/001_init.sql}`, placeholders for the rest (R1), `tests/{migrate,jobs,runs,schema_enums}.rs`.
Acceptance (names from `crates/vowp-store.md` §6): `schema_file_is_identical_to_design`, `open_creates_schema_version_1`, `open_refuses_newer_schema`, `open_is_idempotent`, `schema_matches_enums`, `job_insert_defaults_and_needs_edges`, `job_insert_duplicate_and_unknown_need`, `job_state_roundtrip_all_states`, `list_jobs_filter_and_order`, `edges_add_remove_and_queries`, `delete_job_only_when_unstarted`, `run_lifecycle_and_open_runs`, `gate_results_ordering_and_latest`, `workers_slots_ensure`, `paths_layout`.
**n03b-store-repos** · build · 60 min · n03a · `src/{repo_sessions,repo_loops,repo_ledger,repo_misc}.rs`, `tests/{sessions_index,ledger,misc,loops}.rs`. Acceptance: `session_index_and_natives`, `capsule_insert_sets_last_capsule`, `switch_lifecycle_and_open_switches`, `due_schedules_selects_active_due_only`, `loop_fail_streak`, `ledger_is_append_only`, `cost_totals_group_by_each_dimension`, `cost_totals_respects_filters`, `inbox_answer_flow`, `limits_active_and_clear`, `record_probe_streak`, `releases_roundtrip`, `kv_and_requests_queue`, `fk_cascade_project_delete`, `backup_produces_openable_copy`.
**n03c-store-logs** · build · 90 min · n03a (needs n02b) · `src/{blobs,session_log,session_files,reindex}.rs`, `tests/{log,blobs,reindex,crash}.rs`. Acceptance: `blob_spill_boundary`, `blob_put_is_atomic_and_deduped`, `log_append_assigns_seq_and_ts`, `log_rejects_gap_and_duplicate_envelope`, `log_torn_tail_recovery`, `log_corrupt_middle_is_error_not_repair`, `log_survives_reopen_proptest`, `log_fsync_markers`, `log_lock_blocks_second_writer`, `log_reader_tail_and_read_from`, `log_big_tool_result_roundtrip`, `capsule_file_roundtrip`, `adapters_json_is_append_only`, `reindex_after_deleting_db_restores_index`, `reindex_preserves_switch_capsule_links`, `reindex_is_idempotent`, `reindex_single_session_leaves_others`, `reindex_skips_corrupt_session`.

### W3 — `vowp-graph` (spec: `crates/vowp-graph.md`, `loops.md` §2)

**n04a-graph-core** · build · 75 min · n02c · `crates/vowp-graph/{Cargo.toml,src/{lib,error,graph,machine}.rs}`, placeholders, `tests/{machine_table,graph_props}.rs`. Acceptance: the `t01_…`–`t37_…` transition tests (including `t12_…`, `t13_…`, `t14_…`, `t24_…`, `t33_…`, `t35_…`, `t36_…`, `t37_…`), `invalid_pairs_are_rejected_for_every_state`, `prop_transition_never_panics_and_is_pure`, `prop_terminal_states_accept_nothing`, `prop_ready_jobs_matches_brute_force`, `prop_topo_order_is_valid`, `graph_new_*` rejection tests, `ready_jobs_*` tests (names from `crates/vowp-graph.md` §9).
**n04b-graph-dispatch** · build · 75 min · n04a · `src/{owns,dispatch,render,yaml,guards}.rs`, `tests/{owns,dispatch_props,render,yaml,guards}.rs`. Acceptance: `overlap_table`, `overlap_dotdot_and_absolute_are_global`, `overlap_empty_owns_never_overlaps`, `literal_prefix_examples`, `prop_overlap_is_conservative`, `path_allowed_matches_glob_semantics`, `locks_*`, `dispatch_*`, `prop_dispatch_never_two_overlapping_owns`, `render_ascii_golden`, `render_mermaid_golden`, `yaml_*`, `stall_judge_*`, `repeat_guard_*`, `attempts_left_saturates`.

### W4 — `vowp-core` session (spec: `crates/vowp-core.md` §1–3, `sessions.md`, `adapters.md` §6)

**n07a-core-foundation** · build · 60 min · n02d, n03a, n04a, n05, n06a, n06b, n06c · `crates/vowp-core/{Cargo.toml,src/{lib,error,deps,requests}.rs}`, empty `pub mod` placeholder files per crates/vowp-core.md §1, `tests/support/{fake_adapter,fake_git}.rs`, `tests/{deps,requests}.rs`.
Acceptance: `SystemRunner` timeout/process-group test, `SystemGit` round-trip in a temp repo (worktree add/remove, rebase clean/conflict, `rebase_continue`, `add`, `checkout_path`, `commit`, `reset_hard`, `conflicted_files`, ff merge, push to bare origin, ls-remote), `AdapterRegistry` lookup/fake injection, request push/drain/apply tests, `CoreError::kind` mapping.
**n07b-session-service** · build · 90 min · n07a, n03c, n02b · `src/session/{mod,service,capsule,seed}.rs`, `tests/{session_run,capsule,seed}.rs`, `tests/adapter_contract.rs`. Slice: crate doc §3 `run_turn` + capsule + seed. Acceptance: the `run_turn_*`, `capsule_*`, `seed_*` tests and `adapters_emit_terminal_turn_ended_on_every_exit_path`.
**n07c-switch-bundle** · build · 75 min · n07b · `src/session/{switch,recovery,bundle,importer}.rs`, `tests/{switch,bundle,recovery}.rs`. Acceptance: `switch_*`, `recover_*`, `bundle_*`, `failover_roundtrip_…`; importer (`session watch`) is optional — ship or state it as `open_items`.

### W4 — `vowp-core` gates, ledger, schedules, loops (spec: `loops.md`, `crates/vowp-core.md` §4–7)

**n08a-gates** · build · 80 min · n07a, n03b, n04b · `src/gates/**`, `tests/gates_*.rs`. Slice: `loops.md` §4. Acceptance: every `gates` test in the crate doc (real git in temp repos, `secret_scan_*`).
**n08b-ledger-schedules** · build · 80 min · n07a, n03b · `src/ledger/**`, `src/schedules/**`, `tests/{ledger,schedules}_*.rs`. Slice: crate doc §5–6, `loops.md` §11, plan §11 cost. Acceptance: ledger/price/board/report tests; schedule tests incl. proptest and missed-run policies.
**n08c-loops-core** · build · 90 min · n08a, n08b, n07b, n04b · `src/loops/{mod,supervisor,reconcile,l2_worker,l3_verify,merge_queue,dispatch,runner}.rs`, `tests/loops_*.rs`, `tests/drills_core.rs`. Slice: `loops.md` §1–3, §5–6, §12 (L1 tick order, L2, L3, merge queue). Acceptance: crate-doc tests `tick_*`, `startup_*`, `dispatch_*`, `stall_*`, `continue_*`, `verify_*`, `merge_queue_*`; drills `drill_crash_continue`, `drill_stall_hang`, `drill_merge_conflict`, `drill_merge_lockfile`, `drill_gate_fail_fix_loop`, `drill_orphan_reconcile`, `drill_secret_scan_blocks_push`.
**n08d-loops-planner-failover** · build · 90 min · n08c, n07c · `src/loops/{l4_planner,l5_keeper,l6_failover,l7_inbox,l8_watchdog,digest,prompts}.rs` + `pub mod` lines in `src/loops/mod.rs`, `tests/{failover,planner,inbox}_*.rs`, `tests/drills_failover.rs`. Slice: `loops.md` §7–11. Acceptance: `failover_drill_*` = `drill_failover_and_probe_back`, `drill_credits_park`, `drill_schedule_missed_run`, plus `planner_one_turn_at_a_time`, `keeper_covers_planner…`, `rate_backoff…`, `auth_parks…`, `three_unknown_failures…`, `inbox_answer…`, `board_and_heartbeat…`.

### W5 — CLI (spec: `cli.md`, `crates/vowp-cli.md`)

**n09a-cli-core** · build · 80 min · n07a, n03b, n04b, n11 (embedded templates) · `crates/vowp-cli/{Cargo.toml,src/{main,lib,cli,ctx,output,errors,remote,framework}.rs,src/cmd/{mod,init,doctor,job,graph}.rs}` + allowed stubs for every other `cmd/*.rs` (see crate doc §1), `tests/{core,init,job,graph,remote}.rs`.
Acceptance: `parse_global_flags_and_node`, `exit_code_mapping_table`, `json_error_shape_not_found`, `init_*`, `doctor_*`, `job_*`, `graph_*`, `node_ssh_argv_quoting`, `node_flag_remote_failure_exit_7`, `help_lists_every_v0_1_command`, `ctx_build_requires_init`.
**n09b-cli-run** · build · 75 min · n09a, n08d · `src/signals.rs`, `src/cmd/{up,status,board,attach,plan,window}.rs`, `tests/{up,status,board}.rs`. Acceptance: `up_once_runs_one_tick_with_fake_adapters`, `status_check_exit_1_on_stale_heartbeat`, board golden, `plan` with a fake planner adapter creating jobs through `job add`, down/stop file test, tmux windows created via `FakeWindows` and once on a private socket.
**n09c-cli-sessions-ops** · build · 90 min · n09a, n07c, n08b · `src/cmd/{session,inbox,cost,node,loops,schedule,maint}.rs`, `tests/{session,inbox,cost,loops,schedule,maint}.rs`. Acceptance: remaining `cli.md` §8 tests + `no_command_returns_not_implemented`, `json_flag_outputs_valid_json_for_every_listing_command`.
**n13a-install-ops** · build · 60 min · n09a, n08d · `scripts/**` (`install.sh`, `vowp.service`, `vowp-watchdog.cron`/`.sh` rendered from `loops::l8` constants, uninstall), shellcheck-clean, test with a fake `systemctl`/`crontab` on PATH. Acceptance: scripts run in dry-run mode in a container; `vowp doctor` detects them.

### W5 — promotion (nice-to-have)

**n10a-promote-core** · build · 80 min · n07a, n08a · `crates/vowp-core/src/promote/**`, `tests/promote_*.rs`. Spec: `crates/vowp-core.md` §8, plan §10. Acceptance: every promote test (fake deploy/bank/probe scripts).
**n10b-cli-promote** · build · 45 min · n10a, n09a · `crates/vowp-cli/src/cmd/promote.rs`, `tests/promote.rs`. Acceptance: exit codes 0/5, inbox approval item, `releases` listing.

### W3–W7 — docs, dogfood, site, release (specs are the plan; these are not Rust jobs)

**n12a-docs-concepts** · docs · 70 min · n00 (start W3) · `docs/*.md` except `docs/design/**` (concepts, graph & loops, sessions & switching, providers & failover, Work→Production, security, troubleshooting), `README.md`. Rule: every command shown must exist in `cli.md`; commands are marked *(planned)* until n09 merges, then n12b removes the marks. Acceptance: link check + command-exists check script (`docs/tests/check-commands.sh` reading `cli.md`).
**n12b-docs-reference** · docs · 60 min · n09c, n10b · `docs/reference/**`, CLI reference generated from `vowp --help` output committed as text; JSON Schema pages from `vowp schema`. Acceptance: reference matches `help_lists_every_v0_1_command` constant.
**n12c-example** · docs · 60 min · n09b · `examples/hello-vowp/**` (3-job graph with fake-adapter mode, runnable in 5 min); Acceptance: a script runs it with `FAKE=1` and all three jobs reach `done`.
**n14-dogfood** · build / planner-led · ~60 min · n08d, n09b, n13a · — (no owned paths; fix jobs own what they fix). Switches the night from `night.py` to `vowp up` on the same graph; each breakage becomes a fix job. Acceptance: `vowp status` healthy for 30 min with ≥ 1 real job completed through gates, verify and merge.
**n15-fresh-user** · verify · 60 min · n12a, n12c, n13a · — · A clean container (`node:24` as the plan says, or the build image) follows the README literally: install → `vowp init` → `vowp doctor` → example with fake adapters, plus one tiny real DeepSeek job. Report = list of every deviation.
**n16-site** · docs · 60 min · n12a · `site/**`. Static build; not deployed.
**n17-release-prep** · release / planner · 45 min · all · `CHANGELOG.md`. Tag `v0.1.0-rc.1` in the private repo, `cargo package`/npm-wrapper dry run, morning inbox items (public repo flip, npm publish, Vercel deploy, license confirm) written — nothing published.

## 3. Dependency table (for `vowp job add --needs`)

| Job | needs |
|---|---|
| n01 | — |
| n11 | — |
| n02a | n00, n01 |
| n02b, n02c | n02a |
| n02d | n02b, n02c |
| n05 | n01 |
| n06z | n02b, n02d |
| n06a, n06b, n06c | n06z |
| n03a | n02c |
| n03b | n03a |
| n03c | n03a, n02b |
| n04a | n02c |
| n04b | n04a |
| n07a | n02d, n03a, n04a, n05, n06a, n06b, n06c |
| n07b | n07a, n03c |
| n07c | n07b |
| n08a | n07a, n03b, n04b |
| n08b | n07a, n03b |
| n08c | n08a, n08b, n07b, n04b |
| n08d | n08c, n07c |
| n09a | n07a, n03b, n04b, n11 |
| n09b | n09a, n08d |
| n09c | n09a, n07c, n08b |
| n13a | n09a, n08d |
| n10a | n07a, n08a |
| n10b | n10a, n09a |
| n12a | n00 |
| n12b | n09c, n10b |
| n12c | n09b |
| n14 | n08d, n09b, n13a |
| n15 | n12a, n12c, n13a |
| n16 | n12a |
| n17 | all |

## 4. Parallelism check (ownership disjointness inside a wave)

W2: n02b (`event.rs, capsule.rs`) ∥ n02c (`job…release.rs`) ∥ n05 (`vowp-tmux`) — disjoint. n02d starts after both.
W3: n03b ∥ n03c ∥ n04a→n04b ∥ n06a ∥ n06b ∥ n06c ∥ n12a — disjoint (separate crates/modules; n06a/b/c own distinct module dirs, fixtures and test files).
W4: n08a ∥ n08b (separate module dirs); n07b/n07c both in `session/` but ordered by `needs`; n08c/n08d both in `loops/` ordered by `needs`.
W5: n09b ∥ n09c (distinct `cmd/*.rs`, distinct tests, `lib.rs`/`cli.rs` frozen by n09a), n13a, n10a.

## 5. Notes for the planner

- Dispatch order inside a wave: priority = critical-path first (n02a 90, n02c 85, n03a 80, n07a 80, n07b 75, n08c 75, n08d 70, n09b 65, others 50, docs 30).
- Provider concurrency: up to 6 DeepSeek sessions; W3 has 7 candidates — run n12a last.
- Each build job gets an automatic `verify` job (L3). Verifiers must run the named tests, not just read the diff.
- If a crate job returns `partial`, the planner (or keeper) turns the open items into a `fix` job on the same branch before dependents start.
- The spec may be amended only by a planner-led `spec` job owning `docs/design/**`; builders raise `questions_for_owner`.
