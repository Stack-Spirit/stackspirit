//! `stackspirit backup push|list|restore`: client-side encrypted off-site backups.
//!
//! The historical local `stackspirit backup` forms are unchanged: this front-end delegates
//! every command it does not own to [`vowp_cli::run`], which is the same entrypoint the
//! `vowp` compatibility binary uses. Only the three encrypted R2 subcommands are handled
//! here.
//!
//! A push writes the normal, verified home backup into a private temporary directory with
//! [`vowp_core::backup`], packs that directory into a path-safe archive, encrypts the archive
//! with `age` to a public recipient, and uploads only the ciphertext. A restore downloads
//! the ciphertext, verifies its SHA-256, decrypts it locally, unpacks it into a private
//! temporary directory, and lets [`vowp_core::lifecycle::backup::restore`] verify every file hash and row count
//! before swapping a fresh home into place. The API never receives a plaintext backup, a
//! session token in a URL, or a private age identity.
//!
//! ## API contract
//!
//! The endpoints are the cloud Worker's per-machine R2 routes, authenticated with the account's
//! cloud session token:
//!
//! - `PUT /v1/machines/{machine_id}/backups` with `content-type: application/octet-stream` and
//!   `x-content-sha256: <64 lowercase hex>` stores the age ciphertext. The Worker names the
//!   object `<UTC timestamp>.tar.age`, returns `{name, size, sha256, created_at, pruned}` and
//!   rejects non-age bodies, a mismatched digest and bodies over 64 MiB.
//! - `GET /v1/machines/{machine_id}/backups` returns `{backups: [{name, size, sha256,
//!   created_at}]}`, newest first.
//! - `GET /v1/machines/{machine_id}/backups/{name}` returns the raw ciphertext.
//!
//! The machine id is read from `<home>/relay.json` (the enrollment record written by
//! `stackspirit relay enroll`) unless `--machine <id|name>` names another machine in the
//! account. The record itself, the session token and the age identity are never uploaded or
//! printed.
//!
//! The `vowp` and `stackspirit` binaries both point at this file so they remain the same
//! command-line surface. It is a binary target because the command module directory and its
//! dispatcher are outside this job's owned paths; a later job can fold this handler into
//! `cmd/mod.rs` once that file is owned.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use vowp_cli::cli::{
    BackupArgs, RemoteBackupCommand, RemoteBackupListArgs, RemoteBackupPushArgs, RemoteBackupRestoreArgs,
};
use vowp_cli::{CliError, Command, Ctx, Global, IoStreams, RealIo};
use vowp_core::BackupOptions;

/// Default cloud API, also used by `login`.
const DEFAULT_API_URL: &str = "https://api.stackspirit.io";

/// Marker that starts every local backup archive before encryption.
const ARCHIVE_MAGIC: &[u8] = b"stackspirit-backup-archive/1\n";

/// Maximum accepted archive path length, in bytes.
const MAX_ARCHIVE_PATH: usize = 4096;

/// Maximum accepted HTTP response body for a JSON API response.
const MAX_JSON_BYTES: u64 = 4 * 1024 * 1024;

/// Maximum accepted downloaded ciphertext size (16 GiB).
const MAX_DOWNLOAD_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// The cloud Worker's per-object limit (`src/backups/index.ts`).
const MAX_CLOUD_BACKUP_BYTES: u64 = 64 * 1024 * 1024;

