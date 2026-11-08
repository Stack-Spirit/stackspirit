//! A small TOML subset parser for mapping files.
//!
//! `vowp-adapters` does not declare the `toml` crate, and this job owns only `src/cli/**`,
//! so mapping files are parsed into [`serde_json::Value`] here and then deserialised with
//! serde. The subset covers the schema in `adapters.md` §4.1: tables, arrays of tables,
//! basic/literal strings, integers, booleans, arrays and inline tables. Multi-line strings
//! and arrays/comments spanning lines are accepted too.

use serde_json::{Map, Number, Value};

/// Parses `text` into a JSON value, returning a line/column message on error.
pub(super) fn parse(text: &str) -> Result<Value, String> {
    Parser::new(text).parse_document()
}

/// Where key/value pairs currently go.
enum Current {
    /// A normal `[table]` header.
    Table(Vec<String>),
    /// A `[[array-of-tables]]` header; pairs go into its last element.
    ArrayTable(Vec<String>),
}

/// One cursor over the input text.
struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    /// Creates a parser over `input`.
    fn new(input: &'a str) -> Self {
        let pos = if input.starts_with('\u{feff}') {
            '\u{feff}'.len_utf8()
        } else {
            0
        };
        Self { input, pos }
    }

    /// Parses the whole document.
    fn parse_document(mut self) -> Result<Value, String> {
        let mut root = Value::Object(Map::new());
        let mut current = Current::Table(Vec::new());
        loop {
            self.skip_ws_comments_newlines();
            let Some(first) = self.peek() else {
                break;
            };
            if first == '[' {
                self.bump();
                let array_table = self.peek() == Some('[');
                if array_table {
                    self.bump();
                }
                let path = self.parse_key_path(']')?;
                if array_table {
                    self.expect_char(']')?;
                }
                self.finish_line()?;
                current = if array_table {
                    open_array_table(&mut root, &path)?
                } else {
                    open_table(&mut root, &path)?
                };
            } else {
                let key = self.parse_key()?;
                self.skip_inline_ws();
                self.expect_char('=')?;
                self.skip_inline_ws();
                let value = self.parse_value()?;
                self.finish_line()?;
                insert_value(&mut root, &current, key, value)?;
            }
        }
        Ok(root)
    }

    /// Parses a dotted table path and consumes `terminator`.
    fn parse_key_path(&mut self, terminator: char) -> Result<Vec<String>, String> {
        let mut path = Vec::new();
        loop {
            self.skip_inline_ws();
            path.push(self.parse_key()?);
            self.skip_inline_ws();
            match self.peek() {
                Some('.') => {
                    self.bump();
                }
                Some(c) if c == terminator => {
                    self.bump();
                    return Ok(path);
                }
                _ => {
                    return Err(self.error(&format!("expected '.' or '{terminator}' in table name")));
                }
            }
        }
    }

    /// Parses one bare or quoted key.
    fn parse_key(&mut self) -> Result<String, String> {
        match self.peek() {
            Some('"') => self.parse_basic_string(),
            Some('\'') => self.parse_literal_string(),
            _ => {
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                        self.bump();
                    } else {
                        break;
                    }
                }
                if self.pos == start {
                    return Err(self.error("expected a key"));
                }
                Ok(self.input[start..self.pos].to_owned())
            }
        }
    }

    /// Parses one value.
    fn parse_value(&mut self) -> Result<Value, String> {
        match self.peek() {
            Some('"') => {
                if self.starts_with("\"\"\"") {
                    self.parse_multiline_basic().map(Value::String)
                } else {
                    self.parse_basic_string().map(Value::String)
                }
            }
            Some('\'') => {
                if self.starts_with("'''") {
                    self.parse_multiline_literal().map(Value::String)
                } else {
                    self.parse_literal_string().map(Value::String)
                }
            }
            Some('[') => self.parse_array(),
            Some('{') => self.parse_inline_table(),
            Some('t' | 'f') => self.parse_bool(),
            Some(c) if c == '-' || c == '+' || c.is_ascii_digit() => self.parse_integer(),
            _ => Err(self.error("expected a TOML value")),
        }
    }

    /// Parses a single-line basic string with escapes.
    fn parse_basic_string(&mut self) -> Result<String, String> {
        self.expect_char('"')?;
        let mut out = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(self.error("unterminated string"));
            };
            match c {
                '"' => return Ok(out),
                '\\' => out.push(self.parse_escape()?),
                '\n' | '\r' => return Err(self.error("newline in a single-line string")),
                other => out.push(other),
            }
        }
    }

    /// Parses a single-line literal string (no escapes).
    fn parse_literal_string(&mut self) -> Result<String, String> {
        self.expect_char('\'')?;
        let mut out = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(self.error("unterminated string"));
            };
            match c {
                '\'' => return Ok(out),
                '\n' | '\r' => return Err(self.error("newline in a single-line string")),
                other => out.push(other),
            }
        }
    }

    /// Parses a `"""` multi-line basic string.
    fn parse_multiline_basic(&mut self) -> Result<String, String> {
        self.pos += 3;
        self.trim_opening_newline();
        let mut out = String::new();
        loop {
            if self.starts_with("\"\"\"") {
                self.pos += 3;
                return Ok(out);
            }
            let Some(c) = self.bump() else {
                return Err(self.error("unterminated multi-line string"));
            };
            if c == '\\' {
                if matches!(self.peek(), Some('\n' | '\r')) {
                    if self.peek() == Some('\r') {
                        self.bump();
                    }
                    if self.peek() == Some('\n') {
                        self.bump();
                    }
                    while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
                        self.bump();
                    }
                } else {
                    out.push(self.parse_escape()?);
                }
            } else {
                out.push(c);
            }
        }
    }

    /// Parses a `'''` multi-line literal string.
    fn parse_multiline_literal(&mut self) -> Result<String, String> {
        self.pos += 3;
        self.trim_opening_newline();
        let mut out = String::new();
        loop {
            if self.starts_with("'''") {
                self.pos += 3;
                return Ok(out);
            }
            let Some(c) = self.bump() else {
                return Err(self.error("unterminated multi-line string"));
            };
            out.push(c);
        }
    }

    /// Consumes the optional first newline of a multi-line string.
    fn trim_opening_newline(&mut self) {
        if self.peek() == Some('\r') {
            self.bump();
        }
        if self.peek() == Some('\n') {
            self.bump();
        }
    }

    /// Parses one backslash escape.
    fn parse_escape(&mut self) -> Result<char, String> {
        let Some(c) = self.bump() else {
            return Err(self.error("unterminated escape"));
        };
        Ok(match c {
            '"' => '"',
            '\\' => '\\',
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            'b' => '\u{8}',
            'f' => '\u{c}',
            'u' => self.parse_hex(4)?,
            'U' => self.parse_hex(8)?,
            other => return Err(self.error(&format!("unknown escape \\{other}"))),
        })
    }

    /// Reads `digits` hexadecimal characters and returns the scalar value.
    fn parse_hex(&mut self, digits: usize) -> Result<char, String> {
        let start = self.pos;
        for _ in 0..digits {
            let Some(c) = self.bump() else {
                return Err(self.error("unterminated unicode escape"));
            };
            if !c.is_ascii_hexdigit() {
                return Err(self.error("invalid unicode escape"));
            }
        }
        let code =
            u32::from_str_radix(&self.input[start..self.pos], 16).map_err(|_| self.error("invalid unicode escape"))?;
        char::from_u32(code).ok_or_else(|| self.error("invalid unicode scalar value"))
    }

    /// Parses an integer (underscores allowed).
    fn parse_integer(&mut self) -> Result<Value, String> {
        let start = self.pos;
        if matches!(self.peek(), Some('+' | '-')) {
            self.bump();
        }
        let digits = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '_' {
                self.bump();
            } else {
                break;
            }
        }
        if self.pos == digits {
            return Err(self.error("expected an integer"));
        }
        let raw = self.input[start..self.pos].replace('_', "");
        let number: i64 = raw.parse().map_err(|_| self.error("integer out of range"))?;
        Ok(Value::Number(Number::from(number)))
    }

    /// Parses `true` or `false`.
    fn parse_bool(&mut self) -> Result<Value, String> {
        if self.consume_word("true") {
            Ok(Value::Bool(true))
        } else if self.consume_word("false") {
            Ok(Value::Bool(false))
        } else {
            Err(self.error("expected true or false"))
        }
    }

    /// Parses an array, allowing newlines, comments and a trailing comma.
    fn parse_array(&mut self) -> Result<Value, String> {
        self.expect_char('[')?;
        let mut items = Vec::new();
        loop {
            self.skip_ws_comments_newlines();
            if self.peek() == Some(']') {
                self.bump();
                return Ok(Value::Array(items));
            }
            items.push(self.parse_value()?);
            self.skip_ws_comments_newlines();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some(']') => {
                    self.bump();
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.error("expected ',' or ']' in an array")),
            }
        }
    }

    /// Parses an inline table `{ key = value, ... }`.
    fn parse_inline_table(&mut self) -> Result<Value, String> {
        self.expect_char('{')?;
        let mut map = Map::new();
        loop {
            self.skip_ws_comments_newlines();
            if self.peek() == Some('}') {
                self.bump();
                return Ok(Value::Object(map));
            }
            let key = self.parse_key()?;
            self.skip_inline_ws();
            self.expect_char('=')?;
            self.skip_inline_ws();
            let value = self.parse_value()?;
            if map.insert(key.clone(), value).is_some() {
                return Err(self.error(&format!("duplicate key {key:?}")));
            }
            self.skip_ws_comments_newlines();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some('}') => {
                    self.bump();
                    return Ok(Value::Object(map));
                }
                _ => return Err(self.error("expected ',' or '}' in an inline table")),
            }
        }
    }

    /// Consumes inline whitespace and an optional comment, then a newline or EOF.
    fn finish_line(&mut self) -> Result<(), String> {
        self.skip_inline_ws();
        if self.peek() == Some('#') {
            self.skip_comment();
        }
        match self.peek() {
            None => Ok(()),
            Some('\n') => {
                self.bump();
                Ok(())
            }
            Some('\r') => {
                self.bump();
                if self.peek() == Some('\n') {
                    self.bump();
                }
                Ok(())
            }
            Some(_) => Err(self.error("expected a newline after the value")),
        }
    }

    /// Skips spaces and tabs.
    fn skip_inline_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.bump();
        }
    }

    /// Skips whitespace, newlines and comments.
    fn skip_ws_comments_newlines(&mut self) {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\r' | '\n') => {
                    self.bump();
                }
                Some('#') => self.skip_comment(),
                _ => break,
            }
        }
    }

    /// Skips a comment up to (but not including) the newline.
    fn skip_comment(&mut self) {
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.bump();
        }
    }

    /// Consumes `word` when it is followed by a token boundary.
    fn consume_word(&mut self, word: &str) -> bool {
        if !self.input[self.pos..].starts_with(word) {
            return false;
        }
        let after = self.pos + word.len();
        let boundary = self.input[after..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'));
        if boundary {
            self.pos = after;
        }
        boundary
    }

    /// Consumes `expected` or returns an error.
    fn expect_char(&mut self, expected: char) -> Result<(), String> {
        match self.bump() {
            Some(c) if c == expected => Ok(()),
            Some(c) => Err(self.error(&format!("expected '{expected}', found '{c}'"))),
            None => Err(self.error(&format!("expected '{expected}', found end of input"))),
        }
    }

    /// Returns true when the remaining input starts with `prefix`.
    fn starts_with(&self, prefix: &str) -> bool {
        self.input[self.pos..].starts_with(prefix)
    }

    /// Returns the next character without consuming it.
    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    /// Consumes and returns the next character.
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// Builds a line/column error message.
    fn error(&self, message: &str) -> String {
        let before = &self.input[..self.pos.min(self.input.len())];
        let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = before.rsplit('\n').next().map_or(1, |line| line.chars().count() + 1);
        format!("line {line}, column {column}: {message}")
    }
}

