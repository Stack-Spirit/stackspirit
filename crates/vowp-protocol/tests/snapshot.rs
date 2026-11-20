//! Snapshot documents: the session card's additive `owner` field.
//! Spec: `docs/design/v0.5.1/session-ownership.md` §2.4.

use serde_json::json;

use vowp_protocol::ids::{NodeName, SessionId};
use vowp_protocol::snapshot::SessionSurfaceCard;
use vowp_protocol::surface::{OwnershipMode, OwnershipState, SessionOwner, SurfaceActivity, SurfaceId, SurfaceKind};
use vowp_protocol::time::Timestamp;

fn card_json(id: &SessionId) -> serde_json::Value {
    json!({
        "id": id.to_string(),
        "project": null,
        "job_id": null,
        "role": "orchestrator",
        "title": "main",
        "state": "idle",
        "executor": null,
        "node": "home",
        "active_surface": null,
        "last_event_at": "2023-11-14T22:13:20Z"
    })
}

#[test]
fn session_card_without_owner_still_parses() {
    let id = SessionId::new();
    let card: SessionSurfaceCard = serde_json::from_value(card_json(&id)).expect("a v0.5 card parses");
    assert_eq!(card.owner, None);
    assert_eq!(card.active_surface, None);
}

#[test]
fn session_card_round_trips_an_owner_without_a_token_field() {
    let id = SessionId::new();
    let mut value = card_json(&id);
    let owner = SessionOwner {
        session_id: id.clone(),
        surface_id: Some(SurfaceId::new()),
        kind: SurfaceKind::DesktopUi,
        label: Some("StackSpirit · home".to_owned()),
        holder: "stackspirit-1".to_owned(),
        node: NodeName::parse("home").expect("node"),
        mode: OwnershipMode::Interactive,
        state: OwnershipState::Active,
        activity: SurfaceActivity::Running,
        epoch: 3,
        acquired_at: Timestamp::from_unix_secs(1_700_000_000),
        heartbeat_at: Timestamp::from_unix_secs(1_700_000_004),
        expires_at: None,
    };
    value["owner"] = serde_json::to_value(&owner).expect("owner JSON");
    let text = value.to_string();
    for forbidden in ["token", "password", "hash"] {
        assert!(
            !text.contains(forbidden),
            "owner JSON must not mention {forbidden}: {text}"
        );
    }
    let card: SessionSurfaceCard = serde_json::from_value(value).expect("card with owner parses");
    assert_eq!(card.owner, Some(owner));
}
