//! Comment and uncomment, as Notepad++'s Edit ▸ Comment/Uncomment does it:
//! single-line comments on each line, block comments around a selection, and
//! for languages with only block comments (HTML, CSS…) each line wrapped in
//! the block delimiters. The symbols are Notepad++'s (`langs.model.xml`).
//!
//! Everything works on plain text and byte ranges, so it is tested without
//! an editor.

use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommentSymbols {
    /// Single-line comment, such as `//` or `#`; empty when the language has none.
    pub line: &'static str,
    /// Block comment delimiters, such as `/*` and `*/`; empty when the language has none.
    pub start: &'static str,
    pub end: &'static str,
}

impl CommentSymbols {
    pub fn has_block(&self) -> bool {
        !self.start.is_empty() && !self.end.is_empty()
    }

    /// Whether any comment command can do something in this language.
    pub fn any(&self) -> bool {
        !self.line.is_empty() || self.has_block()
    }
}

const C_LIKE: CommentSymbols = CommentSymbols { line: "//", start: "/*", end: "*/" };
const HASH: CommentSymbols = CommentSymbols { line: "#", start: "", end: "" };
const HTML: CommentSymbols = CommentSymbols { line: "", start: "<!--", end: "-->" };

/// The comment symbols of an OxyPad language (`oxypad_syntax`'s names); none
/// for plain text and for languages that have no comments.
pub fn comment_symbols(language: &str) -> CommentSymbols {
    match language {
        "c" | "cpp" | "csharp" | "go" | "java" | "javascript" | "tsx" | "typescript" | "kotlin" | "php" | "proto"
        | "rust" | "scala" | "swift" => C_LIKE,
        "bash" | "cmake" | "elixir" | "graphql" | "make" | "perl" | "python" | "ruby" | "toml" | "yaml" => HASH,
        "astro" | "ejs" | "erb" | "html" | "svelte" | "xml" => HTML,
        "batch" => CommentSymbols { line: "REM", start: "", end: "" },
        "css" => CommentSymbols { line: "", start: "/*", end: "*/" },
        "ini" => CommentSymbols { line: ";", start: "", end: "" },
        "lua" => CommentSymbols { line: "--", start: "", end: "" },
        "powershell" => CommentSymbols { line: "#", start: "<#", end: "#>" },
        "sql" => CommentSymbols { line: "--", start: "/*", end: "*/" },
        "zig" => CommentSymbols { line: "//", start: "", end: "" },
        // json, markdown, diff and plain text
        _ => CommentSymbols::default(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentMode {
    Toggle,
    Comment,
    Uncomment,
}

/// What a line comment command did to the lines.
pub struct LineComments {
    pub lines: Vec<String>,
    /// How many lines lost a comment.
    pub uncommented: usize,
}

/// Comments or uncomments each line: with `Toggle`, each line on its own. The
/// symbol goes after the indentation, and empty lines are left alone.
/// Languages with only block delimiters get each line wrapped in them. Returns
/// `None` when the language has no comments at all.
pub fn comment_lines(lines: &[String], mode: CommentMode, symbols: CommentSymbols) -> Option<LineComments> {
    let wrap = symbols.line.is_empty();
    if wrap && !symbols.has_block() {
        return None;
    }
    let prefix = format!("{} ", if wrap { symbols.start } else { symbols.line });
    let suffix = if wrap { format!(" {}", symbols.end) } else { String::new() };
    let mut uncommented = 0;
    let lines = lines
        .iter()
        .map(|line| {
            let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
            let (indent, content) = line.split_at(indent_len);
            if content.is_empty() {
                return line.clone();
            }
            if mode != CommentMode::Comment
                && let Some((open, close)) = comment_range(content, &prefix, &suffix)
            {
                uncommented += 1;
                return format!("{indent}{}", &content[open..content.len() - close]);
            }
            if mode != CommentMode::Uncomment {
                return format!("{indent}{prefix}{content}{suffix}");
            }
            line.clone()
        })
        .collect();
    Some(LineComments { lines, uncommented })
}

/// The bytes to remove from the start and end of a commented line, or `None`
/// if it is not commented. The symbol matches in any case ("REM", "rem"), and
/// the space after it is optional.
fn comment_range(content: &str, prefix: &str, suffix: &str) -> Option<(usize, usize)> {
    let opener = prefix.trim_end_matches(' ');
    if !starts_with_ignore_case(content, opener) {
        return None;
    }
    let mut start = opener.len();
    if content[start..].starts_with(' ') {
        start += 1;
    }
    let closer = suffix.trim_start_matches(' ');
    if closer.is_empty() {
        return Some((start, 0));
    }
    if content.len() < start + closer.len() || !content.is_char_boundary(content.len() - closer.len()) {
        return None;
    }
    if !content[content.len() - closer.len()..].eq_ignore_ascii_case(closer) {
        return None;
    }
    let mut end = closer.len();
    let before = &content[..content.len() - end];
    if before.ends_with(' ') && before.len() > start {
        end += 1;
    }
    Some((start, end))
}

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len() && text.is_char_boundary(prefix.len()) && text[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// A change to the text: `range` of the old text becomes `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replacement {
    pub range: Range<usize>,
    pub text: String,
}

/// Wraps the selection (or, with none, the text of the caret's line) in the
/// block delimiters, Notepad++'s "Block Comment". Returns the changes and the
/// selection to leave, in the new text. `None` when the language has no block
/// comments.
pub fn block_comment(
    text: &str,
    selection: Range<usize>,
    symbols: CommentSymbols,
) -> Option<(Vec<Replacement>, Range<usize>)> {
    if !symbols.has_block() {
        return None;
    }
    let (mut start, mut end) = (selection.start, selection.end);
    if start == end {
        let line_start = text[..start].rfind(['\n', '\r']).map_or(0, |at| at + 1);
        let line_end = start + text[start..].find(['\n', '\r']).unwrap_or(text.len() - start);
        let indent = text[line_start..line_end].len() - text[line_start..line_end].trim_start_matches([' ', '\t']).len();
        (start, end) = (line_start + indent, line_end);
    }
    let opener = format!("{} ", symbols.start);
    let closer = format!(" {}", symbols.end);
    let changes = if start == end {
        vec![Replacement { range: start..end, text: format!("{opener}{closer}") }]
    } else {
        vec![
            Replacement { range: start..start, text: opener.clone() },
            Replacement { range: end..end, text: closer },
        ]
    };
    Some((changes, start + opener.len()..end + opener.len()))
}

/// Removes the block comment around the selection's start, then any that
/// start inside the selection, Notepad++'s "Block Uncomment". Returns the
/// changes and the selection to leave, in the new text; `None` when there was
/// nothing to remove.
pub fn block_uncomment(
    text: &str,
    selection: Range<usize>,
    symbols: CommentSymbols,
) -> Option<(Vec<Replacement>, Range<usize>)> {
    if !symbols.has_block() {
        return None;
    }
    let mut removals: Vec<Range<usize>> = Vec::new();
    let mut search_from = selection.start;
    let mut first = true;
    while let Some(comment) = find_block_comment(text, search_from, first, selection.end, symbols) {
        first = false;
        let mut open_end = comment.start + symbols.start.len();
        if text[open_end..].starts_with(' ') {
            open_end += 1;
        }
        let mut close_start = comment.end - symbols.end.len();
        if close_start > open_end && text[..close_start].ends_with(' ') {
            close_start -= 1;
        }
        // The opener's removal first, so the removals stay in order.
        removals.push(comment.start..open_end);
        removals.push(close_start.max(open_end)..comment.end);
        search_from = comment.end;
        if comment.end >= selection.end {
            break;
        }
    }
    if removals.is_empty() {
        return None;
    }
    let shifted = |position: usize| -> usize {
        let removed: usize = removals
            .iter()
            .map(|range| {
                if position >= range.end {
                    range.len()
                } else if position > range.start {
                    position - range.start
                } else {
                    0
                }
            })
            .sum();
        position - removed
    };
    let selection = shifted(selection.start)..shifted(selection.end);
    let changes = removals.into_iter().map(|range| Replacement { range, text: String::new() }).collect();
    Some((changes, selection))
}

/// First pass: the comment that encloses `position`. Later passes: the next
/// comment that starts before `limit`.
fn find_block_comment(
    text: &str,
    position: usize,
    first: bool,
    limit: usize,
    symbols: CommentSymbols,
) -> Option<Range<usize>> {
    let open = if first {
        // Backwards from just past the caret, so a caret inside "/*" counts.
        let until = (position + symbols.start.len()).min(text.len());
        let until = (until..=text.len()).find(|&at| text.is_char_boundary(at))?;
        text[..until].rfind(symbols.start)?
    } else {
        if position >= limit {
            return None;
        }
        position + text[position..].find(symbols.start)?
    };
    if !first && open >= limit {
        return None;
    }
    let after_open = open + symbols.start.len();
    let close = after_open + text[after_open..].find(symbols.end)?;
    let comment = open..close + symbols.end.len();
    // The comment found backwards must reach the caret.
    if first && comment.end < position {
        return None;
    }
    Some(comment)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn apply(text: &str, mut changes: Vec<Replacement>) -> String {
        let mut text = text.to_owned();
        changes.sort_by_key(|change| std::cmp::Reverse(change.range.start));
        for change in changes {
            text.replace_range(change.range, &change.text);
        }
        text
    }

    #[test]
    fn symbols_by_language() {
        assert_eq!(comment_symbols("cpp").line, "//");
        assert_eq!(comment_symbols("python"), HASH);
        assert_eq!(comment_symbols("html"), HTML);
        assert!(!comment_symbols("json").any());
        assert!(!comment_symbols("nothing-like-it").any());
    }

    #[test]
    fn toggle_keeps_indentation_and_skips_empty_lines() {
        let source = lines(&["int a;", "    int b;", "", "int c;"]);
        let commented = comment_lines(&source, CommentMode::Toggle, comment_symbols("cpp")).unwrap();
        assert_eq!(commented.lines, lines(&["// int a;", "    // int b;", "", "// int c;"]));
        assert_eq!(commented.uncommented, 0);
        let restored = comment_lines(&commented.lines, CommentMode::Toggle, comment_symbols("cpp")).unwrap();
        assert_eq!(restored.lines, source);
        assert_eq!(restored.uncommented, 3);
    }

    #[test]
    fn uncomment_without_a_space_and_in_any_case() {
        let cpp = comment_symbols("cpp");
        assert_eq!(comment_lines(&lines(&["//a", "// b"]), CommentMode::Uncomment, cpp).unwrap().lines, lines(&["a", "b"]));
        assert_eq!(
            comment_lines(&lines(&["rem echo"]), CommentMode::Uncomment, comment_symbols("batch")).unwrap().lines,
            lines(&["echo"])
        );
    }

    #[test]
    fn commenting_only_comments_and_uncommenting_only_uncomments() {
        let cpp = comment_symbols("cpp");
        let mixed = lines(&["// a", "b"]);
        assert_eq!(comment_lines(&mixed, CommentMode::Comment, cpp).unwrap().lines, lines(&["// // a", "// b"]));
        assert_eq!(comment_lines(&mixed, CommentMode::Uncomment, cpp).unwrap().lines, lines(&["a", "b"]));
    }

    #[test]
    fn block_only_languages_wrap_each_line() {
        let html = comment_symbols("html");
        let commented = comment_lines(&lines(&["<p>hi</p>"]), CommentMode::Toggle, html).unwrap();
        assert_eq!(commented.lines, lines(&["<!-- <p>hi</p> -->"]));
        assert_eq!(comment_lines(&commented.lines, CommentMode::Toggle, html).unwrap().lines, lines(&["<p>hi</p>"]));
    }

    #[test]
    fn languages_without_comments_do_nothing() {
        assert!(comment_lines(&lines(&["a"]), CommentMode::Toggle, comment_symbols("json")).is_none());
    }

    #[test]
    fn block_comment_and_uncomment() {
        let cpp = comment_symbols("cpp");
        let source = "int a = 1;";
        let (changes, selection) = block_comment(source, 8..9, cpp).unwrap();
        let commented = apply(source, changes);
        assert_eq!(commented, "int a = /* 1 */;");
        assert_eq!(&commented[selection], "1");
        let (changes, _) = block_uncomment(&commented, 12..12, cpp).unwrap();
        assert_eq!(apply(&commented, changes), source);
    }

    #[test]
    fn block_comment_without_a_selection_takes_the_line() {
        let (changes, _) = block_comment("a\n  b c\nd", 4..4, comment_symbols("cpp")).unwrap();
        assert_eq!(apply("a\n  b c\nd", changes), "a\n  /* b c */\nd");
        let (changes, _) = block_comment("", 0..0, comment_symbols("cpp")).unwrap();
        assert_eq!(apply("", changes), "/*  */");
    }

    #[test]
    fn block_uncomment_removes_every_comment_in_the_selection() {
        let cpp = comment_symbols("cpp");
        let source = "/* a */ b /* c */";
        let (changes, selection) = block_uncomment(source, 0..source.len(), cpp).unwrap();
        let result = apply(source, changes);
        assert_eq!(result, "a b c");
        assert_eq!(selection, 0..result.len());
        assert!(block_uncomment("plain", 0..5, cpp).is_none());
    }

    #[test]
    fn block_comments_need_block_symbols() {
        assert!(block_comment("x", 0..1, comment_symbols("python")).is_none());
        assert!(block_uncomment("x", 0..1, comment_symbols("python")).is_none());
    }
}
