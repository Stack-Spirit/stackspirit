//! Safe command errors. Raw VOWP stderr never crosses this boundary.

use spirit_core::engine::KeychainError;
use spirit_core::error::{SpiritError, SpiritErrorCode};

/// The command result type used by every Tauri command.
pub type CommandResult<T> = Result<T, SpiritError>;

/// Maps a keychain failure without including a secret value.
pub fn keychain_error(error: KeychainError) -> SpiritError {
    SpiritError::new(SpiritErrorCode::Keychain, error.to_string())
        .with_hint("Unlock the local keychain and retry.")
}