/// Opens or creates a normal table at `path`.
fn open_table(root: &mut Value, path: &[String]) -> Result<Current, String> {
    let mut current = root;
    for segment in path {
        let object = current
            .as_object_mut()
            .ok_or_else(|| format!("table {segment:?} is not an object"))?;
        current = object
            .entry(segment.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    if !current.is_object() {
        return Err(format!("table {:?} is not an object", path.join(".")));
    }
    Ok(Current::Table(path.to_vec()))
}

/// Appends a new table to the array of tables at `path`.
fn open_array_table(root: &mut Value, path: &[String]) -> Result<Current, String> {
    let Some((last, parents)) = path.split_last() else {
        return Err("empty array-of-tables name".to_owned());
    };
    let mut current = root;
    for segment in parents {
        let object = current
            .as_object_mut()
            .ok_or_else(|| format!("table {segment:?} is not an object"))?;
        current = object
            .entry(segment.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    let object = current
        .as_object_mut()
        .ok_or_else(|| format!("table {:?} is not an object", parents.join(".")))?;
    let entry = object.entry(last.clone()).or_insert_with(|| Value::Array(Vec::new()));
    let array = entry
        .as_array_mut()
        .ok_or_else(|| format!("{last:?} is not an array of tables"))?;
    array.push(Value::Object(Map::new()));
    Ok(Current::ArrayTable(path.to_vec()))
}

/// Inserts a key/value pair into the current table.
fn insert_value(root: &mut Value, current: &Current, key: String, value: Value) -> Result<(), String> {
    let path = match current {
        Current::Table(path) | Current::ArrayTable(path) => path,
    };
    let mut object = root;
    for segment in path {
        let table = object
            .as_object_mut()
            .ok_or_else(|| format!("table {segment:?} is not an object"))?;
        object = table
            .get_mut(segment)
            .ok_or_else(|| format!("table {segment:?} disappeared"))?;
    }
    if matches!(current, Current::ArrayTable(_)) {
        let array = object
            .as_array_mut()
            .ok_or_else(|| format!("{:?} is not an array of tables", path.join(".")))?;
        object = array
            .last_mut()
            .ok_or_else(|| format!("{:?} has no table", path.join(".")))?;
    }
    let table = object
        .as_object_mut()
        .ok_or_else(|| format!("table {:?} is not an object", path.join(".")))?;
    if table.contains_key(&key) {
        return Err(format!("duplicate key {key:?}"));
    }
    table.insert(key, value);
    Ok(())
}
