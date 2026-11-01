//! Port of `PhotoReview.Shell.Tests/Startup/DpiAwarenessBootstrapTests.cs` (baseline 7eaee2a2) for `shell_startup::dpi::ensure_per_monitor_v2` (lane cx-testgaps-g19).
//! Fakes only: no process-wide DPI awareness is touched.
#![cfg(windows)]

use std::cell::Cell;

use pr_platform_win::shell_startup::dpi::{DpiApiMissing, DpiAwarenessNative, ensure_per_monitor_v2};

/// C# `FakeDpi`: `V2` = current state, `AllowSet` = whether the OS accepts the change, `Throws` = OS without the API.
struct FakeDpi {
    v2: Cell<bool>,
    allow_set: bool,
    throws: bool,
    set_calls: Cell<u32>,
}

impl FakeDpi {
    fn new(v2: bool, allow_set: bool, throws: bool) -> Self {
        Self {
            v2: Cell::new(v2),
            allow_set,
            throws,
            set_calls: Cell::new(0),
        }
    }
}

impl DpiAwarenessNative for FakeDpi {
    fn is_per_monitor_v2(&self) -> Result<bool, DpiApiMissing> {
        Ok(self.v2.get())
    }

    fn try_set_per_monitor_v2(&self) -> Result<bool, DpiApiMissing> {
        self.set_calls.set(self.set_calls.get() + 1);
        if self.throws {
            return Err(DpiApiMissing);
        }
        if self.allow_set {
            self.v2.set(true);
        }
        Ok(self.allow_set)
    }
}

// C#: DpiAwarenessBootstrapTests.AlreadyPerMonitorV2_FromTheManifest_IsLeftAlone
#[test]
fn already_per_monitor_v2_from_the_manifest_is_left_alone() {
    let dpi = FakeDpi::new(true, true, false);

    assert!(ensure_per_monitor_v2(Some(&dpi)));
    assert_eq!(dpi.set_calls.get(), 0);
}

// C#: DpiAwarenessBootstrapTests.NotPerMonitorV2_IsSetOnce
#[test]
fn not_per_monitor_v2_is_set_once() {
    let dpi = FakeDpi::new(false, true, false);

    assert!(ensure_per_monitor_v2(Some(&dpi)));
    assert_eq!(dpi.set_calls.get(), 1);
}

// C#: DpiAwarenessBootstrapTests.WhenWindowsRefuses_ReportsFalseWithoutThrowing
#[test]
fn when_windows_refuses_reports_false_without_panicking() {
    let dpi = FakeDpi::new(false, false, false);

    assert!(!ensure_per_monitor_v2(Some(&dpi)));
    assert_eq!(dpi.set_calls.get(), 1);
}

// C#: DpiAwarenessBootstrapTests.OnAnOsWithoutTheApi_ReportsFalseWithoutThrowing
#[test]
fn on_an_os_without_the_api_reports_false_without_panicking() {
    let dpi = FakeDpi::new(false, true, true);

    assert!(!ensure_per_monitor_v2(Some(&dpi)));
}
