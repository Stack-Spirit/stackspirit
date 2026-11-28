//! Hermetic tests for the ACP harness chat adapter.
//!
//! Every test drives the adapter over two `tokio::io::duplex` halves. There is no harness
//! binary, network, VOWP call, keychain read or user file access. The fake server answers
//! `initialize`, `session/new` and `session/prompt` exactly like the `acp.rs` mock-process
//! tests.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, duplex};
use tokio::sync::{Notify, mpsc};

use spirit_core::engine::chat::{
    AcpChatAdapter, ChatToolStatus, HarnessChatAdapter, HarnessEvent, HarnessNativeId,
    HarnessPermissionDecision, HarnessPermissionPolicy, HarnessPermissionRequest, HarnessSession,
    HarnessStart, HarnessTurn, MAX_ACTIVITY_ITEMS, MAX_MESSAGE_CHARS,
};
use spirit_core::error::SpiritErrorCode;
use spirit_core::harnesses::{AcpClient, AcpLaunchSpec, HarnessProtocol};

const NATIVE_ID: &str = "sess-186-native-secret";
const WORKSPACE: &str = "/tmp/ss-186-work";
const EVENT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct RecordingPolicy {
    requests: Mutex<Vec<HarnessPermissionRequest>>,
}

impl RecordingPolicy {
    fn requests(&self) -> Vec<HarnessPermissionRequest> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl HarnessPermissionPolicy for RecordingPolicy {
    fn decide(&self, request: &HarnessPermissionRequest) -> HarnessPermissionDecision {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request.clone());
        HarnessPermissionDecision::AllowOnce
    }
}

fn pair() -> (AcpClient, BufReader<DuplexStream>, DuplexStream) {
    let (client_read, server_write) = duplex(64 * 1024);
    let (server_read, client_write) = duplex(64 * 1024);
    (
        AcpClient::from_io(client_read, client_write),
        BufReader::new(server_read),
        server_write,
    )
}

async fn write_json(writer: &mut DuplexStream, value: Value) {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    writer.write_all(&bytes).await.unwrap();
    writer.flush().await.unwrap();
}

async fn read_json(reader: &mut BufReader<DuplexStream>) -> Value {
    let mut line = String::new();
    let read = reader.read_line(&mut line).await.unwrap();
    assert!(read > 0, "the adapter closed the ACP stream");
    serde_json::from_str(line.trim()).unwrap()
}

async fn initialize_and_new_session(
    reader: &mut BufReader<DuplexStream>,
    writer: &mut DuplexStream,
    workspace: &str,
) {
    let initialize = read_json(reader).await;
    assert_eq!(initialize["method"], "initialize");
    assert_eq!(initialize["params"]["protocolVersion"], 1);
    assert_eq!(
        initialize["params"]["clientCapabilities"]["terminal"],
        false
    );
    assert_eq!(
        initialize["params"]["clientCapabilities"]["fs"]["readTextFile"],
        false
    );
    assert_eq!(
        initialize["params"]["clientCapabilities"]["fs"]["writeTextFile"],
        false
    );
    write_json(
        writer,
        json!({
            "jsonrpc": "2.0",
            "id": initialize["id"],
            "result": {
                "protocolVersion": 1,
                "agentCapabilities": { "loadSession": false },
                "authMethods": []
            }
        }),
    )
    .await;

    let new_session = read_json(reader).await;
    assert_eq!(new_session["method"], "session/new");
    assert_eq!(new_session["params"]["cwd"], workspace);
    write_json(
        writer,
        json!({
            "jsonrpc": "2.0",
            "id": new_session["id"],
            "result": { "sessionId": NATIVE_ID }
        }),
    )
    .await;
}

async fn send_update(writer: &mut DuplexStream, update: Value) {
    write_json(
        writer,
        json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": { "sessionId": NATIVE_ID, "update": update }
        }),
    )
    .await;
}

