//! Port of `PhotoReview.Shell.Win32/Viewing/ViewportController.cs` (283 lines, WP-16, baseline `d92d6aae`). Unit W8-16h (contract `contracts/W8-16h.md`).
//!
//! The Win32 form of the WPF pair `ImageScroll` + `MainImage` (C-08 + C-07, Win32 side). It owns a `pr_app::viewport::state::ViewportState`, takes the
//! client size from the window and the image facts from a [`ViewportContent`] callback, and runs the layout engine (`viewport::layout`) either
//! synchronously ([`ViewportController::update_layout`]) or in one coalesced pass posted at Render priority (like the WPF layout manager). It exposes
//! exactly the members of `pr_app::input::surface::ImageSurface` and `pr_app::viewport::fit::FitSurface`; the adapter that implements those two traits
//! is `shell_input::surface` (W8-14c, C# `ViewportImageSurface`), so this unit has no trait impl for them.
//!
//! Coordinates are DIP (C-06). UI thread only; the owner calls [`ViewportController::invalidate_layout`] when the viewer state or the bitmap changes
//! (the replacement of the WPF bindings).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use pr_app::input::primitives::{PointD, SizeD};
use pr_app::input::surface::{RenderFrameHandler, RenderHookId};
use pr_app::viewport::layout;
use pr_app::viewport::state::{LayoutEngine, ViewportState};
use pr_app::viewport::types::{ScrollBarPolicy, ViewerStretchMode, ViewportInput, ViewportLayout};
use pr_core::abstractions::{DisplayClock, DisplayTiming};

use crate::host::contracts::{EventHandlerId, PointD as HostPointD, ShellCursor, ShellWindowApi, ShellWindowEvent};
use crate::host::frame_clock::base::{FrameHandlerId, FrameTick, HostFrameClock, qpc_now};

/// C# `MaxLayoutIterations`: the layout pass repeats until it is clean (the Fit size feeds back into the layout) at most this many times.
pub const MAX_LAYOUT_ITERATIONS: usize = 8;

/// C# `ViewportContent`: what `MainWindow.xaml` binds into `MainImage` (the viewer state and the displayed bitmap), the layout input that does not come
/// from the window. `is_fit` is `ViewerState.IsFit` (the `MainImage_SizeChanged` branch that calls `UpdateFitSize`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportContent {
    pub stretch: ViewerStretchMode,
    pub image_width: f64,
    pub image_height: f64,
    pub max_image_width: f64,
    pub max_image_height: f64,
    pub bitmap_width: f64,
    pub bitmap_height: f64,
    pub is_fit: bool,
}

impl ViewportContent {
    /// C# `ViewportContent.Empty`: no image yet, like a fresh `ViewerState`: `Uniform`, width / height NaN, maxima `+inf`, no bitmap, `is_fit`.
    pub fn empty() -> ViewportContent {
        ViewportContent {
            stretch: ViewerStretchMode::Uniform,
            image_width: f64::NAN,
            image_height: f64::NAN,
            max_image_width: f64::INFINITY,
            max_image_height: f64::INFINITY,
            bitmap_width: 0.0,
            bitmap_height: 0.0,
            is_fit: true,
        }
    }
}

/// Platform parameters of the controller; the defaults are the WPF values at 96 DPI.
pub struct ViewportControllerOptions {
    /// DIP; `Themes/DarkScrollBars.xaml`: `ScrollBar Width/Height = 10`.
    pub scroll_bar_thickness: f64,
    /// NE-3; `Auto` = like WPF.
    pub scroll_bars: ScrollBarPolicy,
    /// `SM_CXDRAG` / `SM_CYDRAG` in PIXELS at the window's DPI (the host wires `GetSystemMetricsForDpi`); the Windows default is 4.
    pub drag_metrics_pixels: Box<dyn Fn() -> (i32, i32)>,
    /// Pointer position in screen pixels (`GetCursorPos`), `None` when unreadable; the host wires the call.
    pub cursor_screen_pixel: Box<dyn Fn() -> Option<PointD>>,
    /// The vblank clock; `None` = no display timing.
    pub display_clock: Option<Arc<dyn DisplayClock>>,
    /// QPC ticks; test seam. Default `host::frame_clock::base::qpc_now`.
    pub timestamp: Box<dyn Fn() -> i64>,
}

impl Default for ViewportControllerOptions {
    fn default() -> Self {
        Self {
            scroll_bar_thickness: 10.0,
            scroll_bars: ScrollBarPolicy::Auto,
            drag_metrics_pixels: Box::new(|| (4, 4)),
            cursor_screen_pixel: Box::new(|| None),
            display_clock: None,
            timestamp: Box::new(qpc_now),
        }
    }
}

