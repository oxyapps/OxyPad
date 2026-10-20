use std::{collections::HashMap, ops::Range};

/// Severity of a log line, used to color it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// Most severe first, the order settings list them in.
    pub const ALL: [LogLevel; 5] = [Self::Error, Self::Warn, Self::Info, Self::Debug, Self::Trace];

    /// The words that mark a line with this level unless configured
    /// otherwise: those of common logging frameworks, syslog and Android.
    pub fn default_keywords(self) -> &'static [&'static str] {
        match self {
            Self::Error => &["ERROR", "ERR", "FATAL", "CRITICAL", "CRIT", "PANIC", "SEVERE", "EMERG", "ALERT", "FAIL", "E", "F"],
            Self::Warn => &["WARN", "WARNING", "W"],
            Self::Info => &["INFO", "NOTICE", "I"],
            Self::Debug => &["DEBUG", "DBG", "DBUG", "D"],
            Self::Trace => &["TRACE", "TRCE", "VERBOSE", "T", "V"],
        }
    }
}

/// How far into a line a level marker is searched for. Levels sit near the
/// start of a line (after a timestamp), and a bound keeps the scan cheap on
/// very long lines.
const SCAN_LIMIT: usize = 160;

/// Text to highlight wherever it appears in a log line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HighlightRule {
    pub text: String,
    pub match_case: bool,
    /// Mark the whole line, not only the text.
    pub whole_line: bool,
}

/// What a span of a log was marked with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// The whole line, by its level.
    Level(LogLevel),
    /// Text matched by the rule at this index (or its whole line).
    Rule(usize),
}

/// Finds the level of each log line, from configurable keywords, and the
/// text matched by highlight rules.
#[derive(Clone, Debug)]
pub struct LogMatcher {
    /// Upper-case keyword to level.
    levels: HashMap<Vec<u8>, LogLevel>,
    rules: Vec<HighlightRule>,
}

impl Default for LogMatcher {
    fn default() -> Self {
        Self::new(Self::default_keywords(), Vec::new())
    }
}

impl LogMatcher {
    /// The default keywords of every level, in [`LogLevel::ALL`] order.
    pub fn default_keywords() -> [Vec<String>; 5] {
        LogLevel::ALL.map(|level| level.default_keywords().iter().map(|word| word.to_string()).collect())
    }

    /// `keywords` lists the words of each level, in [`LogLevel::ALL`]
    /// order. Only letters count in a keyword; a word listed for two levels
    /// counts for the more severe.
    pub fn new(keywords: [Vec<String>; 5], rules: Vec<HighlightRule>) -> Self {
        let mut levels = HashMap::new();
        for (level, words) in LogLevel::ALL.into_iter().zip(keywords).rev() {
            for word in words {
                let word: Vec<u8> = word.bytes().filter(u8::is_ascii_alphabetic).map(|b| b.to_ascii_uppercase()).collect();
                if !word.is_empty() {
                    levels.insert(word, level);
                }
            }
        }
        let rules = rules.into_iter().filter(|rule| !rule.text.is_empty()).collect();
        Self { levels, rules }
    }

    /// Classifies one line by the first level keyword it contains as a
    /// whole word: in upper case, as most logging frameworks write them, or
    /// capitalized (`[Error]`, as Windows PowerShell does). Lower case only
    /// counts before a colon (`error: …` from compilers, `fail: …` from
    /// .NET), since in running text "error" is usually just a word.
    pub fn classify_line(&self, line: &str) -> Option<LogLevel> {
        let head = &line.as_bytes()[..line.len().min(SCAN_LIMIT)];
        let mut start = 0;
        while start < head.len() {
            if !head[start].is_ascii_alphabetic() {
                start += 1;
                continue;
            }
            let end = head[start..].iter().position(|b| !b.is_ascii_alphabetic()).map_or(head.len(), |len| start + len);
            let word_start_ok = start == 0 || !head[start - 1].is_ascii_alphanumeric();
            let word_end_ok = end == head.len() || !head[end].is_ascii_alphanumeric();
            if word_start_ok
                && word_end_ok
                && let Some(level) = self.level_for_word(&head[start..end], head.get(end) == Some(&b':'))
            {
                return Some(level);
            }
            start = end;
        }
        None
    }

    /// The level a word names, if its case is one levels are written in.
    fn level_for_word(&self, word: &[u8], before_colon: bool) -> Option<LogLevel> {
        let (first, rest) = word.split_first()?;
        let upper = first.is_ascii_uppercase() && rest.iter().all(u8::is_ascii_uppercase);
        let capitalized = first.is_ascii_uppercase() && rest.iter().all(u8::is_ascii_lowercase);
        let lower = word.iter().all(u8::is_ascii_lowercase);
        // A lone letter is a level only in upper case (Android's `E`, `W`, …).
        let accepted = upper || (!rest.is_empty() && (capitalized || (lower && before_colon)));
        if !accepted {
            return None;
        }
        self.levels.get(&word.to_ascii_uppercase()).copied()
    }