async fn next_event(events: &mut mpsc::Receiver<HarnessEvent>) -> HarnessEvent {
    tokio::time::timeout(EVENT_TIMEOUT, events.recv())
        .await
        .expect("timed out waiting for a harness event")
        .expect("the harness event channel closed")
}

async fn collect_until_terminal(events: &mut mpsc::Receiver<HarnessEvent>) -> Vec<HarnessEvent> {
    let mut collected = Vec::new();
    loop {
        let event = next_event(events).await;
        let terminal = matches!(
            event,
            HarnessEvent::Done { .. } | HarnessEvent::Error { .. }
        );
        collected.push(event);
        if terminal {
            return collected;
        }
    }
}

fn event_kind(event: &HarnessEvent) -> &'static str {
    match event {
        HarnessEvent::NativeSession { .. } => "native_session",
        HarnessEvent::MessageStart { .. } => "message_start",
        HarnessEvent::TextDelta { .. } => "text_delta",
        HarnessEvent::TextReplace { .. } => "text_replace",
        HarnessEvent::Tool { .. } => "tool",
        HarnessEvent::Permission { .. } => "permission",
        HarnessEvent::Usage { .. } => "usage",
        HarnessEvent::Done { .. } => "done",
        HarnessEvent::Error { .. } => "error",
    }
}

