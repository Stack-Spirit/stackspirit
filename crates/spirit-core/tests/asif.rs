//! ASIF wire contract, remote-file transfer, client argv/limits and facade behavior.
//!
//! Every test is hermetic: it uses the golden fixtures and `FakeCommandRunner`, never a
//! real VOWP home, a network connection or a real SSH key.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use spirit_core::engine::{EngineSupervisor, EngineSupervisorConfig, FakeEngineRuntime};
use spirit_core::error::SpiritErrorCode;
use spirit_core::host::{HostDraft, HostKeyPin, InMemoryHostStore, NewHostProfile};
use spirit_core::service::SpiritCore;
use spirit_core::ssh::command::{CommandOutput, CommandSpec};
use spirit_core::ssh::{FakeCommandRunner, SshOptions, SshTarget};
use spirit_core::time::Timestamp;
use spirit_core::vowp::{
    AsifExportOptions, AsifImportOptions, AsifLossPolicy, MockVowpClient, SshVowpClient,
    VowpClient, ensure_package_size,
};
use spirit_core::wire::{
    ASIF_ARCHIVE_CAP_BYTES, ASIF_JSON_CAP_BYTES, AsifImportResponse, SessionState, WireWarningCode,
    parse_asif_export_summary, parse_asif_import_response, parse_capacity_report,
    parse_error_document, parse_fleet_status,
};

const EXPORT_FIXTURE: &str = include_str!("../fixtures/vowp/asif-export-summary.json");
const IMPORT_FIXTURE: &str = include_str!("../fixtures/vowp/asif-import-response.json");
const STRICT_REFUSAL: &str = include_str!("../fixtures/vowp/asif-strict-refusal.json");
const IMPORT_CONFLICT: &str = include_str!("../fixtures/vowp/asif-import-conflict.json");
const FRESH: &str = include_str!("../fixtures/vowp/fleet-status-fresh.json");
const CAPACITY: &str = include_str!("../fixtures/vowp/capacity-report-full.json");

const SESSION: &str = "01JCF5WQ8ZK3M2V7N4P6R8T0AB";
const IMPORTED: &str = "01JCF5WQ8ZK3M2V7N4P6R8T0AF";
const TEMP: &str = "/tmp/stackspirit-asif.ABC123";
const EXPORT_BYTES: usize = 18342;

fn target() -> SshTarget {
    SshTarget::new("owner", "192.0.2.10", 2222).unwrap()
}

fn ssh_options() -> SshOptions {
    SshOptions::new("/tmp/opencode/known_hosts")
}

fn package() -> Vec<u8> {
    vec![b'P'; EXPORT_BYTES]
}

fn local_package() -> Vec<u8> {
    b"PK\x03\x04fake-asif-package-bytes".to_vec()
}

fn client_with(responses: Vec<CommandOutput>) -> (Arc<FakeCommandRunner>, SshVowpClient) {
    let fake = Arc::new(FakeCommandRunner::new(responses));
    let client = SshVowpClient::new(fake.clone(), target(), ssh_options(), None);
    (fake, client)
}

fn remote(call: &CommandSpec) -> String {
    call.args.last().cloned().unwrap_or_default()
}

fn ok_export_responses() -> Vec<CommandOutput> {
    vec![
        CommandOutput::ok(format!("{TEMP}\n")),
        CommandOutput::ok(""),
        CommandOutput::ok(EXPORT_FIXTURE),
        CommandOutput::ok(format!("{EXPORT_BYTES}\n")),
        CommandOutput::ok(package()),
        CommandOutput::ok(""),
    ]
}

fn failure(exit: i32, stdout: &[u8], stderr: &str) -> CommandOutput {
    let mut output = CommandOutput::ok(stdout.to_vec());
    output.status = exit;
    output.stderr = stderr.as_bytes().to_vec();
    output
}

fn assert_no_remote_path(error: &spirit_core::SpiritError) {
    let combined = format!(
        "{} {}",
        error.message,
        error.hint.clone().unwrap_or_default()
    );
    assert!(
        !combined.contains(TEMP),
        "remote temp path leaked: {combined}"
    );
    assert!(
        !combined.contains("/tmp/stackspirit-asif"),
        "remote temp path leaked: {combined}"
    );
}

fn assert_last_call_removed(fake: &FakeCommandRunner) {
    let calls = fake.calls();
    let last = calls.last().expect("at least one call");
    let command = remote(last);
    assert!(
        command.contains(&format!("'rm' '-rf' '--' '{TEMP}'")),
        "cleanup was not the last call: {command}"
    );
}

// ---------------------------------------------------------------------------
// Wire contract
// ---------------------------------------------------------------------------

