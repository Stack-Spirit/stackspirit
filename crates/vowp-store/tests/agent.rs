//! v0.4 local-node enrollment records: migration, credentials, codes, path maps and cards.
//! Spec: `docs/design/v0.4/local-orchestrator.md` §4.1, §4.2, §6.1, §6.2 and §11.4.

use std::time::Duration;

use rusqlite::{Connection, params};
use tempfile::TempDir;
use vowp_protocol::{EventId, HarnessKind, NodeName, NodePresence, NodeStatus, ProjectName, SessionId, Timestamp};
use vowp_store::{EnrollmentCode, EnrollmentKind, NodeCredential, NodePath, NodePaths, Store, StoreError, migrate_v02};

const T0: &str = "2026-01-01T00:00:00.000Z";
const T1: &str = "2026-01-01T00:10:00.000Z";
const SESSION_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const EVENT_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

fn ts(text: &str) -> Timestamp {
    Timestamp::parse(text).expect("timestamp")
}

fn node(name: &str) -> NodeName {
    NodeName::parse(name).expect("node")
}

fn project(name: &str) -> ProjectName {
    ProjectName::parse(name).expect("project")
}

/// Builds a real v2 global database: the current schema with the v3 additions removed.
fn v2_home() -> (TempDir, NodePaths) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let paths = NodePaths::new(tmp.path());
    paths.ensure().expect("dirs");
    let conn = Connection::open(paths.db_path()).expect("v2 db");
    conn.pragma_update(None, "journal_mode", "WAL").expect("wal");
    conn.execute_batch(include_str!("../src/schema/global.sql"))
        .expect("v3 base schema");
    conn.execute_batch(
        "DROP TABLE node_credentials;
         DROP TABLE enrollment_codes;
         DROP TABLE node_paths;
         CREATE TABLE nodes_v2 (
           name         TEXT PRIMARY KEY,
           ssh_target   TEXT,
           is_local     INTEGER NOT NULL DEFAULT 0 CHECK (is_local IN (0,1)),
           capabilities TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(capabilities)),
           last_seen_at TEXT,
           created_at   TEXT NOT NULL,
           CHECK ((is_local = 1 AND ssh_target IS NULL) OR is_local = 0)
         ) STRICT;
         INSERT INTO nodes_v2 (name, ssh_target, is_local, capabilities, last_seen_at, created_at)
           SELECT name, ssh_target, is_local, capabilities, last_seen_at, created_at FROM nodes;
         DROP TABLE nodes;
         ALTER TABLE nodes_v2 RENAME TO nodes;
         CREATE UNIQUE INDEX nodes_one_local ON nodes(is_local) WHERE is_local = 1;
         PRAGMA user_version = 2;",
    )
    .expect("v2 schema");
    conn.execute(
        "INSERT INTO kv (key, value, updated_at) VALUES ('node_id', 'preserved-id', ?1)",
        [T0],
    )
    .expect("kv");
    conn.execute(
        "INSERT INTO nodes (name, ssh_target, is_local, capabilities, last_seen_at, created_at)
         VALUES ('local', NULL, 1, '{\"os\":\"linux\"}', ?1, ?1),
                ('mac-studio', 'owner@mac', 0, '{}', NULL, ?1)",
        [T0],
    )
    .expect("nodes");
    conn.execute(
        "INSERT INTO projects (name, path, remote_url, default_branch, config_json, state, weight,
                               paused_reason, archived_at, created_at, updated_at)
         VALUES ('alpha', '/tmp/alpha', NULL, 'main', '{}', 'active', 1, '', NULL, ?1, ?1)",
        [T0],
    )
    .expect("project");
    conn.execute(
        "INSERT INTO sessions (id, project, job_id, role, title, parent_session_id, node, cwd, state,
                               executor, last_seq, last_capsule_id, log_bytes, compacted_at, created_at, updated_at)
         VALUES (?1, 'alpha', NULL, 'user', 'kept', NULL, 'local', '/tmp', 'idle', NULL, 0, NULL, 0, NULL, ?2, ?2)",
        params![SESSION_ID, T0],
    )
    .expect("session");
    conn.execute(
        "INSERT INTO native_events (session_id, harness, native_id, event_id, at)
         VALUES (?1, 'claude-code', 'native-1', ?2, ?3)",
        params![SESSION_ID, EVENT_ID, T0],
    )
    .expect("native event");
    drop(conn);
    (tmp, paths)
}

