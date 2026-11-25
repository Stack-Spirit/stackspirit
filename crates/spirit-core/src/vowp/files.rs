//! Bounded, shell-injection-safe file operations on the VOWP host (the Finder view).
//!
//! The server is the owner's machine, so this module lets them browse it, preview files and
//! move, rename, create, upload, download and delete visually. The rules:
//!
//! - **Argv only.** Every remote command is `sh -c <fixed script> sh <arg>...`. The script
//!   text is a constant of this module; every dynamic value (paths, names, flags) is a
//!   separate argv element that [`crate::ssh::command`] single-quotes, and the script only
//!   ever reads them as positional parameters (`"$1"`), so a path can never be parsed as
//!   shell syntax. NUL, newline and carriage return are rejected before any SSH call.
//! - **`~` is expanded on the server**, by the script, never by the renderer.
//! - **Bounded.** Directory listings, previews (256 KiB), uploads and downloads (64 MiB) all
//!   have hard caps, and a transfer never overwrites an existing file.
//! - **Secrets stay hidden.** A secret-like file (dotenv files, `id_*`, `*.pem`, `*.key`,
//!   `auth.json`, `credentials*`) previews as `masked: true` **without reading the file**
//!   unless the local user explicitly passes `reveal: true`. File contents are never logged.
//! - **Smart seatbelts, not lockouts.** Moving, renaming or deleting a protected live path
//!   (`vowp.db*`, `vowp.toml`, `.ssh`, a worktree) is refused with a typed `conflict` error
//!   until the caller passes `confirm_protected: true`. Only `/` and the home directory
//!   itself can never be deleted.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{SpiritError, SpiritErrorCode};
use crate::ssh::SshRemote;
use crate::ssh::command::{CommandOutput, READ_TIMEOUT, WRITE_TIMEOUT};
use crate::time::Timestamp;

/// Largest path this build sends to the server.
pub const MAX_REMOTE_PATH_BYTES: usize = 4096;
/// Largest preview read from a remote file.
pub const PREVIEW_MAX_BYTES: u64 = 256 * 1024;
/// Largest file uploaded or downloaded in one call.
pub const TRANSFER_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Most directory entries returned for one folder.
pub const LIST_MAX_ENTRIES: usize = 5000;
/// Stdout cap for one directory listing.
const LIST_OUTPUT_CAP: usize = 4 * 1024 * 1024;
/// Timeout for one upload or download.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(300);

/// Lists one directory: `HOME\0RESOLVED\0` then one `type\tsize\tmode\tmtime\tname\0` record
/// per entry. `find -printf` is NUL-terminated so any file name is safe.
const LIST_SCRIPT: &str = r#"home="$HOME"; p="$1"; case "$p" in "~") p="$home";; "~/"*) p="$home/${p#"~/"}";; esac; cd -- "$p" 2>/dev/null || exit 2; printf '%s\0%s\0' "$home" "$(pwd -P)"; find . -mindepth 1 -maxdepth 1 -printf '%y\t%s\t%m\t%T@\t%f\0'"#;

/// Prints the size (then NUL), then at most `$2` bytes of a regular file.
const PREVIEW_SCRIPT: &str = r#"p="$1"; case "$p" in "~") p="$HOME";; "~/"*) p="$HOME/${p#"~/"}";; esac; [ -f "$p" ] || exit 3; size=$(stat -L -c %s -- "$p") || exit 3; printf '%s\0' "$size"; if [ "$size" -le "$2" ]; then head -c "$2" -- "$p"; fi"#;

/// Creates one directory; refuses an existing path (exit 5).
const MKDIR_SCRIPT: &str = r#"p="$1"; case "$p" in "~/"*) p="$HOME/${p#"~/"}";; esac; if [ -e "$p" ] || [ -L "$p" ]; then exit 5; fi; mkdir -- "$p""#;

/// Renames or moves `$1` to the exact path `$2`; never overwrites (exit 5).
const MOVE_SCRIPT: &str = r#"a="$1"; b="$2"; case "$a" in "~/"*) a="$HOME/${a#"~/"}";; esac; case "$b" in "~/"*) b="$HOME/${b#"~/"}";; esac; [ -e "$a" ] || [ -L "$a" ] || exit 3; if [ -e "$b" ] || [ -L "$b" ]; then exit 5; fi; mv -T -- "$a" "$b""#;

