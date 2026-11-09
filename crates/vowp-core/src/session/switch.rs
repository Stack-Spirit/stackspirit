//! Session switch protocol: freeze, drain, capture a capsule, detach the source and
//! attach (optionally start) the target. Spec: `docs/design/sessions.md` §5 and
//! `docs/design/crates/vowp-core.md` §3.
//!
//! The log is never rewritten; a switch only appends events. The `switches` row is the
//! crash-recovery record, [`crate::session::recovery`] finishes it after a restart.
//! A pure switch records the target attach and delivers the seed with the next turn;
//! [`run_switch_with_input`] starts the first seeded turn immediately (the L6 failover
//! path) and retries once with a capsule-only seed when the target rejects the tail.

use std::path::Path;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use vowp_protocol::{
    Actor, AttachAction, CancelToken, CapsuleTrigger, CostEvent, ErrorEvent, ErrorKind, EventBody, EventSource,
    Executor, ExecutorRef, HarnessKind, Limit, LimitEvent, LimitRowKind, NodeName, Run, RunId, RunOutcome, RunPurpose,
    RunRole, Session, SessionExecutor, SessionRole, SessionState, StartInput, Switch, SwitchEvent, SwitchId,
    SwitchRequest, SwitchState, Timestamp, TurnEnded, TurnStarted, UserMessage,
};
use vowp_store::session_files::{AdapterMapping, add_adapter_mapping};
use vowp_store::session_log::{AppendedEvent, NewEvent, SessionLog, index_row};
use vowp_store::{RunFinish, Store};

use crate::deps::Deps;
use crate::error::CoreError;
use crate::session::capsule::CapsuleWriter;
use crate::session::seed::seed_prompt;
use crate::session::service::tail_of_complete_turns;

/// Poll interval for adapter events during a seeded switch turn.
const EVENT_POLL: Duration = Duration::from_millis(200);

/// Poll interval while waiting for an active turn to drain.
const DRAIN_POLL: Duration = Duration::from_millis(20);

/// Margin added once when a seeded turn reports a provider limit.
const LIMIT_MARGIN: Duration = Duration::from_secs(120);

/// How the drain phase ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drain {
    /// The active turn ended before the deadline.
    Finished,
    /// The deadline passed and the active run was stopped.
    Interrupted,
}

/// Runs the switch protocol for `req`; the target is attached idle.
pub fn run_switch(deps: &Deps, req: &SwitchRequest, cancel: &CancelToken) -> Result<Switch, CoreError> {
    run_switch_inner(deps, req, None, cancel)
}

/// Runs the switch protocol and starts the first turn on the target with a seed.
///
/// This is the failover/context variant of [`run_switch`]: the seed is built from the
/// fresh capsule and the last complete turns; if the target rejects it, the switch
/// retries once with the capsule alone and then aborts, re-attaching the source.
pub fn run_switch_with_input(
    deps: &Deps,
    req: &SwitchRequest,
    next_input: &str,
    cancel: &CancelToken,
) -> Result<Switch, CoreError> {
    run_switch_inner(deps, req, Some(next_input), cancel)
}

