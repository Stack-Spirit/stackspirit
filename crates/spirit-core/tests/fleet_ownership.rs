//! Session ownership (VOWP 0.5.1 `SessionSurfaceCard.owner`) and the takeover command.
//!
//! Hermetic: golden fixture plus `FakeCommandRunner`; no VOWP home, network or SSH key.

use std::sync::Arc;

use serde_json::{Value, json};
use spirit_core::connection::LatencyStats;
use spirit_core::error::SpiritErrorCode;
use spirit_core::host::profile::HostId;
use spirit_core::ssh::command::CommandOutput;
use spirit_core::ssh::{FakeCommandRunner, SshOptions, SshTarget};
use spirit_core::time::Timestamp;
use spirit_core::view::FleetView;
use spirit_core::vowp::client::SessionTakeoverRequest;
use spirit_core::vowp::{SshVowpClient, VowpClient};
use spirit_core::wire::parse_fleet_status;

const FRESH: &str = include_str!("../fixtures/vowp/fleet-status-fresh.json");
const SESSION: &str = "01JCF5WQ8ZK3M2V7N4P6R8T0AB";

fn status_with_owner(owner: Option<Value>) -> Vec<u8> {
    let mut doc: Value = serde_json::from_str(FRESH).unwrap();
    let session = &mut doc["snapshot"]["sessions"][0];
    match owner {
        Some(owner) => session["owner"] = owner,
        None => {
            session.as_object_mut().unwrap().remove("owner");
        }
    }
    serde_json::to_vec(&doc).unwrap()
}

fn owner_json(label: Value, activity: &str) -> Value {
    json!({
        "session_id": SESSION,
        "surface_id": "01JCF5WQ8ZK3M2V7N4P6R8T0AG",
        "label": label,
        "holder": "opencode",
        "kind": "opencode",
        "node": "macbook",
        "activity": activity,
        "mode": "interactive",
        "state": "active",
        "epoch": 7,
        "acquired_at": "2026-11-14T11:30:00.000Z",
        "heartbeat_at": "2026-11-14T11:59:55.000Z",
        "expires_at": null
    })
}

fn fleet(bytes: &[u8]) -> FleetView {
    let status = parse_fleet_status(bytes).unwrap().value;
    FleetView::from_status(
        HostId("01JCF5WQ8ZK3M2V7N4P6R8T0AB".to_owned()),
        &status,
        None,
        LatencyStats::default(),
        1,
        Timestamp::parse("2026-11-14T12:00:10.000Z").unwrap(),
    )
}

#[test]
fn owner_populated_snapshot_parses_and_reaches_the_view() {
    let bytes = status_with_owner(Some(owner_json(json!("OpenCode · macbook"), "running")));
    let parsed = parse_fleet_status(&bytes).unwrap().value;
    let owner = parsed.snapshot.sessions[0].owner.as_ref().unwrap();
    assert_eq!(owner.kind, "opencode");
    assert_eq!(owner.node.as_str(), "macbook");
    assert_eq!(owner.activity, "running");
    assert_eq!(owner.epoch, 7);

    let view = fleet(&bytes);
    let owner = view.session_owners.first().unwrap();
    assert_eq!(owner.label, "OpenCode · macbook");
    assert_eq!(owner.holder, "opencode");
    assert_eq!(owner.activity, "running");
    assert_eq!(
        owner.surface_id.as_deref(),
        Some("01JCF5WQ8ZK3M2V7N4P6R8T0AG")
    );
}

#[test]
fn a_null_or_blank_label_falls_back_to_the_holder() {
    for label in [Value::Null, json!("  ")] {
        let view = fleet(&status_with_owner(Some(owner_json(label, "idle"))));
        assert_eq!(view.session_owners.first().unwrap().label, "opencode");
    }
}

