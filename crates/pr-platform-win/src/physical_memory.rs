//! Physical memory sources over `GlobalMemoryStatusEx` (W8-09b slice S2, `contracts/W8-09b.md`, decision D-W8B6-151).
//!
//! Two C# static helpers have no Rust production twin yet:
//! - `RamBudgetPolicy.GetPhysicalMemoryBytes()` = `GC.GetGCMemoryInfo().TotalAvailableMemoryBytes`: Rust seam `pr_imaging::ram_budget::PhysicalMemorySource`;
//! - `MemoryHeadroom.ReadGcMemoryInfo()` = `(TotalAvailableMemoryBytes, MemoryLoadBytes)` of the decoders: Rust seam `pr_imaging::memory_headroom::MemoryInfoSource`.
//!
//! `GC.GetGCMemoryInfo` reports the physical RAM (or the container limit, none on a desktop) and the memory load in bytes; the Win32 equivalent is `GlobalMemoryStatusEx`
//! (`ullTotalPhys`, and `ullTotalPhys - ullAvailPhys`). Same API as `WindowsMemoryProbe` (W6-05); a failed call answers "unknown" (`0` total, `0` load), which both consumers
//! treat as "refuse nothing" / "no clamp" (`<= 0` means unknown in `ram_budget` and `memory_headroom`).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::ram_budget::PhysicalMemorySource;

/// Stateless; `Send + Sync`; one `GlobalMemoryStatusEx` call per query (the decoders ask only after the cheap size threshold passed).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowsPhysicalMemory;

impl PhysicalMemorySource for WindowsPhysicalMemory {
    /// `ullTotalPhys` as `i64` (saturating); `0` when the call fails.
    fn total_available_memory_bytes(&self) -> i64 {
        todo!()
    }
}

impl MemoryInfoSource for WindowsPhysicalMemory {
    /// `MemoryInfo { total_available: ullTotalPhys, load: ullTotalPhys - ullAvailPhys }`; both `0` when the call fails.
    fn read(&self) -> MemoryInfo {
        todo!()
    }
}
