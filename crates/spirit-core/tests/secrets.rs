//! Server secret store: strict list parsing, local name validation, the `rm --yes`
//! argv and the prompt-gated `set` path.
//!
//! Every test is hermetic. The list and remove tests use `FakeCommandRunner`; the set
//! tests use a recording prompt runner or a tiny local `sh` script. Nothing here talks
//! to a VOWP home, a real server or the owner's vault, and no fixture contains a
//! real-looking key.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use spirit_core::engine::SecretString;
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::ssh::command::{CommandOutput, CommandSpec};
use spirit_core::ssh::{
    CommandRunner, FakeCommandRunner, SshOptions, SshTarget, SystemCommandRunner,
};
use spirit_core::time::Timestamp;
use spirit_core::vowp::{
    MockVowpClient, SecretEntry, SecretListView, SecretSetRequest, SshVowpClient, VowpClient,
};
use spirit_core::wire::{parse_capacity_report, parse_fleet_status};

const SECRETS_LIST: &str = r#"[
  {
    "name": "OPENAI_API_KEY",
    "last4": "a1b2",
    "created_at": "2026-11-15T09:00:00.000Z",
    "updated_at": "2026-11-16T10:30:00.000Z"
  },
  {
    "name": "DEEPSEEK_API_KEY",
    "last4": "c3d4",
    "created_at": "2026-11-14T08:00:00.000Z",
    "updated_at": "2026-11-17T11:45:00.000Z"
  }
]"#;

const SECRETS_DOCUMENT: &str = r#"{
  "secrets": [
    {
      "name": "ANTHROPIC_API_KEY",
      "last4": "e5f6",
      "created_at": "2026-11-13T07:00:00.000Z",
      "updated_at": "2026-11-17T12:00:00.000Z"
    }
  ],
  "storage": "vault_user"
}"#;

const FRESH: &str = include_str!("../fixtures/vowp/fleet-status-fresh.json");
const CAPACITY: &str = include_str!("../fixtures/vowp/capacity-report-full.json");

/// A test value split so the repository never contains a key-shaped literal.
fn fake_value() -> String {
    ["unit", "-test", "-value"].concat()
}

fn target() -> SshTarget {
    SshTarget::new("owner", "192.0.2.10", 2222).unwrap()
}

fn options() -> SshOptions {
    SshOptions::new("/tmp/opencode/known_hosts")
}

fn client_with(responses: Vec<CommandOutput>) -> (Arc<FakeCommandRunner>, SshVowpClient) {
    let fake = Arc::new(FakeCommandRunner::new(responses));
    let client = SshVowpClient::new(fake.clone(), target(), options(), None);
    (fake, client)
}

fn remote_command(call: &CommandSpec) -> String {
    call.args.last().cloned().unwrap_or_default()
}

fn failed(status: i32, stderr: &str) -> CommandOutput {
    let mut output = CommandOutput::ok("");
    output.status = status;
    output.stderr = stderr.as_bytes().to_vec();
    output
}

