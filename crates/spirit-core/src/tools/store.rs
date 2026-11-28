//! Custom tool persistence: one strict TOML file per entry under
//! `~/.stackspirit/tools/<id>.toml`. Writes are atomic and owner-only on Unix.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{SpiritError, SpiritErrorCode};
use crate::tools::model::{
    CliSpec, McpSpec, McpTransport, ToolCategory, ToolEntry, ToolFilter, ToolListView,
    transport_from_str,
};
use crate::tools::presets::builtin_tools;
use crate::tools::toml::{TomlTable, TomlValue, parse_toml};

/// Resolves `~/.stackspirit/tools`.
pub fn default_tool_dir() -> Result<PathBuf, SpiritError> {
    let home = std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
        .ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::Io,
                "could not resolve the home directory for tools",
            )
        })?;
    Ok(PathBuf::from(home).join(".stackspirit").join("tools"))
}

/// Built-in presets plus custom entries.
#[derive(Debug, Clone)]
pub struct ToolRegistry {
    dir: PathBuf,
}

impl ToolRegistry {
    /// Registry at an explicit directory.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Registry at `~/.stackspirit/tools`.
    pub fn for_home() -> Result<Self, SpiritError> {
        Ok(Self::new(default_tool_dir()?))
    }

    /// The custom directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Lists built-ins then valid custom entries. A broken file becomes a warning.
    pub fn list(&self) -> Result<ToolListView, SpiritError> {
        let mut warnings = Vec::new();
        let mut customs = Vec::new();
        if self.dir.exists() {
            let entries =
                fs::read_dir(&self.dir).map_err(|e| SpiritError::io("reading tool entries", &e))?;
            for item in entries {
                let Ok(item) = item else {
                    warnings.push("A tool file could not be read".to_owned());
                    continue;
                };
                let path = item.path();
                if path.extension().and_then(|v| v.to_str()) != Some("toml") {
                    continue;
                }
                let name = path.file_name().map_or_else(
                    || "tool.toml".to_owned(),
                    |v| v.to_string_lossy().into_owned(),
                );
                match load_file(&path) {
                    Ok(entry) if is_builtin_id(&entry.id) => warnings.push(format!(
                        "{name}: id '{}' is reserved for a built-in tool",
                        entry.id
                    )),
                    Ok(entry) => customs.push(entry),
                    Err(e) => warnings.push(format!("{name}: {}", e.message)),
                }
            }
        }
        customs.sort_by(|a, b| a.id.cmp(&b.id));
        let mut tools = builtin_tools();
        tools.extend(customs);
        Ok(ToolListView { tools, warnings })
    }

    /// Search and category filter over the full list.
    pub fn search(&self, filter: &ToolFilter) -> Result<Vec<ToolEntry>, SpiritError> {
        Ok(self
            .list()?
            .tools
            .into_iter()
            .filter(|t| t.matches(filter))
            .collect())
    }

    /// Saves a custom entry. Forces the Custom category semantics: `builtin` must be
    /// false and the id must not shadow a preset.
    pub fn save_custom(&self, entry: ToolEntry) -> Result<ToolEntry, SpiritError> {
        if entry.builtin {
            return Err(SpiritError::invalid_input(
                "builtin",
                "a custom tool must have builtin = false",
            ));
        }
        if is_builtin_id(&entry.id) {
            return Err(SpiritError::invalid_input(
                "id",
                "is reserved for a built-in tool",
            ));
        }
        if entry.category != ToolCategory::Custom {
            return Err(SpiritError::invalid_input(
                "category",
                "a custom tool must use the custom category",
            ));
        }
        entry.validate()?;
        fs::create_dir_all(&self.dir)
            .map_err(|e| SpiritError::io("creating the tools directory", &e))?;
        restrict_directory(&self.dir);
        let path = self.dir.join(format!("{}.toml", entry.id));
        let tmp = self.dir.join(format!("{}.toml.tmp", entry.id));
        write_atomic(&tmp, &path, to_toml(&entry).as_bytes())?;
        Ok(entry)
    }

    /// Deletes a custom entry; `false` when absent. Built-ins are refused.
    pub fn delete_custom(&self, id: &str) -> Result<bool, SpiritError> {
        ToolEntry::validate_id(id)?;
        if is_builtin_id(id) {
            return Err(SpiritError::invalid_input(
                "id",
                "is a built-in tool and cannot be deleted",
            ));
        }
        let path = self.dir.join(format!("{id}.toml"));
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(&path).map_err(|e| SpiritError::io("deleting the tool entry", &e))?;
        Ok(true)
    }
}

fn is_builtin_id(id: &str) -> bool {
    builtin_tools().iter().any(|t| t.id == id)
}

