//! Tests for the v0.3 loopback API documents in `vowp_protocol::api`.
//! Spec: `docs/design/v0.3/http-api.md`.
//!
//! These pin the wire shape the daemon serves: the health document, the generic page,
//! the concrete page aliases and the `vowp --json` error shape.

use serde_json::{Value, json};
use vowp_protocol::api::{ApiError, ApiErrorBody, EventPage, Health, JobPage, Page, SessionPage};
use vowp_protocol::schema::schema_for_name;
use vowp_protocol::{ErrorKind, NodeName, ProjectName, Timestamp};

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
fn api_documents_are_exported_schemas() {
    for name in [
        "Health",
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
