//! Physical memory sources over `GlobalMemoryStatusEx` (W8-09b slice S2, `contracts/W8-09b.md`, decision D-W8B6-151).
//!
//! Two C# static helpers have no Rust production twin yet:
//! - `RamBudgetPolicy.GetPhysicalMemoryBytes()` = `GC.GetGCMemoryInfo().TotalAvailableMemoryBytes`: Rust seam `pr_imaging::ram_budget::PhysicalMemorySource`;
//! - `MemoryHeadroom.ReadGcMemoryInfo()` = `(TotalAvailableMemoryBytes, MemoryLoadBytes)` of the decoders: Rust seam `pr_imaging::memory_headroom::MemoryInfoSource`.
//!
//! `GC.GetGCMemoryInfo` reports the physical RAM (or the container limit, none on a desktop) and the memory load in bytes; the Win32 equivalent is `GlobalMemoryStatusEx`
//! (`ullTotalPhys`, and `ullTotalPhys - ullAvailPhys`). Same API as `WindowsMemoryProbe` (W6-05); a failed call answers "unknown" (`0` total, `0` load), which both consumers
//! treat as "refuse nothing" / "no clamp" (`<= 0` means unknown in `ram_budget` and `memory_headroom`).
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::ram_budget::PhysicalMemorySource;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// Stateless; `Send + Sync`; one `GlobalMemoryStatusEx` call per query (the decoders ask only after the cheap size threshold passed).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowsPhysicalMemory;

/// `(ullTotalPhys, ullAvailPhys)` of the machine, or `None` when `GlobalMemoryStatusEx` fails.
fn read_status() -> Option<(u64, u64)> {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `status` is a live, writable MEMORYSTATUSEX whose `dwLength` was set to its size as the API requires; the call writes only inside it.
    match unsafe { GlobalMemoryStatusEx(&mut status) } {
        Ok(()) => Some((status.ullTotalPhys, status.ullAvailPhys)),
        Err(_) => None,
    }
}

/// `u64` to `i64`, saturating at `i64::MAX` (a byte count above 8 EiB cannot occur, but the cast must never wrap negative: negative means "unknown").
pub fn saturating_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// `MemoryInfo { total_available: total, load: total - available }` from the two `MEMORYSTATUSEX` fields; the load never goes negative.
pub fn memory_info_from_status(total_phys: u64, avail_phys: u64) -> MemoryInfo {
    MemoryInfo {
        total_available: saturating_i64(total_phys),
        load: saturating_i64(total_phys.saturating_sub(avail_phys)),
    }
}

impl PhysicalMemorySource for WindowsPhysicalMemory {
    /// `ullTotalPhys` as `i64` (saturating); `0` when the call fails.
    fn total_available_memory_bytes(&self) -> i64 {
        read_status().map_or(0, |(total, _)| saturating_i64(total))
    }
}

impl MemoryInfoSource for WindowsPhysicalMemory {
    /// `MemoryInfo { total_available: ullTotalPhys, load: ullTotalPhys - ullAvailPhys }`; both `0` when the call fails.
    fn read(&self) -> MemoryInfo {
        read_status().map_or(
            MemoryInfo {
                total_available: 0,
                load: 0,
            },
            |(total, available)| memory_info_from_status(total, available),
        )
    }
}