#[test]
fn asif_export_wire_fixture_covers_every_ui_field() {
    let parsed = parse_asif_export_summary(EXPORT_FIXTURE.as_bytes()).unwrap();
    assert!(
        parsed.is_clean(),
        "unexpected warnings: {:?}",
        parsed.warnings
    );
    let summary = parsed.value;
    assert_eq!(summary.format, "asif");
    assert_eq!(summary.session.as_str(), SESSION);
    assert_eq!(summary.bytes, EXPORT_BYTES as u64);
    assert!(summary.is_for_session(&summary.session));

    let report = &summary.report;
    assert_eq!(report.adapter_version, "0.1.1");
    assert_eq!(report.asif_version, "0.4");
    assert_eq!(report.asif_pin, "652d9e23ddbe5656b9ceea311df7e09e93c1cd2f");
    assert_eq!(report.source_session.as_deref(), Some(SESSION));
    assert_eq!(report.target_session, None);
    assert_eq!(report.counts.events_in, 42);
    assert_eq!(report.counts.events_out, 42);
    assert_eq!(report.counts.resources, 3);
    assert_eq!(report.counts.capsules, 1);
    assert_eq!(report.loss_count, 2);
    assert!(report.loss_count_matches());
    assert_eq!(report.losses.len(), 2);
    assert_eq!(report.losses[0].kind, "cwd-omitted");
    assert_eq!(report.losses[0].stage, "translation");
    assert_eq!(report.losses[0].scope.as_deref(), Some("environment"));
    assert!(!report.losses[0].message.is_empty());
    assert_eq!(report.losses[1].kind, "contexts-reconstructed");
    assert_eq!(report.losses[1].stage, "normalization");
    assert_eq!(
        report.unsupported,
        vec![
            "vowp.jobs".to_owned(),
            "vowp.gates".to_owned(),
            "vowp.leases".to_owned(),
            "vowp.project-db".to_owned()
        ]
    );
}

#[test]
fn asif_export_summary_never_carries_the_remote_path() {
    let summary = parse_asif_export_summary(EXPORT_FIXTURE.as_bytes())
        .unwrap()
        .value;
    let serialized = serde_json::to_string(&summary).unwrap();
    assert!(!serialized.contains("\"path\""), "{serialized}");
    assert!(!serialized.contains("\"report_path\""), "{serialized}");
    assert!(!serialized.contains("/srv/example-private"), "{serialized}");
    let debug = format!("{summary:?}");
    assert!(!debug.contains("/srv/example-private"), "{debug}");
}

#[test]
fn asif_import_wire_fixture_covers_every_ui_field_and_hides_cwd() {
    let parsed = parse_asif_import_response(IMPORT_FIXTURE.as_bytes()).unwrap();
    assert!(
        parsed.is_clean(),
        "unexpected warnings: {:?}",
        parsed.warnings
    );
    let response = parsed.value;
    assert!(response.is_idle());
    response.ensure_idle().unwrap();

    let session = &response.session;
    assert_eq!(session.id.as_str(), IMPORTED);
    assert_eq!(session.project.as_ref().map(|p| p.as_str()), Some("vowp"));
    assert_eq!(session.job_id, None);
    assert_eq!(session.role, None);
    assert_eq!(session.title, "Imported session");
    assert_eq!(session.parent_session_id, None);
    assert_eq!(session.node.as_str(), "vps");
    assert_eq!(session.state, SessionState::Idle);
    assert_eq!(session.executor, None);
    assert_eq!(session.last_seq, 42);
    assert_eq!(
        session.last_capsule_id.as_ref().map(|id| id.as_str()),
        Some("cap_01JCF5WQ8ZK3M2V7N4P6R8T0AG")
    );
    assert_eq!(session.log_bytes, 38210);
    assert_eq!(session.created_at.as_rfc3339(), "2026-11-14T12:05:00.000Z");
    assert_eq!(session.updated_at.as_rfc3339(), "2026-11-14T12:05:00.000Z");

    let report = &response.report;
    assert_eq!(report.source_session.as_deref(), Some(SESSION));
    assert_eq!(report.target_session.as_deref(), Some(IMPORTED));
    assert_eq!(report.counts.events_in, 42);
    assert_eq!(report.counts.events_out, 44);
    assert_eq!(report.loss_count, 3);
    assert_eq!(report.losses.len(), 3);
    assert!(report.unsupported.is_empty());
    assert_eq!(response.warnings, vec!["project example-source left null"]);

    let serialized = serde_json::to_string(&response).unwrap();
    assert!(!serialized.contains("\"cwd\""), "{serialized}");
    assert!(!serialized.contains("/srv/vowp"), "{serialized}");
    assert!(!serialized.contains("path"), "{serialized}");
}

#[test]
fn asif_import_parses_a_bound_role_and_executor() {
    let mut import: Value = serde_json::from_str(IMPORT_FIXTURE).unwrap();
    import["session"]["role"] = json!("user");
    import["session"]["job_id"] = json!("ss-13-asif-core");
    import["session"]["parent_session_id"] = json!(SESSION);
    import["session"]["executor"] = json!("opencode:deepseek/deepseek-flash#max");
    let parsed = parse_asif_import_response(&serde_json::to_vec(&import).unwrap()).unwrap();
    assert!(
        parsed.is_clean(),
        "unexpected warnings: {:?}",
        parsed.warnings
    );
    let session = &parsed.value.session;
    assert_eq!(
        session.role.as_ref().map(|role| role.as_str()),
        Some("user")
    );
    assert_eq!(
        session.job_id.as_ref().map(|job| job.as_str()),
        Some("ss-13-asif-core")
    );
    assert_eq!(
        session
            .parent_session_id
            .as_ref()
            .map(|parent| parent.as_str()),
        Some(SESSION)
    );
    assert_eq!(
        session.executor.as_deref(),
        Some("opencode:deepseek/deepseek-flash#max")
    );
    assert!(parsed.value.is_idle());
}

