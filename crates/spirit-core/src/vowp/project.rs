//! One-step `vowp project add`: the request, its validation and the path-free outcome.
//!
//! VOWP prints a `ProjectBootstrap` that includes a home-side `path`. That path is
//! discarded at parse time so it can never reach a view, the bridge or the renderer.

use serde::{Deserialize, Serialize};

use crate::error::SpiritError;
use crate::view::OrchestratorView;
use crate::wire::{OrchestratorApp, OrchestratorCard, ProjectName};

const PATH_MAX_BYTES: usize = 4096;
const URL_MAX_BYTES: usize = 2048;

/// Where the new project comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProjectSource {
    /// Clone a Git repository's default branch (VOWP picks the clone target).
    Git {
        /// Repository URL without embedded credentials.
        url: String,
    },
    /// Adopt an existing folder on the home host.
    Folder {
        /// Absolute (or `~`-relative) home-side path.
        path: String,
    },
}

/// A request to add one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectAddRequest {
    /// The VOWP project name.
    pub name: String,
    /// Where the project comes from.
    pub source: ProjectSource,
    /// Whether to initialise VOWP in the project (always `--init` on the wire).
    #[serde(default)]
    pub init: bool,
    /// Orchestrator app to create alongside the project, when any.
    #[serde(default)]
    pub orchestrator: Option<OrchestratorApp>,
}

impl ProjectAddRequest {
    /// Validates every value that reaches argv.
    pub fn validate(&self) -> Result<(), SpiritError> {
        ProjectName::parse(&self.name)
            .map_err(|_| SpiritError::invalid_input("project name", "must be a valid slug"))?;
        match &self.source {
            ProjectSource::Git { url } => validate_git_url(url),
            ProjectSource::Folder { path } => validate_folder_path(path),
        }
    }
}

fn validate_git_url(url: &str) -> Result<(), SpiritError> {
    if url.is_empty()
        || url.len() > URL_MAX_BYTES
        || url.starts_with('-')
        || url.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(SpiritError::invalid_input(
            "git url",
            "must be a non-empty single-token URL that does not start with '-'",
        ));
    }
    if let Some((scheme, rest)) = url.split_once("://") {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        if let Some((userinfo, _)) = authority.rsplit_once('@') {
            let web = matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https");
            if userinfo.contains(':') || web {
                return Err(SpiritError::invalid_input(
                    "git url",
                    "must not embed credentials; use a credential helper or SSH key",
                ));
            }
        }
    }
    Ok(())
}

fn validate_folder_path(path: &str) -> Result<(), SpiritError> {
    let absolute_ish = path.starts_with('/') || path == "~" || path.starts_with("~/");
    if path.len() > PATH_MAX_BYTES
        || path == "/"
        || !absolute_ish
        || path.chars().any(char::is_control)
    {
        return Err(SpiritError::invalid_input(
            "project folder",
            "must be an absolute path (or start with ~/) of at most 4096 bytes",
        ));
    }
    Ok(())
}

/// The path-free result of `project add`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectAddOutcome {
    /// Project name as VOWP registered it.
    pub project: String,
    /// Whether VOWP initialised the project.
    pub initialized: bool,
    /// The orchestrator VOWP created, when one was requested.
    pub orchestrator: Option<OrchestratorView>,
}

#[derive(Deserialize)]
struct ProjectBootstrap {
    project: String,
    #[serde(default, rename = "path")]
    _path: Option<serde::de::IgnoredAny>,
    initialized: bool,
    #[serde(default)]
    orchestrator: Option<OrchestratorCard>,
}

/// Strictly parses `vowp --json project add` output and discards the home path.
pub fn parse_project_bootstrap(stdout: &[u8]) -> Result<ProjectAddOutcome, SpiritError> {
    let parsed: ProjectBootstrap =
        serde_json::from_slice(stdout).map_err(|err| SpiritError::parse("project add", err))?;
    let project = ProjectName::parse(&parsed.project)
        .map_err(|err| SpiritError::parse("project add", err.message))?;
    Ok(ProjectAddOutcome {
        project: project.as_str().to_owned(),
        initialized: parsed.initialized,
        orchestrator: parsed
            .orchestrator
            .as_ref()
            .map(OrchestratorView::from_card),
    })
}
