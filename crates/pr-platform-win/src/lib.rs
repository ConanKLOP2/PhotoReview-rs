//! Port target: `PhotoReview.Platform.Windows` (Explorer order: INV-7/INV-9; Recycle Bin: INV-6).

#[cfg(windows)]
pub mod wic;

/// Test-only Win32 process memory metrics used by native decoder parity tests.
#[cfg(all(windows, feature = "test-support"))]
pub mod test_support;