#[test]
fn asif_unknown_fields_are_ignored() {
    let mut export: Value = serde_json::from_str(EXPORT_FIXTURE).unwrap();
    export["future_field"] = json!({"nested": true});
    export["report"]["future_report_field"] = json!(1);
    let parsed = parse_asif_export_summary(&serde_json::to_vec(&export).unwrap()).unwrap();
    assert_eq!(parsed.value.bytes, EXPORT_BYTES as u64);

    let mut import: Value = serde_json::from_str(IMPORT_FIXTURE).unwrap();
    import["session"]["future_session_field"] = json!("ignored");
    import["future_field"] = json!([]);
    let parsed = parse_asif_import_response(&serde_json::to_vec(&import).unwrap()).unwrap();
    assert_eq!(parsed.value.session.id.as_str(), IMPORTED);
}

#[test]
fn asif_wrong_format_and_missing_fields_are_typed_errors() {
    let mut export: Value = serde_json::from_str(EXPORT_FIXTURE).unwrap();
    export["format"] = json!("bundle");
    let error = parse_asif_export_summary(&serde_json::to_vec(&export).unwrap()).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::VowpIncompatible);
    assert!(error.message.contains("asif export summary.format"));

    let mut export: Value = serde_json::from_str(EXPORT_FIXTURE).unwrap();
    export.as_object_mut().unwrap().remove("bytes");
    let error = parse_asif_export_summary(&serde_json::to_vec(&export).unwrap()).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Parse);
    assert!(error.message.contains("asif export summary"));

    let mut import: Value = serde_json::from_str(IMPORT_FIXTURE).unwrap();
    import.as_object_mut().unwrap().remove("session");
    let error = parse_asif_import_response(&serde_json::to_vec(&import).unwrap()).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Parse);
    assert!(error.message.contains("asif import response"));
}

#[test]
fn asif_target_mismatch_is_incompatible() {
    let mut import: Value = serde_json::from_str(IMPORT_FIXTURE).unwrap();
    import["report"]["target_session"] = json!(SESSION);
    let error = parse_asif_import_response(&serde_json::to_vec(&import).unwrap()).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::VowpIncompatible);
    assert!(error.message.contains("target_session"));
}

#[test]
fn asif_loss_count_and_unknown_stage_are_warnings_not_truncation() {
    let mut import: Value = serde_json::from_str(IMPORT_FIXTURE).unwrap();
    import["report"]["loss_count"] = json!(9);
    import["report"]["losses"][0]["stage"] = json!("future-stage");
    let parsed = parse_asif_import_response(&serde_json::to_vec(&import).unwrap()).unwrap();
    assert_eq!(parsed.value.report.loss_count, 9);
    assert_eq!(parsed.value.report.losses.len(), 3);
    assert_eq!(parsed.value.report.losses[0].stage, "future-stage");
    assert!(parsed.warnings.iter().any(|warning| {
        warning.path == "report.loss_count" && warning.code == WireWarningCode::Schema
    }));
    assert!(parsed.warnings.iter().any(|warning| {
        warning.path == "report.losses[0].stage" && warning.code == WireWarningCode::UnknownEnum
    }));
}

#[test]
fn asif_unknown_session_state_and_executor_stay_unknown() {
    let mut import: Value = serde_json::from_str(IMPORT_FIXTURE).unwrap();
    import["session"]["state"] = json!("quantum");
    import["session"]["executor"] = json!("quantum:model-a");
    let parsed = parse_asif_import_response(&serde_json::to_vec(&import).unwrap()).unwrap();
    assert!(!parsed.value.session.state.is_known());
    assert_eq!(parsed.value.session.state.as_str(), "quantum");
    assert!(!parsed.value.is_idle());
    assert_eq!(
        parsed.value.ensure_idle().unwrap_err().code,
        SpiritErrorCode::VowpIncompatible
    );
    assert!(parsed.warnings.iter().any(|warning| {
        warning.path == "session.state" && warning.code == WireWarningCode::UnknownEnum
    }));
    assert!(parsed.warnings.iter().any(|warning| {
        warning.path == "session.executor" && warning.code == WireWarningCode::UnknownEnum
    }));
}

#[test]
fn asif_strict_refusal_fixture_is_a_refusal_not_a_force_switch() {
    let document = parse_error_document(STRICT_REFUSAL.as_bytes()).unwrap();
    let error = document.to_spirit_error(5);
    assert_eq!(error.code, SpiritErrorCode::Refused);
    assert!(
        error
            .message
            .contains("required unsupported feature continuation")
    );
    assert!(!error.retryable);

    let conflict = parse_error_document(IMPORT_CONFLICT.as_bytes()).unwrap();
    let error = conflict.to_spirit_error(4);
    assert_eq!(error.code, SpiritErrorCode::Conflict);
    assert!(
        !error.retryable,
        "a conflict is never retried automatically"
    );
    assert!(error.message.contains("already belongs to another session"));
}

