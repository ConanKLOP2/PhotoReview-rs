//! Port of `PhotoReview.Shell.Win32/Viewing/Win32PresentationSink.cs` (103 lines, C-16, baseline `d92d6aae`) and `PhotoReview.Shell.Win32/Startup/ShellPresentationSink.cs`
//! (89 lines: the `ShellPresentationHub`). Unit W8-16i (contract `contracts/W8-16i.md`). WPF `WpfPresentationSink` (116) and `WpfPresentationSinkFactory`
//! (23) are retired.
//!
//! [`Win32PresentationSink`] is the `IPresentationSink` the `ImagePresenter` calls; it forwards each result to the view model's callbacks FIRST (the viewer
//! state learns the new source size and raises `ImageChanging`) and only then to the [`ShellPresentationHub`], which hands the image to the main window
//! (`PresentationComposer`) and emits the perf marks `Rendered` / `Presented` / `RenderedFrame` when the frame that CONTAINS that image is presented (C-18).
//!
//! Not ported: the C# cross-thread marshalling (`InvokeUi`: post to the dispatcher, FIFO guard, `CrossThreadPresentCount`). The Rust sink and hub are
//! `!Send` (`Rc`), so a call from another thread does not compile (D-W8B4V-10). UI thread only.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pr_core::diagnostics::perf::{self, PhotoReviewPerf};

use pr_app::coordinators::presenter::state::ImageChange;
use pr_app::coordinators::traits::IPresentationSink;
use pr_imaging::decode_types::PlatformImage;

use crate::host::main_window::core::PresentationHubPort;

/// C# `ShellPresentationHub`: at most this many perf marks wait for their frame (a minimised window never draws; one wait per token).
pub const MAX_PENDING_TRACES: usize = 16;

/// The perf log as the hub uses it (C# `PhotoReviewPerf.Log`); `RealPerfMarks` forwards to `pr_core::diagnostics::perf::PhotoReviewPerf::log()`.
pub trait PerfMarks {
    /// C# `PhotoReviewPerf.Log.IsEnabled()`.
    fn is_enabled(&self) -> bool;
    /// C# `PhotoReviewPerf.Ms(assignedTimestamp)`: milliseconds from the QPC timestamp to now.
    fn ms_since(&self, assigned_timestamp: i64) -> f64;
    /// C# `Log.Rendered(token, ms)`.
    fn rendered(&self, token: i64, ms_since_assign: f64);
    /// C# `Log.Presented(token, kind)`.
    fn presented(&self, token: i64, kind: &str);
    /// C# `Log.RenderedFrame(token, ms)`.
    fn rendered_frame(&self, token: i64, ms_since_assign: f64);
}

/// The real perf log.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealPerfMarks;

impl PerfMarks for RealPerfMarks {
    fn is_enabled(&self) -> bool {
        PhotoReviewPerf::log().is_enabled()
    }
    fn ms_since(&self, assigned_timestamp: i64) -> f64 {
        perf::ms(assigned_timestamp)
    }
    fn rendered(&self, token: i64, ms_since_assign: f64) {
        PhotoReviewPerf::log().rendered(token, ms_since_assign);
    }
    fn presented(&self, token: i64, kind: &str) {
        PhotoReviewPerf::log().presented(token, kind);
    }
    fn rendered_frame(&self, token: i64, ms_since_assign: f64) {
        PhotoReviewPerf::log().rendered_frame(token, ms_since_assign);
    }
}

/// C# `Action<object?, bool> ImageChanged`: the new image (`None` = empty / compare) and `is_file_change`. UI thread, no `Send`.
pub type ImageChangedHandler = Rc<dyn Fn(Option<&PlatformImage>, bool)>;

/// C# `ShellPresentationHub` (a singleton of the composition root, shared as `Rc`; all methods `&self`).
pub struct ShellPresentationHub {
    perf: Rc<dyn PerfMarks>,
    current_image: RefCell<Option<PlatformImage>>,
    current_image_presented: Cell<bool>,
    image_change_count: Cell<i32>,
    /// `(token, kind, assigned_timestamp)`, oldest first.
    pending_traces: RefCell<Vec<(i64, String, i64)>>,
    handlers: RefCell<Vec<(u64, ImageChangedHandler)>>,
    next_handler_id: Cell<u64>,
}

impl std::fmt::Debug for ShellPresentationHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellPresentationHub").finish_non_exhaustive()
    }
}

impl ShellPresentationHub {
    /// New hub over the real perf log.
    pub fn new() -> Self {
        Self::with_perf(Rc::new(RealPerfMarks))
    }