#[test]
fn migration_v2_to_v3_preserves_nodes_projects_and_sessions() {
    let (_tmp, paths) = v2_home();
    let source_tables: Vec<String> = {
        let source = Connection::open(paths.db_path()).expect("source db");
        let mut statement = source
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .expect("source tables");
        statement
            .query_map([], |row| row.get(0))
            .expect("source table rows")
            .map(|row| row.expect("source table name"))
            .collect()
    };
    let report = migrate_v02(&paths).expect("migrate");
    assert_eq!(report.from_version, 2);
    assert_eq!(report.to_version, 3);
    assert!(report.verified);
    assert!(std::path::Path::new(&report.backup).is_file(), "backup exists");
    for table in &source_tables {
        assert!(
            report.tables.contains_key(&format!("global:{table}")),
            "the migration did not copy v2 table {table}"
        );
    }
    assert_eq!(report.tables["global:nodes"].before, 2);
    assert_eq!(report.tables["global:nodes"].after, 2);
    assert_eq!(report.tables["global:projects"].before, 1);
    assert_eq!(report.tables["global:sessions"].before, 1);
    assert_eq!(report.tables["global:native_events"].before, 1);
    assert_eq!(report.tables["global:native_events"].after, 1);

    let backup = Connection::open(&report.backup).expect("backup db");
    let backup_version: i64 = backup
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("backup version");
    assert_eq!(backup_version, 2, "the backup is the untouched v2 file");
    let backup_events: i64 = backup
        .query_row("SELECT COUNT(*) FROM native_events", [], |row| row.get(0))
        .expect("backup native events");
    assert_eq!(backup_events, 1, "the backup keeps the v2 row");
    drop(backup);

    let store = Store::open(&paths).expect("open migrated home");
    assert_eq!(store.global_schema_version().expect("version"), 3);
    assert_eq!(store.list_nodes().expect("nodes").len(), 2);
    assert_eq!(store.kv_get("node_id").expect("kv"), Some("preserved-id".to_owned()));
    let projects = store.list_projects().expect("projects");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, project("alpha"));
    assert_eq!(store.list_sessions(&Default::default()).expect("sessions").len(), 1);
    assert_eq!(
        store
            .native_event(
                &SessionId::parse(SESSION_ID).expect("session id"),
                HarnessKind::ClaudeCode,
                "native-1",
            )
            .expect("native event"),
        Some(EventId::parse(EVENT_ID).expect("event id"))
    );
    assert_eq!(store.local_node().expect("local").expect("present").name, node("local"));
    assert_eq!(
        store
            .require_node(&node("mac-studio"))
            .expect("remote")
            .ssh_target
            .as_deref(),
        Some("owner@mac")
    );
}

#[test]
fn local_node_is_home_after_migration() {
    let (_tmp, paths) = v2_home();
    migrate_v02(&paths).expect("migrate");
    let store = Store::open_global(&paths).expect("store");
    let conn = Connection::open(paths.db_path()).expect("inspect");
    for (name, expected) in [("local", "home"), ("mac-studio", "ssh")] {
        let kind: String = conn
            .query_row("SELECT kind FROM nodes WHERE name = ?1", [name], |row| row.get(0))
            .expect("kind");
        assert_eq!(kind, expected, "kind for {name}");
    }
    assert_eq!(store.local_node().expect("local").expect("present").name, node("local"));
    assert_eq!(
        store.require_node(&node("mac-studio")).expect("remote").name,
        node("mac-studio")
    );
}

#[test]
fn enrollment_code_is_single_use() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let paths = NodePaths::new(tmp.path());
    let store = Store::open(&paths).expect("store");
    store
        .insert_pending_node(&node("mac-studio"), &[], ts(T0))
        .expect("pending node");
    let code = EnrollmentCode {
        code_hash: "abc123".to_owned(),
        node: node("mac-studio"),
        kind: EnrollmentKind::Node,
        surface_id: None,
        expires_at: ts(T1),
        used_at: None,
        created_by: "owner".to_owned(),
        created_at: ts(T0),
    };
    store.insert_enrollment_code(&code).expect("code");

    let consumed = store
        .consume_enrollment_code("abc123", ts(T0))
        .expect("consume")
        .expect("first use wins");
    assert_eq!(consumed.code_hash, "abc123");
    assert_eq!(consumed.used_at, Some(ts(T0)));
    assert!(
        store
            .consume_enrollment_code("abc123", ts(T0))
            .expect("second consume")
            .is_none(),
        "a code is consumed exactly once"
    );

    // Two callers racing on separate connections must still produce one winner.
    let code = EnrollmentCode {
        code_hash: "race456".to_owned(),
        node: node("mac-studio"),
        kind: EnrollmentKind::Node,
        surface_id: None,
        expires_at: ts(T1),
        used_at: None,
        created_by: "owner".to_owned(),
        created_at: ts(T0),
    };
    store.insert_enrollment_code(&code).expect("race code");
    drop(store);
    let winners = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            let store = Store::open_global(&paths).expect("first store");
            store
                .consume_enrollment_code("race456", ts(T0))
                .expect("first consume")
                .is_some()
        });
        let second = scope.spawn(|| {
            let store = Store::open_global(&paths).expect("second store");
            store
                .consume_enrollment_code("race456", ts(T0))
                .expect("second consume")
                .is_some()
        });
        u8::from(first.join().expect("first thread")) + u8::from(second.join().expect("second thread"))
    });
    assert_eq!(winners, 1, "exactly one racing caller redeems the code");
}

