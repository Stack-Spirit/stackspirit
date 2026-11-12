//! Tests for the v0.3 loopback API documents in `vowp_protocol::api`.
//! Spec: `docs/design/v0.3/http-api.md`.
//!
//! These pin the wire shape the daemon serves: the health document, the generic page,
//! the concrete page aliases and the `vowp --json` error shape.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use vowp_protocol::api::{
    ApiError, ApiErrorBody, Board, BoardLimit, BoardRun, EventPage, Health, JobPage, Page, SessionPage,
};
use vowp_protocol::schema::schema_for_name;
use vowp_protocol::{ErrorKind, JobId, LimitRowKind, NodeName, ProjectName, Timestamp};

/// Fixed test instant.
fn epoch() -> Timestamp {
    Timestamp::parse("2026-01-01T00:00:00.000Z").expect("epoch")
}

#[test]
fn health_roundtrips_with_and_without_a_heartbeat() {
    let health = Health {
        version: "0.2.0".to_owned(),
        node: NodeName::parse("local").expect("node"),
        project: ProjectName::parse("vowp").expect("project"),
        schema_version: 1,
        heartbeat: None,
    };
    let value = serde_json::to_value(&health).expect("health JSON");
    assert_eq!(
        value,
        json!({
            "version": "0.2.0",
            "node": "local",
            "project": "vowp",
            "schema_version": 1,
            "heartbeat": null,
        })
    );
    assert_eq!(
        serde_json::from_value::<Health>(value).expect("health roundtrip"),
        health
    );

    let with_heartbeat = Health {
        heartbeat: Some(epoch()),
        ..health
    };
    let value = serde_json::to_value(&with_heartbeat).expect("health JSON");
    assert_eq!(value["heartbeat"], json!("2026-01-01T00:00:00.000Z"));
    assert_eq!(
        serde_json::from_value::<Health>(value).expect("health roundtrip"),
        with_heartbeat
    );
}

