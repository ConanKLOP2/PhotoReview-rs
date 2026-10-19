//! Port target: `PhotoReview.App` (composition root, windows, view models). UI framework: docs/adr/R0001.

/// Phase 0 UI spike (docs/adr/R0001): Win32 window + Direct2D viewer used to measure switch time.
#[cfg(all(windows, feature = "ui-spike"))]
pub mod spike;

// Wave 8 batch B1 modules (contract stubs, lead-owned wiring; contracts/README.md D-W8B1-*).
pub mod composition;
pub mod coordinators;
pub mod input;
pub mod localization;
pub mod menus;
pub mod misc;
pub mod overlay;
pub mod presenters;
pub mod services;
pub mod viewing;
pub mod viewmodels;
pub mod viewport;
pub mod windowing;
