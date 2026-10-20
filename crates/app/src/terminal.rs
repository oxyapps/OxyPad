//! Opening a folder in a terminal: the one chosen in Settings, one of the
//! usual ones for each system, or a command of the user's own.

use std::{
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context as _, Result, bail};

/// A terminal OxyPad knows how to open in a folder.
pub struct Preset {
    /// Stored in the preferences.
    pub id: &'static str,
    pub name: &'static str,
    /// Run when the preset is chosen; the folder is also the working directory.
    command: fn(&Path) -> Command,
    /// Found when this program is on the PATH (or, on macOS, installed);
    /// `None` when it is always there.
    needs: Option<&'static str>,
}

/// The preference value for "your own command".
pub const CUSTOM: &str = "custom";

#[cfg(target_os = "macos")]
pub const PRESETS: &[Preset] = &[
    Preset { id: "terminal", name: "Terminal", command: |dir| open_app("Terminal", dir), needs: None },
    Preset { id: "iterm", name: "iTerm", command: |dir| open_app("iTerm", dir), needs: Some("iTerm") },
    Preset { id: "warp", name: "Warp", command: |dir| open_app("Warp", dir), needs: Some("Warp") },
    Preset {
        id: "ghostty",
        name: "Ghostty",
        command: |dir| {
            let mut command = Command::new("open");
            command.args(["-na", "Ghostty", "--args"]).arg(format!("--working-directory={}", dir.display()));
            command
        },
        needs: Some("Ghostty"),
    },
];

#[cfg(target_os = "windows")]
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "windows-terminal",
        name: "Windows Terminal",
        command: |dir| {
            let mut command = Command::new("wt.exe");
            command.arg("-d").arg(dir);
            command
        },
        needs: Some("wt.exe"),
    },
    Preset { id: "pwsh", name: "PowerShell 7", command: |_| console("pwsh.exe", &["-NoExit"]), needs: Some("pwsh.exe") },
    Preset {
        id: "powershell",
        name: "Windows PowerShell",
        command: |_| console("powershell.exe", &["-NoExit"]),
        needs: None,
    },
    Preset { id: "cmd", name: "Command Prompt", command: |_| console("cmd.exe", &["/K"]), needs: None },
];

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub const PRESETS: &[Preset] = &[
    Preset { id: "gnome-terminal", name: "GNOME Terminal", command: |dir| with_flag("gnome-terminal", "--working-directory=", dir), needs: Some("gnome-terminal") },
    Preset { id: "konsole", name: "Konsole", command: |dir| with_arg("konsole", "--workdir", dir), needs: Some("konsole") },
    Preset { id: "xfce4-terminal", name: "Xfce Terminal", command: |dir| with_flag("xfce4-terminal", "--working-directory=", dir), needs: Some("xfce4-terminal") },
    Preset { id: "ptyxis", name: "Ptyxis", command: |dir| with_arg("ptyxis", "--working-directory", dir), needs: Some("ptyxis") },
    Preset { id: "kitty", name: "kitty", command: |dir| with_arg("kitty", "--directory", dir), needs: Some("kitty") },
    Preset { id: "alacritty", name: "Alacritty", command: |dir| with_arg("alacritty", "--working-directory", dir), needs: Some("alacritty") },
    Preset { id: "wezterm", name: "WezTerm", command: |dir| {
        let mut command = Command::new("wezterm");
        command.args(["start", "--cwd"]).arg(dir);
        command
    }, needs: Some("wezterm") },
    Preset { id: "foot", name: "foot", command: |dir| with_arg("foot", "-D", dir), needs: Some("foot") },
    Preset { id: "x-terminal-emulator", name: "x-terminal-emulator", command: |_| Command::new("x-terminal-emulator"), needs: Some("x-terminal-emulator") },
    Preset { id: "xterm", name: "xterm", command: |_| Command::new("xterm"), needs: Some("xterm") },
];

#[cfg(target_os = "macos")]
fn open_app(app: &str, dir: &Path) -> Command {
    let mut command = Command::new("open");
    command.args(["-a", app]).arg(dir);
    command
}

