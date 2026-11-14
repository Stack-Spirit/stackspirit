//! `surface`: create, list and revoke the home side of a paired local surface.
//!
//! Spec: `docs/design/v0.4/local-orchestrator.md` §4.3, §4.6 and §12.1. The commands are
//! home-only and use the global store. `surface pair` creates a `pending` surface and prints
//! a ten-minute one-time code exactly once; `surface ls` reports state, scopes and expiry but
//! never a token; `surface revoke` revokes every token and marks the surface revoked.
//!
//! The v4 store read model returns one surface by id. This module keeps the ids it has
//! created in `<home>/surfaces.json` (surface ids are not secrets) and re-reads the surface,
//! its state, its scopes and its expiry from the store for every listing.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

use serde::Serialize;
use vowp_core::agent::pair::{create_pending_surface, revoke_surface};
use vowp_protocol::{NodeName, SessionId, SurfaceId, SurfaceInfo, SurfaceKind, SurfaceScope, SurfaceState, Timestamp};
use vowp_store::{Store, SurfaceToken};

use crate::cli::{SurfaceCommand, SurfaceLsArgs, SurfacePairArgs, SurfaceRevokeArgs};
use crate::node_ctx::NodeRole;
use crate::output::{Table, local_time_at, print_json};
use crate::{CliError, Ctx, IoStreams};

/// Private file holding the surface ids this home CLI created.
const SURFACE_INDEX_FILE: &str = "surfaces.json";

/// Runs one surface subcommand. `node` is the global `--node <name>` value: for these
/// commands it names the surface's node, not an SSH redirect.
pub fn run(ctx: &Ctx, io: &mut dyn IoStreams, command: &SurfaceCommand, node: Option<&str>) -> Result<i32, CliError> {
    match command {
        SurfaceCommand::Pair(args) => pair(ctx, io, args, node),
        SurfaceCommand::Ls(args) => list(ctx, io, args, node),
        SurfaceCommand::Revoke(args) => revoke(ctx, io, args),
    }
}

fn pair(ctx: &Ctx, io: &mut dyn IoStreams, args: &SurfacePairArgs, node: Option<&str>) -> Result<i32, CliError> {
    require_home(ctx)?;
    let node = node
        .ok_or_else(|| CliError::Invalid("surface pair needs --node <name> to name the surface's node".to_owned()))?;
    let node = NodeName::parse(node).map_err(|_| CliError::Invalid(format!("invalid node name {node:?}")))?;
    let session = SessionId::parse(&args.session)
        .map_err(|_| CliError::Invalid(format!("invalid session id {:?}", args.session)))?;
    let kind = parse_kind(&args.kind)?;
    let store = ctx.fleet_store()?;
    if let Some(existing) = pending_duplicate(&store, ctx, &node, &session, kind, args.app.as_deref())? {
        return Err(CliError::Conflict(format!(
            "surface {} is already pending for session {session}; revoke it before pairing again",
            existing.id
        )));
    }
    let pending = create_pending_surface(&store, &node, &session, kind, args.app.as_deref(), ctx.clock().now())?;
    append_index(ctx, &pending.surface.id)?;

    let command = agent_pair_command(&pending.surface.id, &session, args.app.as_deref());
    let output = SurfacePairOutput {
        surface: pending.surface.id.clone(),
        node: node.clone(),
        session: session.clone(),
        kind,
        app: args.app.clone(),
        code: pending.code.clone(),
        expires_at: pending.expires_at,
        command: command.clone(),
    };
    if ctx.json() {
        print_json(io, &output)?;
    } else {
        writeln!(
            io.out(),
            "created pending surface {} on {node} ({})",
            pending.surface.id,
            args.kind
        )?;
        writeln!(io.out(), "session: {session}")?;
        writeln!(io.out(), "code: {}", pending.code)?;
        writeln!(io.out(), "expires: {}", pending.expires_at.as_rfc3339())?;
        writeln!(io.out(), "run on the Mac:")?;
        writeln!(io.out(), "  {command}")?;
    }
    Ok(0)
}

