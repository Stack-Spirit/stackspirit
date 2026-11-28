//! The searchable CLI & MCP tools hub.
//!
//! A declarative [`ToolRegistry`] merges built-in presets with custom user entries saved
//! as strict TOML under `~/.stackspirit/tools/<id>.toml`. An entry describes *how to
//! reach* a tool (binary, install and auth commands, MCP launch or URL) and never holds
//! a secret: unknown fields, credential-looking arguments and URLs with userinfo or
//! token query parameters are rejected. Per-tool target selection and independent Mac and
//! Home connection status live in [`ToolConnections`].

mod connections;
mod model;
mod presets;
mod store;
mod toml;

pub use connections::{ToolConnectionState, ToolConnections, ToolStatus, ToolTarget};
pub use model::{
    CliSpec, McpSpec, McpTransport, ToolCategory, ToolEntry, ToolFilter, ToolListView, ToolSurface,
};
pub use presets::builtin_tools;
pub use store::{ToolRegistry, default_tool_dir};