/// Shared implementation of the pure and seeded switch protocols.
fn run_switch_inner(
    deps: &Deps,
    req: &SwitchRequest,
    next_input: Option<&str>,
    cancel: &CancelToken,
) -> Result<Switch, CoreError> {
    if cancel.is_cancelled() {
        return Err(CoreError::Cancelled);
    }
    let store = deps.open_store()?;
    let session = store.require_session(&req.session_id)?;
    if session.state == SessionState::Switching {
        return Err(CoreError::Store(vowp_store::StoreError::Conflict(
            "session is already switching".to_owned(),
        )));
    }
    if session.state == SessionState::Closed {
        return Err(CoreError::Invalid("session is closed; cannot switch".to_owned()));
    }

    let previous_executor = session.executor.clone();
    let mut previous_state = session.state;
    let to_node = req.to.node.clone().unwrap_or_else(|| deps.node.clone());
    let to = ExecutorRef {
        executor: req.to.executor.clone(),
        node: Some(to_node.clone()),
    };
    let at_seq = SessionLog::open_read(&deps.paths, &req.session_id)?.last_seq()?;

    store.set_session_state(&req.session_id, SessionState::Switching, previous_executor.as_ref())?;
    let mut switch = Switch {
        id: SwitchId::new(),
        session_id: req.session_id.clone(),
        from_executor: previous_executor.clone(),
        to_executor: req.to.executor.clone(),
        from_node: previous_executor.as_ref().map(|_| session.node.clone()),
        to_node: to_node.clone(),
        capsule_id: None,
        reason: req.reason,
        state: SwitchState::Started,
        error: None,
        started_at: deps.clock.now(),
        ended_at: None,
        at_seq,
    };
    store.insert_switch(&switch)?;

    let target = deps.adapters.get(&req.to.executor);
    let probe = target.as_ref().map(|adapter| adapter.probe());
    let probe_ok = probe.as_ref().is_some_and(|probe| probe.ok);
    if !probe_ok {
        let detail = match probe {
            Some(probe) => format!("target probe failed: {}", probe.detail),
            None => "target harness is not installed".to_owned(),
        };
        return abort(
            deps,
            &store,
            &switch,
            previous_state,
            previous_executor.as_ref(),
            &session.node,
            false,
            ErrorKind::AdapterUnavailable,
            &detail,
        );
    }

    if is_active(&store, &session)? {
        drain(deps, &store, &session, req.drain_secs, cancel)?;
        previous_state = SessionState::Idle;
    }

    let capsule = match CapsuleWriter::new(deps.clone()).write(&req.session_id, CapsuleTrigger::Switch) {
        Ok(capsule) => Some(capsule),
        Err(error) => match store.latest_capsule(&req.session_id)? {
            Some(previous) => {
                let log = SessionLog::open(&deps.paths, &req.session_id, deps.clock.clone())?;
                append_event(
                    &store,
                    &log,
                    HarnessKind::Vowp,
                    EventBody::Note(vowp_protocol::Note {
                        text: format!("stale capsule used: {error}"),
                    }),
                    Some(deps.node.clone()),
                    None,
                )?;
                log.sync()?;
                Some(previous)
            }
            None => {
                return abort(
                    deps,
                    &store,
                    &switch,
                    previous_state,
                    previous_executor.as_ref(),
                    &session.node,
                    false,
                    ErrorKind::Parse,
                    &format!("capsule invalid after repair: {error}"),
                );
            }
        },
    };
    switch.capsule_id = capsule.as_ref().map(|capsule| capsule.capsule_id.clone());

    let log = SessionLog::open(&deps.paths, &req.session_id, deps.clock.clone())?;
    if let Some(source) = &previous_executor {
        append_event(
            &store,
            &log,
            source.harness,
            EventBody::SessionExecutor(SessionExecutor {
                action: AttachAction::Detach,
                executor: source.clone(),
                node: session.node.clone(),
                native_session_id: None,
                run_id: None,
                reason: Some("switch".to_owned()),
            }),
            Some(session.node.clone()),
            None,
        )?;
    }
    store.set_session_state(&req.session_id, SessionState::Switching, None)?;

    let mut final_state = SessionState::Idle;
    match next_input {
        Some(input) => {
            let capsule = capsule
                .as_ref()
                .ok_or_else(|| CoreError::CapsuleInvalid("switch has no capsule to seed".to_owned()))?;
            let events = SessionLog::open_read(&deps.paths, &req.session_id)?.read_all()?;
            let tail = tail_of_complete_turns(&events, deps.config.capsule.tail_turns);
            let seeded = seed_prompt(capsule, &tail, deps.config.capsule.tail_turns, input);
            let capsule_only = seed_prompt(capsule, &[], deps.config.capsule.tail_turns, input);
            match run_target_turn(deps, &store, &log, &session, &to, seeded) {
                Ok(state) => final_state = state,
                Err(first) => match run_target_turn(deps, &store, &log, &session, &to, capsule_only) {
                    Ok(state) => final_state = state,
                    Err(second) => {
                        drop(log);
                        return abort(
                            deps,
                            &store,
                            &switch,
                            previous_state,
                            previous_executor.as_ref(),
                            &session.node,
                            true,
                            ErrorKind::AdapterFailed,
                            &format!("target rejected the seed twice: {first}; {second}"),
                        );
                    }
                },
            }
        }
        None => {
            append_event(
                &store,
                &log,
                req.to.executor.harness,
                EventBody::SessionExecutor(SessionExecutor {
                    action: AttachAction::Attach,
                    executor: req.to.executor.clone(),
                    node: to_node.clone(),
                    native_session_id: None,
                    run_id: None,
                    reason: None,
                }),
                Some(to_node.clone()),
                None,
            )?;
            store.set_session_state(&req.session_id, SessionState::Switching, Some(&req.to.executor))?;
        }
    }

    append_event(
        &store,
        &log,
        req.to.executor.harness,
        EventBody::Switch(SwitchEvent {
            switch_id: switch.id.clone(),
            from: previous_executor.clone().map(|executor| ExecutorRef {
                executor,
                node: Some(session.node.clone()),
            }),
            to,
            capsule_id: switch.capsule_id.clone(),
            reason: req.reason,
        }),
        Some(to_node),
        None,
    )?;
    log.sync()?;
    store.finish_switch(&switch.id, SwitchState::Completed, None)?;
    store.set_session_state(&req.session_id, final_state, Some(&req.to.executor))?;
    switch.state = SwitchState::Completed;
    switch.ended_at = Some(deps.clock.now());
    Ok(switch)
}

