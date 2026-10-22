//! Thin executable package above `pr-app` and `pr-platform-win` (contracts/README.md D-W8B1-1): `pr-platform-win` depends on
//! `pr-app` (it implements the `pr_app::services::render` traits and the host traits), so everything that names both lives here.
//! Port target: the composition root of `PhotoReview.App` (W8-09a `composition`, later W8-09b/W8-17/W8-20).

pub mod composition;

/// Wave 8 batch B6 composition root and production dialog adapter (W8-17c; 17d/17e append theirs): see `startup/mod.rs`.
#[cfg(windows)]
pub mod startup;

/// Phase 0 UI spike (docs/adr/R0001): Win32 window + Direct2D viewer used to measure switch time.
#[cfg(all(windows, feature = "ui-spike"))]
pub mod spike;

/// Wave 8 batch B6 process entry point (W8-20a): the Rust `Program.cs`. `main.rs` is a three-line call into it (`contracts/W8-20a.md`).
#[cfg(windows)]
pub mod entry;