    /// Spans of `text` to mark, offset by `base`: each line with a level,
    /// then the text matched by rules (or its line), ordered by start.
    pub fn spans(&self, text: &str, base: usize) -> Vec<(Range<usize>, Mark)> {
        let mut spans = Vec::new();
        let mut offset = 0;
        let mut folded = String::new();
        for line in text.split_inclusive('\n') {
            let content = line.strip_suffix('\n').unwrap_or(line);
            let content = content.strip_suffix('\r').unwrap_or(content);
            let start = base + offset;
            if let Some(level) = self.classify_line(content) {
                spans.push((start..start + content.len(), Mark::Level(level)));
            }
            if !self.rules.is_empty() {
                folded.clear();
                folded.push_str(content);
                // ASCII only, so byte offsets stay those of `content`.
                folded.make_ascii_lowercase();
                for (ix, rule) in self.rules.iter().enumerate() {
                    let (haystack, needle) = if rule.match_case {
                        (content, std::borrow::Cow::Borrowed(rule.text.as_str()))
                    } else {
                        (folded.as_str(), std::borrow::Cow::Owned(rule.text.to_ascii_lowercase()))
                    };
                    if rule.whole_line {
                        if haystack.contains(needle.as_ref()) {
                            spans.push((start..start + content.len(), Mark::Rule(ix)));
                        }
                    } else {
                        for (at, found) in haystack.match_indices(needle.as_ref()) {
                            spans.push((start + at..start + at + found.len(), Mark::Rule(ix)));
                        }
                    }
                }
            }
            offset += line.len();
        }
        spans.sort_by_key(|(range, _)| range.start);
        spans
    }
}

/// [`LogMatcher::classify_line`] with the default keywords.
pub fn classify_line(line: &str) -> Option<LogLevel> {
    LogMatcher::default().classify_line(line)
}

/// Byte ranges (offset by `base`) and levels of the lines in `text` that
/// carry one, with the default keywords. Lines without a level are skipped.
pub fn level_spans(text: &str, base: usize) -> Vec<(Range<usize>, LogLevel)> {
    LogMatcher::default()
        .spans(text, base)
        .into_iter()
        .filter_map(|(range, mark)| match mark {
            Mark::Level(level) => Some((range, level)),
            Mark::Rule(_) => None,
        })
        .collect()
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
    fn classifies_capitalized_and_colon_forms() {
        // Windows PowerShell.
        assert_eq!(classify_line("2026-10-20 09:00:01 [Error] Access denied"), Some(LogLevel::Error));
        assert_eq!(classify_line("2026-10-20 09:00:02 [Warning] Low disk"), Some(LogLevel::Warn));
        assert_eq!(classify_line("2026-10-20 09:00:03 [Information] Started"), None);
        // .NET console logger, compilers.
        assert_eq!(classify_line("fail: Microsoft.AspNetCore[0]"), Some(LogLevel::Error));
        assert_eq!(classify_line("dbug: App.Startup[1]"), Some(LogLevel::Debug));
        assert_eq!(classify_line("src/main.rs:3: warning: unused variable"), Some(LogLevel::Warn));
        assert_eq!(classify_line("error: could not compile"), Some(LogLevel::Error));
    }

    #[test]
    fn ignores_levels_in_running_text() {
        assert_eq!(classify_line("retrying after an error in the network"), None);
        assert_eq!(classify_line("no warnings, all fine"), None);
        assert_eq!(classify_line("eRRor mixed case"), None);
        assert_eq!(classify_line("a e: lower single letter"), None);
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

    #[test]
    fn keywords_can_be_configured() {
        let mut keywords = LogMatcher::default_keywords();
        keywords[0].push("Exception".into());
        keywords[2].clear();
        let matcher = LogMatcher::new(keywords, Vec::new());
        assert_eq!(matcher.classify_line("12:00 EXCEPTION in thread main"), Some(LogLevel::Error));
        assert_eq!(matcher.classify_line("12:00 INFO started"), None);
    }

    #[test]
    fn rules_mark_text_or_lines() {
        let rules = vec![
            HighlightRule { text: "timeout".into(), match_case: false, whole_line: false },
            HighlightRule { text: "user=42".into(), match_case: true, whole_line: true },
        ];
        let matcher = LogMatcher::new(LogMatcher::default_keywords(), rules);
        let text = "a ERROR Timeout x timeout\nb user=42\nc USER=42\n";
        assert_eq!(
            matcher.spans(text, 0),
            vec![
                (0..25, Mark::Level(LogLevel::Error)),
                (8..15, Mark::Rule(0)),
                (18..25, Mark::Rule(0)),
                (26..35, Mark::Rule(1)),
            ]
        );
    }
}