#[test]
fn owner_defaults_apply_when_optional_fields_are_missing() {
    let owner = json!({
        "session_id": SESSION,
        "kind": "chat",
        "holder": "stackspirit",
        "node": "macbook",
        "acquired_at": "2026-11-14T11:30:00.000Z",
        "heartbeat_at": "2026-11-14T11:59:55.000Z"
    });
    let view = fleet(&status_with_owner(Some(owner)));
    let owner = view.session_owners.first().unwrap();
    assert_eq!(owner.activity, "idle");
    assert_eq!(owner.mode, "interactive");
    assert_eq!(owner.state, "active");
    assert_eq!(owner.epoch, 0);
    assert_eq!(owner.surface_id, None);
    assert_eq!(owner.expires_at, None);
}

#[test]
fn older_snapshot_without_owner_still_parses() {
    let bytes = status_with_owner(None);
    let parsed = parse_fleet_status(&bytes).unwrap().value;
    assert!(parsed.snapshot.sessions[0].owner.is_none());
    // The shipped fixture predates `owner` and must keep working unchanged.
    let view = fleet(FRESH.as_bytes());
    assert!(view.session_owners.is_empty());
}

fn client(responses: Vec<CommandOutput>) -> (Arc<FakeCommandRunner>, SshVowpClient) {
    let fake = Arc::new(FakeCommandRunner::new(responses));
    let target = SshTarget::new("owner", "192.0.2.10", 2222).unwrap();
    let client = SshVowpClient::new(
        fake.clone(),
        target,
        SshOptions::new("/tmp/opencode/known_hosts"),
        None,
    );
    (fake, client)
}

fn request() -> SessionTakeoverRequest {
    SessionTakeoverRequest {
        session: SESSION.to_owned(),
        node: "macbook".to_owned(),
        label: Some("StackSpirit · macbook".to_owned()),
    }
}

fn failure(status: i32, stderr: &str) -> CommandOutput {
    let mut output = CommandOutput::ok("");
    output.status = status;
    output.stderr = stderr.as_bytes().to_vec();
    output
}

#[tokio::test]
async fn takeover_runs_the_documented_command_without_force() {
    let (fake, client) = client(vec![CommandOutput::ok(
        r#"{"previous":"opencode","drained":true}"#,
    )]);
    let result = client.session_takeover(request()).await.unwrap();
    assert_eq!(result.session_id, SESSION);
    assert_eq!(result.previous.as_deref(), Some("opencode"));

    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    let command = calls[0].args.last().cloned().unwrap();
    assert!(command.contains(
        "'vowp' '--json' 'session' 'takeover' '--kind' 'chat' '--holder' 'stackspirit' '--node' 'macbook' '--label'"
    ));
    assert!(command.contains(SESSION));
    assert!(!command.contains("--force"));
    assert!(!command.contains("lease"));
}

#[tokio::test]
async fn takeover_maps_a_missing_command_to_not_supported() {
    for output in [
        failure(2, "error: unrecognized subcommand 'takeover'"),
        failure(127, "vowp: command not found"),
    ] {
        let (_, client) = client(vec![output]);
        let error = client.session_takeover(request()).await.unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::Gate);
        assert!(
            error.message.starts_with("not_supported"),
            "{}",
            error.message
        );
    }
}

#[tokio::test]
async fn takeover_validates_argv_values_before_running() {
    let (fake, client) = client(vec![]);
    for bad in [
        SessionTakeoverRequest {
            session: "x; rm -rf /".to_owned(),
            ..request()
        },
        SessionTakeoverRequest {
            node: "Bad Node".to_owned(),
            ..request()
        },
        SessionTakeoverRequest {
            label: Some("a\nb".to_owned()),
            ..request()
        },
    ] {
        let error = client.session_takeover(bad).await.unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::InvalidInput);
    }
    assert!(fake.calls().is_empty());
}

#[tokio::test]
async fn the_default_client_never_fakes_ownership() {
    use spirit_core::vowp::MockVowpClient;
    let status = parse_fleet_status(FRESH.as_bytes()).unwrap().value;
    let capacity = spirit_core::wire::parse_capacity_report(include_bytes!(
        "../fixtures/vowp/capacity-report-full.json"
    ))
    .unwrap()
    .value;
    let mock = MockVowpClient::new(status, capacity);
    let error = mock.session_takeover(request()).await.unwrap_err();
    assert!(error.message.starts_with("not_supported"));
}
