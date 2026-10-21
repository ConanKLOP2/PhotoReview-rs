//! Port of `PhotoReview.Shell.Win32/Input/ViewportImageSurface.cs` (87 lines, baseline `d92d6aae`). Unit W8-14c. Contract: `contracts/W8-14c.md`.
//!
//! WP-22: `IImageSurface` + `IFitSurface` of the viewport controller (W8-16h). A thin forwarder plus `Capture()`, the snapshot the Fit loop compares
//! between two passes (T89). The WPF `WpfImageSurface` (135 lines) is retired.
//!
//! The viewport controller is taken as the trait [`ViewportControllerApi`] (its members are the C# `ViewportController` members this adapter
//! forwards); W8-16h implements it for its controller, tests implement it with a fake (D-W8B4I-6). The surface never holds a `ViewerHandle` borrow
//! across a call into the controller: `capture` borrows the viewer immutably for the duration of reading five numbers.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::fmt;
use std::rc::Rc;

use pr_app::input::primitives::{PointD, SizeD};
use pr_app::input::surface::{ImageSurface, RenderFrameHandler, RenderHookId};
use pr_app::viewmodels::main::ViewerHandle;
use pr_app::viewport::fit::FitSurface;
use pr_app::viewport::types::{ViewportLayout, ViewportSnapshot};
use pr_core::abstractions::DisplayTiming;

use super::dispatch::{FrameRouter, InputSurface};

/// The members of the C# `ViewportController` that `ViewportImageSurface` forwards (`Viewing/ViewportController.cs` lines 92-262). `&self`: the
/// controller is shared with the main window and re-entered by the window procedure.
pub trait ViewportControllerApi {
    /// C# `Layout` (extent, viewport and bar visibility).
    fn layout(&self) -> ViewportLayout;
    /// C# `ViewportSize` (DIP).
    fn viewport_size(&self) -> (f64, f64);
    /// C# `IsLoaded`.
    fn is_loaded(&self) -> bool;
    /// C# `UpdateLayout()`.
    fn update_layout(&self);
    /// C# `UpdateFitSize()`: pushes the viewport size to the viewer and the decode box (borrows the viewer: the caller must hold none).
    fn update_fit_size(&self);
    /// C# `ScrollHome()`.
    fn scroll_home(&self);
    /// C# `InvalidateLayout()` (`RefreshView`).
    fn invalidate_layout(&self);
    /// C# `HorizontalOffset`.
    fn horizontal_offset(&self) -> f64;
    /// C# `VerticalOffset`.
    fn vertical_offset(&self) -> f64;
    /// C# `DragThreshold` (DIP).
    fn drag_threshold(&self) -> (f64, f64);
    /// C# `ScrollTo(horizontal, vertical)`.
    fn scroll_to(&self, horizontal: f64, vertical: f64);
    /// C# `ImageOrigin`.
    fn image_origin(&self) -> PointD;
    /// C# `ToImageElement(point)`.
    fn to_image_element(&self, surface_point: PointD) -> PointD;
    /// C# `ImageActualWidth`.
    fn image_actual_width(&self) -> f64;
    /// C# `ImageActualHeight`.
    fn image_actual_height(&self) -> f64;
    /// C# `SourceSize`.
    fn source_size(&self) -> Option<SizeD>;
    /// C# `CaptureMouse()` (window `capture_pointer`).
    fn capture_mouse(&self);
    /// C# `ReleaseMouseCapture()`: releases only when the window holds the capture. It may send `WM_CAPTURECHANGED` synchronously (D-W8B4I-4).
    fn release_mouse_capture(&self);
    /// C# `SetPanCursor(panning)`.
    fn set_pan_cursor(&self, panning: bool);
    /// C# `HookRenderFrame(handler)`: subscribes to the frame clock; the id unhooks.
    fn hook_render_frame(&self, handler: RenderFrameHandler) -> RenderHookId;
    /// C# `UnhookRenderFrame(handler)`.
    fn unhook_render_frame(&self, id: RenderHookId);
    /// C# `Timestamp` (Stopwatch ticks).
    fn timestamp(&self) -> i64;
    /// C# `DisplayTiming` (starts the monitor's vblank clock on read).
    fn display_timing(&self) -> Option<DisplayTiming>;
    /// C# `PointerPosition`.
    fn pointer_position(&self) -> Option<PointD>;
    /// C# `YieldToRenderAsync()` as a continuation: runs `continuation` on the UI thread once the dispatcher drained down to Render priority.
    fn yield_to_render(&self, continuation: Box<dyn FnOnce()>);
}

