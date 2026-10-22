//! W8-09b slice S2: `physical_memory` (`GlobalMemoryStatusEx` as `PhysicalMemorySource` / `MemoryInfoSource`).
//!
//! C# twins: none (`RamBudgetPolicy.GetPhysicalMemoryBytes` and `MemoryHeadroom.ReadGcMemoryInfo` read `GC.GetGCMemoryInfo`); every test is `// new:`.
#![cfg(windows)]

use std::sync::mpsc;
use std::time::Duration;

use pr_imaging::memory_headroom::MemoryInfoSource;
use pr_imaging::ram_budget::PhysicalMemorySource;
use pr_platform_win::physical_memory::{WindowsPhysicalMemory, memory_info_from_status, saturating_i64};

const GIB: u64 = 1024 * 1024 * 1024;

// new: load = total - available, total passes through.
#[test]
fn memory_info_is_total_and_used_bytes() {
    let info = memory_info_from_status(16 * GIB, 4 * GIB);
    assert_eq!(info.total_available, (16 * GIB) as i64);
    assert_eq!(info.load, (12 * GIB) as i64);
    let all_free = memory_info_from_status(8 * GIB, 8 * GIB);
    assert_eq!(all_free.load, 0);
}

// new: a status where available exceeds total (never reported by Windows) must not wrap to a huge or negative load.
#[test]
fn memory_info_load_never_goes_negative() {
    let info = memory_info_from_status(GIB, 2 * GIB);
    assert_eq!(info.total_available, GIB as i64);
    assert_eq!(info.load, 0);
}

// new: byte counts above i64::MAX saturate instead of turning negative ("unknown").
#[test]
fn saturating_i64_clamps_at_the_maximum() {
    assert_eq!(saturating_i64(0), 0);
    assert_eq!(saturating_i64(i64::MAX as u64), i64::MAX);
    assert_eq!(saturating_i64(i64::MAX as u64 + 1), i64::MAX);
    assert_eq!(saturating_i64(u64::MAX), i64::MAX);
    assert_eq!(memory_info_from_status(u64::MAX, 0).total_available, i64::MAX);
}

// new: smoke test of the real call: plausible numbers, both traits agree on the total.
#[test]
#[ignore = "native: GlobalMemoryStatusEx reads live system memory"]
fn live_memory_status_is_plausible() {
    let (sender, receiver) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let memory = WindowsPhysicalMemory;
        let total = memory.total_available_memory_bytes();
        let info = memory.read();
        let _ = sender.send((total, info));
    });
    let (total, info) = receiver.recv_timeout(Duration::from_secs(30)).expect("bounded");
    handle.join().expect("thread");
    assert!(total > 0);
    assert_eq!(info.total_available, total);
    assert!(info.load >= 0 && info.load <= info.total_available, "{info:?}");
}
