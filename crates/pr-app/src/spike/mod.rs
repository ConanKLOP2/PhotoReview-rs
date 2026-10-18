//! Phase 0 UI spike (docs/adr/R0001, P-06). Not a port of C# code; it decides the UI framework.
//!
//! `view` (zoom/pan maths) and `stats` (percentiles) are plain safe code. `viewer` is the Win32 + Direct2D viewer built
//! on `pr_platform_win::spike_ui`, which owns all the `unsafe`.

pub mod stats;
pub mod view;

#[cfg(windows)]
pub mod viewer;
