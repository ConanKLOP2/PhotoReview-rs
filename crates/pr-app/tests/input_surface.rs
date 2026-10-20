//! Port-shape tests for `PhotoReview.App.Shared/Input/IImageSurface.cs` (unit W8-05a, contract `contracts/W8-05a.md`).
//!
//! No C# test of `IImageSurface` itself is ported here: the consumers (`PointerInputController`, `KineticPan`) are W8-05f/g/h.
//! The fake below implements the whole trait and is reused by W8-05g/h, so its behaviour is pinned here.
//!
//! The fake only exercises its own code, so these tests pass on the stub. They are not red.

use pr_app::input::primitives::{PointD, SizeD};
use pr_app::input::surface::{ImageSurface, RenderFrameHandler, RenderHookId};
use pr_core::abstractions::DisplayTiming;

/// Fixed-value fake of [`ImageSurface`]. Stores only what the tests check: the drag thresholds and the render hooks.
#[derive(Default)]
struct FakeImageSurface {
    drag: (f64, f64),
    hooks: Vec<(RenderHookId, RenderFrameHandler)>,
    next_hook: u64,
}

impl ImageSurface for FakeImageSurface {
    fn is_loaded(&self) -> bool {
        true
    }

    fn horizontal_offset(&self) -> f64 {
        0.0
    }

    fn vertical_offset(&self) -> f64 {
        0.0
    }

    fn viewport_width(&self) -> f64 {
        100.0
    }

    fn viewport_height(&self) -> f64 {
        80.0
    }

    fn extent_width(&self) -> f64 {
        200.0
    }

    fn extent_height(&self) -> f64 {
        160.0
    }

    fn drag_threshold(&self) -> (f64, f64) {
        self.drag
    }

    fn scroll_to(&mut self, _horizontal: f64, _vertical: f64) {}

    fn update_layout(&mut self) {}

    fn yield_to_render(&mut self, _continuation: Box<dyn FnOnce()>) {}

    fn image_origin(&self) -> PointD {
        PointD { x: 0.0, y: 0.0 }
    }

    fn to_image_element(&self, surface_point: PointD) -> PointD {
        surface_point
    }

    fn image_actual_width(&self) -> f64 {
        200.0
    }

    fn image_actual_height(&self) -> f64 {
        160.0
    }

    fn source_size(&self) -> Option<SizeD> {
        Some(SizeD {
            width: 200.0,
            height: 160.0,
        })
    }

    fn capture_mouse(&mut self) {}

    fn release_mouse_capture(&mut self) {}

    fn set_pan_cursor(&mut self, _panning: bool) {}

    fn hook_render_frame(&mut self, handler: RenderFrameHandler) -> RenderHookId {
        self.next_hook += 1;
        let id = RenderHookId(self.next_hook);
        self.hooks.push((id, handler));
        id
    }

    fn unhook_render_frame(&mut self, id: RenderHookId) {
        self.hooks.retain(|(existing, _)| *existing != id);
    }

    fn timestamp(&self) -> i64 {
        0
    }

    fn display_timing(&mut self) -> Option<DisplayTiming> {
        None
    }

    fn pointer_position(&self) -> Option<PointD> {
        None
    }
}

// new: `ImageSurface` is object safe (the pointer controller holds `Box<dyn ImageSurface>`). Drag thresholds come back
// as (horizontal, vertical). Render hooks: distinct ids, unhooking an unknown id is a no-op, a known id is removed.
#[test]
fn image_surface_is_object_safe() {
    let fake = FakeImageSurface {
        drag: (4.0, 6.0),
        ..FakeImageSurface::default()
    };
    let boxed: Box<dyn ImageSurface> = Box::new(fake);
    assert_eq!(
        boxed.drag_threshold(),
        (4.0, 6.0),
        "(horizontal, vertical) in that order"
    );

    let mut fake = FakeImageSurface::default();
    let first;
    let second;
    {
        let surface: &mut dyn ImageSurface = &mut fake;
        surface.scroll_to(10.0, 20.0);
        surface.update_layout();
        first = surface.hook_render_frame(Box::new(|_frame_time: Option<i64>| {}));
        second = surface.hook_render_frame(Box::new(|_frame_time: Option<i64>| {}));
        assert_ne!(first, second, "distinct hooks get distinct ids");

        surface.unhook_render_frame(RenderHookId(u64::MAX)); // unknown id: no-op, must not panic
        surface.unhook_render_frame(first);
    }
    assert_eq!(fake.hooks.len(), 1, "only the second hook remains");
    assert_eq!(fake.hooks[0].0, second);
}