#[test]
fn api_error_matches_the_cli_json_shape() {
    let error = ApiError {
        error: ApiErrorBody {
            kind: ErrorKind::NotFound,
            message: "session 01ARZ3NDEKTSV4RRFFQ69G5FAV not found".to_owned(),
            hint: Some("vowp session ls".to_owned()),
        },
    };
    let value = serde_json::to_value(&error).expect("error JSON");
    assert_eq!(
        value,
        json!({
            "error": {
                "kind": "not_found",
                "message": "session 01ARZ3NDEKTSV4RRFFQ69G5FAV not found",
                "hint": "vowp session ls",
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<ApiError>(value).expect("error roundtrip"),
        error
    );
}

#[test]
fn page_json_and_schema_have_the_documented_fields() {
    let page: Page<String> = Page {
        items: vec!["first".to_owned()],
        next: Some(7),
        total: 1,
    };
    let value = serde_json::to_value(&page).expect("page JSON");
    assert_eq!(value, json!({ "items": ["first"], "next": 7, "total": 1 }));
    assert_eq!(
        serde_json::from_value::<Page<String>>(value).expect("page roundtrip"),
        page
    );

    let schema = schema_for_name("Page").expect("Page is exported");
    for field in ["items", "next", "total"] {
        assert!(
            schema["properties"].get(field).is_some(),
            "Page schema has no {field} property: {schema}"
        );
    }
    let required = schema["required"].as_array().expect("required array");
    assert!(required.contains(&json!("items")), "items must be required");
    assert!(required.contains(&json!("total")), "total must be required");
}

#[test]
fn concrete_page_aliases_serialise_their_item_type() {
    let jobs: JobPage = Page {
        items: Vec::new(),
        next: None,
        total: 0,
    };
    let sessions: SessionPage = Page {
        items: Vec::new(),
        next: None,
        total: 0,
    };
    let events: EventPage = Page {
        items: Vec::new(),
        next: None,
        total: 0,
    };
    for page in [
        serde_json::to_value(jobs).expect("job page"),
        serde_json::to_value(sessions).expect("session page"),
        serde_json::to_value(events).expect("event page"),
    ] {
        assert_eq!(page, json!({ "items": [], "next": null, "total": 0 }));
    }
}

#[test]
fn board_matches_the_cli_json_shape() {
    let board = Board {
        at: epoch(),
        project: ProjectName::parse("vowp").expect("project"),
        jobs: BTreeMap::from([("pending".to_owned(), 2), ("running".to_owned(), 1)]),
        running: vec![BoardRun {
            job: JobId::parse("job-a").expect("job id"),
            executor: Some("claude-code:model-a@medium".to_owned()),
            age_secs: 90,
            last_event_age_secs: Some(30),
        }],
        slots_busy: 1,
        slots_total: 6,
        planner: Some("claude-code:model-a@medium".to_owned()),
        keeper: None,
        limits: vec![BoardLimit {
            provider: "claude".to_owned(),
            kind: LimitRowKind::Quota,
            resets_at: Some(Timestamp::parse("2026-01-01T01:00:00.000Z").expect("reset")),
        }],
        cost_today_usd: Some(1.25),
        inbox_open: 2,
        last_checkpoint: Some(Timestamp::parse("2025-12-31T23:00:00.000Z").expect("checkpoint")),
    };
    let value = serde_json::to_value(&board).expect("board JSON");
    assert_eq!(
        value,
        json!({
            "at": "2026-01-01T00:00:00.000Z",
            "project": "vowp",
            "jobs": { "pending": 2, "running": 1 },
            "running": [{
                "job": "job-a",
                "executor": "claude-code:model-a@medium",
                "age_secs": 90,
                "last_event_age_secs": 30,
            }],
            "slots_busy": 1,
            "slots_total": 6,
            "planner": "claude-code:model-a@medium",
            "keeper": null,
            "limits": [{
                "provider": "claude",
                "kind": "quota",
                "resets_at": "2026-01-01T01:00:00.000Z",
            }],
            "cost_today_usd": 1.25,
            "inbox_open": 2,
            "last_checkpoint": "2025-12-31T23:00:00.000Z",
        })
    );
    assert_eq!(serde_json::from_value::<Board>(value).expect("board roundtrip"), board);
}

#[test]
fn board_keeps_unknown_cost_and_unavailable_limits_null_or_named() {
    let board = Board {
        at: epoch(),
        project: ProjectName::parse("vowp").expect("project"),
        jobs: BTreeMap::new(),
        running: vec![BoardRun {
            job: JobId::parse("job-a").expect("job id"),
            executor: None,
            age_secs: 0,
            last_event_age_secs: None,
        }],
        slots_busy: 0,
        slots_total: 0,
        planner: None,
        keeper: None,
        limits: vec![BoardLimit {
            provider: "local".to_owned(),
            kind: LimitRowKind::Unavailable,
            resets_at: None,
        }],
        cost_today_usd: None,
        inbox_open: 0,
        last_checkpoint: None,
    };
    let value = serde_json::to_value(&board).expect("board JSON");
    assert_eq!(value["cost_today_usd"], Value::Null);
    assert_eq!(value["limits"][0]["kind"], json!("unavailable"));
    assert_eq!(value["limits"][0]["resets_at"], Value::Null);
    assert_eq!(value["running"][0]["executor"], Value::Null);
    assert_eq!(value["running"][0]["last_event_age_secs"], Value::Null);
    assert_eq!(serde_json::from_value::<Board>(value).expect("board roundtrip"), board);
}

#[test]
fn board_schemas_have_the_documented_fields() {
    let board = schema_for_name("Board").expect("Board is exported");
    for field in [
        "at",
        "project",
        "jobs",
        "running",
        "slots_busy",
        "slots_total",
        "planner",
        "keeper",
        "limits",
        "cost_today_usd",
        "inbox_open",
        "last_checkpoint",
    ] {
        assert!(
            board["properties"].get(field).is_some(),
            "Board schema has no {field} property: {board}"
        );
    }
    for field in [
        "at",
        "project",
        "jobs",
        "running",
        "slots_busy",
        "slots_total",
        "limits",
        "inbox_open",
    ] {
        assert!(
            board["required"]
                .as_array()
                .is_some_and(|required| required.contains(&json!(field))),
            "Board schema does not require {field}: {board}"
        );
    }

    let run = schema_for_name("BoardRun").expect("BoardRun is exported");
    for field in ["job", "executor", "age_secs", "last_event_age_secs"] {
        assert!(run["properties"].get(field).is_some(), "BoardRun has no {field}: {run}");
    }

    let limit = schema_for_name("BoardLimit").expect("BoardLimit is exported");
    for field in ["provider", "kind", "resets_at"] {
        assert!(
            limit["properties"].get(field).is_some(),
            "BoardLimit has no {field}: {limit}"
        );
    }
    let mut kinds = Vec::new();
    for one_of in [
        limit["properties"]["kind"]["oneOf"].as_array(),
        limit["$defs"]["LimitRowKind"]["oneOf"].as_array(),
    ]
    .into_iter()
    .flatten()
    {
        kinds.extend(
            one_of
                .iter()
                .filter_map(|variant| variant["const"].as_str())
                .collect::<Vec<_>>(),
        );
    }
    assert!(!kinds.is_empty(), "BoardLimit kind has no variants: {limit}");
    for kind in ["quota", "credits", "rate", "context", "auth", "unavailable"] {
        assert!(kinds.contains(&kind), "BoardLimit kind is missing {kind}: {limit}");
    }
}

#[test]
fn api_documents_are_exported_schemas() {
    for name in [
        "Health",
        "Board",
        "BoardRun",
        "BoardLimit",
        "Page",
        "JobPage",
        "SessionPage",
        "EventPage",
        "ApiError",
        "ApiErrorBody",
    ] {
        let schema: Value = schema_for_name(name).unwrap_or_else(|| panic!("{name} is not exported"));
        assert!(schema.is_object(), "{name} schema must be a JSON object");
    }
}
