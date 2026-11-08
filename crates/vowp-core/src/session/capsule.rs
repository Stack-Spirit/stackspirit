//! Capsule writer: compact the session log, ask a cheap executor for a handoff draft,
//! validate it, repair once, then persist file + row + `capsule` event.
//! Spec: `docs/design/sessions.md` §3 and `docs/design/crates/vowp-core.md` §3.

use std::path::Path;
use std::time::Duration;

use vowp_protocol::{
    CancelToken, Capsule, CapsuleDraft, CapsuleTrigger, EventBody, EventEnvelope, EventType, Executor, ExecutorRef,
    HarnessKind, RunPurpose, RunRole, SessionId, SessionRole, TurnRequest, schema_for_name,
};
use vowp_store::Store;
use vowp_store::session_log::{NewEvent, SessionLog, index_row};

use crate::deps::Deps;
use crate::error::CoreError;
use crate::session::service::{last_assistant_text, persist_capsule, telemetry, trigger_name};

/// Maximum characters of one tool call input shown to the capsule model.
const TOOL_INPUT_CHARS: usize = 300;

/// Maximum characters of one tool result body shown before compaction.
const TOOL_RESULT_CHARS: usize = 400;

/// Maximum characters of one assistant message before compaction.
const MESSAGE_COMPACT_CHARS: usize = 1200;

/// Number of trailing events compaction always keeps.
const KEEP_TAIL_EVENTS: usize = 20;

/// The capsule writer.
pub struct CapsuleWriter {
    deps: Deps,
}

impl CapsuleWriter {
    /// Creates a writer over `deps`.
    pub fn new(deps: Deps) -> Self {
        Self { deps }
    }

    /// Writes a capsule for `session`.
    ///
    /// Debounced triggers return the latest capsule when it is younger than
    /// `[capsule].min_interval_secs`; switch, manual, export and limit never debounce.
    pub fn write(&self, session: &SessionId, trigger: CapsuleTrigger) -> Result<Capsule, CoreError> {
        let store = self.deps.open_store()?;
        let row = store.require_session(session)?;
        let latest = store.latest_capsule(session)?;
        if !exempt_from_debounce(trigger) {
            if let Some(capsule) = &latest {
                let min = Duration::from_secs(self.deps.config.capsule.min_interval_secs);
                if self.deps.clock.now().since(capsule.created_at) < min {
                    return Ok(capsule.clone());
                }
            }
        }

        let events = SessionLog::open_read(&self.deps.paths, session)?.read_all()?;
        let since: Vec<EventEnvelope> = events
            .iter()
            .filter(|event| latest.as_ref().is_none_or(|capsule| event.seq > capsule.at_seq))
            .filter(|event| event.replayable)
            .filter(|event| !matches!(event.event_type, EventType::AssistantReasoning | EventType::Cost))
            .cloned()
            .collect();

        let (executor, rung) = self.capsule_executor()?;
        let from = ExecutorRef {
            executor: row.executor.clone().unwrap_or_else(|| executor.clone()),
            node: Some(row.node.clone()),
        };
        let prompt = render_prompt(
            &row.title,
            row.job_id.as_ref().map(ToString::to_string).as_deref(),
            &row.cwd,
            latest.as_ref(),
            &since,
            &from,
            trigger,
            self.deps.config.capsule.max_input_chars as usize,
        );

        let capsule_session = crate::session::service::SessionService::new(self.deps.clone()).create_session(
            vowp_protocol::NewSession {
                project: row.project.clone(),
                job_id: row.job_id.clone(),
                role: SessionRole::Keeper,
                title: format!("capsule {session}"),
                cwd: Path::new(&row.cwd).to_path_buf(),
                parent: Some(session.clone()),
                executor: Some(executor.clone()),
            },
        )?;

        let draft = match self.run_draft(&capsule_session, &executor, rung, &prompt, row.job_id.clone()) {
            Ok(draft) => draft,
            Err(first) => {
                let repair = repair_prompt(&first.to_string());
                match self.run_draft(&capsule_session, &executor, rung, &repair, row.job_id.clone()) {
                    Ok(draft) => draft,
                    Err(second) => {
                        let message = format!("capsule invalid after repair: {second}");
                        self.log_error(session, &message)?;
                        return Err(CoreError::CapsuleInvalid(message));
                    }
                }
            }
        };

        let at_seq = events.last().map(|event| event.seq).unwrap_or(1).max(1);
        let capsule = Capsule {
            version: vowp_protocol::CAPSULE_VERSION,
            capsule_id: vowp_protocol::CapsuleId::new(),
            session_id: session.clone(),
            created_at: self.deps.clock.now(),
            at_seq,
            trigger,
            from,
            intent: draft.intent,
            constraints: draft.constraints,
            decisions: draft.decisions,
            state: draft.state,
            files: draft.files,
            gotchas: draft.gotchas,
            open_questions: draft.open_questions,
            telemetry: telemetry(&events),
        };
        capsule.validate()?;
        persist_capsule(&self.deps, &store, &capsule)?;
        self.record_state_ledger(&store, &capsule)?;
        Ok(capsule)
    }

