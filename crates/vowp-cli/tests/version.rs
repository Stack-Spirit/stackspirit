//! Integration tests for the `vowp` binary.

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
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "vowp 0.1.0-dev");
}
