//! Port of `PhotoReview.Platform.Windows/WindowsMemoryProbe.cs`.
//! Reads system-wide physical memory with GlobalMemoryStatusEx and fails closed on API errors.

use pr_core::abstractions::{Log, MemoryProbe, MemorySnapshot, NullLog};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// Windows implementation of the shared RAM headroom probe.
#[derive(Clone)]
pub struct WindowsMemoryProbe {
    log: std::sync::Arc<dyn Log>,
}

impl std::fmt::Debug for WindowsMemoryProbe {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("WindowsMemoryProbe").finish_non_exhaustive()
    }
}

impl Default for WindowsMemoryProbe {
    fn default() -> Self {
        Self::new(std::sync::Arc::new(NullLog))
    }
}

impl WindowsMemoryProbe {
    /// Creates a probe using the supplied logger.
    pub fn new(log: std::sync::Arc<dyn Log>) -> Self {
        Self { log }
    }
}

impl MemoryProbe for WindowsMemoryProbe {
    fn has_headroom(&self, maximum_load: f64, reserve_bytes: i64) -> bool {
        let Some(snapshot) = self.get_snapshot() else {
            return false;
        };
        f64::from(snapshot.load_percent) < maximum_load * 100.0
            && snapshot.available_bytes >= reserve_bytes.max(0) as u64
    }

    fn get_snapshot(&self) -> Option<MemorySnapshot> {
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: `status` is a writable, correctly sized MEMORYSTATUSEX initialized above as required by the API.
        match unsafe { GlobalMemoryStatusEx(&mut status) } {
            Ok(()) => Some(MemorySnapshot {
                load_percent: status.dwMemoryLoad,
                available_bytes: status.ullAvailPhys,
            }),
            Err(error) => {
                self.log.error(
                    &format!(
                        "GlobalMemoryStatusEx failed: Win32Error={}",
                        (error.code().0 as u32) & 0xFFFF
                    ),
                    Some(&error),
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "native: GlobalMemoryStatusEx reads live system memory"]
    fn snapshot_has_plausible_physical_memory_values() {
        // C# twins: PlatformPrimitivesTests.WindowsMemoryProbeReportsRealisticMetrics and
        // LibraryImportInteropTests.MemoryProbe_ReturnsAPlausibleSnapshot.
        let probe = WindowsMemoryProbe::default();
        let snapshot = probe.get_snapshot().expect("GlobalMemoryStatusEx succeeds");
        assert!(snapshot.load_percent <= 100);
        assert!(snapshot.available_bytes > 0);
        assert!(probe.has_headroom(1.0, 0));
    }
}
