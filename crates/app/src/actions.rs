use gpui_kit::{App, KeyBinding, actions};

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
        KeyBinding::new("f7", NextChange, context),
        KeyBinding::new("shift-f7", PreviousChange, context),
        KeyBinding::new("alt-z", ToggleSoftWrap, context),
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("secondary-,", OpenSettings, None),
    ]);
}
