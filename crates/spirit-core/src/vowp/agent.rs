//! The Mac-side local-agent boundary and its command/mock implementations.
//!
//! These commands run with the local [`CommandRunner`], never over SSH. The pairing
//! code travels on stdin with `--code-file -`, so it never appears in argv, a temporary
//! file, a log or an error. Path maps are validated locally and only [`PathRef`] values
//! may leave this module.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

use crate::error::{SpiritError, SpiritErrorCode};
use crate::ssh::CommandRunner;
use crate::ssh::command::{CommandOutput, CommandSpec, WRITE_TIMEOUT};
use crate::vowp::client::check_command_output;
use crate::vowp::local::{AgentPairRequest, AgentPairing};
use crate::wire::{
    AgentConnectionState, AgentPairOutput, AgentPathAddRequest, AgentPathMap, AgentStatusDocument,
    ProjectName,
};

/// The Mac-side commands D6 needs. It never opens a VOWP database and never talks to
/// the home over SSH.
#[async_trait]
pub trait LocalAgentClient: Send + Sync {
    /// Reads `vowp agent status --json`.
    async fn status(&self) -> Result<AgentStatusDocument, SpiritError>;

    /// Redeems a home pairing code through `vowp agent pair`.
    async fn pair(&self, request: &AgentPairRequest) -> Result<AgentPairing, SpiritError> {
        let _ = request;
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "local agent pairing is unavailable for this client",
        )
        .with_hint("Use the Mac-side local agent client."))
    }

    /// Reads `vowp agent paths ls --json`.
    async fn paths(&self) -> Result<Vec<AgentPathMap>, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "the local path map is unavailable for this client",
        )
        .with_hint("Use the Mac-side local agent client."))
    }

    /// Adds or updates one `vowp agent paths add` entry.
    async fn add_path(&self, request: &AgentPathAddRequest) -> Result<AgentPathMap, SpiritError> {
        let _ = request;
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "updating the local path map is unavailable for this client",
        )
        .with_hint("Use the Mac-side local agent client."))
    }

    /// Removes one `vowp agent paths rm` entry.
    async fn remove_path(&self, project: &ProjectName) -> Result<(), SpiritError> {
        let _ = project;
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "updating the local path map is unavailable for this client",
        )
        .with_hint("Use the Mac-side local agent client."))
    }
}

/// The production local client: runs `vowp ... --json` with the injected runner.
#[derive(Clone)]
pub struct CommandLocalAgent {
    runner: Arc<dyn CommandRunner>,
}

impl std::fmt::Debug for CommandLocalAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandLocalAgent").finish_non_exhaustive()
    }
}

impl CommandLocalAgent {
    /// Builds a local client from the app's command runner.
    #[must_use]
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        Self { runner }
    }

    fn base_argv(&self) -> Vec<String> {
        vec!["--json".to_owned()]
    }

    async fn run(
        &self,
        argv: Vec<String>,
        stdin: Option<Vec<u8>>,
        timeout: Duration,
    ) -> Result<CommandOutput, SpiritError> {
        let mut spec = CommandSpec::new("vowp", argv).with_timeout(timeout);
        if let Some(stdin) = stdin {
            spec = spec.with_stdin(stdin);
        }
        self.runner.run(&spec).await
    }

    fn missing_status(error: &SpiritError) -> Option<AgentStatusDocument> {
        matches!(
            error.code,
            SpiritErrorCode::SshUnavailable | SpiritErrorCode::VowpMissing
        )
        .then(|| AgentStatusDocument::with_connection(AgentConnectionState::NotInstalled))
    }
}