fn entry(name: &str, last4: &str) -> SecretEntry {
    SecretEntry {
        name: name.to_owned(),
        last4: last4.to_owned(),
        created_at: Timestamp::parse("2026-11-15T09:00:00.000Z").unwrap(),
        updated_at: Timestamp::parse("2026-11-16T10:30:00.000Z").unwrap(),
    }
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

#[tokio::test]
async fn secret_list_parses_metadata_without_a_value_and_keeps_vowp_order() {
    let (fake, client) = client_with(vec![CommandOutput::ok(SECRETS_LIST)]);
    let view = client.secret_list().await.unwrap();

    assert_eq!(view.storage, None, "0.4.0 reports no storage mode");
    assert_eq!(
        view.secrets
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["OPENAI_API_KEY", "DEEPSEEK_API_KEY"],
        "VOWP order must be preserved"
    );
    assert_eq!(view.secrets[0].last4, "a1b2");
    assert_eq!(
        view.secrets[0].updated_at,
        Timestamp::parse("2026-11-16T10:30:00.000Z").unwrap()
    );

    let call = fake.last_call().expect("one list call");
    assert_eq!(remote_command(&call), "'vowp' '--json' 'secret' 'ls'");
    assert!(
        !call.args.iter().any(|arg| arg == "-tt"),
        "a plain read must not allocate a pty"
    );

    // The read model has metadata keys only: no value field exists anywhere.
    let serialized = serde_json::to_value(&view).unwrap();
    let entries = serialized["secrets"].as_array().unwrap();
    for item in entries {
        let mut keys = item
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();
        assert_eq!(
            keys,
            vec!["created_at", "last4", "name", "updated_at"],
            "a full value must never be part of the list model"
        );
    }
    assert!(serialized.get("value").is_none());
}

#[tokio::test]
async fn secret_list_accepts_a_storage_document_but_never_invents_a_mode() {
    let (_, client) = client_with(vec![CommandOutput::ok(SECRETS_DOCUMENT)]);
    let view = client.secret_list().await.unwrap();
    assert_eq!(view.storage.as_deref(), Some("vault_user"));
    assert_eq!(view.secrets[0].name, "ANTHROPIC_API_KEY");
}

#[tokio::test]
async fn secret_list_maps_the_vowp_05_mode_to_storage() {
    for mode in ["basic", "vault_user", "encrypted", "future_mode"] {
        let doc = format!(
            r#"{{"mode":"{mode}","secrets":[{{"name":"DEEPSEEK_API_KEY","last4":"3b35","created_at":"2026-11-15T09:00:00.000Z","updated_at":"2026-11-16T10:30:00.000Z"}}]}}"#
        );
        let (_, client) = client_with(vec![CommandOutput::ok(doc)]);
        let view = client.secret_list().await.unwrap();
        assert_eq!(view.storage.as_deref(), Some(mode));
        assert_eq!(view.secrets[0].name, "DEEPSEEK_API_KEY");
    }
    let (_, client) = client_with(vec![CommandOutput::ok(r#"{"secrets":[]}"#)]);
    assert_eq!(client.secret_list().await.unwrap().storage, None);
}

#[tokio::test]
async fn secret_list_rejects_an_unsafe_mode() {
    for mode in [
        "Basic",
        "has space",
        "a\\nb",
        "",
        "1basic",
        "x-y",
        &"a".repeat(33),
    ] {
        let doc = format!(r#"{{"mode":"{mode}","secrets":[]}}"#);
        let (_, client) = client_with(vec![CommandOutput::ok(doc)]);
        let error = client.secret_list().await.unwrap_err();
        assert_eq!(
            error.code,
            SpiritErrorCode::VowpIncompatible,
            "mode {mode:?}"
        );
    }
}

#[tokio::test]
async fn secret_list_rejects_an_unsafe_metadata_row() {
    let unsafe_name = r#"[{"name":"lower_case","last4":"a1b2","created_at":"2026-11-15T09:00:00.000Z","updated_at":"2026-11-16T10:30:00.000Z"}]"#;
    let (_, client) = client_with(vec![CommandOutput::ok(unsafe_name)]);
    let error = client.secret_list().await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::VowpIncompatible);

    let unsafe_last4 = r#"[{"name":"TEST_KEY","last4":"ab\ncd","created_at":"2026-11-15T09:00:00.000Z","updated_at":"2026-11-16T10:30:00.000Z"}]"#;
    let (_, client) = client_with(vec![CommandOutput::ok(unsafe_last4)]);
    let error = client.secret_list().await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::VowpIncompatible);
}

// ---------------------------------------------------------------------------
// Remove
// ---------------------------------------------------------------------------

#[tokio::test]
async fn secret_remove_validates_the_name_before_any_command() {
    let (fake, client) = client_with(vec![]);
    let error = client.secret_remove("lower_case").await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::InvalidInput);
    assert_eq!(fake.call_count(), 0, "an invalid name must never reach ssh");
}

#[tokio::test]
async fn secret_remove_sends_yes_and_returns_the_vowp_outcome() {
    let (fake, client) = client_with(vec![CommandOutput::ok(
        r#"{"name":"TEST_KEY","removed":true}"#,
    )]);
    let outcome = client.secret_remove("TEST_KEY").await.unwrap();
    assert_eq!(outcome.name, "TEST_KEY");
    assert!(outcome.removed);

    let call = fake.last_call().expect("one remove call");
    assert_eq!(
        remote_command(&call),
        "'vowp' '--json' 'secret' 'rm' 'TEST_KEY' '--yes'"
    );
    assert!(
        !call.args.iter().any(|arg| arg == "-tt"),
        "a remove must not allocate a pty"
    );
}

#[tokio::test]
async fn secret_remove_maps_a_missing_name_to_typed_not_found_without_stderr() {
    let raw_stderr = "internal vault detail 42; secret TEST_KEY missing";
    let (_, client) = client_with(vec![failed(3, raw_stderr)]);
    let error = client.secret_remove("TEST_KEY").await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::NotFound);
    assert!(error.message.contains("TEST_KEY"));
    let rendered = format!(
        "{error:?} {error} {}",
        error.hint.clone().unwrap_or_default()
    );
    assert!(
        !rendered.contains("internal vault detail 42"),
        "raw stderr leaked into the error: {rendered}"
    );
}

// ---------------------------------------------------------------------------
// Set: the prompt-gated path
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptBehavior {
    Succeed,
    Timeout,
    Fail,
}

#[derive(Debug)]
struct PromptCall {
    spec: CommandSpec,
    prompt: String,
    input: Vec<u8>,
    /// Offset in the scripted stream where the exact prompt ended.
    prompt_end: Option<usize>,
    /// Offset at which this fake would write stdin.
    wrote_at: Option<usize>,
}

/// A runner that only "writes" after it finds the exact prompt in its scripted stream.
struct PromptRunner {
    stream: Vec<u8>,
    behavior: PromptBehavior,
    calls: Mutex<Vec<PromptCall>>,
}

impl PromptRunner {
    fn new(stream: impl Into<Vec<u8>>, behavior: PromptBehavior) -> Self {
        Self {
            stream: stream.into(),
            behavior,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<PromptCall> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect()
    }
}

#[async_trait]
impl CommandRunner for PromptRunner {
    async fn run(&self, _spec: &CommandSpec) -> Result<CommandOutput, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Internal,
            "the set path must use run_with_prompt, not run",
        ))
    }

    async fn run_with_prompt(
        &self,
        spec: &CommandSpec,
        prompt: &str,
        input: &[u8],
    ) -> Result<CommandOutput, SpiritError> {
        let text = String::from_utf8_lossy(&self.stream).into_owned();
        let prompt_end = text.find(prompt).map(|index| index + prompt.len());
        // The fake writes only after the exact prompt; a missing prompt is a timeout,
        // never an early write.
        let wrote_at = prompt_end;
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(PromptCall {
                spec: spec.clone(),
                prompt: prompt.to_owned(),
                input: input.to_vec(),
                prompt_end,
                wrote_at,
            });
        if prompt_end.is_none() {
            return Err(SpiritError::remote_timeout(spec.timeout.as_secs()));
        }
        match self.behavior {
            PromptBehavior::Succeed => Ok(CommandOutput::ok(self.stream.clone())),
            PromptBehavior::Timeout => Err(SpiritError::remote_timeout(spec.timeout.as_secs())),
            PromptBehavior::Fail => {
                let mut output = CommandOutput::ok(self.stream.clone());
                output.status = 5;
                output.stderr = self.stream.clone();
                Ok(output)
            }
        }
    }
}