#[test]
fn asif_options_reject_allow_unsupported() {
    let error = serde_json::from_value::<AsifExportOptions>(json!({
        "loss": "default",
        "allow_unsupported": true
    }))
    .unwrap_err();
    assert!(error.to_string().contains("unknown field"), "{error}");

    let error = serde_json::from_value::<AsifImportOptions>(json!({
        "loss": "strict",
        "allow_unsupported": true
    }))
    .unwrap_err();
    assert!(error.to_string().contains("unknown field"), "{error}");
}

// ---------------------------------------------------------------------------
// Export argv, caps and cleanup
// ---------------------------------------------------------------------------

#[tokio::test]
async fn asif_export_uses_the_documented_two_step_argv() {
    let (fake, client) = client_with(ok_export_responses());
    let export = client
        .asif_export("vowp", SESSION, AsifExportOptions::default())
        .await
        .unwrap();

    assert_eq!(export.summary.session.as_str(), SESSION);
    assert_eq!(export.summary.bytes, EXPORT_BYTES as u64);
    assert_eq!(export.package.len(), EXPORT_BYTES);
    assert_eq!(export.package.as_bytes(), package());
    assert_eq!(export.file_name(), format!("{SESSION}.asif.zip"));
    assert_eq!(export.report().loss_count, 2);

    let calls = fake.calls();
    assert_eq!(calls.len(), 6, "mktemp, chmod, export, stat, cat, rm");
    assert!(remote(&calls[0]).contains("'mktemp' '-d' '-t' 'stackspirit-asif.XXXXXXXXXX'"));
    assert!(remote(&calls[1]).contains(&format!("'chmod' '700' '{TEMP}'")));
    let export_command = remote(&calls[2]);
    assert!(
        export_command.contains(&format!(
            "'--project' 'vowp' '--json' 'session' 'export' '{SESSION}' '--format' 'asif' '--out' '{TEMP}/{SESSION}.asif.zip'"
        )),
        "{export_command}"
    );
    assert!(!export_command.contains("'--out' '-'"));
    assert_eq!(calls[2].output_cap, ASIF_JSON_CAP_BYTES);
    assert!(remote(&calls[3]).contains(&format!("'stat' '-c' '%s' '{TEMP}/{SESSION}.asif.zip'")));
    assert!(remote(&calls[4]).contains(&format!("'cat' '{TEMP}/{SESSION}.asif.zip'")));
    assert_eq!(
        calls[4].output_cap,
        usize::try_from(ASIF_ARCHIVE_CAP_BYTES).unwrap()
    );
    assert_last_call_removed(&fake);

    for call in &calls {
        let command = remote(call);
        for forbidden in ["|", ";", "&&", "<", ">", "$", "`"] {
            assert!(
                !command.contains(forbidden),
                "remote command contains {forbidden:?}: {command}"
            );
        }
        assert!(!command.contains("--asif-stream"));
        assert!(!command.contains("--allow-unsupported"));
    }

    let serialized = serde_json::to_string(&export.summary).unwrap();
    assert!(!serialized.contains("path"), "{serialized}");
    assert!(!serialized.contains(TEMP), "{serialized}");
    let debug = format!("{export:?}");
    assert!(!debug.contains("PPPP"), "raw package bytes leaked: {debug}");
    assert!(debug.contains("package_bytes"));
}

#[tokio::test]
async fn asif_export_loss_policy_never_sends_both_flags() {
    for (policy, expected) in [
        (AsifLossPolicy::Default, None),
        (AsifLossPolicy::Strict, Some("'--strict'")),
        (AsifLossPolicy::AllowLoss, Some("'--allow-loss'")),
    ] {
        let (fake, client) = client_with(ok_export_responses());
        client
            .asif_export("vowp", SESSION, AsifExportOptions::new(policy))
            .await
            .unwrap();
        let command = remote(&fake.calls()[2]);
        match expected {
            Some(flag) => {
                assert!(command.contains(flag), "{policy:?}: {command}");
                let other = if flag == "'--strict'" {
                    "'--allow-loss'"
                } else {
                    "'--strict'"
                };
                assert!(!command.contains(other), "{policy:?}: {command}");
            }
            None => {
                assert!(!command.contains("'--strict'"), "{command}");
                assert!(!command.contains("'--allow-loss'"), "{command}");
            }
        }
        assert!(!command.contains("--allow-unsupported"));
    }
}