/// Returns whether the session has a run that has not finished.
fn is_active(store: &Store, session: &Session) -> Result<bool, CoreError> {
    if session.state == SessionState::Running {
        return Ok(true);
    }
    Ok(store
        .open_runs()?
        .iter()
        .any(|run| run.session_id.as_ref() == Some(&session.id)))
}

/// Waits up to `drain_secs` for the active turn to end, then stops it.
///
/// The adapter handle lives in the runner thread, which is not reachable from here; the
/// observable contract is the one the spec records in the database: the active run is
/// finished as `killed` and an interrupted `turn.ended` is appended when the adapter did
/// not emit one.
fn drain(
    deps: &Deps,
    store: &Store,
    session: &Session,
    drain_secs: u64,
    cancel: &CancelToken,
) -> Result<Drain, CoreError> {
    let start_seq = SessionLog::open_read(&deps.paths, &session.id)?.last_seq()?;
    let deadline = deps.clock.now().plus(Duration::from_secs(drain_secs));
    loop {
        if cancel.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        let events = SessionLog::open_read(&deps.paths, &session.id)?.read_all()?;
        if events
            .iter()
            .any(|event| event.seq > start_seq && matches!(event.body(), Ok(EventBody::TurnEnded(_))))
        {
            return Ok(Drain::Finished);
        }
        if deps.clock.now() >= deadline {
            break;
        }
        std::thread::sleep(DRAIN_POLL);
    }

    for run in store
        .open_runs()?
        .into_iter()
        .filter(|run| run.session_id.as_ref() == Some(&session.id))
    {
        store.finish_run(
            &run.id,
            &RunFinish {
                outcome: RunOutcome::Killed,
                exit_code: None,
                limit_kind: None,
                cost: vowp_protocol::Cost::unknown(),
                native_session_id: None,
                detail: "interrupted by switch drain timeout".to_owned(),
            },
        )?;
    }
    let events = SessionLog::open_read(&deps.paths, &session.id)?.read_all()?;
    let interrupted = events
        .iter()
        .any(|event| event.seq > start_seq && matches!(event.body(), Ok(EventBody::TurnEnded(_))));
    if !interrupted {
        let turn = events
            .iter()
            .filter(|event| event.event_type == vowp_protocol::EventType::TurnStarted)
            .count()
            .saturating_add(1) as u32;
        let log = SessionLog::open(&deps.paths, &session.id, deps.clock.clone())?;
        append_event(
            store,
            &log,
            HarnessKind::Vowp,
            EventBody::TurnEnded(TurnEnded {
                turn,
                stop_reason: vowp_protocol::StopReason::Interrupted,
                duration_ms: None,
            }),
            Some(deps.node.clone()),
            None,
        )?;
        log.sync()?;
    }
    Ok(Drain::Interrupted)
}

