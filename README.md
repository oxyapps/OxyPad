# OxyPad

A fast, native, cross-platform text editor written in Rust, in the spirit of
Notepad++ and MacPad++. Prototype stage.

- Tabs, syntax highlighting and folding for 40 languages with Lexilla, the
  lexers of Scintilla and Notepad++, find/replace
- Toolbar whose buttons enable themselves for the active tab
- Tab menu (right click): close, pin, rename, read-only, show in Finder/Explorer,
  copy the folder, name or full path, six tab colors, move to start/end, save;
  drag tabs to reorder them
- Several windows (Mod-Shift-N, or Move to Window from a tab's menu), all
  restored by the session
- Printing with syntax colors (Mod-P), through the system's print dialog
- Side-by-side comparison of two open tabs: 50/50 columns with a draggable
  divider (double-click resets it), both columns scroll together, each scrolls
  sideways on its own, changed lines and words are highlighted, F7 jumps
  between differences
- Color themes: about 40 built in (Catppuccin, Solarized, Gruvbox, Tokyo Night,
  Ayu, …), a color scheme remembered for each of light and dark mode, plus your own theme files
- English and Portuguese (Brazil) interface, following the system language or
  chosen in Settings; adding a language is one YAML file (see `TRANSLATING.md`)
- Settings window (Mod-,): session, font, indentation, display, theme,
  format of new documents, log options; saved in `settings.json` next to the session
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

## Build and run

Requires Rust (stable) and, on macOS, Xcode for the Metal shader compiler.

```sh
cargo run --release -p oxypad -- file.rs -f /var/log/system.log
```

| Option         | Effect                                   |
| -------------- | ---------------------------------------- |
| `FILE`         | open in an editor tab                    |
| `-f`, `--follow LOG` | follow LOG read-only, like `tail -f` |

Set `OXYPAD_DATA_DIR` to keep the session somewhere other than the per-user
data directory (useful while developing).

## Syntax highlighting

Highlighting and folding come from [Lexilla](https://www.scintilla.org/Lexilla.html),
the lexers behind Scintilla, SciTE and Notepad++ (`crates/syntax`). A lexer keeps
one style byte per byte of text and a fold level per line, so a 1 MB file costs
about 1 MB to highlight. Lexing is incremental: after an edit it restarts at the
edited line and stops once its results match the previous ones. Files over 2 MB
are lexed only as far as they have been shown.

The keyword lists come from SciTE's `.properties` files
(`crates/syntax/scripts/import-scite-keywords.py` writes `src/keywords.rs`); the
table of languages, with the lexer, keywords and colors of each, is
`crates/syntax/src/languages.rs`.

## Themes

Settings › Appearance picks the mode (light, dark or matching the system) and
the color scheme for the mode in use; light and dark each keep their own. The
built-in themes come from [gpui-kit](https://github.com/longbridge/gpui-kit)
(see `crates/app/themes/README.md`). To add one, put a theme file in the same
JSON format in the themes folder (Settings › Appearance › Open Folder); it
shows up right away, and editing it updates the open windows.

## Packaging and icons

Each platform has one script in `packaging/`, which the release workflow runs
and which runs the same way locally. The version comes from one place,
`[workspace.package] version` in `Cargo.toml` (`packaging/env.sh`), and the
files go to `dist/`, each with a `.sha256`.

| Platform | Command | Output |
| -------- | ------- | ------ |
| macOS    | `packaging/macos/package.sh --arch apple-silicon\|intel` (`--app-only` stops at `OxyPad.app`) | `OxyPad-<version>-macos-<arch>.dmg` |
| Windows  | `pwsh packaging/windows/package.ps1 -Arch x64\|arm64` (needs WiX v5) | `.msi` and `-portable.zip` |
| Linux    | `packaging/linux/package.sh` (needs nfpm for deb/rpm) | `.AppImage`, `.deb`, `.rpm`, `.tar.gz` |

`scripts/install-linux.sh` installs a development build into `~/.local`.

The artwork lives in `oxypad-icons/`: macOS uses `macos/oxypad-macos.icns`
(made from `png/oxypad-1024.png` by `scripts/make-macos-icons.sh`), Windows
embeds `oxypad.ico` in `oxypad.exe` (`crates/app/build.rs`), and Linux takes
`png/` and `oxypad.svg`. The app id is `org.oxyapps.oxypad` everywhere (bundle
identifier, Wayland/X11 app id, desktop file and AppStream id).

## Releases

OxyPad follows [Semantic Versioning](https://semver.org) and keeps a
[changelog](CHANGELOG.md). Commits follow [Conventional Commits](https://www.conventionalcommits.org)
(`feat:`, `fix:`, `docs:`…), and every user-visible change gets a line under
`[Unreleased]` in `CHANGELOG.md`.

To release:

1. Set `version` in `Cargo.toml` (`[workspace.package]`) and move the
   `[Unreleased]` notes in `CHANGELOG.md` under a new `## [X.Y.Z] - date` heading,
   with its compare link at the bottom.
2. Commit as `chore(release): X.Y.Z`, then tag and push:
   `git tag vX.Y.Z && git push origin main vX.Y.Z`.
3. `.github/workflows/release.yml` checks that the tag matches `Cargo.toml` and
   that the changelog has the section, builds everything and publishes a GitHub
   Release with the changelog section as notes and these files:

| File | For |
| ---- | --- |
| `OxyPad-<version>-macos-apple-silicon.dmg` | Macs with M-series chips |
| `OxyPad-<version>-macos-intel.dmg` | Intel Macs |
| `OxyPad-<version>-windows-x64.msi` | Windows on x64, installer |
| `OxyPad-<version>-windows-x64-portable.zip` | Windows on x64, no installation |
| `OxyPad-<version>-windows-arm64.msi` | Windows on ARM, installer |
| `OxyPad-<version>-windows-arm64-portable.zip` | Windows on ARM, no installation |

The MSI asks for two options, both on by default: a desktop shortcut, and *Edit
with OxyPad* in the right-click menu of every file. It also lists OxyPad under
*Open with* and in *Settings ▸ Default apps* for text and code files; Windows
leaves choosing the default app to the user. Unattended installs take
`INSTALLDESKTOPSHORTCUT=0` and `INSTALLCONTEXTMENU=0`. On macOS, OxyPad is
under *Open With* in Finder for every file, never the default.
| `OxyPad-<version>-linux-<arch>.AppImage` | Linux on x86_64 or aarch64, any distribution |
| `OxyPad-<version>-linux-<arch>.deb` / `.rpm` | Debian and Ubuntu / Fedora and openSUSE |
| `OxyPad-<version>-linux-<arch>.tar.gz` | Linux, the files alone |

Before publishing, the workflow installs the Windows ARM64 MSI on an ARM64
runner, checks and runs OxyPad.exe, and uninstalls it. Running the workflow by
hand from the Actions tab builds and checks the same files without publishing
them. The macOS DMGs are signed with a Developer ID and notarized when the
repository has the secrets listed at the top of the workflow; otherwise they are
signed ad hoc. The Windows builds are not signed.

`.github/workflows/packaging-lint.yml` checks the workflows, the packaging
scripts, the installer XML, the desktop entry and the AppStream metadata on
every change to them, without building.

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
| F7 / Shift-F7   | Next / previous difference in a comparison |

## Layout

```text
crates/
  core/     document model: encodings, line endings, atomic saves, language detection
  tail/     log follower (truncation/rotation) and log-level classification
  session/  session and unsaved-text backups
  syntax/   syntax highlighting and folding with Lexilla (vendored C++ in vendor/)
  lint/     syntax checks for JSON, YAML, TOML and XML
  app/      the GPUI application (gpui-kit)
```

`core`, `tail`, `session`, `syntax` and `lint` do not depend on the UI and are unit
tested (`cargo test --workspace`).

## License

Apache-2.0. `crates/syntax/vendor` holds Lexilla 5.5.3 and three headers of
Scintilla 5.6.6, by Neil Hodgson, under their own permissive license
(`crates/syntax/vendor/lexilla/License.txt`); the keyword lists come from
SciTE, under the same license.