#[tokio::test]
async fn one_turn_maps_text_tool_permission_and_done_without_leaking_native_ids() {
    let (client, mut reader, mut writer) = pair();
    let server = tokio::spawn(async move {
        initialize_and_new_session(&mut reader, &mut writer, WORKSPACE).await;
        let prompt = read_json(&mut reader).await;
        assert_eq!(prompt["method"], "session/prompt");
        assert_eq!(prompt["params"]["sessionId"], NATIVE_ID);
        assert_eq!(prompt["params"]["prompt"][0]["type"], "text");
        assert_eq!(prompt["params"]["prompt"][0]["text"], "hello");

        send_update(
            &mut writer,
            json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "Hi " }
            }),
        )
        .await;
        send_update(
            &mut writer,
            json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "tool-1",
                "title": "Run the tests",
                "kind": "shell",
                "status": "in_progress"
            }),
        )
        .await;
        send_update(
            &mut writer,
            json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "tool-1",
                "status": "completed"
            }),
        )
        .await;
        write_json(
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": 99,
                "method": "session/request_permission",
                "params": {
                    "sessionId": NATIVE_ID,
                    "toolCall": {
                        "toolCallId": "tool-1",
                        "title": "Run the tests",
                        "kind": "shell"
                    },
                    "options": [
                        { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
                        { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" }
                    ]
                }
            }),
        )
        .await;
        let permission = read_json(&mut reader).await;
        assert_eq!(permission["id"], 99);
        assert_eq!(permission["result"]["outcome"]["outcome"], "selected");
        assert_eq!(permission["result"]["outcome"]["optionId"], "allow-once");

        send_update(
            &mut writer,
            json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "there" }
            }),
        )
        .await;
        write_json(
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": prompt["id"],
                "result": { "stopReason": "end_turn" }
            }),
        )
        .await;
        // `close` must shut ACP stdin; the fake server then observes EOF instead of
        // leaving a half-open child.
        let mut trailing = String::new();
        let read = reader.read_line(&mut trailing).await.unwrap();
        assert_eq!(read, 0, "the adapter did not close the ACP stream");
    });

    let policy = Arc::new(RecordingPolicy::default());
    let adapter =
        AcpChatAdapter::from_client("goose", client).with_permission_policy(policy.clone());

    let gate = adapter.preflight().await.unwrap();
    assert_eq!(gate.reason(), "harness_not_wired");
    assert!(!gate.is_ready());

    let start = HarnessStart::new("/tmp/ss-186-private-workspace");
    assert!(!format!("{start:?}").contains("ss-186-private-workspace"));

    let session = adapter.start(HarnessStart::new(WORKSPACE)).await.unwrap();
    assert_eq!(session.harness_id(), "goose");
    assert_eq!(session.protocol(), HarnessProtocol::Acp);
    assert_eq!(session.native_id(), Some(NATIVE_ID));
    assert!(!format!("{session:?}").contains(NATIVE_ID));
    assert!(!format!("{adapter:?}").contains(NATIVE_ID));

    let again = adapter.start(HarnessStart::new(WORKSPACE)).await.unwrap();
    assert_eq!(again, session);

    let (tx, mut rx) = mpsc::channel(256);
    let accepted = adapter
        .send_turn(&session, HarnessTurn::new("hello"), tx)
        .await
        .unwrap();
    let events = collect_until_terminal(&mut rx).await;

    match &events[0] {
        HarnessEvent::NativeSession { id, title } => {
            assert_eq!(id.as_str(), NATIVE_ID);
            assert_eq!(title, &None);
        }
        other => panic!("the first event must be NativeSession, got {other:?}"),
    }
    let kinds: Vec<&str> = events.iter().map(event_kind).collect();
    assert_eq!(
        kinds,
        vec![
            "native_session",
            "message_start",
            "text_delta",
            "tool",
            "tool",
            "permission",
            "text_delta",
            "done"
        ]
    );

    let mut text = String::new();
    let mut tool_statuses = Vec::new();
    let mut permissions = 0;
    for event in &events {
        match event {
            HarnessEvent::TextDelta {
                message_id,
                text: delta,
                ..
            } => {
                assert_eq!(message_id, &accepted.message_id);
                text.push_str(delta);
            }
            HarnessEvent::Tool { activity } => {
                assert_eq!(activity.name, "shell");
                assert_eq!(activity.summary.as_deref(), Some("Run the tests"));
                tool_statuses.push(activity.status);
            }
            HarnessEvent::Permission { prompt } => {
                permissions += 1;
                assert_eq!(prompt.permission, "shell");
                assert!(!prompt.id.is_empty());
            }
            HarnessEvent::Done {
                message_id,
                truncated,
            } => {
                assert_eq!(message_id, &accepted.message_id);
                assert!(!truncated);
            }
            HarnessEvent::Error { error } => panic!("unexpected error event: {error:?}"),
            _ => {}
        }
    }
    assert_eq!(text, "Hi there");
    assert_eq!(
        tool_statuses,
        vec![ChatToolStatus::Running, ChatToolStatus::Completed]
    );
    assert_eq!(permissions, 1);
    assert_eq!(
        policy.requests(),
        vec![HarnessPermissionRequest {
            permission: "shell".to_owned(),
            patterns: vec!["Run the tests".to_owned(), "shell".to_owned()],
        }]
    );

    for event in &events {
        let debug = format!("{event:?}");
        assert!(!debug.contains(NATIVE_ID), "native id leaked: {debug}");
        assert!(!debug.contains("jsonrpc"), "raw frame leaked: {debug}");
    }

    adapter.close(&session).await.unwrap();
    // Close must be idempotent and still succeed after the first call.
    adapter.close(&session).await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn a_second_send_is_chat_busy_before_a_second_prompt_is_written() {
    let (client, mut reader, mut writer) = pair();
    let prompts = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(Notify::new());
    let server = tokio::spawn({
        let prompts = Arc::clone(&prompts);
        let release = Arc::clone(&release);
        async move {
            initialize_and_new_session(&mut reader, &mut writer, WORKSPACE).await;
            let prompt = read_json(&mut reader).await;
            prompts.fetch_add(1, Ordering::SeqCst);
            send_update(
                &mut writer,
                json!({
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": "working" }
                }),
            )
            .await;
            release.notified().await;
            write_json(
                &mut writer,
                json!({
                    "jsonrpc": "2.0",
                    "id": prompt["id"],
                    "result": { "stopReason": "end_turn" }
                }),
            )
            .await;
            // No second prompt may follow the rejected send. A line here is a contract
            // failure; EOF is the successful close path.
            let mut extra = String::new();
            let read = reader.read_line(&mut extra).await.unwrap();
            assert_eq!(read, 0, "the adapter wrote a second prompt: {extra}");
        }
    });

    let adapter = AcpChatAdapter::from_client("goose", client);
    let session = adapter.start(HarnessStart::new(WORKSPACE)).await.unwrap();
    let (tx, mut rx) = mpsc::channel(256);
    adapter
        .send_turn(&session, HarnessTurn::new("one"), tx)
        .await
        .unwrap();

    // Wait until the first prompt is in flight and has produced a chunk.
    loop {
        if matches!(next_event(&mut rx).await, HarnessEvent::TextDelta { .. }) {
            break;
        }
    }

    let (second_tx, _second_rx) = mpsc::channel(4);
    let error = adapter
        .send_turn(&session, HarnessTurn::new("two"), second_tx)
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Conflict);
    assert!(error.message.starts_with("chat_busy"));
    assert_eq!(prompts.load(Ordering::SeqCst), 1);

    release.notify_one();
    let events = collect_until_terminal(&mut rx).await;
    assert!(events.iter().any(|event| matches!(
        event,
        HarnessEvent::Done {
            truncated: false,
            ..
        }
    )));
    adapter.close(&session).await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn abort_sends_cancel_and_reports_an_unconfirmed_stop() {
    let (client, mut reader, mut writer) = pair();
    let cancelled = Arc::new(Notify::new());
    let server = tokio::spawn({
        let cancelled = Arc::clone(&cancelled);
        async move {
            initialize_and_new_session(&mut reader, &mut writer, WORKSPACE).await;
            let prompt = read_json(&mut reader).await;
            send_update(
                &mut writer,
                json!({
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": "working" }
                }),
            )
            .await;
            let cancel = read_json(&mut reader).await;
            assert_eq!(cancel["method"], "session/cancel");
            assert!(cancel.get("id").is_none());
            assert_eq!(cancel["params"]["sessionId"], NATIVE_ID);
            cancelled.notify_one();
            // The adapter deliberately does not wait for this response; it must report
            // the stop as unconfirmed instead of pretending it read it.
            write_json(
                &mut writer,
                json!({
                    "jsonrpc": "2.0",
                    "id": prompt["id"],
                    "result": { "stopReason": "cancelled" }
                }),
            )
            .await;
        }
    });

    let adapter = AcpChatAdapter::from_client("goose", client);
    let session = adapter.start(HarnessStart::new(WORKSPACE)).await.unwrap();
    let (tx, mut rx) = mpsc::channel(256);
    adapter
        .send_turn(&session, HarnessTurn::new("long turn"), tx)
        .await
        .unwrap();
    loop {
        if matches!(next_event(&mut rx).await, HarnessEvent::TextDelta { .. }) {
            break;
        }
    }

    let abort = adapter.abort(&session).await.unwrap();
    assert!(!abort.stop_confirmed);
    tokio::time::timeout(EVENT_TIMEOUT, cancelled.notified())
        .await
        .expect("the fake server never received session/cancel");
    server.await.unwrap();

    // An unconfirmed abort must not fabricate a Done event.
    let mut saw_done = false;
    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await {
        saw_done |= matches!(event, HarnessEvent::Done { .. });
    }
    assert!(!saw_done);
    adapter.close(&session).await.unwrap();
}

