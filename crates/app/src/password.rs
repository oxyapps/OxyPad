//! Tools › Password Generator…: a dialog that makes a password, copies it or
//! inserts it at the cursor of the active tab. The length is shared out
//! among the kinds of character by the rules of MacPad++'s generator
//! (`oxypad_core::PasswordGenerator`), and the settings are kept in the
//! preferences between launches.

use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Sizable, WindowExt,
        button::{Button, ButtonVariants},
        checkbox::Checkbox,
        h_flex,
        input::{InputEvent, InputState, NumberInput},
        v_flex,
    },
    prelude::FluentBuilder,
    *,
};
use oxypad_core::{PasswordGenerator, PasswordProblem};

use crate::{editor_tab::EditorTab, preferences};

/// The names of the kinds, in `PasswordGenerator::counts` order.
fn kind_names() -> [String; 4] {
    [t!("password.uppercase"), t!("password.lowercase"), t!("password.digits"), t!("password.symbols")]
        .map(|name| name.to_string())
}

/// Opens the dialog over `window`. `target` is the tab Insert writes to, if
/// the active tab can take the password. Return inserts, as the primary
/// button does.
pub fn open(target: Option<Entity<EditorTab>>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| PasswordView::new(target, window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        let view = view.clone();
        dialog
            .title(t!("password.title").to_string())
            .w(px(420.))
            .on_ok({
                let view = view.clone();
                move |_, window, cx| view.update(cx, |view, cx| view.insert(window, cx))
            })
            .child(view)
    });
}

struct PasswordView {
    generator: PasswordGenerator,
    /// The tab Insert writes to; none when the active tab can't take text.
    target: Option<Entity<EditorTab>>,
    password: String,
    /// The password was copied since it was made.
    copied: bool,
    length: Entity<InputState>,
    /// The count of each kind, in `PasswordGenerator::counts` order.
    counts: [Entity<InputState>; 4],
    _subscriptions: Vec<Subscription>,
}