/// Runs the StackSpirit front-end and returns the process exit code.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut io = RealIo::new();

    let cli = match vowp_cli::Cli::try_parse_from(args.iter().cloned()) {
        Ok(cli) => cli,
        // Help, version and parse errors are rendered by the library entrypoint so the
        // remote and local spellings keep exactly the same diagnostics.
        Err(_) => return delegate(args, &mut io),
    };

    let Command::Backup(backup) = &cli.command else {
        return delegate(args, &mut io);
    };
    let Some(remote) = backup.remote.as_deref() else {
        return delegate(args, &mut io);
    };

    let code = match run_remote(&cli.global, backup, remote, &mut io) {
        Ok(code) => code,
        Err(error) => {
            vowp_cli::output::print_error(&mut io, &error, cli.global.json);
            error.exit_code()
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

/// Delegates one command to the unchanged CLI entrypoint.
fn delegate(args: Vec<String>, io: &mut RealIo) -> ExitCode {
    let code = vowp_cli::run(args, io);
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

/// Dispatches one encrypted backup subcommand.
fn run_remote(
    global: &Global,
    backup: &BackupArgs,
    command: &RemoteBackupCommand,
    io: &mut dyn IoStreams,
) -> Result<i32, CliError> {
    if global.node.is_some() {
        return Err(CliError::Invalid(
            "backup push|list|restore runs against the local home; drop --node".to_owned(),
        ));
    }
    reject_local_flags(backup)?;
    let ctx = Ctx::build(global, io)?;
    match command {
        RemoteBackupCommand::Push(args) => run_push(&ctx, io, args),
        RemoteBackupCommand::List(args) => run_list(&ctx, io, args),
        RemoteBackupCommand::Restore(args) => run_restore(&ctx, io, args),
    }
}

/// Refuses mixing the historical local flags with a remote subcommand.
fn reject_local_flags(args: &BackupArgs) -> Result<(), CliError> {
    if args.to.is_some()
        || args.offsite.is_some()
        || args.encrypt.is_some()
        || args.restore.is_some()
        || args.to_home.is_some()
        || args.force
    {
        return Err(CliError::Invalid(
            "local backup flags cannot be combined with `backup push|list|restore`; run `backup` for a local backup"
                .to_owned(),
        ));
    }
    Ok(())
}

/// Archives, encrypts and uploads one backup.
fn run_push(ctx: &Ctx, io: &mut dyn IoStreams, args: &RemoteBackupPushArgs) -> Result<i32, CliError> {
    if !ctx.paths().db_path().is_file() {
        return Err(CliError::NotFound(format!(
            "{} home {} is not initialised; run `{} init` or pass --home",
            brand(),
            ctx.paths().root().display(),
            brand()
        )));
    }
    let session = CloudSession::load(ctx.paths().root())?;
    let api = Api::new(&resolve_api_url(args.api_url.as_deref(), &session)?)?;
    let machine = resolve_machine(&api, &session.access_token, args.machine.as_deref(), ctx.paths().root())?;
    let recipient = resolve_recipient(args.recipient.as_deref())?;
    let work = WorkDir::create(&ctx.paths().tmp_dir())?;

    let backup_dir = work.path.join("backup");
    let options = BackupOptions {
        to: Some(backup_dir.display().to_string()),
        ..BackupOptions::default()
    };
    let store = ctx.fleet_store()?;
    let report = vowp_core::backup(ctx.deps(), &store, &options)?;

    let archive = work.path.join("backup.archive");
    write_archive(&backup_dir, &archive)?;
    let ciphertext = work.path.join("backup.age");
    encrypt_file(&recipient, &archive, &ciphertext)?;

    let bytes = fs::metadata(&ciphertext)?.len();
    if bytes > MAX_CLOUD_BACKUP_BYTES {
        return Err(CliError::Limited(format!(
            "the encrypted backup is {bytes} bytes; the cloud backup add-on accepts at most 64 MiB"
        )));
    }
    let sha256 = hash_hex(&sha256_file(&ciphertext)?);
    let file = File::open(&ciphertext)?;
    let row = api.put_backup(&session.access_token, &machine, &sha256, &file)?;

    if ctx.json() {
        vowp_cli::output::print_json(
            io,
            &json!({
                "backup": row,
                "machine": machine,
                "plaintext_bytes": report.bytes,
                "files": report.files.len(),
                "skipped": report.skipped,
            }),
        )?;
    } else {
        writeln!(io.out(), "pushed {} ({} bytes, sha256 {sha256})", row.name, row.size)?;
        writeln!(io.out(), "machine: {machine}")?;
        writeln!(
            io.out(),
            "local snapshot: {} files, {} bytes; only the age ciphertext left this machine",
            report.files.len(),
            report.bytes
        )?;
        for skipped in &report.skipped {
            writeln!(io.err(), "skipped: {skipped}")?;
        }
    }
    Ok(0)
}

/// Lists the machine's encrypted R2 backups.
fn run_list(ctx: &Ctx, io: &mut dyn IoStreams, args: &RemoteBackupListArgs) -> Result<i32, CliError> {
    let session = CloudSession::load(ctx.paths().root())?;
    let api = Api::new(&resolve_api_url(args.api_url.as_deref(), &session)?)?;
    let machine = resolve_machine(&api, &session.access_token, args.machine.as_deref(), ctx.paths().root())?;
    let mut rows = api.list_backups(&session.access_token, &machine)?;
    if let Some(limit) = args.limit {
        rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    }

    if ctx.json() {
        vowp_cli::output::print_json(io, &json!({ "backups": rows, "machine": machine }))?;
    } else if rows.is_empty() {
        writeln!(io.out(), "no encrypted backups for {machine}")?;
    } else {
        writeln!(io.out(), "{:<32} {:<24} {:>12}", "NAME", "CREATED", "BYTES")?;
        for row in &rows {
            writeln!(io.out(), "{:<32} {:<24} {:>12}", row.name, row.created_at, row.size)?;
        }
    }
    Ok(0)
}

/// Downloads, decrypts, verifies and restores one backup.
fn run_restore(ctx: &Ctx, io: &mut dyn IoStreams, args: &RemoteBackupRestoreArgs) -> Result<i32, CliError> {
    let session = CloudSession::load(ctx.paths().root())?;
    let api = Api::new(&resolve_api_url(args.api_url.as_deref(), &session)?)?;
    let machine = resolve_machine(&api, &session.access_token, args.machine.as_deref(), ctx.paths().root())?;
    let to_home = args
        .to_home
        .clone()
        .ok_or_else(|| CliError::Invalid("restore needs --to-home <fresh-home>".to_owned()))?;
    let identity = resolve_identity(args.identity.as_deref())?;

    let rows = api.list_backups(&session.access_token, &machine)?;
    let row = match args.id.as_deref() {
        Some(name) => {
            validate_backup_name(name)?;
            rows.into_iter()
                .find(|row| row.name == name)
                .ok_or_else(|| CliError::NotFound(format!("backup {name} on machine {machine}")))?
        }
        None => rows
            .into_iter()
            .next()
            .ok_or_else(|| CliError::NotFound(format!("no encrypted backup on machine {machine}")))?,
    };

    let work = WorkDir::create(&ctx.paths().tmp_dir())?;
    let ciphertext = work.path.join("backup.age");
    api.download_backup(&session.access_token, &machine, &row.name, &ciphertext)?;
    verify_download(&row, &ciphertext)?;

    let archive = work.path.join("backup.archive");
    decrypt_file(&identity, &ciphertext, &archive)?;
    let backup_dir = work.path.join("backup");
    extract_archive(&archive, &backup_dir)?;

    let options = BackupOptions {
        restore: Some(backup_dir),
        to_home: Some(to_home.clone()),
        force: args.force,
        ..BackupOptions::default()
    };
    let report = vowp_core::lifecycle::backup::restore(ctx.deps(), &options)?;

    if ctx.json() {
        vowp_cli::output::print_json(
            io,
            &json!({
                "backup": row,
                "machine": machine,
                "to_home": to_home.display().to_string(),
                "files": report.files.len(),
                "bytes": report.bytes,
            }),
        )?;
    } else {
        writeln!(
            io.out(),
            "restored {} from {machine} into {} ({} files, {} bytes)",
            row.name,
            to_home.display(),
            report.files.len(),
            report.bytes
        )?;
    }
    Ok(0)
}

/// One cloud session, read from `<home>/cloud-session.json`.
#[derive(Debug, Deserialize)]
struct CloudSession {
    /// API base recorded by `login`.
    #[serde(default)]
    api_url: String,
    /// Access token; never rendered.
    access_token: String,
}

impl CloudSession {
    /// Loads the session file, enforcing the same private mode as `login`.
    fn load(home: &Path) -> Result<Self, CliError> {
        let path = home.join("cloud-session.json");
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(not_signed_in()),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() {
            return Err(CliError::Auth(format!(
                "the cloud session file {} must not be a symlink; run `{} login` again",
                path.display(),
                brand()
            )));
        }
        use std::os::unix::fs::PermissionsExt as _;
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(CliError::Auth(format!(
                "the cloud session file {} is mode {mode:03o}; it must be 0600. Run `{} login` again",
                path.display(),
                brand()
            )));
        }
        let text = fs::read_to_string(&path)?;
        let session: Self = serde_json::from_str(&text).map_err(|_| {
            CliError::Invalid(format!(
                "the cloud session file {} is not valid JSON; run `{} login` again",
                path.display(),
                brand()
            ))
        })?;
        if session.access_token.trim().is_empty() {
            return Err(CliError::Invalid(format!(
                "the cloud session file {} has no access token; run `{} login` again",
                path.display(),
                brand()
            )));
        }
        Ok(session)
    }
}

/// Returns the not-signed-in error without reading any token.
fn not_signed_in() -> CliError {
    CliError::Auth(format!(
        "not signed in to the StackSpirit cloud; run `{} login` first",
        brand()
    ))
}

/// Returns the branded command name.
fn brand() -> &'static str {
    vowp_cli::cli::binary_brand()
}

/// Resolves the API base from the flag, then the session, then the public default.
fn resolve_api_url(flag: Option<&str>, session: &CloudSession) -> Result<String, CliError> {
    let value = flag
        .filter(|value| !value.trim().is_empty())
        .or_else(|| (!session.api_url.trim().is_empty()).then_some(session.api_url.as_str()))
        .unwrap_or(DEFAULT_API_URL);
    Ok(value.trim().trim_end_matches('/').to_owned())
}

/// Resolves and validates the public age recipient.
fn resolve_recipient(flag: Option<&str>) -> Result<age::x25519::Recipient, CliError> {
    let value = flag
        .map(str::to_owned)
        .or_else(|| std::env::var("STACKSPIRIT_BACKUP_RECIPIENT").ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            CliError::Invalid(
                "push needs a public age recipient; pass --recipient age1... or set STACKSPIRIT_BACKUP_RECIPIENT (the private identity stays offline)"
                    .to_owned(),
            )
        })?;
    value
        .parse::<age::x25519::Recipient>()
        .map_err(|_| CliError::Invalid("--recipient is not a valid age x25519 recipient (age1...)".to_owned()))
}

