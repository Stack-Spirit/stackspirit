//! A strict, dependency-free TOML subset for one harness manifest.
//!
//! The registry file is intentionally simple: only top-level `key = value` lines with
//! strings, booleans, integers and string arrays. Tables, dotted keys, multi-line values
//! and unknown keys are rejected. That is enough for a manifest and prevents a custom
//! file from smuggling in a field such as `api_key` or `oauth_token`.

use std::collections::BTreeMap;

use crate::error::SpiritError;
use crate::harnesses::manifest::{HarnessAuthMode, HarnessManifest, HarnessProtocol};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TomlValue {
    String(String),
    Bool(bool),
    Integer(i64),
    StringArray(Vec<String>),
}

pub(crate) type TomlTable = BTreeMap<String, TomlValue>;

/// Serializes one manifest in a stable field order. The output is a valid TOML document
/// using only the supported subset.
pub(crate) fn manifest_to_toml(manifest: &HarnessManifest) -> String {
    let mut out = String::new();
    push_string(&mut out, "id", &manifest.id);
    push_string(&mut out, "name", &manifest.name);
    push_string(&mut out, "binary", &manifest.binary);
    push_string(&mut out, "install_cmd", &manifest.install_cmd);
    push_string(&mut out, "auth_cmd", &manifest.auth_cmd);
    push_string(&mut out, "protocol", manifest.protocol.as_str());
    push_bool(
        &mut out,
        "supports_embedded_chat",
        manifest.supports_embedded_chat,
    );
    let modes = manifest
        .auth_modes
        .iter()
        .map(|mode| mode.as_str())
        .collect::<Vec<_>>();
    push_string_array(&mut out, "auth_modes", &modes);
    push_string(&mut out, "tos_note", &manifest.tos_note);
    push_string(&mut out, "docs_url", &manifest.docs_url);
    if let Some(port) = manifest.oauth_callback_port {
        out.push_str(&format!("oauth_callback_port = {port}\n"));
    }
    push_bool(&mut out, "builtin", manifest.builtin);
    out
}

/// Parses the supported TOML subset into a string-keyed table.
pub(crate) fn parse_toml(input: &str) -> Result<TomlTable, SpiritError> {
    let mut table = TomlTable::new();
    for (index, raw) in input.lines().enumerate() {
        let line_no = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            return Err(parse_error(line_no, "tables are not supported"));
        }
        let Some(equal) = line.find('=') else {
            return Err(parse_error(line_no, "expected key = value"));
        };
        let key = line[..equal].trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(parse_error(line_no, "invalid key"));
        }
        if table.contains_key(key) {
            return Err(parse_error(line_no, "duplicate key"));
        }
        let value_text = line[equal + 1..].trim_start();
        let (value, rest) = parse_value(value_text, line_no)?;
        let rest = rest.trim_start();
        if !rest.is_empty() && !rest.starts_with('#') {
            return Err(parse_error(line_no, "unexpected text after the value"));
        }
        table.insert(key.to_owned(), value);
    }
    Ok(table)
}

/// Builds a manifest from a parsed table, rejecting unknown and missing fields.
pub(crate) fn manifest_from_toml(input: &str) -> Result<HarnessManifest, SpiritError> {
    let mut table = parse_toml(input)?;
    if !table.is_empty() {
        if let Some(unknown) = table.keys().find(|key| {
            !matches!(
                key.as_str(),
                "id" | "name"
                    | "binary"
                    | "install_cmd"
                    | "auth_cmd"
                    | "protocol"
                    | "supports_embedded_chat"
                    | "auth_modes"
                    | "tos_note"
                    | "docs_url"
                    | "oauth_callback_port"
                    | "builtin"
            )
        }) {
            return Err(SpiritError::invalid_input(
                "harness manifest",
                format!("unknown field '{unknown}'"),
            ));
        }
    }
    let id = take_string(&mut table, "id")?;
    let name = take_string(&mut table, "name")?;
    let binary = take_string(&mut table, "binary")?;
    let install_cmd = take_string(&mut table, "install_cmd")?;
    let auth_cmd = take_string(&mut table, "auth_cmd")?;
    let protocol = HarnessProtocol::parse(&take_string(&mut table, "protocol")?)?;
    let supports_embedded_chat = take_bool(&mut table, "supports_embedded_chat")?;
    let auth_modes = take_string_array(&mut table, "auth_modes")?
        .iter()
        .map(|mode| HarnessAuthMode::parse(mode))
        .collect::<Result<Vec<_>, _>>()?;
    let tos_note = take_string(&mut table, "tos_note")?;
    let docs_url = take_string(&mut table, "docs_url")?;
    let oauth_callback_port = take_optional_port(&mut table)?;
    let builtin = take_optional_bool(&mut table, "builtin")?.unwrap_or(false);
    if !table.is_empty() {
        return Err(SpiritError::invalid_input(
            "harness manifest",
            "contains unsupported fields",
        ));
    }
    if builtin {
        return Err(SpiritError::invalid_input(
            "builtin",
            "a custom harness file must not claim to be built in",
        ));
    }
    let manifest = HarnessManifest {
        id,
        name,
        binary,
        install_cmd,
        auth_cmd,
        protocol,
        supports_embedded_chat,
        auth_modes,
        tos_note,
        docs_url,
        oauth_callback_port,
        builtin,
    };
    manifest.validate()?;
    Ok(manifest)
}

