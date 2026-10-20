//! The tab bar: pinned tabs, tab colors, the tab context menu, renaming,
//! and reordering by drag and drop. Pinned tabs always come first; moving
//! a tab keeps it among the tabs of its kind (pinned or not).

use std::{path::PathBuf, rc::Rc};

use gpui_kit::{
    assets::IconName as Lucide,
    component::{
        ActiveTheme, Icon, IconName, Sizable, WindowExt,
        button::{Button, ButtonVariants},
        h_flex,
        input::{Input, InputEvent, InputState},
        menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
        tab::{Tab, TabBar},
    },
    prelude::FluentBuilder,
    *,
};
use oxypad_session::TabColor;

use super::{TabView, Workspace};
use crate::editor_tab::EditorTab;

/// What the workspace keeps about a tab besides its content.
#[derive(Clone, Copy, Default)]
pub(super) struct TabMeta {
    pub pinned: bool,
    pub color: Option<TabColor>,
}

/// A tab being dragged to another place in the bar.
#[derive(Clone)]
struct DraggedTab {
    id: u64,
    title: SharedString,
}

/// What follows the pointer while a tab is dragged.
struct DraggedTabView {
    title: SharedString,
}

impl Render for DraggedTabView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .shadow_md()
            .text_sm()
            .child(self.title.clone())
    }
}

type Callback = Rc<dyn Fn(&mut Window, &mut App)>;

/// What "Copy to Clipboard" copies from a tab's file.
#[derive(Clone, Copy)]
enum CopyWhat {
    Folder,
    FileName,
    FullPath,
}

fn color_value(color: TabColor, cx: &App) -> Hsla {
    let theme = cx.theme();
    match color {
        TabColor::Red => theme.red,
        TabColor::Yellow => theme.yellow,
        TabColor::Green => theme.green,
        TabColor::Cyan => theme.cyan,
        TabColor::Blue => theme.blue,
        TabColor::Magenta => theme.magenta,
    }
}

fn color_name(color: TabColor) -> SharedString {
    match color {
        TabColor::Red => t!("tab_menu.red"),
        TabColor::Yellow => t!("tab_menu.yellow"),
        TabColor::Green => t!("tab_menu.green"),
        TabColor::Cyan => t!("tab_menu.cyan"),
        TabColor::Blue => t!("tab_menu.blue"),
        TabColor::Magenta => t!("tab_menu.magenta"),
    }
    .into()
}

/// "Show in Finder" and its equivalents.
fn reveal_label() -> SharedString {
    if cfg!(target_os = "macos") {
        t!("tab_menu.show_in_finder").into()
    } else if cfg!(target_os = "windows") {
        t!("tab_menu.show_in_explorer").into()
    } else {
        t!("tab_menu.show_in_folder").into()
    }
}

impl Workspace {
    pub(super) fn meta(&self, id: u64) -> TabMeta {
        self.meta.get(&id).copied().unwrap_or_default()
    }

    /// How many tabs at the start of the bar are pinned.
    fn pinned_count(&self, cx: &App) -> usize {
        self.tabs.iter().take_while(|tab| self.meta(tab.id(cx)).pinned).count()
    }

