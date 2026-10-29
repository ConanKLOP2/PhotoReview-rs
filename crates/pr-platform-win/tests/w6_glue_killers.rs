//! Remaining seam-observable W6 glue mutants; C# oracle: 7eaee2a2.
#![cfg(windows)]

#[path = "../src/display_clock.rs"]
mod display_clock;
#[allow(dead_code)] // Source inclusion provides the existing fake-driver seam, not native adapter coverage.
#[path = "../src/vblank_driver.rs"]
mod vblank_driver;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
};
use std::time::Duration;

use display_clock::WindowsDisplayClock;
use pr_core::abstractions::{DisplayClock, DisplayTiming};
use pr_platform_win::recycle_bin_settings::{
    RecycleBinSettingsSource, RegistryHive, RegistryValue, WindowsRecycleBinSettingsSource,
};
use vblank_driver::{IVBlankDriver, NativeImportError};

#[test]
fn settings_debug_names_the_adapter() {
    // Mutant recycle_bin_settings.rs:74:9: Debug::fmt -> Ok(Default::default()).
    let source = WindowsRecycleBinSettingsSource::with_reader(|_, _, _| panic!("Debug must not read registry"));
    assert_eq!(format!("{source:?}"), "WindowsRecycleBinSettingsSource");
}

#[test]
fn settings_with_reader_preserves_the_injected_registry_reader() {
    // Mutant recycle_bin_settings.rs:92:9: with_reader -> Default::default() (recursive construction).
    // C# RecycleBinCapacity policy: prefer the stricter user/machine quota; no real registry access.
    let source = WindowsRecycleBinSettingsSource::with_reader(|hive, key, name| {
        assert!(key.ends_with("Policies\\Explorer") || key.ends_with("Explorer\\BitBucket"));
        Ok(match (hive, name) {
            (RegistryHive::CurrentUser, "RecycleBinSize") => Some(RegistryValue::Dword(17)),
            (RegistryHive::LocalMachine, "RecycleBinSize") => Some(RegistryValue::Dword(9)),
            (RegistryHive::CurrentUser, "NoRecycleFiles") => Some(RegistryValue::Dword(1)),
            (_, "NoRecycleFiles" | "NukeOnDelete") => None,
            _ => panic!("unexpected read: {hive:?} {key} {name}"),
        })
    });
    let policy = source.read_policy().expect("injected DWORDs");
    assert!(policy.no_recycle_files);
    assert_eq!(policy.max_size_percent, Some(9));
    assert!(!policy.legacy_global_nuke_on_delete);
}

#[test]
fn display_clock_debug_names_the_clock() {
    // Mutant display_clock.rs:64:9: Debug::fmt -> Ok(Default::default()).
    let clock = WindowsDisplayClock::new();
    assert_eq!(format!("{clock:?}"), "WindowsDisplayClock { .. }");
}

#[test]
fn display_clock_instance_reuses_the_process_singleton() {
    // Mutant display_clock.rs:82:9: instance -> Box::leak(Box::new(Default::default())).
    // C# WindowsDisplayClock.Instance; constructing the clock does not start its worker.
    assert!(std::ptr::eq(
        WindowsDisplayClock::instance(),
        WindowsDisplayClock::instance()
    ));
}

#[test]
fn native_import_error_preserves_exception_type_and_loader_message() {
    // Mutants vblank_driver.rs:29:9,36:9: type_name/message -> empty or xyzzy.
    for (error, kind, message) in [
        (
            NativeImportError::dll_not_found("missing library"),
            "DllNotFoundException",
            "missing library",
        ),
        (
            NativeImportError::entry_point_not_found("missing function"),
            "EntryPointNotFoundException",
            "missing function",
        ),
    ] {
        assert_eq!(error.type_name(), kind);
        assert_eq!(error.message(), message);
    }
}

struct RetargetDriver {
    waiting: Sender<()>,
    release: Mutex<Receiver<()>>,
    panic_on_close: AtomicBool,
}

impl IVBlankDriver for RetargetDriver {
    fn monitor_from_window(&self, window: isize) -> isize {
        window
    }

    fn try_open(&self, monitor: isize) -> Result<Option<(u32, u32)>, NativeImportError> {
        Ok(Some((monitor as u32, 7)))
    }

    fn wait_for_vblank(&self, _: u32, _: u32) -> Result<i32, NativeImportError> {
        self.waiting.send(()).expect("test owns event receiver");
        // Timeout is only a hang guard; scheduling and the clock never select the outcome.
        self.release
            .lock()
            .expect("release gate")
            .recv_timeout(Duration::from_secs(10))
            .expect("test releases worker");
        Ok(0)
    }

    fn close(&self, _: u32) {
        assert!(
            !self.panic_on_close.swap(false, Ordering::AcqRel),
            "injected close failure"
        );
    }

    fn dwm_timing(&self) -> Option<DisplayTiming> {
        Some(DisplayTiming {
            last_vblank: 123,
            refresh_period: 456,
        })
    }
}

#[test]
fn clock_retarget_close_failure_blames_the_opened_monitor() {
    // Mutant display_clock.rs:308:33: opened != 0 -> == 0 in worker_main.
    // C# WindowsDisplayClock.Run catch(Exception): opened != Zero ? opened : target.
    let (waiting_tx, waiting_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let driver = Arc::new(RetargetDriver {
        waiting: waiting_tx,
        release: Mutex::new(release_rx),
        panic_on_close: AtomicBool::new(true),
    });
    let clock =
        WindowsDisplayClock::with_driver(None, Some(driver), Some(Arc::new(|| 1_000_000))).expect("fake dependencies");
    assert_eq!(clock.get_timing(0xA1), None);
    waiting_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("first adapter waiting");
    assert_eq!(clock.get_timing(0xB1), None);
    release_tx.send(()).expect("release old monitor wait");
    // This waits for a state transition, never for a wall-clock-derived timing estimate.
    clock.await_failed_monitor(0xA1);
    clock.shutdown_and_join();
    assert_eq!(
        clock.get_timing(0xA1),
        Some(DisplayTiming {
            last_vblank: 123,
            refresh_period: 456
        })
    );
}