#[tokio::test]
async fn asif_export_removes_the_temp_dir_on_every_failure_path() {
    // chmod failure.
    let (fake, client) = client_with(vec![
        CommandOutput::ok(format!("{TEMP}\n")),
        CommandOutput::fail(1, "chmod failed"),
        CommandOutput::ok(""),
    ]);
    let error = client
        .asif_export("vowp", SESSION, AsifExportOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Remote);
    assert_no_remote_path(&error);
    assert_eq!(fake.call_count(), 3);
    assert_last_call_removed(&fake);

    // VOWP export refusal (exit 5) preserves its message and still cleans up.
    let (fake, client) = client_with(vec![
        CommandOutput::ok(format!("{TEMP}\n")),
        CommandOutput::ok(""),
        failure(5, STRICT_REFUSAL.as_bytes(), ""),
        CommandOutput::ok(""),
    ]);
    let error = client
        .asif_export("vowp", SESSION, AsifExportOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Refused);
    assert!(error.message.contains("continuation"));
    assert!(!error.retryable);
    assert_no_remote_path(&error);
    assert_eq!(fake.call_count(), 4);
    assert_last_call_removed(&fake);

    // A summary for another session is a typed incompatibility.
    let mut summary: Value = serde_json::from_str(EXPORT_FIXTURE).unwrap();
    summary["session"] = json!(IMPORTED);
    summary["report"]["source_session"] = json!(IMPORTED);
    let (fake, client) = client_with(vec![
        CommandOutput::ok(format!("{TEMP}\n")),
        CommandOutput::ok(""),
        CommandOutput::ok(serde_json::to_vec(&summary).unwrap()),
        CommandOutput::ok(""),
    ]);
    let error = client
        .asif_export("vowp", SESSION, AsifExportOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::VowpIncompatible);
    assert_eq!(fake.call_count(), 4);
    assert_last_call_removed(&fake);
}

#[tokio::test]
async fn asif_export_enforces_the_archive_cap_before_the_read() {
    // The summary itself declares an oversized package: no stat or cat call.
    let mut summary: Value = serde_json::from_str(EXPORT_FIXTURE).unwrap();
    summary["bytes"] = json!(ASIF_ARCHIVE_CAP_BYTES + 1);
    let (fake, client) = client_with(vec![
        CommandOutput::ok(format!("{TEMP}\n")),
        CommandOutput::ok(""),
        CommandOutput::ok(serde_json::to_vec(&summary).unwrap()),
        CommandOutput::ok(""),
    ]);
    let error = client
        .asif_export("vowp", SESSION, AsifExportOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::InvalidInput);
    assert!(error.message.contains(&ASIF_ARCHIVE_CAP_BYTES.to_string()));
    assert_eq!(fake.call_count(), 4, "no stat or cat after the cap check");
    assert_last_call_removed(&fake);

    // A stat result above the cap is refused before cat.
    let (fake, client) = client_with(vec![
        CommandOutput::ok(format!("{TEMP}\n")),
        CommandOutput::ok(""),
        CommandOutput::ok(EXPORT_FIXTURE),
        CommandOutput::ok(format!("{}\n", ASIF_ARCHIVE_CAP_BYTES + 1)),
        CommandOutput::ok(""),
    ]);
    let error = client
        .asif_export("vowp", SESSION, AsifExportOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::InvalidInput);
    assert_eq!(fake.call_count(), 5);
    assert_last_call_removed(&fake);
}

#[tokio::test]
async fn asif_export_verifies_the_package_byte_count() {
    // stat and cat agree with each other but not with the summary.
    let (fake, client) = client_with(vec![
        CommandOutput::ok(format!("{TEMP}\n")),
        CommandOutput::ok(""),
        CommandOutput::ok(EXPORT_FIXTURE),
        CommandOutput::ok(format!("{}\n", EXPORT_BYTES - 1)),
        CommandOutput::ok(vec![b'P'; EXPORT_BYTES - 1]),
        CommandOutput::ok(""),
    ]);
    let error = client
        .asif_export("vowp", SESSION, AsifExportOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Remote);
    assert!(error.retryable);
    assert!(error.message.contains("does not match"));
    assert_eq!(fake.call_count(), 6);
    assert_last_call_removed(&fake);

    // `stat` and `cat` disagree: the read is torn and refused.
    let (fake, client) = client_with(vec![
        CommandOutput::ok(format!("{TEMP}\n")),
        CommandOutput::ok(""),
        CommandOutput::ok(EXPORT_FIXTURE),
        CommandOutput::ok(format!("{EXPORT_BYTES}\n")),
        CommandOutput::ok(vec![b'P'; 10]),
        CommandOutput::ok(""),
    ]);
    let error = client
        .asif_export("vowp", SESSION, AsifExportOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Remote);
    assert!(error.retryable);
    assert!(error.message.contains("changed size"));
    assert_last_call_removed(&fake);
}

#[tokio::test]
async fn asif_export_rejects_a_temp_path_it_did_not_create() {
    for path in [
        "/tmp/not-ours",
        "/tmp/../etc/stackspirit-asif.ABC",
        "/tmp/stackspirit-asif.ABC;reboot",
        "/tmp/stackspirit-asif.ABC/extra",
        "relative/stackspirit-asif.ABC",
    ] {
        let (fake, client) = client_with(vec![CommandOutput::ok(format!("{path}\n"))]);
        let error = client
            .asif_export("vowp", SESSION, AsifExportOptions::default())
            .await
            .unwrap_err();
        assert_eq!(
            error.code,
            SpiritErrorCode::InvalidInput,
            "path {path:?} was not rejected"
        );
        assert!(!format!("{error:?}").contains(path));
        assert_eq!(fake.call_count(), 1, "no command used an unvalidated path");
    }
}