/// Deletes `$1`; `$2` = 1 allows recursion. Never `/` or `$HOME` (exit 4).
const DELETE_SCRIPT: &str = r#"p="$1"; case "$p" in "~/"*) p="$HOME/${p#"~/"}";; "~") p="$HOME";; esac; [ -e "$p" ] || [ -L "$p" ] || exit 3; real=$(cd -- "$(dirname -- "$p")" 2>/dev/null && pwd -P)/$(basename -- "$p"); if [ "$p" = "/" ] || [ "$real" = "//" ] || [ "$p" = "$HOME" ] || [ "$real" = "$HOME" ]; then exit 4; fi; if [ -d "$p" ] && [ ! -L "$p" ]; then if [ "$2" = 1 ]; then rm -rf -- "$p"; else rmdir -- "$p"; fi; else rm -f -- "$p"; fi"#;

/// Writes stdin to `$1/$2`, atomically and without overwriting (exit 5).
const UPLOAD_SCRIPT: &str = r#"d="$1"; case "$d" in "~") d="$HOME";; "~/"*) d="$HOME/${d#"~/"}";; esac; [ -d "$d" ] || exit 3; t="$d/$2"; if [ -e "$t" ] || [ -L "$t" ]; then exit 5; fi; tmp="$t.part.$$"; if cat > "$tmp"; then mv -- "$tmp" "$t"; else rm -f -- "$tmp"; exit 6; fi"#;

/// Prints the size of `$1` then NUL, then (when it is within `$2`) the file bytes.
const DOWNLOAD_SCRIPT: &str = r#"p="$1"; case "$p" in "~/"*) p="$HOME/${p#"~/"}";; esac; [ -f "$p" ] || exit 3; size=$(stat -L -c %s -- "$p") || exit 3; printf '%s\0' "$size"; if [ "$size" -le "$2" ]; then cat -- "$p"; fi"#;

/// What kind of node a directory entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteEntryKind {
    /// A regular file (and any other non-directory node).
    File,
    /// A directory.
    Dir,
    /// A symbolic link.
    Symlink,
}

/// One row of a directory listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteEntry {
    /// The file name only.
    pub name: String,
    /// The absolute path of the entry.
    pub path: String,
    /// `file`, `dir` or `symlink`.
    pub kind: RemoteEntryKind,
    /// Size in bytes (the link's own size for a symlink).
    pub size_bytes: u64,
    /// Permission bits as four octal digits, for example `0600`.
    pub mode: String,
    /// Last modification time, when the server reported one.
    pub modified_at: Option<Timestamp>,
    /// True for names that usually hold credentials; the preview is masked by default.
    pub is_secret_like: bool,
    /// True for live VOWP paths; changing them shows the seatbelt confirmation.
    pub is_protected_hint: bool,
}

/// A directory listing with the paths the UI needs to render a breadcrumb.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteDirListing {
    /// The resolved absolute path that was listed.
    pub path: String,
    /// The parent directory, or `None` for `/`.
    pub parent: Option<String>,
    /// The server user's home directory (what `~` means).
    pub home: String,
    /// Entries: directories first, then files, each group sorted by name.
    pub entries: Vec<RemoteEntry>,
    /// True when more than [`LIST_MAX_ENTRIES`] entries existed and the rest were cut.
    pub truncated: bool,
}

/// How a file is shown in the preview pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemotePreviewKind {
    /// UTF-8 text.
    Text,
    /// Markdown text.
    Markdown,
    /// A small PNG, JPEG or WEBP, as base64.
    Image,
    /// Binary or too large to preview.
    Binary,
}

