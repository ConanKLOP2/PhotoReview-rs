//! `ShellScrollBars` part 2 (W8-16f, contract `contracts/W8-16f.md`): the read-only geometry queries of `ScrollBars.cs` (lines 104-142 `CurrentLayout`,
//! `HitTest`, `HoveredThumbAxis`; 174-178 `ToDip`; 327-332 `IsOverPageRegion`). Nothing here changes state or touches the window except for reading it.
//! Baseline `d92d6aae`.

use pr_app::input::primitives::{PointD, SizeD};
use pr_app::viewing::scrollbar_geometry::{self, ScrollAxis, ScrollBarLayout, ScrollBarPart};

use super::state::ShellScrollBars;
use crate::shell_viewing::compare;
use crate::shell_viewing::viewport_controller::normalized_dpi;

impl ShellScrollBars {
    /// The client size in DIP as a `pr_app` `SizeD` (the window API returns the interim host twin, D-W8B3H-1). `None` when the window is gone.
    pub(super) fn client_size(&self) -> Option<SizeD> {
        let window = self.window.upgrade()?;
        let size = window.client_size_dip();
        Some(SizeD {
            width: size.width,
            height: size.height,
        })
    }

    /// C# `CurrentLayout`: `scrollbar_geometry::compute(client, &viewport.layout(), viewport.horizontal_offset(), viewport.vertical_offset(), &metrics)`;
    /// `ScrollBarLayout::none()` when the window is gone.
    pub fn current_layout(&self) -> ScrollBarLayout {
        let Some(client) = self.client_size() else {
            return ScrollBarLayout::none();
        };
        scrollbar_geometry::compute(
            client,
            &self.viewport.layout(),
            self.viewport.horizontal_offset(),
            self.viewport.vertical_offset(),
            &self.options.metrics,
        )
    }

    /// C# `HitTest(point)` on `current_layout()`: the part of the bars visible at the client DIP point.
    pub fn hit_test(&self, point: PointD) -> ScrollBarPart {
        let layout = self.current_layout();
        self.hit_test_in(&layout, point)
    }

    /// C# private `HitTest(in layout, point)`: while the compare panel is visible and `scrollbar_geometry::inside(compare::layout::panel(client), point)`
    /// holds, the panel (8 DIP margin, canvas background) covers the bars, so the answer is `ScrollBarPart::None`; otherwise `scrollbar_geometry::hit_test`.
    pub(super) fn hit_test_in(&self, layout: &ScrollBarLayout, point: PointD) -> ScrollBarPart {
        if (self.is_compare_visible)() {
            if let Some(client) = self.client_size() {
                if scrollbar_geometry::inside(compare::layout::panel(client), point) {
                    return ScrollBarPart::None;
                }
            }
        }
        scrollbar_geometry::hit_test(layout, point)
    }

    /// C# `HoveredThumbAxis`: the axis whose thumb the pointer is on; `None` while dragging or paging, when the pointer is outside the window, or when
    /// `hit_test_in(current_layout, pointer)` is not `Thumb`.
    pub fn hovered_thumb_axis(&self) -> Option<ScrollAxis> {
        if self.dragging.get() || self.paging.get() || !self.pointer_inside.get() {
            return None;
        }
        let layout = self.current_layout();
        let pointer = self.pointer.get();
        if self.hit_test_in(&layout, pointer) != ScrollBarPart::Thumb {
            return None;
        }
        scrollbar_geometry::axis_at(&layout, pointer)
    }

    /// Test seam (no C# twin, W8-16f D-W8B4V-6): sets the remembered pointer (client DIP) and the "pointer inside the window" flag exactly as `on_move`
    /// does, so the hover query is testable before `input.rs` (W8-16g) is ported. Not used by production code.
    #[doc(hidden)]
    pub fn set_pointer_for_tests(&self, point: PointD, inside: bool) {
        self.pointer.set(point);
        self.pointer_inside.set(inside);
    }

    /// C# `ToDip(pixelX, pixelY)`: client pixels to DIP, divided by `viewport_controller::normalized_dpi(window.dpi_scale())`.
    // Used by `input.rs` (W8-16g); that unit removes this allow.
    #[allow(dead_code)]
    pub(super) fn to_dip(&self, pixel_x: i32, pixel_y: i32) -> PointD {
        let Some(window) = self.window.upgrade() else {
            return PointD { x: 0.0, y: 0.0 };
        };
        let scale = normalized_dpi(window.dpi_scale());
        PointD {
            x: f64::from(pixel_x) / scale,
            y: f64::from(pixel_y) / scale,
        }
    }

    /// C# `IsOverPageRegion(layout, point)`: the point is on the track part the held page button belongs to (`TrackBefore` for direction -1, else
    /// `TrackAfter`) AND on the axis being paged.
    // Used by `input.rs` (W8-16g); that unit removes this allow.
    #[allow(dead_code)]
    pub(super) fn is_over_page_region(&self, layout: &ScrollBarLayout, point: PointD) -> bool {
        let wanted = if self.page_direction.get() < 0 {
            ScrollBarPart::TrackBefore
        } else {
            ScrollBarPart::TrackAfter
        };
        self.hit_test_in(layout, point) == wanted
            && scrollbar_geometry::axis_at(layout, point) == Some(self.page_axis.get())
    }
}