    /// Renders the exact capsule prompt for `last` and the new `events` slice.
    ///
    /// The trigger is inferred from the previous capsule, else from a fresh `limit` event,
    /// else `turn_end`; the writer itself always uses its explicit trigger.
    pub fn build_prompt(&self, last: Option<&Capsule>, events: &[EventEnvelope], from: &ExecutorRef) -> String {
        let trigger = last.map(|capsule| capsule.trigger).unwrap_or_else(|| {
            if events.iter().any(|event| event.event_type == EventType::Limit) {
                CapsuleTrigger::Limit
            } else {
                CapsuleTrigger::TurnEnd
            }
        });
        let created = events.iter().find_map(|event| match event.body() {
            Ok(EventBody::SessionCreated(payload)) => Some(payload),
            _ => None,
        });
        let session_id = last
            .map(|capsule| capsule.session_id.clone())
            .or_else(|| events.first().map(|event| event.session_id.clone()))
            .unwrap_or_else(|| {
                last.map(|capsule| capsule.session_id.clone())
                    .unwrap_or_else(SessionId::new)
            });
        let title = created
            .as_ref()
            .map(|payload| payload.title.clone())
            .unwrap_or_else(|| session_id.to_string());
        let cwd = created.as_ref().map(|payload| payload.cwd.clone()).unwrap_or_default();
        let job_id = created
            .as_ref()
            .and_then(|payload| payload.job_id.as_ref().map(ToString::to_string));
        render_prompt_inferred(
            &title,
            job_id.as_deref(),
            &cwd,
            last,
            events,
            from,
            trigger,
            self.deps.config.capsule.max_input_chars as usize,
        )
    }

    /// Parses a capsule draft from model text, tolerating fences and leading prose.
    pub fn parse_draft(&self, text: &str) -> Result<CapsuleDraft, CoreError> {
        let start = text
            .find('{')
            .ok_or_else(|| CoreError::CapsuleInvalid("no JSON object in the capsule answer".to_owned()))?;
        let candidate = first_json_object(&text[start..])
            .ok_or_else(|| CoreError::CapsuleInvalid("unterminated JSON object in the capsule answer".to_owned()))?;
        serde_json::from_str::<CapsuleDraft>(candidate).map_err(|error| CoreError::CapsuleInvalid(error.to_string()))
    }

    /// Resolves the configured capsule executor and ladder rung.
    fn capsule_executor(&self) -> Result<(Executor, u32), CoreError> {
        if let Some(executor) = &self.deps.config.capsule.executor {
            return Ok((executor.clone(), 0));
        }
        let keeper = self
            .deps
            .config
            .profiles
            .get("keeper")
            .ok_or_else(|| CoreError::Invalid("no [capsule].executor and no keeper profile".to_owned()))?;
        let rung = keeper.ladder.len().saturating_sub(1);
        let executor = keeper
            .ladder
            .get(rung)
            .cloned()
            .ok_or_else(|| CoreError::Invalid("keeper profile has an empty ladder".to_owned()))?;
        Ok((executor, rung as u32))
    }

