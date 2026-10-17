//! Port targets: `PhotoReview.Platform.Windows` (Explorer order: INV-7/INV-9; Recycle Bin: INV-6;
//! W6-05: `WindowsMemoryProbe`, `WindowsNaturalComparer`, `WindowsDarkTitleBar`,
//! `WindowsLiveOperationRegistry`).

#[cfg(windows)]
pub mod wic;

#[cfg(windows)]
pub mod dark_title_bar;

#[cfg(windows)]
pub mod live_operation_registry;

#[cfg(windows)]
pub mod memory_probe;

#[cfg(windows)]
pub mod natural_comparer;

#[cfg(windows)]
#[allow(dead_code)] // W6-04a consumes this driver seam before the Wave 6 integration gate.
pub(crate) mod vblank_driver;

/// Test-only Win32 process memory metrics used by native decoder parity tests.
#[cfg(all(windows, feature = "test-support"))]
pub mod test_support;