#[test]
fn enrollment_code_expires() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let paths = NodePaths::new(tmp.path());
    let store = Store::open(&paths).expect("store");
    store
        .insert_pending_node(&node("mac-studio"), &[], ts(T0))
        .expect("pending node");
    let expired = EnrollmentCode {
        code_hash: "expired".to_owned(),
        node: node("mac-studio"),
        kind: EnrollmentKind::Node,
        surface_id: None,
        expires_at: ts(T1),
        used_at: None,
        created_by: "owner".to_owned(),
        created_at: ts(T0),
    };
    store.insert_enrollment_code(&expired).expect("expired code");
    assert!(
        store
            .consume_enrollment_code("expired", ts(T1))
            .expect("at expiry")
            .is_none(),
        "a code is invalid at its expiry instant"
    );
    assert_eq!(
        store.purge_expired_enrollment_codes(ts(T1)).expect("purge"),
        1,
        "the expired row is purged"
    );
    assert!(
        store
            .consume_enrollment_code("expired", ts(T0))
            .expect("after purge")
            .is_none()
    );
}

#[test]
fn node_credential_round_trips_and_revokes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let paths = NodePaths::new(tmp.path());
    let store = Store::open(&paths).expect("store");
    store
        .insert_pending_node(&node("mac-studio"), &[], ts(T0))
        .expect("pending node");
    let credential = NodeCredential {
        node: node("mac-studio"),
        key_id: "sha256:key-one".to_owned(),
        public_key: "aa".repeat(32),
        created_at: ts(T0),
        last_used_at: None,
        revoked_at: None,
    };
    store.upsert_node_credential(&credential).expect("insert");
    assert_eq!(
        store
            .node_credential(&node("mac-studio"), "sha256:key-one")
            .expect("read"),
        Some(credential.clone())
    );

    store
        .upsert_node_credential(&NodeCredential {
            public_key: "bb".repeat(32),
            last_used_at: Some(ts(T1)),
            ..credential.clone()
        })
        .expect("update");
    let updated = store
        .node_credential(&node("mac-studio"), "sha256:key-one")
        .expect("read updated")
        .expect("present");
    assert_eq!(updated.public_key, "bb".repeat(32));
    assert_eq!(updated.created_at, ts(T0), "created_at is preserved");
    assert_eq!(updated.last_used_at, Some(ts(T1)));

    assert_eq!(
        store
            .revoke_node_credentials(&node("mac-studio"), ts(T1))
            .expect("revoke"),
        1
    );
    let revoked = store
        .node_credential(&node("mac-studio"), "sha256:key-one")
        .expect("read revoked")
        .expect("present");
    assert_eq!(revoked.revoked_at, Some(ts(T1)));
    assert_eq!(
        store
            .revoke_node_credentials(&node("mac-studio"), ts(T1))
            .expect("revoke again"),
        0,
        "revoking twice is idempotent"
    );
    assert!(
        store
            .node_credential(&node("mac-studio"), "missing")
            .expect("missing")
            .is_none()
    );
    let error = store
        .upsert_node_credential(&NodeCredential {
            node: node("unknown"),
            ..credential
        })
        .expect_err("unknown node must fail the foreign key");
    assert!(matches!(error, StoreError::InvalidInput(_)), "got {error:?}");
}