fn list(ctx: &Ctx, io: &mut dyn IoStreams, _args: &SurfaceLsArgs, node: Option<&str>) -> Result<i32, CliError> {
    require_home(ctx)?;
    let filter = match node {
        Some(value) => {
            Some(NodeName::parse(value).map_err(|_| CliError::Invalid(format!("invalid node name {value:?}")))?)
        }
        None => None,
    };
    let store = ctx.fleet_store()?;
    let mut entries = Vec::new();
    for id in read_index(ctx)? {
        let Some(surface) = store.surface_by_id(&id)? else {
            continue;
        };
        if filter.as_ref().is_some_and(|node| surface.node != *node) {
            continue;
        }
        let tokens = store.list_surface_tokens(&id)?;
        entries.push(SurfaceEntry::from_store(surface, &tokens));
    }
    entries.sort_by(|left, right| left.id.0.cmp(&right.id.0));

    if ctx.json() {
        print_json(io, &entries)?;
        return Ok(0);
    }
    let now = ctx.clock().now();
    let mut table = Table::new(&["ID", "NODE", "KIND", "STATE", "SCOPES", "EXPIRES"]);
    for entry in &entries {
        table.row(vec![
            entry.id.to_string(),
            entry.node.to_string(),
            wire_name(&entry.kind),
            wire_name(&entry.state),
            join_or_dash(&entry.scopes),
            entry
                .expires_at
                .map(|expiry| local_time_at(expiry, now, ctx.tz_offset_minutes()))
                .unwrap_or_else(|| "-".to_owned()),
        ]);
    }
    writeln!(io.out(), "{}", table.render(ctx.color()))?;
    Ok(0)
}

fn revoke(ctx: &Ctx, io: &mut dyn IoStreams, args: &SurfaceRevokeArgs) -> Result<i32, CliError> {
    require_home(ctx)?;
    let id = parse_surface_id(&args.id)?;
    if !args.yes {
        return Err(CliError::Guard(format!(
            "refusing to revoke surface {id} without --yes"
        )));
    }
    let store = ctx.fleet_store()?;
    let revoked = revoke_surface(&store, &id, ctx.clock().now())?;
    append_index(ctx, &id)?;
    if ctx.json() {
        print_json(
            io,
            &SurfaceRevokeOutput {
                surface: id.clone(),
                status: "revoked".to_owned(),
                tokens_revoked: revoked,
            },
        )?;
    } else {
        writeln!(io.out(), "revoked surface {id}; {revoked} token(s) revoked")?;
    }
    Ok(0)
}

/// Returns the exact Mac command for one pending surface. The code is never part of it.
fn agent_pair_command(surface: &SurfaceId, session: &SessionId, app: Option<&str>) -> String {
    let mut command = format!("vowp agent pair --code-file - --surface {surface} --session {session}");
    if let Some(app) = app {
        command.push_str(&format!(" --app {app}"));
    }
    command
}

fn parse_kind(value: &str) -> Result<SurfaceKind, CliError> {
    match value {
        "opencode" => Ok(SurfaceKind::Opencode),
        "chat" => Ok(SurfaceKind::Chat),
        other => Err(CliError::Invalid(format!(
            "invalid surface kind {other:?}; use opencode or chat"
        ))),
    }
}

fn parse_surface_id(value: &str) -> Result<SurfaceId, CliError> {
    if value.len() == 26 && value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        Ok(SurfaceId(value.to_owned()))
    } else {
        Err(CliError::Invalid(format!(
            "invalid surface id {value:?}; expected a 26-character ULID"
        )))
    }
}

fn require_home(ctx: &Ctx) -> Result<(), CliError> {
    match ctx.role() {
        NodeRole::Home => Ok(()),
        NodeRole::Control => Err(CliError::Guard(
            "surface commands need the home store; run them on the home machine".to_owned(),
        )),
        NodeRole::Uninitialised => Err(CliError::NotFound(
            "vowp home is not initialised; run `vowp setup` or `vowp init`".to_owned(),
        )),
    }
}

fn index_path(ctx: &Ctx) -> PathBuf {
    ctx.paths().root().join(SURFACE_INDEX_FILE)
}

fn read_index(ctx: &Ctx) -> Result<Vec<SurfaceId>, CliError> {
    let path = index_path(ctx);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(CliError::Io(error)),
    };
    serde_json::from_str(&text).map_err(|error| CliError::Protocol(vowp_protocol::ProtocolError::from(error)))
}

