//! Pixel measurement shared by dialog edit centering tests.
//!
//! Native window creation and pixel sampling are intentionally exercised only by
//! the desktop test harnesses that own the concrete dialog and font setup.

/// Returns the absolute difference between two measured vertical gaps.
pub fn gap_difference(top_gap: i32, bottom_gap: i32) -> i32 {
    top_gap.abs_diff(bottom_gap) as i32
}