#[tokio::test]
async fn asif_export_reports_cleanup_failure_and_returns_no_package() {
    let (fake, client) = client_with(vec![
        CommandOutput::ok(format!("{TEMP}\n")),
        CommandOutput::ok(""),
        CommandOutput::ok(EXPORT_FIXTURE),
        CommandOutput::ok(format!("{EXPORT_BYTES}\n")),
        CommandOutput::ok(package()),
        CommandOutput::fail(1, "cannot remove"),
    ]);
    let error = client
        .asif_export("vowp", SESSION, AsifExportOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Remote);
    assert!(!error.retryable);
    assert!(error.message.contains("could not be removed"));
    assert_no_remote_path(&error);
    assert_eq!(fake.call_count(), 6);
    assert_last_call_removed(&fake);
}

#[tokio::test]
async fn asif_export_scrubs_the_remote_path_from_vowp_errors() {
    let body = json!({
        "error": {
            "kind": "conflict",
            "message": format!("target {TEMP}/package.asif.zip already exists"),
            "hint": format!("remove the file under {TEMP}")
        }
    });
    let (fake, client) = client_with(vec![
        CommandOutput::ok(format!("{TEMP}\n")),
        CommandOutput::ok(""),
        failure(4, &serde_json::to_vec(&body).unwrap(), ""),
        CommandOutput::ok(""),
    ]);
    let error = client
        .asif_export("vowp", SESSION, AsifExportOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Conflict);
    assert!(!error.retryable);
    assert_no_remote_path(&error);
    assert!(error.message.contains("[remote-path]"));
    assert_eq!(fake.call_count(), 4);
    assert_last_call_removed(&fake);
}

// ---------------------------------------------------------------------------
// Import streaming, parsing and exit codes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn asif_import_streams_the_package_and_parses_every_field() {
    let (fake, client) = client_with(vec![CommandOutput::ok(IMPORT_FIXTURE)]);
    let package = local_package();
    let options = AsifImportOptions {
        branch: Some("branch-1".to_owned()),
        cwd: Some("/srv/example-work".to_owned()),
        loss: AsifLossPolicy::Strict,
    };
    let response = client.asif_import("vowp", &package, options).await.unwrap();

    assert!(response.is_idle());
    assert_eq!(response.session.id.as_str(), IMPORTED);
    assert_eq!(response.session.title, "Imported session");
    assert_eq!(response.session.node.as_str(), "vps");
    assert_eq!(response.report.loss_count, 3);
    assert_eq!(response.report.losses.len(), 3);
    assert!(response.report.unsupported.is_empty());
    assert_eq!(response.warnings, vec!["project example-source left null"]);

    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].stdin.as_deref(), Some(package.as_slice()));
    let command = remote(&calls[0]);
    assert!(
        command.contains(
            "'--project' 'vowp' '--json' 'session' 'import' '-' '--format' 'asif' '--branch' 'branch-1' '--cwd' '/srv/example-work' '--strict'"
        ),
        "{command}"
    );
    for forbidden in [
        "--allow-unsupported",
        "--asif-stream",
        "--same-id",
        "--executor",
        "--report",
        "--node",
        "'--allow-loss'",
    ] {
        assert!(
            !command.contains(forbidden),
            "unexpected {forbidden}: {command}"
        );
    }
    assert!(!command.contains("'job'"));
    assert!(!command.contains("'lease'"));
    assert!(!command.contains("'turn'"));
    assert!(!command.contains("'capsule'"));
}

#[tokio::test]
async fn asif_import_loss_policy_never_sends_both_flags() {
    for (policy, expected) in [
        (AsifLossPolicy::Default, None),
        (AsifLossPolicy::Strict, Some("'--strict'")),
        (AsifLossPolicy::AllowLoss, Some("'--allow-loss'")),
    ] {
        let (fake, client) = client_with(vec![CommandOutput::ok(IMPORT_FIXTURE)]);
        client
            .asif_import(
                "vowp",
                &local_package(),
                AsifImportOptions {
                    loss: policy,
                    ..AsifImportOptions::default()
                },
            )
            .await
            .unwrap();
        let command = remote(&fake.calls()[0]);
        match expected {
            Some(flag) => {
                assert!(command.contains(flag), "{policy:?}: {command}");
                let other = if flag == "'--strict'" {
                    "'--allow-loss'"
                } else {
                    "'--strict'"
                };
                assert!(!command.contains(other), "{policy:?}: {command}");
            }
            None => {
                assert!(!command.contains("'--strict'"), "{command}");
                assert!(!command.contains("'--allow-loss'"), "{command}");
            }
        }
        assert!(!command.contains("--allow-unsupported"));
    }
}