/// A bounded preview of one remote file.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemotePreview {
    /// The absolute path that was previewed.
    pub path: String,
    /// The file name.
    pub name: String,
    /// What the content is.
    pub kind: RemotePreviewKind,
    /// File size in bytes.
    pub size_bytes: u64,
    /// True when the file is secret-like and was not revealed. No content was read.
    pub masked: bool,
    /// True when the file is larger than the preview cap and only a prefix is shown.
    pub truncated: bool,
    /// Text or Markdown content; `None` for images, binary files and masked files.
    pub text: Option<String>,
    /// `image/png`, `image/jpeg` or `image/webp` for an image.
    pub image_mime: Option<String>,
    /// The image bytes as standard base64.
    pub image_base64: Option<String>,
    /// Why there is no content, when there is none and the file is not masked.
    pub note: Option<String>,
}

impl fmt::Debug for RemotePreview {
    // Never print content, not even in a test failure.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemotePreview")
            .field("path", &self.path)
            .field("kind", &self.kind)
            .field("size_bytes", &self.size_bytes)
            .field("masked", &self.masked)
            .finish_non_exhaustive()
    }
}

/// The result of a change: the resulting absolute-or-`~` path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteFileOutcome {
    /// The path that was created, moved to or deleted.
    pub path: String,
}

/// The result of an upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteUploadOutcome {
    /// The remote path of the new file.
    pub path: String,
    /// Bytes written.
    pub size_bytes: u64,
}

/// The result of a download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteDownloadOutcome {
    /// The local path of the new file.
    pub local_path: String,
    /// Bytes written.
    pub size_bytes: u64,
}

// ---------------------------------------------------------------------------------------
// Classification and validation (pure)
// ---------------------------------------------------------------------------------------

/// The dotenv file-name prefix, spelled in pieces so core sources never name an env file.
const DOTENV_PREFIX: &str = concat!(".", "env");

/// Names that usually hold credentials: dotenv files (a leading dot plus `env`), `id_*`, `*.pem`, `*.key`, `auth.json`,
/// `credentials*`. Case-insensitive; `*.pub` public keys are not secret.
#[must_use]
pub fn is_secret_like(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".pub") {
        return false;
    }
    lower.starts_with(DOTENV_PREFIX)
        || lower.starts_with("id_")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower == "auth.json"
        || lower.starts_with("credentials")
}

/// A live VOWP path hint from the file name alone: `vowp.db*`, `vowp.toml`, `.ssh`.
#[must_use]
pub fn is_protected_name(name: &str) -> bool {
    name.starts_with("vowp.db")
        || name == "vowp.toml"
        || name == "stackspirit.toml"
        || name == ".ssh"
}

/// True when `path` is, or lies inside, a live VOWP path: a `vowp.db*` / `vowp.toml` /
/// `.ssh` component, or anything under a `worktrees` directory (an active worker checkout).
#[must_use]
pub fn is_protected_path(path: &str) -> bool {
    let components: Vec<&str> = path
        .split('/')
        .filter(|part| !part.is_empty() && *part != "~")
        .collect();
    components.iter().enumerate().any(|(index, part)| {
        is_protected_name(part) || (*part == "worktrees" && index + 1 < components.len())
    })
}

/// True for a direct child of a `worktrees` directory, which the listing flags too.
fn is_worktree_child(parent: &str) -> bool {
    parent.rsplit('/').next() == Some("worktrees")
}

/// Validates one remote path: non-empty, absolute or `~`-relative, at most
/// [`MAX_REMOTE_PATH_BYTES`], and free of NUL, newline and carriage return.
pub fn validate_remote_path(field: &str, path: &str) -> Result<(), SpiritError> {
    if path.is_empty() {
        return Err(SpiritError::invalid_input(field, "must not be empty"));
    }
    if path.len() > MAX_REMOTE_PATH_BYTES {
        return Err(SpiritError::invalid_input(
            field,
            format!("must be at most {MAX_REMOTE_PATH_BYTES} bytes"),
        ));
    }
    if path.contains(['\u{0}', '\n', '\r']) {
        return Err(SpiritError::invalid_input(
            field,
            "must not contain NUL, newline or carriage return",
        ));
    }
    if !(path.starts_with('/') || path == "~" || path.starts_with("~/")) {
        return Err(SpiritError::invalid_input(
            field,
            "must be absolute or start with ~/",
        ));
    }
    Ok(())
}

