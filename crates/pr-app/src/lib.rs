//! Port target: `PhotoReview.App` (composition root, windows, view models). UI framework: docs/adr/R0001.

/// Phase 0 UI spike (docs/adr/R0001): Win32 window + Direct2D viewer used to measure switch time.
#[cfg(all(windows, feature = "ui-spike"))]
pub mod spike;