#[tokio::test]
async fn the_default_policy_cancels_a_permission_request() {
    let (client, mut reader, mut writer) = pair();
    let server = tokio::spawn(async move {
        initialize_and_new_session(&mut reader, &mut writer, WORKSPACE).await;
        let prompt = read_json(&mut reader).await;
        write_json(
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "session/request_permission",
                "params": {
                    "sessionId": NATIVE_ID,
                    "toolCall": { "toolCallId": "tool-7", "title": "Delete a file", "kind": "edit" },
                    "options": [
                        { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" }
                    ]
                }
            }),
        )
        .await;
        let permission = read_json(&mut reader).await;
        assert_eq!(permission["id"], 7);
        assert_eq!(permission["result"]["outcome"]["outcome"], "cancelled");
        write_json(
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": prompt["id"],
                "result": { "stopReason": "cancelled" }
            }),
        )
        .await;
    });

    let adapter = AcpChatAdapter::from_client("goose", client);
    let session = adapter.start(HarnessStart::new(WORKSPACE)).await.unwrap();
    let (tx, mut rx) = mpsc::channel(256);
    adapter
        .send_turn(&session, HarnessTurn::new("ask me"), tx)
        .await
        .unwrap();
    let events = collect_until_terminal(&mut rx).await;
    assert!(events.iter().any(|event| matches!(
        event,
        HarnessEvent::Permission { prompt } if prompt.permission == "edit"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        HarnessEvent::Done {
            truncated: false,
            ..
        }
    )));
    adapter.close(&session).await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn a_json_rpc_error_becomes_a_safe_error_event_and_stops_the_session() {
    let (client, mut reader, mut writer) = pair();
    let server = tokio::spawn(async move {
        initialize_and_new_session(&mut reader, &mut writer, WORKSPACE).await;
        let prompt = read_json(&mut reader).await;
        write_json(
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": prompt["id"],
                "error": {
                    "code": -32000,
                    "message": format!("boom jsonrpc raw frame {NATIVE_ID}")
                }
            }),
        )
        .await;
    });

    let adapter = AcpChatAdapter::from_client("goose", client);
    let session = adapter.start(HarnessStart::new(WORKSPACE)).await.unwrap();
    let (tx, mut rx) = mpsc::channel(256);
    adapter
        .send_turn(&session, HarnessTurn::new("fail"), tx)
        .await
        .unwrap();
    let events = collect_until_terminal(&mut rx).await;
    let kinds: Vec<&str> = events.iter().map(event_kind).collect();
    assert_eq!(kinds.last(), Some(&"error"));

    let error = events
        .iter()
        .find_map(|event| match event {
            HarnessEvent::Error { error } => Some(error),
            _ => None,
        })
        .expect("missing the safe error event");
    assert_eq!(error.code, SpiritErrorCode::Parse);
    assert!(error.message.starts_with("harness_protocol_error"));
    for forbidden in [NATIVE_ID, "boom", "jsonrpc", "raw frame"] {
        assert!(
            !error.message.contains(forbidden),
            "raw detail leaked into the typed error: {}",
            error.message
        );
    }
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, HarnessEvent::Done { .. }))
    );

    // A protocol failure stops the session instead of reusing a desynchronized stream.
    let (second_tx, _second_rx) = mpsc::channel(4);
    let error = adapter
        .send_turn(&session, HarnessTurn::new("again"), second_tx)
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::NotFound);
    assert!(error.message.starts_with("harness_session_lost"));
    server.await.unwrap();
    adapter.close(&session).await.unwrap();
}

