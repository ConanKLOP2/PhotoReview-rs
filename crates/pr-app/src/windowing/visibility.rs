//! "Is a saved placement still usable": the rect must share at least 80x80 px with a monitor work area.
//!
//! Port of `PhotoReview.App.Shared/Windowing/WindowPlacementVisibility.cs` (baseline d92d6aae). Pure. Contract:
//! `contracts/W8-03a.md`.

use super::monitor_layout::ScreenRect;

/// Smallest width AND height (physical px) of the part of a placement that must lie inside one work area.
pub const MIN_VISIBLE_SIDE: i32 = 80;

/// True when the rectangle is not empty (`right > left` and `bottom > top`) and intersects at least one work area in a region of
/// at least [`MIN_VISIBLE_SIDE`] by [`MIN_VISIBLE_SIDE`] pixels. The parts that lie in different work areas are never added up.
///
/// All arithmetic reproduces C# `System.Drawing.Rectangle` in an `unchecked` context (wrapping `i32`), see the contract
/// (note 7): the C# code builds `Rectangle.FromLTRB`, calls `Rectangle.Intersect` and reads `Width`/`Height`.
pub fn is_visible(left: i32, top: i32, right: i32, bottom: i32, work_areas: &[ScreenRect]) -> bool {
    if right <= left || bottom <= top {
        return false;
    }
    // Rectangle.FromLTRB(left, top, right, bottom): width and height wrap like C# unchecked int arithmetic.
    let aw = right.wrapping_sub(left);
    let ah = bottom.wrapping_sub(top);
    for work in work_areas {
        let (bx, by, bw, bh) = (work.left, work.top, work.width(), work.height());
        // Rectangle.Intersect
        let x1 = left.max(bx);
        let x2 = left.wrapping_add(aw).min(bx.wrapping_add(bw));
        let y1 = top.max(by);
        let y2 = top.wrapping_add(ah).min(by.wrapping_add(bh));
        if x2 >= x1 && y2 >= y1 {
            let iw = x2.wrapping_sub(x1);
            let ih = y2.wrapping_sub(y1);
            if iw >= MIN_VISIBLE_SIDE && ih >= MIN_VISIBLE_SIDE {
                return true;
            }
        }
    }
    false
}