    /// New hub over a given perf log (tests).
    pub fn with_perf(perf: Rc<dyn PerfMarks>) -> Self {
        Self {
            perf,
            current_image: RefCell::new(None),
            current_image_presented: Cell::new(false),
            image_change_count: Cell::new(0),
            pending_traces: RefCell::new(Vec::new()),
            handlers: RefCell::new(Vec::new()),
            next_handler_id: Cell::new(1),
        }
    }

    /// C# `CurrentImage`: the image set last (`PlatformImage::Pixels` in the shell); `None` = empty or compare.
    pub fn current_image(&self) -> Option<PlatformImage> {
        self.current_image.borrow().clone()
    }

    /// C# `ImageChangeCount`: how many times the image was set (diagnostics, tests).
    pub fn image_change_count(&self) -> i32 {
        self.image_change_count.get()
    }

    /// Number of perf marks waiting for a frame (tests; C# reads the private list).
    pub fn pending_trace_count(&self) -> usize {
        self.pending_traces.borrow().len()
    }

    /// C# `ImageChanged +=`. Handlers run in subscription order on the UI thread, after `current_image` is stored.
    pub fn subscribe_image_changed(&self, handler: ImageChangedHandler) -> u64 {
        let id = self.next_handler_id.get();
        self.next_handler_id.set(id + 1);
        self.handlers.borrow_mut().push((id, handler));
        id
    }

    /// C# `ImageChanged -=`; an unknown id is a no-op.
    pub fn unsubscribe_image_changed(&self, id: u64) {
        self.handlers.borrow_mut().retain(|(handler_id, _)| *handler_id != id);
    }

    /// C# `SetCurrentImage(image, isFileChange)` (internal; the sink calls it): stores the image, clears the "presented" flag, counts, raises `ImageChanged`.
    pub fn set_current_image(&self, image: Option<PlatformImage>, is_file_change: bool) {
        *self.current_image.borrow_mut() = image.clone();
        self.current_image_presented.set(false);
        self.image_change_count
            .set(self.image_change_count.get().wrapping_add(1));
        // Snapshot: no borrow is held while a handler runs (it may subscribe, unsubscribe itself or read `current_image`).
        let snapshot: Vec<ImageChangedHandler> = self
            .handlers
            .borrow()
            .iter()
            .map(|(_, handler)| Rc::clone(handler))
            .collect();
        for handler in snapshot {
            handler(image.as_ref(), is_file_change);
        }
    }

    /// C# `AddTrace(token, kind, assignedTimestamp)`: when the current image is already in a presented frame, emit now (the first image is drawn inside
    /// the assigning pass, before the presenter posts its mark; a static screen may never produce another frame); otherwise queue it, dropping the
    /// OLDEST when 16 are waiting.
    pub fn add_trace(&self, token: i64, kind: &str, assigned_timestamp: i64) {
        if self.current_image_presented.get() {
            self.emit(token, kind, assigned_timestamp);
            return;
        }
        let mut pending = self.pending_traces.borrow_mut();
        if pending.len() >= MAX_PENDING_TRACES {
            pending.remove(0);
        }
        pending.push((token, kind.to_owned(), assigned_timestamp));
    }

    /// C# `NoteFramePresented(containedImage = true)`: a frame was presented. A frame that did not contain the current image changes nothing. Otherwise
    /// the "presented" flag is set and every queued mark is emitted, in order, and the queue cleared (C-18: `RenderedFrame` = the `Present` of the
    /// frame that has the image).
    pub fn note_frame_presented(&self, contained_image: bool) {
        if !contained_image {
            return;
        }
        self.current_image_presented.set(true);
        let queued = std::mem::take(&mut *self.pending_traces.borrow_mut());
        for (token, kind, assigned) in queued {
            self.emit(token, &kind, assigned);
        }
    }
}

impl ShellPresentationHub {
    /// C# `Emit`: `Rendered`, `Presented`, `RenderedFrame` in this order (`pr_perf::nav` reads `RenderedFrame` after `Presented`).
    fn emit(&self, token: i64, kind: &str, assigned_timestamp: i64) {
        if !self.perf.is_enabled() {
            return;
        }
        self.perf.rendered(token, self.perf.ms_since(assigned_timestamp));
        self.perf.presented(token, kind);
        self.perf.rendered_frame(token, self.perf.ms_since(assigned_timestamp));
    }
}

/// The window sees the hub through this port (W8-13g).
impl PresentationHubPort for ShellPresentationHub {
    fn current_image(&self) -> Option<PlatformImage> {
        ShellPresentationHub::current_image(self)
    }
    fn subscribe_image_changed(&self, handler: ImageChangedHandler) -> u64 {
        ShellPresentationHub::subscribe_image_changed(self, handler)
    }
    fn unsubscribe_image_changed(&self, id: u64) {
        ShellPresentationHub::unsubscribe_image_changed(self, id);
    }
    fn note_frame_presented(&self, contained_image: bool) {
        ShellPresentationHub::note_frame_presented(self, contained_image);
    }
}