/// Resolves and validates the private age identity file.
fn resolve_identity(flag: Option<&Path>) -> Result<age::x25519::Identity, CliError> {
    let path = flag
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("AGE_IDENTITY").map(PathBuf::from))
        .ok_or_else(|| {
            CliError::Invalid(
                "restore needs the private age identity; pass --identity <file> or set AGE_IDENTITY".to_owned(),
            )
        })?;
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            CliError::NotFound(format!("age identity file {}", path.display()))
        } else {
            CliError::Io(error)
        }
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CliError::Invalid(format!(
            "the age identity {} must be a regular file, not a symlink",
            path.display()
        )));
    }
    use std::os::unix::fs::PermissionsExt as _;
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(CliError::Invalid(format!(
            "the age identity {} is mode {mode:03o}; it must be 0600",
            path.display()
        )));
    }
    if metadata.len() > 16 * 1024 {
        return Err(CliError::Invalid(format!(
            "the age identity {} is unexpectedly large",
            path.display()
        )));
    }
    let text = fs::read_to_string(&path)?;
    text.trim().parse::<age::x25519::Identity>().map_err(|_| {
        CliError::Invalid(format!(
            "the age identity {} is not a valid x25519 identity",
            path.display()
        ))
    })
}

/// A private working directory removed on drop.
struct WorkDir {
    path: PathBuf,
}

