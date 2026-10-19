//! The toolbar above the tabs. Every button is always present; each one is
//! enabled only when it applies to the active tab.

use gpui_kit::{
    assets::IconName as Lucide,
    component::{
        ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable,
        button::{Button, ButtonVariants},
        h_flex,
        menu::{DropdownMenu, PopupMenuItem},
    },
    *,
};

use super::{TabView, Workspace};
use crate::actions::*;

/// What the toolbar can do for the active tab.
struct ToolbarState {
    is_editor: bool,
    is_log: bool,
    is_compare: bool,
    can_save: bool,
    can_tail: bool,
    soft_wrap: bool,
    compare_has_changes: bool,
    /// Other tabs the active one can be compared with: id and title.
    compare_targets: Vec<(u64, SharedString)>,
}

impl Workspace {
    fn toolbar_state(&self, cx: &App) -> ToolbarState {
        let active = self.tabs.get(self.active);
        let mut state = ToolbarState {
            is_editor: false,
            is_log: false,
            is_compare: false,
            can_save: false,
            can_tail: false,
            soft_wrap: false,
            compare_has_changes: false,
            compare_targets: Vec::new(),
        };
        match active {
            Some(TabView::Editor(tab)) => {
                let tab = tab.read(cx);
                state.is_editor = true;
                state.can_save = tab.is_dirty() || tab.path().is_none();
                // Following replaces the editor, so unsaved edits must be saved first.
                state.can_tail = tab.path().is_some() && !tab.is_dirty();
                state.soft_wrap = tab.soft_wrap();
            }
            Some(TabView::Log(_)) => {
                state.is_log = true;
                state.can_tail = true;
            }
            Some(TabView::Compare(tab)) => {
                state.is_compare = true;
                state.compare_has_changes = tab.read(cx).has_changes();
            }
            None => {}
        }
        if state.is_editor || state.is_log {
            state.compare_targets = self
                .tabs
                .iter()
                .enumerate()
                .filter(|(ix, tab)| *ix != self.active && !matches!(tab, TabView::Compare(_)))
                .map(|(_, tab)| (tab.id(cx), tab.title(cx)))
                .collect();
        }
        state
    }

    pub(super) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.toolbar_state(cx);
        let (border, background) = (cx.theme().border, cx.theme().title_bar);
        let separator = || div().w(px(1.)).h_4().mx_1().bg(border);

        let compare_targets = state.compare_targets.clone();
        let workspace = cx.entity().downgrade();
        let compare = tool("compare", Lucide::Columns2, t!("toolbar.compare_tooltip"))
            .label(t!("toolbar.compare"))
            .disabled(compare_targets.is_empty())
            .dropdown_menu(move |mut menu, _, _| {
                menu = menu.label(t!("toolbar.compare_with"));
                for (id, title) in &compare_targets {
                    let id = *id;
                    let workspace = workspace.clone();
                    menu = menu.item(PopupMenuItem::new(title.clone()).icon(IconName::File).on_click(
                        move |_, window, cx| {
                            let _ = workspace.update(cx, |this, cx| this.open_compare(id, window, cx));
                        },
                    ));
                }
                menu
            });

        h_flex()
            .id("toolbar")
            .gap_0p5()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .bg(background)
            .child(action_tool("new", Lucide::FilePlus, &t!("toolbar.new"), shortcut("N"), NewFile, cx))
            .child(action_tool("open", IconName::FolderOpen, &t!("toolbar.open"), shortcut("O"), OpenFile, cx))
            .child(
                action_tool("save", Lucide::Save, &t!("toolbar.save"), shortcut("S"), Save, cx).disabled(!state.can_save),
            )
            .child(
                action_tool("save-as", Lucide::SavePen, &t!("toolbar.save_as"), shortcut("Shift+S"), SaveAs, cx)
                    .disabled(!state.is_editor),
            )
            .child(separator())
            .child(
                tool("undo", IconName::Undo2, format!("{} ({})", t!("toolbar.undo"), shortcut("Z")))
                    .disabled(!state.is_editor)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dispatch_to_editor(Box::new(gpui_kit::component::input::Undo), window, cx)
                    })),
            )
            .child(
                tool("redo", IconName::Redo2, format!("{} ({})", t!("toolbar.redo"), shortcut("Shift+Z")))
                    .disabled(!state.is_editor)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dispatch_to_editor(Box::new(gpui_kit::component::input::Redo), window, cx)
                    })),
            )
            .child(separator())
            .child(
                action_tool("find", IconName::Search, &t!("toolbar.find"), shortcut("F"), Find, cx)
                    .disabled(!(state.is_editor || state.is_log)),
            )
            .child(
                action_tool("replace", IconName::Replace, &t!("toolbar.replace"), replace_shortcut(), FindReplace, cx)
                    .disabled(!state.is_editor),
            )
            .child(
                action_tool("wrap", Lucide::TextWrap, &t!("toolbar.word_wrap"), "Alt+Z".into(), ToggleSoftWrap, cx)
                    .disabled(!state.is_editor)
                    .selected(state.soft_wrap),
            )
            .child(separator())
            .child(
                action_tool(
                    "tail",
                    Lucide::ScrollText,
                    &if state.is_log { t!("toolbar.tail_on") } else { t!("toolbar.tail_off") },
                    shortcut("Alt+T"),
                    ToggleTail,
                    cx,
                )
                .label(t!("toolbar.tail"))
                .disabled(!state.can_tail)
                .selected(state.is_log),
            )
            .child(separator())
            .child(compare)
            .child(
                action_tool("prev-change", IconName::ChevronUp, &t!("toolbar.previous_difference"), "Shift+F7".into(), PreviousChange, cx)
                    .disabled(!state.compare_has_changes),
            )
            .child(
                action_tool("next-change", IconName::ChevronDown, &t!("toolbar.next_difference"), "F7".into(), NextChange, cx)
                    .disabled(!state.compare_has_changes),
            )
            .child(
                action_tool("refresh-compare", IconName::RefreshCw, &t!("toolbar.compare_again"), SharedString::default(), RefreshCompare, cx)
                    .disabled(!state.is_compare),
            )
            .child(div().flex_1())
            .child(action_tool("settings", IconName::Settings, &t!("toolbar.settings"), shortcut(","), OpenSettings, cx))
    }
}

/// A compact icon button.
fn tool(id: &'static str, icon: impl Into<Icon>, tooltip: impl Into<SharedString>) -> Button {
    Button::new(id).icon(icon).ghost().small().tooltip(tooltip)
}

/// An icon button that dispatches `action` to the workspace.
fn action_tool<A: Action + Clone>(
    id: &'static str,
    icon: impl Into<Icon>,
    name: &str,
    keys: SharedString,
    action: A,
    cx: &mut Context<Workspace>,
) -> Button {
    let tooltip = if keys.is_empty() { name.to_string() } else { format!("{name} ({keys})") };
    tool(id, icon, tooltip).on_click(cx.listener(move |_, _, window, cx| {
        window.dispatch_action(action.boxed_clone(), cx);
    }))
}

/// Replace follows each platform's convention, like the editor's own binding.
fn replace_shortcut() -> SharedString {
    if cfg!(target_os = "macos") { shortcut("Shift+F") } else { "Ctrl+H".into() }
}

/// The platform's spelling of the primary shortcut modifier plus `keys`.
fn shortcut(keys: &str) -> SharedString {
    if cfg!(target_os = "macos") {
        let keys = keys.replace("Shift+", "⇧").replace("Alt+", "⌥");
        format!("⌘{keys}").into()
    } else {
        format!("Ctrl+{keys}").into()
    }
}
