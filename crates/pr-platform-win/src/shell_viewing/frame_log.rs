//! Port of `PhotoReview.Shell.Win32/Viewing/FrameLog.cs` (70 lines, WP21-STALL, baseline d92d6aae): per-frame diagnostic log, OFF
//! by default. When the variable is set to a file path, every drawn frame appends one line:
//! `F gap=<ms since previous frame> frame=<ms> endDraw=<ms> present=<ms>` (fixed decimals, invariant culture).
//! The line format is the contract with the stall tooling (`tools/diag/stall-experiment.ps1` in the C# repo counts frames
//! blocked by `Present`). A write error (disk full, lock) switches the log off; it never affects drawing.
//!
//! Rust deviations (contract D-16a-3): the variable is `PHOTOREVIEW_RS_FRAME_LOG` (AGENTS.md: `PHOTOREVIEW_RS_*` env vars);
//! lines end with CRLF (C# `StreamWriter.WriteLine` on Windows writes `Environment.NewLine`); each line is written with one
//! unbuffered `write_all`, so readers see it at once (C# `AutoFlush = true`).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use crate::shell_viewing::frame_stats::ticks_to_ms;

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
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: Some(file),
            last_frame: 0,
            ticks_per_second,
        })
    }

    /// C# `FromEnvironment(get)`. `None` when the value is unset, blank (whitespace only) or the file cannot be opened.
    /// Production passes `|name| std::env::var(name).ok()`; the test passes a closure.
    pub fn from_environment(get: impl Fn(&str) -> Option<String>, ticks_per_second: i64) -> Option<Self> {
        let value = get(ENV_VARIABLE)?;
        if value.trim().is_empty() {
            return None;
        }
        Self::open(Path::new(&value), ticks_per_second).ok()
    }

    /// C# `Format(gapMs, frameMs, endDrawMs, presentMs)`: `F gap={:F1} frame={:F2} endDraw={:F2} present={:F2}`, no line ending.
    /// Decimals use the same rule as `pr_perf::rules::fmt_fixed` (the C# `F` format), NaN and infinities as in that helper.
    pub fn format(gap_ms: f64, frame_ms: f64, end_draw_ms: f64, present_ms: f64) -> String {
        format!(
            "F gap={} frame={} endDraw={} present={}",
            pr_perf::rules::fmt_fixed(gap_ms, 1),
            pr_perf::rules::fmt_fixed(frame_ms, 2),
            pr_perf::rules::fmt_fixed(end_draw_ms, 2),
            pr_perf::rules::fmt_fixed(present_ms, 2),
        )
    }

    /// C# `Record(frameStart, frameEnd, endDrawMs, presentMs)` with Stopwatch-tick arguments. No-op after `close`. The gap is
    /// `0.0` for the first frame and whenever the previous frame's start tick was `0` (C# `_lastFrame == 0` test, kept as is);
    /// otherwise `ticks_to_ms(frame_start - last_frame)`. `last_frame` becomes `frame_start` before the write.
    pub fn record(&mut self, frame_start: i64, frame_end: i64, end_draw_ms: f64, present_ms: f64) {
        if self.file.is_none() {
            return;
        }
        // Wrapping differences match C# unchecked `long` arithmetic and never panic in debug builds.
        let gap_ms = if self.last_frame == 0 {
            0.0
        } else {
            ticks_to_ms(frame_start.wrapping_sub(self.last_frame), self.ticks_per_second)
        };
        let frame_ms = ticks_to_ms(frame_end.wrapping_sub(frame_start), self.ticks_per_second);
        self.last_frame = frame_start;

        let line = format!("{}\r\n", Self::format(gap_ms, frame_ms, end_draw_ms, present_ms));
        let written = match self.file.as_mut() {
            Some(file) => file.write_all(line.as_bytes()).is_ok(),
            None => false,
        };
        if !written {
            // C# disposes the writer on an IOException; the log then stays off for the session.
            self.close();
        }
    }

    /// C# `Dispose()`: releases the file; later `record` calls do nothing. Idempotent.
    pub fn close(&mut self) {
        self.file = None;
    }
}

impl Drop for FrameLog {
    fn drop(&mut self) {
        self.close();
    }
}
