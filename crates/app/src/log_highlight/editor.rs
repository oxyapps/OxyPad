//! The screen that edits tail mode's highlighting, a dialog over Settings:
//! a color and the keywords of each level, the user's own highlight rules,
//! and a sample log showing the result as it is edited. Nothing changes
//! until Save.

use std::rc::Rc;

use gpui_kit::{
    assets::IconName as Lucide,
    component::{
        ActiveTheme, Icon, Sizable, StyledExt as _, WindowExt,
        button::{Button, ButtonVariants},
        checkbox::Checkbox,
        h_flex,
        input::{Input, InputEvent, InputState},
        menu::{DropdownMenu, PopupMenuItem},
        v_flex,
    },
    *,
};
use oxypad_session::{LogColor, LogHighlighting, LogRule};
use oxypad_tail::{LogLevel, LogMatcher};

use super::{color_name, line_styles, matcher};
use crate::preferences;

/// The log the preview shows.
const SAMPLE: &[&str] = &[
    "2026-10-20 09:14:02.118 INFO  Server started on port 8080",
    "2026-10-20 09:14:05.402 DEBUG Cache warmed: 1532 entries",
    "2026-10-20 09:15:11.007 WARN  Slow query took 2380 ms (user=42)",
    "2026-10-20 09:15:12.930 ERROR Request failed: timeout after 30 s",
    "2026-10-20 09:15:13.001 TRACE Retrying request 7f3a",
];

/// Stores a picked color in the draft.
type SetColor = Rc<dyn Fn(&mut LogHighlighting, LogColor)>;

/// Opens the editor over the Settings window.
pub fn open(window: &mut Window, cx: &mut App) {
    let editor = cx.new(|cx| LogHighlightEditor::new(preferences::get(cx).log_highlighting.clone(), window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        let editor = editor.clone();
        dialog
            .title(t!("log_highlighting.title").to_string())
            .w(px(720.))
            .child(editor.clone())
            .footer(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(Button::new("log-highlighting-defaults").label(t!("log_highlighting.restore_defaults")).on_click({
                        let editor = editor.clone();
                        move |_, window, cx| editor.update(cx, |editor, cx| editor.reset(LogHighlighting::default(), window, cx))
                    }))
                    .child(div().flex_1())
                    .child(
                        Button::new("log-highlighting-cancel")
                            .label(t!("dialog.cancel"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(Button::new("log-highlighting-save").primary().label(t!("log_highlighting.save")).on_click(
                        move |_, window, cx| {
                            let draft = editor.read(cx).draft.clone();
                            preferences::update(cx, |p| p.log_highlighting = draft);
                            window.close_dialog(cx);
                        },
                    )),
            )
    });
}

struct LogHighlightEditor {
    draft: LogHighlighting,
    /// The draft's matcher, for the preview.
    matcher: LogMatcher,
    /// The keywords of each level, in `LogLevel::ALL` order.
    keywords: Vec<Entity<InputState>>,
    /// The text of each rule, in order.
    rule_texts: Vec<Entity<InputState>>,
    _subscriptions: Vec<Subscription>,
}

impl LogHighlightEditor {
    fn new(draft: LogHighlighting, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            matcher: matcher(&draft),
            draft: LogHighlighting::default(),
            keywords: Vec::new(),
            rule_texts: Vec::new(),
            _subscriptions: Vec::new(),
        };
        this.reset(draft, window, cx);
        this
    }

    /// Shows `draft`, with new text fields.
    fn reset(&mut self, draft: LogHighlighting, window: &mut Window, cx: &mut Context<Self>) {
        self._subscriptions.clear();
        let defaults = LogMatcher::default_keywords();
        self.keywords = draft
            .levels()
            .into_iter()
            .zip(&defaults)
            .enumerate()
            .map(|(ix, (style, default))| {
                let words = style.keywords.as_ref().unwrap_or(default).join(" ");
                let input = cx.new(|cx| InputState::new(window, cx).default_value(words));
                self._subscriptions.push(cx.subscribe(&input, move |this, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        let words = words_in(&input.read(cx).value());
                        let default = &LogMatcher::default_keywords()[ix];
                        this.draft.levels_mut()[ix].keywords = (words != *default).then_some(words);
                        this.changed(cx);
                    }
                }));
                input
            })
            .collect();
        self.rule_texts.clear();
        self.draft = draft;
        for ix in 0..self.draft.rules.len() {
            let text = self.draft.rules[ix].text.clone();
            self.push_rule_input(text, window, cx);
        }
        self.changed(cx);
    }

    fn push_rule_input(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            InputState::new(window, cx).default_value(text).placeholder(t!("log_highlighting.rule_placeholder"))
        });
        self._subscriptions.push(cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change)
                && let Some(ix) = this.rule_texts.iter().position(|each| *each == input)
            {
                this.draft.rules[ix].text = input.read(cx).value().to_string();
                this.changed(cx);
            }
        }));
        self.rule_texts.push(input);
    }

    fn add_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft.rules.push(LogRule { color: LogColor::Magenta, background: true, ..Default::default() });
        self.push_rule_input(String::new(), window, cx);
        if let Some(input) = self.rule_texts.last() {
            input.update(cx, |input, cx| input.focus(window, cx));
        }
        self.changed(cx);
    }

    fn remove_rule(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.draft.rules.len() {
            self.draft.rules.remove(ix);
            self.rule_texts.remove(ix);
            self.changed(cx);
        }
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.matcher = matcher(&self.draft);
        cx.notify();
    }

    /// A button showing `current` that opens a menu of the colors; `set`
    /// stores the one picked.
    fn color_picker(
        &self,
        id: impl Into<ElementId>,
        current: LogColor,
        set: SetColor,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let this = cx.entity().downgrade();
        Button::new(id)
            .small()
            .outline()
            .w(px(120.))
            .icon(swatch(current, cx))
            .label(color_name(current))
            .dropdown_menu(move |mut menu, _, cx| {
                for color in LogColor::ALL {
                    let (this, set) = (this.clone(), set.clone());
                    menu = menu.item(
                        PopupMenuItem::new(color_name(color))
                            .icon(swatch(color, cx))
                            .checked(color == current)
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |this, cx| {
                                    set(&mut this.draft, color);
                                    this.changed(cx);
                                });
                            }),
                    );
                }
                menu
            })
    }

    fn render_levels(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows = v_flex().gap_2();
        for (ix, level) in LogLevel::ALL.into_iter().enumerate() {
            let current = self.draft.levels()[ix].color;
            let set = Rc::new(move |h: &mut LogHighlighting, color| h.levels_mut()[ix].color = color);
            rows = rows.child(
                h_flex()
                    .gap_2()
                    .child(div().w(px(80.)).text_sm().child(level_name(level)))
                    .child(self.color_picker(("level-color", ix), current, set, cx))
                    .child(div().flex_1().child(Input::new(&self.keywords[ix]).small())),
            );
        }
        rows
    }

    fn render_rules(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows = v_flex().gap_2();
        if self.draft.rules.is_empty() {
            rows = rows.child(
                div().text_sm().text_color(cx.theme().muted_foreground).child(t!("log_highlighting.no_rules").to_string()),
            );
        }
        for (ix, rule) in self.draft.rules.iter().enumerate() {
            let this = cx.entity().downgrade();
            let toggle = move |id: &'static str, label: SharedString, checked: bool, set: fn(&mut LogRule, bool)| {
                let this = this.clone();
                Checkbox::new((id, ix)).label(label).checked(checked).on_click(move |checked, _, cx| {
                    let checked = *checked;
                    let _ = this.update(cx, |this, cx| {
                        if let Some(rule) = this.draft.rules.get_mut(ix) {
                            set(rule, checked);
                            this.changed(cx);
                        }
                    });
                })
            };
            let set = Rc::new(move |h: &mut LogHighlighting, color| {
                if let Some(rule) = h.rules.get_mut(ix) {
                    rule.color = color;
                }
            });
            let color = self.color_picker(("rule-color", ix), rule.color, set, cx);
            rows = rows.child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&self.rule_texts[ix]).small()))
                            .child(color)
                            .child(
                                Button::new(("remove-rule", ix))
                                    .ghost()
                                    .small()
                                    .icon(Lucide::X)
                                    .tooltip(t!("log_highlighting.remove_rule"))
                                    .on_click(cx.listener(move |this, _, _, cx| this.remove_rule(ix, cx))),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_4()
                            .text_sm()
                            .child(toggle("rule-background", t!("log_highlighting.background").into(), rule.background, |r, v| {
                                r.background = v
                            }))
                            .child(toggle("rule-line", t!("log_highlighting.whole_line").into(), rule.whole_line, |r, v| {
                                r.whole_line = v
                            }))
                            .child(toggle("rule-case", t!("log_highlighting.match_case").into(), rule.match_case, |r, v| {
                                r.match_case = v
                            })),
                    ),
            );
        }
        rows.child(
            h_flex().child(
                Button::new("add-rule")
                    .small()
                    .outline()
                    .label(t!("log_highlighting.add_rule"))
                    .on_click(cx.listener(|this, _, window, cx| this.add_rule(window, cx))),
            ),
        )
    }

    fn render_preview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .p_2()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .font_family(theme.mono_font_family.clone())
            .text_size(px(12.))
            .children(SAMPLE.iter().map(|line| {
                let styles = line_styles(&self.matcher, &self.draft, line, cx);
                div().whitespace_nowrap().child(StyledText::new(*line).with_highlights(styles))
            }))
    }
}

