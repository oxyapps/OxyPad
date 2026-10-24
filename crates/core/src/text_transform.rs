//! The text transformations behind the Edit menu's Convert Case, Line
//! Operations and Blank Operations, ported from MacPad++, which ported them
//! from Notepad++ (`ScintillaEditView::changeCase`, `Sorters.h`,
//! `Notepad_plus::wsTabConvert`). They work on plain strings, so they are
//! tested without an editor.

use std::cmp::Ordering;

use crate::password::random_below;

// ---- Case ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Upper,
    Lower,
    /// Capitalizes each word and lowercases the rest.
    ProperForce,
    /// Capitalizes each word, leaving the rest as it is.
    ProperBlend,
    /// Capitalizes each sentence and lowercases the rest.
    SentenceForce,
    /// Capitalizes each sentence, leaving the rest as it is.
    SentenceBlend,
    Invert,
    Random,
}

pub fn convert_case(text: &str, case: Case) -> String {
    match case {
        Case::Upper => text.to_uppercase(),
        Case::Lower => text.to_lowercase(),
        Case::ProperForce | Case::ProperBlend => proper_case(text, case == Case::ProperForce),
        Case::SentenceForce | Case::SentenceBlend => sentence_case(text, case == Case::SentenceForce),
        Case::Invert => text
            .chars()
            .map(|ch| if ch.is_lowercase() { ch.to_uppercase().collect() } else { ch.to_lowercase().collect::<String>() })
            .collect(),
        Case::Random => text
            .chars()
            .map(|ch| match ch.is_alphabetic() {
                true if random_below(2) == Some(1) => ch.to_uppercase().collect::<String>(),
                true => ch.to_lowercase().collect(),
                false => ch.to_string(),
            })
            .collect(),
    }
}

fn is_single_quote(ch: char) -> bool {
    ch == '\'' || ch == '\u{2019}'
}

fn is_alphanumeric(ch: char) -> bool {
    ch.is_alphabetic() || ch.is_numeric()
}

/// "it's a dog" → "It's A Dog": the letter after an apostrophe inside a word stays small.
fn proper_case(text: &str, force: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len());
    for (index, &ch) in chars.iter().enumerate() {
        if !ch.is_alphabetic() {
            result.push(ch);
            continue;
        }
        let keep = |result: &mut String| {
            if force {
                result.extend(ch.to_lowercase());
            } else {
                result.push(ch);
            }
        };
        if index >= 2 && is_single_quote(chars[index - 1]) && is_alphanumeric(chars[index - 2]) {
            keep(&mut result);
        } else if index == 0 || !is_alphanumeric(chars[index - 1]) {
            result.extend(ch.to_uppercase());
        } else {
            keep(&mut result);
        }
    }
    result
}

/// A sentence starts the text, follows ". ", "! " or "? ", or follows a blank
/// line. A lone "i" becomes "I", as in English.
fn sentence_case(text: &str, force: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len());
    let mut new_sentence = true;
    let mut line_breaks = 0;
    for (index, &ch) in chars.iter().enumerate() {
        if ch.is_alphabetic() {
            let mut converted: String = if new_sentence {
                new_sentence = false;
                ch.to_uppercase().collect()
            } else if force {
                ch.to_lowercase().collect()
            } else {
                ch.to_string()
            };
            line_breaks = 0;
            if converted == "i"
                && index > 0
                && (chars[index - 1].is_whitespace() || "(\"".contains(chars[index - 1]))
                && index + 1 < chars.len()
                && (chars[index + 1].is_whitespace() || chars[index + 1] == '\'')
            {
                converted = "I".into();
            }
            result.push_str(&converted);
            continue;
        }
        if ".!?".contains(ch) {
            new_sentence = !(index + 1 == chars.len() || is_alphanumeric(chars[index + 1]));
        } else if ch == '\n' || ch == '\r' {
            line_breaks += 1;
            if line_breaks >= 2 {
                new_sentence = true;
            }
        }
        result.push(ch);
    }
    result
}

