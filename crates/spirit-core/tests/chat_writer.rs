//! Hermetic tests for the Phase 1 chat writer/lease manager.
//!
//! Every test runs the checked-in fake `vowp` from `tests/fixtures/fake-vowp/` through
//! a private temporary `PATH` and `VOWP_AGENT_HOME`. No test touches a real VOWP home,
//! `~/.vowp`, `~/.vowp-agent`, the keychain, the network or a real engine. The fake
//! attaches to a local fake OpenCode server only when a send-gate test needs one.
#![cfg(unix)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::{Value, json};

use spirit_core::engine::chat::{
    ChatAccess, ChatEngine, ChatModelTier, ChatModelView, ChatSessionId, ChatSessionLocation,
    ChatSessionState, ChatStartInput, ChatWriter, ChatWriterConfig, ChatWriterError,
    ChatWriterReason, ChatWriterState, LocalChatProvider, VowpEndpointResolver, WriterTarget,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};

const HOME_SESSION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const OTHER_SESSION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const NATIVE: &str = "ses_fake_native";
const ENGINE_CREDENTIAL: &str = "fake-engine-credential";

static CANARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn canary() -> String {
    let sequence = CANARY_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(
        "fake-lease-canary-{}-{sequence}-{nanos}",
        std::process::id()
    )
}

/// Inherited `PATH` directories minus any that hold a host `stackspirit` or `vowp`, so a
/// host install cannot shadow the fake binary under test.
fn host_path_without_engines() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|existing| {
            std::env::split_paths(&existing)
                .filter(|dir| !dir.join("stackspirit").exists() && !dir.join("vowp").exists())
                .collect()
        })
        .unwrap_or_default()
}

fn path_with(dir: &Path) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    paths.extend(host_path_without_engines());
    std::env::join_paths(paths).expect("join fake PATH")
}

async fn wait_until(mut predicate: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    predicate()
}

struct FakeVowp {
    /// Keeps the private temporary tree alive for the whole test.
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    bin: PathBuf,
    state: PathBuf,
    canary: String,
}

impl FakeVowp {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("fake vowp temp dir");
        let bin = dir.path().join("bin");
        let state = dir.path().join("state");
        std::fs::create_dir_all(&bin).expect("fake bin dir");
        std::fs::create_dir_all(&state).expect("fake state dir");
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-vowp/vowp");
        let binary = bin.join("vowp");
        std::fs::copy(&source, &binary).expect("copy fake vowp");
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake vowp");
        let canary = canary();
        let fake = Self {
            dir,
            bin,
            state,
            canary,
        };
        fake.write("mode", "attach-grant");
        fake.write("canary", &fake.canary);
        fake.write("session", HOME_SESSION);
        fake.write("native", NATIVE);
        fake.write("node", "fake-node");
        fake
    }

    fn write(&self, name: &str, value: &str) {
        std::fs::write(self.state.join(name), value).expect("write fake state");
    }

    fn read(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.state.join(name)).ok()
    }

    fn set_mode(&self, mode: &str) {
        self.write("mode", mode);
    }

    fn set_grant_variant(&self, variant: &str) {
        self.write("grant-variant", variant);
    }

    fn set_url(&self, url: &str) {
        self.write("url", url);
    }

    fn set_session(&self, session: &str) {
        self.write("session", session);
    }

    fn path(&self) -> std::ffi::OsString {
        path_with(&self.bin)
    }

    fn config(&self) -> ChatWriterConfig {
        ChatWriterConfig::new()
            .with_path_env(self.path())
            .with_agent_home(self.state.clone())
            .with_home_env(self.state.clone())
            .with_probe_timeout(Duration::from_secs(2))
            .with_attach_timeout(Duration::from_secs(2))
            .with_release_timeout(Duration::from_secs(2))
            .with_attach_drain_timeout(Duration::from_millis(300))
            .with_watch_interval(Duration::from_millis(25))
            .with_watch_retry_delay(Duration::from_millis(20))
    }

    fn resolver(&self) -> VowpEndpointResolver {
        VowpEndpointResolver::with_path_env(self.path())
            .with_agent_home(self.state.clone())
            .with_home_env(self.state.clone())
    }

    fn target(&self) -> WriterTarget {
        WriterTarget::new("global", ChatSessionId::Global, HOME_SESSION, NATIVE)
            .with_node(Some("fake-node".to_owned()))
    }

    fn argv(&self) -> Vec<Vec<String>> {
        let Ok(text) = std::fs::read_to_string(self.state.join("argv.log")) else {
            return Vec::new();
        };
        let mut calls = Vec::new();
        let mut current = Vec::new();
        for line in text.lines() {
            if line == "CALL" {
                if !current.is_empty() {
                    calls.push(std::mem::take(&mut current));
                }
            } else if let Some(arg) = line.strip_prefix("ARG\t") {
                current.push(arg.to_owned());
            }
        }
        if !current.is_empty() {
            calls.push(current);
        }
        calls
    }

    fn attach_calls(&self) -> Vec<Vec<String>> {
        self.argv()
            .into_iter()
            .filter(|call| {
                call.iter().any(|arg| arg == "attach")
                    && !call.iter().any(|arg| arg == "--help")
                    && !call.iter().any(|arg| arg == "endpoint")
            })
            .collect()
    }

    fn attach_count(&self) -> u32 {
        self.read_number("attach-count")
    }

    fn watch_count(&self) -> u32 {
        self.read_number("watch-count")
    }

    fn read_number(&self, name: &str) -> u32 {
        std::fs::read_to_string(self.state.join(name))
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0)
    }

    fn surface_ids(&self) -> Vec<String> {
        std::fs::read_to_string(self.state.join("surface-ids.log"))
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    fn released(&self) -> bool {
        self.state.join("released").exists()
    }

    fn stdin_open(&self) -> bool {
        self.state.join("stdin-open").exists()
    }

    fn pid(&self) -> Option<u32> {
        std::fs::read_to_string(self.state.join("attach-pid"))
            .ok()
            .and_then(|value| value.trim().parse().ok())
    }

    /// Asserts the canary token appears in no app-visible file.
    ///
    /// `canary` is the test's planted input, so it is the one state file excluded.
    fn assert_no_token_leak(&self, app_dir: &Path) {
        for path in files_under(app_dir) {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(
                !text.contains(&self.canary),
                "the canary token leaked into {}",
                path.display()
            );
        }
        for path in files_under(&self.state) {
            if path.file_name().is_some_and(|name| name == "canary") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(
                !text.contains(&self.canary),
                "the canary token leaked into {}",
                path.display()
            );
        }
    }
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(path);
        }
    }
    files
}