fn write_index(ctx: &Ctx, ids: &[SurfaceId]) -> Result<(), CliError> {
    let path = index_path(ctx);
    let bytes = serde_json::to_vec_pretty(ids).map_err(vowp_protocol::ProtocolError::from)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

/// Returns an existing pending surface with the same node, session, kind and app.
///
/// A repeated `surface pair` for one pending surface would create a second code the owner
/// did not ask for; it is a conflict (exit 4). Revoking the pending surface is the documented
/// way to start over.
fn pending_duplicate(
    store: &Store,
    ctx: &Ctx,
    node: &NodeName,
    session: &SessionId,
    kind: SurfaceKind,
    app: Option<&str>,
) -> Result<Option<SurfaceInfo>, CliError> {
    for id in read_index(ctx)? {
        let Some(surface) = store.surface_by_id(&id)? else {
            continue;
        };
        if surface.state != SurfaceState::Pending
            || surface.node != *node
            || surface.kind != kind
            || surface.app.as_deref() != app
        {
            continue;
        }
        if surface.metadata.get("session").and_then(serde_json::Value::as_str) == Some(session.as_str()) {
            return Ok(Some(surface));
        }
    }
    Ok(None)
}

fn append_index(ctx: &Ctx, id: &SurfaceId) -> Result<(), CliError> {
    let mut ids = read_index(ctx)?;
    if !ids.contains(id) {
        ids.push(id.clone());
        write_index(ctx, &ids)?;
    }
    Ok(())
}

fn join_or_dash(scopes: &[SurfaceScope]) -> String {
    if scopes.is_empty() {
        "-".to_owned()
    } else {
        scopes.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")
    }
}

fn wire_name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "-".to_owned())
}

/// One `surface ls` row: the read model plus its token scopes and active expiry.
#[derive(Debug, Serialize)]
struct SurfaceEntry {
    id: SurfaceId,
    node: NodeName,
    kind: SurfaceKind,
    state: SurfaceState,
    scopes: Vec<SurfaceScope>,
    expires_at: Option<Timestamp>,
}

impl SurfaceEntry {
    fn from_store(surface: SurfaceInfo, tokens: &[SurfaceToken]) -> Self {
        let mut scopes: Vec<SurfaceScope> = Vec::new();
        for scope in tokens.iter().flat_map(|token| token.scopes.iter()) {
            if !scopes.contains(scope) {
                scopes.push(scope.clone());
            }
        }
        let expires_at = tokens
            .iter()
            .filter(|token| token.revoked_at.is_none())
            .filter_map(|token| token.expires_at)
            .max();
        Self {
            id: surface.id,
            node: surface.node,
            kind: surface.kind,
            state: surface.state,
            scopes,
            expires_at,
        }
    }
}

/// JSON shape of `vowp surface pair`.
#[derive(Debug, Serialize)]
struct SurfacePairOutput {
    surface: SurfaceId,
    node: NodeName,
    session: SessionId,
    kind: SurfaceKind,
    app: Option<String>,
    code: String,
    expires_at: Timestamp,
    command: String,
}

/// JSON shape of `vowp surface revoke`.
#[derive(Debug, Serialize)]
struct SurfaceRevokeOutput {
    surface: SurfaceId,
    status: String,
    tokens_revoked: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_pair_command_carries_surface_session_and_app_but_never_a_code() {
        let surface = SurfaceId("01ARZ3NDEKTSV4RRFFQ69G5FAW".to_owned());
        let session = SessionId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("session");
        let command = agent_pair_command(&surface, &session, Some("opencode"));
        assert_eq!(
            command,
            "vowp agent pair --code-file - --surface 01ARZ3NDEKTSV4RRFFQ69G5FAW \
             --session 01ARZ3NDEKTSV4RRFFQ69G5FAV --app opencode"
        );
        assert!(!command.contains("code:"), "{command}");
    }

    #[test]
    fn parse_surface_id_rejects_a_path_traversal_value() {
        assert!(parse_surface_id("../../etc/passwd").is_err());
        assert!(parse_surface_id("").is_err());
        assert!(parse_surface_id("01ARZ3NDEKTSV4RRFFQ69G5FAW").is_ok());
    }
}
