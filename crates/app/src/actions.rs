use gpui_kit::{App, KeyBinding, actions};

use crate::edit_actions::*;

actions!(
    oxypad,
    [
        NewFile,
        NewWindow,
        OpenFile,
        OpenLog,
        Save,
        SaveAs,
        Print,
        CloseTab,
        NextTab,
        PreviousTab,
        ToggleTail,
        ToggleSoftWrap,
        Find,
        FindReplace,
        NextChange,
        PreviousChange,
        RefreshCompare,
        GeneratePassword,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        About,
        OpenSettings,
        Quit,
    ]
);

/// Key context set on the workspace, so these bindings apply wherever focus
/// is inside the window.
pub const WORKSPACE_CONTEXT: &str = "Workspace";

/// `secondary` is Cmd on macOS and Ctrl on Windows and Linux.
pub fn bind_keys(cx: &mut App) {
    let context = Some(WORKSPACE_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("secondary-n", NewFile, context),
        KeyBinding::new("secondary-shift-n", NewWindow, context),
        KeyBinding::new("secondary-o", OpenFile, context),
        KeyBinding::new("secondary-shift-o", OpenLog, context),
        KeyBinding::new("secondary-s", Save, context),
        KeyBinding::new("secondary-shift-s", SaveAs, context),
        KeyBinding::new("secondary-p", Print, context),
        KeyBinding::new("secondary-w", CloseTab, context),
        KeyBinding::new("ctrl-tab", NextTab, context),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, context),
        KeyBinding::new("secondary-shift-]", NextTab, context),
        KeyBinding::new("secondary-shift-[", PreviousTab, context),
        KeyBinding::new("secondary-alt-t", ToggleTail, context),
        KeyBinding::new("secondary-f", Find, context),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-f", FindReplace, context),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-h", FindReplace, context),
        KeyBinding::new("secondary-alt-p", GeneratePassword, context),
        KeyBinding::new("secondary-]", IndentMore, context),
        KeyBinding::new("secondary-[", IndentLess, context),
        KeyBinding::new("secondary-shift-d", DuplicateLines, context),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-ctrl-j", JoinLines, context),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-j", JoinLines, context),
        // Notepad++'s keys: the editor itself uses Mod-Alt-Up/Down (add a cursor)
        // and Mod-Enter, so these differ from MacPad++'s.
        // (Ctrl-Shift-Up/Down is Notepad++'s; on macOS Ctrl-Up is Mission Control's.)
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-ctrl-up", MoveLinesUp, context),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-ctrl-down", MoveLinesDown, context),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-up", MoveLinesUp, context),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-down", MoveLinesDown, context),
        KeyBinding::new("secondary-alt-enter", InsertLineAbove, context),
        KeyBinding::new("secondary-alt-shift-enter", InsertLineBelow, context),
        KeyBinding::new("secondary-/", ToggleComment, context),
        KeyBinding::new("secondary-alt-/", BlockComment, context),
        KeyBinding::new("secondary-=", ZoomIn, context),
        KeyBinding::new("secondary-+", ZoomIn, context),
        KeyBinding::new("secondary--", ZoomOut, context),
        KeyBinding::new("secondary-0", ZoomReset, context),
        KeyBinding::new("f7", NextChange, context),
        KeyBinding::new("shift-f7", PreviousChange, context),
        KeyBinding::new("alt-z", ToggleSoftWrap, context),
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("secondary-,", OpenSettings, None),
    ]);
}
