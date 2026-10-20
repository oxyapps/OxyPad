//! The diagnostic log, for troubleshooting: what OxyPad does and how long
//! it takes, written to `oxypad.log` in the data folder. It is on with
//! `--log` or the Diagnostic log setting. OxyPad's own problems also go to
//! stderr, as before, though release builds on Windows have no console.
//!
//! The rest of the app writes to it with the `log` macros, as GPUI does.

use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use log::{Level, LevelFilter, Log, Metadata, Record};

const FILE_NAME: &str = "oxypad.log";
/// A log longer than this is moved to `oxypad.old.log` when it is opened.
const MAX_SIZE: u64 = 1024 * 1024;

/// On from the command line, whatever the setting says.
static FORCED: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(false);
static STARTED: OnceLock<Instant> = OnceLock::new();
/// Opened the first time the log is turned on.
static FILE: Mutex<Option<File>> = Mutex::new(None);
static WATCHING_EXIT: AtomicBool = AtomicBool::new(false);

struct Logger;

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let ours = metadata.target().starts_with("oxypad");
        if !ENABLED.load(Ordering::Relaxed) {
            return ours && metadata.level() <= Level::Warn;
        }
        // GPUI's own debug messages are too many to be useful here.
        ours || metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if record.level() <= Level::Warn && record.target().starts_with("oxypad") {
            eprintln!("oxypad: {}", record.args());
        }
        if !ENABLED.load(Ordering::Relaxed) {
            return;
        }
        let elapsed = STARTED.get().map_or(0.0, |started| started.elapsed().as_secs_f64());
        let thread = thread::current();
        let line = format!(
            "{elapsed:>9.3}s {:<5} [{}] {}: {}\n",
            record.level(),
            thread.name().unwrap_or("?"),
            record.target(),
            record.args()
        );
        if let Some(file) = FILE.lock().unwrap().as_mut() {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {}
}

/// Installs the logger, on when `forced` (`--log`). Call first thing.
pub fn init(forced: bool) {
    STARTED.get_or_init(Instant::now);
    if log::set_logger(&Logger).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    }
    FORCED.store(forced, Ordering::Relaxed);
    apply(false);
}

/// Follows the Diagnostic log setting.
pub fn apply(setting: bool) {
    let on = setting || FORCED.load(Ordering::Relaxed);
    if on == ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if on && !open() {
        return;
    }
    if !on {
        log::info!("diagnostic log turned off");
    }
    ENABLED.store(on, Ordering::Relaxed);
    if on {
        log::info!(
            "OxyPad {} on {} {}, process {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::process::id()
        );
    }
}

/// Opens the log file, if it is not open yet.
fn open() -> bool {
    let mut file = FILE.lock().unwrap();
    if file.is_some() {
        return true;
    }
    let Some(path) = path() else { return false };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() > MAX_SIZE) {
        let _ = std::fs::rename(&path, path.with_extension("old.log"));
    }
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(opened) => {
            *file = Some(opened);
            true
        }
        Err(err) => {
            eprintln!("oxypad: could not open {}: {err}", path.display());
            false
        }
    }
}

/// Where the log is written.
pub fn path() -> Option<PathBuf> {
    oxypad_session::data_dir().ok().map(|dir| dir.join(FILE_NAME))
}

/// From now until the process ends, notes every second that it is still
/// running, so the log shows how long quitting takes after OxyPad's last
/// step.
pub fn watch_exit() {
    if !ENABLED.load(Ordering::Relaxed) || WATCHING_EXIT.swap(true, Ordering::Relaxed) {
        return;
    }
    let started = Instant::now();
    let _ = thread::Builder::new().name("oxypad-exit-watch".into()).spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(1));
            log::info!("still running {:.0}s after quitting started", started.elapsed().as_secs_f64());
        }
    });
}