    /// Runs one capsule turn and parses its final assistant message.
    fn run_draft(
        &self,
        capsule_session: &SessionId,
        executor: &Executor,
        rung: u32,
        prompt: &str,
        job_id: Option<vowp_protocol::JobId>,
    ) -> Result<CapsuleDraft, CoreError> {
        let service = crate::session::service::SessionService::new(self.deps.clone());
        let result = service.run_turn(
            TurnRequest {
                session_id: capsule_session.clone(),
                executor: ExecutorRef {
                    executor: executor.clone(),
                    node: None,
                },
                purpose: RunPurpose::Capsule,
                role: RunRole::Capsule,
                job_id,
                prompt: prompt.to_owned(),
                profile: Some("keeper".to_owned()),
                rung,
                window: None,
                timeout: None,
            },
            &CancelToken::default(),
        )?;
        let text = last_assistant_text(&self.deps.paths, capsule_session)?
            .or(result.summary.result_text)
            .ok_or_else(|| CoreError::CapsuleInvalid("capsule executor returned no text".to_owned()))?;
        let draft = self.parse_draft(&text)?;
        // Validate the same structural rules the assembled capsule will face.
        Capsule {
            version: vowp_protocol::CAPSULE_VERSION,
            capsule_id: vowp_protocol::CapsuleId::new(),
            session_id: capsule_session.clone(),
            created_at: self.deps.clock.now(),
            at_seq: 1,
            trigger: CapsuleTrigger::Manual,
            from: ExecutorRef {
                executor: executor.clone(),
                node: None,
            },
            intent: draft.intent.clone(),
            constraints: draft.constraints.clone(),
            decisions: draft.decisions.clone(),
            state: draft.state.clone(),
            files: draft.files.clone(),
            gotchas: draft.gotchas.clone(),
            open_questions: draft.open_questions.clone(),
            telemetry: Default::default(),
        }
        .validate()?;
        Ok(draft)
    }

    /// Appends the error event to the summarised session.
    fn log_error(&self, session: &SessionId, message: &str) -> Result<(), CoreError> {
        let store = self.deps.open_store()?;
        let log = SessionLog::open(&self.deps.paths, session, self.deps.clock.clone())?;
        let appended = log.append(NewEvent {
            actor: vowp_protocol::Actor::System,
            source: vowp_protocol::EventSource {
                harness: HarnessKind::Vowp,
                model: None,
                node: Some(self.deps.node.clone()),
                native_session_id: None,
            },
            body: EventBody::Error(vowp_protocol::ErrorEvent {
                kind: vowp_protocol::ErrorKind::Parse,
                message: message.to_owned(),
                retryable: false,
            }),
            raw: None,
            ts: None,
        })?;
        store.index_event(&index_row(&appended))?;
        log.sync()?;
        Ok(())
    }

    /// Appends the `state` ledger row for a written capsule.
    fn record_state_ledger(&self, store: &Store, capsule: &Capsule) -> Result<(), CoreError> {
        store.append_ledger(&vowp_protocol::LedgerEntry {
            id: None,
            ts: self.deps.clock.now(),
            kind: vowp_protocol::LedgerKind::State,
            project: None,
            job_id: None,
            run_id: None,
            session_id: Some(capsule.session_id.clone()),
            role: None,
            provider: None,
            harness: None,
            model: None,
            cost: None,
            summary: "capsule written".to_owned(),
            detail: serde_json::json!({
                "capsule_id": capsule.capsule_id,
                "trigger": trigger_name(capsule.trigger),
                "at_seq": capsule.at_seq,
            }),
        })?;
        Ok(())
    }
}

/// Returns true for triggers exempt from the 120-second debounce.
fn exempt_from_debounce(trigger: CapsuleTrigger) -> bool {
    matches!(
        trigger,
        CapsuleTrigger::Switch | CapsuleTrigger::Manual | CapsuleTrigger::Export | CapsuleTrigger::Limit
    )
}

/// Builds the prompt from a stored session row.
#[allow(clippy::too_many_arguments)]
fn render_prompt(
    title: &str,
    job_id: Option<&str>,
    cwd: &str,
    last: Option<&Capsule>,
    events: &[EventEnvelope],
    from: &ExecutorRef,
    trigger: CapsuleTrigger,
    max_input_chars: usize,
) -> String {
    render_prompt_inferred(title, job_id, cwd, last, events, from, trigger, max_input_chars)
}