impl WorkDir {
    /// Creates a fresh mode-0700 directory under `root`.
    fn create(root: &Path) -> Result<Self, CliError> {
        fs::create_dir_all(root)?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        for attempt in 0..100 {
            let path = root.join(format!("backup-r2-{}-{nanos}-{attempt}", std::process::id()));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(CliError::Conflict(
            "could not allocate a private backup work directory".to_owned(),
        ))
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// The backup API client, with no dependency on the private library cloud module.
struct Api {
    agent: ureq::Agent,
    base: String,
    allow_http: bool,
}

impl Api {
    /// Builds a client for `base`, which may use plain HTTP only on loopback.
    fn new(base: &str) -> Result<Self, CliError> {
        let base = base.trim().trim_end_matches('/').to_owned();
        let allow_http = base.starts_with("http://") && is_loopback(&base);
        validate_url(&base, true)?;
        if base.starts_with("http://") && !allow_http {
            return Err(CliError::Invalid(format!(
                "plain HTTP is only accepted for a loopback API base, got {base}"
            )));
        }
        let config = ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::Rustls)
                    .unversioned_rustls_crypto_provider(tls_provider())
                    .build(),
            )
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(120)))
            .build();
        Ok(Self {
            agent: ureq::Agent::new_with_config(config),
            base,
            allow_http,
        })
    }

    /// Sends one authenticated `GET` and returns its JSON body.
    fn get_json(&self, path: &str, token: &str) -> Result<Value, CliError> {
        let url = format!("{}{}", self.base, path);
        validate_url(&url, self.allow_http)?;
        let mut response = self
            .agent
            .get(&url)
            .header("authorization", format!("Bearer {token}"))
            .call()
            .map_err(unreachable)?;
        let status = response.status().as_u16();
        let body = read_json_body(&mut response)?;
        if (200..300).contains(&status) {
            Ok(body)
        } else {
            Err(api_error(status, &body))
        }
    }

    /// Lists the account's machines for `--machine <name>` resolution.
    fn list_machines(&self, token: &str) -> Result<Vec<Machine>, CliError> {
        let body = self.get_json("/v1/machines", token)?;
        let rows = body
            .get("machines")
            .and_then(Value::as_array)
            .ok_or_else(|| CliError::Remote("the cloud API returned no machine list".to_owned()))?;
        rows.iter()
            .map(|value| {
                let id = value
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| CliError::Remote("the cloud API returned a machine without an id".to_owned()))?;
                Ok(Machine {
                    id: id.to_owned(),
                    name: value.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                })
            })
            .collect()
    }

    /// Lists one machine's backups, newest first.
    fn list_backups(&self, token: &str, machine: &str) -> Result<Vec<BackupRow>, CliError> {
        let body = self.get_json(&format!("/v1/machines/{machine}/backups"), token)?;
        let rows = body
            .get("backups")
            .and_then(Value::as_array)
            .ok_or_else(|| CliError::Remote("the backup API returned no backup list".to_owned()))?;
        let mut backups: Vec<BackupRow> = rows
            .iter()
            .map(|value| {
                backup_row(value)
                    .ok_or_else(|| CliError::Remote("the backup API returned a row without a name".to_owned()))
            })
            .collect::<Result<_, _>>()?;
        backups.sort_by(|left, right| right.name.cmp(&left.name));
        Ok(backups)
    }

    /// PUTs one age ciphertext to the machine's backup route.
    fn put_backup(&self, token: &str, machine: &str, sha256: &str, file: &File) -> Result<BackupRow, CliError> {
        let url = format!("{}/v1/machines/{machine}/backups", self.base);
        validate_url(&url, self.allow_http)?;
        let mut response = self
            .agent
            .put(&url)
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/octet-stream")
            .header("x-content-sha256", sha256)
            .send(file)
            .map_err(unreachable)?;
        let status = response.status().as_u16();
        let body = read_json_body(&mut response)?;
        if (200..300).contains(&status) {
            backup_row(&body).ok_or_else(|| CliError::Remote("the backup API returned no backup metadata".to_owned()))
        } else {
            Err(api_error(status, &body))
        }
    }

    /// Downloads one ciphertext to a private file.
    fn download_backup(&self, token: &str, machine: &str, name: &str, destination: &Path) -> Result<(), CliError> {
        validate_backup_name(name)?;
        let url = format!("{}/v1/machines/{machine}/backups/{name}", self.base);
        validate_url(&url, self.allow_http)?;
        let mut response = self
            .agent
            .get(&url)
            .header("authorization", format!("Bearer {token}"))
            .call()
            .map_err(unreachable)?;
        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            write_response(response.body_mut(), destination)
        } else {
            let body = read_json_body(&mut response).unwrap_or(Value::Null);
            Err(api_error(status, &body))
        }
    }
}

