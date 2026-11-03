//! Port of `PhotoReview.Shell.Win32/Viewing/FrameLog.cs` (70 lines, WP21-STALL, baseline d92d6aae): per-frame diagnostic log, OFF
//! by default. When the variable is set to a file path, every drawn frame appends one line:
//! `F gap=<ms since previous frame> frame=<ms> endDraw=<ms> present=<ms>` (fixed decimals, invariant culture).
//! The line format is the contract with the stall tooling (`tools/diag/stall-experiment.ps1` in the C# repo counts frames
//! blocked by `Present`). A write error (disk full, lock) switches the log off; it never affects drawing.
//!
//! Rust deviations (contract D-16a-3): the variable is `PHOTOREVIEW_RS_FRAME_LOG` (AGENTS.md: `PHOTOREVIEW_RS_*` env vars);
//! lines end with CRLF (C# `StreamWriter.WriteLine` on Windows writes `Environment.NewLine`); each line is written with one
//! unbuffered `write_all` (no `BufWriter`), by a writer thread (P-301, below).
//!
//! P-301 (Different from C# `AutoFlush = true`, which writes synchronously on the UI path): [`FrameLog::record`] only does the gap
//! arithmetic and hands four numbers to a bounded queue; a writer thread formats and writes each line, so measuring does not bias
//! the frame time. `record` never blocks: when the queue is full the new record is dropped and counted ([`FrameLog::dropped`]);
//! the count is reported once on stderr when the log closes. [`FrameLog::close`] (and `Drop`) drains the queue and joins the
//! thread, so no queued record is lost at shutdown and the file is released. [`FrameLog::flush`] is the sync point for readers
//! that need the records written so far. A panic or a write error in the writer switches the log off; neither reaches the caller.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread::JoinHandle;

use crate::shell_viewing::frame_stats::ticks_to_ms;

/// The Rust environment variable (AGENTS.md `PHOTOREVIEW_RS_*`). Read by [`FrameLog::from_environment`] in production.
pub const ENV_VARIABLE: &str = "PHOTOREVIEW_RS_FRAME_LOG";

/// The C# variable name. Only the golden test uses it (the oracle keys its cases by the C# name, as W1-15 does).
pub const CSHARP_ENV_VARIABLE: &str = "PHOTOREVIEW_FRAME_LOG";

/// Records the queue holds before new ones are dropped (about 17 s of frames at 60 fps; a message is 32 bytes).
const QUEUE_CAPACITY: usize = 1024;

/// What the caller sends to the writer thread.
#[derive(Debug)]
enum Message {
    Frame {
        gap_ms: f64,
        frame_ms: f64,
        end_draw_ms: f64,
        present_ms: f64,
    },
    /// Answered once every earlier message was written (or dropped with the writer).
    Flush(SyncSender<()>),
}

/// C# `FrameLog` (`internal sealed class`, `IDisposable`). `Drop` is the C# `Dispose` (idempotent).
#[derive(Debug)]
pub struct FrameLog {
    sender: Option<SyncSender<Message>>,
    writer: Option<JoinHandle<()>>,
    dropped: Arc<AtomicU64>,
    failed: Arc<AtomicBool>,
    last_frame: i64,
    ticks_per_second: i64,
}

impl FrameLog {
    /// C# `new FrameLog(path)`: opens for append, creating the file when missing (no BOM). `Err` when the file cannot be opened.
    pub fn open(path: &Path, ticks_per_second: i64) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        // C# `StreamWriter(path, append: true)` opens with FileShare.Read: a second writer or a delete fails while the log is open.
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_SHARE_READ: u32 = 1;
            options.share_mode(FILE_SHARE_READ);
        }
        // Opened here, on the caller's thread, so open errors and the sharing mode behave as before P-301.
        let file = options.open(path)?;
        Ok(Self::open_with_sink(file, ticks_per_second, QUEUE_CAPACITY))
    }

    /// P-301: starts the writer thread over any sink with a queue of `capacity` records. [`FrameLog::open`] passes the file; the
    /// tests pass sinks that block, panic or fail.
    pub fn open_with_sink<W: Write + Send + 'static>(sink: W, ticks_per_second: i64, capacity: usize) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<Message>(capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicBool::new(false));
        let thread_failed = Arc::clone(&failed);
        let spawned = std::thread::Builder::new()
            .name("frame-log-writer".to_owned())
            .spawn(move || {
                let mut sink = sink;
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    while let Ok(message) = receiver.recv() {
                        match message {
                            Message::Frame {
                                gap_ms,
                                frame_ms,
                                end_draw_ms,
                                present_ms,
                            } => {
                                let line = format!("{}\r\n", Self::format(gap_ms, frame_ms, end_draw_ms, present_ms));
                                if sink.write_all(line.as_bytes()).is_err() {
                                    return false; // C# disposes the writer on an IOException; the log then stays off
                                }
                            }
                            Message::Flush(done) => {
                                let _ = done.send(());
                            }
                        }
                    }
                    true
                }));
                if !matches!(outcome, Ok(true)) {
                    thread_failed.store(true, Ordering::Release);
                }
                drop(sink); // releases the file first
                drop(receiver); // then wakes any caller waiting in `flush`
            });
        let (sender, writer) = match spawned {
            Ok(writer) => (Some(sender), Some(writer)),
            Err(_) => (None, None), // no thread: the log is off, like after a write error
        };
        Self {
            sender,
            writer,
            dropped,
            failed,
            last_frame: 0,
            ticks_per_second,
        }
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
    /// otherwise `ticks_to_ms(frame_start - last_frame)`. `last_frame` becomes `frame_start` before the record is queued.
    /// Never blocks (P-301): a full queue drops this record and counts it.
    pub fn record(&mut self, frame_start: i64, frame_end: i64, end_draw_ms: f64, present_ms: f64) {
        if self.sender.is_none() {
            return;
        }
        if self.failed.load(Ordering::Acquire) {
            // The writer hit a write error or panicked: the log stays off for the session (C# disposes on an IOException).
            self.close();
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

        let message = Message::Frame {
            gap_ms,
            frame_ms,
            end_draw_ms,
            present_ms,
        };
        let sent = match self.sender.as_ref() {
            Some(sender) => sender.try_send(message),
            None => return,
        };
        match sent {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => self.close(), // the writer is gone
        }
    }

    /// Records dropped because the queue was full (P-301).
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// P-301 sync point: returns when every record queued before the call was written, or at once when the log is off or the
    /// writer is gone. Blocks the caller until then, so the frame path never calls it; tests and the shutdown path do.
    pub fn flush(&self) {
        let Some(sender) = self.sender.as_ref() else {
            return;
        };
        let (done, wait) = mpsc::sync_channel(1);
        if sender.send(Message::Flush(done)).is_ok() {
            let _ = wait.recv(); // Err when the writer died before answering: nothing more to wait for
        }
    }

    /// C# `Dispose()`: drains the queue, stops the writer thread and releases the file; later `record` calls do nothing.
    /// Idempotent. Blocks only for the records already queued (at most the queue capacity).
    pub fn close(&mut self) {
        self.sender = None; // the writer drains what is queued, sees the channel closed and ends
        if let Some(writer) = self.writer.take() {
            let _ = writer.join(); // the panic, if any, was caught inside the thread
        }
        let dropped = self.dropped.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            // Logged once, here, so the report is not itself a per-frame cost.
            eprintln!("frame log: {dropped} record(s) dropped because the writer queue was full");
        }
    }
}

impl Drop for FrameLog {
    fn drop(&mut self) {
        self.close();
    }
}
