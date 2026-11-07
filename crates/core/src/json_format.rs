//! Format JSON (pretty-print) and Compress JSON (minify), as Notepad++'s JSON
//! Viewer plugin does.
//!
//! The text is rewritten token by token, not through a JSON value: keys keep
//! their order, numbers and strings keep their exact spelling (`1.0`,
//! `"é"`), and duplicate keys survive. Trailing commas, which JSONC files
//! allow, are dropped. Comments cannot be placed reliably in either layout, so
//! text with comments is refused rather than losing them.

/// Why the text could not be formatted. `offset` is a byte offset into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonError {
    pub offset: usize,
    pub kind: JsonErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonErrorKind {
    /// Not valid JSON at `offset`.
    Invalid,
    /// A comment at `offset`.
    Comment,
}

/// Containers nested deeper than this are refused, so the parser's recursion
/// stays well inside the stack.
const MAX_DEPTH: usize = 512;

/// Rewrites the JSON value in `text`, one member per line indented by
/// `indent`, or with no whitespace at all when `indent` is `None`. Whitespace
/// around the value is dropped.
pub fn format_json(text: &str, indent: Option<&str>) -> Result<String, JsonError> {
    let mut writer = Writer { text, at: 0, out: String::with_capacity(text.len()), indent };
    writer.skip_space()?;
    writer.value(0)?;
    writer.skip_space()?;
    if writer.at < text.len() {
        return Err(writer.invalid());
    }
    Ok(writer.out)
}

struct Writer<'a> {
    text: &'a str,
    at: usize,
    out: String,
    indent: Option<&'a str>,
}

impl Writer<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn invalid(&self) -> JsonError {
        JsonError { offset: self.at, kind: JsonErrorKind::Invalid }
    }

    fn skip_space(&mut self) -> Result<(), JsonError> {
        while let Some(byte) = self.peek() {
            match byte {
                b' ' | b'\t' | b'\n' | b'\r' => self.at += 1,
                b'/' if matches!(self.text.as_bytes().get(self.at + 1), Some(b'/' | b'*')) => {
                    return Err(JsonError { offset: self.at, kind: JsonErrorKind::Comment });
                }
                _ => break,
            }
        }
        Ok(())
    }

    /// A line break and the indentation of `depth`, when pretty-printing.
    fn new_line(&mut self, depth: usize) {
        if let Some(indent) = self.indent {
            self.out.push('\n');
            for _ in 0..depth {
                self.out.push_str(indent);
            }
        }
    }

    /// Writes the value at the cursor, which is past any space before it.
    fn value(&mut self, depth: usize) -> Result<(), JsonError> {
        match self.peek() {
            Some(b'{') => self.container(depth, b'}', true),
            Some(b'[') => self.container(depth, b']', false),
            Some(b'"') => self.string(),
            _ => self.literal(),
        }
    }

    /// An object (`object`) or an array, from its opening bracket.
    fn container(&mut self, depth: usize, close: u8, object: bool) -> Result<(), JsonError> {
        if depth >= MAX_DEPTH {
            return Err(self.invalid());
        }
        self.out.push(self.text.as_bytes()[self.at] as char);
        self.at += 1;
        self.skip_space()?;
        if self.peek() == Some(close) {
            self.at += 1;
            self.out.push(close as char);
            return Ok(());
        }
        loop {
            self.new_line(depth + 1);
            if object {
                if self.peek() != Some(b'"') {
                    return Err(self.invalid());
                }
                self.string()?;
                self.skip_space()?;
                if self.peek() != Some(b':') {
                    return Err(self.invalid());
                }
                self.at += 1;
                self.out.push(':');
                if self.indent.is_some() {
                    self.out.push(' ');
                }
                self.skip_space()?;
            }
            self.value(depth + 1)?;
            self.skip_space()?;
            match self.peek() {
                Some(b',') => {
                    self.at += 1;
                    self.skip_space()?;
                    // A trailing comma, as JSONC allows: dropped.
                    if self.peek() == Some(close) {
                        break;
                    }
                    self.out.push(',');
                }
                Some(byte) if byte == close => break,
                _ => return Err(self.invalid()),
            }
        }
        self.at += 1;
        self.new_line(depth);
        self.out.push(close as char);
        Ok(())
    }

    /// A string, copied as written, from its opening quote.
    fn string(&mut self) -> Result<(), JsonError> {
        let start = self.at;
        let bytes = self.text.as_bytes();
        let mut at = start + 1;
        loop {
            match bytes.get(at) {
                Some(b'"') => break,
                Some(b'\\') => at += 2,
                // A raw line break or other control character: the string was never closed.
                Some(byte) if *byte < 0x20 => {
                    self.at = at;
                    return Err(self.invalid());
                }
                Some(_) => at += 1,
                None => {
                    self.at = start;
                    return Err(self.invalid());
                }
            }
        }
        self.at = at + 1;
        self.out.push_str(&self.text[start..self.at]);
        Ok(())
    }

    /// A number, `true`, `false` or `null`.
    fn literal(&mut self) -> Result<(), JsonError> {
        let start = self.at;
        let end = self.text[start..]
            .find(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ':' | '{' | '}' | '[' | ']' | '"' | '/'))
            .map_or(self.text.len(), |length| start + length);
        let word = &self.text[start..end];
        if !matches!(word, "true" | "false" | "null") && !is_number(word) {
            return Err(self.invalid());
        }
        self.at = end;
        self.out.push_str(word);
        Ok(())
    }
}