fn esc(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn push(out: &mut String, key: &str, value: &str) {
    out.push_str(&format!("{key} = \"{}\"\n", esc(value)));
}

fn to_toml(entry: &ToolEntry) -> String {
    let mut out = String::new();
    push(&mut out, "id", &entry.id);
    push(&mut out, "name", &entry.name);
    push(&mut out, "category", entry.category.as_str());
    push(&mut out, "description", &entry.description);
    if let Some(cli) = &entry.cli {
        push(&mut out, "cli_binary", &cli.binary);
        push(&mut out, "cli_install_cmd", &cli.install_cmd);
        push(&mut out, "cli_auth_cmd", &cli.auth_cmd);
    }
    if let Some(mcp) = &entry.mcp {
        push(&mut out, "mcp_transport", mcp.transport.as_str());
        push(&mut out, "mcp_command", &mcp.command);
        let args = mcp
            .args
            .iter()
            .map(|a| format!("\"{}\"", esc(a)))
            .collect::<Vec<_>>();
        out.push_str(&format!("mcp_args = [{}]\n", args.join(", ")));
        push(&mut out, "mcp_url", &mcp.url);
    }
    out.push_str("builtin = false\n");
    out
}

fn string(table: &mut TomlTable, key: &str) -> Result<Option<String>, SpiritError> {
    match table.remove(key) {
        None => Ok(None),
        Some(TomlValue::String(v)) => Ok(Some(v)),
        Some(_) => Err(SpiritError::invalid_input(key, "must be a string")),
    }
}

fn required(table: &mut TomlTable, key: &str) -> Result<String, SpiritError> {
    string(table, key)?.ok_or_else(|| SpiritError::invalid_input(key, "is required"))
}

fn from_toml(input: &str) -> Result<ToolEntry, SpiritError> {
    let mut t = parse_toml(input)?;
    let id = required(&mut t, "id")?;
    let name = required(&mut t, "name")?;
    let category = ToolCategory::parse(&required(&mut t, "category")?)
        .ok_or_else(|| SpiritError::invalid_input("category", "is not a known category"))?;
    let description = string(&mut t, "description")?.unwrap_or_default();
    match t.remove("builtin") {
        None | Some(TomlValue::Bool(false)) => {}
        Some(_) => return Err(SpiritError::invalid_input("builtin", "must be false")),
    }
    let cli = match string(&mut t, "cli_binary")? {
        Some(binary) => Some(CliSpec {
            binary,
            install_cmd: string(&mut t, "cli_install_cmd")?.unwrap_or_default(),
            auth_cmd: string(&mut t, "cli_auth_cmd")?.unwrap_or_default(),
        }),
        None => None,
    };
    let mcp = match string(&mut t, "mcp_transport")? {
        Some(raw) => {
            let transport: McpTransport = transport_from_str(&raw).ok_or_else(|| {
                SpiritError::invalid_input("mcp_transport", "must be stdio, http or sse")
            })?;
            let args = match t.remove("mcp_args") {
                None => Vec::new(),
                Some(TomlValue::StringArray(v)) => v,
                Some(_) => {
                    return Err(SpiritError::invalid_input(
                        "mcp_args",
                        "must be a string array",
                    ));
                }
            };
            Some(McpSpec {
                transport,
                command: string(&mut t, "mcp_command")?.unwrap_or_default(),
                args,
                url: string(&mut t, "mcp_url")?.unwrap_or_default(),
            })
        }
        None => None,
    };
    if let Some(unknown) = t.keys().next() {
        return Err(SpiritError::invalid_input(
            unknown,
            "is not a supported field",
        ));
    }
    let entry = ToolEntry {
        id,
        name,
        category,
        description,
        cli,
        mcp,
        builtin: false,
    };
    entry.validate()?;
    Ok(entry)
}

fn load_file(path: &Path) -> Result<ToolEntry, SpiritError> {
    let bytes = fs::read(path).map_err(|e| SpiritError::io("reading a tool entry", &e))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| SpiritError::parse("tool entry", "file is not valid UTF-8"))?;
    let entry = from_toml(text)?;
    let stem = path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or_default();
    if entry.id != stem {
        return Err(SpiritError::invalid_input("id", "must match the file name"));
    }
    Ok(entry)
}

fn write_atomic(tmp: &Path, dest: &Path, bytes: &[u8]) -> Result<(), SpiritError> {
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options
            .open(tmp)
            .map_err(|e| SpiritError::io("creating a tool entry", &e))?;
        f.write_all(bytes)
            .map_err(|e| SpiritError::io("writing a tool entry", &e))?;
        f.sync_all()
            .map_err(|e| SpiritError::io("syncing a tool entry", &e))?;
    }
    fs::rename(tmp, dest).map_err(|e| SpiritError::io("replacing the tool entry", &e))
}

#[cfg(unix)]
fn restrict_directory(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) {}
