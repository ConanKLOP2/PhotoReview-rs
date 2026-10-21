//! Port of `PhotoReview.Shell.Win32/Input/ViewportImageSurface.cs` (87 lines, baseline `d92d6aae`). Unit W8-14c. Contract: `contracts/W8-14c.md`.
//!
//! WP-22: `IImageSurface` + `IFitSurface` of the viewport controller (W8-16h). A thin forwarder plus `Capture()`, the snapshot the Fit loop compares
//! between two passes (T89). The WPF `WpfImageSurface` (135 lines) is retired.
//!
//! The viewport controller is taken as the trait [`ViewportControllerApi`] (its members are the C# `ViewportController` members this adapter
//! forwards); W8-16h implements it for its controller, tests implement it with a fake (D-W8B4I-6). The surface never holds a `ViewerHandle` borrow
//! across a call into the controller: `capture` borrows the viewer immutably for the duration of reading five numbers.
use std::fmt;
use std::rc::Rc;

use pr_app::input::primitives::{PointD, SizeD};
use pr_app::input::surface::{ImageSurface, RenderFrameHandler, RenderHookId};
use pr_app::viewmodels::main::ViewerHandle;
use pr_app::viewport::fit::FitSurface;
use pr_app::viewport::types::{ViewportLayout, ViewportSnapshot};
use pr_core::abstractions::DisplayTiming;

use super::dispatch::{FrameRouter, InputSurface};
use crate::shell_viewing::viewport_controller::ViewportController;

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
        self.viewport.is_loaded()
    }
    fn horizontal_offset(&self) -> f64 {
        self.viewport.horizontal_offset()
    }
    fn vertical_offset(&self) -> f64 {
        self.viewport.vertical_offset()
    }
    fn viewport_width(&self) -> f64 {
        self.viewport.layout().viewport_width
    }
    fn viewport_height(&self) -> f64 {
        self.viewport.layout().viewport_height
    }
    fn extent_width(&self) -> f64 {
        self.viewport.layout().extent_width
    }
    fn extent_height(&self) -> f64 {
        self.viewport.layout().extent_height
    }
    fn drag_threshold(&self) -> (f64, f64) {
        self.viewport.drag_threshold()
    }
    fn scroll_to(&mut self, horizontal: f64, vertical: f64) {
        self.viewport.scroll_to(horizontal, vertical);
    }
    fn update_layout(&mut self) {
        self.viewport.update_layout();
    }
    fn yield_to_render(&mut self, continuation: Box<dyn FnOnce()>) {
        self.viewport.yield_to_render(continuation);
    }
    fn image_origin(&self) -> PointD {
        self.viewport.image_origin()
    }
    fn to_image_element(&self, surface_point: PointD) -> PointD {
        self.viewport.to_image_element(surface_point)
    }
    fn image_actual_width(&self) -> f64 {
        self.viewport.image_actual_width()
    }
    fn image_actual_height(&self) -> f64 {
        self.viewport.image_actual_height()
    }
    fn source_size(&self) -> Option<SizeD> {
        self.viewport.source_size()
    }
    fn capture_mouse(&mut self) {
        self.viewport.capture_mouse();
    }
    fn release_mouse_capture(&mut self) {
        self.viewport.release_mouse_capture();
    }
    fn set_pan_cursor(&mut self, panning: bool) {
        self.viewport.set_pan_cursor(panning);
    }
    /// D-W8B4I-5: the router is captured at hook time and called after the controller's own handler, once per frame.
    fn hook_render_frame(&mut self, mut handler: RenderFrameHandler) -> RenderHookId {
        let router = self.router.clone();
        self.viewport.hook_render_frame(Box::new(move |ticks| {
            handler(ticks);
            if let Some(router) = &router {
                router(ticks);
            }
        }))
    }
    fn unhook_render_frame(&mut self, id: RenderHookId) {
        self.viewport.unhook_render_frame(id);
    }
    fn timestamp(&self) -> i64 {
        self.viewport.timestamp()
    }
    fn display_timing(&mut self) -> Option<DisplayTiming> {
        self.viewport.display_timing()
    }
    fn pointer_position(&self) -> Option<PointD> {
        self.viewport.pointer_position()
    }
}

