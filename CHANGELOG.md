# Changelog

All notable changes to OxyPad are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A native text editor for macOS and Windows, written in Rust with GPUI: tabs, syntax
  highlighting for 30+ languages, find and replace, word wrap, and files kept in their
  own encoding (UTF-8, UTF-16, legacy code pages) and line endings.
- Sessions like Notepad++: open tabs and unsaved changes survive quitting and crashes.
- Tail mode: any saved file can be followed live and read-only, like `tail -F`. It
  survives truncation and log rotation, colors error and warning lines, and pauses
  while you scroll up. It never keeps the program writing the log from writing,
  including Windows PowerShell's `Add-Content`.
- Side-by-side comparison of two open tabs, with changed lines and words highlighted,
  synchronized scrolling, a draggable divider and F7 to jump between differences.
- When a file is changed by another program, OxyPad offers to reload it, follow it as a
  log, keep your version or close it; when it is deleted, to save it again.
- Settings for the editor font, indentation, display, new documents and logs.
- About 40 color schemes, one remembered for light mode and one for dark mode, plus
  your own theme files.
- Interface in English and Brazilian Portuguese, following the system language or the
  one picked in Settings ▸ General; new languages are single YAML files (see
  TRANSLATING.md).
- An additional code language pack with Kotlin, C#, Scala, Swift, Ruby, Elixir, Zig and PHP
  highlighting, kept out of the app to keep it small.
- Release builds: DMGs for Apple silicon and Intel Macs, and MSI installers and portable
  zips for Windows x64 and ARM64. The installer adds a Start menu shortcut and lists
  OxyPad under "Open with" for text and code files.

[Unreleased]: https://github.com/oxyapps/OxyPad/commits/main
