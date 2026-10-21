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

// Contract stub: the bodies are `todo!()` until the W8-16i port; removed with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::rc::Rc;

use pr_app::coordinators::presenter::state::ImageChange;
use pr_app::coordinators::traits::IPresentationSink;
use pr_imaging::decode_types::PlatformImage;

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
        todo!("W8-16i")
    }
    fn ms_since(&self, assigned_timestamp: i64) -> f64 {
        todo!("W8-16i")
    }
    fn rendered(&self, token: i64, ms_since_assign: f64) {
        todo!("W8-16i")
    }
    fn presented(&self, token: i64, kind: &str) {
        todo!("W8-16i")
    }
    fn rendered_frame(&self, token: i64, ms_since_assign: f64) {
        todo!("W8-16i")
    }
}

/// C# `Action<object?, bool> ImageChanged`: the new image (`None` = empty / compare) and `is_file_change`. UI thread, no `Send`.
pub type ImageChangedHandler = Rc<dyn Fn(Option<&PlatformImage>, bool)>;

/// C# `ShellPresentationHub` (a singleton of the composition root, shared as `Rc`; all methods `&self`).
pub struct ShellPresentationHub {
    // Fields (interior-mutable): perf marks, current image, current_image_presented flag, image change count, pending traces (token, kind,
    // assigned timestamp), ImageChanged handlers + next id.
    _private: (),
}

impl std::fmt::Debug for ShellPresentationHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellPresentationHub").finish_non_exhaustive()
    }
}

impl ShellPresentationHub {
    /// New hub over the real perf log.
    pub fn new() -> Self {
        todo!("W8-16i")
    }

    /// New hub over a given perf log (tests).
    pub fn with_perf(perf: Rc<dyn PerfMarks>) -> Self {
        todo!("W8-16i")
    }

    /// C# `CurrentImage`: the image set last (`PlatformImage::Pixels` in the shell); `None` = empty or compare.
    pub fn current_image(&self) -> Option<PlatformImage> {
        todo!("W8-16i")
    }

    /// C# `ImageChangeCount`: how many times the image was set (diagnostics, tests).
    pub fn image_change_count(&self) -> i32 {
        todo!("W8-16i")
    }

    /// Number of perf marks waiting for a frame (tests; C# reads the private list).
    pub fn pending_trace_count(&self) -> usize {
        todo!("W8-16i")
    }

    /// C# `ImageChanged +=`. Handlers run in subscription order on the UI thread, after `current_image` is stored.
    pub fn subscribe_image_changed(&self, handler: ImageChangedHandler) -> u64 {
        todo!("W8-16i")
    }

    /// C# `ImageChanged -=`; an unknown id is a no-op.
    pub fn unsubscribe_image_changed(&self, id: u64) {
        todo!("W8-16i")
    }

    /// C# `SetCurrentImage(image, isFileChange)` (internal; the sink calls it): stores the image, clears the "presented" flag, counts, raises `ImageChanged`.
    pub fn set_current_image(&self, image: Option<PlatformImage>, is_file_change: bool) {
        todo!("W8-16i")
    }

    /// C# `AddTrace(token, kind, assignedTimestamp)`: when the current image is already in a presented frame, emit now (the first image is drawn inside
    /// the assigning pass, before the presenter posts its mark; a static screen may never produce another frame); otherwise queue it, dropping the
    /// OLDEST when 16 are waiting.
    pub fn add_trace(&self, token: i64, kind: &str, assigned_timestamp: i64) {
        todo!("W8-16i")
    }

    /// C# `NoteFramePresented(containedImage = true)`: a frame was presented. A frame that did not contain the current image changes nothing. Otherwise
    /// the "presented" flag is set and every queued mark is emitted, in order, and the queue cleared (C-18: `RenderedFrame` = the `Present` of the
    /// frame that has the image).
    pub fn note_frame_presented(&self, contained_image: bool) {
        todo!("W8-16i")
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
        todo!("W8-16i")
    }
}

impl IPresentationSink for Win32PresentationSink {
    /// C# `SetCurrentImage`: `callbacks.set_current_image(image.clone(), is_file_change, change)` FIRST, then `hub.set_current_image(image,
    /// is_file_change)`.
    fn set_current_image(&mut self, image: Option<PlatformImage>, is_file_change: bool, change: &ImageChange<'_>) {
        todo!("W8-16i")
    }

    /// The view model's pending zoom change (`callbacks.take_viewer_zoom_change`).
    fn take_viewer_zoom_change(&mut self) -> Option<Option<f64>> {
        todo!("W8-16i")
    }

    /// C# `SetStatusText`: `callbacks.set_status_text`.
    fn set_status_text(&mut self, status: &str) {
        todo!("W8-16i")
    }

    /// C# `ApplyInitialViewMode`: `callbacks.apply_initial_view_mode`.
    fn apply_initial_view_mode(&mut self) {
        todo!("W8-16i")
    }

    /// C# `OnPresented(path)`: `callbacks.on_presented`.
    fn on_presented(&mut self, path: &str) {
        todo!("W8-16i")
    }

    /// C# `TracePresented`: `hub.add_trace(token, kind, assigned_timestamp)`.
    fn trace_presented(&mut self, token: i64, kind: &str, assigned_timestamp: i64) {
        todo!("W8-16i")
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
        todo!("W8-16i")
    }

    /// C# `Create(callbacks)`: a sink over the shared hub.
    pub fn create(&self, callbacks: SinkCallbacks) -> Box<dyn IPresentationSink> {
        todo!("W8-16i")
    }
}
