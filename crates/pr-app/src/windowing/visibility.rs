//! "Is a saved placement still usable": the rect must share at least 80x80 px with a monitor work area.
//!
//! Port of `PhotoReview.App.Shared/Windowing/WindowPlacementVisibility.cs` (baseline d92d6aae). Pure. Contract:
//! `contracts/W8-03a.md`.

#![allow(unused_variables, dead_code)] // D11: remove with the last `todo!()`.

use super::monitor_layout::ScreenRect;

/// Smallest width AND height (physical px) of the part of a placement that must lie inside one work area.
pub const MIN_VISIBLE_SIDE: i32 = 80;

/// True when the rectangle is not empty (`right > left` and `bottom > top`) and intersects at least one work area in a region of
/// at least [`MIN_VISIBLE_SIDE`] by [`MIN_VISIBLE_SIDE`] pixels. The parts that lie in different work areas are never added up.
///
/// All arithmetic reproduces C# `System.Drawing.Rectangle` in an `unchecked` context (wrapping `i32`), see the contract
/// (note 7): the C# code builds `Rectangle.FromLTRB`, calls `Rectangle.Intersect` and reads `Width`/`Height`.
pub fn is_visible(left: i32, top: i32, right: i32, bottom: i32, work_areas: &[ScreenRect]) -> bool {
    todo!()
}