fn take_string(table: &mut TomlTable, key: &str) -> Result<String, SpiritError> {
    match table.remove(key) {
        Some(TomlValue::String(value)) => Ok(value),
        Some(_) => Err(SpiritError::invalid_input(key, "must be a quoted string")),
        None => Err(SpiritError::invalid_input(key, "is required")),
    }
}

fn take_bool(table: &mut TomlTable, key: &str) -> Result<bool, SpiritError> {
    match table.remove(key) {
        Some(TomlValue::Bool(value)) => Ok(value),
        Some(_) => Err(SpiritError::invalid_input(key, "must be true or false")),
        None => Err(SpiritError::invalid_input(key, "is required")),
    }
}

fn take_optional_bool(table: &mut TomlTable, key: &str) -> Result<Option<bool>, SpiritError> {
    match table.remove(key) {
        Some(TomlValue::Bool(value)) => Ok(Some(value)),
        Some(_) => Err(SpiritError::invalid_input(key, "must be true or false")),
        None => Ok(None),
    }
}

fn take_string_array(table: &mut TomlTable, key: &str) -> Result<Vec<String>, SpiritError> {
    match table.remove(key) {
        Some(TomlValue::StringArray(value)) => Ok(value),
        Some(_) => Err(SpiritError::invalid_input(
            key,
            "must be an array of quoted strings",
        )),
        None => Err(SpiritError::invalid_input(key, "is required")),
    }
}

fn take_optional_port(table: &mut TomlTable) -> Result<Option<u16>, SpiritError> {
    match table.remove("oauth_callback_port") {
        Some(TomlValue::Integer(value)) => {
            let port = u16::try_from(value).map_err(|_| {
                SpiritError::invalid_input("oauth_callback_port", "must be 1-65535")
            })?;
            if port == 0 {
                return Err(SpiritError::invalid_input(
                    "oauth_callback_port",
                    "must be 1-65535",
                ));
            }
            Ok(Some(port))
        }
        Some(_) => Err(SpiritError::invalid_input(
            "oauth_callback_port",
            "must be an integer",
        )),
        None => Ok(None),
    }
}

fn parse_value(input: &str, line_no: usize) -> Result<(TomlValue, &str), SpiritError> {
    if input.starts_with('"') {
        let (value, rest) = parse_string(input, line_no)?;
        return Ok((TomlValue::String(value), rest));
    }
    if let Some(rest) = input.strip_prefix("true") {
        return Ok((TomlValue::Bool(true), rest));
    }
    if let Some(rest) = input.strip_prefix("false") {
        return Ok((TomlValue::Bool(false), rest));
    }
    if input.starts_with('[') {
        let (value, rest) = parse_string_array(input, line_no)?;
        return Ok((TomlValue::StringArray(value), rest));
    }
    let (value, rest) = parse_integer(input, line_no)?;
    Ok((TomlValue::Integer(value), rest))
}

fn parse_string(input: &str, line_no: usize) -> Result<(String, &str), SpiritError> {
    let mut chars = input.char_indices();
    let Some((_, '"')) = chars.next() else {
        return Err(parse_error(line_no, "expected a quoted string"));
    };
    let mut value = String::new();
    while let Some((index, character)) = chars.next() {
        match character {
            '"' => return Ok((value, &input[index + 1..])),
            '\\' => {
                let Some((_, escape)) = chars.next() else {
                    return Err(parse_error(line_no, "unterminated escape"));
                };
                match escape {
                    '"' => value.push('"'),
                    '\\' => value.push('\\'),
                    'b' => value.push('\u{0008}'),
                    't' => value.push('\t'),
                    'n' => value.push('\n'),
                    'f' => value.push('\u{000c}'),
                    'r' => value.push('\r'),
                    'u' => value.push(parse_unicode(&mut chars, 4, line_no)?),
                    'U' => value.push(parse_unicode(&mut chars, 8, line_no)?),
                    _ => return Err(parse_error(line_no, "unsupported escape")),
                }
            }
            c if c.is_control() => {
                return Err(parse_error(line_no, "control character in string"));
            }
            c => value.push(c),
        }
    }
    Err(parse_error(line_no, "unterminated string"))
}