#[tokio::test]
async fn assistant_text_and_tool_rows_are_capped() {
    let (client, mut reader, mut writer) = pair();
    let server = tokio::spawn(async move {
        initialize_and_new_session(&mut reader, &mut writer, WORKSPACE).await;
        let prompt = read_json(&mut reader).await;
        send_update(
            &mut writer,
            json!({
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "type": "text",
                    "text": "a".repeat(MAX_MESSAGE_CHARS + 50)
                }
            }),
        )
        .await;
        for index in 0..(MAX_ACTIVITY_ITEMS + 6) {
            send_update(
                &mut writer,
                json!({
                    "sessionUpdate": "tool_call",
                    "toolCallId": format!("tool-{index}"),
                    "title": "row",
                    "kind": "shell",
                    "status": "completed"
                }),
            )
            .await;
        }
        write_json(
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": prompt["id"],
                "result": { "stopReason": "end_turn" }
            }),
        )
        .await;
    });

    let adapter = AcpChatAdapter::from_client("goose", client);
    let session = adapter.start(HarnessStart::new(WORKSPACE)).await.unwrap();
    let (tx, mut rx) = mpsc::channel(256);
    adapter
        .send_turn(&session, HarnessTurn::new("cap it"), tx)
        .await
        .unwrap();
    let events = collect_until_terminal(&mut rx).await;

    let text_chars: usize = events
        .iter()
        .map(|event| match event {
            HarnessEvent::TextDelta { text, .. } => text.chars().count(),
            _ => 0,
        })
        .sum();
    assert_eq!(text_chars, MAX_MESSAGE_CHARS);
    let tool_rows = events
        .iter()
        .filter(|event| matches!(event, HarnessEvent::Tool { .. }))
        .count();
    assert_eq!(tool_rows, MAX_ACTIVITY_ITEMS);
    assert!(events.iter().any(|event| matches!(
        event,
        HarnessEvent::Done {
            truncated: true,
            ..
        }
    )));
    server.await.unwrap();
    adapter.close(&session).await.unwrap();
}