/// A shell in a console window of its own (OxyPad has none to share).
#[cfg(target_os = "windows")]
fn console(program: &str, args: &[&str]) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x10;
    let mut command = Command::new(program);
    command.args(args).creation_flags(CREATE_NEW_CONSOLE);
    command
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn with_flag(program: &str, flag: &str, dir: &Path) -> Command {
    let mut command = Command::new(program);
    command.arg(format!("{flag}{}", dir.display()));
    command
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn with_arg(program: &str, flag: &str, dir: &Path) -> Command {
    let mut command = Command::new(program);
    command.arg(flag).arg(dir);
    command
}

impl Preset {
    fn is_installed(&self) -> bool {
        self.needs.is_none_or(is_installed)
    }
}

/// The presets installed here, for Settings.
pub fn installed() -> Vec<&'static Preset> {
    PRESETS.iter().filter(|preset| preset.is_installed()).collect()
}

/// Opens `dir` in the terminal `choice` names: a preset's id, [`CUSTOM`]
/// with `custom` as the command, or empty for the first one installed.
pub fn open(dir: &Path, choice: &str, custom: &str) -> Result<()> {
    let mut command = if choice == CUSTOM {
        custom_command(custom, dir)?
    } else {
        let preset = PRESETS
            .iter()
            .find(|preset| preset.id == choice)
            .or_else(|| PRESETS.iter().find(|preset| preset.is_installed()))
            .context("no terminal found; choose one in Settings")?;
        (preset.command)(dir)
    };
    command.current_dir(dir).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let program = command.get_program().to_string_lossy().into_owned();
    let mut child = command.spawn().with_context(|| format!("could not start {program}"))?;
    // Reaped in the background: terminals that fork return at once, others
    // run until closed.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The user's command line, with `{dir}` replaced by the folder.
fn custom_command(line: &str, dir: &Path) -> Result<Command> {
    let dir = dir.to_string_lossy();
    let words = split_words(line);
    let Some((program, args)) = words.split_first() else {
        bail!("no terminal command set; enter one in Settings");
    };
    let mut command = Command::new(program.replace("{dir}", &dir));
    command.args(args.iter().map(|arg| arg.replace("{dir}", &dir)));
    Ok(command)
}

/// Splits a command line on spaces, keeping "quoted parts" together.
fn split_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut in_word = false;
    for ch in line.chars() {
        match (quote, ch) {
            (Some(open), ch) if ch == open => quote = None,
            (Some(_), ch) => word.push(ch),
            (None, '"' | '\'') => {
                quote = Some(ch);
                in_word = true;
            }
            (None, ch) if ch.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            (None, ch) => {
                word.push(ch);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(word);
    }
    words
}

/// Whether `program` is on the PATH, or on macOS an installed app.
fn is_installed(program: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        let app = format!("{program}.app");
        let home = std::env::var_os("HOME").map(|home| Path::new(&home).join("Applications").join(&app));
        if Path::new("/Applications").join(&app).exists() || home.is_some_and(|path| path.exists()) {
            return true;
        }
    }
    let Some(path) = std::env::var_os("PATH") else { return false };
    // Not `exists()`: Windows' app execution aliases (wt.exe) are links
    // that cannot be followed.
    std::env::split_paths(&path).any(|dir| dir.join(program).symlink_metadata().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lines_split_like_a_shell() {
        assert_eq!(split_words(r#"open -a "Some App" {dir}"#), ["open", "-a", "Some App", "{dir}"]);
        assert_eq!(split_words("  wt  -d '{dir}' "), ["wt", "-d", "{dir}"]);
        assert_eq!(split_words(r#"x """#), ["x", ""]);
        assert!(split_words("   ").is_empty());
    }

    #[test]
    fn custom_commands_get_the_folder() {
        let command = custom_command("my-term --cwd={dir} -e bash", Path::new("/tmp/a b")).unwrap();
        assert_eq!(command.get_program(), "my-term");
        let args: Vec<_> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["--cwd=/tmp/a b", "-e", "bash"]);
        assert!(custom_command("  ", Path::new("/tmp")).is_err());
    }

    #[test]
    fn custom_commands_run_in_the_folder() {
        let dir = std::env::temp_dir();
        let program = if cfg!(windows) { "cmd.exe /C exit" } else { "true {dir}" };
        assert!(open(&dir, CUSTOM, program).is_ok());
        assert!(open(&dir, CUSTOM, "no-such-terminal-oxypad").is_err());
    }

    #[test]
    fn a_terminal_is_always_found() {
        // Terminal on macOS, Windows PowerShell on Windows.
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            assert!(!installed().is_empty());
        }
        eprintln!("installed: {:?}", installed().iter().map(|preset| preset.name).collect::<Vec<_>>());
    }
}