// ---- Sorting ---------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    LexicographicAscending,
    LexicographicDescending,
    CaseInsensitiveAscending,
    CaseInsensitiveDescending,
    LocaleAscending,
    LocaleDescending,
    IntegerAscending,
    IntegerDescending,
    DecimalCommaAscending,
    DecimalCommaDescending,
    DecimalDotAscending,
    DecimalDotDescending,
    LengthAscending,
    LengthDescending,
    Reverse,
    Random,
}

impl Sort {
    fn is_descending(self) -> bool {
        matches!(
            self,
            Self::LexicographicDescending
                | Self::CaseInsensitiveDescending
                | Self::LocaleDescending
                | Self::IntegerDescending
                | Self::DecimalCommaDescending
                | Self::DecimalDotDescending
                | Self::LengthDescending
        )
    }
}

/// A numeric sort met a line that does not start with a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortError {
    /// 0-based index of the line among those sorted.
    pub line_index: usize,
}

pub fn sort_lines(lines: &[String], sort: Sort) -> Result<Vec<String>, SortError> {
    let descending = sort.is_descending();
    let ordered = |compare: &dyn Fn(&str, &str) -> Ordering| {
        let mut sorted = lines.to_vec();
        // Stable, like Notepad++'s sort: equal lines keep their order in both directions.
        sorted.sort_by(|a, b| if descending { compare(b, a) } else { compare(a, b) });
        sorted
    };
    Ok(match sort {
        Sort::LexicographicAscending | Sort::LexicographicDescending => {
            ordered(&|a, b| utf16(a).cmp(&utf16(b)))
        }
        Sort::CaseInsensitiveAscending | Sort::CaseInsensitiveDescending => {
            ordered(&|a, b| utf16(&a.to_lowercase()).cmp(&utf16(&b.to_lowercase())))
        }
        Sort::LocaleAscending | Sort::LocaleDescending => ordered(&locale_compare),
        Sort::IntegerAscending | Sort::IntegerDescending => {
            ordered(&|a, b| compare_natural(&utf16(a), &utf16(b)).cmp(&0))
        }
        Sort::DecimalCommaAscending | Sort::DecimalCommaDescending => numeric_sort(lines, descending, true)?,
        Sort::DecimalDotAscending | Sort::DecimalDotDescending => numeric_sort(lines, descending, false)?,
        Sort::LengthAscending | Sort::LengthDescending => {
            ordered(&|a, b| a.encode_utf16().count().cmp(&b.encode_utf16().count()))
        }
        Sort::Reverse => lines.iter().rev().cloned().collect(),
        Sort::Random => {
            let mut shuffled = lines.to_vec();
            for index in (1..shuffled.len()).rev() {
                if let Some(other) = random_below(index + 1) {
                    shuffled.swap(index, other);
                }
            }
            shuffled
        }
    })
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// An approximation of the system's locale order, which Rust has no access
/// to: letters compare ignoring case and accents, and when only those differ,
/// lowercase comes before uppercase and unaccented before accented.
fn locale_compare(a: &str, b: &str) -> Ordering {
    let key = |text: &str| -> Vec<char> { text.chars().flat_map(char::to_lowercase).map(fold_accent).collect() };
    key(a).cmp(&key(b)).then_with(|| {
        // Lowercase first, as in most locales.
        let lowercase_first = |text: &str| -> Vec<bool> { text.chars().map(char::is_uppercase).collect() };
        lowercase_first(a).cmp(&lowercase_first(b)).then_with(|| a.cmp(b))
    })
}

/// The base letter of a Latin letter with a diacritic.
fn fold_accent(ch: char) -> char {
    match ch {
        'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'ď' | 'đ' => 'd',
        'è'..='ë' | 'ē' | 'ę' | 'ě' => 'e',
        'ì'..='ï' | 'ī' | 'į' | 'ı' => 'i',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò'..='ö' | 'ø' | 'ō' | 'ő' => 'o',
        'ř' => 'r',
        'ś' | 'š' | 'ş' => 's',
        'ť' | 'ţ' => 't',
        'ù'..='ü' | 'ū' | 'ů' | 'ű' | 'ų' => 'u',
        'ý' | 'ÿ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        other => other,
    }
}

fn is_digit(unit: u16) -> bool {
    (0x30..=0x39).contains(&unit)
}

const MINUS: u16 = 0x2D;

fn compare_units(a: &[u16], b: &[u16]) -> i32 {
    match a.cmp(b) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// Notepad++'s "Sort as Integers": text compared chunk by chunk, with runs of
/// digits (optionally negative) compared as numbers of any size.
fn compare_natural(a: &[u16], b: &[u16]) -> i32 {
    let (mut i, mut j) = (0, 0);
    loop {
        if i >= a.len() || j >= b.len() {
            return compare_units(&a[i.min(a.len())..], &b[j.min(b.len())..]);
        }
        let (mut a_is_number, mut b_is_number) = (is_digit(a[i]), is_digit(b[j]));
        let (mut a_sign, mut b_sign) = (1, 1);
        if !a_is_number && i + 1 < a.len() {
            a_is_number = a[i] == MINUS && is_digit(a[i + 1]);
            a_sign = -1;
        }
        if !b_is_number && j + 1 < b.len() {
            b_is_number = b[j] == MINUS && is_digit(b[j + 1]);
            b_sign = -1;
        }
        if a_is_number != b_is_number {
            let result = i32::from(a[i]) - i32::from(b[j]);
            if result != 0 {
                return result;
            }
            i += 1;
            j += 1;
        } else if a_is_number {
            if a_sign != b_sign {
                return if a_sign == 1 { 1 } else { -1 };
            }
            if a_sign == -1 {
                i += 1;
                j += 1;
            }
            let (mut a_end, mut b_end) = (i, j);
            while a_end < a.len() && is_digit(a[a_end]) {
                a_end += 1;
            }
            while b_end < b.len() && is_digit(b[b_end]) {
                b_end += 1;
            }
            let (mut a_zeros, mut b_zeros) = (0, 0);
            while i < a_end && a[i] == 0x30 {
                a_zeros += 1;
                i += 1;
            }
            while j < b_end && b[j] == 0x30 {
                b_zeros += 1;
                j += 1;
            }
            let (a_length, b_length) = (a_end - i, b_end - j);
            if a_length != b_length {
                return if a_length > b_length { a_sign } else { -a_sign };
            }
            while i < a_end {
                let result = (i32::from(a[i]) - i32::from(b[j])) * a_sign;
                if result != 0 {
                    return result;
                }
                i += 1;
                j += 1;
            }
            if b_zeros != a_zeros {
                return b_zeros - a_zeros;
            }
        } else {
            if a[i] == MINUS {
                i += 1;
            }
            if b[j] == MINUS {
                j += 1;
            }
            let (mut a_end, mut b_end) = (i, j);
            while a_end < a.len() && !is_digit(a[a_end]) && a[a_end] != MINUS {
                a_end += 1;
            }
            while b_end < b.len() && !is_digit(b[b_end]) && b[b_end] != MINUS {
                b_end += 1;
            }
            let result = compare_units(&a[i..a_end], &b[j..b_end]);
            if result != 0 {
                return result;
            }
            i = a_end;
            j = b_end;
        }
    }
}

/// Lines are read as decimal numbers; blank lines go first (last when descending).
fn numeric_sort(lines: &[String], descending: bool, decimal_comma: bool) -> Result<Vec<String>, SortError> {
    let admissible = |ch: char| {
        matches!(ch, ' ' | '\t' | '0'..='9' | '-') || ch == if decimal_comma { ',' } else { '.' }
    };
    let mut numbered: Vec<(&String, f64)> = Vec::new();
    let mut blanks: Vec<String> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let mut prepared: String = line.chars().take_while(|&ch| admissible(ch)).collect();
        if decimal_comma {
            prepared = prepared.replace(',', ".");
        }
        let trimmed = prepared.trim_matches([' ', '\t']);
        if trimmed.is_empty() && line.chars().all(char::is_whitespace) {
            blanks.push(line.clone());
            continue;
        }
        let value = leading_number(trimmed).ok_or(SortError { line_index: index })?;
        numbered.push((line, value));
    }
    // Stable, so equal numbers keep their order.
    numbered.sort_by(|a, b| {
        let order = a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal);
        if descending { order.reverse() } else { order }
    });
    let sorted = numbered.into_iter().map(|(line, _)| line.clone());
    Ok(if descending { sorted.chain(blanks).collect() } else { blanks.into_iter().chain(sorted).collect() })
}

