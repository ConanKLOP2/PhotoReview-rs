//! Port of `PhotoReview.Shell.Win32/Startup/ShellErrorLog.cs` (117 lines, baseline `d92d6aae`). Unit W8-17a. Contract: `contracts/W8-17a.md`.
//!
//! The small error file of the shell (`shell-errors.log`, next to `config.json`): menu build/show errors (and similar swallowed errors) are written EVEN WHEN the
//! general log is off (INV-10). Nothing is written on the normal path; the file is cut to its newest half when it grows over [`MAX_BYTES`]. A line holds the UTC
//! time, the context, the error type, the HRESULT, the message (cut at 300 characters) and at most 12 stack frames: no image or settings path. Never fails.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::error::Error;
use std::path::{Path, PathBuf};

use pr_core::time::DateTimeUtc;

/// C# `ShellErrorLog.MaxBytes` (256 KiB).
pub const MAX_BYTES: u64 = 256 * 1024;
/// C# `ShellErrorLog.MaxMessageChars` (UTF-16 code units).
pub const MAX_MESSAGE_CHARS: usize = 300;
/// C# `ShellErrorLog.MaxStackFrames`.
pub const MAX_STACK_FRAMES: usize = 12;
/// C# `ShellErrorLog.FileName`.
pub const FILE_NAME: &str = "shell-errors.log";

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
        todo!()
    }

    /// The details of a Rust error: `type_name` `"std::io::Error"` for an `io::Error` else `"Error"`; `hresult` from `raw_os_error` (`HRESULT_FROM_WIN32`) else
    /// `E_FAIL`; `message` = `to_string()`; `stack` = the lines of `Backtrace::force_capture()` at the call; `inner` = `source()` (type `"Error"`).
    pub fn from_error(error: &(dyn Error + 'static)) -> Self {
        todo!()
    }
}

/// C# `Configure(path)`: the file in use (`None` = not configured: [`report`] does nothing). Process-wide, thread-safe.
pub fn configure(path: Option<PathBuf>) {
    todo!()
}

/// C# `CurrentPath`.
pub fn current_path() -> Option<PathBuf> {
    todo!()
}

/// C# `Report(context, exception)` for a Rust error: `report_details(context, &ErrorDetails::from_error(error))`.
pub fn report(context: &str, error: &(dyn Error + 'static)) {
    todo!()
}

/// C# `Report` with prepared details: reads the configured path under the gate, then [`append`]s to it; nothing when not configured.
pub fn report_details(context: &str, details: &ErrorDetails) {
    todo!()
}

/// C# `Format(utcNow, context, exception)`: the text of one entry, lines ended by `"\r\n"`.
pub fn format(utc_now: DateTimeUtc, context: &str, details: &ErrorDetails) -> String {
    todo!()
}

/// C# `Append(path, context, exception, maxBytes)` with the clock read inside: `append_at(.., DateTimeUtc::now())`.
pub fn append(path: &Path, context: &str, details: &ErrorDetails, max_bytes: u64) {
    todo!()
}

/// C# `Append` with the time as an argument (the test seam): creates the folder, cuts an over-limit file to its newest half on a line boundary, appends
/// `format(..)` as UTF-8 without BOM. Every `io::Error` is swallowed.
pub fn append_at(path: &Path, context: &str, details: &ErrorDetails, max_bytes: u64, utc_now: DateTimeUtc) {
    todo!()
}