#[async_trait]
impl LocalAgentClient for CommandLocalAgent {
    async fn status(&self) -> Result<AgentStatusDocument, SpiritError> {
        let mut argv = self.base_argv();
        argv.extend(["agent".to_owned(), "status".to_owned()]);
        match self.run(argv, None, Duration::from_secs(10)).await {
            Ok(output) if output.status == 3 => Ok(AgentStatusDocument::with_connection(
                AgentConnectionState::NotEnrolled,
            )),
            Ok(output) if output.success() => serde_json::from_slice(&output.stdout)
                .map_err(|err| SpiritError::parse("agent status", err)),
            Ok(output) => match check_command_output(output) {
                Ok(output) => serde_json::from_slice(&output.stdout)
                    .map_err(|err| SpiritError::parse("agent status", err)),
                Err(error) => Err(error),
            },
            Err(error) => {
                if let Some(status) = Self::missing_status(&error) {
                    Ok(status)
                } else {
                    Err(error)
                }
            }
        }
    }

    async fn pair(&self, request: &AgentPairRequest) -> Result<AgentPairing, SpiritError> {
        request.validate()?;
        let mut argv = self.base_argv();
        argv.extend([
            "agent".to_owned(),
            "pair".to_owned(),
            "--code-file".to_owned(),
            "-".to_owned(),
            "--surface".to_owned(),
            request.surface.clone(),
            "--session".to_owned(),
            request.session.clone(),
        ]);
        if let Some(app) = &request.app {
            argv.extend(["--app".to_owned(), app.clone()]);
        }
        let mut code = request.code.expose().as_bytes().to_vec();
        code.push(b'\n');
        let output = self.run(argv, Some(code), WRITE_TIMEOUT).await?;
        let output = check_command_output(output)?;
        let parsed: AgentPairOutput = serde_json::from_slice(&output.stdout)
            .map_err(|err| SpiritError::parse("agent pair", err))?;
        Ok(AgentPairing::from_output(parsed))
    }

    async fn paths(&self) -> Result<Vec<AgentPathMap>, SpiritError> {
        let mut argv = self.base_argv();
        argv.extend(["agent".to_owned(), "paths".to_owned(), "ls".to_owned()]);
        let output = self.run(argv, None, Duration::from_secs(10)).await?;
        let output = check_command_output(output)?;
        let maps: Vec<AgentPathMap> = serde_json::from_slice(&output.stdout)
            .map_err(|err| SpiritError::parse("agent paths", err))?;
        for map in &maps {
            map.validate()?;
        }
        Ok(maps)
    }

    async fn add_path(&self, request: &AgentPathAddRequest) -> Result<AgentPathMap, SpiritError> {
        let project = request.validate()?;
        let mut argv = self.base_argv();
        argv.extend([
            "agent".to_owned(),
            "paths".to_owned(),
            "add".to_owned(),
            "--project".to_owned(),
            request.project_root.clone(),
            "--name".to_owned(),
            project.to_string(),
        ]);
        if let Some(worktree_root) = &request.worktree_root {
            argv.extend(["--worktree-root".to_owned(), worktree_root.clone()]);
        }
        for allowed in &request.allow {
            argv.extend(["--allow".to_owned(), allowed.clone()]);
        }
        let output = self.run(argv, None, WRITE_TIMEOUT).await?;
        let output = check_command_output(output)?;
        let map: AgentPathMap = serde_json::from_slice(&output.stdout)
            .map_err(|err| SpiritError::parse("agent paths add", err))?;
        map.validate()?;
        Ok(map)
    }

    async fn remove_path(&self, project: &ProjectName) -> Result<(), SpiritError> {
        let mut argv = self.base_argv();
        argv.extend([
            "agent".to_owned(),
            "paths".to_owned(),
            "rm".to_owned(),
            project.to_string(),
        ]);
        let output = self.run(argv, None, WRITE_TIMEOUT).await?;
        check_command_output(output)?;
        Ok(())
    }
}

#[derive(Debug)]
struct MockLocalAgentState {
    status: AgentStatusDocument,
    paths: Vec<AgentPathMap>,
    pair_result: Option<AgentPairOutput>,
    fail_status: Option<SpiritError>,
    status_calls: u32,
    pair_calls: Vec<AgentPairRequest>,
    path_calls: u32,
    added: Vec<AgentPathAddRequest>,
    removed: Vec<ProjectName>,
}