/// JSON's number grammar: `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`.
fn is_number(word: &str) -> bool {
    let mut rest = word.strip_prefix('-').unwrap_or(word).as_bytes();
    let digits = |rest: &mut &[u8]| {
        let count = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
        *rest = &rest[count..];
        count
    };
    match rest.first() {
        Some(b'0') => rest = &rest[1..],
        Some(b'1'..=b'9') => {
            digits(&mut rest);
        }
        _ => return false,
    }
    if let Some(after) = rest.strip_prefix(b".") {
        rest = after;
        if digits(&mut rest) == 0 {
            return false;
        }
    }
    if let Some(after) = rest.strip_prefix(b"e").or_else(|| rest.strip_prefix(b"E")) {
        rest = after.strip_prefix(b"+").or_else(|| after.strip_prefix(b"-")).unwrap_or(after);
        if digits(&mut rest) == 0 {
            return false;
        }
    }
    rest.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pretty(text: &str) -> String {
        format_json(text, Some("  ")).unwrap()
    }

    #[test]
    fn pretty_prints_one_member_per_line() {
        assert_eq!(
            pretty(r#"{"a":1,"b":[true,null,{"c":"x"}],"d":{}, "e":[ ]}"#),
            "{\n  \"a\": 1,\n  \"b\": [\n    true,\n    null,\n    {\n      \"c\": \"x\"\n    }\n  ],\n  \"d\": {},\n  \"e\": []\n}"
        );
    }

    #[test]
    fn compresses_to_no_whitespace() {
        let text = "{\n  \"a\" : [ 1 , 2 ],\n  \"b\": \"x y\"\n}\n";
        assert_eq!(format_json(text, None).unwrap(), r#"{"a":[1,2],"b":"x y"}"#);
    }

    #[test]
    fn keeps_spelling_order_and_duplicates() {
        let text = r#"{"z":1.0,"a":"é\"\\","z":-0.5e+10}"#;
        assert_eq!(format_json(text, None).unwrap(), text);
    }

    #[test]
    fn indents_with_tabs_and_formats_scalars() {
        assert_eq!(format_json("[1]", Some("\t")).unwrap(), "[\n\t1\n]");
        assert_eq!(pretty("  \"x\"  "), "\"x\"");
        assert_eq!(pretty("42"), "42");
    }

    #[test]
    fn drops_trailing_commas() {
        assert_eq!(format_json("{\"a\": [1, 2,],}", None).unwrap(), r#"{"a":[1,2]}"#);
    }

    #[test]
    fn reports_where_the_text_is_not_json() {
        let invalid = |text: &str| format_json(text, None).unwrap_err();
        assert_eq!(invalid("{\"a\": 1 \"b\": 2}"), JsonError { offset: 8, kind: JsonErrorKind::Invalid });
        assert_eq!(invalid("{a: 1}").offset, 1);
        assert_eq!(invalid("[1, 2").offset, 5);
        assert_eq!(invalid("[01]").offset, 1);
        assert_eq!(invalid("\"open").offset, 0);
        assert_eq!(invalid("{} {}").offset, 3);
        assert_eq!(invalid("").offset, 0);
        assert_eq!(invalid("[tru]").kind, JsonErrorKind::Invalid);
    }

    #[test]
    fn refuses_comments() {
        let error = format_json("{\n  // note\n  \"a\": 1\n}", Some("  ")).unwrap_err();
        assert_eq!(error, JsonError { offset: 4, kind: JsonErrorKind::Comment });
        assert_eq!(format_json("[1 /* x */]", None).unwrap_err().kind, JsonErrorKind::Comment);
    }

    #[test]
    fn refuses_deep_nesting_instead_of_overflowing() {
        let text = "[".repeat(10_000);
        assert_eq!(format_json(&text, None).unwrap_err().kind, JsonErrorKind::Invalid);
    }

    #[test]
    fn reads_numbers_as_json_does() {
        for good in ["0", "-0", "12", "1.5", "1e9", "1E-2", "-3.25e+4"] {
            assert!(is_number(good), "{good}");
        }
        for bad in ["", "-", "01", "1.", ".5", "1e", "+1", "0x1", "NaN", "1.2.3"] {
            assert!(!is_number(bad), "{bad}");
        }
    }
}