/// Aborts a switch: error event, state restored, row finished `aborted`.
#[allow(clippy::too_many_arguments)]
fn abort(
    deps: &Deps,
    store: &Store,
    switch: &Switch,
    previous_state: SessionState,
    previous_executor: Option<&Executor>,
    previous_node: &NodeName,
    reattach_source: bool,
    kind: ErrorKind,
    detail: &str,
) -> Result<Switch, CoreError> {
    let log = SessionLog::open(&deps.paths, &switch.session_id, deps.clock.clone())?;
    append_event(
        store,
        &log,
        HarnessKind::Vowp,
        EventBody::Error(ErrorEvent {
            kind,
            message: vowp_adapters::classify::redact(detail, 300),
            retryable: false,
        }),
        Some(deps.node.clone()),
        None,
    )?;
    if reattach_source {
        match previous_executor {
            Some(executor) => {
                append_event(
                    store,
                    &log,
                    executor.harness,
                    EventBody::SessionExecutor(SessionExecutor {
                        action: AttachAction::Attach,
                        executor: executor.clone(),
                        node: previous_node.clone(),
                        native_session_id: None,
                        run_id: None,
                        reason: Some("switch_abort".to_owned()),
                    }),
                    Some(previous_node.clone()),
                    None,
                )?;
                store.set_session_state(&switch.session_id, previous_state, Some(executor))?;
            }
            None => {
                store.set_session_state(&switch.session_id, SessionState::Blocked, None)?;
            }
        }
    } else {
        store.set_session_state(&switch.session_id, previous_state, previous_executor)?;
    }
    log.sync()?;
    store.finish_switch(&switch.id, SwitchState::Aborted, Some(detail))?;
    let mut aborted = switch.clone();
    aborted.state = SwitchState::Aborted;
    aborted.error = Some(detail.to_owned());
    aborted.ended_at = Some(deps.clock.now());
    Err(CoreError::SwitchAborted(detail.to_owned()))
}