impl std::fmt::Debug for ViewportControllerOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewportControllerOptions")
            .field("scroll_bar_thickness", &self.scroll_bar_thickness)
            .field("scroll_bars", &self.scroll_bars)
            .field("has_display_clock", &self.display_clock.is_some())
            .finish_non_exhaustive()
    }
}

/// The only dispatcher feature the controller uses: C# `_dispatcher.Post(action, UiPriority.Render)` and `InvokeAsync(noop, UiPriority.Render)`.
/// The closure is not `Send` (it reaches the `!Send` controller), so the host keeps it in a UI-thread queue and posts a `Send` wake job at Render priority
/// (D-W8B4V-4; the same idea as `StripeScheduler`, D-W8B3S-2). FIFO among the closures posted through one poster.
pub trait RenderPoster {
    /// Runs `job` later on the UI thread at Render priority; never inline.
    fn post_render(&self, job: Box<dyn FnOnce()>);
}

/// `ViewportLayoutEngine.Compute` / `ClampOffset` as the `LayoutEngine` seam `ViewportState` takes: `viewport::layout::compute` and `clamp_offset`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ViewportLayoutEngine;

impl LayoutEngine for ViewportLayoutEngine {
    fn compute(&self, input: &ViewportInput) -> ViewportLayout {
        layout::compute(input)
    }

    fn clamp_offset(&self, layout: &ViewportLayout, horizontal: f64, vertical: f64) -> (f64, f64) {
        layout::clamp_offset(layout, horizontal, vertical)
    }
}

/// C# `NormalizedDpi(dpiScale)` (same range as `ViewerState.NormalizeDpi`): `0.25..=16` is kept, anything else (also NaN) is `1.0`.
pub fn normalized_dpi(dpi_scale: f64) -> f64 {
    if (0.25..=16.0).contains(&dpi_scale) {
        dpi_scale
    } else {
        1.0
    }
}

/// A `LayoutUpdated` subscriber.
type LayoutUpdatedHandler = Rc<dyn Fn()>;

/// C# `ViewportController`. Share it as `Rc<ViewportController>`; every method takes `&self` (fields are interior-mutable, no borrow is held across a
/// callback into the host, D-W8B4V-9).
pub struct ViewportController {
    window: Rc<dyn ShellWindowApi>,
    poster: Rc<dyn RenderPoster>,
    frame_clock: Rc<dyn HostFrameClock>,
    content: Rc<dyn Fn() -> ViewportContent>,
    update_fit_size: Rc<dyn Fn()>,
    options: ViewportControllerOptions,
    /// Borrowed only for single calls (`set_input`, `update_layout`, a getter), never across a callback.
    state: RefCell<ViewportState<ViewportLayoutEngine>>,
    layout_posted: Cell<bool>,
    disposed: Cell<bool>,
    /// C# `_frameHooks`: `(hook id, clock subscription)`.
    frame_hooks: RefCell<Vec<(RenderHookId, FrameHandlerId)>>,
    next_hook_id: Cell<u64>,
    layout_updated: RefCell<Vec<(u64, LayoutUpdatedHandler)>>,
    next_layout_updated_id: Cell<u64>,
    client_size_subscription: Cell<Option<EventHandlerId>>,
    this: Weak<ViewportController>,
}

impl std::fmt::Debug for ViewportController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewportController").finish_non_exhaustive()
    }
}

/// `ViewportLayoutMath.SanitizeLength`: finite and `> 0` keeps the value, anything else is 0.
fn sanitize_length(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 { value } else { 0.0 }
}