/// C# `ViewportImageSurface(viewport, viewer)`.
pub struct ViewportImageSurface {
    viewport: Rc<dyn ViewportControllerApi>,
    viewer: ViewerHandle,
    router: Option<FrameRouter>,
}

impl ViewportImageSurface {
    /// C# primary constructor.
    pub fn new(viewport: Rc<dyn ViewportControllerApi>, viewer: ViewerHandle) -> Self {
        Self {
            viewport,
            viewer,
            router: None,
        }
    }
}

impl fmt::Debug for ViewportImageSurface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewportImageSurface").finish_non_exhaustive()
    }
}

impl ImageSurface for ViewportImageSurface {
    fn is_loaded(&self) -> bool {
        todo!()
    }
    fn horizontal_offset(&self) -> f64 {
        todo!()
    }
    fn vertical_offset(&self) -> f64 {
        todo!()
    }
    fn viewport_width(&self) -> f64 {
        todo!()
    }
    fn viewport_height(&self) -> f64 {
        todo!()
    }
    fn extent_width(&self) -> f64 {
        todo!()
    }
    fn extent_height(&self) -> f64 {
        todo!()
    }
    fn drag_threshold(&self) -> (f64, f64) {
        todo!()
    }
    fn scroll_to(&mut self, horizontal: f64, vertical: f64) {
        todo!()
    }
    fn update_layout(&mut self) {
        todo!()
    }
    fn yield_to_render(&mut self, continuation: Box<dyn FnOnce()>) {
        todo!()
    }
    fn image_origin(&self) -> PointD {
        todo!()
    }
    fn to_image_element(&self, surface_point: PointD) -> PointD {
        todo!()
    }
    fn image_actual_width(&self) -> f64 {
        todo!()
    }
    fn image_actual_height(&self) -> f64 {
        todo!()
    }
    fn source_size(&self) -> Option<SizeD> {
        todo!()
    }
    fn capture_mouse(&mut self) {
        todo!()
    }
    fn release_mouse_capture(&mut self) {
        todo!()
    }
    fn set_pan_cursor(&mut self, panning: bool) {
        todo!()
    }
    fn hook_render_frame(&mut self, handler: RenderFrameHandler) -> RenderHookId {
        todo!()
    }
    fn unhook_render_frame(&mut self, id: RenderHookId) {
        todo!()
    }
    fn timestamp(&self) -> i64 {
        todo!()
    }
    fn display_timing(&mut self) -> Option<DisplayTiming> {
        todo!()
    }
    fn pointer_position(&self) -> Option<PointD> {
        todo!()
    }
}

impl FitSurface for ViewportImageSurface {
    fn viewport_size(&self) -> (f64, f64) {
        todo!()
    }
    fn is_loaded(&self) -> bool {
        todo!()
    }
    fn update_layout(&mut self) {
        todo!()
    }
    fn update_fit_size(&mut self) {
        todo!()
    }
    fn capture(&self) -> ViewportSnapshot {
        todo!()
    }
    fn scroll_home(&mut self) {
        todo!()
    }
}

impl InputSurface for ViewportImageSurface {
    fn set_frame_router(&mut self, router: Option<FrameRouter>) {
        todo!()
    }
}