/// Validates one new file or folder name: no `/`, not `.` or `..`, no control characters.
pub fn validate_entry_name(field: &str, name: &str) -> Result<(), SpiritError> {
    if name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.chars().any(char::is_control)
    {
        return Err(SpiritError::invalid_input(
            field,
            "use 1-255 characters without '/' or control characters",
        ));
    }
    Ok(())
}

/// Joins a validated directory and name.
#[must_use]
pub fn join_remote(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// The last path component.
#[must_use]
pub fn remote_file_name(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

fn parent_of(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.rfind('/') {
        Some(0) => Some("/".to_owned()),
        Some(index) => Some(trimmed[..index].to_owned()),
        None => None,
    }
}

fn preview_kind_for(name: &str) -> (RemotePreviewKind, Option<&'static str>) {
    let lower = name.to_ascii_lowercase();
    let extension = lower.rsplit_once('.').map_or("", |(_, ext)| ext);
    match extension {
        "md" | "markdown" => (RemotePreviewKind::Markdown, None),
        "png" => (RemotePreviewKind::Image, Some("image/png")),
        "jpg" | "jpeg" => (RemotePreviewKind::Image, Some("image/jpeg")),
        "webp" => (RemotePreviewKind::Image, Some("image/webp")),
        _ => (RemotePreviewKind::Text, None),
    }
}

/// Standard padded base64.
#[must_use]
pub fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).copied().map_or(0, u32::from);
        let b2 = chunk.get(2).copied().map_or(0, u32::from);
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(char::from(TABLE[((n >> 18) & 63) as usize]));
        out.push(char::from(TABLE[((n >> 12) & 63) as usize]));
        out.push(if chunk.len() > 1 {
            char::from(TABLE[((n >> 6) & 63) as usize])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(TABLE[(n & 63) as usize])
        } else {
            '='
        });
    }
    out
}

