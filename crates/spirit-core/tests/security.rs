//! Security invariants: redaction, quoting, fixture hygiene, no local shell.

use std::path::Path;

use spirit_core::error::{SpiritError, SpiritErrorCode, redact};
use spirit_core::ssh::command::quote_arg;

fn secret_prefix() -> String {
    ["s", "k", "-"].concat()
}

fn github_prefix() -> String {
    ["g", "hp", "_"].concat()
}

fn pem_header() -> String {
    ["-----BEGIN ", "OPENSSH ", "PRIVATE KEY", "-----"].concat()
}

#[test]
fn redact_removes_common_secret_shapes() {
    let sk = secret_prefix();
    let ghp = github_prefix();
    let secret = ["sk", "-live-", "123456"].concat();
    let token_name = ["to", "ken"].concat();
    let key_name = ["api", "_key"].concat();
    let input = format!(
        "{token_name}=shhh {key_name}=abc123 {secret} Bearer abcdef {ghp}123456 {} harmless",
        pem_header()
    );
    let output = redact(&input);
    assert!(!output.contains("shhh"));
    assert!(!output.contains("abc123"));
    assert!(!output.contains(&secret));
    assert!(!output.contains(&format!("{ghp}123456")));
    assert!(!output.contains("Bearer"));
    assert!(!output.contains("OPENSSH"));
    assert!(output.contains("harmless"));
    assert!(!sk.is_empty());
}

#[test]
fn redact_removes_authorization_headers_with_a_scheme() {
    let output = redact("authorization: Bearer abcdef");
    assert!(!output.contains("abcdef"), "bearer token leaked: {output}");
    assert!(!output.contains("Bearer"), "scheme leaked: {output}");

    let output = redact("Authorization: Basic dXNlcjpwYXNz");
    assert!(
        !output.contains("dXNlcjpwYXNz"),
        "basic credential leaked: {output}"
    );
    assert!(!output.contains("Basic"), "scheme leaked: {output}");
}

#[test]
fn redact_removes_colon_separated_secret_values() {
    let output = redact("api_key: super-secret-value");
    assert!(
        !output.contains("super-secret-value"),
        "api key leaked: {output}"
    );

    let output = redact("password: hunter2");
    assert!(!output.contains("hunter2"), "password leaked: {output}");
}

#[test]
fn redact_removes_whole_private_key_blocks() {
    let begin = pem_header();
    let end = ["-----END ", "OPENSSH ", "PRIVATE KEY", "-----"].concat();
    let body = ["Fake", "Secret", "Body"].concat();
    let input = format!("before\n{begin}\n{body}\nafter\n{end}\nafter-block");
    let output = redact(&input);
    assert!(!output.contains(&body), "key body leaked: {output}");
    assert!(!output.contains("OPENSSH"));
    assert!(output.contains("before"));
    assert!(output.contains("after-block"));

    let truncated = format!("before\n{begin}\n{body}\nunfinished");
    let output = redact(&truncated);
    assert!(
        !output.contains(&body),
        "truncated key body leaked: {output}"
    );
    assert!(output.contains("before"));
}

#[test]
fn error_display_never_echoes_secret() {
    let key_name = ["api", "_key"].concat();
    let token_name = ["to", "ken"].concat();
    let error = SpiritError::new(
        SpiritErrorCode::SshAuth,
        redact(&format!(
            "permission denied for {key_name}=super-secret-value"
        )),
    )
    .with_hint(redact(&format!("check {token_name}=another-secret")));
    let display = format!("{error:?} {error}");
    assert!(!display.contains("super-secret-value"));
    assert!(!display.contains("another-secret"));
    assert!(display.contains("[redacted]") || display.contains("permission denied"));
    let json = serde_json::to_string(&error).unwrap();
    assert!(!json.contains("super-secret-value"));
    assert!(!json.contains("another-secret"));
}

#[test]
fn control_characters_cannot_cross_the_ssh_boundary() {
    assert!(quote_arg("plain text").is_ok());
    assert!(quote_arg("a\nb").is_err());
    assert!(quote_arg("a\rb").is_err());
    assert!(quote_arg("a\0b").is_err());
}

#[test]
fn core_sources_do_not_spawn_a_local_shell() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0;
    for entry in walk(&root) {
        if entry.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(&entry).unwrap();
        for forbidden in [
            "Command::new(\"sh\")",
            "Command::new(\"/bin/sh\")",
            "Command::new(\"bash\")",
            "\"sh\", \"-c\"",
            "\"bash\", \"-c\"",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} contains a local shell spawn",
                entry.display()
            );
        }
        assert!(
            !source.contains(".env"),
            "{} must not read an env file",
            entry.display()
        );
        checked += 1;
    }
    assert!(checked > 10);
}

fn walk(root: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn fixtures_use_documentation_addresses_only() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/vowp");
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains(&["BEGIN ", "RSA ", "PRIVATE KEY"].concat()));
        assert!(!text.contains(&["OPENSSH ", "PRIVATE KEY"].concat()));
        assert!(!text.contains("github.com"));
        assert!(!text.contains("amazonaws.com"));
        for literal in text.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
            let parts: Vec<&str> = literal.split('.').collect();
            if parts.len() != 4 {
                continue;
            }
            let octets: Option<Vec<u8>> =
                parts.iter().map(|part| part.parse::<u8>().ok()).collect();
            let Some(octets) = octets else {
                continue;
            };
            let documentation = (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
                || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
                || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113);
            assert!(
                documentation,
                "{} contains a non-documentation IPv4 address {literal}",
                path.display()
            );
        }
    }
}

#[test]
fn provider_key_names_are_the_only_key_data_in_fixtures() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/vowp");
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for token in
            text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/' | '.')))
        {
            let sk = secret_prefix();
            let ghp = github_prefix();
            let pat = ["git", "hub", "_pat_"].concat();
            assert!(!token.starts_with(&sk), "{}", path.display());
            assert!(!token.starts_with(&ghp), "{}", path.display());
            assert!(!token.starts_with(&pat), "{}", path.display());
        }
    }
}