impl ViewportController {
    /// C# constructor `(window, dispatcher, frameClock, content, updateFitSize, options)` (`IUiDispatcher` -> [`RenderPoster`]). Subscribes to the
    /// window's `ClientSizeChanged` (handler: `update_fit_size()` then `invalidate_layout()`), then calls `invalidate_layout()` once.
    pub fn new(
        window: Rc<dyn ShellWindowApi>,
        poster: Rc<dyn RenderPoster>,
        frame_clock: Rc<dyn HostFrameClock>,
        content: Rc<dyn Fn() -> ViewportContent>,
        update_fit_size: Rc<dyn Fn()>,
        options: ViewportControllerOptions,
    ) -> Rc<ViewportController> {
        let controller = Rc::new_cyclic(|this| ViewportController {
            window,
            poster,
            frame_clock,
            content,
            update_fit_size,
            options,
            state: RefCell::new(ViewportState::new(ViewportLayoutEngine)),
            layout_posted: Cell::new(false),
            disposed: Cell::new(false),
            frame_hooks: RefCell::new(Vec::new()),
            next_hook_id: Cell::new(1),
            layout_updated: RefCell::new(Vec::new()),
            next_layout_updated_id: Cell::new(1),
            client_size_subscription: Cell::new(None),
            this: this.clone(),
        });
        let weak = Rc::downgrade(&controller);
        let id = controller.window.subscribe(
            ShellWindowEvent::ClientSizeChanged,
            Box::new(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.on_client_size_changed();
                }
            }),
        );
        controller.client_size_subscription.set(Some(id));
        controller.invalidate_layout();
        controller
    }

    /// C# `LayoutUpdated +=`: after a layout pass that changed the layout or an offset (the window was already invalidated).
    pub fn subscribe_layout_updated(&self, handler: Rc<dyn Fn()>) -> u64 {
        let id = self.next_layout_updated_id.get();
        self.next_layout_updated_id.set(id + 1);
        self.layout_updated.borrow_mut().push((id, handler));
        id
    }

    /// C# `LayoutUpdated -=`; an unknown id is a no-op.
    pub fn unsubscribe_layout_updated(&self, id: u64) {
        self.layout_updated.borrow_mut().retain(|(known, _)| *known != id);
    }

    /// C# `Layout`: the layout of the latest pass (draw the image at `image_rect - offset`; bars by their visibility flags).
    pub fn layout(&self) -> ViewportLayout {
        self.state.borrow().layout()
    }

    /// C# `IsLayoutPosted`: a Render-priority pass is queued.
    pub fn is_layout_posted(&self) -> bool {
        self.layout_posted.get()
    }

    /// C# `InvalidateLayout()`: the input (viewer state, bitmap) may have changed; queue ONE layout pass at Render priority (coalesced). No-op when
    /// disposed or already queued.
    pub fn invalidate_layout(&self) {
        if self.layout_posted.get() || self.disposed.get() {
            return;
        }
        self.layout_posted.set(true);
        let weak = self.this.clone();
        self.poster.post_render(Box::new(move || {
            if let Some(controller) = weak.upgrade() {
                controller.run_posted_layout();
            }
        }));
    }

    /// `options.scroll_bar_thickness` (DIP): the single source of the bar thickness (D-W8B4X-7). The main window gives the same value to
    /// `ScrollBarOptions::metrics.thickness` (13i), so the layout engine and the painted/hit-tested bars cannot disagree.
    pub fn scroll_bar_thickness(&self) -> f64 {
        self.options.scroll_bar_thickness
    }

    /// C# `RunPostedLayout`: the flag is cleared BEFORE the layout runs, so an `invalidate_layout()` made inside the pass posts a new job (T3).
    fn run_posted_layout(&self) {
        self.layout_posted.set(false);
        if !self.disposed.get() {
            self.run_layout();
        }
    }

    /// C# `CurrentInput`: the window's client size is passed on unsanitized (the engine sanitizes).
    fn current_input(&self, content: &ViewportContent) -> ViewportInput {
        let client = self.window.client_size_dip();
        ViewportInput {
            client_width: client.width,
            client_height: client.height,
            scroll_bar_thickness: self.options.scroll_bar_thickness,
            scroll_bars: self.options.scroll_bars,
            stretch: content.stretch,
            image_width: content.image_width,
            image_height: content.image_height,
            max_image_width: content.max_image_width,
            max_image_height: content.max_image_height,
            bitmap_width: content.bitmap_width,
            bitmap_height: content.bitmap_height,
        }
    }

    /// C# `RunLayout`: repeats until the layout is clean (at most [`MAX_LAYOUT_ITERATIONS`]); while Fit, a changed image size runs `update_fit_size`
    /// (`MainImage_SizeChanged`) and the next iteration lays out again. The state borrow never spans a callback (D-W8B4V-9). A `LayoutUpdated`
    /// handler that calls `update_layout()` runs a nested pass with its own iteration cap, so nesting terminates whenever the nested passes settle.
    fn run_layout(&self) {
        let mut changed = false;
        for _ in 0..MAX_LAYOUT_ITERATIONS {
            let content = (self.content)();
            let input = self.current_input(&content);
            let (before, updated, after) = {
                let mut state = self.state.borrow_mut();
                state.set_input(input);
                let before = (state.image_actual_width(), state.image_actual_height());
                let updated = state.update_layout();
                (
                    before,
                    updated,
                    (state.image_actual_width(), state.image_actual_height()),
                )
            };
            if !updated {
                break;
            }
            changed = true;
            // IEEE `!=` on both sides like the C# tuple operator: a NaN size always counts as changed (T2).
            if content.is_fit && (before.0 != after.0 || before.1 != after.1) {
                (self.update_fit_size)();
            }
        }
        if !changed {
            return;
        }
        self.window.invalidate();
        let handlers: Vec<Rc<dyn Fn()>> = self
            .layout_updated
            .borrow()
            .iter()
            .map(|(_, handler)| Rc::clone(handler))
            .collect();
        for handler in handlers {
            handler();
        }
    }

    /// C# `OnClientSizeChanged`: `UpdateFitSize`, then layout.
    fn on_client_size_changed(&self) {
        (self.update_fit_size)();
        self.invalidate_layout();
    }

    // ---- members of `FitSurface` (C-07) ----

    /// C# `ViewportSize`: the client size in DIP with each side passed through `ViewportLayoutMath.SanitizeLength` (NaN / negative / infinite = 0).
    pub fn viewport_size(&self) -> (f64, f64) {
        let client = self.window.client_size_dip();
        (sanitize_length(client.width), sanitize_length(client.height))
    }

    /// C# `IsLoaded`: `window.is_loaded() && !disposed`.
    pub fn is_loaded(&self) -> bool {
        self.window.is_loaded() && !self.disposed.get()
    }

    /// C# `UpdateLayout()` (`ImageScroll.UpdateLayout`): the layout pass NOW, synchronously.
    pub fn update_layout(&self) {
        self.run_layout();
    }

    /// C# `UpdateFitSize()`: forwards to the `update_fit_size` callback.
    pub fn update_fit_size(&self) {
        (self.update_fit_size)();
    }

    /// C# `YieldToRenderAsync()`, as a continuation (D-W8-2): runs `continuation` on the UI thread after every Render-priority job posted before it,
    /// that is after a layout pass queued earlier. `FitSurface` has no such member (the yield is `FitStep::AwaitRender`); `ImageSurface::yield_to_render`
    /// calls this.
    pub fn yield_to_render(&self, continuation: Box<dyn FnOnce()>) {
        self.poster.post_render(continuation);
    }

    /// C# `ScrollHome()`: `ViewportState::scroll_home` then `invalidate_layout`.
    pub fn scroll_home(&self) {
        self.state.borrow_mut().scroll_home();
        self.invalidate_layout();
    }

    // ---- members of `ImageSurface` (C-07) ----

    /// C# `HorizontalOffset`: the effective (clamped) offset after the latest pass.
    pub fn horizontal_offset(&self) -> f64 {
        self.state.borrow().horizontal_offset()
    }

    /// C# `VerticalOffset`: the effective (clamped) offset after the latest pass.
    pub fn vertical_offset(&self) -> f64 {
        self.state.borrow().vertical_offset()
    }

    /// C# `ViewportWidth`: from the latest layout.
    pub fn viewport_width(&self) -> f64 {
        self.state.borrow().layout().viewport_width
    }

    /// C# `ViewportHeight`: from the latest layout.
    pub fn viewport_height(&self) -> f64 {
        self.state.borrow().layout().viewport_height
    }

    /// C# `ExtentWidth`: from the latest layout.
    pub fn extent_width(&self) -> f64 {
        self.state.borrow().layout().extent_width
    }

    /// C# `ExtentHeight`: from the latest layout.
    pub fn extent_height(&self) -> f64 {
        self.state.borrow().layout().extent_height
    }

    /// C# `DragThreshold`: `drag_metrics_pixels()` divided by `normalized_dpi(window.dpi_scale())`, in DIP.
    pub fn drag_threshold(&self) -> (f64, f64) {
        let (horizontal, vertical) = (self.options.drag_metrics_pixels)();
        let dpi = normalized_dpi(self.window.dpi_scale());
        (f64::from(horizontal) / dpi, f64::from(vertical) / dpi)
    }

    /// C# `ScrollTo(h, v)`: `ViewportState::scroll_to` (clamped below 0 only), then `invalidate_layout`; applied by the next pass.
    pub fn scroll_to(&self, horizontal: f64, vertical: f64) {
        self.state.borrow_mut().scroll_to(horizontal, vertical);
        self.invalidate_layout();
    }

    /// C# `ImageOrigin`.
    pub fn image_origin(&self) -> PointD {
        self.state.borrow().image_origin()
    }

    /// C# `ToImageElement(surfacePoint)`.
    pub fn to_image_element(&self, surface_point: PointD) -> PointD {
        self.state.borrow().to_image_element(surface_point)
    }

    /// C# `ImageActualWidth`.
    pub fn image_actual_width(&self) -> f64 {
        self.state.borrow().image_actual_width()
    }

    /// C# `ImageActualHeight`.
    pub fn image_actual_height(&self) -> f64 {
        self.state.borrow().image_actual_height()
    }

    /// C# `SourceSize`: the bitmap size from `content()`, `None` unless both sides are `> 0`.
    pub fn source_size(&self) -> Option<SizeD> {
        let content = (self.content)();
        if content.bitmap_width > 0.0 && content.bitmap_height > 0.0 {
            Some(SizeD {
                width: content.bitmap_width,
                height: content.bitmap_height,
            })
        } else {
            None
        }
    }

    /// C# `CaptureMouse()`: `window.capture_pointer()`.
    pub fn capture_mouse(&self) {
        self.window.capture_pointer();
    }

    /// C# `ReleaseMouseCapture()`: releases only when the window holds the capture.
    pub fn release_mouse_capture(&self) {
        if self.window.has_pointer_capture() {
            self.window.release_pointer();
        }
    }

    /// C# `SetPanCursor(panning)`: `SizeAll` while panning, else `Arrow`.
    pub fn set_pan_cursor(&self, panning: bool) {
        self.window.set_cursor(if panning {
            ShellCursor::SizeAll
        } else {
            ShellCursor::Arrow
        });
    }

    /// C# `HookRenderFrame(handler)` (`CompositionTarget.Rendering +=`): every hook is its own frame subscription (hooking twice = called twice, like
    /// WPF) followed by `frame_clock.request_frame()`. The handler gets the frame's rendering time in `TimeSpan` ticks (100 ns): `Some(trunc(rendering_time_ms *
    /// 10_000))`.
    pub fn hook_render_frame(&self, mut handler: RenderFrameHandler) -> RenderHookId {
        let id = RenderHookId(self.next_hook_id.get());
        self.next_hook_id.set(id.0 + 1);
        // The clock takes a running handler out of its slot, so a hook that hooks or unhooks from inside its own delivery does not re-borrow.
        let subscription = self.frame_clock.subscribe(Box::new(move |tick: &FrameTick| {
            // `TimeSpan.FromMilliseconds` ticks (100 ns); the saturating `as` cast is intended (T4).
            handler(Some((tick.rendering_time_ms * 10_000.0) as i64));
        }));
        self.frame_hooks.borrow_mut().push((id, subscription));
        self.frame_clock.request_frame();
        id
    }

    /// C# `UnhookRenderFrame(handler)`: removes that hook's frame subscription; an unknown or already removed id is a no-op. Safe to call from inside the
    /// hooked handler.
    pub fn unhook_render_frame(&self, id: RenderHookId) {
        let subscription = {
            let mut hooks = self.frame_hooks.borrow_mut();
            hooks
                .iter()
                .position(|(known, _)| *known == id)
                .map(|index| hooks.remove(index).1)
        };
        if let Some(subscription) = subscription {
            self.frame_clock.unsubscribe(subscription);
        }
    }

    /// C# `Timestamp`: `options.timestamp()` (QPC ticks).
    pub fn timestamp(&self) -> i64 {
        (self.options.timestamp)()
    }

    /// C# `DisplayTiming`: `display_clock.get_timing(window.hwnd())`, `None` without a clock.
    pub fn display_timing(&self) -> Option<DisplayTiming> {
        self.options
            .display_clock
            .as_ref()
            .and_then(|clock| clock.get_timing(self.window.hwnd()))
    }

    /// C# `PointerPosition`: `cursor_screen_pixel()` -> `window.screen_to_client_dip`; `None` when the cursor is unreadable or outside
    /// `0..=viewport_width` x `0..=viewport_height` (both ends inclusive).
    pub fn pointer_position(&self) -> Option<PointD> {
        let screen = (self.options.cursor_screen_pixel)()?;
        let point = self.window.screen_to_client_dip(HostPointD {
            x: screen.x,
            y: screen.y,
        });
        let inside =
            point.x >= 0.0 && point.y >= 0.0 && point.x <= self.viewport_width() && point.y <= self.viewport_height();
        inside.then_some(PointD { x: point.x, y: point.y })
    }

    /// C# `Dispose()`: idempotent; unsubscribes from the window and from the frame clock (all hooks), later `invalidate_layout` calls do nothing.
    /// `Drop` calls it.
    pub fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        if let Some(id) = self.client_size_subscription.take() {
            self.window.unsubscribe(id);
        }
        let hooks: Vec<(RenderHookId, FrameHandlerId)> = self.frame_hooks.borrow_mut().drain(..).collect();
        for (_, subscription) in hooks {
            self.frame_clock.unsubscribe(subscription);
        }
    }
}

impl Drop for ViewportController {
    fn drop(&mut self) {
        self.dispose();
    }
}