/// One account machine returned by `GET /v1/machines`.
#[derive(Debug, Clone)]
struct Machine {
    /// `mch_...` id.
    id: String,
    /// Display name.
    name: String,
}

/// One backup row as the cloud API and the CLI render it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct BackupRow {
    /// Object name (`YYYYMMDDTHHMMSS[mmm]Z.tar.age`).
    #[serde(default)]
    name: String,
    /// Ciphertext size in bytes.
    #[serde(default)]
    size: u64,
    /// SHA-256 of the ciphertext.
    #[serde(default)]
    sha256: String,
    /// Server timestamp.
    #[serde(default)]
    created_at: String,
    /// The object is always age ciphertext.
    #[serde(default = "default_true")]
    encrypted: bool,
    /// Human-readable container format.
    #[serde(default)]
    format: String,
}

/// Serde default for the always-encrypted marker.
fn default_true() -> bool {
    true
}

/// Extracts a backup row from one cloud API object.
fn backup_row(value: &Value) -> Option<BackupRow> {
    let object = value.get("backup").unwrap_or(value);
    let name = object.get("name").and_then(Value::as_str)?.to_owned();
    let created_at = object
        .get("created_at")
        .map(|value| match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default();
    Some(BackupRow {
        name,
        size: object
            .get("size")
            .and_then(Value::as_u64)
            .or_else(|| object.get("bytes").and_then(Value::as_u64))
            .unwrap_or_default(),
        sha256: object
            .get("sha256")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        created_at,
        encrypted: true,
        format: "stackspirit-backup-r2/1".to_owned(),
    })
}

/// Reads one JSON body with a size cap.
fn read_json_body(response: &mut ureq::http::Response<ureq::Body>) -> Result<Value, CliError> {
    if let Some(length) = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        && length > MAX_JSON_BYTES
    {
        return Err(CliError::Remote("the backup API response is too large".to_owned()));
    }
    let mut reader = response.body_mut().as_reader().take(MAX_JSON_BYTES + 1);
    let mut text = String::new();
    reader
        .read_to_string(&mut text)
        .map_err(|error| CliError::Remote(format!("cannot read the StackSpirit API response: {error}")))?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > MAX_JSON_BYTES {
        return Err(CliError::Remote("the backup API response is too large".to_owned()));
    }
    if text.trim().is_empty() {
        Ok(Value::Null)
    } else {
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }
}

/// Streams one binary response into a private file with a size cap.
fn write_response(body: &mut ureq::Body, destination: &Path) -> Result<(), CliError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)?;
    let mut reader = body.as_reader().take(MAX_DOWNLOAD_BYTES + 1);
    let copied = io::copy(&mut reader, &mut file)?;
    if copied > MAX_DOWNLOAD_BYTES {
        drop(file);
        let _ = fs::remove_file(destination);
        return Err(CliError::Remote("the downloaded backup is too large".to_owned()));
    }
    file.sync_all()?;
    Ok(())
}

/// Maps a transport error without ever including a token.
fn unreachable(error: ureq::Error) -> CliError {
    CliError::Remote(format!("the StackSpirit API is unreachable: {error}"))
}