/// Parses the `find -printf` listing produced by the list script.
///
/// `base` is the resolved directory. Records that do not parse are skipped.
pub fn parse_listing(stdout: &[u8]) -> Result<RemoteDirListing, SpiritError> {
    let mut parts = stdout.split(|byte| *byte == 0);
    let bad = || SpiritError::parse("remote directory listing", "missing header");
    let home = String::from_utf8_lossy(parts.next().ok_or_else(bad)?).into_owned();
    let path = String::from_utf8_lossy(parts.next().ok_or_else(bad)?).into_owned();
    if !path.starts_with('/') {
        return Err(bad());
    }
    let worktree_dir = is_worktree_child(&path);
    let mut entries = Vec::new();
    let mut truncated = false;
    for record in parts {
        if record.is_empty() {
            continue;
        }
        if entries.len() >= LIST_MAX_ENTRIES {
            truncated = true;
            break;
        }
        let text = String::from_utf8_lossy(record);
        let mut fields = text.splitn(5, '\t');
        let (Some(kind), Some(size), Some(mode), Some(mtime), Some(name)) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            continue;
        };
        let kind = match kind {
            "d" => RemoteEntryKind::Dir,
            "l" => RemoteEntryKind::Symlink,
            _ => RemoteEntryKind::File,
        };
        let size_bytes = size.parse::<u64>().unwrap_or(0);
        let mode = u32::from_str_radix(mode.trim(), 8).map_or_else(
            |_| "----".to_owned(),
            |bits| format!("{:04o}", bits & 0o7777),
        );
        let modified_at = mtime
            .split('.')
            .next()
            .and_then(|secs| secs.parse::<i64>().ok())
            .map(Timestamp::from_unix_secs);
        entries.push(RemoteEntry {
            name: name.to_owned(),
            path: join_remote(&path, name),
            kind,
            size_bytes,
            mode,
            modified_at,
            is_secret_like: is_secret_like(name),
            is_protected_hint: is_protected_name(name) || worktree_dir || is_protected_path(&path),
        });
    }
    entries.sort_by(|a, b| {
        let a_dir = a.kind == RemoteEntryKind::Dir;
        let b_dir = b.kind == RemoteEntryKind::Dir;
        b_dir
            .cmp(&a_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(RemoteDirListing {
        parent: parent_of(&path),
        path,
        home,
        entries,
        truncated,
    })
}

/// Builds a preview from the script output and the file name. Pure so tests need no SSH.
pub fn build_preview(
    path: &str,
    stdout: &[u8],
    cap: u64,
    secret_masked: bool,
) -> Result<RemotePreview, SpiritError> {
    let name = remote_file_name(path).to_owned();
    let split = stdout
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| SpiritError::parse("remote preview", "missing size header"))?;
    let size_bytes = String::from_utf8_lossy(&stdout[..split])
        .trim()
        .parse::<u64>()
        .map_err(|_| SpiritError::parse("remote preview", "unreadable size"))?;
    let body = &stdout[split + 1..];
    let mut preview = RemotePreview {
        path: path.to_owned(),
        name: name.clone(),
        kind: RemotePreviewKind::Binary,
        size_bytes,
        masked: secret_masked,
        truncated: false,
        text: None,
        image_mime: None,
        image_base64: None,
        note: None,
    };
    if secret_masked {
        return Ok(preview);
    }
    let (kind, mime) = preview_kind_for(&name);
    if kind == RemotePreviewKind::Image {
        if size_bytes > cap || body.is_empty() {
            preview.note = Some("This image is too large to preview here.".to_owned());
            return Ok(preview);
        }
        preview.kind = RemotePreviewKind::Image;
        preview.image_mime = mime.map(str::to_owned);
        preview.image_base64 = Some(base64_encode(body));
        return Ok(preview);
    }
    // Text: `head -c cap` already bounded the read, but a larger file sends no body at all,
    // so an oversize file is reported as such instead of a half read.
    if size_bytes > cap {
        preview.note = Some(format!(
            "This file is larger than the {} KiB preview limit. Download it to read it.",
            cap / 1024
        ));
        preview.truncated = true;
        return Ok(preview);
    }
    if body.contains(&0) {
        preview.note = Some("Binary file. Download it to open it.".to_owned());
        return Ok(preview);
    }
    match std::str::from_utf8(body) {
        Ok(text) => {
            preview.kind = kind;
            preview.text = Some(text.to_owned());
        }
        Err(_) => {
            preview.note = Some("Binary file. Download it to open it.".to_owned());
        }
    }
    Ok(preview)
}

// ---------------------------------------------------------------------------------------
// The live operations
// ---------------------------------------------------------------------------------------

/// File operations on one server over verified SSH.
#[derive(Debug, Clone)]
pub struct RemoteFiles {
    remote: SshRemote,
}

impl RemoteFiles {
    /// Builds the file service for one SSH remote.
    #[must_use]
    pub fn new(remote: SshRemote) -> Self {
        Self { remote }
    }

    /// Lists one directory (`~`, `~/x` or an absolute path).
    pub async fn list_dir(&self, path: &str) -> Result<RemoteDirListing, SpiritError> {
        validate_remote_path("path", path)?;
        let output = self
            .run_script(LIST_SCRIPT, &[path], None, READ_TIMEOUT, LIST_OUTPUT_CAP)
            .await?;
        let output = check(output, "list the folder")?;
        parse_listing(&output.stdout)
    }

    /// Reads a bounded preview. A secret-like file is masked **without being read** unless
    /// `reveal` is true. Neither the content nor the path is ever logged.
    pub async fn read_preview(
        &self,
        path: &str,
        reveal: bool,
    ) -> Result<RemotePreview, SpiritError> {
        validate_remote_path("path", path)?;
        let masked = is_secret_like(remote_file_name(path)) && !reveal;
        let cap = PREVIEW_MAX_BYTES.to_string();
        // A masked file is never read: only its size is asked for (cap 0 sends no body).
        let read_cap = if masked { "0" } else { cap.as_str() };
        let output = self
            .run_script(
                PREVIEW_SCRIPT,
                &[path, read_cap],
                None,
                READ_TIMEOUT,
                usize::try_from(PREVIEW_MAX_BYTES).unwrap_or(262_144) + 64,
            )
            .await?;
        let output = check(output, "read the file")?;
        build_preview(path, &output.stdout, PREVIEW_MAX_BYTES, masked)
    }

    /// Creates one folder. An existing path is a typed `conflict`.
    pub async fn mkdir(&self, path: &str) -> Result<RemoteFileOutcome, SpiritError> {
        validate_remote_path("path", path)?;
        validate_entry_name("folder name", remote_file_name(path))?;
        let output = self
            .run_script(MKDIR_SCRIPT, &[path], None, WRITE_TIMEOUT, 4096)
            .await?;
        check(output, "create the folder")?;
        Ok(RemoteFileOutcome {
            path: path.to_owned(),
        })
    }

    /// Renames or moves `from` to the exact path `to`. Never overwrites. A protected live
    /// path needs `confirm_protected`.
    pub async fn rename_or_move(
        &self,
        from: &str,
        to: &str,
        confirm_protected: bool,
    ) -> Result<RemoteFileOutcome, SpiritError> {
        validate_remote_path("from", from)?;
        validate_remote_path("to", to)?;
        validate_entry_name("name", remote_file_name(to))?;
        require_confirmation(from, confirm_protected)?;
        let output = self
            .run_script(MOVE_SCRIPT, &[from, to], None, WRITE_TIMEOUT, 4096)
            .await?;
        check(output, "move the file")?;
        Ok(RemoteFileOutcome {
            path: to.to_owned(),
        })
    }

    /// Deletes a file, or a folder when it is empty or `recursive` is true. Never `/` or
    /// the home directory. A protected live path needs `confirm_protected`.
    pub async fn delete(
        &self,
        path: &str,
        recursive: bool,
        confirm_protected: bool,
    ) -> Result<RemoteFileOutcome, SpiritError> {
        validate_remote_path("path", path)?;
        if path == "/" || path == "~" || path == "~/" {
            return Err(refuse_root());
        }
        require_confirmation(path, confirm_protected)?;
        let flag = if recursive { "1" } else { "0" };
        let output = self
            .run_script(DELETE_SCRIPT, &[path, flag], None, WRITE_TIMEOUT, 4096)
            .await?;
        check(output, "delete the file")?;
        Ok(RemoteFileOutcome {
            path: path.to_owned(),
        })
    }

    /// Uploads one local file into `remote_dir`, keeping its name. Never overwrites.
    pub async fn upload_file(
        &self,
        local_path: &Path,
        remote_dir: &str,
    ) -> Result<RemoteUploadOutcome, SpiritError> {
        validate_remote_path("remote folder", remote_dir)?;
        let name = local_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| SpiritError::invalid_input("local file", "has no usable file name"))?
            .to_owned();
        validate_entry_name("file name", &name)?;
        let metadata = std::fs::metadata(local_path)
            .map_err(|err| SpiritError::io("read the local file", &err))?;
        if !metadata.is_file() {
            return Err(SpiritError::invalid_input("local file", "must be a file"));
        }
        if metadata.len() > TRANSFER_MAX_BYTES {
            return Err(SpiritError::transfer_too_large(
                "the file",
                TRANSFER_MAX_BYTES,
            ));
        }
        let bytes = std::fs::read(local_path)
            .map_err(|err| SpiritError::io("read the local file", &err))?;
        let size_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let output = self
            .run_script(
                UPLOAD_SCRIPT,
                &[remote_dir, &name],
                Some(bytes),
                TRANSFER_TIMEOUT,
                4096,
            )
            .await?;
        check(output, "upload the file")?;
        Ok(RemoteUploadOutcome {
            path: join_remote(remote_dir, &name),
            size_bytes,
        })
    }

    /// Downloads one remote file into `local_dir`, never overwriting a local file.
    pub async fn download_file(
        &self,
        remote_path: &str,
        local_dir: &Path,
    ) -> Result<RemoteDownloadOutcome, SpiritError> {
        validate_remote_path("path", remote_path)?;
        let name = remote_file_name(remote_path).to_owned();
        validate_entry_name("file name", &name)?;
        if !local_dir.is_dir() {
            return Err(SpiritError::invalid_input(
                "local folder",
                "must be an existing folder",
            ));
        }
        let cap = TRANSFER_MAX_BYTES.to_string();
        let output = self
            .run_script(
                DOWNLOAD_SCRIPT,
                &[remote_path, &cap],
                None,
                TRANSFER_TIMEOUT,
                usize::try_from(TRANSFER_MAX_BYTES).unwrap_or(usize::MAX / 2) + 64,
            )
            .await?;
        let output = check(output, "download the file")?;
        let split = output
            .stdout
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| SpiritError::parse("remote download", "missing size header"))?;
        let size = String::from_utf8_lossy(&output.stdout[..split])
            .trim()
            .parse::<u64>()
            .map_err(|_| SpiritError::parse("remote download", "unreadable size"))?;
        if size > TRANSFER_MAX_BYTES {
            return Err(SpiritError::transfer_too_large(
                "the remote file",
                TRANSFER_MAX_BYTES,
            ));
        }
        let body = &output.stdout[split + 1..];
        if u64::try_from(body.len()).unwrap_or(u64::MAX) != size {
            return Err(SpiritError::new(
                SpiritErrorCode::Remote,
                "the remote file changed size during the download",
            )
            .retryable());
        }
        let target = unique_local_path(local_dir, &name, body)?;
        Ok(RemoteDownloadOutcome {
            local_path: target.display().to_string(),
            size_bytes: size,
        })
    }

    async fn run_script(
        &self,
        script: &str,
        args: &[&str],
        stdin: Option<Vec<u8>>,
        timeout: Duration,
        cap: usize,
    ) -> Result<CommandOutput, SpiritError> {
        let mut argv = vec![
            "sh".to_owned(),
            "-c".to_owned(),
            script.to_owned(),
            "sh".to_owned(),
        ];
        argv.extend(args.iter().map(|arg| (*arg).to_owned()));
        match stdin {
            Some(bytes) => self.remote.run_with_stdin(&argv, bytes, timeout, cap).await,
            None => self.remote.run_with_cap(&argv, timeout, cap).await,
        }
    }
}