#[tokio::test]
async fn an_incompatible_handshake_gates_and_reaps_the_client() {
    let (client, mut reader, mut writer) = pair();
    let server = tokio::spawn(async move {
        let initialize = read_json(&mut reader).await;
        write_json(
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": initialize["id"],
                "result": { "protocolVersion": 2 }
            }),
        )
        .await;
        let mut trailing = String::new();
        let read = reader.read_line(&mut trailing).await.unwrap();
        assert_eq!(read, 0, "a failed start did not reap the ACP stream");
    });

    let adapter = AcpChatAdapter::from_client("goose", client);
    let error = adapter
        .start(HarnessStart::new(WORKSPACE))
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert!(error.message.starts_with("harness_incompatible"));
    server.await.unwrap();
}

#[tokio::test]
async fn preflight_is_inert_and_a_spawn_failure_is_typed() {
    let adapter = AcpChatAdapter::new(
        "goose",
        AcpLaunchSpec::new("/nonexistent/ss-186-harness-binary"),
    );
    let gate = adapter.preflight().await.unwrap();
    assert_eq!(gate.reason(), "harness_not_wired");
    assert!(!gate.is_ready());
    assert!(!format!("{adapter:?}").contains("ss-186-harness-binary"));

    let error = adapter
        .start(HarnessStart::new(WORKSPACE))
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Engine);
    assert!(error.message.starts_with("harness_spawn_failed"));
    assert!(!error.message.contains("ss-186-harness-binary"));
    adapter
        .close(&HarnessSession::new(
            "goose",
            HarnessProtocol::Acp,
            Some(HarnessNativeId::new(NATIVE_ID)),
        ))
        .await
        .unwrap();
}

#[test]
fn core_only_values_redact_native_ids_and_workspaces() {
    let native = HarnessNativeId::new("sess-secret-42");
    let session = HarnessSession::new(
        "goose",
        HarnessProtocol::Acp,
        Some(HarnessNativeId::new("sess-secret-42")),
    );
    assert!(!format!("{native:?}").contains("sess-secret-42"));
    assert!(!format!("{session:?}").contains("sess-secret-42"));
    assert!(format!("{session:?}").contains("<redacted>"));

    let event = HarnessEvent::NativeSession {
        id: native,
        title: None,
    };
    assert!(!format!("{event:?}").contains("sess-secret-42"));
    assert!(!format!("{event:?}").contains("jsonrpc"));

    let start = HarnessStart::new("/tmp/private-workspace-186");
    assert!(!format!("{start:?}").contains("private-workspace-186"));
    let turn = HarnessTurn::new("private conversation text");
    assert!(!format!("{turn:?}").contains("private conversation text"));
}