/// Maps a non-2xx response onto the CLI's error classes.
fn api_error(status: u16, body: &Value) -> CliError {
    let code = body
        .pointer("/error/code")
        .and_then(Value::as_str)
        .or_else(|| body.get("code").and_then(Value::as_str));
    let message = body
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| body.get("message").and_then(Value::as_str));
    let described = match (code, message) {
        (Some(code), Some(message)) => format!("{code}: {message}"),
        (Some(code), None) => code.to_owned(),
        (None, Some(message)) => message.to_owned(),
        (None, None) => format!("HTTP {status}"),
    };
    match status {
        401 | 403 => CliError::Auth(format!(
            "the cloud API refused the session: {described}; run `{} login` again",
            brand()
        )),
        400 | 415 => CliError::Invalid(format!("the cloud API rejected the backup request: {described}")),
        402 => CliError::Limited(format!("the backup add-on is not active for this machine: {described}")),
        404 => CliError::NotFound(format!("backup API object: {described}")),
        409 => CliError::Conflict(described),
        413 => CliError::Limited(format!("the encrypted backup is too large: {described}")),
        422 => CliError::Invalid(format!("the cloud API rejected the backup: {described}")),
        429 => CliError::Limited(format!("the cloud API is rate limiting backups: {described}")),
        _ => CliError::Remote(format!("the backup API failed: {described}")),
    }
}

/// Returns true when a plain-HTTP URL points at this machine.
fn is_loopback(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = if let Some(end) = authority.find(']') {
        &authority[..=end]
    } else {
        authority.rsplit_once(':').map(|(host, _)| host).unwrap_or(authority)
    };
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

/// Validates an absolute HTTP(S) URL without parsing userinfo or a path.
fn validate_url(url: &str, allow_http: bool) -> Result<(), CliError> {
    let rest = if let Some(rest) = url.strip_prefix("https://") {
        rest
    } else if allow_http {
        url.strip_prefix("http://")
            .ok_or_else(|| CliError::Invalid(format!("the backup URL must start with https://, got {url}")))?
    } else {
        return Err(CliError::Invalid(format!(
            "the backup URL must start with https://, got {url}"
        )));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty()
        || authority.contains('@')
        || authority.contains(' ')
        || authority.chars().any(char::is_control)
    {
        return Err(CliError::Invalid(format!("the backup URL has no valid host: {url}")));
    }
    Ok(())
}

/// Resolves the machine id from `--machine`, or from the home's relay enrollment.
fn resolve_machine(api: &Api, token: &str, flag: Option<&str>, home: &Path) -> Result<String, CliError> {
    let Some(value) = flag.map(str::trim).filter(|value| !value.is_empty()) else {
        return read_relay_machine(home);
    };
    if is_machine_id(value) {
        return Ok(value.to_owned());
    }
    let machines = api.list_machines(token)?;
    let matches: Vec<&Machine> = machines
        .iter()
        .filter(|machine| machine.name == value || machine.id == value)
        .collect();
    match matches.as_slice() {
        [machine] => Ok(machine.id.clone()),
        [] => Err(CliError::NotFound(format!("machine {value} in this account"))),
        _ => Err(CliError::Invalid(format!(
            "machine name {value} is ambiguous; pass the mch_... id"
        ))),
    }
}

/// Reads `machine_id` from the home's `relay.json` without exposing the credential.
fn read_relay_machine(home: &Path) -> Result<String, CliError> {
    let path = home.join("relay.json");
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            CliError::NotFound(format!(
                "this home is not enrolled with the relay ({}); run `{} relay enroll ...` or pass --machine <id|name>",
                path.display(),
                brand()
            ))
        } else {
            CliError::Io(error)
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(CliError::Guard(format!(
            "{} must not be a symlink; run `{} relay enroll ...` again",
            path.display(),
            brand()
        )));
    }
    use std::os::unix::fs::PermissionsExt as _;
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(CliError::Guard(format!(
            "{} is mode {mode:03o}; it must be 0600",
            path.display()
        )));
    }
    let text = fs::read_to_string(&path)?;
    let value: Value = serde_json::from_str(&text).map_err(|_| {
        CliError::Invalid(format!(
            "{} is not valid JSON; run `{} relay enroll ...` again",
            path.display(),
            brand()
        ))
    })?;
    let machine = value.get("machine_id").and_then(Value::as_str).ok_or_else(|| {
        CliError::Invalid(format!(
            "{} has no machine_id; run `{} relay enroll ...` again",
            path.display(),
            brand()
        ))
    })?;
    validate_machine_id(machine)?;
    Ok(machine.to_owned())
}

/// Returns whether `value` is a syntactically valid `mch_...` id.
fn is_machine_id(value: &str) -> bool {
    validate_machine_id(value).is_ok()
}

/// Validates the cloud Worker's `mch_` id shape.
fn validate_machine_id(value: &str) -> Result<(), CliError> {
    let valid = value.strip_prefix("mch_").is_some_and(|rest| {
        rest.len() == 26
            && rest.chars().all(|character| {
                character.is_ascii_digit()
                    || matches!(character, 'A'..='H' | 'J' | 'K' | 'M' | 'N' | 'P'..='T' | 'V'..='Z')
            })
    });
    if valid {
        Ok(())
    } else {
        Err(CliError::Invalid(format!(
            "machine id {value} must match mch_ followed by 26 Crockford characters"
        )))
    }
}