/// Builds the exact prompt template from sessions.md §3.
#[allow(clippy::too_many_arguments)]
fn render_prompt_inferred(
    title: &str,
    job_id: Option<&str>,
    cwd: &str,
    last: Option<&Capsule>,
    events: &[EventEnvelope],
    from: &ExecutorRef,
    trigger: CapsuleTrigger,
    max_input_chars: usize,
) -> String {
    let schema = schema_for_name("CapsuleDraft").unwrap_or(serde_json::Value::Null);
    let schema = serde_json::to_string_pretty(&schema).unwrap_or_default();
    let previous = match last {
        Some(capsule) => serde_json::to_string_pretty(capsule).unwrap_or_else(|_| "none".to_owned()),
        None => "none".to_owned(),
    };
    let base_at_seq = last.map(|capsule| capsule.at_seq).unwrap_or(0);
    let first_seq = events.first().map(|event| event.seq).unwrap_or(base_at_seq + 1);
    let last_seq = events.last().map(|event| event.seq).unwrap_or(base_at_seq);
    let rendered = compact_events(events, max_input_chars);

    format!(
        "You are the handoff writer for a software-engineering session. Another AI agent, possibly on a different harness and model, will continue this work\nfrom your handoff alone. Write a precise, factual capsule. Do not invent facts that are not in the material. Prefer concrete file paths, commands and decisions.\n\nSESSION: {title}  (job: {}, working directory: {cwd})\nEXECUTOR THAT WAS RUNNING: {}   REASON FOR HANDOFF: {}\n\nPREVIOUS CAPSULE (covers events up to seq {base_at_seq}; update it, do not discard still-valid facts):\n{previous}\n\nNEW MATERIAL (events seq {first_seq}..{last_seq}):\n{rendered}\n\nReturn ONLY one JSON object, no markdown fence, no commentary, that validates against this JSON Schema:\n{schema}\n\nRules:\n- \"intent\": one paragraph, what the user is trying to achieve (never empty).\n- \"state.done/in_progress/blocked/next_steps\": short imperative or past-tense lines; next_steps ordered by priority.\n- \"files\": only files actually read/created/modified/deleted, with a role and a short note.\n- \"decisions\": what was decided and why; \"gotchas\": traps the next agent must know; \"open_questions\": unresolved items.\n- Never include secrets, tokens, or key values; refer to them by name only.\n",
        job_id.unwrap_or("none"),
        from.executor,
        trigger_name(trigger),
    )
}

/// Returns the repair prompt appended after an invalid first answer.
fn repair_prompt(error: &str) -> String {
    let schema = schema_for_name("CapsuleDraft")
        .and_then(|schema| serde_json::to_string_pretty(&schema).ok())
        .unwrap_or_default();
    format!("Your previous answer was invalid: {error}. Return ONLY the corrected JSON object.\n\n{schema}\n")
}

