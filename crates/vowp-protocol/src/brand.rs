//! Product naming shared by every crate: the StackSpirit Engine brand and the legacy VOWP names.
//!
//! Phase 1 of the rebrand (`docs/design/stackspirit-engine-rebrand.md`) is additive: the new
//! names are accepted everywhere, the old names keep working, and no on-disk default moves.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Product display name.
pub const PRODUCT_NAME: &str = "StackSpirit Engine";
/// One-line product description used by `--help` and the MCP server info.
pub const PRODUCT_DESCRIPTION: &str = "StackSpirit Engine (formerly VOWP): a local supervisor for AI coding agents";
/// New home environment variable; wins over [`LEGACY_HOME_ENV`].
pub const HOME_ENV: &str = "STACKSPIRIT_HOME";
/// Legacy home environment variable.
pub const LEGACY_HOME_ENV: &str = "VOWP_HOME";
/// New project config file name; preferred when present.
pub const PROJECT_CONFIG_FILE: &str = "stackspirit.toml";
/// Legacy project config file name; also what a new project gets in this slice.
pub const LEGACY_PROJECT_CONFIG_FILE: &str = "vowp.toml";

/// Resolves the home override from the environment: `STACKSPIRIT_HOME`, then `VOWP_HOME`.
///
/// Empty values count as unset.
pub fn home_override_from_env() -> Option<PathBuf> {
    home_override(|name| std::env::var_os(name))
}

/// Same as [`home_override_from_env`] with an injectable variable lookup.
pub fn home_override(get: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    [HOME_ENV, LEGACY_HOME_ENV]
        .into_iter()
        .find_map(|name| get(name).filter(|value| !value.is_empty()))
        .map(PathBuf::from)
}

/// Returns the project config path for a checkout: `stackspirit.toml` when it is a file,
/// otherwise `vowp.toml` (whether or not it exists). Never writes.
pub fn project_config_path(root: &Path) -> PathBuf {
    let new = root.join(PROJECT_CONFIG_FILE);
    if new.is_file() {
        new
    } else {
        root.join(LEGACY_PROJECT_CONFIG_FILE)
    }
}

/// Returns true when `root` holds either project config file.
pub fn has_project_config(root: &Path) -> bool {
    root.join(PROJECT_CONFIG_FILE).is_file() || root.join(LEGACY_PROJECT_CONFIG_FILE).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn stackspirit_home_beats_vowp_home() {
        let env = [("VOWP_HOME", "/old"), ("STACKSPIRIT_HOME", "/new")];
        assert_eq!(home_override(lookup(&env)), Some(PathBuf::from("/new")));
    }

    #[test]
    fn vowp_home_used_alone_and_empty_ignored() {
        assert_eq!(
            home_override(lookup(&[("VOWP_HOME", "/old")])),
            Some(PathBuf::from("/old"))
        );
        let env = [("STACKSPIRIT_HOME", ""), ("VOWP_HOME", "/old")];
        assert_eq!(home_override(lookup(&env)), Some(PathBuf::from("/old")));
        assert_eq!(home_override(lookup(&[])), None);
    }

    #[test]
    fn config_discovery_prefers_new_name_and_falls_back() {
        let dir = std::env::temp_dir().join(format!("vowp-brand-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        assert!(!has_project_config(&dir));
        assert_eq!(project_config_path(&dir), dir.join("vowp.toml"));
        std::fs::write(dir.join("vowp.toml"), "").expect("write");
        assert!(has_project_config(&dir));
        assert_eq!(project_config_path(&dir), dir.join("vowp.toml"));
        std::fs::write(dir.join("stackspirit.toml"), "").expect("write");
        assert_eq!(project_config_path(&dir), dir.join("stackspirit.toml"));
        std::fs::remove_file(dir.join("vowp.toml")).expect("rm");
        assert_eq!(project_config_path(&dir), dir.join("stackspirit.toml"));
        assert!(has_project_config(&dir));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
