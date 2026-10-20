# Changelog

All notable changes to OxyPad are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-10-20

### Added

- A native text editor for macOS and Windows, written in Rust with GPUI: tabs, syntax
  highlighting and folding for 40 languages with Lexilla (the lexers of Scintilla and
  Notepad++), find and replace, word wrap, and files kept in their own encoding (UTF-8,
  UTF-16, legacy code pages) and line endings.
- Sessions like Notepad++: open tabs and unsaved changes survive quitting and crashes.
- Syntax checking for JSON (with comments and trailing commas in JSONC files such as
  `tsconfig.json`), YAML, TOML and XML, a moment after typing stops: the first error is
  underlined, its message shows on hover and in the status bar, and clicking it there
  moves to it. Files over 4 MB are not checked.
- One OxyPad at a time: opening files while it runs (from File Explorer, Finder, a file
  manager or the command line) adds them as tabs of the front window instead of starting
  another OxyPad with the same session.
- Windows installer: a welcome screen offering a desktop shortcut and *Edit with OxyPad*
  in the right-click menu of files, and OxyPad listed in *Settings ▸ Default apps* for
  text and code files. On macOS, OxyPad is offered under *Open With* in Finder, and files
  dropped on its Dock icon open.
- A menu on each tab (right click): close, pin to the left of the bar, rename (untitled
  tabs get a new name, saved files are saved under one), read-only, show the file in
  Finder or File Explorer, copy its folder, name or full path, give the tab one of six
  colors, move it to the start or end, and save it when it has unsaved changes. Pinned
  tabs, colors and read-only tabs are kept in the session. Tabs can be dragged to a
  new place in the bar.
- Open in Terminal, in a tab's menu, opens the folder of its file in the terminal chosen
  in Settings ▸ General: Terminal, iTerm, Warp or Ghostty on macOS; Windows Terminal,
  PowerShell 7, Windows PowerShell or Command Prompt on Windows; GNOME Terminal, Konsole,
  kitty and others on Linux (those installed are listed); or a command of your own, with
  `{dir}` for the folder.
- A double click on the free space of the tab bar opens a new tab.
- Several windows: File ▸ New Window (Mod-Shift-N), and Move to Window in a tab's
  menu, to a new window or one already open, which takes the tab with its unsaved text
  (moving a window's last tab closes that window). The session restores every window.
  Closing a window closes only it, asking first when its unsaved changes would be
  lost; closing the last one quits.
- Printing (File ▸ Print…, Mod-P, or a tab's menu), with syntax colors: the system's
  print panel on macOS; on Windows a preview of the pages, laid out for the default
  printer, then the Print dialog (with the file name and page numbers on each page);
  and the default printer through `lp` on Linux.
- Tail mode: any saved file can be followed live and read-only, like `tail -F`. It
  survives truncation and log rotation, colors error and warning lines, and pauses
  while you scroll up. It never keeps the program writing the log from writing,
  including Windows PowerShell's `Add-Content`.
- Log highlighting settings (Settings ▸ Logs ▸ Colors and keywords): the color and the
  keywords of each level, and your own highlights, text marked in a color or with a
  background, optionally its whole line and matching case, drawn over the level colors.
  A sample log previews the changes before they are saved.
- Side-by-side comparison of two open tabs, with changed lines and words highlighted,
  synchronized scrolling, a draggable divider and F7 to jump between differences.
- When a file is changed by another program, OxyPad offers to reload it, follow it as a
  log, keep your version or close it; when it is deleted, to save it again. After you
  keep your version it stops asking, and saving asks before overwriting the other
  program's changes (or saves yours as a new file).
- New untitled tabs take the lowest free number, like Notepad++: after closing "new 3",
  the next one is "new 3" again.
- Settings for the editor font, indentation, display, new documents and logs.
- About 40 color schemes, one remembered for light mode and one for dark mode, plus
  your own theme files.
- Interface in English and Brazilian Portuguese, following the system language or the
  one picked in Settings ▸ General; new languages are single YAML files (see
  TRANSLATING.md).
- Release builds: DMGs for Apple silicon and Intel Macs, MSI installers and portable
  zips for Windows x64 and ARM64, and AppImage, deb, rpm and tar.gz packages for Linux
  x86_64 and aarch64. The installer adds a Start menu shortcut and lists OxyPad under
  "Open with" for text and code files.
- `oxypad --version` prints the version.
- A diagnostic log for troubleshooting, `oxypad.log` in the data folder, turned on in
  Settings ▸ General ▸ Troubleshooting or with `oxypad --log`: what OxyPad does and how
  long each step takes, such as saving the session and quitting.

[Unreleased]: https://github.com/oxyapps/OxyPad/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/oxyapps/OxyPad/releases/tag/v0.1.0