/// Renders events, then compacts them to `max_input_chars`.
fn compact_events(events: &[EventEnvelope], max_input_chars: usize) -> String {
    let mut blocks: Vec<Block> = events.iter().filter_map(render_block).collect();
    if blocks.is_empty() {
        return "(no new events)".to_owned();
    }

    let mut total = total_chars(&blocks);
    while total > max_input_chars {
        let mut changed = false;
        for block in &mut blocks {
            if block.tool_body {
                block.text = format!("{} (output omitted)", block.header);
                block.tool_body = false;
                changed = true;
                if total <= max_input_chars {
                    break;
                }
            }
        }
        total = total_chars(&blocks);
        if total <= max_input_chars || !changed {
            break;
        }
    }

    if total > max_input_chars {
        for block in &mut blocks {
            if block.kind == BlockKind::Assistant && block.text.chars().count() > MESSAGE_COMPACT_CHARS {
                block.text = middle_truncate(&block.text, MESSAGE_COMPACT_CHARS);
            }
        }
        total = total_chars(&blocks);
    }

    if total > max_input_chars {
        let mut dropped = 0_usize;
        let mut index = 0_usize;
        while index < blocks.len() && total > max_input_chars {
            let first_user_now = blocks
                .iter()
                .position(|block| block.kind == BlockKind::User)
                .unwrap_or(0);
            let protected = index == first_user_now || index >= blocks.len().saturating_sub(KEEP_TAIL_EVENTS);
            if protected {
                index += 1;
                continue;
            }
            total = total.saturating_sub(blocks[index].text.chars().count() + 1);
            blocks.remove(index);
            dropped += 1;
        }
        if dropped > 0 {
            let marker = format!("[… {dropped} earlier events omitted …]");
            let insert_at = blocks
                .iter()
                .position(|block| block.kind == BlockKind::User)
                .map(|index| index + 1)
                .unwrap_or(0)
                .min(blocks.len());
            blocks.insert(
                insert_at,
                Block {
                    text: marker,
                    header: String::new(),
                    kind: BlockKind::Note,
                    tool_body: false,
                },
            );
        }
    }

    blocks
        .iter()
        .map(|block| block.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// One rendered event block.
struct Block {
    text: String,
    header: String,
    kind: BlockKind,
    tool_body: bool,
}

/// Classes that compaction treats differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    User,
    Assistant,
    Other,
    Note,
}

/// Renders one event block, or `None` for events excluded from capsule input.
fn render_block(event: &EventEnvelope) -> Option<Block> {
    let seq = event.seq;
    let (text, kind, tool_body) = match event.body().ok()? {
        EventBody::UserMessage(payload) => (
            format!("[seq {seq} · user] {}", clip(&payload.text, 3000)),
            BlockKind::User,
            false,
        ),
        EventBody::AssistantMessage(payload) => (
            format!("[seq {seq} · assistant] {}", clip(&payload.text, 3000)),
            BlockKind::Assistant,
            false,
        ),
        EventBody::ToolCall(payload) => (
            format!(
                "[seq {seq} · tool.call] {} {}",
                payload.name,
                clip(
                    &serde_json::to_string(&payload.input).unwrap_or_default(),
                    TOOL_INPUT_CHARS
                )
            ),
            BlockKind::Other,
            false,
        ),
        EventBody::ToolResult(payload) => {
            let status = if payload.ok { "ok" } else { "failed" };
            let header = format!("[seq {seq} · tool.result {status}]");
            match (&payload.output, &payload.output_ref) {
                (_, Some(reference)) => (
                    format!(
                        "{header} {} (… {} bytes more, blob {})",
                        clip(&reference.preview, TOOL_RESULT_CHARS),
                        reference.bytes.saturating_sub(TOOL_RESULT_CHARS as u64),
                        short_sha(&reference.sha256)
                    ),
                    BlockKind::Other,
                    true,
                ),
                (Some(output), None) => {
                    let remaining = output.chars().count().saturating_sub(TOOL_RESULT_CHARS);
                    let more = if remaining > 0 {
                        format!(" (… {remaining} chars more)")
                    } else {
                        String::new()
                    };
                    (
                        format!("{header} {}{more}", clip(output, TOOL_RESULT_CHARS)),
                        BlockKind::Other,
                        true,
                    )
                }
                (None, None) => (format!("{header} (no output)"), BlockKind::Other, false),
            }
        }
        EventBody::FileChange(payload) => (
            format!(
                "[seq {seq} · file.change] {} {}",
                payload.path,
                serde_json::to_value(payload.op)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default()
            ),
            BlockKind::Other,
            false,
        ),
        EventBody::Plan(payload) => {
            let items = payload
                .items
                .iter()
                .map(|item| {
                    format!(
                        "{}: {}",
                        serde_json::to_value(item.status)
                            .ok()
                            .and_then(|value| value.as_str().map(str::to_owned))
                            .unwrap_or_default(),
                        item.text
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            (
                format!("[seq {seq} · plan] {}", clip(&items, 300)),
                BlockKind::Other,
                false,
            )
        }
        EventBody::Limit(payload) => (
            format!("[seq {seq} · limit] {}", clip(&payload.message, 300)),
            BlockKind::Other,
            false,
        ),
        EventBody::Error(payload) => (
            format!("[seq {seq} · error] {}", clip(&payload.message, 300)),
            BlockKind::Other,
            false,
        ),
        EventBody::Switch(payload) => (
            format!(
                "[seq {seq} · switch] {} -> {}",
                payload
                    .from
                    .as_ref()
                    .map(|from| from.executor.to_string())
                    .unwrap_or_else(|| "none".to_owned()),
                payload.to.executor
            ),
            BlockKind::Other,
            false,
        ),
        EventBody::Note(payload) => (
            format!("[seq {seq} · note] {}", clip(&payload.text, 300)),
            BlockKind::Other,
            false,
        ),
        _ => return None,
    };
    Some(Block {
        text,
        header: if tool_body {
            match event.body() {
                Ok(EventBody::ToolResult(payload)) => {
                    let status = if payload.ok { "ok" } else { "failed" };
                    format!("[seq {seq} · tool.result {status}]")
                }
                _ => String::new(),
            }
        } else {
            String::new()
        },
        kind,
        tool_body,
    })
}

/// Returns the first balanced JSON object at the start of `text`.
fn first_json_object(text: &str) -> Option<&str> {
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return text.get(..index + character.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

/// Clips `text` to `max` characters, appending an ellipsis when clipped.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut clipped: String = text.chars().take(max.saturating_sub(1)).collect();
    clipped.push('…');
    clipped
}

/// Keeps the head and tail of `text` around a middle ellipsis.
fn middle_truncate(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max || max < 5 {
        return text.to_owned();
    }
    let head = max / 2;
    let tail = max - head - 1;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(chars[chars.len() - tail..].iter());
    out
}

/// Returns the total rendered characters plus one separator per block.
fn total_chars(blocks: &[Block]) -> usize {
    blocks.iter().map(|block| block.text.chars().count() + 1).sum()
}

/// Returns the first eight sha256 characters.
fn short_sha(sha: &str) -> &str {
    sha.get(..8).unwrap_or(sha)
}
