//! Red tests (W8-16a, test porter) for `pr_platform_win::shell_viewing::frame_log`: the port of
//! `PhotoReview.Shell.Win32/Viewing/FrameLog.cs` (baseline `d92d6aae`, WP21-STALL).
//!
//! Ported from `tests/PhotoReview.Shell.Tests/Viewing/FrameLogTests.cs` (8 tests, same names in snake_case, each with a
//! `// C#:` comment). Deviations, all from contract D-16a-3: the environment variable is `PHOTOREVIEW_RS_FRAME_LOG`
//! (`ENV_VARIABLE`), so the env test asks for that name; lines end with CRLF (trap T9, checked on the raw bytes).
//! Golden drivers over `tests/golden/frame-log.json` live in `shell_viewing_golden.rs`.
//!
//! Every temp file is unique (`temp_dir` + pid + nanos) and removed by [`TempFile`], which runs after the log is dropped.
//! The unopenable-path tests use a directory that does not exist; no test asserts on the temp prefix.
//!
//! Every test here fails through `todo!()` until the bodies land.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pr_platform_win::shell_viewing::frame_log::{CSHARP_ENV_VARIABLE, ENV_VARIABLE, FrameLog};

/// Ticks per second used by every test in this file (the golden's `ticks_per_second`).
const TPS: i64 = 10_000_000;

/// A unique temp path that is removed on drop (the C# `finally { File.Delete(path) }`).
struct TempFile(PathBuf);

impl TempFile {
    fn new(tag: &str) -> Self {
        Self(std::env::temp_dir().join(format!("pr-w8-16a-{tag}-{}-{}.log", std::process::id(), unique_nanos())))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn unique_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0)
}

// C#: FrameLogTests.Format_IsTheDocumentedLine_InvariantCulture
#[test]
fn format_is_the_documented_line() {
    assert_eq!(
        FrameLog::format(250.34, 3.456, 1.2, 0.987),
        "F gap=250.3 frame=3.46 endDraw=1.20 present=0.99"
    );
}

// C#: FrameLogTests.FromEnvironment_IsNullWhenUnsetOrBlank
#[test]
fn from_environment_is_none_when_unset_or_blank() {
    assert!(FrameLog::from_environment(|_| None, TPS).is_none());
    assert!(FrameLog::from_environment(|_| Some("  ".to_owned()), TPS).is_none());
}

// C#: FrameLogTests.FromEnvironment_IsNullWhenThePathCannotBeOpened
#[test]
fn from_environment_is_none_when_path_cannot_be_opened() {
    let missing_dir = std::env::temp_dir().join(format!("no-such-dir-{}-{}", std::process::id(), unique_nanos()));
    let path = missing_dir.join("f.log").to_string_lossy().into_owned();

    assert!(FrameLog::from_environment(|_| Some(path.clone()), TPS).is_none());
}

// C#: FrameLogTests.FromEnvironment_ReadsTheDocumentedVariable
#[test]
fn from_environment_reads_the_documented_variable() {
    let file = TempFile::new("env");
    let asked = RefCell::new(String::new());
    let path = file.path().to_string_lossy().into_owned();

    let log = FrameLog::from_environment(
        |name| {
            *asked.borrow_mut() = name.to_owned();
            Some(path.clone())
        },
        TPS,
    );

    assert!(log.is_some());
    drop(log); // the log closes before the TempFile removes the file
    assert_eq!(*asked.borrow(), ENV_VARIABLE);
    assert_eq!(ENV_VARIABLE, "PHOTOREVIEW_RS_FRAME_LOG"); // new: D-16a-3, the C# name is only the golden's key
    assert_eq!(CSHARP_ENV_VARIABLE, "PHOTOREVIEW_FRAME_LOG");
}

// C#: FrameLogTests.Record_WritesOneLinePerFrame_WithTheGapSincePreviousFrame
#[test]
fn record_writes_one_line_per_frame_with_gap() {
    let file = TempFile::new("gap");
    let f = TPS;
    {
        let mut log = FrameLog::open(file.path(), TPS).expect("open the frame log");
        log.record(10 * f, 10 * f + f / 100, 1.5, 0.5); // 10 ms
        log.record(10 * f + f / 4, 10 * f + f / 4 + f / 50, 12.0, 1.0); // gap 250 ms, 20 ms
        log.close();
    }

    let text = std::fs::read_to_string(file.path()).expect("read the frame log");
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        [
            "F gap=0.0 frame=10.00 endDraw=1.50 present=0.50",
            "F gap=250.0 frame=20.00 endDraw=12.00 present=1.00",
        ]
    );
    // new: trap T9, the raw bytes are CRLF after every line (C# Environment.NewLine on Windows)
    assert_eq!(
        text,
        "F gap=0.0 frame=10.00 endDraw=1.50 present=0.50\r\nF gap=250.0 frame=20.00 endDraw=12.00 present=1.00\r\n"
    );
}

// C#: FrameLogTests.Record_IsFlushedImmediately_WhileTheLogIsStillOpen
#[test]
fn record_is_visible_to_a_reader_while_open() {
    let file = TempFile::new("flush");
    let mut log = FrameLog::open(file.path(), TPS).expect("open the frame log");
    log.record(5, 5 + TPS / 100, 1.0, 0.5);

    // the measuring tool reads the file while the process is still running (C# AutoFlush = true)
    let text = std::fs::read_to_string(file.path()).expect("read while the log is open");
    assert!(text.starts_with("F gap=0.0 frame=10.00"), "got {text:?}");

    log.close();
}

// C#: FrameLogTests.NewInstances_AppendToAnExistingFile
#[test]
fn new_instances_append_to_an_existing_file() {
    let file = TempFile::new("append");
    {
        let mut first = FrameLog::open(file.path(), TPS).expect("open the first log");
        first.record(1, 2, 0.0, 0.0);
        first.close();
    }
    {
        let mut second = FrameLog::open(file.path(), TPS).expect("open the second log");
        second.record(1, 2, 0.0, 0.0);
        second.close();
    }

    let text = std::fs::read_to_string(file.path()).expect("read the frame log");
    assert_eq!(text.lines().count(), 2);
}

// C#: FrameLogTests.Record_AfterDispose_IsANoOp
#[test]
fn record_after_close_is_a_no_op() {
    let file = TempFile::new("closed");
    let mut log = FrameLog::open(file.path(), TPS).expect("open the frame log");
    log.close();

    log.record(1, 2, 0.0, 0.0);
    log.close(); // idempotent

    let text = std::fs::read_to_string(file.path()).expect("read the frame log");
    assert_eq!(text, "");
}