#[tokio::test]
async fn asif_import_exit_codes_map_without_retry_or_mutation() {
    let cases = [
        (
            2,
            Some(
                br#"{"error":{"kind":"invalid_input","message":"invalid ASIF package","hint":null}}"#
                    .to_vec(),
            ),
            "invalid ASIF package",
            SpiritErrorCode::InvalidInput,
            false,
        ),
        (
            3,
            Some(
                br#"{"error":{"kind":"not_found","message":"unknown source session","hint":null}}"#
                    .to_vec(),
            ),
            "unknown source session",
            SpiritErrorCode::NotFound,
            true,
        ),
        (
            4,
            Some(IMPORT_CONFLICT.as_bytes().to_vec()),
            "already belongs to another session",
            SpiritErrorCode::Conflict,
            false,
        ),
        (
            5,
            Some(STRICT_REFUSAL.as_bytes().to_vec()),
            "required unsupported feature continuation",
            SpiritErrorCode::Refused,
            false,
        ),
        (
            7,
            None,
            "remote transport failure",
            SpiritErrorCode::Remote,
            true,
        ),
    ];

    for (exit, stdout, needle, expected_code, expected_retry) in cases {
        let output = match stdout {
            Some(stdout) => failure(exit, &stdout, ""),
            None => failure(exit, b"", "remote transport failure"),
        };
        let fake = Arc::new(FakeCommandRunner::new(vec![output]));
        let client = SshVowpClient::new(fake.clone(), target(), ssh_options(), None);
        let package = local_package();
        let error = client
            .asif_import("vowp", &package, AsifImportOptions::default())
            .await
            .unwrap_err();

        assert_eq!(error.code, expected_code, "exit {exit}");
        assert_eq!(error.retryable, expected_retry, "exit {exit}");
        assert!(error.message.contains(needle), "exit {exit}: {error:?}");
        assert_eq!(fake.call_count(), 1, "exit {exit} must not be retried");
        let calls = fake.calls();
        assert_eq!(
            calls[0].stdin.as_deref(),
            Some(package.as_slice()),
            "exit {exit} must not mutate the package"
        );
    }
}

#[tokio::test]
async fn asif_import_scrubs_the_remote_cwd_from_errors() {
    let body = json!({
        "error": {
            "kind": "guard",
            "message": "cwd /srv/example-private is outside the allow-list",
            "hint": "choose a directory under the project"
        }
    });
    let fake = Arc::new(FakeCommandRunner::new(vec![failure(
        5,
        &serde_json::to_vec(&body).unwrap(),
        "",
    )]));
    let client = SshVowpClient::new(fake, target(), ssh_options(), None);
    let error = client
        .asif_import(
            "vowp",
            &local_package(),
            AsifImportOptions {
                cwd: Some("/srv/example-private".to_owned()),
                ..AsifImportOptions::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Refused);
    assert!(!error.message.contains("/srv/example-private"));
    assert!(error.message.contains("[remote-path]"));
}

#[tokio::test]
async fn asif_import_rejects_non_idle_and_oversized_inputs() {
    let mut import: Value = serde_json::from_str(IMPORT_FIXTURE).unwrap();
    import["session"]["state"] = json!("running");
    let fake = Arc::new(FakeCommandRunner::new(vec![CommandOutput::ok(
        serde_json::to_vec(&import).unwrap(),
    )]));
    let client = SshVowpClient::new(fake.clone(), target(), ssh_options(), None);
    let error = client
        .asif_import("vowp", &local_package(), AsifImportOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::VowpIncompatible);
    assert!(error.message.contains("must be idle"));
    assert_eq!(fake.call_count(), 1);

    assert!(ensure_package_size(ASIF_ARCHIVE_CAP_BYTES).is_ok());
    let error = ensure_package_size(ASIF_ARCHIVE_CAP_BYTES + 1).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::InvalidInput);
    assert!(error.message.contains(&ASIF_ARCHIVE_CAP_BYTES.to_string()));
    assert!(error.hint.is_some());
}

#[tokio::test]
async fn asif_import_validates_branch_and_cwd_before_the_command() {
    let fake = Arc::new(FakeCommandRunner::new(vec![]));
    let client = SshVowpClient::new(fake.clone(), target(), ssh_options(), None);
    for options in [
        AsifImportOptions {
            branch: Some("-oProxyCommand=evil".to_owned()),
            ..AsifImportOptions::default()
        },
        AsifImportOptions {
            branch: Some("bad\nbranch".to_owned()),
            ..AsifImportOptions::default()
        },
        AsifImportOptions {
            cwd: Some("relative/path".to_owned()),
            ..AsifImportOptions::default()
        },
        AsifImportOptions {
            cwd: Some("/".to_owned()),
            ..AsifImportOptions::default()
        },
        AsifImportOptions {
            cwd: Some("bad\ncwd".to_owned()),
            ..AsifImportOptions::default()
        },
    ] {
        let error = client
            .asif_import("vowp", &local_package(), options)
            .await
            .unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::InvalidInput);
    }
    assert_eq!(fake.call_count(), 0);
}