/// A dot in `color`, or an outline of one for no color.
fn swatch(color: LogColor, cx: &App) -> Icon {
    let value = super::color(color, cx).unwrap_or(cx.theme().border);
    Icon::from(Lucide::Circle).text_color(value)
}

/// The words of a keywords field: separated by spaces or commas.
fn words_in(text: &str) -> Vec<String> {
    text.split(|ch: char| ch.is_whitespace() || ch == ',').filter(|word| !word.is_empty()).map(str::to_owned).collect()
}

fn level_name(level: LogLevel) -> String {
    match level {
        LogLevel::Error => t!("log_highlighting.error"),
        LogLevel::Warn => t!("log_highlighting.warning"),
        LogLevel::Info => t!("log_highlighting.info"),
        LogLevel::Debug => t!("log_highlighting.debug"),
        LogLevel::Trace => t!("log_highlighting.trace"),
    }
    .to_string()
}

impl Render for LogHighlightEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let heading = |text: String, cx: &App| div().text_sm().font_semibold().text_color(cx.theme().foreground).child(text);
        let note = |text: String, cx: &App| div().text_xs().text_color(cx.theme().muted_foreground).child(text);
        v_flex()
            .gap_3()
            .child(heading(t!("log_highlighting.levels").to_string(), cx))
            .child(note(t!("log_highlighting.levels_note").to_string(), cx))
            .child(self.render_levels(cx))
            .child(heading(t!("log_highlighting.rules").to_string(), cx))
            .child(note(t!("log_highlighting.rules_note").to_string(), cx))
            .child(self.render_rules(cx))
            .child(heading(t!("log_highlighting.preview").to_string(), cx))
            .child(self.render_preview(cx))
    }
}
