//! Fixes the unbounded `join()` of the Shell restore worker (ST-WIN32 M3). Fakes only: the real Shell and the real
//! Recycle Bin are never touched. C# has no twin test (its `ShellStaRunner.Run` joins without limit).

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use pr_core::time::DateTimeUtc;
use pr_platform_win::recycle_bin::RecycleBinRestoreBackend;
use pr_platform_win::recycle_restore::{RecycleRestoreSession, WindowsRecycleBinRestoreBackend};

fn stamp() -> DateTimeUtc {
    DateTimeUtc::from_ticks(638_000_000_000_000_000).expect("test timestamp is in range")
}

fn backend(
    open: impl Fn() -> io::Result<Option<Box<dyn RecycleRestoreSession>>> + Send + Sync + 'static,
    timeout: Duration,
) -> WindowsRecycleBinRestoreBackend {
    WindowsRecycleBinRestoreBackend::with_adapters(
        open,
        |_, _, _| true,
        || {},
        1,
        Arc::new(pr_core::abstractions::NullLog),
    )
    .with_restore_timeout(timeout)
}

#[test]
fn try_restore_worker_stuck_in_shell_returns_timed_out_error_not_restored() {
    let (release, gate) = mpsc::channel::<()>();
    let gate = std::sync::Mutex::new(gate);
    let backend = backend(
        move || {
            // Stands for a hidden Shell dialog: blocks until the test releases it.
            let _ = gate.lock().expect("gate lock").recv();
            Ok(None)
        },
        Duration::from_millis(50),
    );
    let result = backend.try_restore(Path::new(r"C:\photos\a.jpg"), 1, stamp());
    let error = result.expect_err("a stuck Shell must not report success or 'not found'");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    let _ = release.send(()); // lets the detached worker end
}

#[test]
fn try_restore_worker_finishes_in_time_returns_its_result() {
    let backend = backend(|| Ok(None), Duration::from_secs(30));
    assert!(
        !backend
            .try_restore(Path::new(r"C:\photos\a.jpg"), 1, stamp())
            .expect("ok")
    );
}

#[test]
fn try_restore_worker_panics_returns_panicked_error() {
    let backend = backend(|| panic!("fake shell panic"), Duration::from_secs(30));
    let error = backend
        .try_restore(Path::new(r"C:\photos\a.jpg"), 1, stamp())
        .expect_err("panic is an error");
    assert!(error.to_string().contains("panicked"));
}
