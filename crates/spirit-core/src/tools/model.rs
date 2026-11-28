//! Tool entry types, validation and search.

use serde::{Deserialize, Serialize};

use crate::error::SpiritError;

/// Catalog category shown as a filter pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    /// Deploy & Cloud.
    DeployCloud,
    /// Code & Issues.
    CodeIssues,
    /// Communication & Calendar.
    CommunicationCalendar,
    /// Databases.
    Databases,
    /// Browser & Docs.
    BrowserDocs,
    /// Custom user entries.
    Custom,
}

impl ToolCategory {
    /// Every category in display order.
    pub const ALL: [ToolCategory; 6] = [
        Self::DeployCloud,
        Self::CodeIssues,
        Self::CommunicationCalendar,
        Self::Databases,
        Self::BrowserDocs,
        Self::Custom,
    ];

    /// Stable wire string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeployCloud => "deploy_cloud",
            Self::CodeIssues => "code_issues",
            Self::CommunicationCalendar => "communication_calendar",
            Self::Databases => "databases",
            Self::BrowserDocs => "browser_docs",
            Self::Custom => "custom",
        }
    }

    /// Human label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::DeployCloud => "Deploy & Cloud",
            Self::CodeIssues => "Code & Issues",
            Self::CommunicationCalendar => "Communication & Calendar",
            Self::Databases => "Databases",
            Self::BrowserDocs => "Browser & Docs",
            Self::Custom => "Custom",
        }
    }

    /// Parses the wire string.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == value)
    }
}

/// How an MCP server is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    /// Local child process over stdio.
    Stdio,
    /// Streamable HTTP endpoint.
    Http,
    /// Server-sent events endpoint.
    Sse,
}

impl McpTransport {
    /// Stable wire string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
            Self::Sse => "sse",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "stdio" => Some(Self::Stdio),
            "http" => Some(Self::Http),
            "sse" => Some(Self::Sse),
            _ => None,
        }
    }
}

/// Which surfaces an entry offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSurface {
    /// CLI only.
    Cli,
    /// MCP only.
    Mcp,
    /// Both a CLI and an MCP server.
    CliAndMcp,
}

/// CLI half of an entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliSpec {
    /// Executable name on `PATH`.
    pub binary: String,
    /// Copy-only install command.
    pub install_cmd: String,
    /// Copy-only sign-in command (empty when none).
    pub auth_cmd: String,
}

/// MCP half of an entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpSpec {
    /// Transport.
    pub transport: McpTransport,
    /// Stdio launch command (empty for HTTP/SSE).
    pub command: String,
    /// Stdio arguments.
    pub args: Vec<String>,
    /// Remote URL (empty for stdio).
    pub url: String,
}

/// One catalog entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolEntry {
    /// Lowercase slug, `[a-z0-9-]`, at most 48 characters.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Category.
    pub category: ToolCategory,
    /// One-line description.
    pub description: String,
    /// CLI half.
    pub cli: Option<CliSpec>,
    /// MCP half.
    pub mcp: Option<McpSpec>,
    /// True for a shipped preset.
    pub builtin: bool,
}

/// Search + category filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolFilter {
    /// Case-insensitive substring query over id, name, description and binary.
    pub query: String,
    /// Restrict to one category.
    pub category: Option<ToolCategory>,
}

/// Registry listing: entries plus safe warnings for skipped custom files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolListView {
    /// Built-ins first, then custom entries sorted by id.
    pub tools: Vec<ToolEntry>,
    /// One warning per skipped custom file. Never contains file contents.
    pub warnings: Vec<String>,
}

const SECRET_MARKERS: [&str; 9] = [
    "token=",
    "key=",
    "secret",
    "password",
    "passwd",
    "bearer ",
    "authorization",
    "apikey",
    "sk-",
];

fn looks_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    SECRET_MARKERS.iter().any(|marker| lower.contains(marker))
}

fn check_text(field: &str, value: &str, max: usize) -> Result<(), SpiritError> {
    if value.chars().count() > max {
        return Err(SpiritError::invalid_input(field, "is too long"));
    }
    if value.chars().any(char::is_control) {
        return Err(SpiritError::invalid_input(
            field,
            "must not contain control characters",
        ));
    }
    Ok(())
}

