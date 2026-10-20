//! Syntax checks for data and configuration files: JSON, YAML, TOML and
//! XML. Each file is parsed as a whole and the first error is reported,
//! with where it is; nothing is run but the parsers, nothing is installed.
//!
//! JSON may have comments and trailing commas (JSONC, as in `tsconfig.json`
//! or VS Code's settings) when its name says so or when it has comments.

use std::path::Path;

/// The first syntax error in a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// 0-based.
    pub line: usize,
    /// 0-based, in characters.
    pub column: usize,
    pub message: String,
}

/// Whether `language` (a highlighter language name) is checked.
pub fn is_supported(language: &str) -> bool {
    matches!(language, "json" | "yaml" | "toml" | "xml")
}

/// Checks `text`, written in `language`, from the file at `path` if any.
/// `None` when it parses, or when the language is not checked.
pub fn check(language: &str, path: Option<&Path>, text: &str) -> Option<Problem> {
    match language {
        "json" => json(text, path),
        "yaml" => yaml(text),
        "toml" => toml_check(text),
        "xml" => xml(text),
        _ => None,
    }
}

fn json(text: &str, path: Option<&Path>) -> Option<Problem> {
    // Blanking keeps every byte where it was, so positions in `parsed` are
    // positions in `text`.
    let (stripped, had_comments) = strip_json_comments(text);
    let parsed = if had_comments || is_jsonc(path) { allow_trailing_commas(&stripped) } else { stripped };
    let err = serde_json::from_str::<serde::de::IgnoredAny>(&parsed).err()?;
    if err.line() == 0 {
        return Some(Problem { line: 0, column: 0, message: err.to_string() });
    }
    // serde_json counts columns in bytes, from 1, and adds where to its message.
    let line = err.line() - 1;
    let column = chars_before(text.lines().nth(line).unwrap_or(""), err.column().saturating_sub(1));
    let message = err.to_string();
    let message = message.rsplit_once(" at line ").map_or(message.as_str(), |(message, _)| message);
    Some(Problem { line, column, message: capitalized(message) })
}

/// Files that are JSON with comments by convention.
fn is_jsonc(path: Option<&Path>) -> bool {
    let Some(path) = path else { return false };
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("").to_ascii_lowercase();
    name.ends_with(".jsonc")
        || name.starts_with("tsconfig")
        || name.starts_with("jsconfig")
        || name == "devcontainer.json"
        || path.components().any(|part| part.as_os_str() == ".vscode")
}

/// `text` with its `//` and `/* */` comments blanked out, keeping every
/// position, and whether it had any.
fn strip_json_comments(text: &str) -> (String, bool) {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut had_comments = false;
    let mut ix = 0;
    let mut in_string = false;
    while ix < bytes.len() {
        let byte = bytes[ix];
        if in_string {
            out.push(byte);
            if byte == b'\\' && ix + 1 < bytes.len() {
                out.push(bytes[ix + 1]);
                ix += 1;
            } else if byte == b'"' {
                in_string = false;
            }
            ix += 1;
            continue;
        }
        match (byte, bytes.get(ix + 1)) {
            (b'"', _) => {
                in_string = true;
                out.push(byte);
                ix += 1;
            }
            (b'/', Some(b'/')) => {
                had_comments = true;
                while ix < bytes.len() && bytes[ix] != b'\n' {
                    out.push(blank(bytes[ix]));
                    ix += 1;
                }
            }
            (b'/', Some(b'*')) => {
                had_comments = true;
                let end = text[ix + 2..].find("*/").map_or(bytes.len(), |end| ix + 2 + end + 2);
                out.extend(bytes[ix..end].iter().map(|&byte| blank(byte)));
                ix = end;
            }
            _ => {
                out.push(byte);
                ix += 1;
            }
        }
    }
    // Only ASCII bytes were replaced, and only by ASCII.
    (String::from_utf8(out).unwrap_or_else(|_| text.to_owned()), had_comments)
}

/// A comment byte as blank: line breaks stay, so lines keep their numbers;
/// the bytes of a character all become spaces, so columns keep counting.
fn blank(byte: u8) -> u8 {
    if byte == b'\n' || byte == b'\r' { byte } else { b' ' }
}

/// `text` with commas before a closing `}` or `]` blanked out.
fn allow_trailing_commas(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let mut in_string = false;
    let mut ix = 0;
    while ix < bytes.len() {
        let byte = bytes[ix];
        if in_string {
            if byte == b'\\' {
                ix += 1;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if byte == b'"' {
            in_string = true;
        } else if byte == b',' {
            let next = bytes[ix + 1..].iter().find(|byte| !byte.is_ascii_whitespace());
            if matches!(next, Some(b'}' | b']')) {
                out[ix] = b' ';
            }
        }
        ix += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

fn yaml(text: &str) -> Option<Problem> {
    for event in saphyr_parser::Parser::new_from_str(text) {
        if let Err(err) = event {
            let marker = err.marker();
            let line = marker.line().saturating_sub(1);
            let column = marker.col();
            return Some(Problem { line, column, message: capitalized(err.info()) });
        }
    }
    None
}

fn toml_check(text: &str) -> Option<Problem> {
    let err = text.parse::<toml::Table>().err()?;
    let offset = err.span().map_or(0, |span| span.start);
    let (line, column) = line_and_column(text, offset);
    Some(Problem { line, column, message: capitalized(err.message().trim()) })
}

fn xml(text: &str) -> Option<Problem> {
    let options = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let err = roxmltree::Document::parse_with_options(text, options).err()?;
    let position = err.pos();
    let message = err.to_string();
    let message = message.rsplit_once(" at ").map_or(message.as_str(), |(message, _)| message);
    Some(Problem {
        line: (position.row as usize).saturating_sub(1),
        column: (position.col as usize).saturating_sub(1),
        message: capitalized(message),
    })
}

/// The 0-based line and character column of byte `offset` in `text`.
fn line_and_column(text: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |ix| ix + 1);
    (line, before[line_start..].chars().count())
}

/// Characters in the first `bytes` bytes of `line`.
fn chars_before(line: &str, bytes: usize) -> usize {
    let mut bytes = bytes.min(line.len());
    while !line.is_char_boundary(bytes) {
        bytes -= 1;
    }
    line[..bytes].chars().count()
}

fn capitalized(message: &str) -> String {
    let mut chars = message.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests;
