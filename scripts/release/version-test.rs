//! Integration test for the `vowp --version` output.
//!
//! The publishable `vowp-cli` package manifest lives at the workspace root (see the comment
//! in the root `Cargo.toml`), so this test is wired up as the `version` target there. It is
//! the same check as `crates/vowp-cli/tests/version.rs`, which the packaging job does not own
//! and therefore cannot update with the release version.

use std::process::Command;

#[test]
fn version_flag_prints_package_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_vowp"))
        .arg("--version")
        .output()
        .expect("failed to run vowp --version");

    assert!(
        output.status.success(),
        "vowp --version exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("vowp {}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(env!("CARGO_PKG_VERSION"), "0.1.0");
}