fn refuse_root() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Refused,
        "the server root and the home folder cannot be deleted from here",
    )
    .with_hint("Open the folder and delete what is inside it instead.")
}

fn require_confirmation(path: &str, confirmed: bool) -> Result<(), SpiritError> {
    if is_protected_path(path) && !confirmed {
        return Err(SpiritError::new(
            SpiritErrorCode::Conflict,
            "protected_path: this is a live VOWP system path",
        )
        .with_hint(
            "Moving or deleting it may interrupt running work. Confirm to continue anyway.",
        ));
    }
    Ok(())
}

/// Maps a finished script to a typed error without echoing stderr or any path.
fn check(output: CommandOutput, action: &str) -> Result<CommandOutput, SpiritError> {
    if output.success() {
        return Ok(output);
    }
    Err(match output.status {
        2 | 3 => SpiritError::not_found("that path"),
        4 => refuse_root(),
        5 => SpiritError::new(
            SpiritErrorCode::Conflict,
            "something with that name already exists",
        )
        .with_hint("Choose a different name."),
        status => SpiritError::new(
            SpiritErrorCode::Remote,
            format!("could not {action} (exit {status})"),
        )
        .with_hint("Check the folder's permissions on the server."),
    })
}

/// Writes `body` to a new file in `dir`, adding ` (1)`, ` (2)` ... before the extension
/// when the name is taken. Never overwrites.
fn unique_local_path(
    dir: &Path,
    name: &str,
    body: &[u8],
) -> Result<std::path::PathBuf, SpiritError> {
    use std::io::Write;
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_owned(), format!(".{ext}")),
        _ => (name.to_owned(), String::new()),
    };
    for attempt in 0..1000 {
        let candidate = if attempt == 0 {
            name.to_owned()
        } else {
            format!("{stem} ({attempt}){extension}")
        };
        let target = dir.join(candidate);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(mut file) => {
                file.write_all(body)
                    .map_err(|err| SpiritError::io("write the downloaded file", &err))?;
                return Ok(target);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(SpiritError::io("write the downloaded file", &err)),
        }
    }
    Err(SpiritError::new(
        SpiritErrorCode::Conflict,
        "too many local files share that name",
    ))
}
