//! Regression: the packaged webview CSP must let the desktop app reach StackSpirit Cloud.
//!
//! `SettingsView` renders `CloudAccountPanel` and `ManagedHomesPanel` on Desktop too, but the
//! cloud client fetches `https://api.stackspirit.io` and opens `wss://relay.stackspirit.io`.
//! Without those origins in `connect-src` the Tauri webview blocks sign-in, machine lists,
//! managed homes and the relay socket. Keep the shell's existing loopback allowances intact.

use std::fs;
use std::path::PathBuf;

/// Origins the desktop shell needed before the cloud UI landed. None may be dropped.
const PREVIOUS_CONNECT_SRC: &[&str] = &[
    "'self'",
    "ipc:",
    "http://ipc.localhost",
    "http://127.0.0.1:*",
    "http://localhost:*",
    "ws://127.0.0.1:*",
    "ws://localhost:*",
];

/// Cloud origins the shared UI talks to; scoped to exact origins, never blanket schemes.
const CLOUD_CONNECT_SRC: &[&str] = &["https://api.stackspirit.io", "wss://relay.stackspirit.io"];

fn read_config() -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let raw =
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()))
}

fn connect_src_tokens(csp: &str) -> Vec<&str> {
    csp.split(';')
        .map(str::trim)
        .find(|directive| directive.split_whitespace().next() == Some("connect-src"))
        .map(|directive| directive.split_whitespace().skip(1).collect())
        .unwrap_or_default()
}

fn assert_connect_src(csp: &str, key: &str) {
    let tokens = connect_src_tokens(csp);
    assert!(
        !tokens.is_empty(),
        "{key} has no connect-src sources: {csp}"
    );
    for required in PREVIOUS_CONNECT_SRC {
        assert!(
            tokens.contains(required),
            "{key} connect-src dropped {required}: {csp}"
        );
    }
    for cloud in CLOUD_CONNECT_SRC {
        assert!(
            tokens.contains(cloud),
            "{key} connect-src is missing {cloud}: {csp}"
        );
    }
    for blanket in ["https:", "wss:", "https://*", "wss://*"] {
        assert!(
            !tokens.contains(&blanket),
            "{key} connect-src must stay origin-scoped, found {blanket}: {csp}"
        );
    }
}

#[test]
fn app_csp_allows_the_cloud_api_and_relay() {
    let config = read_config();
    let csp = config["app"]["security"]["csp"]
        .as_str()
        .expect("app.security.csp must be a string");
    assert_connect_src(csp, "app.security.csp");
}

#[test]
fn dev_csp_allows_the_cloud_api_and_relay_when_present() {
    let config = read_config();
    let Some(csp) = config["app"]["security"]["devCsp"].as_str() else {
        return;
    };
    assert_connect_src(csp, "app.security.devCsp");
}