fn writer_for(fake: &FakeVowp) -> Arc<ChatWriter> {
    Arc::new(ChatWriter::new(fake.target(), fake.config()))
}

fn safe_view_json(value: &impl serde::Serialize) -> Value {
    serde_json::to_value(value).expect("serialize a safe view")
}

fn assert_safe_writer_view(value: &Value) {
    for forbidden in [
        "token",
        "surface_id",
        "url",
        "port",
        "native_session_id",
        "session_id",
        "epoch",
        "expires_at",
        "path",
        "Authorization",
        "http://",
        "127.0.0.1",
        "surf_fake_",
        "01ARZ",
    ] {
        assert!(
            !value.to_string().contains(forbidden),
            "a writer view leaked {forbidden}: {value}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_available_uses_the_home_fallback_and_never_spawns_an_attach() {
    let fake = FakeVowp::new();
    let writer = writer_for(&fake);
    let view = writer.probe().await;
    assert_eq!(view.state, ChatWriterState::Available);
    assert!(view.can_take_over);
    assert!(!view.can_release);
    assert_eq!(view.reason, None);
    assert_safe_writer_view(&safe_view_json(&view));
    assert_eq!(fake.attach_count(), 0);
    assert!(!fake.stdin_open());
    let calls = fake.argv();
    assert!(
        calls.iter().any(|call| call
            == &[
                "agent".to_owned(),
                "orchestrator".to_owned(),
                "endpoint".to_owned(),
                "global".to_owned(),
                "--json".to_owned()
            ]),
        "the agent command must be tried first: {calls:?}"
    );
    assert!(
        calls.iter().any(|call| call
            == &[
                "orchestrator".to_owned(),
                "endpoint".to_owned(),
                "global".to_owned(),
                "--json".to_owned()
            ]),
        "the home fallback must run only after the agent has no local record: {calls:?}"
    );
    assert!(
        calls.iter().any(|call| call
            == &[
                "orchestrator".to_owned(),
                "attach".to_owned(),
                "--help".to_owned()
            ]),
        "the capability probe must be read-only: {calls:?}"
    );
    assert!(
        !calls
            .iter()
            .any(|call| call.iter().any(|arg| arg == "--node")),
        "--node must never be used: {calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_node_hosted_agent_document_is_unavailable() {
    let fake = FakeVowp::new();
    fake.set_mode("endpoint-node");
    let writer = writer_for(&fake);
    let view = writer.probe().await;
    assert_eq!(view.state, ChatWriterState::Unavailable);
    assert_eq!(view.reason, Some(ChatWriterReason::NodeHosted));
    assert!(!view.can_take_over);
    assert_eq!(fake.attach_count(), 0);
    assert!(
        !fake.argv().iter().any(|call| call
            == &[
                "orchestrator".to_owned(),
                "endpoint".to_owned(),
                "global".to_owned(),
                "--json".to_owned()
            ]),
        "a node agent document must not trigger the home fallback"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_node_hosted_home_metadata_is_unavailable() {
    let fake = FakeVowp::new();
    fake.set_mode("endpoint-home-node");
    let writer = writer_for(&fake);
    let view = writer.probe().await;
    assert_eq!(view.state, ChatWriterState::Unavailable);
    assert_eq!(view.reason, Some(ChatWriterReason::NodeHosted));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_vowp_too_old_when_attach_is_missing() {
    let fake = FakeVowp::new();
    fake.set_mode("attach-old");
    let writer = writer_for(&fake);
    let view = writer.probe().await;
    assert_eq!(view.state, ChatWriterState::Unavailable);
    assert_eq!(view.reason, Some(ChatWriterReason::VowpTooOld));
    assert_eq!(fake.attach_count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_stopped_not_home_and_session_mismatch_are_typed() {
    for (mode, expected) in [
        ("endpoint-stopped", ChatWriterReason::OrchestratorStopped),
        ("not-home", ChatWriterReason::NotHome),
    ] {
        let fake = FakeVowp::new();
        fake.set_mode(mode);
        let writer = writer_for(&fake);
        let view = writer.probe().await;
        assert_eq!(view.state, ChatWriterState::Unavailable, "{mode}");
        assert_eq!(view.reason, Some(expected), "{mode}");
        assert_eq!(fake.attach_count(), 0, "{mode}");
    }

    let fake = FakeVowp::new();
    fake.set_session(OTHER_SESSION);
    let writer = writer_for(&fake);
    let view = writer.probe().await;
    assert_eq!(view.state, ChatWriterState::Unavailable);
    assert_eq!(
        view.reason,
        Some(ChatWriterReason::SessionMismatch),
        "argv={:?} mode={:?} session={:?} url={:?}",
        fake.argv(),
        fake.read("mode"),
        fake.read("session"),
        fake.read("url")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn take_over_free_attach_validates_reports_and_releases() {
    let fake = FakeVowp::new();
    let app = tempfile::tempdir().unwrap();
    let writer = writer_for(&fake);
    assert_eq!(writer.probe().await.state, ChatWriterState::Available);
    let view = writer
        .take_over(false)
        .await
        .expect("the fake attach grants");
    assert_eq!(view.state, ChatWriterState::Held);
    assert!(!view.can_take_over);
    assert!(view.can_release);
    assert_eq!(fake.attach_count(), 1);
    assert!(
        wait_until(|| fake.stdin_open(), Duration::from_secs(1)).await,
        "the child must hold stdin open"
    );
    let calls = fake.attach_calls();
    assert_eq!(calls.len(), 1);
    assert!(
        !calls[0].iter().any(|arg| arg == "--takeover"),
        "a free attach must not pass --takeover: {calls:?}"
    );
    assert!(
        !calls[0].iter().any(|arg| arg == "--node"),
        "--node must never be used: {calls:?}"
    );
    assert_eq!(
        calls[0].iter().filter(|arg| *arg == "--holder").count(),
        1,
        "the holder is passed exactly once"
    );
    assert!(
        calls[0]
            .windows(2)
            .any(|pair| pair[0] == "--holder" && pair[1] == "stackspirit"),
        "the holder must be the fixed stackspirit name: {calls:?}"
    );

    let value = safe_view_json(&view);
    assert_safe_writer_view(&value);
    let available = safe_view_json(&writer.probe().await);
    assert_eq!(
        available["state"], "held",
        "a probe must not disturb a held lease"
    );
    assert!(!format!("{writer:?}").contains(&fake.canary));
    assert!(!format!("{view:?}").contains(&fake.canary));
    fake.assert_no_token_leak(app.path());

    let released = writer.release().await;
    assert_eq!(released.state, ChatWriterState::Available);
    assert!(!released.can_release);
    assert!(
        wait_until(|| fake.released(), Duration::from_secs(2)).await,
        "the child must observe stdin EOF and write its release marker"
    );
    assert!(
        wait_until(|| !fake.stdin_open(), Duration::from_secs(2)).await,
        "the child must clear its stdin-open marker"
    );
    fake.assert_no_token_leak(app.path());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_attach_child_environment_has_no_inherited_secrets() {
    let fake = FakeVowp::new();
    let writer = writer_for(&fake);
    writer.probe().await;
    writer.take_over(false).await.unwrap();
    let environment = fake
        .read("attach-env.log")
        .expect("the fake attach records its environment");
    assert!(environment.contains("PATH="));
    assert!(environment.contains("VOWP_AGENT_HOME="));
    for inherited in [
        "CARGO_MANIFEST_DIR=",
        "OPENCODE_AUTH_CONTENT=",
        "OPENAI_API_KEY=",
        "ANTHROPIC_API_KEY=",
        "VOWP_TOKEN=",
    ] {
        assert!(
            !environment.contains(inherited),
            "the attach child inherited a non-allow-listed variable: {inherited}"
        );
    }
    let _ = writer.release().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn take_over_after_confirmation_passes_takeover() {
    let fake = FakeVowp::new();
    let writer = writer_for(&fake);
    writer.probe().await;
    let view = writer.take_over(true).await.unwrap();
    assert_eq!(view.state, ChatWriterState::Held);
    let calls = fake.attach_calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].iter().any(|arg| arg == "--takeover"));
    let _ = writer.release().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_take_over_returns_held_and_spawns_once() {
    let fake = FakeVowp::new();
    let writer = writer_for(&fake);
    writer.probe().await;
    let first = writer.take_over(false).await.unwrap();
    let second = writer.take_over(false).await.unwrap();
    assert_eq!(first.state, ChatWriterState::Held);
    assert_eq!(second.state, ChatWriterState::Held);
    assert_eq!(fake.attach_count(), 1, "a duplicate must not spawn a child");
    let _ = writer.release().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grant_validation_releases_and_rejects_every_bad_shape() {
    let variants = [
        ("wrong-session", ChatWriterError::SessionMismatch),
        ("wrong-native", ChatWriterError::SessionMismatch),
        ("non-loopback", ChatWriterError::GrantRejected),
        ("no-auth", ChatWriterError::GrantRejected),
        ("read-only", ChatWriterError::GrantRejected),
        ("wrong-surface", ChatWriterError::GrantRejected),
        ("other-app", ChatWriterError::HarnessNotWired),
        ("malformed", ChatWriterError::AttachFailed),
    ];
    for (variant, expected) in variants {
        let fake = FakeVowp::new();
        fake.set_grant_variant(variant);
        let app = tempfile::tempdir().unwrap();
        let writer = writer_for(&fake);
        writer.probe().await;
        let error = writer
            .take_over(false)
            .await
            .expect_err("a bad grant must not be accepted");
        assert_eq!(error, expected, "{variant}");
        assert_eq!(
            writer.view().state,
            ChatWriterState::Unavailable,
            "{variant} must not leave a writer state"
        );
        assert!(
            wait_until(
                || fake.released() || fake.pid().is_none(),
                Duration::from_secs(2)
            )
            .await,
            "{variant} must release the attach child"
        );
        let rendered = format!("{} {:?}", error.spirit().message, error.spirit().hint);
        assert!(
            !rendered.contains(&fake.canary),
            "{variant} leaked the token into an error"
        );
        assert!(rendered.contains("chat_") || rendered.contains("harness_"));
        fake.assert_no_token_leak(app.path());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attach_conflict_keeps_the_action_available() {
    let fake = FakeVowp::new();
    fake.set_mode("attach-conflict");
    let writer = writer_for(&fake);
    writer.probe().await;
    let error = writer
        .take_over(false)
        .await
        .expect_err("a conflict is not a grant");
    assert_eq!(error, ChatWriterError::AttachConflict);
    assert_eq!(writer.view().state, ChatWriterState::Available);
    assert!(writer.view().can_take_over);
    assert!(error.spirit().message.contains("chat_takeover_conflict"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attach_node_refusal_is_typed_and_unavailable() {
    let fake = FakeVowp::new();
    fake.set_mode("attach-node");
    let writer = writer_for(&fake);
    writer.probe().await;
    let error = writer
        .take_over(false)
        .await
        .expect_err("a node refusal is not a grant");
    assert_eq!(error, ChatWriterError::AttachRefused);
    let view = writer.view();
    assert_eq!(view.state, ChatWriterState::Unavailable);
    assert_eq!(view.reason, Some(ChatWriterReason::AttachRefused));
    assert!(!view.can_take_over);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attach_usage_exit_is_classified_without_reading_stderr() {
    let fake = FakeVowp::new();
    fake.set_mode("attach-usage");
    let writer = writer_for(&fake);
    writer.probe().await;
    let error = writer
        .take_over(false)
        .await
        .expect_err("a usage error is not a grant");
    assert_eq!(error, ChatWriterError::VowpTooOld);
    let rendered = format!("{} {:?}", error.spirit().message, error.spirit().hint);
    assert!(
        !rendered.contains("holder"),
        "raw stderr must not be rendered: {rendered}"
    );
    assert!(writer.view().state == ChatWriterState::Unavailable);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attach_timeout_is_typed_and_leaves_no_child() {
    let fake = FakeVowp::new();
    fake.set_mode("attach-hang");
    let writer = Arc::new(ChatWriter::new(
        fake.target(),
        fake.config()
            .with_attach_timeout(Duration::from_millis(150)),
    ));
    writer.probe().await;
    let error = writer
        .take_over(false)
        .await
        .expect_err("no grant must time out");
    assert_eq!(error, ChatWriterError::AttachTimeout);
    assert_eq!(writer.view().state, ChatWriterState::Available);
    assert!(error.spirit().message.contains("chat_takeover_failed"));
    assert!(
        wait_until(|| fake.pid().is_none(), Duration::from_secs(2)).await,
        "the hung child must be reaped"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unexpected_child_exit_is_lost_and_not_retried() {
    let fake = FakeVowp::new();
    fake.set_mode("attach-crash");
    let writer = writer_for(&fake);
    writer.probe().await;
    let _ = writer.take_over(false).await;
    assert!(
        wait_until(
            || writer.view().state == ChatWriterState::Lost,
            Duration::from_secs(2)
        )
        .await,
        "an exited attach child must flip the writer to lost"
    );
    assert_eq!(writer.view().reason, Some(ChatWriterReason::LeaseLost));
    assert!(!writer.view().can_release);
    assert_eq!(fake.attach_count(), 1, "the manager must not auto-retry");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_watcher_keeps_a_live_lease_and_flips_a_replaced_one() {
    let fake = FakeVowp::new();
    let writer = writer_for(&fake);
    writer.probe().await;
    writer.take_over(false).await.unwrap();
    writer.start_watcher();
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(writer.view().state, ChatWriterState::Held);

    fake.set_mode("endpoint-other");
    assert!(
        wait_until(
            || writer.view().state == ChatWriterState::Lost,
            Duration::from_secs(2)
        )
        .await,
        "the watcher must notice another owner"
    );
    assert_eq!(writer.view().reason, Some(ChatWriterReason::LeaseLost));
    assert!(
        wait_until(
            || fake.released() || fake.pid().is_none(),
            Duration::from_secs(2)
        )
        .await,
        "a lost lease must reap the attach child"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stale_lease_is_lost_and_requires_a_new_attach() {
    let fake = FakeVowp::new();
    let writer = writer_for(&fake);
    writer.probe().await;
    writer.take_over(false).await.unwrap();
    fake.write("owner-state", "stale");
    let view = writer.check_lease().await;
    assert_eq!(view.state, ChatWriterState::Lost);
    assert_eq!(view.reason, Some(ChatWriterReason::LeaseLost));
    assert!(!view.can_release);
    assert!(
        wait_until(
            || fake.released() || fake.pid().is_none(),
            Duration::from_secs(2)
        )
        .await,
        "a stale lease must reap its child"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_watch_failure_is_retried_and_two_are_terminal() {
    let fake = FakeVowp::new();
    fake.set_mode("watch-once-fail");
    let writer = writer_for(&fake);
    writer.probe().await;
    writer.take_over(false).await.unwrap();
    let view = writer.check_lease().await;
    assert_eq!(view.state, ChatWriterState::Held);
    assert!(fake.watch_count() >= 2, "the transient failure is retried");
    let _ = writer.release().await;

    let fake = FakeVowp::new();
    fake.set_mode("watch-always-fail");
    let writer = writer_for(&fake);
    writer.probe().await;
    writer.take_over(false).await.unwrap();
    let view = writer.check_lease().await;
    assert_eq!(view.state, ChatWriterState::Lost);
    assert_eq!(view.reason, Some(ChatWriterReason::LeaseLost));
    assert!(
        wait_until(
            || fake.released() || fake.pid().is_none(),
            Duration::from_secs(2)
        )
        .await
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_release_then_new_takeover_gets_a_new_surface_and_token() {
    let fake = FakeVowp::new();
    let app = tempfile::tempdir().unwrap();
    let writer = writer_for(&fake);
    writer.probe().await;
    writer.take_over(false).await.unwrap();
    let first = writer.release().await;
    assert_eq!(first.state, ChatWriterState::Available);
    writer.take_over(false).await.unwrap();
    let ids = fake.surface_ids();
    assert_eq!(ids.len(), 2, "each attach must use one new surface");
    assert_ne!(ids[0], ids[1], "a reissue must not reuse the old surface");
    assert_eq!(fake.attach_count(), 2);
    fake.assert_no_token_leak(app.path());
    let _ = writer.release().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_writer_releases_the_child_without_blocking() {
    let fake = FakeVowp::new();
    let writer = writer_for(&fake);
    writer.probe().await;
    writer.take_over(false).await.unwrap();
    assert!(
        wait_until(|| fake.pid().is_some(), Duration::from_secs(1)).await,
        "the attach pid is recorded"
    );
    let pid = fake.pid().expect("the attach pid is recorded");
    drop(writer);
    let released = wait_until(|| fake.released(), Duration::from_secs(8)).await;
    assert!(
        released,
        "drop must close stdin and let the child release (stdin_open={}, pid={:?}, pid_alive={}, hold_start={:?}, hold_eof={:?})",
        fake.stdin_open(),
        fake.pid(),
        fake.pid()
            .is_some_and(|pid| Path::new(&format!("/proc/{pid}")).exists()),
        std::fs::read_to_string(fake.state.join("hold-start-at")).ok(),
        std::fs::read_to_string(fake.state.join("hold-eof-at")).ok()
    );
    if cfg!(target_os = "linux") {
        assert!(
            wait_until(
                || !Path::new(&format!("/proc/{pid}")).exists(),
                Duration::from_secs(4)
            )
            .await,
            "no attach process may remain"
        );
    }
}

// ---------------------------------------------------------------------------
// Local send gate
// ---------------------------------------------------------------------------

fn model() -> ChatModelView {
    ChatModelView {
        id: "opencode/big-pickle".to_owned(),
        label: "Big Pickle".to_owned(),
        provider: "opencode".to_owned(),
        tier: ChatModelTier::Included,
        context_tokens: Some(128_000),
        variants: None,
    }
}

struct PanicEngine;

#[async_trait]
impl ChatEngine for PanicEngine {
    async fn ensure_ready(&self) -> Result<u16, SpiritError> {
        panic!("a global/project writer test must never start the bundled engine");
    }

    fn selected_model(&self) -> Option<ChatModelView> {
        panic!("a global/project writer test must not copy the setup model");
    }
}

struct SetupEngine {
    port: u16,
}

#[async_trait]
impl ChatEngine for SetupEngine {
    async fn ensure_ready(&self) -> Result<u16, SpiritError> {
        Ok(self.port)
    }

    fn selected_model(&self) -> Option<ChatModelView> {
        Some(model())
    }
}

fn global_input() -> ChatStartInput {
    ChatStartInput {
        session: ChatSessionId::Global,
        project: None,
        location: ChatSessionLocation::Local,
        resume: None,
    }
}

fn setup_input() -> ChatStartInput {
    ChatStartInput {
        session: ChatSessionId::Setup,
        project: None,
        location: ChatSessionLocation::Local,
        resume: None,
    }
}

#[derive(Clone)]
struct RequestRecord {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

struct ServerState {
    expected_auth: Option<String>,
    sessions: HashMap<String, Vec<Value>>,
    requests: Vec<RequestRecord>,
    next_session: usize,
    assistant_text: String,
}

struct FakeEngine {
    port: u16,
    state: Arc<Mutex<ServerState>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FakeEngine {
    fn new(expected_auth: Option<String>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind fake engine");
        listener
            .set_nonblocking(true)
            .expect("nonblocking fake engine");
        let port = listener.local_addr().expect("fake engine addr").port();
        let mut sessions = HashMap::new();
        sessions.insert(
            NATIVE.to_owned(),
            vec![json!({
                "info": {"id": "msg_welcome", "role": "assistant", "sessionID": NATIVE,
                         "time": {"created": 1000}},
                "parts": [{"type": "text", "text": "Welcome back"}],
            })],
        );
        let state = Arc::new(Mutex::new(ServerState {
            expected_auth,
            sessions,
            requests: Vec::new(),
            next_session: 1,
            assistant_text: "Hello world".to_owned(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_state = Arc::clone(&state);
        let thread_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || serve(listener, thread_state, thread_stop));
        Self {
            port,
            state,
            stop,
            thread: Some(handle),
        }
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn requests(&self) -> Vec<RequestRecord> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .requests
            .clone()
    }

    fn created_sessions(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .next_session
            .saturating_sub(1)
    }
}

impl Drop for FakeEngine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn serve(listener: TcpListener, state: Arc<Mutex<ServerState>>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let state = Arc::clone(&state);
                thread::spawn(move || handle_connection(stream, state));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(_) => return,
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<RequestRecord> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        if let Some(position) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        if raw.len() > 64 * 1024 {
            return None;
        }
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        raw.extend_from_slice(&buffer[..read]);
    };
    let head = String::from_utf8_lossy(&raw[..header_end]);
    let mut lines = head.lines();
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_owned();
    let path = parts.next()?.to_owned();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = raw[header_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    Some(RequestRecord {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<ServerState>>) {
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let path = request.path.split('?').next().unwrap_or(&request.path);
    let auth_ok = {
        let state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .expected_auth
            .as_ref()
            .is_none_or(|expected| request.headers.get("authorization") == Some(expected))
    };
    if !auth_ok {
        write_json(&mut stream, 401, &json!({"error": "auth"}));
        return;
    }
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .requests
        .push(request.clone());
    match (request.method.as_str(), path) {
        ("GET", "/global/health") => {
            write_json(
                &mut stream,
                200,
                &json!({"healthy": true, "version": "1.19.5"}),
            );
        }
        ("GET", "/session") => {
            write_json(&mut stream, 200, &json!({"data": []}));
        }
        ("GET", "/api/session") => {
            write_json(&mut stream, 200, &json!({"data": []}));
        }
        ("POST", "/session") => {
            let id = {
                let mut state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let id = format!("ses_created_{}", state.next_session);
                state.next_session = state.next_session.saturating_add(1);
                state.sessions.insert(id.clone(), Vec::new());
                id
            };
            write_json(&mut stream, 200, &json!({"id": id, "title": "Chat"}));
        }
        ("GET", "/event") => {
            write_json(&mut stream, 404, &json!({}));
        }
        _ => handle_session_route(&mut stream, &state, &request.method, path, &request.body),
    }
}

fn handle_session_route(
    stream: &mut TcpStream,
    state: &Arc<Mutex<ServerState>>,
    method: &str,
    path: &str,
    body: &str,
) {
    let Some(rest) = path.strip_prefix("/session/") else {
        write_json(stream, 404, &json!({}));
        return;
    };
    if let Some(id) = rest.strip_suffix("/message") {
        if method == "POST" {
            let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
            let Some(user_id) = parsed.get("messageID").and_then(Value::as_str) else {
                write_json(
                    stream,
                    400,
                    &json!({"name": "BadRequest", "data": {"message": "missing messageID"}}),
                );
                return;
            };
            let user_id = user_id.to_owned();
            let text = parsed
                .get("parts")
                .and_then(Value::as_array)
                .and_then(|parts| {
                    parts
                        .iter()
                        .find(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                })
                .and_then(|part| part.get("text"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let (exists, assistant_text) = {
                let mut state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let exists = state.sessions.contains_key(id);
                if exists {
                    if let Some(log) = state.sessions.get_mut(id) {
                        log.push(json!({
                            "info": {"id": user_id, "role": "user", "sessionID": id,
                                     "time": {"created": now_ms()}},
                            "parts": [{"type": "text", "text": text}],
                        }));
                    }
                }
                (exists, state.assistant_text.clone())
            };
            if !exists {
                write_json(stream, 404, &json!({}));
                return;
            }
            let assistant = json!({
                "info": {"id": "msg_assistant", "role": "assistant", "sessionID": id,
                         "time": {"created": now_ms()}},
                "parts": [{"type": "text", "text": assistant_text}],
            });
            if let Some(log) = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .sessions
                .get_mut(id)
            {
                log.push(assistant.clone());
            }
            write_json(stream, 200, &assistant);
            return;
        }
        let messages = {
            let state = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.sessions.get(id).cloned()
        };
        match messages {
            Some(messages) => write_json(stream, 200, &Value::Array(messages)),
            None => write_json(stream, 404, &json!({})),
        }
        return;
    }
    if method != "GET" {
        write_json(stream, 405, &json!({}));
        return;
    }
    let exists = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .sessions
        .contains_key(rest);
    if exists {
        write_json(stream, 200, &json!({"id": rest, "title": "Chat"}));
    } else {
        write_json(stream, 404, &json!({}));
    }
}

fn write_json(stream: &mut TcpStream, status: u16, value: &Value) {
    let body = serde_json::to_vec(value).unwrap_or_default();
    let _ = write!(
        stream,
        "HTTP/1.1 {status} Status\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
}

fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = u32::from(chunk.get(1).copied().unwrap_or(0));
        let third = u32::from(chunk.get(2).copied().unwrap_or(0));
        let value = (first << 16) | (second << 8) | third;
        output.push(TABLE[((value >> 18) & 63) as usize] as char);
        output.push(TABLE[((value >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn expected_auth() -> String {
    format!(
        "Basic {}",
        base64(format!("opencode:{ENGINE_CREDENTIAL}").as_bytes())
    )
}

fn provider_with_writer(
    fake: &FakeVowp,
    engine: Arc<dyn ChatEngine>,
    workspace: &Path,
) -> LocalChatProvider {
    LocalChatProvider::new(engine, workspace.to_path_buf())
        .with_endpoint_resolver(Arc::new(fake.resolver()))
        .with_chat_writer(fake.config())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_only_orchestrator_send_is_chat_read_only() {
    let fake = FakeVowp::new();
    let engine = FakeEngine::new(Some(expected_auth()));
    fake.set_url(&format!("http://127.0.0.1:{}", engine.port()));
    let workspace = tempfile::tempdir().unwrap();
    let provider = provider_with_writer(&fake, Arc::new(PanicEngine), workspace.path());

    let view = provider.start(&global_input()).await.unwrap();
    assert_eq!(view.state, ChatSessionState::Ready);
    assert_eq!(view.location, ChatSessionLocation::Local);
    assert_eq!(view.access, ChatAccess::ReadOnly);
    let writer = view.writer.expect("the writer view is present");
    assert_eq!(writer.state, ChatWriterState::Available);
    assert!(writer.can_take_over);
    let value = safe_view_json(&view);
    for forbidden in [
        "\"surface_id\"",
        "\"native_session_id\"",
        "\"url\"",
        "\"port\"",
        "\"token\"",
        "http://",
        "127.0.0.1",
    ] {
        assert!(
            !value.to_string().contains(forbidden),
            "session view leaked {forbidden}"
        );
    }

    let error = provider
        .send(&ChatSessionId::Global, "Hello?")
        .await
        .expect_err("a read-only orchestrator must not send");
    assert_eq!(error.code, SpiritErrorCode::Refused);
    assert!(error.message.contains("chat_read_only"));
    assert!(
        !engine
            .requests()
            .iter()
            .any(|request| request.method == "POST" && request.path.ends_with("/message")),
        "no turn may reach the engine before this app holds the lease"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_held_orchestrator_sends_with_the_grant_endpoint_then_releases() {
    let fake = FakeVowp::new();
    let engine = FakeEngine::new(Some(expected_auth()));
    fake.set_url(&format!("http://127.0.0.1:{}", engine.port()));
    let workspace = tempfile::tempdir().unwrap();
    let app = tempfile::tempdir().unwrap();
    let provider = provider_with_writer(&fake, Arc::new(PanicEngine), workspace.path());

    let view = provider.start(&global_input()).await.unwrap();
    assert_eq!(view.access, ChatAccess::ReadOnly);
    let held = provider
        .take_over(&ChatSessionId::Global, false)
        .await
        .expect("the fake attach grants");
    assert_eq!(held.access, ChatAccess::Owner);
    assert_eq!(
        held.writer.expect("the writer view").state,
        ChatWriterState::Held
    );
    assert_eq!(fake.attach_count(), 1);

    let receipt = provider
        .send(&ChatSessionId::Global, "Hello?")
        .await
        .expect("a held orchestrator sends");
    assert!(receipt.accepted);
    assert!(
        wait_until(
            || engine
                .requests()
                .iter()
                .any(|request| request.method == "POST" && request.path.ends_with("/message")),
            Duration::from_secs(2)
        )
        .await,
        "the turn must reach the grant endpoint"
    );
    let expected = expected_auth();
    let sent = engine
        .requests()
        .into_iter()
        .find(|request| request.method == "POST" && request.path.ends_with("/message"))
        .expect("the turn must reach the grant endpoint");
    assert_eq!(
        sent.headers.get("authorization").map(String::as_str),
        Some(expected.as_str())
    );
    assert!(
        sent.body.contains("Hello?"),
        "the user text must be sent through the chat path"
    );
    fake.assert_no_token_leak(app.path());

    let released = provider
        .release_writer(&ChatSessionId::Global)
        .await
        .expect("release is idempotent");
    assert_eq!(released.access, ChatAccess::ReadOnly);
    assert_eq!(
        released.writer.expect("the writer view").state,
        ChatWriterState::Available
    );
    assert!(
        wait_until(|| fake.released(), Duration::from_secs(2)).await,
        "release must close stdin and let the fake child exit"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_pre_send_watch_rejects_a_lease_taken_by_another_surface() {
    let fake = FakeVowp::new();
    let engine = FakeEngine::new(Some(expected_auth()));
    fake.set_url(&format!("http://127.0.0.1:{}", engine.port()));
    let workspace = tempfile::tempdir().unwrap();
    let provider = provider_with_writer(&fake, Arc::new(PanicEngine), workspace.path());

    provider.start(&global_input()).await.unwrap();
    provider
        .take_over(&ChatSessionId::Global, false)
        .await
        .unwrap();
    fake.set_mode("endpoint-other");
    let error = provider
        .send(&ChatSessionId::Global, "Hello?")
        .await
        .expect_err("a replaced lease must reject the turn");
    assert_eq!(error.code, SpiritErrorCode::Refused);
    assert!(error.message.contains("chat_read_only"));
    let writer = provider
        .writer_view(&ChatSessionId::Global)
        .await
        .expect("the writer view");
    assert_eq!(writer.state, ChatWriterState::Lost);
    assert_eq!(writer.reason, Some(ChatWriterReason::LeaseLost));
    assert!(!writer.can_release);
    assert!(
        wait_until(
            || fake.released() || fake.pid().is_none(),
            Duration::from_secs(2)
        )
        .await
    );
    let refreshed = provider.attach(&ChatSessionId::Global).await.unwrap();
    assert_eq!(refreshed.access, ChatAccess::ReadOnly);
    assert_eq!(
        refreshed
            .ownership
            .and_then(|owner| owner.holder)
            .as_deref(),
        Some("other-writer"),
        "a reattach must name the surface that replaced the lease"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn setup_takeover_is_harness_not_wired_and_spawns_no_child() {
    let fake = FakeVowp::new();
    let engine = FakeEngine::new(None);
    let workspace = tempfile::tempdir().unwrap();
    let provider = LocalChatProvider::new(
        Arc::new(SetupEngine {
            port: engine.port(),
        }),
        workspace.path().to_path_buf(),
    )
    .with_chat_writer(fake.config());

    let view = provider.start(&setup_input()).await.unwrap();
    assert_eq!(view.access, ChatAccess::Owner);
    assert!(
        view.writer.is_none(),
        "setup is app-owned and has no writer"
    );
    let receipt = provider
        .send(&ChatSessionId::Setup, "Hello?")
        .await
        .unwrap();
    assert!(receipt.accepted);
    assert!(
        wait_until(
            || engine
                .requests()
                .iter()
                .any(|request| request.method == "POST" && request.path.ends_with("/message")),
            Duration::from_secs(2)
        )
        .await,
        "the setup chat must keep its app-owned send path"
    );
    let error = provider
        .take_over(&ChatSessionId::Setup, false)
        .await
        .expect_err("setup is not a VOWP writer session");
    assert!(error.message.contains("harness_not_wired"));
    assert_eq!(fake.attach_count(), 0);
    assert_eq!(engine.created_sessions(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_engine_backed_project_keeps_its_send_path_and_rejects_takeover() {
    let fake = FakeVowp::new();
    fake.set_mode("not-home");
    let engine = FakeEngine::new(None);
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspaces/setup");
    let provider = provider_with_writer(
        &fake,
        Arc::new(SetupEngine {
            port: engine.port(),
        }),
        &workspace,
    );

    let view = provider
        .provision_orchestrator("stackspirit")
        .await
        .expect("the bundled-engine fallback opens");
    let session = ChatSessionId::Project("stackspirit".to_owned());
    assert_eq!(view.session, session);
    assert_eq!(view.access, ChatAccess::Owner);
    assert!(view.writer.is_none(), "the fallback is app-owned");
    let receipt = provider.send(&session, "Hello?").await.unwrap();
    assert!(receipt.accepted);
    assert!(
        wait_until(
            || engine
                .requests()
                .iter()
                .any(|request| request.method == "POST" && request.path.ends_with("/message")),
            Duration::from_secs(2)
        )
        .await,
        "an engine-backed project keeps its send path"
    );
    let error = provider
        .take_over(&session, false)
        .await
        .expect_err("an engine-backed project has no VOWP lease");
    assert!(error.message.contains("harness_not_wired"));
    assert_eq!(fake.attach_count(), 0);
}
