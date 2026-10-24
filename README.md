# OxyPad

A fast, native, cross-platform text editor written in Rust, in the spirit of
Notepad++ and MacPad++, for macOS, Windows and Linux. Prototype stage.

Downloads: [GitHub Releases](https://github.com/oxyapps/OxyPad/releases)

## Features

- Tabs, syntax highlighting and folding for 40 languages with Lexilla, the
  lexers of Scintilla and Notepad++, find/replace
- Toolbar whose buttons enable themselves for the active tab
- Tab menu (right click): close, pin, rename, read-only, show in Finder/Explorer,
  open the folder in a terminal (chosen in Settings: Terminal, iTerm, Warp, Ghostty;
  Windows Terminal, PowerShell, Command Prompt; the usual Linux ones; or your own
  command), copy the folder, name or full path, six tab colors, move to start/end, save;
  drag tabs to reorder them; double-click the free space of the tab bar for a new tab
- Several windows (Mod-Shift-N, or Move to Window from a tab's menu), all
  restored by the session
- Printing with syntax colors (Mod-P), through the system's print dialog
- Side-by-side comparison of two open tabs: 50/50 columns with a draggable
  divider (double-click resets it), both columns scroll together, each scrolls
  sideways on its own, changed lines and words are highlighted, F7 jumps
  between differences
- Notepad++'s text operations in the Edit menu: indent, convert case, line operations (duplicate,
  join, move, remove duplicates and empty lines), 14 ways to sort lines, trim, tabs to spaces
  and back, and comment toggling
- Zoom each tab's text with View ▸ Zoom, Mod-scroll or a trackpad pinch; the zoom level
  is in the status bar, and clicking it opens the zoom menu. The status bar also counts
  the selected characters
- Password generator (Tools menu, Mod-Alt-P): the length is shared out among uppercase,
  lowercase, numbers and symbols, optionally without ambiguous characters;
  copy the password or insert it at the cursor
- Color themes: about 40 built in (Catppuccin, Solarized, Gruvbox, Tokyo Night,
  Ayu, …), a color scheme remembered for each of light and dark mode, plus your own
  theme files (Settings › Appearance › Open Folder)
- English and Portuguese (Brazil) interface, following the system language or
  chosen in Settings; adding a language is one YAML file (see [TRANSLATING.md](TRANSLATING.md))
- Settings window (Mod-,): session, font, indentation, display, theme,
  format of new documents, log options
- Sessions like Notepad++: open tabs and unsaved edits survive quitting and crashes
- Syntax check for JSON (and JSON with comments), YAML, TOML and XML as you type: the
  error is underlined, explained on hover and shown in the status bar (click to go there)
- One OxyPad at a time: a file opened from File Explorer (*Open with*, *Edit with
  OxyPad*), Finder, a file manager or the command line becomes a tab of the running one
- Notices files changed by other programs and asks what to do: reload, follow
  as a log, keep your version or close (and save again when a file was deleted)
- Encodings and line endings kept on save (UTF-8, UTF-8 BOM, UTF-16, legacy code pages; LF/CRLF/CR)
- **Log mode (`tail -F`)**: the Tail toolbar button turns it on and off for any saved file; read-only, follows appends, survives truncation and
  rotation, colors ERROR/WARN/DEBUG lines, pauses when you scroll up. Settings ▸ Logs ▸
  Colors and keywords sets each level's color and words, and your own highlights (text,
  color, background or whole line), with a live preview

The session and the settings live in `~/Library/Application Support/oxypad` on
macOS, `%LOCALAPPDATA%\oxypad` on Windows and `~/.local/share/oxypad` on Linux.

## Download

Get the latest version from [GitHub Releases](https://github.com/oxyapps/OxyPad/releases).
Each file has a `.sha256` next to it.

| File | For |
| ---- | --- |
| `OxyPad-<version>-macos-apple-silicon.dmg` | Macs with M-series chips |
| `OxyPad-<version>-macos-intel.dmg` | Intel Macs |
| `OxyPad-<version>-windows-x64.msi` | Windows on x64, installer |
| `OxyPad-<version>-windows-x64-portable.zip` | Windows on x64, no installation |
| `OxyPad-<version>-windows-arm64.msi` | Windows on ARM, installer |
| `OxyPad-<version>-windows-arm64-portable.zip` | Windows on ARM, no installation |
| `OxyPad-<version>-linux-<arch>.AppImage` | Linux on x86_64 or aarch64, any distribution |
| `OxyPad-<version>-linux-<arch>.deb` / `.rpm` | Debian and Ubuntu / Fedora and openSUSE |
| `OxyPad-<version>-linux-<arch>.tar.gz` | Linux, the files alone |

The MSI asks for two options, both on by default: a desktop shortcut, and *Edit
with OxyPad* in the right-click menu of every file. It also lists OxyPad under
*Open with* and in *Settings ▸ Default apps* for text and code files; Windows
leaves choosing the default app to you. Unattended installs take
`INSTALLDESKTOPSHORTCUT=0` and `INSTALLCONTEXTMENU=0`. The Windows builds are
not signed yet, so SmartScreen may ask before the first run.

On macOS, OxyPad is under *Open With* in Finder for every file, never the default.

## Shortcuts

`Mod` is Cmd on macOS and Ctrl on Windows/Linux.

| Keys            | Action                     |
| --------------- | -------------------------- |
| Mod-N / Mod-O   | New / Open                 |
| Mod-Shift-N     | New window                 |
| Mod-Shift-O     | Follow a log               |
| Mod-S / Mod-Shift-S | Save / Save As         |
| Mod-P           | Print                      |
| Mod-W           | Close tab                  |
| Ctrl-Tab        | Next tab                   |
| Alt-Z           | Word wrap                  |
| Mod-,           | Settings                   |
| Mod-F           | Find                       |
| Cmd-Shift-F / Ctrl-H | Replace               |
| Mod-Alt-T       | Follow the open file as a log (tail) and back |
| Mod-Alt-P       | Password generator         |
| Mod-+ / Mod-- / Mod-0 | Zoom in / out / actual size |
| F7 / Shift-F7   | Next / previous difference in a comparison |

## Development

Building, testing, packaging, releasing and the source layout are covered in
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Credits

- [Notepad++](https://notepad-plus-plus.org) by Don Ho and
  [MacPad++](https://github.com/macpad-plus-plus/macpad-plus-plus), the inspiration;
  the password generator follows MacPad++'s rules.
- [Lexilla and Scintilla](https://www.scintilla.org) by Neil Hodgson, the syntax
  highlighting and folding; the keyword lists come from SciTE.
- [gpui-kit](https://github.com/longbridge/gpui-kit), the UI components and the
  built-in color themes, on [GPUI](https://www.gpui.rs) from the Zed editor.
- Built with [Claude](https://claude.ai).

## License

Apache-2.0. `crates/syntax/vendor` holds Lexilla 5.5.3 and three headers of
Scintilla 5.6.6, by Neil Hodgson, under their own permissive license
(`crates/syntax/vendor/lexilla/License.txt`); the keyword lists come from
SciTE, under the same license.
