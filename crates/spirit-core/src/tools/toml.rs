//! Minimal strict TOML subset for tool entries: top-level `key = "string"`,
//! `key = true|false` and `key = ["a", "b"]` lines only. Tables, duplicate keys and
//! multi-line values are rejected.

use std::collections::BTreeMap;

use crate::error::SpiritError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TomlValue {
    String(String),
    Bool(bool),
    StringArray(Vec<String>),
}

pub(crate) type TomlTable = BTreeMap<String, TomlValue>;

fn err(line: usize, detail: &str) -> SpiritError {
    SpiritError::parse("tool entry", format!("line {line}: {detail}"))
}

pub(crate) fn parse_toml(input: &str) -> Result<TomlTable, SpiritError> {
    let mut table = TomlTable::new();
    for (index, raw) in input.lines().enumerate() {
        let n = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            return Err(err(n, "tables are not supported"));
        }
        let Some(eq) = line.find('=') else {
            return Err(err(n, "expected key = value"));
        };
        let key = line[..eq].trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(err(n, "invalid key"));
        }
        if table.contains_key(key) {
            return Err(err(n, "duplicate key"));
        }
        let text = line[eq + 1..].trim();
        let value = if text == "true" {
            TomlValue::Bool(true)
        } else if text == "false" {
            TomlValue::Bool(false)
        } else if text.starts_with('"') {
            let (s, rest) = parse_string(text, n)?;
            if !rest.trim().is_empty() {
                return Err(err(n, "unexpected text after the value"));
            }
            TomlValue::String(s)
        } else if let Some(inner) = text.strip_prefix('[') {
            let mut rest = inner.trim_start();
            let mut items = Vec::new();
            loop {
                if let Some(r) = rest.strip_prefix(']') {
                    if !r.trim().is_empty() {
                        return Err(err(n, "unexpected text after the value"));
                    }
                    break;
                }
                if !rest.starts_with('"') {
                    return Err(err(n, "expected a string in the array"));
                }
                let (s, r) = parse_string(rest, n)?;
                items.push(s);
                rest = r.trim_start();
                if let Some(r) = rest.strip_prefix(',') {
                    rest = r.trim_start();
                } else if !rest.starts_with(']') {
                    return Err(err(n, "expected ',' or ']'"));
                }
            }
            TomlValue::StringArray(items)
        } else {
            return Err(err(n, "unsupported value"));
        };
        table.insert(key.to_owned(), value);
    }
    Ok(table)
}

fn parse_string(input: &str, n: usize) -> Result<(String, &str), SpiritError> {
    let mut out = String::new();
    let mut chars = input.char_indices().skip(1);
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Ok((out, &input[i + 1..])),
            '\\' => match chars.next() {
                Some((_, '"')) => out.push('"'),
                Some((_, '\\')) => out.push('\\'),
                Some((_, 'n')) => out.push('\n'),
                _ => return Err(err(n, "unsupported escape")),
            },
            c => out.push(c),
        }
    }
    Err(err(n, "unterminated string"))
}
