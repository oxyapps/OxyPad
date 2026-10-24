# Developing OxyPad

Building, testing, packaging, releasing and the source layout. For what the
app does and how to install it, see the [README](../README.md); for
translations, [TRANSLATING.md](../TRANSLATING.md).

## Build and run

Requires Rust (stable) and, on macOS, Xcode for the Metal shader compiler.

```sh
cargo run --release -p oxypad -- file.rs -f /var/log/system.log
```

| Option         | Effect                                   |
| -------------- | ---------------------------------------- |
| `FILE`         | open in an editor tab                    |
| `-f`, `--follow LOG` | follow LOG read-only, like `tail -f` |
| `--log`        | write a diagnostic log (`oxypad.log` in the data folder) |

Set `OXYPAD_DATA_DIR` to keep the session and the settings somewhere other
than the per-user data directory (useful while developing).

Only one OxyPad runs at a time per user: a second launch hands its files to
the running one and exits. To run a build next to the OxyPad you use day to
day, give it another `USER` and data directory:

```sh
USER=oxypad-dev OXYPAD_DATA_DIR=/tmp/oxypad-dev cargo run -p oxypad
```

Tests: `cargo test --workspace`.

## Layout

```text
crates/
  core/     document model: encodings, line endings, atomic saves, language
            detection, comparison, password generator
  tail/     log follower (truncation/rotation) and log-level classification
  session/  session, settings and unsaved-text backups
  syntax/   syntax highlighting and folding with Lexilla (vendored C++ in vendor/)
  lint/     syntax checks for JSON, YAML, TOML and XML
  app/      the GPUI application (gpui-kit)
vendor/
  gpui-base/  gpui-kit's editor, patched to lay tabs out to tab stops, for
              straight block selection, to leave Mod-scroll to the app's zoom,
              and to report the selection size to the status bar
```

`core`, `tail`, `session`, `syntax` and `lint` do not depend on the UI and are unit
tested.

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

The built-in themes come from [gpui-kit](https://github.com/longbridge/gpui-kit)
(see `crates/app/themes/README.md`). A user theme is a file in the same JSON
format in the themes folder (Settings › Appearance › Open Folder); it shows up
right away, and editing it updates the open windows.

## Password generator

The rules are MacPad++'s, ported with its tests: `crates/core/src/password.rs`
shares the length out among the kinds of character (largest remainder, by the
weights 5, 5, 4, 2), and draws the characters from the system's secure random
generator (`getrandom`). The dialog is `crates/app/src/password.rs`; its
settings are the `password` entry of `settings.json`.

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
[changelog](../CHANGELOG.md). Commits follow [Conventional Commits](https://www.conventionalcommits.org)
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
   Release with the changelog section as notes and the files listed in the
   [README](../README.md#download).

Before publishing, the workflow installs the Windows ARM64 MSI on an ARM64
runner, checks and runs OxyPad.exe, and uninstalls it. Running the workflow by
hand from the Actions tab builds and checks the same files without publishing
them. The macOS DMGs are signed with a Developer ID and notarized when the
repository has the secrets listed at the top of the workflow; otherwise they are
signed ad hoc. The Windows builds are not signed.

`.github/workflows/packaging-lint.yml` checks the workflows, the packaging
scripts, the installer XML, the desktop entry and the AppStream metadata on
every change to them, without building.
