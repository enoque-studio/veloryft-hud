//! Startup and failure diagnostics.
//!
//! The release binary is built for the Windows subsystem, so it has no console:
//! anything written to stderr is lost. Failures that a user can act on are
//! therefore written to a bounded log file and, when they prevent startup,
//! shown once in a message box.
//!
//! Deliberate limits:
//! * Nothing here runs per frame. The render loop must stay allocation-free and
//!   lock-free; call these only on startup, on shutdown, and on error paths.
//! * Telemetry values, machine identifiers, and file contents are never logged.
//!   Log lines describe *what failed*, not *what the user was driving*.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::paths;

/// Log file name inside [`crate::paths::log_dir`].
const LOG_FILE_NAME: &str = "veloryft.log";
/// Previous log, kept across exactly one rotation.
const ROTATED_LOG_FILE_NAME: &str = "veloryft.log.1";
/// Rotate once the active log passes this size. Two files of this size is the
/// worst case a user ever pays on disk.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// Environment variable selecting the minimum level written to the log.
pub const LOG_LEVEL_VAR: &str = "VELORYFT_LOG";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

impl Level {
    const fn label(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "error" => Some(Self::Error),
            "warn" | "warning" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" | "trace" => Some(Self::Debug),
            _ => None,
        }
    }
}

/// Minimum level actually written. Read on every record, so the check is a
/// single relaxed atomic load rather than a lock.
static MIN_LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

fn log_file() -> Option<&'static Mutex<File>> {
    static FILE: OnceLock<Option<Mutex<File>>> = OnceLock::new();
    FILE.get_or_init(|| {
        let dir = paths::log_dir();
        fs::create_dir_all(&dir).ok()?;
        rotate_if_oversized(&dir);
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(LOG_FILE_NAME))
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

fn rotate_if_oversized(dir: &std::path::Path) {
    let active = dir.join(LOG_FILE_NAME);
    let oversized = fs::metadata(&active).is_ok_and(|meta| meta.len() >= MAX_LOG_BYTES);
    if oversized {
        // Replaces the previous rotation; two files is the whole budget.
        let _ = fs::rename(&active, dir.join(ROTATED_LOG_FILE_NAME));
    }
}

/// Reads the configured level and opens the log file. Safe to call more than
/// once; only the first call has an effect on the file handle.
pub fn init() {
    if let Some(level) = std::env::var(LOG_LEVEL_VAR)
        .ok()
        .as_deref()
        .and_then(Level::parse)
    {
        MIN_LEVEL.store(level as u8, Ordering::Relaxed);
    }
    let _ = log_file();
}

/// Writes one record. Never panics and never blocks on anything but the log
/// mutex; if the file cannot be opened the record is dropped.
pub fn record(level: Level, message: &fmt::Arguments<'_>) {
    if (level as u8) > MIN_LEVEL.load(Ordering::Relaxed) {
        return;
    }
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    if let Some(file) = log_file() {
        if let Ok(mut file) = file.lock() {
            let _ = writeln!(file, "{seconds} {} {message}", level.label());
        }
    }
    #[cfg(debug_assertions)]
    eprintln!("[{}] {message}", level.label());
}

/// One-shot latch for failures that can repeat every frame.
///
/// The render and throttle loops must never write a log line per frame. Guard
/// their error branches with this so the first failure is recorded and the rest
/// are silent. The atomic is only touched on the error path.
pub struct Once(AtomicBool);

impl Once {
    #[must_use]
    pub const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    /// `true` exactly once, for the first caller.
    pub fn first(&self) -> bool {
        !self.0.swap(true, Ordering::Relaxed)
    }
}

impl Default for Once {
    fn default() -> Self {
        Self::new()
    }
}

macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::diagnostics::record(
            $crate::diagnostics::Level::Error,
            &format_args!($($arg)*),
        )
    };
}

macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::diagnostics::record(
            $crate::diagnostics::Level::Warn,
            &format_args!($($arg)*),
        )
    };
}

macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::diagnostics::record(
            $crate::diagnostics::Level::Info,
            &format_args!($($arg)*),
        )
    };
}

pub(crate) use {log_error, log_info, log_warn};

/// Shows a startup failure the user can act on, then returns.
///
/// `summary` is one plain sentence with the suggested fix; `detail` is the
/// technical cause and also goes to the log. Falls back to stderr off Windows.
pub fn report_startup_failure(summary: &str, detail: &fmt::Arguments<'_>) {
    record(
        Level::Error,
        &format_args!("startup failed: {summary} ({detail})"),
    );

    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

        let body = crate::localization::startup_failure_body(summary, detail, &paths::log_dir());
        let body = wide(&body);
        let title = wide(crate::localization::text().startup_dialog_title);
        // SAFETY: both pointers refer to local NUL-terminated UTF-16 buffers that
        // outlive the call, and a null owner window is valid for a modal box.
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(body.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    #[cfg(not(windows))]
    eprintln!("{summary}\n{detail}");
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_parsing_accepts_the_documented_spellings() {
        assert_eq!(Level::parse("error"), Some(Level::Error));
        assert_eq!(Level::parse("  WARN "), Some(Level::Warn));
        assert_eq!(Level::parse("warning"), Some(Level::Warn));
        assert_eq!(Level::parse("Info"), Some(Level::Info));
        assert_eq!(Level::parse("debug"), Some(Level::Debug));
        assert_eq!(Level::parse("nonsense"), None);
    }

    #[test]
    fn levels_order_from_most_to_least_severe() {
        assert!(Level::Error < Level::Warn);
        assert!(Level::Warn < Level::Info);
        assert!(Level::Info < Level::Debug);
    }

    #[test]
    fn rotation_only_triggers_past_the_size_budget() {
        let dir =
            std::env::temp_dir().join(format!("veloryft-log-rotation-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let active = dir.join(LOG_FILE_NAME);
        fs::write(&active, b"small").unwrap();
        rotate_if_oversized(&dir);
        assert!(active.exists(), "a small log must not be rotated");
        assert!(!dir.join(ROTATED_LOG_FILE_NAME).exists());

        fs::write(&active, vec![b'x'; MAX_LOG_BYTES as usize]).unwrap();
        rotate_if_oversized(&dir);
        assert!(!active.exists(), "an oversized log must be moved aside");
        assert!(dir.join(ROTATED_LOG_FILE_NAME).exists());

        fs::remove_dir_all(&dir).unwrap();
    }
}