/// Runs the first turn after a seeded switch, retrying is the caller's policy.
///
/// The caller holds the session's writer lock for `log`; this function appends the
/// attach, turn framing, adapter events and detach, in the same order as `run_turn`.
fn run_target_turn(
    deps: &Deps,
    store: &Store,
    log: &SessionLog,
    session: &Session,
    target: &ExecutorRef,
    prompt: String,
) -> Result<SessionState, CoreError> {
    let executor = target.executor.clone();
    let node = target.node.clone().unwrap_or_else(|| deps.node.clone());
    let adapter = deps.adapters.get(&executor).ok_or_else(|| {
        CoreError::Adapter(vowp_protocol::AdapterError::Unavailable(
            "harness not installed".to_owned(),
        ))
    })?;
    let prior = SessionLog::open_read(&deps.paths, &session.id)?.read_all()?;
    let turn = prior
        .iter()
        .filter(|event| event.event_type == vowp_protocol::EventType::TurnStarted)
        .count()
        .saturating_add(1) as u32;
    let run_id = RunId::new();
    let run = Run {
        id: run_id.clone(),
        job_id: session.job_id.clone(),
        session_id: Some(session.id.clone()),
        role: run_role(session.role),
        purpose: RunPurpose::Continue,
        profile: None,
        rung: 0,
        executor: Some(executor.clone()),
        provider: Some(executor.provider()),
        node: node.clone(),
        native_session_id: None,
        pid: None,
        window: None,
        cwd: Some(session.cwd.clone()),
        started_at: deps.clock.now(),
        ended_at: None,
        exit_code: None,
        outcome: None,
        limit_kind: None,
        cost: vowp_protocol::Cost::unknown(),
        detail: String::new(),
    };
    let attach = append_event(
        store,
        log,
        executor.harness,
        EventBody::SessionExecutor(SessionExecutor {
            action: AttachAction::Attach,
            executor: executor.clone(),
            node: node.clone(),
            native_session_id: None,
            run_id: Some(run_id.clone()),
            reason: None,
        }),
        Some(node.clone()),
        None,
    )?;
    let attach_seq = attach.envelope.seq;
    append_event(
        store,
        log,
        executor.harness,
        EventBody::TurnStarted(TurnStarted { turn }),
        Some(node.clone()),
        None,
    )?;
    append_event(
        store,
        log,
        executor.harness,
        EventBody::UserMessage(UserMessage {
            text: prompt.clone(),
            attachments: Vec::new(),
            note: Some("handoff".to_owned()),
        }),
        Some(node.clone()),
        None,
    )?;
    store.insert_run(&run)?;
    store.set_session_state(&session.id, SessionState::Running, Some(&executor))?;

    let start = StartInput {
        session_id: session.id.clone(),
        executor: executor.clone(),
        cwd: Path::new(&session.cwd).to_path_buf(),
        prompt,
        resume: None,
        new_native_id: None,
        env: start_env(deps, &executor),
        permission_mode: harness_config(deps, &executor).and_then(|config| config.permission_mode.clone()),
        system_note: None,
        event_log_hint: Some(deps.paths.run_raw_path(&run_id)),
        timeout: None,
    };
    let mut adapter_run = match adapter.start(start) {
        Ok(adapter_run) => adapter_run,
        Err(error) => {
            append_event(
                store,
                log,
                executor.harness,
                EventBody::Error(ErrorEvent {
                    kind: ErrorKind::AdapterFailed,
                    message: vowp_adapters::classify::redact(&error.to_string(), 300),
                    retryable: false,
                }),
                Some(node.clone()),
                None,
            )?;
            append_event(
                store,
                log,
                executor.harness,
                EventBody::TurnEnded(TurnEnded {
                    turn,
                    stop_reason: vowp_protocol::StopReason::Error,
                    duration_ms: None,
                }),
                Some(node.clone()),
                None,
            )?;
            append_event(
                store,
                log,
                executor.harness,
                EventBody::SessionExecutor(SessionExecutor {
                    action: AttachAction::Detach,
                    executor: executor.clone(),
                    node: node.clone(),
                    native_session_id: None,
                    run_id: Some(run_id.clone()),
                    reason: Some("crash".to_owned()),
                }),
                Some(node.clone()),
                None,
            )?;
            log.sync()?;
            store.finish_run(
                &run_id,
                &RunFinish {
                    outcome: RunOutcome::Error,
                    exit_code: None,
                    limit_kind: None,
                    cost: vowp_protocol::Cost::unknown(),
                    native_session_id: None,
                    detail: vowp_adapters::classify::redact(&error.to_string(), 1500),
                },
            )?;
            store.set_session_state(&session.id, SessionState::Idle, Some(&executor))?;
            return Err(CoreError::Adapter(error));
        }
    };
    store.update_run_attach(&run_id, adapter_run.pid(), None, None, None)?;

    let mut emitted_cost = false;
    let mut emitted_limit = false;
    let mut emitted_error = false;
    let mut terminal: Option<vowp_protocol::HarnessEvent> = None;
    loop {
        match adapter_run.events().recv_timeout(EVENT_POLL) {
            Ok(event) => {
                match &event.body {
                    EventBody::Cost(_) => emitted_cost = true,
                    EventBody::Limit(_) => emitted_limit = true,
                    EventBody::Error(_) => emitted_error = true,
                    EventBody::TurnEnded(_) => {
                        terminal = Some(event);
                        break;
                    }
                    _ => {}
                }
                append_harness(store, log, &event, &executor, &node)?;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let summary = match adapter_run.wait() {
        Ok(summary) => summary,
        Err(error) => {
            store.finish_run(
                &run_id,
                &RunFinish {
                    outcome: RunOutcome::Error,
                    exit_code: None,
                    limit_kind: None,
                    cost: vowp_protocol::Cost::unknown(),
                    native_session_id: None,
                    detail: error.to_string(),
                },
            )?;
            store.set_session_state(&session.id, SessionState::Idle, Some(&executor))?;
            return Err(CoreError::Adapter(error));
        }
    };
    if summary.cost.usd.is_some() && !emitted_cost {
        append_event(
            store,
            log,
            executor.harness,
            EventBody::Cost(CostEvent {
                cost: summary.cost.clone(),
                run_id: Some(run_id.clone()),
            }),
            Some(node.clone()),
            summary.native_session_id.clone(),
        )?;
    }
    if let Some(limit) = &summary.limit
        && !emitted_limit
    {
        append_event(
            store,
            log,
            executor.harness,
            EventBody::Limit(limit.clone()),
            Some(node.clone()),
            summary.native_session_id.clone(),
        )?;
    } else if !emitted_error && matches!(summary.outcome, RunOutcome::Error | RunOutcome::Crash) {
        append_event(
            store,
            log,
            executor.harness,
            EventBody::Error(ErrorEvent {
                kind: ErrorKind::AdapterFailed,
                message: vowp_adapters::classify::redact(
                    summary
                        .result_text
                        .clone()
                        .unwrap_or_else(|| "adapter ended without a result".to_owned())
                        .as_str(),
                    300,
                ),
                retryable: summary.outcome == RunOutcome::Crash,
            }),
            Some(node.clone()),
            summary.native_session_id.clone(),
        )?;
    }
    let (duration_ms, terminal_ts) = match terminal.as_ref().map(|event| (&event.body, event.ts)) {
        Some((EventBody::TurnEnded(ended), ts)) => (ended.duration_ms, ts),
        _ => (None, None),
    };
    append_ts_event(
        store,
        log,
        executor.harness,
        EventBody::TurnEnded(TurnEnded {
            turn,
            stop_reason: summary.stop_reason,
            duration_ms,
        }),
        terminal_ts,
        Some(node.clone()),
        summary.native_session_id.clone(),
    )?;
    append_event(
        store,
        log,
        executor.harness,
        EventBody::SessionExecutor(SessionExecutor {
            action: AttachAction::Detach,
            executor: executor.clone(),
            node: node.clone(),
            native_session_id: summary.native_session_id.clone(),
            run_id: Some(run_id.clone()),
            reason: Some(
                match summary.limit {
                    Some(_) => "limit",
                    None if summary.outcome == RunOutcome::Crash => "crash",
                    None => "turn_done",
                }
                .to_owned(),
            ),
        }),
        Some(node.clone()),
        summary.native_session_id.clone(),
    )?;
    log.sync()?;

    if let Some(native) = &summary.native_session_id {
        store.upsert_native(&session.id, executor.harness, native, &node, attach_seq)?;
        add_adapter_mapping(
            &deps.paths,
            &session.id,
            AdapterMapping {
                harness: executor.harness,
                native_session_id: native.clone(),
                node: node.clone(),
                first_seq: attach_seq,
                created_at: deps.clock.now(),
            },
        )?;
    }
    let state = if matches!(
        summary.outcome,
        RunOutcome::Limit | RunOutcome::Credits | RunOutcome::Auth | RunOutcome::Rate | RunOutcome::Context
    ) {
        SessionState::Blocked
    } else {
        SessionState::Idle
    };
    store.set_session_state(&session.id, state, Some(&executor))?;
    store.finish_run(
        &run_id,
        &RunFinish {
            outcome: summary.outcome,
            exit_code: summary.exit_code,
            limit_kind: summary.limit.as_ref().map(|limit| limit.kind),
            cost: summary.cost.clone(),
            native_session_id: summary.native_session_id.clone(),
            detail: vowp_adapters::classify::redact(
                summary
                    .limit
                    .as_ref()
                    .map(|limit| limit.message.clone())
                    .or_else(|| summary.result_text.clone())
                    .unwrap_or_default()
                    .as_str(),
                1500,
            ),
        },
    )?;
    if let Some(limit) = &summary.limit {
        record_limit(deps, store, limit, executor.harness)?;
    }
    Ok(state)
}

/// Appends one event with the canonical actor and indexes it.
fn append_event(
    store: &Store,
    log: &SessionLog,
    harness: HarnessKind,
    body: EventBody,
    node: Option<NodeName>,
    native: Option<String>,
) -> Result<AppendedEvent, CoreError> {
    append_ts_event(store, log, harness, body, None, node, native)
}

/// Appends one event with an optional harness timestamp and indexes it.
fn append_ts_event(
    store: &Store,
    log: &SessionLog,
    harness: HarnessKind,
    body: EventBody,
    ts: Option<Timestamp>,
    node: Option<NodeName>,
    native: Option<String>,
) -> Result<AppendedEvent, CoreError> {
    let appended = log.append(NewEvent {
        actor: actor_for(&body),
        source: EventSource {
            harness,
            model: None,
            node,
            native_session_id: native,
        },
        body,
        raw: None,
        ts,
    })?;
    store.index_event(&index_row(&appended))?;
    Ok(appended)
}

/// Forwards one adapter event.
fn append_harness(
    store: &Store,
    log: &SessionLog,
    event: &vowp_protocol::HarnessEvent,
    executor: &Executor,
    node: &NodeName,
) -> Result<(), CoreError> {
    let appended = log.append(NewEvent {
        actor: event.actor,
        source: EventSource {
            harness: executor.harness,
            model: Some(executor.model.clone()),
            node: Some(node.clone()),
            native_session_id: None,
        },
        body: event.body.clone(),
        raw: event.raw.clone(),
        ts: event.ts,
    })?;
    store.index_event(&index_row(&appended))?;
    Ok(())
}

/// Records a provider limit with the single two-minute margin.
fn record_limit(deps: &Deps, store: &Store, event: &LimitEvent, harness: HarnessKind) -> Result<(), CoreError> {
    store.upsert_limit(&Limit {
        provider: event.provider.clone(),
        harness: Some(harness),
        kind: match event.kind {
            vowp_protocol::LimitKind::Quota => LimitRowKind::Quota,
            vowp_protocol::LimitKind::Credits => LimitRowKind::Credits,
            vowp_protocol::LimitKind::Rate => LimitRowKind::Rate,
            vowp_protocol::LimitKind::Context => LimitRowKind::Context,
            vowp_protocol::LimitKind::Auth => LimitRowKind::Auth,
        },
        resets_at: event.resets_at.map(|resets| resets.plus(LIMIT_MARGIN)),
        detail: event.message.clone(),
        since: deps.clock.now(),
        last_probe_at: None,
        probe_fail_streak: 0,
        concurrency_cap: None,
        cap_until: None,
    })?;
    Ok(())
}

/// Returns the canonical actor for a framing event.
fn actor_for(body: &EventBody) -> Actor {
    match body {
        EventBody::UserMessage(_) => Actor::User,
        EventBody::AssistantMessage(_)
        | EventBody::AssistantReasoning(_)
        | EventBody::ToolCall(_)
        | EventBody::ToolResult(_)
        | EventBody::FileChange(_)
        | EventBody::Plan(_) => Actor::Assistant,
        _ => Actor::System,
    }
}

/// Maps a session role to the run role used by a seeded switch turn.
fn run_role(role: Option<SessionRole>) -> RunRole {
    match role {
        Some(SessionRole::Planner) => RunRole::Planner,
        Some(SessionRole::Keeper) => RunRole::Keeper,
        Some(SessionRole::Verifier) => RunRole::Verifier,
        Some(SessionRole::Worker | SessionRole::User) | None => RunRole::Worker,
    }
}

/// Returns the configured harness entry for `executor`.
fn harness_config<'a>(deps: &'a Deps, executor: &Executor) -> Option<&'a vowp_protocol::HarnessConfig> {
    let key = match executor.harness {
        HarnessKind::ClaudeCode => "claude-code",
        HarnessKind::Opencode => "opencode",
        HarnessKind::Cli => executor.model.as_str(),
        HarnessKind::Vowp => return None,
    };
    deps.config
        .harnesses
        .get(key)
        .or_else(|| deps.config.harnesses.get(&format!("cli:{key}")))
}

/// Resolves `[harnesses.*].env_pass` names from the process environment.
fn start_env(deps: &Deps, executor: &Executor) -> Vec<(String, String)> {
    let Some(config) = harness_config(deps, executor) else {
        return Vec::new();
    };
    config
        .env_pass
        .iter()
        .filter_map(|name| std::env::var(name).ok().map(|value| (name.clone(), value)))
        .collect()
}