    fn tab_index(&self, id: u64, cx: &App) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id(cx) == id)
    }

    /// Moves tab `id` to index `to` of the bar without it, kept among the
    /// tabs of its kind: a pinned tab stays before the others, an unpinned
    /// one after them. `usize::MAX` moves it to the end of its kind.
    pub(super) fn move_tab(&mut self, id: u64, to: usize, cx: &mut Context<Self>) {
        let Some(from) = self.tab_index(id, cx) else { return };
        let active = self.tabs.get(self.active).map(|tab| tab.id(cx));
        let tab = self.tabs.remove(from);
        let pins = self.pinned_count(cx);
        let to = if self.meta(id).pinned { to.min(pins) } else { to.clamp(pins, self.tabs.len()) };
        self.tabs.insert(to, tab);
        if let Some(ix) = active.and_then(|active| self.tab_index(active, cx)) {
            self.active = ix;
        }
        self.mark_changed(cx);
    }

    /// Puts pinned tabs first, keeping the order within each kind; for a
    /// restored session.
    pub(super) fn sort_pinned_first(&mut self, cx: &App) {
        let active = self.tabs.get(self.active).map(|tab| tab.id(cx));
        let meta = &self.meta;
        self.tabs.sort_by_key(|tab| !meta.get(&tab.id(cx)).is_some_and(|meta| meta.pinned));
        if let Some(ix) = active.and_then(|active| self.tab_index(active, cx)) {
            self.active = ix;
        }
    }

    /// Drops tab `id` on the tab at `target`, which it then sits before.
    fn drop_tab(&mut self, id: u64, target: usize, cx: &mut Context<Self>) {
        let Some(from) = self.tab_index(id, cx) else { return };
        let to = if from < target { target - 1 } else { target };
        self.move_tab(id, to, cx);
    }

    fn set_pinned(&mut self, id: u64, pinned: bool, cx: &mut Context<Self>) {
        self.meta.entry(id).or_default().pinned = pinned;
        // To the end of the pinned tabs, or to the start of the others.
        self.move_tab(id, if pinned { usize::MAX } else { 0 }, cx);
    }

    fn set_color(&mut self, id: u64, color: Option<TabColor>, cx: &mut Context<Self>) {
        self.meta.entry(id).or_default().color = color;
        self.mark_changed(cx);
    }

    fn editor_by_id(&self, id: u64, cx: &App) -> Option<Entity<EditorTab>> {
        self.tabs.iter().find_map(|tab| match tab {
            TabView::Editor(editor) if editor.read(cx).id() == id => Some(editor.clone()),
            _ => None,
        })
    }

    fn tab_path(&self, id: u64, cx: &App) -> Option<PathBuf> {
        self.tabs.iter().find(|tab| tab.id(cx) == id).and_then(|tab| tab.path(cx))
    }

    fn close_tab_by_id(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.tab_index(id, cx) {
            self.close_tab_at(ix, window, cx);
        }
    }

    fn save_tab_by_id(&mut self, id: u64, save_as: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.editor_by_id(id, cx) {
            self.save_tab(tab, save_as, false, window, cx);
        }
    }

    /// Untitled tabs get a new name; tabs with a file are saved under one.
    fn rename_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.editor_by_id(id, cx) else { return };
        if tab.read(cx).is_untitled() {
            self.open_rename_dialog(tab, window, cx);
        } else {
            self.save_tab(tab, true, false, window, cx);
        }
    }

    fn open_rename_dialog(&mut self, tab: Entity<EditorTab>, window: &mut Window, cx: &mut Context<Self>) {
        let current = tab.read(cx).title();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(current));
        let rename: Callback = Rc::new({
            let input = input.clone();
            move |window, cx| {
                let name = input.read(cx).value().trim().to_string();
                if name.is_empty() {
                    return;
                }
                tab.update(cx, |tab, cx| tab.rename(name.into(), cx));
                window.close_dialog(cx);
            }
        });
        // Enter renames, as the button does.
        self.rename_subscription = Some(cx.subscribe_in(&input, window, {
            let rename = rename.clone();
            move |_, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    rename(window, cx);
                }
            }
        }));

        window.open_dialog(cx, {
            let input = input.clone();
            move |dialog, _, _| {
                dialog.title(t!("tab_menu.rename_title").to_string()).child(Input::new(&input)).footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("rename-cancel")
                                .label(t!("dialog.cancel"))
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("rename-ok").primary().label(t!("tab_menu.rename_button")).on_click({
                            let rename = rename.clone();
                            move |_, window, cx| rename(window, cx)
                        })),
                )
            }
        });
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        });
    }

    fn toggle_read_only(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(tab) = self.editor_by_id(id, cx) {
            tab.update(cx, |tab, cx| {
                let read_only = !tab.is_read_only();
                tab.set_read_only(read_only, cx);
            });
        }
    }

    fn copy_path(&self, id: u64, what: CopyWhat, cx: &mut Context<Self>) {
        let Some(path) = self.tab_path(id, cx) else { return };
        let text = match what {
            CopyWhat::Folder => path.parent().map(|dir| dir.display().to_string()),
            CopyWhat::FileName => path.file_name().map(|name| name.to_string_lossy().into_owned()),
            CopyWhat::FullPath => Some(path.display().to_string()),
        };
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub(super) fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = self.tabs.iter().enumerate().map(|(ix, tab)| {
            let id = tab.id(cx);
            let meta = self.meta(id);
            let title = tab.title(cx);
            let read_only = matches!(tab, TabView::Editor(editor) if editor.read(cx).is_read_only());
            let icon: Icon = if meta.pinned {
                Lucide::Pin.into()
            } else if read_only {
                Lucide::Lock.into()
            } else {
                match tab {
                    TabView::Editor(_) => IconName::File.into(),
                    TabView::Log(_) => IconName::Eye.into(),
                    TabView::Compare(_) => IconName::Copy.into(),
                }
            };
            let color = meta.color.map(|color| color_value(color, cx));
            let label = if tab.is_dirty(cx) { format!("{title} ●") } else { title.to_string() };
            let context_tab = self.context_tab.clone();

            Tab::new()
                .label(label)
                .prefix(div().pl_2().child(icon.when_some(color, |icon, color| icon.text_color(color))))
                // The tab's color also underlines its name.
                .when_some(color, |tab, color| {
                    tab.child(div().absolute().left_0().right_0().bottom_0().h(px(2.)).rounded_full().bg(color))
                })
                // Pinned tabs close from the menu only, so they are not closed by accident.
                .when(!meta.pinned, |tab| {
                    tab.suffix(
                        Button::new(("close-tab", id))
                            .icon(IconName::Close)
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_tab_by_id(id, window, cx);
                            })),
                    )
                })
                .on_click(cx.listener(move |this, _, window, cx| this.activate(ix, window, cx)))
                // Runs before the bar's context menu opens, so it knows which tab to show it for.
                .on_mouse_down(MouseButton::Right, move |_, _, _| context_tab.set(Some(id)))
                .on_drag(DraggedTab { id, title }, |drag, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| DraggedTabView { title: drag.title.clone() })
                })
                .drag_over::<DraggedTab>(|style, _, _, cx| style.border_l_2().border_color(cx.theme().drag_border))
                .on_drop(cx.listener(move |this, drag: &DraggedTab, _, cx| this.drop_tab(drag.id, ix, cx)))
        });

        let workspace = cx.entity().downgrade();
        let context_tab = self.context_tab.clone();
        div()
            .id("tab-strip")
            .w_full()
            .child(
                TabBar::new("tabs")
                    .small()
                    .selected_index(self.active)
                    .children(tabs)
                    .last_empty_space(
                        // Dropping a tab past the last one moves it to the end.
                        div()
                            .id("tabs-end")
                            .h_full()
                            .flex_grow(1.)
                            .min_w_16()
                            .drag_over::<DraggedTab>(|style, _, _, cx| style.bg(cx.theme().drop_target))
                            .on_drop(cx.listener(|this, drag: &DraggedTab, _, cx| {
                                this.move_tab(drag.id, usize::MAX, cx)
                            })),
                    )
                    .suffix(
                        Button::new("new-tab")
                            .icon(IconName::Plus)
                            .ghost()
                            .xsmall()
                            .tooltip(t!("tabs.new_tab"))
                            .on_click(cx.listener(|this, _, window, cx| this.new_untitled(window, cx))),
                    ),
            )
            .context_menu(move |menu, window, cx| match context_tab.take() {
                Some(id) => tab_menu(&workspace, id, menu, window, cx),
                // A right click on the bar outside the tabs: no menu.
                None => menu,
            })
    }
}