/// The number at the start of `text`, like C's strtod: "12.5-3" is 12.5.
fn leading_number(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    let mut end = usize::from(bytes.first() == Some(&b'-'));
    let digits = |from: usize| bytes[from..].iter().take_while(|byte| byte.is_ascii_digit()).count();
    let whole = digits(end);
    let mut seen_digits = whole > 0;
    end += whole;
    if bytes.get(end) == Some(&b'.') {
        let fraction = digits(end + 1);
        if whole > 0 || fraction > 0 {
            end += 1 + fraction;
            seen_digits = true;
        }
    }
    if !seen_digits {
        return None;
    }
    text[..end].parse().ok()
}

// ---- Duplicates and blank lines --------------------------------------------

pub fn remove_duplicates(lines: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    lines.iter().filter(|line| seen.insert(line.as_str())).cloned().collect()
}

pub fn remove_consecutive_duplicates(lines: &[String]) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for line in lines {
        if result.last() != Some(line) {
            result.push(line.clone());
        }
    }
    result
}

pub fn remove_empty_lines(lines: &[String], including_blank: bool) -> Vec<String> {
    lines
        .iter()
        .filter(|line| if including_blank { !line.chars().all(is_blank) } else { !line.is_empty() })
        .cloned()
        .collect()
}

// ---- Whitespace ------------------------------------------------------------