#[test]
fn node_path_upsert_is_idempotent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let paths = NodePaths::new(tmp.path());
    let store = Store::open(&paths).expect("store");
    store
        .insert_pending_node(&node("mac-studio"), &[], ts(T0))
        .expect("pending node");
    let path = NodePath {
        node: node("mac-studio"),
        project: project("vowp"),
        local_root: "/Users/owner/src/vowp".to_owned(),
        worktree_root: "/Users/owner/src/vowp/.vowp/worktrees".to_owned(),
        allow: vec!["src".to_owned(), "docs".to_owned()],
        updated_at: ts(T0),
    };
    store.upsert_node_path(&path).expect("insert");
    store
        .upsert_node_path(&NodePath {
            local_root: "/Users/owner/src/vowp-renamed".to_owned(),
            allow: vec!["src".to_owned()],
            updated_at: ts(T1),
            ..path.clone()
        })
        .expect("update");
    let paths_for_node = store.list_node_paths(&node("mac-studio")).expect("paths");
    assert_eq!(paths_for_node.len(), 1, "the primary key makes the upsert idempotent");
    assert_eq!(paths_for_node[0].local_root, "/Users/owner/src/vowp-renamed");
    assert_eq!(paths_for_node[0].allow, vec!["src".to_owned()]);
    assert_eq!(paths_for_node[0].updated_at, ts(T1));
    assert_eq!(paths_for_node[0].worktree_root, path.worktree_root);
}

#[test]
fn node_status_and_presence_persist() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let paths = NodePaths::new(tmp.path());
    let store = Store::open(&paths).expect("store");
    let mac = node("mac-studio");
    store
        .insert_pending_node(&mac, &["darwin-arm64".to_owned()], ts(T0))
        .expect("pending node");
    let pending = store.node_card(&mac, ts(T0)).expect("card").expect("present");
    assert_eq!(pending.status, NodeStatus::Pending);
    assert_eq!(pending.labels, vec!["darwin-arm64".to_owned()]);
    assert_eq!(pending.presence, NodePresence::Offline);
    assert!(pending.capacity.is_none());

    store
        .set_node_status(&mac, NodeStatus::Active, "", ts(T0))
        .expect("activate");
    store
        .touch_node(
            &mac,
            ts(T0),
            Some("0.4.0"),
            Some("vowp-agent/1"),
            &serde_json::json!({
                "capacity": {
                    "cpu_cores": 8,
                    "load1": 0.5,
                    "mem_total_bytes": 17179869184_u64,
                    "mem_free_bytes": 8589934592_u64,
                    "disk_free_bytes": 107374182400_u64,
                    "busy_slots": 1,
                    "max_slots": 4,
                    "on_battery": false,
                    "thermal_state": "nominal"
                }
            }),
        )
        .expect("heartbeat");
    let online = store
        .node_card(&mac, ts(T0).plus(Duration::from_secs(10)))
        .expect("card")
        .expect("present");
    assert_eq!(online.status, NodeStatus::Active);
    assert_eq!(online.presence, NodePresence::Online);
    assert_eq!(online.agent_version.as_deref(), Some("0.4.0"));
    assert_eq!(online.protocol.as_deref(), Some("vowp-agent/1"));
    assert_eq!(online.capacity.expect("capacity").cpu_cores, 8);

    let degraded = store
        .node_card(&mac, ts(T0).plus(Duration::from_secs(60)))
        .expect("card")
        .expect("present");
    assert_eq!(degraded.presence, NodePresence::Degraded);
    let offline = store
        .node_card(&mac, ts(T0).plus(Duration::from_secs(120)))
        .expect("card")
        .expect("present");
    assert_eq!(offline.presence, NodePresence::Offline);
    assert_eq!(store.list_node_cards(ts(T0)).expect("cards").len(), 1);

    store
        .set_node_status(&mac, NodeStatus::Active, "path_missing", ts(T1))
        .expect("record error");
    let with_error = store.node_card(&mac, ts(T1)).expect("card").expect("present");
    assert_eq!(with_error.last_error.as_deref(), Some("path_missing"));
}

#[test]
fn revoked_node_is_not_active() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let paths = NodePaths::new(tmp.path());
    let store = Store::open(&paths).expect("store");
    let mac = node("mac-studio");
    store.insert_pending_node(&mac, &[], ts(T0)).expect("pending node");
    store
        .set_node_status(&mac, NodeStatus::Active, "", ts(T0))
        .expect("activate");
    store
        .touch_node(&mac, ts(T0), None, None, &serde_json::json!({}))
        .expect("touch");
    store
        .set_node_status(&mac, NodeStatus::Revoked, "credential revoked", ts(T1))
        .expect("revoke");
    // A heartbeat after revocation must not make the card look active.
    store
        .touch_node(
            &mac,
            ts(T1).plus(Duration::from_secs(1)),
            None,
            None,
            &serde_json::json!({}),
        )
        .expect("late heartbeat");
    let card = store
        .node_card(&mac, ts(T1).plus(Duration::from_secs(2)))
        .expect("card")
        .expect("present");
    assert_eq!(card.status, NodeStatus::Revoked);
    assert_eq!(card.presence, NodePresence::Revoked);
    assert_eq!(card.last_error.as_deref(), Some("credential revoked"));
    assert_ne!(card.status, NodeStatus::Active);
}