// ---------------------------------------------------------------------------
// SpiritCore facade
// ---------------------------------------------------------------------------

fn supervisor() -> EngineSupervisor {
    EngineSupervisor::new(
        Arc::new(FakeEngineRuntime::new()),
        EngineSupervisorConfig {
            restart_budget: 3,
            health_attempts: 1,
            retry_delay: Duration::ZERO,
            port: 0,
        },
    )
}

fn host_input() -> NewHostProfile {
    NewHostProfile {
        draft: HostDraft {
            label: "primary-vps".to_owned(),
            ssh: target(),
            identity_file: None,
            vowp_home: None,
            default_project: Some("vowp".to_owned()),
        },
        host_key: HostKeyPin {
            algorithm: "ssh-ed25519".to_owned(),
            sha256: "SHA256:AbCdEfGhIjKlMnOpQrStUvWxYz0123456789ABCDEFG".to_owned(),
            pinned_at: Timestamp::epoch(),
        },
    }
}

fn mock_client() -> MockVowpClient {
    // The golden snapshot is fixed at 2026-11-14. Move the freshness markers to "now"
    // so the connection is live; a stale view intentionally refuses imports.
    let now = Timestamp::now().as_rfc3339();
    let mut status: Value = serde_json::from_str(FRESH).unwrap();
    status["fetched_at"] = json!(now);
    status["snapshot"]["generated_at"] = json!(now);
    status["snapshot"]["supervisor"]["last_tick"] = json!(now);
    let status = parse_fleet_status(&serde_json::to_vec(&status).unwrap())
        .unwrap()
        .value;
    let capacity = parse_capacity_report(CAPACITY.as_bytes()).unwrap().value;
    let summary = parse_asif_export_summary(EXPORT_FIXTURE.as_bytes())
        .unwrap()
        .value;
    let response = parse_asif_import_response(IMPORT_FIXTURE.as_bytes())
        .unwrap()
        .value;
    MockVowpClient::new(status, capacity)
        .with_asif_export(summary, package())
        .with_asif_import(response)
}

#[tokio::test]
async fn spirit_core_asif_uses_the_active_project_and_import_stays_idle() {
    let store = Arc::new(InMemoryHostStore::new());
    let core = SpiritCore::new(store, supervisor());
    let host = core.add_host(host_input()).unwrap();
    let client = mock_client();
    core.connect(host.id, &client).await.unwrap();

    assert_eq!(core.active_project().as_deref(), Some("vowp"));

    let export = core
        .export_asif(&client, None, SESSION, AsifExportOptions::default())
        .await
        .unwrap();
    assert_eq!(export.summary.session.as_str(), SESSION);
    assert_eq!(export.package.len(), EXPORT_BYTES);
    let exports = client.asif_exports();
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].0, "vowp");
    assert_eq!(exports[0].1, SESSION);

    let package = local_package();
    let response = core
        .import_asif(&client, None, &package, AsifImportOptions::default())
        .await
        .unwrap();
    assert!(response.is_idle());
    assert_eq!(response.session.id.as_str(), IMPORTED);
    let imports = client.asif_imports();
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].0, "vowp");
    assert_eq!(imports[0].1, package.len());

    // No job action, lease, target write or extra poll follows an import.
    assert!(client.actions().is_empty());
    assert!(client.targets().is_empty());
    assert_eq!(client.status_calls(), 1);
}

#[tokio::test]
async fn spirit_core_asif_import_refuses_without_a_live_snapshot() {
    let store = Arc::new(InMemoryHostStore::new());
    let core = SpiritCore::new(store, supervisor());
    let client = mock_client();

    assert_eq!(core.active_project(), None);
    let error = core
        .import_asif(
            &client,
            Some("vowp"),
            &local_package(),
            AsifImportOptions::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Conflict);
    assert!(client.asif_imports().is_empty());

    let error = core
        .export_asif(&client, None, SESSION, AsifExportOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::NotFound);
    assert!(client.asif_exports().is_empty());
}

#[tokio::test]
async fn spirit_core_asif_rejects_an_invalid_project_and_a_non_idle_import() {
    let store = Arc::new(InMemoryHostStore::new());
    let core = SpiritCore::new(store, supervisor());
    let host = core.add_host(host_input()).unwrap();
    let client = mock_client();
    core.connect(host.id, &client).await.unwrap();

    let error = core
        .export_asif(
            &client,
            Some("../bad"),
            SESSION,
            AsifExportOptions::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::InvalidInput);
    assert!(client.asif_exports().is_empty());

    let mut import: Value = serde_json::from_str(IMPORT_FIXTURE).unwrap();
    import["session"]["state"] = json!("running");
    let non_idle: AsifImportResponse =
        serde_json::from_value(import).expect("the fixture stays parseable");
    let client = mock_client().with_asif_import(non_idle);
    let error = core
        .import_asif(
            &client,
            Some("vowp"),
            &local_package(),
            AsifImportOptions::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::VowpIncompatible);
    assert!(error.message.contains("must be idle"));
    assert_eq!(client.asif_imports().len(), 1);
}
