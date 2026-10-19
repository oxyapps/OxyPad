use std::ops::Range;

/// Severity of a log line, used to color it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// How far into a line a level marker is searched for. Levels sit near the
/// start of a line (after a timestamp), and a bound keeps the scan cheap on
/// very long lines.
const SCAN_LIMIT: usize = 160;

/// Classifies one line by the first level keyword it contains as a whole
/// word: `ERROR`, `WARN`, `INFO`, … in upper case, as nearly every logging
/// framework writes them, plus the common short and syslog forms.
pub fn classify_line(line: &str) -> Option<LogLevel> {
    let head = &line.as_bytes()[..line.len().min(SCAN_LIMIT)];
    let mut start = 0;
    while start < head.len() {
        if !head[start].is_ascii_uppercase() {
            start += 1;
            continue;
        }
        let end = head[start..]
            .iter()
            .position(|b| !b.is_ascii_uppercase())
            .map_or(head.len(), |len| start + len);
        let word_start_ok = start == 0 || !head[start - 1].is_ascii_alphanumeric();
        let word_end_ok = end == head.len() || !head[end].is_ascii_alphanumeric();
        if word_start_ok
            && word_end_ok
            && let Some(level) = level_for_word(&head[start..end])
        {
            return Some(level);
        }
        start = end;
    }
    None
}

fn level_for_word(word: &[u8]) -> Option<LogLevel> {
    Some(match word {
        b"ERROR" | b"ERR" | b"FATAL" | b"CRITICAL" | b"CRIT" | b"PANIC" | b"SEVERE" | b"EMERG"
        | b"ALERT" | b"E" | b"F" => LogLevel::Error,
        b"WARN" | b"WARNING" | b"W" => LogLevel::Warn,
        b"INFO" | b"NOTICE" | b"I" => LogLevel::Info,
        b"DEBUG" | b"DBG" | b"D" => LogLevel::Debug,
        b"TRACE" | b"VERBOSE" | b"T" | b"V" => LogLevel::Trace,
        _ => return None,
    })
}

/// Byte ranges (offset by `base`) and levels of the lines in `text` that
/// carry one. Lines without a level are skipped.
pub fn level_spans(text: &str, base: usize) -> Vec<(Range<usize>, LogLevel)> {
    let mut spans = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        if let Some(level) = classify_line(content) {
            spans.push((base + offset..base + offset + content.len(), level));
        }
        offset += line.len();
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_formats() {
        assert_eq!(classify_line("2026-10-19 12:00:01 ERROR db: timeout"), Some(LogLevel::Error));
        assert_eq!(classify_line("[WARN] disk at 91%"), Some(LogLevel::Warn));
        assert_eq!(classify_line("12:00:01.123 INFO  started"), Some(LogLevel::Info));
        assert_eq!(classify_line("level=DEBUG msg=hi"), Some(LogLevel::Debug));
        assert_eq!(classify_line("10-19 12:00:01.123  1234  5678 E ActivityManager: crash"), Some(LogLevel::Error));
    }

    #[test]
    fn ignores_words_that_only_contain_a_level() {
        assert_eq!(classify_line("INFORMATION about ERRORS"), None);
        assert_eq!(classify_line("no level here"), None);
        assert_eq!(classify_line("GET /index.html 200"), None);
    }

    #[test]
    fn spans_cover_line_content_without_newline() {
        let text = "a INFO x\nplain\nb ERROR y\n";
        assert_eq!(
            level_spans(text, 100),
            vec![(100..108, LogLevel::Info), (115..124, LogLevel::Error)]
        );
    }
}