/// A programmable local agent for hermetic tests and previews.
#[derive(Debug, Clone)]
pub struct MockLocalAgentClient {
    state: Arc<Mutex<MockLocalAgentState>>,
}

impl MockLocalAgentClient {
    /// Builds a mock with the supplied status.
    #[must_use]
    pub fn new(status: AgentStatusDocument) -> Self {
        Self {
            state: Arc::new(Mutex::new(MockLocalAgentState {
                status,
                paths: Vec::new(),
                pair_result: None,
                fail_status: None,
                status_calls: 0,
                pair_calls: Vec::new(),
                path_calls: 0,
                added: Vec::new(),
                removed: Vec::new(),
            })),
        }
    }

    /// Builds a connected local agent with the required protocol.
    #[must_use]
    pub fn connected() -> Self {
        let status = AgentStatusDocument {
            connection: AgentConnectionState::Connected,
            node: Some("mac-studio".to_owned()),
            home: Some("home-1".to_owned()),
            protocol: Some(crate::wire::AGENT_PROTOCOL.to_owned()),
            cache_entries: 4,
            cache_bytes: 4096,
            ..AgentStatusDocument::default()
        };
        Self::new(status)
    }

    /// Replaces the status document.
    #[must_use]
    pub fn with_status(self, status: AgentStatusDocument) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .status = status;
        self
    }

    /// Replaces the path map.
    #[must_use]
    pub fn with_paths(self, paths: Vec<AgentPathMap>) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .paths = paths;
        self
    }

    /// Configures the result of `pair`.
    #[must_use]
    pub fn with_pair(self, result: AgentPairOutput) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pair_result = Some(result);
        self
    }

    /// Makes `status` fail.
    #[must_use]
    pub fn failing_status(self, error: SpiritError) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fail_status = Some(error);
        self
    }

    /// Returns the number of `status` calls.
    #[must_use]
    pub fn status_calls(&self) -> u32 {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .status_calls
    }

    /// Returns recorded pair requests.
    #[must_use]
    pub fn pair_calls(&self) -> Vec<AgentPairRequest> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pair_calls
            .clone()
    }

    /// Returns the number of `paths` calls.
    #[must_use]
    pub fn path_calls(&self) -> u32 {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .path_calls
    }

    /// Returns recorded path-add requests.
    #[must_use]
    pub fn added_paths(&self) -> Vec<AgentPathAddRequest> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .added
            .clone()
    }

    /// Returns removed project names.
    #[must_use]
    pub fn removed_paths(&self) -> Vec<ProjectName> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .removed
            .clone()
    }
}

#[async_trait]
impl LocalAgentClient for MockLocalAgentClient {
    async fn status(&self) -> Result<AgentStatusDocument, SpiritError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.status_calls = state.status_calls.saturating_add(1);
        if let Some(error) = &state.fail_status {
            return Err(error.clone());
        }
        Ok(state.status.clone())
    }

    async fn pair(&self, request: &AgentPairRequest) -> Result<AgentPairing, SpiritError> {
        request.validate()?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.pair_calls.push(request.clone());
        state
            .pair_result
            .clone()
            .map(AgentPairing::from_output)
            .ok_or_else(|| {
                SpiritError::new(
                    SpiritErrorCode::Internal,
                    "the mock local agent has no configured pairing result",
                )
            })
    }

    async fn paths(&self) -> Result<Vec<AgentPathMap>, SpiritError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.path_calls = state.path_calls.saturating_add(1);
        Ok(state.paths.clone())
    }

    async fn add_path(&self, request: &AgentPathAddRequest) -> Result<AgentPathMap, SpiritError> {
        let project = request.validate()?;
        let map = AgentPathMap {
            project,
            local_root: request.project_root.clone(),
            worktree_root: request.worktree_root.clone(),
            allow: request.allow.clone(),
        };
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .added
            .push(request.clone());
        Ok(map)
    }

    async fn remove_path(&self, project: &ProjectName) -> Result<(), SpiritError> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .removed
            .push(project.clone());
        Ok(())
    }
}