fn set_stream(last4: &str) -> Vec<u8> {
    let json = serde_json::json!({
        "name": "TEST_KEY",
        "last4": last4,
        "created_at": "2026-11-15T09:00:00.000Z",
        "updated_at": "2026-11-16T10:30:00.000Z"
    });
    // A pty transcript: a control sequence, the exact hidden prompt, CRLF, then the
    // pretty JSON VOWP prints.
    format!(
        "\u{1b}[2Kvalue for TEST_KEY: \r\n\u{1b}[?25l{}\r\n\u{1b}[?25h",
        serde_json::to_string_pretty(&json).unwrap()
    )
    .into_bytes()
}

#[tokio::test]
async fn secret_set_writes_only_after_the_exact_prompt() {
    let value = fake_value();
    let runner = Arc::new(PromptRunner::new(
        set_stream("9f8e"),
        PromptBehavior::Succeed,
    ));
    let client = SshVowpClient::new(runner.clone(), target(), options(), None);
    let request = SecretSetRequest::new("TEST_KEY", SecretString::new(value.clone())).unwrap();

    let debug_request = format!("{request:?}");
    assert!(debug_request.contains("TEST_KEY"));
    assert!(!debug_request.contains(&value));
    assert!(debug_request.contains("redacted"));

    let outcome = client.secret_set(request).await.unwrap();
    assert_eq!(outcome.name, "TEST_KEY");
    assert_eq!(outcome.last4, "9f8e");
    assert!(!format!("{outcome:?}").contains(&value));

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(call.prompt, "value for TEST_KEY: ");
    assert_eq!(
        call.input,
        format!("{value}\n").into_bytes(),
        "the client writes the value and one newline only"
    );
    assert_eq!(
        call.wrote_at, call.prompt_end,
        "stdin must be written exactly when the prompt ends"
    );
    assert!(
        call.prompt_end.is_some(),
        "the exact prompt must be present before stdin is written"
    );

    // The SSH argv carries the name, `-tt` and the normal options, never the value.
    let command = remote_command(&call.spec);
    assert_eq!(
        command, "'vowp' '--json' 'secret' 'set' 'TEST_KEY'",
        "the value must never be an argv element"
    );
    assert!(call.spec.args.iter().any(|arg| arg == "-tt"));
    assert!(call.spec.args.iter().any(|arg| arg == "BatchMode=yes"));
    assert!(
        call.spec
            .args
            .iter()
            .any(|arg| arg == "StrictHostKeyChecking=yes")
    );
    assert!(
        call.spec
            .args
            .iter()
            .any(|arg| arg == "UserKnownHostsFile=/tmp/opencode/known_hosts")
    );
    for arg in &call.spec.args {
        assert!(!arg.contains(&value), "the value leaked into argv: {arg}");
    }
    assert!(!format!("{:?}", call.spec).contains(&value));
    assert!(!format!("{command:?}").contains(&value));
}

