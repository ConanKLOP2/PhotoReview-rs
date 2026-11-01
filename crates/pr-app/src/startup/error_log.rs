//! Port of `PhotoReview.Shell.Win32/Startup/ShellErrorLog.cs` (117 lines, baseline `d92d6aae`). Unit W8-17a. Contract: `contracts/W8-17a.md`.
//!
//! The small error file of the shell (`shell-errors.log`, next to `config.json`): menu build/show errors (and similar swallowed errors) are written EVEN WHEN the
//! general log is off (INV-10). Nothing is written on the normal path; the file is cut to its newest half when it grows over [`MAX_BYTES`]. A line holds the UTC
//! time, the context, the error type, the HRESULT, the message (cut at 300 characters) and at most 12 stack frames: no image or settings path. Never fails.
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use pr_core::time::DateTimeUtc;

/// C# `ShellErrorLog.MaxBytes` (256 KiB).
pub const MAX_BYTES: u64 = 256 * 1024;
/// C# `ShellErrorLog.MaxMessageChars` (UTF-16 code units).
pub const MAX_MESSAGE_CHARS: usize = 300;
/// C# `ShellErrorLog.MaxStackFrames`.
pub const MAX_STACK_FRAMES: usize = 12;
/// C# `ShellErrorLog.FileName`.
pub const FILE_NAME: &str = "shell-errors.log";

static CONFIGURED_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
static APPEND_GATE: Mutex<()> = Mutex::new(());

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The inner error of an [`ErrorDetails`] (C# `exception.InnerException`: type and message only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InnerErrorDetails {
    /// C# `GetType().FullName`.
    pub type_name: String,
    /// C# `Message`.
    pub message: String,
}

/// What the log needs of a C# `Exception`: Rust errors carry no type name, HRESULT or stack, so the reporter builds this value (D-W8B6-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorDetails {
    /// C# `exception.GetType().FullName`.
    pub type_name: String,
    /// C# `exception.HResult`.
    pub hresult: i32,
    /// C# `exception.Message`.
    pub message: String,
    /// C# `exception.StackTrace` split on `'\n'`, one entry per line, untrimmed, empty entries removed.
    pub stack: Vec<String>,
    /// C# `exception.InnerException`.
    pub inner: Option<InnerErrorDetails>,
}

impl ErrorDetails {
    /// A value with no stack and no inner error.
    pub fn new(type_name: impl Into<String>, hresult: i32, message: impl Into<String>) -> Self {
        Self {
            type_name: type_name.into(),
            hresult,
            message: message.into(),
            stack: Vec::new(),
            inner: None,
        }
    }

    /// The details of a Rust error: `type_name` `"std::io::Error"` for an `io::Error` else `"Error"`; `hresult` from `raw_os_error` (`HRESULT_FROM_WIN32`) else
    /// `E_FAIL`; `message` = `to_string()`; `stack` = the lines of `Backtrace::force_capture()` at the call; `inner` = `source()` (type `"Error"`).
    pub fn from_error(error: &(dyn Error + 'static)) -> Self {
        let io_error = error.downcast_ref::<io::Error>();
        let hresult = match io_error.and_then(io::Error::raw_os_error) {
            Some(code) if code <= 0 => code,
            Some(code) => (((code as u32) & 0xFFFF) | 0x8007_0000) as i32,
            None => 0x8000_4005u32 as i32,
        };
        let stack = std::backtrace::Backtrace::force_capture()
            .to_string()
            .split('\n')
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        let inner = error.source().map(|source| InnerErrorDetails {
            type_name: "Error".to_owned(),
            message: source.to_string(),
        });
        Self {
            type_name: if io_error.is_some() { "std::io::Error" } else { "Error" }.to_owned(),
            hresult,
            message: error.to_string(),
            stack,
            inner,
        }
    }
}

/// C# `Configure(path)`: the file in use (`None` = not configured: [`report`] does nothing). Process-wide, thread-safe.
pub fn configure(path: Option<PathBuf>) {
    *lock(&CONFIGURED_PATH) = path;
}

/// C# `CurrentPath`.
pub fn current_path() -> Option<PathBuf> {
    lock(&CONFIGURED_PATH).clone()
}

/// C# `Report(context, exception)` for a Rust error: `report_details(context, &ErrorDetails::from_error(error))`.
pub fn report(context: &str, error: &(dyn Error + 'static)) {
    // C# reads the path first: nothing (no backtrace capture) when not configured.
    if lock(&CONFIGURED_PATH).is_none() {
        return;
    }
    report_details(context, &ErrorDetails::from_error(error));
}

/// C# `Report` with prepared details: reads the configured path under the gate, then [`append`]s to it; nothing when not configured.
pub fn report_details(context: &str, details: &ErrorDetails) {
    let path = lock(&CONFIGURED_PATH).clone();
    if let Some(path) = path {
        append(&path, context, details, MAX_BYTES);
    }
}

/// C# `Format(utcNow, context, exception)`: the text of one entry, lines ended by `"\r\n"`.
pub fn format(utc_now: DateTimeUtc, context: &str, details: &ErrorDetails) -> String {
    let json = utc_now.to_json();
    let timestamp = format!("{}Z", json.get(..19).unwrap_or(&json));
    let mut output = String::new();
    output.push_str(&format!(
        "{timestamp} [{context}] {} 0x{:08X}: {}\r\n",
        details.type_name,
        details.hresult as u32,
        cut_message(&replace_line_endings(&details.message))
    ));
    for frame in details.stack.iter().take(MAX_STACK_FRAMES) {
        output.push_str("    ");
        output.push_str(frame.trim());
        output.push_str("\r\n");
    }
    if let Some(inner) = &details.inner {
        output.push_str(&format!(
            "  inner: {}: {}\r\n",
            inner.type_name,
            cut_message(&replace_line_endings(&inner.message))
        ));
    }
    output
}

/// C# `Append(path, context, exception, maxBytes)` with the clock read inside: `append_at(.., DateTimeUtc::now())`.
pub fn append(path: &Path, context: &str, details: &ErrorDetails, max_bytes: u64) {
    append_at(path, context, details, max_bytes, DateTimeUtc::now());
}

/// C# `Append` with the time as an argument (the test seam): creates the folder, cuts an over-limit file to its newest half on a line boundary, appends
/// `format(..)` as UTF-8 without BOM. Every `io::Error` is swallowed.
pub fn append_at(path: &Path, context: &str, details: &ErrorDetails, max_bytes: u64, utc_now: DateTimeUtc) {
    let _gate = lock(&APPEND_GATE);
    let result = (|| -> io::Result<()> {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        if path.is_file() && fs::metadata(path)?.len() > max_bytes {
            truncate(path, max_bytes)?;
        }
        let mut file = OpenOptions::new().append(true).create(true).open(path)?;
        file.write_all(format(utc_now, context, details).as_bytes())
    })();
    let _ = result;
}

fn truncate(path: &Path, max_bytes: u64) -> io::Result<()> {
    let bytes = fs::read(path)?;
    let keep = bytes.len().min((max_bytes / 2).min(usize::MAX as u64) as usize);
    let mut start = bytes.len() - keep;
    while start < bytes.len() && start > 0 && bytes[start - 1] != b'\n' {
        start += 1;
    }
    fs::write(path, &bytes[start..])
}

fn replace_line_endings(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push(' ');
            }
            '\n' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

fn cut_message(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().take(MAX_MESSAGE_CHARS).collect();
    String::from_utf16_lossy(&units)
}