impl ToolEntry {
    /// Which surfaces this entry offers.
    #[must_use]
    pub fn surface(&self) -> ToolSurface {
        match (&self.cli, &self.mcp) {
            (Some(_), Some(_)) => ToolSurface::CliAndMcp,
            (Some(_), None) => ToolSurface::Cli,
            _ => ToolSurface::Mcp,
        }
    }

    /// Validates an id.
    pub fn validate_id(id: &str) -> Result<(), SpiritError> {
        if id.is_empty()
            || id.len() > 48
            || !id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            || id.starts_with('-')
        {
            return Err(SpiritError::invalid_input(
                "id",
                "must be 1-48 characters of a-z, 0-9 and '-'",
            ));
        }
        Ok(())
    }

    /// Validates the entry. Rejects anything that looks like an embedded secret.
    pub fn validate(&self) -> Result<(), SpiritError> {
        Self::validate_id(&self.id)?;
        if self.name.trim().is_empty() {
            return Err(SpiritError::invalid_input("name", "must not be empty"));
        }
        check_text("name", &self.name, 80)?;
        check_text("description", &self.description, 240)?;
        if self.cli.is_none() && self.mcp.is_none() {
            return Err(SpiritError::invalid_input(
                "tool",
                "needs a CLI or an MCP server",
            ));
        }
        if let Some(cli) = &self.cli {
            check_text("binary", &cli.binary, 80)?;
            if cli.binary.is_empty() || cli.binary.chars().any(|c| c.is_whitespace() || c == '/') {
                return Err(SpiritError::invalid_input(
                    "binary",
                    "must be a bare executable name",
                ));
            }
            check_text("install_cmd", &cli.install_cmd, 240)?;
            check_text("auth_cmd", &cli.auth_cmd, 240)?;
            for (field, value) in [
                ("install_cmd", &cli.install_cmd),
                ("auth_cmd", &cli.auth_cmd),
            ] {
                if looks_secret(value) {
                    return Err(SpiritError::invalid_input(
                        field,
                        "must not contain a secret",
                    ));
                }
            }
        }
        if let Some(mcp) = &self.mcp {
            check_text("command", &mcp.command, 160)?;
            check_text("url", &mcp.url, 300)?;
            match mcp.transport {
                McpTransport::Stdio => {
                    if mcp.command.trim().is_empty() {
                        return Err(SpiritError::invalid_input("command", "must not be empty"));
                    }
                    if !mcp.url.is_empty() {
                        return Err(SpiritError::invalid_input("url", "is not used by stdio"));
                    }
                }
                McpTransport::Http | McpTransport::Sse => {
                    if !mcp.command.is_empty() || !mcp.args.is_empty() {
                        return Err(SpiritError::invalid_input(
                            "command",
                            "is not used by HTTP or SSE",
                        ));
                    }
                    validate_url(&mcp.url)?;
                }
            }
            for arg in &mcp.args {
                check_text("args", arg, 200)?;
            }
            if mcp.args.len() > 32 {
                return Err(SpiritError::invalid_input("args", "has too many entries"));
            }
            if looks_secret(&mcp.command) || mcp.args.iter().any(|a| looks_secret(a)) {
                return Err(SpiritError::invalid_input(
                    "args",
                    "must not contain a secret; use the secret vault",
                ));
            }
        }
        Ok(())
    }

    /// Case-insensitive match of a query against this entry.
    #[must_use]
    pub fn matches(&self, filter: &ToolFilter) -> bool {
        if filter.category.is_some_and(|c| c != self.category) {
            return false;
        }
        let query = filter.query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return true;
        }
        let binary = self.cli.as_ref().map_or("", |c| c.binary.as_str());
        query.split_whitespace().all(|word| {
            [&self.id, &self.name, &self.description]
                .iter()
                .any(|f| f.to_ascii_lowercase().contains(word))
                || binary.to_ascii_lowercase().contains(word)
        })
    }
}

fn validate_url(url: &str) -> Result<(), SpiritError> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| SpiritError::invalid_input("url", "must start with https:// or http://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') || authority.contains(' ') {
        return Err(SpiritError::invalid_input(
            "url",
            "must have a host and no embedded credentials",
        ));
    }
    if url.contains('?') && looks_secret(url) {
        return Err(SpiritError::invalid_input(
            "url",
            "must not carry a token in the query",
        ));
    }
    Ok(())
}

pub(crate) fn transport_from_str(value: &str) -> Option<McpTransport> {
    McpTransport::parse(value)
}