/// Validates a backup object name before it is placed in a URL path.
fn validate_backup_name(name: &str) -> Result<(), CliError> {
    if name.is_empty()
        || name.len() > 128
        || name.starts_with('.')
        || name.contains("..")
        || name.contains('/')
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '.' || character == '-')
    {
        return Err(CliError::Invalid("the backup id/name is not valid".to_owned()));
    }
    Ok(())
}

/// Verifies the downloaded ciphertext against the metadata and the age header.
fn verify_download(row: &BackupRow, path: &Path) -> Result<(), CliError> {
    if row.size != 0 && fs::metadata(path)?.len() != row.size {
        return Err(CliError::Conflict(format!(
            "downloaded backup {} has the wrong size",
            row.name
        )));
    }
    if row.sha256.is_empty() {
        return Err(CliError::Remote(format!(
            "backup {} has no SHA-256 in its metadata; refusing an unverified restore",
            row.name
        )));
    }
    let actual = hash_hex(&sha256_file(path)?);
    if !actual.eq_ignore_ascii_case(&row.sha256) {
        return Err(CliError::Conflict(format!(
            "downloaded backup {} failed its SHA-256 check",
            row.name
        )));
    }
    let mut file = File::open(path)?;
    let mut header = [0u8; 21];
    file.read_exact(&mut header)?;
    if &header != b"age-encryption.org/v1" {
        return Err(CliError::Conflict(format!(
            "downloaded backup {} is not an age ciphertext",
            row.name
        )));
    }
    Ok(())
}

/// Encrypts `source` to `recipient` in a streaming age file.
fn encrypt_file(recipient: &age::x25519::Recipient, source: &Path, destination: &Path) -> Result<(), CliError> {
    let encryptor = age::Encryptor::with_recipients(std::iter::once(recipient as &dyn age::Recipient))
        .map_err(|error| CliError::Invalid(format!("could not start age encryption: {error}")))?;
    let input = File::open(source)?;
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)?;
    let mut reader = BufReader::new(input);
    let mut writer = encryptor.wrap_output(BufWriter::new(output)).map_err(CliError::Io)?;
    io::copy(&mut reader, &mut writer)?;
    let output = writer.finish()?;
    output
        .into_inner()
        .map_err(|error| CliError::Io(error.into_error()))?
        .sync_all()?;
    Ok(())
}

/// Decrypts one age file with `identity`.
fn decrypt_file(identity: &age::x25519::Identity, source: &Path, destination: &Path) -> Result<(), CliError> {
    let input = BufReader::new(File::open(source)?);
    let decryptor = age::Decryptor::new(input)
        .map_err(|_| CliError::Conflict("the downloaded backup is not a valid age file".to_owned()))?;
    let reader = decryptor
        .decrypt(std::iter::once(identity as &dyn age::Identity))
        .map_err(|_| {
            CliError::Invalid("the age identity does not decrypt this backup; check --identity/AGE_IDENTITY".to_owned())
        })?;
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)?;
    let mut reader = BufReader::new(reader);
    let mut writer = BufWriter::new(output);
    io::copy(&mut reader, &mut writer)?;
    writer.flush()?;
    writer
        .into_inner()
        .map_err(|error| CliError::Io(error.into_error()))?
        .sync_all()?;
    Ok(())
}

/// Collects every regular file under `root`, sorted for a deterministic archive.
fn collect_files(root: &Path) -> Result<Vec<PathBuf>, CliError> {
    fn walk(directory: &Path, out: &mut Vec<PathBuf>) -> Result<(), CliError> {
        let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                // The core backup never writes symlinks; skipping one here keeps an
                // unexpected link from escaping the archive.
                continue;
            }
            if metadata.is_dir() {
                walk(&path, out)?;
            } else if metadata.is_file() {
                out.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    walk(root, &mut files)?;
    files.sort_by(|left, right| left.strip_prefix(root).ok().cmp(&right.strip_prefix(root).ok()));
    Ok(files)
}

/// Writes the path-safe archive format.
fn write_archive(root: &Path, destination: &Path) -> Result<(), CliError> {
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)?;
    output.write_all(ARCHIVE_MAGIC)?;
    for path in collect_files(root)? {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| CliError::Invalid("a backup file escaped its directory".to_owned()))?;
        let relative = relative
            .to_str()
            .ok_or_else(|| CliError::Invalid("a backup path is not valid UTF-8".to_owned()))?;
        let path_len =
            u32::try_from(relative.len()).map_err(|_| CliError::Invalid("a backup path is too long".to_owned()))?;
        let bytes = fs::metadata(&path)?.len();
        let sha256 = sha256_file(&path)?;
        output.write_all(&path_len.to_le_bytes())?;
        output.write_all(&0o600_u32.to_le_bytes())?;
        output.write_all(&bytes.to_le_bytes())?;
        output.write_all(&sha256)?;
        output.write_all(relative.as_bytes())?;
        let mut file = File::open(&path)?;
        let copied = io::copy(&mut file, &mut output)?;
        if copied != bytes {
            return Err(CliError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("{} changed while it was archived", path.display()),
            )));
        }
    }
    output.write_all(&0_u32.to_le_bytes())?;
    output.sync_all()?;
    Ok(())
}