impl PasswordView {
    fn new(target: Option<Entity<EditorTab>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let generator = preferences::get(cx).password.clone();
        let lengths = PasswordGenerator::LENGTHS;
        let number = |value: usize, min: usize, max: usize, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value.to_string())
                    .step(1.)
                    .min(min as f64)
                    .max(max as f64)
            })
        };
        let length = number(generator.length, *lengths.start(), *lengths.end(), window, cx);
        let counts = generator.counts().map(|count| number(count, 0, generator.length, window, cx));

        // Text that is not a number in range yet is left alone while typing;
        // the field clamps it when it loses focus, which reports it again.
        let mut subscriptions = vec![cx.subscribe_in(&length, window, |this, input, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::Change)
                && let Some(value) = parse(input, cx).filter(|value| PasswordGenerator::LENGTHS.contains(value))
                && value != this.generator.length
            {
                this.generator.set_length(value);
                this.changed(None, window, cx);
            }
        })];
        for (ix, input) in counts.iter().enumerate() {
            subscriptions.push(cx.subscribe_in(input, window, move |this, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change)
                    && let Some(value) = parse(input, cx).filter(|&value| value <= this.generator.length)
                    && value != this.generator.counts()[ix]
                {
                    this.generator.set_count(ix, value);
                    // The field being typed in keeps its text unless the value was capped.
                    let typed = (this.generator.counts()[ix] == value).then_some(ix);
                    this.changed(typed, window, cx);
                }
            }));
        }

        let mut this = Self {
            generator,
            target,
            password: String::new(),
            copied: false,
            length,
            counts,
            _subscriptions: subscriptions,
        };
        this.regenerate(cx);
        this
    }

    /// Shows the generator's numbers in the fields (except `typed`, the one
    /// being edited), saves the settings and makes a new password.
    fn changed(&mut self, typed: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let length = self.generator.length;
        for (ix, (input, count)) in self.counts.iter().zip(self.generator.counts()).enumerate() {
            input.update(cx, |input, cx| {
                input.set_max(Some(length as f64), window, cx);
                if Some(ix) != typed && input.value().parse::<usize>().ok() != Some(count) {
                    input.set_value(count.to_string(), window, cx);
                }
            });
        }
        let generator = self.generator.clone();
        preferences::update(cx, |p| p.password = generator);
        self.regenerate(cx);
    }

    fn regenerate(&mut self, cx: &mut Context<Self>) {
        self.password = self.generator.generate().unwrap_or_default();
        self.copied = false;
        cx.notify();
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.password.clone()));
        self.copied = true;
        cx.notify();
    }

    /// Puts the password at the cursor of the target tab. Returns whether it
    /// did, which is when the dialog closes.
    fn insert(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(tab) = self.target.clone().filter(|_| self.generator.problem().is_none()) else {
            return false;
        };
        let password = self.password.clone();
        tab.update(cx, |tab, cx| tab.insert(&password, window, cx));
        window.close_dialog(cx);
        true
    }

    fn render_actions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let usable = self.generator.problem().is_none();
        h_flex()
            .gap_2()
            .child(
                Button::new("password-new")
                    .label(t!("password.new"))
                    .disabled(!usable)
                    .on_click(cx.listener(|this, _, _, cx| this.regenerate(cx))),
            )
            .child(div().flex_1())
            .child(
                Button::new("password-copy")
                    .label(if self.copied { t!("password.copied") } else { t!("password.copy") })
                    .disabled(!usable)
                    .on_click(cx.listener(|this, _, _, cx| this.copy(cx))),
            )
            .child(
                Button::new("password-insert")
                    .primary()
                    .label(t!("password.insert"))
                    .disabled(self.target.is_none() || !usable)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.insert(window, cx);
                    })),
            )
    }

    fn set_exclude_ambiguous(&mut self, exclude: bool, cx: &mut Context<Self>) {
        self.generator.exclude_ambiguous = exclude;
        let generator = self.generator.clone();
        preferences::update(cx, |p| p.password = generator);
        self.regenerate(cx);
    }

    fn render_result(&self, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let message = match self.generator.problem() {
            Some(PasswordProblem::NoKinds) => Some(t!("password.no_kinds").to_string()),
            Some(PasswordProblem::CountsExceedLength) => {
                Some(t!("password.counts_exceed_length", length = self.generator.length).to_string())
            }
            None => None,
        };
        let frame = div().p_2().rounded(theme.radius).border_1().border_color(theme.border).bg(theme.background);
        match message {
            Some(message) => frame.text_sm().text_color(theme.muted_foreground).child(message),
            None => frame.font_family(theme.mono_font_family.clone()).child(self.password.clone()),
        }
    }
}

/// The field's text as a whole number, if it is one.
fn parse(input: &Entity<InputState>, cx: &App) -> Option<usize> {
    input.read(cx).value().trim().parse().ok()
}

/// A label with a number field at the end of the row.
fn number_row(label: String, input: &Entity<InputState>) -> impl IntoElement {
    h_flex()
        .gap_2()
        .child(div().flex_1().text_sm().child(label.clone()))
        .child(NumberInput::new(input).small().w_32().accessibility_label(label))
}

impl Render for PasswordView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let note = div().text_xs().text_color(cx.theme().muted_foreground).child(t!("password.counts_note").to_string());
        let kinds = v_flex()
            .gap_2()
            .children(kind_names().into_iter().zip(&self.counts).map(|(name, input)| number_row(name, input)));
        v_flex()
            .gap_3()
            .child(number_row(t!("password.length").to_string(), &self.length))
            .child(kinds)
            .child(note)
            .child(
                Checkbox::new("password-exclude-ambiguous")
                    .label(t!("password.exclude_ambiguous").to_string())
                    .checked(self.generator.exclude_ambiguous)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| this.set_exclude_ambiguous(*checked, cx))),
            )
            .child(self.render_result(cx))
            .child(self.render_actions(cx))
            .when(self.target.is_none(), |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("password.insert_unavailable").to_string()),
                )
            })
    }
}