#[tokio::test]
async fn secret_set_refuses_an_invalid_name_or_value_before_ssh() {
    let value = fake_value();
    let runner = Arc::new(PromptRunner::new(
        set_stream("9f8e"),
        PromptBehavior::Succeed,
    ));

    assert!(SecretSetRequest::new("lower_case", SecretString::new(&value)).is_err());
    assert!(SecretSetRequest::new("TEST_KEY", SecretString::new("")).is_err());
    assert!(
        SecretSetRequest::new("TEST_KEY", SecretString::new("two\nlines")).is_err(),
        "a value that cannot be one hidden line must be refused"
    );
    assert!(
        SecretSetRequest::new(
            "TEST_KEY",
            SecretString::new("x".repeat(spirit_core::vowp::SECRET_VALUE_MAX_BYTES + 1))
        )
        .is_err()
    );
    assert!(runner.calls().is_empty());
}

#[tokio::test]
async fn secret_set_prompt_timeout_is_typed_and_never_contains_the_value() {
    let value = fake_value();
    let runner = Arc::new(PromptRunner::new(
        set_stream("9f8e"),
        PromptBehavior::Timeout,
    ));
    let client = SshVowpClient::new(runner.clone(), target(), options(), None);
    let request = SecretSetRequest::new("TEST_KEY", SecretString::new(value.clone())).unwrap();

    let error = client.secret_set(request).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::RemoteTimeout);
    let rendered = format!(
        "{error:?} {error} {}",
        error.hint.clone().unwrap_or_default()
    );
    assert!(!rendered.contains(&value), "the value leaked: {rendered}");
}

