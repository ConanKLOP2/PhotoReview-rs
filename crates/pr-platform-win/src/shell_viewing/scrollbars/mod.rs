//! The self-drawn scroll bars of the main window: `PhotoReview.Shell.Win32/Viewing/ScrollBars.cs` (490 lines, WP-26 NE-3 a, baseline `d92d6aae`), split
//! over four files by responsibility (contract decision D-W8B4V-6). W8-16f owns `state` and `layout`, W8-16g owns `input` and `paint`. Lead-owned index.
//!
//! One type, [`ShellScrollBars`] (C# `ShellScrollBars`), declared with ALL its fields in `state.rs`; the other files add `impl` blocks. The pure
//! geometry is `pr_app::viewing::scrollbar_geometry` (W8-16e), the two OS calls are `shell_viewing::scrollbar_native` (W8-16e).

pub mod input;
pub mod layout;
pub mod paint;
pub mod state;

pub use state::{ScrollBarColors, ScrollBarOptions, ShellScrollBars};