/// Extracts the path-safe archive format into `destination`.
fn extract_archive(archive: &Path, destination: &Path) -> Result<(), CliError> {
    fs::DirBuilder::new().recursive(true).mode(0o700).create(destination)?;
    let mut input = BufReader::new(File::open(archive)?);
    let mut magic = vec![0_u8; ARCHIVE_MAGIC.len()];
    input.read_exact(&mut magic)?;
    if magic != ARCHIVE_MAGIC {
        return Err(CliError::Conflict(
            "the decrypted backup has an unsupported archive format".to_owned(),
        ));
    }

    let mut seen = BTreeSet::new();
    loop {
        let mut length = [0_u8; 4];
        input.read_exact(&mut length)?;
        let path_len = u32::from_le_bytes(length);
        if path_len == 0 {
            break;
        }
        let mut mode = [0_u8; 4];
        input.read_exact(&mut mode)?;
        if u32::from_le_bytes(mode) & 0o7000 != 0 {
            return Err(CliError::Conflict(
                "the backup archive contains an unsafe file mode".to_owned(),
            ));
        }
        let mut size = [0_u8; 8];
        input.read_exact(&mut size)?;
        let size = u64::from_le_bytes(size);
        let mut expected = [0_u8; 32];
        input.read_exact(&mut expected)?;

        let path_len = usize::try_from(path_len)
            .map_err(|_| CliError::Conflict("the backup archive path is too long".to_owned()))?;
        if path_len > MAX_ARCHIVE_PATH {
            return Err(CliError::Conflict("the backup archive path is too long".to_owned()));
        }
        let mut path = vec![0_u8; path_len];
        input.read_exact(&mut path)?;
        let text = std::str::from_utf8(&path)
            .map_err(|_| CliError::Conflict("the backup archive path is not UTF-8".to_owned()))?;
        let relative = safe_relative(text)?;
        if !seen.insert(relative.clone()) {
            return Err(CliError::Conflict(
                "the backup archive contains a duplicate path".to_owned(),
            ));
        }

        let target = destination.join(&relative);
        if let Some(parent) = target.parent() {
            fs::DirBuilder::new().recursive(true).mode(0o700).create(parent)?;
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&target)?;
        let mut hasher = Sha256::new();
        let mut remaining = size;
        let mut buffer = [0_u8; 64 * 1024];
        while remaining > 0 {
            let want = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
            let read = input.read(&mut buffer[..want])?;
            if read == 0 {
                return Err(CliError::Conflict("the backup archive ended inside a file".to_owned()));
            }
            output.write_all(&buffer[..read])?;
            hasher.update(&buffer[..read]);
            remaining -= u64::try_from(read).unwrap_or_default();
        }
        output.sync_all()?;
        let actual: [u8; 32] = hasher.finalize().into();
        if actual != expected {
            return Err(CliError::Conflict(format!(
                "the backup archive file {} failed its SHA-256 check",
                relative.display()
            )));
        }
    }

    let mut trailing = [0_u8; 1];
    if input.read(&mut trailing)? != 0 {
        return Err(CliError::Conflict("the backup archive has trailing data".to_owned()));
    }
    Ok(())
}

/// Rejects absolute, parent-relative and non-normal archive paths.
fn safe_relative(text: &str) -> Result<PathBuf, CliError> {
    if text.is_empty() || text.contains('\0') || text.contains('\\') {
        return Err(CliError::Conflict(
            "the backup archive contains an unsafe path".to_owned(),
        ));
    }
    let path = Path::new(text);
    if path.is_absolute() {
        return Err(CliError::Conflict(
            "the backup archive contains an absolute path".to_owned(),
        ));
    }
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => relative.push(part),
            _ => {
                return Err(CliError::Conflict(
                    "the backup archive contains a parent or root component".to_owned(),
                ));
            }
        }
    }
    if relative.as_os_str().is_empty() {
        return Err(CliError::Conflict(
            "the backup archive contains an empty path".to_owned(),
        ));
    }
    Ok(relative)
}

/// Returns the lowercase hexadecimal SHA-256 of a file.
fn sha256_file(path: &Path) -> Result<[u8; 32], CliError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

/// Formats a digest as lowercase hexadecimal.
fn hash_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Returns the TLS crypto provider, matching the library cloud client.
fn tls_provider() -> Arc<rustls::crypto::CryptoProvider> {
    rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
}
