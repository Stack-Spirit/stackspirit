//! `vowp session tail`: argv construction, event parsing and secret redaction.

use std::sync::Arc;

use spirit_core::error::SpiritErrorCode;
use spirit_core::ssh::command::CommandOutput;
use spirit_core::ssh::{FakeCommandRunner, SshOptions, SshTarget};
use spirit_core::vowp::{SshVowpClient, VowpClient};

const SESSION: &str = "01JCF5WQ8ZK3M2V7N4P6R8T0AB";

fn client(responses: Vec<CommandOutput>) -> (Arc<FakeCommandRunner>, SshVowpClient) {
    let fake = Arc::new(FakeCommandRunner::new(responses));
    let target = SshTarget::new("owner", "192.0.2.10", 2222).unwrap();
    let client = SshVowpClient::new(
        fake.clone(),
        target,
        SshOptions::new("/tmp/opencode/known_hosts"),
        Some("/srv/vowp".to_owned()),
    );
    (fake, client)
}

const EVENTS: &str = r#"{"events":[
 {"seq":3,"ts":"2026-11-24T10:00:03.000Z","type":"tool.call","actor":"assistant","payload":{"call_id":"c1","name":"Bash","input":{"command":"cargo test -p spirit-core"}}},
 {"seq":1,"ts":"2026-11-24T10:00:01.000Z","type":"turn.started","actor":"system","payload":{"turn":1}},
 {"seq":2,"ts":"2026-11-24T10:00:02.000Z","type":"assistant.message","actor":"assistant","payload":{"text":"Reading the brief.\nThen the code."}},
 {"seq":4,"ts":"2026-11-24T10:00:04.000Z","type":"tool.result","actor":"tool","payload":{"call_id":"c1","ok":true,"output":"ok","exit_code":0}},
 {"seq":5,"ts":"2026-11-24T10:00:05.000Z","type":"future.thing","actor":"system","payload":{}}
]}"#;

#[tokio::test]
async fn tail_runs_the_documented_command_with_from() {
    let (fake, client) = client(vec![CommandOutput::ok(EVENTS)]);
    client
        .session_tail("repo", SESSION, Some(2), None)
        .await
        .unwrap();
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    let command = calls[0].args.last().cloned().unwrap();
    assert!(
        command.contains(&format!(
            "'stackspirit' '--home' '/srv/vowp' '--project' 'repo' '--json' 'session' 'tail' '--from' '2' '{SESSION}'"
        )),
        "{command}"
    );
    assert!(!command.contains("--follow"));
    assert!(!command.contains("--limit"));
}

#[tokio::test]
async fn tail_without_cursor_omits_from() {
    let (fake, client) = client(vec![CommandOutput::ok("[]")]);
    let view = client
        .session_tail("repo", SESSION, None, None)
        .await
        .unwrap();
    assert!(view.events.is_empty());
    assert_eq!(view.next_seq, 0);
    let command = fake.calls()[0].args.last().cloned().unwrap();
    assert!(!command.contains("--from"));
}

#[tokio::test]
async fn tail_parses_sorts_and_summarises_events() {
    let (_, client) = client(vec![CommandOutput::ok(EVENTS)]);
    let view = client
        .session_tail("repo", SESSION, None, None)
        .await
        .unwrap();
    let seqs: Vec<u64> = view.events.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
    assert_eq!(view.next_seq, 5);
    assert_eq!(view.events[0].summary, "Turn 1 started");
    assert_eq!(view.events[1].summary, "Assistant: Reading the brief.");
    assert!(
        view.events[1]
            .detail
            .as_deref()
            .unwrap()
            .contains("Then the code")
    );
    assert_eq!(view.events[2].summary, "Bash: cargo test -p spirit-core");
    assert_eq!(view.events[3].summary, "Tool result: ok (exit 0)");
    assert_eq!(view.events[4].summary, "future.thing");
}

#[tokio::test]
async fn tail_limit_keeps_newest_page_and_cursor_filters_old_events() {
    let (_, client) = client(vec![CommandOutput::ok(EVENTS), CommandOutput::ok(EVENTS)]);
    let newest = client
        .session_tail("repo", SESSION, None, Some(2))
        .await
        .unwrap();
    assert_eq!(
        newest.events.iter().map(|e| e.seq).collect::<Vec<_>>(),
        vec![4, 5]
    );
    let resumed = client
        .session_tail("repo", SESSION, Some(3), None)
        .await
        .unwrap();
    assert_eq!(
        resumed.events.iter().map(|e| e.seq).collect::<Vec<_>>(),
        vec![4, 5]
    );
    assert_eq!(resumed.next_seq, 5);
}

#[tokio::test]
async fn tail_accepts_json_lines() {
    let lines = concat!(
        r#"{"seq":1,"ts":"t","type":"user.message","actor":"user","payload":{"text":"hi"}}"#,
        "\n",
        r#"{"seq":2,"ts":"t","type":"error","actor":"system","payload":{"message":"boom"}}"#,
        "\n"
    );
    let (_, client) = client(vec![CommandOutput::ok(lines)]);
    let view = client
        .session_tail("repo", SESSION, None, None)
        .await
        .unwrap();
    assert_eq!(view.events.len(), 2);
    assert_eq!(view.events[1].summary, "Error: boom");
}

#[tokio::test]
async fn tail_redacts_secret_patterns_everywhere() {
    let keyword = ["api", "key"].join("_");
    let prefix = ["sk", "ant", "api03"].join("-");
    let doc = r#"[{"seq":1,"ts":"t","type":"tool.call","actor":"assistant","payload":{"name":"Bash","input":{"command":"curl -H 'Authorization: Bearer TOKPREFIX-ABCDEFGHIJKLMNOPQRSTUV' https://x"}}},
 {"seq":2,"ts":"t","type":"assistant.message","actor":"assistant","payload":{"text":"KEYWORD=hunter2supersecret\nnext"}}]"#.replace("KEYWORD", &keyword)
        .replace("TOKPREFIX", &prefix);
    let (_, client) = client(vec![CommandOutput::ok(doc)]);
    let view = client
        .session_tail("repo", SESSION, None, None)
        .await
        .unwrap();
    let dump = serde_json::to_string(&view).unwrap();
    assert!(!dump.contains(&prefix), "{dump}");
    assert!(!dump.contains("hunter2supersecret"), "{dump}");
    assert!(dump.contains("[redacted]"));
}

#[tokio::test]
async fn tail_validates_argv_values_before_running() {
    let (fake, client) = client(vec![]);
    for (project, session) in [("../repo", SESSION), ("repo", "x; rm -rf /")] {
        let error = client
            .session_tail(project, session, None, None)
            .await
            .unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::InvalidInput);
    }
    assert!(fake.calls().is_empty());
}
