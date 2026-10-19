use gpui_kit::{
    App, Entity, Menu, MenuItem, OsAction,
    component::{GlobalState, input, menu::AppMenuBar},
};

use crate::actions::*;

/// Installs the menus: the system menu bar on macOS and, through the
/// returned [`AppMenuBar`], an in-window menu bar on Windows and Linux.
///
/// Must run after the key bindings, since menus copy their shortcuts from
/// the keymap when they are built.
pub fn install(cx: &mut App) -> Entity<AppMenuBar> {
    reload(cx);
    AppMenuBar::new(cx)
}

/// Rebuilds the menus, e.g. in another language. The in-window menu bar
/// picks them up when its [`AppMenuBar::reload`] runs.
pub fn reload(cx: &mut App) {
    cx.set_menus(build());
    let owned = build().into_iter().map(Menu::owned).collect();
    GlobalState::global_mut(cx).set_app_menus(owned);
}

fn build() -> Vec<Menu> {
    vec![
        Menu::new("OxyPad").items([
            MenuItem::action(t!("menu.about"), About),
            MenuItem::separator(),
            MenuItem::action(t!("menu.settings"), OpenSettings),
            MenuItem::separator(),
            MenuItem::action(t!("menu.quit"), Quit),
        ]),
        Menu::new(t!("menu.file")).items([
            MenuItem::action(t!("menu.new"), NewFile),
            MenuItem::action(t!("menu.open"), OpenFile),
            MenuItem::action(t!("menu.follow_log"), OpenLog),
            MenuItem::separator(),
            MenuItem::action(t!("menu.save"), Save),
            MenuItem::action(t!("menu.save_as"), SaveAs),
            MenuItem::separator(),
            MenuItem::action(t!("menu.close_tab"), CloseTab),
        ]),
        Menu::new(t!("menu.edit")).items([
            MenuItem::os_action(t!("menu.undo"), input::Undo, OsAction::Undo),
            MenuItem::os_action(t!("menu.redo"), input::Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action(t!("menu.cut"), input::Cut, OsAction::Cut),
            MenuItem::os_action(t!("menu.copy"), input::Copy, OsAction::Copy),
            MenuItem::os_action(t!("menu.paste"), input::Paste, OsAction::Paste),
            MenuItem::os_action(t!("menu.select_all"), input::SelectAll, OsAction::SelectAll),
            MenuItem::separator(),
            MenuItem::action(t!("menu.find"), Find),
            MenuItem::action(t!("menu.replace"), FindReplace),
        ]),
        Menu::new(t!("menu.view")).items([
            MenuItem::action(t!("menu.word_wrap"), ToggleSoftWrap),
            MenuItem::action(t!("menu.follow_file"), ToggleTail),
            MenuItem::separator(),
            MenuItem::action(t!("menu.next_difference"), NextChange),
            MenuItem::action(t!("menu.previous_difference"), PreviousChange),
            MenuItem::action(t!("menu.refresh_comparison"), RefreshCompare),
            MenuItem::separator(),
            MenuItem::action(t!("menu.next_tab"), NextTab),
            MenuItem::action(t!("menu.previous_tab"), PreviousTab),
        ]),
    ]
}