fn parse_unicode(
    chars: &mut std::str::CharIndices<'_>,
    digits: usize,
    line_no: usize,
) -> Result<char, SpiritError> {
    let mut value = 0_u32;
    for _ in 0..digits {
        let Some((_, character)) = chars.next() else {
            return Err(parse_error(line_no, "short unicode escape"));
        };
        let digit = character
            .to_digit(16)
            .ok_or_else(|| parse_error(line_no, "invalid unicode escape"))?;
        value = value * 16 + digit;
    }
    char::from_u32(value).ok_or_else(|| parse_error(line_no, "invalid unicode scalar"))
}

fn parse_string_array(input: &str, line_no: usize) -> Result<(Vec<String>, &str), SpiritError> {
    let mut rest = input
        .strip_prefix('[')
        .ok_or_else(|| parse_error(line_no, "expected an array"))?;
    let mut values = Vec::new();
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix(']') {
            return Ok((values, after));
        }
        if !rest.starts_with('"') {
            return Err(parse_error(line_no, "array entries must be quoted strings"));
        }
        let (value, after) = parse_string(rest, line_no)?;
        values.push(value);
        rest = after.trim_start();
        if let Some(after) = rest.strip_prefix(',') {
            rest = after;
            continue;
        }
        if let Some(after) = rest.strip_prefix(']') {
            return Ok((values, after));
        }
        return Err(parse_error(line_no, "expected ',' or ']' in array"));
    }
}

fn parse_integer(input: &str, line_no: usize) -> Result<(i64, &str), SpiritError> {
    let mut end = 0;
    for (index, character) in input.char_indices() {
        if index == 0 && character == '-' {
            end = 1;
            continue;
        }
        if character.is_ascii_digit() {
            end = index + character.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 || (end == 1 && input.starts_with('-')) {
        return Err(parse_error(line_no, "expected a value"));
    }
    let value = input[..end]
        .parse::<i64>()
        .map_err(|_| parse_error(line_no, "integer is out of range"))?;
    Ok((value, &input[end..]))
}

fn parse_error(line: usize, detail: &str) -> SpiritError {
    SpiritError::parse("harness manifest", format!("line {line}: {detail}"))
}

fn push_string(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(" = \"");
    out.push_str(&escape_string(value));
    out.push_str("\"\n");
}

fn push_bool(out: &mut String, key: &str, value: bool) {
    out.push_str(key);
    out.push_str(if value { " = true\n" } else { " = false\n" });
}

fn push_string_array(out: &mut String, key: &str, values: &[&str]) {
    out.push_str(key);
    out.push_str(" = [");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push('"');
        out.push_str(&escape_string(value));
        out.push('"');
    }
    out.push_str("]\n");
}

fn escape_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                out.push_str(&format!("\\u{:04X}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harnesses::manifest::builtin_harnesses;

    #[test]
    fn round_trips_a_custom_manifest() {
        let mut manifest = builtin_harnesses().remove(0);
        manifest.id = "my-opencode".to_owned();
        manifest.name = "My OpenCode \"fork\"".to_owned();
        manifest.install_cmd = "echo \"install\" | bash".to_owned();
        manifest.auth_cmd = "opencode auth login".to_owned();
        manifest.auth_modes = vec![HarnessAuthMode::ApiKey, HarnessAuthMode::Subscription];
        manifest.oauth_callback_port = Some(1455);
        manifest.builtin = false;
        let encoded = manifest_to_toml(&manifest);
        let decoded = manifest_from_toml(&encoded).unwrap();
        assert_eq!(decoded, manifest);
    }

    #[test]
    fn rejects_unknown_fields_and_tables() {
        let base = "id = \"mine\"\nname = \"Mine\"\nbinary = \"mine\"\ninstall_cmd = \"\"\n\
                    auth_cmd = \"\"\nprotocol = \"cli-pty\"\nsupports_embedded_chat = false\n\
                    auth_modes = [\"api_key\"]\ntos_note = \"BYOK\"\n\
                    docs_url = \"https://example.com\"\n";
        assert!(manifest_from_toml(base).is_ok());
        let secret = format!("{base}api_key = \"value\"\n");
        assert!(manifest_from_toml(&secret).is_err());
        let table = format!("{base}[extra]\nkey = \"value\"\n");
        assert!(manifest_from_toml(&table).is_err());
    }

    #[test]
    fn rejects_builtin_claim_and_zero_port() {
        let base = "id = \"mine\"\nname = \"Mine\"\nbinary = \"mine\"\ninstall_cmd = \"\"\n\
                    auth_cmd = \"\"\nprotocol = \"cli-pty\"\nsupports_embedded_chat = false\n\
                    auth_modes = [\"api_key\"]\ntos_note = \"BYOK\"\n\
                    docs_url = \"https://example.com\"\n";
        assert!(manifest_from_toml(&format!("{base}builtin = true\n")).is_err());
        assert!(manifest_from_toml(&format!("{base}oauth_callback_port = 0\n")).is_err());
    }
}