/// Spaces and tabs, the only characters the Trim commands remove.
pub fn is_blank(ch: char) -> bool {
    ch == ' ' || ch == '\t'
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trim {
    Leading,
    Trailing,
    Both,
}

pub fn trim_line(line: &str, trim: Trim) -> String {
    let line = if trim == Trim::Trailing { line } else { line.trim_start_matches(is_blank) };
    let line = if trim == Trim::Leading { line } else { line.trim_end_matches(is_blank) };
    line.to_owned()
}

/// Joins lines the way Scintilla's `SCI_LINESJOIN` does: each line ending
/// becomes a space, unless the text before it already ends in one.
pub fn join_lines(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut previous_non_blank = true;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' | '\n' => {
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if previous_non_blank {
                    result.push(' ');
                }
            }
            _ => {
                previous_non_blank = ch != ' ';
                result.push(ch);
            }
        }
    }
    result
}

/// Expands each tab to the spaces that reach the next tab stop.
pub fn tabs_to_spaces(line: &str, tab_width: usize) -> String {
    let tab_width = tab_width.max(1);
    let mut result = String::with_capacity(line.len());
    let mut column = 0;
    for ch in line.chars() {
        if ch == '\t' {
            let spaces = tab_width - column % tab_width;
            result.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else {
            result.push(ch);
            column += 1;
        }
    }
    result
}

/// Replaces runs of two or more spaces that end on a tab stop with a tab;
/// with `leading_only`, only in the indentation.
pub fn spaces_to_tabs(line: &str, tab_width: usize, leading_only: bool) -> String {
    let tab_width = tab_width.max(1);
    let mut result = String::with_capacity(line.len());
    let mut pending_spaces = 0;
    let mut column = 0;
    let mut in_indentation = true;
    for ch in line.chars() {
        let converting = !leading_only || in_indentation;
        if ch == ' ' && converting {
            pending_spaces += 1;
            column += 1;
            if column % tab_width == 0 {
                result.push(if pending_spaces > 1 { '\t' } else { ' ' });
                pending_spaces = 0;
            }
        } else if ch == '\t' && converting {
            // Spaces just before a tab are absorbed by it.
            pending_spaces = 0;
            result.push('\t');
            column += tab_width - column % tab_width;
        } else {
            result.extend(std::iter::repeat_n(' ', pending_spaces));
            pending_spaces = 0;
            result.push(ch);
            column += 1;
            in_indentation = false;
        }
    }
    result.extend(std::iter::repeat_n(' ', pending_spaces));
    result
}

/// Moves a line's indentation by `delta` columns (never below zero), writing
/// it with tabs when `hard_tabs` and with spaces otherwise, as Scintilla's
/// `SCI_SETLINEINDENTATION` does. Returns the new line.
pub fn reindent_line(line: &str, tab_width: usize, hard_tabs: bool, delta: isize) -> String {
    let tab_width = tab_width.max(1);
    let indent_len = line.len() - line.trim_start_matches(is_blank).len();
    let mut column = 0;
    for ch in line[..indent_len].chars() {
        column += if ch == '\t' { tab_width - column % tab_width } else { 1 };
    }
    let target = (column as isize + delta).max(0) as usize;
    let indent = if hard_tabs {
        "\t".repeat(target / tab_width) + &" ".repeat(target % tab_width)
    } else {
        " ".repeat(target)
    };
    format!("{indent}{}", &line[indent_len..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    // Case

    #[test]
    fn proper_case_forced_and_blended() {
        assert_eq!(convert_case("hello wORLD it's", Case::ProperForce), "Hello World It's");
        assert_eq!(convert_case("hello wORLD it's", Case::ProperBlend), "Hello WORLD It's");
    }

    #[test]
    fn sentence_case() {
        assert_eq!(
            convert_case("hello THERE. how are you? i am fine", Case::SentenceForce),
            "Hello there. How are you? I am fine"
        );
        assert_eq!(convert_case("hello THERE. how", Case::SentenceBlend), "Hello THERE. How");
        // A decimal point does not start a sentence.
        assert_eq!(convert_case("pi is 3.14 ok", Case::SentenceForce), "Pi is 3.14 ok");
        // A blank line does.
        assert_eq!(convert_case("one\n\ntwo", Case::SentenceForce), "One\n\nTwo");
    }

    #[test]
    fn upper_lower_invert_and_random_case() {
        assert_eq!(convert_case("aBc ç", Case::Upper), "ABC Ç");
        assert_eq!(convert_case("aBc Ç", Case::Lower), "abc ç");
        assert_eq!(convert_case("aBc 1", Case::Invert), "AbC 1");
        assert_eq!(convert_case("abcdef 123", Case::Random).to_lowercase(), "abcdef 123");
    }

    // Sorting

    #[test]
    fn lexicographic_sorts() {
        let items = lines(&["b", "B", "a", "C"]);
        assert_eq!(sort_lines(&items, Sort::LexicographicAscending).unwrap(), lines(&["B", "C", "a", "b"]));
        assert_eq!(sort_lines(&items, Sort::LexicographicDescending).unwrap(), lines(&["b", "a", "C", "B"]));
        assert_eq!(
            sort_lines(&lines(&["b", "C", "a"]), Sort::CaseInsensitiveAscending).unwrap(),
            lines(&["a", "b", "C"])
        );
    }

    #[test]
    fn locale_sort_ignores_case_and_accents() {
        assert_eq!(
            sort_lines(&lines(&["b", "Á", "a", "C"]), Sort::LocaleAscending).unwrap(),
            lines(&["a", "Á", "b", "C"])
        );
        assert_eq!(sort_lines(&lines(&["A", "a"]), Sort::LocaleAscending).unwrap(), lines(&["a", "A"]));
    }

    #[test]
    fn integer_sort_compares_numbers_inside_text() {
        assert_eq!(
            sort_lines(&lines(&["file10", "file2", "file1", "-3", "7"]), Sort::IntegerAscending).unwrap(),
            lines(&["-3", "7", "file1", "file2", "file10"])
        );
        assert_eq!(
            sort_lines(&lines(&["10", "9", "100"]), Sort::IntegerDescending).unwrap(),
            lines(&["100", "10", "9"])
        );
    }

    #[test]
    fn decimal_sorts() {
        assert_eq!(
            sort_lines(&lines(&["1,5", "", "-2", "1,25"]), Sort::DecimalCommaAscending).unwrap(),
            lines(&["", "-2", "1,25", "1,5"])
        );
        assert_eq!(
            sort_lines(&lines(&["1.5", "10", "2.25"]), Sort::DecimalDotDescending).unwrap(),
            lines(&["10", "2.25", "1.5"])
        );
        assert_eq!(
            sort_lines(&lines(&["1", "x", "2"]), Sort::DecimalDotAscending),
            Err(SortError { line_index: 1 })
        );
    }

    #[test]
    fn length_reverse_and_random_sorts() {
        assert_eq!(
            sort_lines(&lines(&["ccc", "a", "bb"]), Sort::LengthAscending).unwrap(),
            lines(&["a", "bb", "ccc"])
        );
        assert_eq!(sort_lines(&lines(&["1", "2", "3"]), Sort::Reverse).unwrap(), lines(&["3", "2", "1"]));
        let mut random = sort_lines(&lines(&["1", "2", "3"]), Sort::Random).unwrap();
        random.sort();
        assert_eq!(random, lines(&["1", "2", "3"]));
    }

    // Lines and whitespace

    #[test]
    fn duplicates_and_empty_lines() {
        assert_eq!(remove_duplicates(&lines(&["a", "b", "a", "c", "b"])), lines(&["a", "b", "c"]));
        assert_eq!(remove_consecutive_duplicates(&lines(&["a", "a", "b", "a"])), lines(&["a", "b", "a"]));
        let items = lines(&["a", "", " \t", "b"]);
        assert_eq!(remove_empty_lines(&items, false), lines(&["a", " \t", "b"]));
        assert_eq!(remove_empty_lines(&items, true), lines(&["a", "b"]));
    }

    #[test]
    fn trimming() {
        assert_eq!(trim_line(" \t a b \t", Trim::Leading), "a b \t");
        assert_eq!(trim_line(" \t a b \t", Trim::Trailing), " \t a b");
        assert_eq!(trim_line(" \t a b \t", Trim::Both), "a b");
    }

    #[test]
    fn joining_lines() {
        assert_eq!(join_lines("a\nb\nc"), "a b c");
        assert_eq!(join_lines("a \nb"), "a b");
        assert_eq!(join_lines("a\r\nb"), "a b");
    }

    #[test]
    fn reindenting() {
        assert_eq!(reindent_line("abc", 4, false, 4), "    abc");
        assert_eq!(reindent_line("    abc", 4, false, -4), "abc");
        assert_eq!(reindent_line("  abc", 4, false, -4), "abc");
        assert_eq!(reindent_line("abc", 4, true, 4), "\tabc");
        assert_eq!(reindent_line("\tabc", 4, false, 4), "        abc");
        assert_eq!(reindent_line("\t  abc", 4, true, 1), "\t   abc");
    }

    #[test]
    fn tabs_and_spaces() {
        assert_eq!(tabs_to_spaces("\tab\tc", 4), "    ab  c");
        assert_eq!(spaces_to_tabs("        x   y", 4, false), "\t\tx\ty");
        assert_eq!(spaces_to_tabs("        x   y", 4, true), "\t\tx   y");
        // A single space reaching a tab stop stays a space.
        assert_eq!(spaces_to_tabs("abc d", 4, false), "abc d");
    }
}
