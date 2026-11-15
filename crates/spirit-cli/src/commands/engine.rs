//! Read-only local engine status.
//!
//! The CLI never starts, stops, probes or reconfigures the engine, never accepts a
//! provider key, and never reads the OS keychain. It reports the saved local
//! preferences and an honest "not probed by this process" state.

use serde::Serialize;
use spirit_core::engine::{
    EngineChoice, EnginePreferences, EnginePreferencesStore, EngineState, EngineStatus,
    JsonEnginePreferences,
};

use crate::args::EngineCommand;
use crate::context::CliEnv;
use crate::error::CliError;
use crate::output::{Output, safe_text};

pub(crate) fn dispatch(
    command: &EngineCommand,
    env: &CliEnv,
    output: &mut Output<'_>,
) -> Result<(), CliError> {
    match command {
        EngineCommand::Status => status(env, output),
    }
}

/// The read-only engine status document.
#[derive(Debug, Serialize)]
struct EngineStatusReport {
    /// Always true for this command.
    read_only: bool,
    /// Core engine status, flattened so the JSON keeps the familiar core field names.
    /// Its state is `not_configured` or `stopped`; the CLI does not start a process
    /// or query a live engine.
    #[serde(flatten)]
    engine: EngineStatus,
    /// The saved local choice and selected model id.
    preferences: EnginePreferences,
    /// Key-state summary. The CLI does not read the keychain, so this is empty.
    keychain: KeychainSummary,
}

#[derive(Debug, Serialize)]
struct KeychainSummary {
    /// False because the CLI never opens the OS keychain.
    checked: bool,
    /// Provider names only; always empty in this build.
    providers: Vec<String>,
    /// Human explanation.
    note: &'static str,
}

fn status(env: &CliEnv, output: &mut Output<'_>) -> Result<(), CliError> {
    let store = JsonEnginePreferences::new(env.paths().engine_file());
    let preferences = store.load()?;
    let state = if preferences.onboarding_complete {
        EngineState::Stopped
    } else {
        EngineState::NotConfigured
    };
    let engine = EngineStatus {
        choice: preferences.choice.clone(),
        state,
        onboarding_complete: preferences.onboarding_complete,
        ..EngineStatus::default()
    };
    let report = EngineStatusReport {
        read_only: true,
        engine,
        preferences,
        keychain: KeychainSummary {
            checked: false,
            providers: Vec::new(),
            note: "the CLI never reads the OS keychain; manage provider keys in the desktop app",
        },
    };

    if output.is_json() {
        return output.json(&report);
    }
    output.text("read-only: yes")?;
    output.text(format!(
        "engine state: {}",
        engine_state_label(&report.engine.state)
    ))?;
    output.text(format!(
        "engine choice: {}",
        engine_choice_label(&report.engine.choice)
    ))?;
    output.text(format!(
        "onboarding: {}",
        if report.engine.onboarding_complete {
            "complete"
        } else {
            "incomplete"
        }
    ))?;
    output.text(format!(
        "selected model: {}",
        report
            .preferences
            .selected_model_id
            .as_deref()
            .map_or_else(|| "none".to_owned(), |model| safe_text(model, 120))
    ))?;
    output.text("engine port: none (the CLI does not start or query a running engine)")?;
    output.text("provider keys: not read by the CLI")?;
    Ok(())
}

fn engine_state_label(state: &EngineState) -> &'static str {
    match state {
        EngineState::NotConfigured => "not_configured",
        EngineState::Stopped => "stopped",
        EngineState::Starting => "starting",
        EngineState::Ready => "ready",
        EngineState::Degraded { .. } => "degraded",
        EngineState::Failed { .. } => "failed",
    }
}

fn engine_choice_label(choice: &EngineChoice) -> String {
    match choice {
        EngineChoice::Bundled => "bundled".to_owned(),
        EngineChoice::External { path, kind } => format!(
            "external {} ({})",
            safe_text(&path.display().to_string(), 300),
            kind.as_str()
        ),
    }
}