impl Default for ShellPresentationHub {
    fn default() -> Self {
        Self::new()
    }
}

/// C# `Action<object?, bool>` of `MainViewModelSinkCallbacks`, extended with the presenter facts (W8-07h).
pub type SetCurrentImageCallback = Box<dyn FnMut(Option<PlatformImage>, bool, &ImageChange<'_>)>;

/// Field-for-field twin of `pr_shell::composition::contracts::MainViewModelSinkCallbacks` PLUS the pull of the viewer zoom
/// (`take_viewer_zoom_change`, W8-08a O1): `pr-platform-win` cannot name `pr-shell` types (D-W8B4V-11); the composition root converts one into the other.
pub struct SinkCallbacks {
    /// View model: the image changed (viewer state, `ImageChanging`, source size).
    pub set_current_image: SetCurrentImageCallback,
    /// View model: the status line changed.
    pub set_status_text: Box<dyn FnMut(&str)>,
    /// View model: apply the initial view mode (Fit / Uniform / 100 %).
    pub apply_initial_view_mode: Box<dyn FnMut()>,
    /// View model: the image of this path is ready to show.
    pub on_presented: Box<dyn FnMut(&str)>,
    /// View model: the zoom (`None` = Fit) it asked for during the last call, once (`PresentationBridge::take_viewer_zoom_change`).
    pub take_viewer_zoom_change: Box<dyn FnMut() -> Option<Option<f64>>>,
}

impl std::fmt::Debug for SinkCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SinkCallbacks").finish_non_exhaustive()
    }
}

/// C# `Win32PresentationSink`.
pub struct Win32PresentationSink {
    hub: Rc<ShellPresentationHub>,
    callbacks: SinkCallbacks,
}

impl std::fmt::Debug for Win32PresentationSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Win32PresentationSink").finish_non_exhaustive()
    }
}

impl Win32PresentationSink {
    /// C# constructor `(hub, callbacks, dispatcher, metrics)`; the dispatcher and the metrics only served the cross-thread path (not ported).
    pub fn new(hub: Rc<ShellPresentationHub>, callbacks: SinkCallbacks) -> Self {
        Self { hub, callbacks }
    }
}

impl IPresentationSink for Win32PresentationSink {
    /// C# `SetCurrentImage`: `callbacks.set_current_image(image.clone(), is_file_change, change)` FIRST, then `hub.set_current_image(image,
    /// is_file_change)`.
    fn set_current_image(&mut self, image: Option<PlatformImage>, is_file_change: bool, change: &ImageChange<'_>) {
        // The view model first (viewer state, `ImageChanging`), then the surface (ADR 0005 / C-16).
        (self.callbacks.set_current_image)(image.clone(), is_file_change, change);
        self.hub.set_current_image(image, is_file_change);
    }

    /// The view model's pending zoom change (`callbacks.take_viewer_zoom_change`).
    fn take_viewer_zoom_change(&mut self) -> Option<Option<f64>> {
        (self.callbacks.take_viewer_zoom_change)()
    }

    /// C# `SetStatusText`: `callbacks.set_status_text`.
    fn set_status_text(&mut self, status: &str) {
        (self.callbacks.set_status_text)(status);
    }

    /// C# `ApplyInitialViewMode`: `callbacks.apply_initial_view_mode`.
    fn apply_initial_view_mode(&mut self) {
        (self.callbacks.apply_initial_view_mode)();
    }

    /// C# `OnPresented(path)`: `callbacks.on_presented`.
    fn on_presented(&mut self, path: &str) {
        (self.callbacks.on_presented)(path);
    }

    /// C# `TracePresented`: `hub.add_trace(token, kind, assigned_timestamp)`.
    fn trace_presented(&mut self, token: i64, kind: &str, assigned_timestamp: i64) {
        self.hub.add_trace(token, kind, assigned_timestamp);
    }
}

/// C# `Win32PresentationSinkFactory(hub, dispatcher, metrics)`. The `pr_shell` `PresentationSinkFactory` impl is a one-line adapter over
/// [`Win32PresentationSinkFactory::create`] in the composition root (W8-09b / W8-17c).
#[derive(Debug, Clone)]
pub struct Win32PresentationSinkFactory {
    hub: Rc<ShellPresentationHub>,
}

impl Win32PresentationSinkFactory {
    pub fn new(hub: Rc<ShellPresentationHub>) -> Self {
        Self { hub }
    }

    /// C# `Create(callbacks)`: a sink over the shared hub.
    pub fn create(&self, callbacks: SinkCallbacks) -> Box<dyn IPresentationSink> {
        Box::new(Win32PresentationSink::new(Rc::clone(&self.hub), callbacks))
    }
}