impl FitSurface for ViewportImageSurface {
    fn viewport_size(&self) -> (f64, f64) {
        self.viewport.viewport_size()
    }
    fn is_loaded(&self) -> bool {
        self.viewport.is_loaded()
    }
    fn update_layout(&mut self) {
        self.viewport.update_layout();
    }
    fn update_fit_size(&mut self) {
        self.viewport.update_fit_size();
    }
    /// C# `Capture()` (T1): the viewer is borrowed immutably for the four reads only and released before the controller is read.
    fn capture(&self) -> ViewportSnapshot {
        let layout = self.viewport.layout();
        let (zoom, stretch, max_image_width, max_image_height) = {
            let viewer = self.viewer.borrow();
            (
                viewer.zoom(),
                viewer.stretch(),
                viewer.max_image_width(),
                viewer.max_image_height(),
            )
        };
        ViewportSnapshot {
            zoom,
            stretch,
            max_image_width,
            max_image_height,
            actual_image_width: self.viewport.image_actual_width(),
            actual_image_height: self.viewport.image_actual_height(),
            extent_width: layout.extent_width,
            extent_height: layout.extent_height,
            viewport_width: layout.viewport_width,
            viewport_height: layout.viewport_height,
            horizontal_offset: self.viewport.horizontal_offset(),
            vertical_offset: self.viewport.vertical_offset(),
            horizontal_scroll_bar_visible: layout.horizontal_bar_visible,
            vertical_scroll_bar_visible: layout.vertical_bar_visible,
        }
    }
    fn scroll_home(&mut self) {
        self.viewport.scroll_home();
    }
}

impl InputSurface for ViewportImageSurface {
    fn set_frame_router(&mut self, router: Option<FrameRouter>) {
        self.router = router;
    }
}

/// Plain forwarders (D-W8B4I-6, D-W8B4X-2): the real controller is the production [`ViewportControllerApi`].
impl ViewportControllerApi for ViewportController {
    fn layout(&self) -> ViewportLayout {
        ViewportController::layout(self)
    }
    fn viewport_size(&self) -> (f64, f64) {
        ViewportController::viewport_size(self)
    }
    fn is_loaded(&self) -> bool {
        ViewportController::is_loaded(self)
    }
    fn update_layout(&self) {
        ViewportController::update_layout(self);
    }
    fn update_fit_size(&self) {
        ViewportController::update_fit_size(self);
    }
    fn scroll_home(&self) {
        ViewportController::scroll_home(self);
    }
    fn invalidate_layout(&self) {
        ViewportController::invalidate_layout(self);
    }
    fn horizontal_offset(&self) -> f64 {
        ViewportController::horizontal_offset(self)
    }
    fn vertical_offset(&self) -> f64 {
        ViewportController::vertical_offset(self)
    }
    fn drag_threshold(&self) -> (f64, f64) {
        ViewportController::drag_threshold(self)
    }
    fn scroll_to(&self, horizontal: f64, vertical: f64) {
        ViewportController::scroll_to(self, horizontal, vertical);
    }
    fn image_origin(&self) -> PointD {
        ViewportController::image_origin(self)
    }
    fn to_image_element(&self, surface_point: PointD) -> PointD {
        ViewportController::to_image_element(self, surface_point)
    }
    fn image_actual_width(&self) -> f64 {
        ViewportController::image_actual_width(self)
    }
    fn image_actual_height(&self) -> f64 {
        ViewportController::image_actual_height(self)
    }
    fn source_size(&self) -> Option<SizeD> {
        ViewportController::source_size(self)
    }
    fn capture_mouse(&self) {
        ViewportController::capture_mouse(self);
    }
    fn release_mouse_capture(&self) {
        ViewportController::release_mouse_capture(self);
    }
    fn set_pan_cursor(&self, panning: bool) {
        ViewportController::set_pan_cursor(self, panning);
    }
    fn hook_render_frame(&self, handler: RenderFrameHandler) -> RenderHookId {
        ViewportController::hook_render_frame(self, handler)
    }
    fn unhook_render_frame(&self, id: RenderHookId) {
        ViewportController::unhook_render_frame(self, id);
    }
    fn timestamp(&self) -> i64 {
        ViewportController::timestamp(self)
    }
    fn display_timing(&self) -> Option<DisplayTiming> {
        ViewportController::display_timing(self)
    }
    fn pointer_position(&self) -> Option<PointD> {
        ViewportController::pointer_position(self)
    }
    fn yield_to_render(&self, continuation: Box<dyn FnOnce()>) {
        ViewportController::yield_to_render(self, continuation);
    }
}