/// The context menu of tab `id`.
fn tab_menu(
    workspace: &WeakEntity<Workspace>,
    id: u64,
    mut menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(this) = workspace.upgrade() else { return menu };
    let this = this.read(cx);
    let Some(ix) = this.tab_index(id, cx) else { return menu };
    let tab = &this.tabs[ix];
    let meta = this.meta(id);
    let path = tab.path(cx);
    let editor = match tab {
        TabView::Editor(editor) => Some(editor.read(cx)),
        _ => None,
    };
    let unsaved = editor.is_some_and(|editor| editor.is_dirty() || editor.is_untitled());
    let read_only = editor.is_some_and(EditorTab::is_read_only);
    let pins = this.pinned_count(cx);
    let (first, last) = if meta.pinned { (0, pins.saturating_sub(1)) } else { (pins, this.tabs.len() - 1) };

    // A click handler that runs `f` on the workspace for this tab.
    let run = |f: fn(&mut Workspace, u64, &mut Window, &mut Context<Workspace>)| {
        let workspace = workspace.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            let _ = workspace.update(cx, |this, cx| f(this, id, window, cx));
        }
    };

    if unsaved {
        menu = menu
            .item(PopupMenuItem::new(t!("tab_menu.save")).icon(Lucide::Save).on_click(run(|this, id, window, cx| {
                this.save_tab_by_id(id, false, window, cx)
            })))
            .item(PopupMenuItem::new(t!("tab_menu.save_as")).icon(Lucide::SavePen).on_click(run(
                |this, id, window, cx| this.save_tab_by_id(id, true, window, cx),
            )))
            .separator();
    }

    menu = menu
        .item(PopupMenuItem::new(t!("tab_menu.close")).icon(Lucide::X).on_click(run(Workspace::close_tab_by_id)))
        .item(
            PopupMenuItem::new(if meta.pinned { t!("tab_menu.unpin") } else { t!("tab_menu.pin") })
                .icon(if meta.pinned { Lucide::PinOff } else { Lucide::Pin })
                .on_click(run(|this, id, _, cx| {
                    let pinned = this.meta(id).pinned;
                    this.set_pinned(id, !pinned, cx);
                })),
        );
    if editor.is_some() {
        menu = menu
            .item(PopupMenuItem::new(t!("tab_menu.rename")).icon(Lucide::PenLine).on_click(run(Workspace::rename_tab)))
            .item(
                PopupMenuItem::new(t!("tab_menu.read_only"))
                    .icon(Lucide::Lock)
                    .checked(read_only)
                    .on_click(run(|this, id, _, cx| this.toggle_read_only(id, cx))),
            );
    }

    if path.is_some() {
        menu = menu
            .separator()
            .item(PopupMenuItem::new(reveal_label()).icon(Lucide::FolderOpen).on_click(run(|this, id, _, cx| {
                if let Some(path) = this.tab_path(id, cx) {
                    cx.reveal_path(&path);
                }
            })))
            .submenu_with_icon(Some(Lucide::ClipboardCopy.into()), t!("tab_menu.copy"), window, cx, {
                let workspace = workspace.clone();
                move |menu, _, _| {
                    let copy = |what: CopyWhat| {
                        let workspace = workspace.clone();
                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            let _ = workspace.update(cx, |this, cx| this.copy_path(id, what, cx));
                        }
                    };
                    menu.item(PopupMenuItem::new(t!("tab_menu.copy_folder")).on_click(copy(CopyWhat::Folder)))
                        .item(PopupMenuItem::new(t!("tab_menu.copy_file_name")).on_click(copy(CopyWhat::FileName)))
                        .item(PopupMenuItem::new(t!("tab_menu.copy_full_path")).on_click(copy(CopyWhat::FullPath)))
                }
            });
    } else {
        menu = menu.separator();
    }

    menu = menu.submenu_with_icon(Some(Lucide::Palette.into()), t!("tab_menu.color"), window, cx, {
        let workspace = workspace.clone();
        move |mut menu, _, cx| {
            let current = workspace.upgrade().and_then(|this| this.read(cx).meta(id).color);
            let pick = |color: Option<TabColor>| {
                let workspace = workspace.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    let _ = workspace.update(cx, |this, cx| this.set_color(id, color, cx));
                }
            };
            menu = menu.item(PopupMenuItem::new(t!("tab_menu.no_color")).checked(current.is_none()).on_click(pick(None)));
            for color in TabColor::ALL {
                let swatch = Icon::from(Lucide::Circle).text_color(color_value(color, cx));
                menu = menu.item(
                    PopupMenuItem::new(color_name(color))
                        .icon(swatch)
                        .checked(current == Some(color))
                        .on_click(pick(Some(color))),
                );
            }
            menu
        }
    });

    menu.separator()
        .item(
            PopupMenuItem::new(t!("tab_menu.move_to_start"))
                .icon(Lucide::ArrowLeftToLine)
                .disabled(ix == first)
                .on_click(run(|this, id, _, cx| this.move_tab(id, 0, cx))),
        )
        .item(
            PopupMenuItem::new(t!("tab_menu.move_to_end"))
                .icon(Lucide::ArrowRightToLine)
                .disabled(ix == last)
                .on_click(run(|this, id, _, cx| this.move_tab(id, usize::MAX, cx))),
        )
}
