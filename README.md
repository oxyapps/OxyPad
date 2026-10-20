# OxyPad

A fast, native, cross-platform text editor written in Rust, in the spirit of
Notepad++ and MacPad++. Prototype stage.

- Tabs, syntax highlighting (Tree-sitter, 30+ languages), find/replace
- Toolbar whose buttons enable themselves for the active tab
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
- Notices files changed by other programs and asks what to do: reload, follow
  as a log, keep your version or close (and save again when a file was deleted)
- Encodings and line endings kept on save (UTF-8, UTF-8 BOM, UTF-16, legacy code pages; LF/CRLF/CR)
- **Log mode (`tail -F`)**: the Tail toolbar button turns it on and off for any saved file; read-only, follows appends, survives truncation and
  rotation, colors ERROR/WARN/DEBUG lines, pauses when you scroll up

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

## Code language pack

The most common languages are built in. Kotlin, C#, Scala, Swift, Ruby,
Elixir, Zig and PHP have large grammars and ship separately, keeping the app
about 22 MB smaller. Opening one of those files shows a toast with a button to
the [releases page](https://github.com/oxyapps/OxyPad/releases); copy the
pack's folders into the code languages folder (Settings › Code Languages › Open Folder)
and choose Reload Languages.

Build the pack for the current platform with:

```sh
cargo run --release -p oxypad-language-pack        # → target/language-pack/
```

`.github/workflows/language-pack.yml` builds it for macOS (universal), Linux
(x86_64, aarch64) and Windows and attaches the zips to each published release.
Each language folder holds `language.json` (name, extensions, library,
symbol), the grammar as a shared library and its highlight queries; a folder
with this layout for any other Tree-sitter grammar works too.

## Themes

Settings › Appearance picks the mode (light, dark or matching the system) and
the color scheme for the mode in use; light and dark each keep their own. The
built-in themes come from [gpui-kit](https://github.com/longbridge/gpui-kit)
(see `crates/app/themes/README.md`). To add one, put a theme file in the same
JSON format in the themes folder (Settings › Appearance › Open Folder); it
shows up right away, and editing it updates the open windows.

## Packaging and icons

The artwork lives in `oxypad-icons/`; each platform takes it from there.

| Platform | How                                   | Icon source                          |
| -------- | ------------------------------------- | ------------------------------------ |
| macOS    | `scripts/bundle-macos.sh` → `target/release/bundle/OxyPad.app`; `scripts/build-dmg.sh apple-silicon\|intel` → `dist/*.dmg` | `macos/oxypad-macos.icns`, made from `png/oxypad-1024.png` by `scripts/make-macos-icons.sh` |
| Windows  | `cargo build --release` embeds it in `oxypad.exe` (`crates/app/build.rs`); the release workflow builds an MSI from `packaging/windows/OxyPad.wxs` with WiX v5 | `oxypad.ico` |
| Linux    | `scripts/install-linux.sh` (binary, `.desktop`, hicolor icons into `~/.local`) | `png/`, `oxypad.svg` |

The app id is `org.oxyapps.oxypad` everywhere (bundle identifier, Wayland/X11
app id, desktop file name).

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
| `oxypad-code-languages-<version>-<platform>.zip` | the code language pack |

Running the workflow by hand from the Actions tab builds the same files
without publishing them. The macOS DMGs are signed with a Developer ID and
notarized when the repository has the secrets listed at the top of the
workflow; otherwise they are signed ad hoc. The Windows builds are not signed.

## Shortcuts

`Mod` is Cmd on macOS and Ctrl on Windows/Linux.

| Keys            | Action                     |
| --------------- | -------------------------- |
| Mod-N / Mod-O   | New / Open                 |
| Mod-Shift-O     | Follow a log               |
| Mod-S / Mod-Shift-S | Save / Save As         |
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
  app/      the GPUI application (gpui-kit)
```

`core`, `tail` and `session` do not depend on the UI and are unit tested
(`cargo test --workspace`).