#[tokio::test]
async fn secret_set_command_failure_never_echoes_the_pty_transcript() {
    let value = fake_value();
    // A broken remote that echoed the value must still not surface it.
    let stream = format!(
        "value for TEST_KEY: {value}\r\n{}",
        String::from_utf8(set_stream("9f8e")).unwrap()
    );
    let runner = Arc::new(PromptRunner::new(stream.into_bytes(), PromptBehavior::Fail));
    let client = SshVowpClient::new(runner, target(), options(), None);
    let request = SecretSetRequest::new("TEST_KEY", SecretString::new(value.clone())).unwrap();

    let error = client.secret_set(request).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Refused);
    let rendered = format!(
        "{error:?} {error} {}",
        error.hint.clone().unwrap_or_default()
    );
    assert!(!rendered.contains(&value), "the value leaked: {rendered}");
    assert!(
        !rendered.contains("9f8e"),
        "the pty transcript leaked into the error: {rendered}"
    );
}

#[tokio::test]
async fn system_runner_prompt_timeout_is_typed_and_does_not_include_the_value() {
    let value = fake_value();
    let spec =
        CommandSpec::new("sleep", vec!["5".to_owned()]).with_timeout(Duration::from_millis(150));
    let started = std::time::Instant::now();
    let error = SystemCommandRunner
        .run_with_prompt(
            &spec,
            "value for TEST_KEY: ",
            format!("{value}\n").as_bytes(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::RemoteTimeout);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(!format!("{error:?} {error}").contains(&value));
}

#[tokio::test]
async fn system_runner_never_writes_without_the_exact_prompt() {
    // If stdin were written immediately, `read` would succeed and the shell would exit
    // zero. A correct runner never writes, so the read blocks until the timeout.
    let value = fake_value();
    let script = "printf 'not the prompt'; IFS= read -r line; printf 'read:%s' \"$line\"";
    let spec = CommandSpec::new("sh", vec!["-c".to_owned(), script.to_owned()])
        .with_timeout(Duration::from_millis(300));
    let error = SystemCommandRunner
        .run_with_prompt(
            &spec,
            "value for TEST_KEY: ",
            format!("{value}\n").as_bytes(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::RemoteTimeout);
    assert!(!format!("{error:?} {error}").contains(&value));
}

#[tokio::test]
async fn system_runner_writes_after_the_exact_prompt() {
    let value = fake_value();
    let script = "printf 'value for TEST_KEY: '; IFS= read -r line; printf '{\"name\":\"TEST_KEY\",\"last4\":\"%s\"}\\n' \"${#line}\"";
    let spec = CommandSpec::new("sh", vec!["-c".to_owned(), script.to_owned()])
        .with_timeout(Duration::from_secs(5));
    let output = SystemCommandRunner
        .run_with_prompt(
            &spec,
            "value for TEST_KEY: ",
            format!("{value}\n").as_bytes(),
        )
        .await
        .unwrap();
    assert_eq!(output.status, 0);
    let expected = value.len().to_string();
    assert!(
        output
            .stdout_text()
            .contains(&format!(r#""last4":"{expected}""#)),
        "the runner did not deliver the line: {}",
        output.stdout_text()
    );
}

#[tokio::test]
async fn the_default_run_with_prompt_refuses_before_writing() {
    let fake = FakeCommandRunner::new(vec![CommandOutput::ok("never")]);
    let spec = CommandSpec::new("ssh", Vec::new());
    let error = fake
        .run_with_prompt(
            &spec,
            "value for TEST_KEY: ",
            format!("{}\n", fake_value()).as_bytes(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert!(error.message.contains("interactive input unavailable"));
    assert_eq!(
        fake.call_count(),
        0,
        "the default must not fall back to run and write immediately"
    );
}

// ---------------------------------------------------------------------------
// Mock and trait defaults
// ---------------------------------------------------------------------------

fn mock() -> MockVowpClient {
    let status = parse_fleet_status(FRESH.as_bytes()).unwrap().value;
    let capacity = parse_capacity_report(CAPACITY.as_bytes()).unwrap().value;
    MockVowpClient::new(status, capacity)
}

#[tokio::test]
async fn mock_lists_configured_secrets_and_refuses_writes() {
    let view = SecretListView {
        secrets: vec![entry("TEST_KEY", "9f8e")],
        storage: None,
    };
    let client = mock().with_secrets(view.clone());
    assert_eq!(client.secret_list().await.unwrap(), view);

    let request = SecretSetRequest::new("TEST_KEY", SecretString::new(fake_value())).unwrap();
    let error = client.secret_set(request).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert!(!format!("{error:?}").contains(&fake_value()));

    let error = client.secret_remove("TEST_KEY").await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
}

#[tokio::test]
async fn a_client_without_a_server_vault_refuses_all_three_calls() {
    struct BareClient;

    #[async_trait]
    impl VowpClient for BareClient {
        async fn probe(&self) -> Result<spirit_core::vowp::VowpProbe, SpiritError> {
            Err(SpiritError::new(SpiritErrorCode::Gate, "no probe"))
        }
        async fn fleet_status(&self) -> Result<spirit_core::wire::FleetStatus, SpiritError> {
            Err(SpiritError::new(SpiritErrorCode::Gate, "no status"))
        }
        async fn capacity_report(
            &self,
        ) -> Result<spirit_core::wire::CapacityReportDoc, SpiritError> {
            Err(SpiritError::new(SpiritErrorCode::Gate, "no capacity"))
        }
        async fn fleet_queue(&self) -> Result<Vec<spirit_core::wire::FleetQueueRow>, SpiritError> {
            Err(SpiritError::new(SpiritErrorCode::Gate, "no queue"))
        }
        async fn fleet_inbox(
            &self,
            _all: bool,
        ) -> Result<Vec<spirit_core::wire::InboxItem>, SpiritError> {
            Err(SpiritError::new(SpiritErrorCode::Gate, "no inbox"))
        }
        async fn job_action(
            &self,
            _project: &str,
            _job: &str,
            _action: spirit_core::wire::JobAction,
            _reason: Option<&str>,
        ) -> Result<(), SpiritError> {
            Err(SpiritError::new(SpiritErrorCode::Gate, "no action"))
        }
        async fn answer_inbox(
            &self,
            _project: Option<&str>,
            _id: &str,
            _answer: &str,
        ) -> Result<spirit_core::wire::InboxItem, SpiritError> {
            Err(SpiritError::new(SpiritErrorCode::Gate, "no answer"))
        }
        async fn set_target(
            &self,
            _target: u32,
            _mode: spirit_core::wire::TargetMode,
            _override_: bool,
            _reason: &str,
        ) -> Result<spirit_core::wire::CapacityTarget, SpiritError> {
            Err(SpiritError::new(SpiritErrorCode::Gate, "no target"))
        }
    }

    let client = BareClient;
    assert_eq!(
        client.secret_list().await.unwrap_err().code,
        SpiritErrorCode::Gate
    );
    let request = SecretSetRequest::new("TEST_KEY", SecretString::new(fake_value())).unwrap();
    assert_eq!(
        client.secret_set(request).await.unwrap_err().code,
        SpiritErrorCode::Gate
    );
    assert_eq!(
        client.secret_remove("TEST_KEY").await.unwrap_err().code,
        SpiritErrorCode::Gate
    );
}
