//! Port of `PhotoReview.Shell.Win32/Viewing/FrameLog.cs` (70 lines, WP21-STALL, baseline d92d6aae): per-frame diagnostic log, OFF
//! by default. When the variable is set to a file path, every drawn frame appends one line:
//! `F gap=<ms since previous frame> frame=<ms> endDraw=<ms> present=<ms>` (fixed decimals, invariant culture).
//! The line format is the contract with the stall tooling (`tools/diag/stall-experiment.ps1` in the C# repo counts frames
//! blocked by `Present`). A write error (disk full, lock) switches the log off; it never affects drawing.
//!
//! Rust deviations (contract D-16a-3): the variable is `PHOTOREVIEW_RS_FRAME_LOG` (AGENTS.md: `PHOTOREVIEW_RS_*` env vars);
//! lines end with CRLF (C# `StreamWriter.WriteLine` on Windows writes `Environment.NewLine`); each line is written with one
//! unbuffered `write_all`, so readers see it at once (C# `AutoFlush = true`).
//!
//! Stub: bodies are `todo!()`; signatures are final (contract W8-16a, Public API).

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use std::fs::File;
use std::io;
use std::path::Path;

/// The Rust environment variable (AGENTS.md `PHOTOREVIEW_RS_*`). Read by [`FrameLog::from_environment`] in production.
pub const ENV_VARIABLE: &str = "PHOTOREVIEW_RS_FRAME_LOG";

/// The C# variable name. Only the golden test uses it (the oracle keys its cases by the C# name, as W1-15 does).
pub const CSHARP_ENV_VARIABLE: &str = "PHOTOREVIEW_FRAME_LOG";

/// C# `FrameLog` (`internal sealed class`, `IDisposable`). `Drop` is the C# `Dispose` (idempotent).
#[derive(Debug)]
pub struct FrameLog {
    file: Option<File>,
    last_frame: i64,
    ticks_per_second: i64,
}

impl FrameLog {
    /// C# `new FrameLog(path)`: opens for append, creating the file when missing (no BOM). `Err` when the file cannot be opened.
    pub fn open(path: &Path, ticks_per_second: i64) -> io::Result<Self> {
        todo!()
    }

    /// C# `FromEnvironment(get)`. `None` when the value is unset, blank (whitespace only) or the file cannot be opened.
    /// Production passes `|name| std::env::var(name).ok()`; the test passes a closure.
    pub fn from_environment(get: impl Fn(&str) -> Option<String>, ticks_per_second: i64) -> Option<Self> {
        todo!()
    }

    /// C# `Format(gapMs, frameMs, endDrawMs, presentMs)`: `F gap={:F1} frame={:F2} endDraw={:F2} present={:F2}`, no line ending.
    /// Decimals use the same rule as `pr_perf::rules::fmt_fixed` (the C# `F` format), NaN and infinities as in that helper.
    pub fn format(gap_ms: f64, frame_ms: f64, end_draw_ms: f64, present_ms: f64) -> String {
        todo!()
    }

    /// C# `Record(frameStart, frameEnd, endDrawMs, presentMs)` with Stopwatch-tick arguments. No-op after `close`. The gap is
    /// `0.0` for the first frame and whenever the previous frame's start tick was `0` (C# `_lastFrame == 0` test, kept as is);
    /// otherwise `ticks_to_ms(frame_start - last_frame)`. `last_frame` becomes `frame_start` before the write.
    pub fn record(&mut self, frame_start: i64, frame_end: i64, end_draw_ms: f64, present_ms: f64) {
        todo!()
    }

    /// C# `Dispose()`: releases the file; later `record` calls do nothing. Idempotent.
    pub fn close(&mut self) {
        todo!()
    }
}
