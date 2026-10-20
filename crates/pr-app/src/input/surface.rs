//! Port of `PhotoReview.App.Shared/Input/IImageSurface.cs` (AR13a): what the pointer controller needs from the image view.
//!
//! Points are in ImageScroll coordinates (DIP). The Win32 adapter is `pr_platform_win::shell_input::surface` (W8-14c); tests
//! use a fake that implements this trait. Baseline `d92d6aae`.
//!
//! Decisions for this file (see the W8-05a contract, "Semantics"):
//! - D-W8-2: C# `Task YieldToRenderAsync()` becomes `yield_to_render(continuation)`. No async runtime.
//! - `RenderingTime(EventArgs)` is not a member. The platform extracts it and passes it to the render handler.
//! - Handler identity (C# `EventHandler` reference) becomes a [`RenderHookId`].

use pr_core::abstractions::DisplayTiming;

use crate::input::primitives::{PointD, SizeD};

/// Opaque handle of one render-frame hook (C# `CompositionTarget.Rendering += h`). Used to unhook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderHookId(pub u64);

/// Per-frame render callback. Its argument is the frame time from the render event's args, in `TimeSpan` ticks
/// (100 ns, C# `TimeSpan.Ticks`), or `None` when the args carry no frame time (C# `RenderingTime` returned `null`).
pub type RenderFrameHandler = Box<dyn FnMut(Option<i64>)>;

/// `IImageSurface` (C-05 / AR13a). Rust name without the `I` prefix.
pub trait ImageSurface {
    /// The window is loaded. A glide frame or a zoom continuation after close does nothing.
    fn is_loaded(&self) -> bool;

    fn horizontal_offset(&self) -> f64;
    fn vertical_offset(&self) -> f64;
    fn viewport_width(&self) -> f64;
    fn viewport_height(&self) -> f64;
    fn extent_width(&self) -> f64;
    fn extent_height(&self) -> f64;

    /// `SystemParameters.MinimumHorizontalDragDistance` / `MinimumVerticalDragDistance`, as `(horizontal, vertical)`.
    fn drag_threshold(&self) -> (f64, f64);

    /// C# `ScrollToHorizontalOffset(horizontal)` then `ScrollToVerticalOffset(vertical)`. Two calls, in this order.
    fn scroll_to(&mut self, horizontal: f64, vertical: f64);

    /// C# `ImageScroll.UpdateLayout()`.
    fn update_layout(&mut self);

    /// Replaces C# `Task YieldToRenderAsync()`: runs `continuation` once the dispatcher has drained its queue down to
    /// Render priority (D-W8-2). The continuation runs on the UI thread, never inline.
    fn yield_to_render(&mut self, continuation: Box<dyn FnOnce()>);

    /// The image element's top-left corner in ImageScroll coordinates.
    fn image_origin(&self) -> PointD;

    /// C# `ToImageElement(surfacePoint)`: ImageScroll point to image-element coordinates.
    fn to_image_element(&self, surface_point: PointD) -> PointD;

    /// `ImageActualWidth` / `ImageActualHeight` of the image element.
    fn image_actual_width(&self) -> f64;
    fn image_actual_height(&self) -> f64;

    /// The displayed bitmap's size (`ImageSource.Width` / `Height`), or `None` without one (C# `(double, double)?`).
    fn source_size(&self) -> Option<SizeD>;

    /// The image element captures the mouse.
    fn capture_mouse(&mut self);

    /// Releases the capture if, and only if, the image element holds it.
    fn release_mouse_capture(&mut self);

    /// The pan cursor while a pannable press is tracked (`panning == true`), the arrow otherwise.
    fn set_pan_cursor(&mut self, panning: bool);

    /// C# `CompositionTarget.Rendering += handler`. Returns the id needed by [`ImageSurface::unhook_render_frame`].
    fn hook_render_frame(&mut self, handler: RenderFrameHandler) -> RenderHookId;

    /// C# `CompositionTarget.Rendering -= handler`. An unknown id is a no-op.
    fn unhook_render_frame(&mut self, id: RenderHookId);

    /// Now, in Stopwatch (QPC) ticks: the time base of [`DisplayTiming`].
    fn timestamp(&self) -> i64;

    /// Vblank timing of the monitor showing the image, or `None` while unknown. Non-blocking. Reading it starts the clock,
    /// so this takes `&mut self` (C# `IDisplayClock` behaviour).
    fn display_timing(&mut self) -> Option<DisplayTiming>;

    /// The mouse position in ImageScroll coordinates, or `None` when it is outside the viewport or not available
    /// (window not active). Anchors a keyboard or menu zoom at the cursor.
    fn pointer_position(&self) -> Option<PointD>;
}
