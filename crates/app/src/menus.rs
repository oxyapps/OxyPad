use gpui_kit::{
    App, Menu, MenuItem, OsAction, SharedString,
    component::{GlobalState, input},
};

use crate::{actions::*, edit_actions::*, syntax};

/// Installs the menus: the system menu bar on macOS, and the menus each
/// window's in-window `AppMenuBar` shows on Windows and Linux.
///
/// Must run after the key bindings, since menus copy their shortcuts from
/// the keymap when they are built.
pub fn install(cx: &mut App) {
    reload(cx);
}

/// Rebuilds the menus, e.g. in another language. The in-window menu bars
/// pick them up when their `AppMenuBar::reload` runs.
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
            MenuItem::action(t!("menu.new_window"), NewWindow),
            MenuItem::action(t!("menu.open"), OpenFile),
            MenuItem::action(t!("menu.follow_log"), OpenLog),
            MenuItem::separator(),
            MenuItem::action(t!("menu.save"), Save),
            MenuItem::action(t!("menu.save_as"), SaveAs),
            MenuItem::separator(),
            MenuItem::action(t!("menu.print"), Print),
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
            submenu(t!("edit.indent"), [
                MenuItem::action(t!("edit.indent_more"), IndentMore),
                MenuItem::action(t!("edit.indent_less"), IndentLess),
            ]),
            submenu(t!("edit.convert_case"), [
                MenuItem::action(t!("edit.upper"), CaseUpper),
                MenuItem::action(t!("edit.lower"), CaseLower),
                MenuItem::action(t!("edit.proper"), CaseProper),
                MenuItem::action(t!("edit.proper_blend"), CaseProperBlend),
                MenuItem::action(t!("edit.sentence"), CaseSentence),
                MenuItem::action(t!("edit.sentence_blend"), CaseSentenceBlend),
                MenuItem::action(t!("edit.invert"), CaseInvert),
                MenuItem::action(t!("edit.random"), CaseRandom),
            ]),
            submenu(t!("edit.lines"), [
                MenuItem::action(t!("edit.duplicate"), DuplicateLines),
                MenuItem::action(t!("edit.remove_duplicates"), RemoveDuplicateLines),
                MenuItem::action(t!("edit.remove_consecutive"), RemoveConsecutiveDuplicateLines),
                MenuItem::action(t!("edit.join"), JoinLines),
                MenuItem::action(t!("edit.move_up"), MoveLinesUp),
                MenuItem::action(t!("edit.move_down"), MoveLinesDown),
                MenuItem::action(t!("edit.remove_empty"), RemoveEmptyLines),
                MenuItem::action(t!("edit.remove_blank"), RemoveBlankLines),
                MenuItem::action(t!("edit.blank_above"), InsertLineAbove),
                MenuItem::action(t!("edit.blank_below"), InsertLineBelow),
                MenuItem::action(t!("edit.reverse"), ReverseLines),
                MenuItem::action(t!("edit.randomize"), RandomizeLines),
                MenuItem::separator(),
                MenuItem::action(t!("edit.sort_lex_asc"), SortLexAscending),
                MenuItem::action(t!("edit.sort_nocase_asc"), SortNoCaseAscending),
                MenuItem::action(t!("edit.sort_locale_asc"), SortLocaleAscending),
                MenuItem::action(t!("edit.sort_int_asc"), SortIntegerAscending),
                MenuItem::action(t!("edit.sort_comma_asc"), SortCommaAscending),
                MenuItem::action(t!("edit.sort_dot_asc"), SortDotAscending),
                MenuItem::action(t!("edit.sort_len_asc"), SortLengthAscending),
                MenuItem::separator(),
                MenuItem::action(t!("edit.sort_lex_desc"), SortLexDescending),
                MenuItem::action(t!("edit.sort_nocase_desc"), SortNoCaseDescending),
                MenuItem::action(t!("edit.sort_locale_desc"), SortLocaleDescending),
                MenuItem::action(t!("edit.sort_int_desc"), SortIntegerDescending),
                MenuItem::action(t!("edit.sort_comma_desc"), SortCommaDescending),
                MenuItem::action(t!("edit.sort_dot_desc"), SortDotDescending),
                MenuItem::action(t!("edit.sort_len_desc"), SortLengthDescending),
            ]),
            submenu(t!("edit.comments"), [
                MenuItem::action(t!("edit.toggle_comment"), ToggleComment),
                MenuItem::action(t!("edit.line_comment"), LineComment),
                MenuItem::action(t!("edit.line_uncomment"), LineUncomment),
                MenuItem::action(t!("edit.block_comment"), BlockComment),
                MenuItem::action(t!("edit.block_uncomment"), BlockUncomment),
            ]),
            submenu(t!("edit.blank_ops"), [
                MenuItem::action(t!("edit.trim_trailing"), TrimTrailing),
                MenuItem::action(t!("edit.trim_leading"), TrimLeading),
                MenuItem::action(t!("edit.trim_both"), TrimBoth),
                MenuItem::action(t!("edit.eol_to_space"), EolToSpace),
                MenuItem::action(t!("edit.trim_and_join"), TrimAndJoin),
                MenuItem::separator(),
                MenuItem::action(t!("edit.tabs_to_spaces"), TabsToSpaces),
                MenuItem::action(t!("edit.spaces_to_tabs"), SpacesToTabs),
                MenuItem::action(t!("edit.leading_spaces_to_tabs"), LeadingSpacesToTabs),
            ]),
            submenu(t!("edit.json"), [
                MenuItem::action(t!("edit.format_json"), FormatJson),
                MenuItem::action(t!("edit.compress_json"), CompressJson),
            ]),
            MenuItem::separator(),
            MenuItem::action(t!("menu.find"), Find),
            MenuItem::action(t!("menu.replace"), FindReplace),
        ]),
        Menu::new(t!("menu.view")).items([
            MenuItem::action(t!("menu.word_wrap"), ToggleSoftWrap),
            MenuItem::action(t!("menu.follow_file"), ToggleTail),
            MenuItem::action(t!("menu.markdown_preview"), ToggleMarkdownPreview),
            MenuItem::separator(),
            MenuItem::action(t!("menu.zoom_in"), ZoomIn),
            MenuItem::action(t!("menu.zoom_out"), ZoomOut),
            MenuItem::action(t!("menu.actual_size"), ZoomReset),
            MenuItem::separator(),
            MenuItem::action(t!("menu.next_difference"), NextChange),
            MenuItem::action(t!("menu.previous_difference"), PreviousChange),
            MenuItem::action(t!("menu.refresh_comparison"), RefreshCompare),
            MenuItem::separator(),
            MenuItem::action(t!("menu.next_tab"), NextTab),
            MenuItem::action(t!("menu.previous_tab"), PreviousTab),
        ]),
        Menu::new(t!("menu.language")).items(
            [MenuItem::action(t!("language.plaintext"), SetLanguage { language: "plaintext".into() }), MenuItem::separator()]
                .into_iter()
                .chain(syntax::language_menu().into_iter().map(|entry| match entry {
                    syntax::LanguageEntry::One(name, label) => language_item(name, label),
                    syntax::LanguageEntry::Letter(letter, languages) => MenuItem::submenu(
                        Menu::new(letter.to_string())
                            .items(languages.into_iter().map(|(name, label)| language_item(name, label))),
                    ),
                })),
        ),
        Menu::new(t!("menu.tools")).items([MenuItem::action(t!("menu.password_generator"), GeneratePassword)]),
    ]
}

fn language_item(name: &str, label: &str) -> MenuItem {
    MenuItem::action(label.to_owned(), SetLanguage { language: name.into() })
}

/// A submenu of the Edit menu.
fn submenu<const N: usize>(name: impl Into<SharedString>, items: [MenuItem; N]) -> MenuItem {
    MenuItem::submenu(Menu::new(name).items(items))
}
