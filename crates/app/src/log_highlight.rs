//! Tail mode's highlighting, as the preferences set it: which words mark
//! each log level and in which color, and the user's own highlight rules.
//! `editor` is the screen that edits it, opened from Settings ▸ Logs.

pub mod editor;

use std::ops::Range;

use gpui_kit::{
    component::{ActiveTheme, input::TextDecoration},
    *,
};
use oxypad_session::{LogColor, LogHighlighting};
use oxypad_tail::{HighlightRule, LogLevel, LogMatcher, Mark};

/// The matcher for `highlighting`: its keywords, or the built-in ones for
/// levels that keep them, and its rules.
pub fn matcher(highlighting: &LogHighlighting) -> LogMatcher {
    let mut keywords = LogMatcher::default_keywords();
    for (words, style) in keywords.iter_mut().zip(highlighting.levels()) {
        if let Some(own) = &style.keywords {
            words.clone_from(own);
        }
    }
    let rules = highlighting
        .rules
        .iter()
        .map(|rule| HighlightRule { text: rule.text.clone(), match_case: rule.match_case, whole_line: rule.whole_line })
        .collect();
    LogMatcher::new(keywords, rules)
}

/// The theme's color for `color`.
pub fn color(color: LogColor, cx: &App) -> Option<Hsla> {
    let theme = cx.theme();
    Some(match color {
        LogColor::None => return None,
        LogColor::Red => theme.red,
        LogColor::Yellow => theme.yellow,
        LogColor::Green => theme.green,
        LogColor::Cyan => theme.cyan,
        LogColor::Blue => theme.blue,
        LogColor::Magenta => theme.magenta,
        LogColor::Gray => theme.muted_foreground,
    })
}

pub fn color_name(color: LogColor) -> SharedString {
    match color {
        LogColor::None => t!("log_highlighting.no_color"),
        LogColor::Red => t!("tab_menu.red"),
        LogColor::Yellow => t!("tab_menu.yellow"),
        LogColor::Green => t!("tab_menu.green"),
        LogColor::Cyan => t!("tab_menu.cyan"),
        LogColor::Blue => t!("tab_menu.blue"),
        LogColor::Magenta => t!("tab_menu.magenta"),
        LogColor::Gray => t!("log_highlighting.gray"),
    }
    .into()
}

fn level_index(level: LogLevel) -> usize {
    LogLevel::ALL.iter().position(|&each| each == level).unwrap_or(0)
}

/// The style of one mark, if it shows.
fn style(mark: Mark, highlighting: &LogHighlighting, cx: &App) -> Option<HighlightStyle> {
    match mark {
        Mark::Level(level) => {
            let color = color(highlighting.levels()[level_index(level)].color, cx)?;
            Some(HighlightStyle { color: Some(color), ..Default::default() })
        }
        Mark::Rule(ix) => {
            let rule = highlighting.rules.get(ix)?;
            let color = color(rule.color, cx)?;
            Some(if rule.background {
                HighlightStyle { background_color: Some(color.opacity(0.3)), ..Default::default() }
            } else {
                HighlightStyle { color: Some(color), ..Default::default() }
            })
        }
    }
}

/// The highlights of `text` (offset by `base`): the rules' and the
/// levels', apart, since rules are drawn over levels.
pub struct Decorations {
    pub rules: Vec<TextDecoration>,
    pub levels: Vec<TextDecoration>,
}

pub fn decorations(matcher: &LogMatcher, highlighting: &LogHighlighting, text: &str, base: usize, cx: &App) -> Decorations {
    let mut decorations = Decorations { rules: Vec::new(), levels: Vec::new() };
    for (range, mark) in matcher.spans(text, base) {
        let Some(style) = style(mark, highlighting, cx) else { continue };
        let list = if matches!(mark, Mark::Rule(_)) { &mut decorations.rules } else { &mut decorations.levels };
        list.push(TextDecoration::new(range, style));
    }
    decorations
}

/// The highlights of one short line, flattened for a `StyledText`: where a
/// rule and the level overlap, the rule's style wins.
pub fn line_styles(
    matcher: &LogMatcher,
    highlighting: &LogHighlighting,
    line: &str,
    cx: &App,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut styles: Vec<HighlightStyle> = vec![HighlightStyle::default(); line.len()];
    let mut spans = matcher.spans(line, 0);
    // Levels first, so rules are applied over them.
    spans.sort_by_key(|(_, mark)| matches!(mark, Mark::Rule(_)));
    for (range, mark) in spans {
        let Some(style) = style(mark, highlighting, cx) else { continue };
        for each in &mut styles[range] {
            *each = HighlightStyle {
                color: style.color.or(each.color),
                background_color: style.background_color.or(each.background_color),
                ..*each
            };
        }
    }
    let mut runs: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    for (at, style) in styles.into_iter().enumerate() {
        match runs.last_mut() {
            Some((range, last)) if *last == style && range.end == at => range.end = at + 1,
            _ => runs.push((at..at + 1, style)),
        }
    }
    // Runs end on character boundaries, since every byte of a character
    // gets the same style.
    runs.retain(|(_, style)| *style != HighlightStyle::default());
    runs
}
