//! Fixture of the W8-14b tests (`tests/shell_input_dispatch.rs`): the Rust twin of `InputTestSupport.cs` (baseline `d92d6aae`): `FakeShellWindow`,
//! `RecordingInputHost`, `QueueDispatcher` and `InputRig`.
//!
//! The rig is the C# `InputRig`: a window without an HWND whose real handler list is run by [`InputWindow::dispatch`] in registration order, a REAL
//! `ViewportController` (layout engine, scroll offsets, frame hooks) on a manual frame clock, a REAL `ViewerState` and a REAL `ShellInput`. Only the
//! window, the host, the clock and the dispatcher are fakes. Everything the system provides besides the message (modifier keys, time stamps, the wheel
//! device) is set through the `Cell`s of [`Rig`].
//!
//! The viewport's content callback does NOT read the `ViewerHandle`: `PointerContext::viewer` holds a `borrow_mut` for the whole pure call and the
//! `ViewerHandle` has no `try_borrow`, so a layout pass inside the call (`ScrollAnchorTo` -> `update_layout`) reads a cache ([`Rig::refresh_content`]) that
//! the rig refreshes before every job it runs and before every message. The layout inside the call is therefore the layout of the previous pass; the
//! controller notices the difference at the Render-priority resume and re-anchors, which is the observable end state of the C# (see the report of W8-14b).
#![allow(dead_code)] // shared by the modules of one test crate, each of which uses a different part

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pr_app::input::key_id::KeyId;
use pr_app::input::key_mapping;
use pr_app::input::primitives::SizeD as AppSizeD;
use pr_app::input::primitives::{KeyModifiers, PointD};
use pr_app::input::review_command::ReviewCommand;
use pr_app::input::surface::{ImageSurface, RenderFrameHandler, RenderHookId};
use pr_app::input::touchpad_gestures::WheelDeviceHint;
use pr_app::viewmodels::main::ViewerHandle;
use pr_app::viewmodels::viewer_state::ViewerState;
use pr_app::viewport::fit::FitSurface;
use pr_app::viewport::types::ViewportSnapshot;
use pr_core::abstractions::{DisplayTiming, UiAction, UiCompletion, UiDispatcher, UiPriority, UiScheduler};
use pr_core::settings::app_settings::AppSettings;
use pr_platform_win::host::contracts::{
    ClosingHandler, EventHandlerId, PointD as HostPointD, ShellCursor, ShellWindowApi, ShellWindowEvent, SizeD,
    WindowEventHandler, WindowMessage, WindowMessageHandler,
};
use pr_platform_win::host::error::HostError;
use pr_platform_win::shell_input::dispatch::{FrameRouter, InputSurface, ShellInput, ShellInputDeps, ShellInputHost};
use pr_platform_win::shell_input::environment::ShellInputEnvironment;
use pr_platform_win::shell_viewing::viewport_controller::{
    RenderPoster, ViewportContent, ViewportController, ViewportControllerOptions,
};
use pr_platform_win::win32::messages::{WM_CAPTURECHANGED, WM_KEYDOWN, WM_SYSKEYDOWN};
use pr_platform_win::win32::structs::{Hwnd, Lparam, Lresult};
#[allow(unused_imports)] // re-exported for the tests that assert on log levels
pub use pr_test_support::LogLevel;
use pr_test_support::RecordingLog;

use crate::viewing_fakes::FakeHostClock;

// ---------------------------------------------------------------------------------------------------------------------------------------------
// FakeShellWindow
// ---------------------------------------------------------------------------------------------------------------------------------------------

type SharedHandler = Rc<RefCell<WindowEventHandler>>;

/// C# `FakeShellWindow`: no HWND; size, DPI and capture are set by the test; the handler list is run by [`InputWindow::dispatch`].
pub struct InputWindow {
    pub dpi_scale: Cell<f64>,
    pub client_size: Cell<SizeD>,
    pub loaded: Cell<bool>,
    pub captured: Cell<bool>,
    pub invalidations: Cell<u32>,
    pub capture_calls: Cell<u32>,
    pub releases: Cell<u32>,
    pub cursor: Cell<Option<ShellCursor>>,
    pub client_origin_pixel: Cell<HostPointD>,
    /// When set, `release_pointer` sends `WM_CAPTURECHANGED` through the handler chain SYNCHRONOUSLY, like `ReleaseCapture` does (D-W8B4I-4, trap T1).
    pub capture_changed_on_release: Cell<bool>,
    /// How many times `unsubscribe` was called with a known id.
    pub unsubscribes: Cell<u32>,
    handlers: RefCell<Vec<Rc<dyn WindowMessageHandler>>>,
    subscriptions: RefCell<Vec<(EventHandlerId, ShellWindowEvent, SharedHandler)>>,
    next_id: Cell<u64>,
}

impl InputWindow {
    pub fn new(client_width: f64, client_height: f64, dpi_scale: f64) -> Rc<InputWindow> {
        Rc::new(InputWindow {
            dpi_scale: Cell::new(dpi_scale),
            client_size: Cell::new(SizeD {
                width: client_width,
                height: client_height,
            }),
            loaded: Cell::new(true),
            captured: Cell::new(false),
            invalidations: Cell::new(0),
            capture_calls: Cell::new(0),
            releases: Cell::new(0),
            cursor: Cell::new(None),
            client_origin_pixel: Cell::new(HostPointD::default()),
            capture_changed_on_release: Cell::new(false),
            unsubscribes: Cell::new(0),
            handlers: RefCell::new(Vec::new()),
            subscriptions: RefCell::new(Vec::new()),
            next_id: Cell::new(0),
        })
    }

    /// C# `Resize`: changes the client size, then raises `ClientSizeChanged`.
    pub fn resize(&self, width: f64, height: f64) {
        self.client_size.set(SizeD { width, height });
        self.raise(ShellWindowEvent::ClientSizeChanged);
    }

    /// C# `RaiseDeactivated`.
    pub fn raise_deactivated(&self) {
        self.raise(ShellWindowEvent::Deactivated);
    }

    /// Runs the handlers of `event` in subscription order (on a snapshot).
    pub fn raise(&self, event: ShellWindowEvent) {
        let handlers: Vec<SharedHandler> = self
            .subscriptions
            .borrow()
            .iter()
            .filter(|(_, known, _)| *known == event)
            .map(|(_, _, handler)| Rc::clone(handler))
            .collect();
        for handler in handlers {
            (handler.borrow_mut())();
        }
    }

    /// Number of live subscriptions of `event`.
    pub fn subscriber_count(&self, event: ShellWindowEvent) -> usize {
        self.subscriptions
            .borrow()
            .iter()
            .filter(|(_, known, _)| *known == event)
            .count()
    }

    /// Number of message handlers added so far.
    pub fn handler_count(&self) -> usize {
        self.handlers.borrow().len()
    }

    /// C# `Dispatch`: runs the message through the handlers in registration order; `(handled, result)`.
    pub fn dispatch(&self, msg: u32, wparam: usize, lparam: Lparam) -> (bool, Lresult) {
        let message = WindowMessage {
            hwnd: 0x1234,
            msg,
            wparam,
            lparam,
        };
        let handlers: Vec<Rc<dyn WindowMessageHandler>> = self.handlers.borrow().clone();
        for handler in handlers {
            if let Some(result) = handler.try_handle(&message) {
                return (true, result);
            }
        }
        (false, 0)
    }
}

impl ShellWindowApi for InputWindow {
    fn hwnd(&self) -> Hwnd {
        0x1234
    }
    fn dpi_scale(&self) -> f64 {
        self.dpi_scale.get()
    }
    fn client_size_dip(&self) -> SizeD {
        self.client_size.get()
    }
    fn is_active(&self) -> bool {
        true
    }
    fn is_loaded(&self) -> bool {
        self.loaded.get()
    }
    fn subscribe(&self, event: ShellWindowEvent, handler: WindowEventHandler) -> EventHandlerId {
        self.next_id.set(self.next_id.get() + 1);
        let id = self.next_id.get();
        self.subscriptions
            .borrow_mut()
            .push((id, event, Rc::new(RefCell::new(handler))));
        id
    }
    fn subscribe_closing(&self, _handler: ClosingHandler) -> EventHandlerId {
        self.next_id.set(self.next_id.get() + 1);
        self.next_id.get()
    }
    fn unsubscribe(&self, id: EventHandlerId) {
        let before = self.subscriptions.borrow().len();
        self.subscriptions.borrow_mut().retain(|(known, _, _)| *known != id);
        if self.subscriptions.borrow().len() != before {
            self.unsubscribes.set(self.unsubscribes.get() + 1);
        }
    }
    fn add_message_handler(&self, handler: Rc<dyn WindowMessageHandler>) {
        self.handlers.borrow_mut().push(handler);
    }
    fn insert_message_handler_first(&self, handler: Rc<dyn WindowMessageHandler>) {
        self.handlers.borrow_mut().insert(0, handler);
    }
    fn is_destroyed(&self) -> bool {
        false
    }
    fn invalidate(&self) {
        self.invalidations.set(self.invalidations.get() + 1);
    }
    fn set_title(&self, _title: &str) -> Result<(), HostError> {
        Ok(())
    }
    fn set_cursor(&self, cursor: ShellCursor) {
        self.cursor.set(Some(cursor));
    }
    fn capture_pointer(&self) {
        self.capture_calls.set(self.capture_calls.get() + 1);
        self.captured.set(true);
    }
    fn release_pointer(&self) {
        self.releases.set(self.releases.get() + 1);
        self.captured.set(false);
        if self.capture_changed_on_release.get() {
            let _ = self.dispatch(WM_CAPTURECHANGED, 0, 0);
        }
    }
    fn has_pointer_capture(&self) -> bool {
        self.captured.get()
    }
    fn screen_to_client_dip(&self, screen_pixel: HostPointD) -> HostPointD {
        let origin = self.client_origin_pixel.get();
        let scale = self.dpi_scale.get();
        HostPointD {
            x: (screen_pixel.x - origin.x) / scale,
            y: (screen_pixel.y - origin.y) / scale,
        }
    }
    fn close(&self) {}
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// RecordingInputHost
// ---------------------------------------------------------------------------------------------------------------------------------------------

/// The error `RecordingHost::execute_failure` makes `execute` return.
#[derive(Debug)]
pub struct HostFailure(pub String);

impl fmt::Display for HostFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for HostFailure {}

/// C# `RecordingInputHost`: the host state the tests set, and the calls `ShellInput` made on it.
pub struct RecordingHost {
    pub has_images: Cell<bool>,
    pub is_fullscreen: Cell<bool>,
    pub is_compare_visible: Cell<bool>,
    pub current_has_compare_pair: Cell<bool>,
    pub current_has_capture_pair: Cell<bool>,
    pub duplicate_check_running: Cell<bool>,
    /// `"CancelDuplicateCheck"`, `"failed:{name}:{message}"`.
    pub log: RefCell<Vec<String>>,
    pub commands: RefCell<Vec<ReviewCommand>>,
    pub nexts: Cell<u32>,
    pub previouses: Cell<u32>,
    /// When set, `execute` returns this error.
    pub execute_failure: RefCell<Option<String>>,
    /// Every host call in order (`"next"`, `"previous"`, `"execute:{kind:?}"`), to pin the order of effects.
    pub calls: RefCell<Vec<String>>,
    /// How often `on_viewer_zoom_mode_changed` was called (zoom-detail-wire).
    pub zoom_mode_changes: Cell<u32>,
}

impl RecordingHost {
    pub fn new() -> Rc<RecordingHost> {
        Rc::new(RecordingHost {
            has_images: Cell::new(true),
            is_fullscreen: Cell::new(false),
            is_compare_visible: Cell::new(false),
            current_has_compare_pair: Cell::new(false),
            current_has_capture_pair: Cell::new(false),
            duplicate_check_running: Cell::new(false),
            log: RefCell::new(Vec::new()),
            commands: RefCell::new(Vec::new()),
            nexts: Cell::new(0),
            previouses: Cell::new(0),
            execute_failure: RefCell::new(None),
            calls: RefCell::new(Vec::new()),
            zoom_mode_changes: Cell::new(0),
        })
    }

    pub fn log_snapshot(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    pub fn commands_snapshot(&self) -> Vec<ReviewCommand> {
        self.commands.borrow().clone()
    }
}

impl ShellInputHost for RecordingHost {
    fn has_images(&self) -> bool {
        self.has_images.get()
    }
    fn is_fullscreen(&self) -> bool {
        self.is_fullscreen.get()
    }
    fn is_compare_visible(&self) -> bool {
        self.is_compare_visible.get()
    }
    fn current_has_compare_pair(&self) -> bool {
        self.current_has_compare_pair.get()
    }
    fn current_has_capture_pair(&self) -> bool {
        self.current_has_capture_pair.get()
    }
    fn next(&self) {
        self.nexts.set(self.nexts.get() + 1);
        self.calls.borrow_mut().push("next".to_owned());
    }
    fn previous(&self) {
        self.previouses.set(self.previouses.get() + 1);
        self.calls.borrow_mut().push("previous".to_owned());
    }
    fn on_viewer_zoom_mode_changed(&self) {
        self.zoom_mode_changes.set(self.zoom_mode_changes.get() + 1);
        self.calls.borrow_mut().push("zoom_mode_changed".to_owned());
    }
    fn cancel_duplicate_check(&self) -> bool {
        if !self.duplicate_check_running.get() {
            return false;
        }
        self.duplicate_check_running.set(false);
        self.log.borrow_mut().push("CancelDuplicateCheck".to_owned());
        true
    }
    fn execute(&self, command: ReviewCommand) -> Result<(), Box<dyn Error>> {
        self.commands.borrow_mut().push(command);
        self.calls.borrow_mut().push(format!("execute:{:?}", command.kind));
        match self.execute_failure.borrow().as_ref() {
            Some(message) => Err(Box::new(HostFailure(message.clone()))),
            None => Ok(()),
        }
    }
    fn report_action_failed(&self, action_name: &str, error: &dyn Error) {
        self.log.borrow_mut().push(format!("failed:{action_name}:{error}"));
    }
    fn zoom_action_name(&self) -> String {
        "Zoom".to_owned()
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// QueueDispatcher
// ---------------------------------------------------------------------------------------------------------------------------------------------

enum Entry {
    Action(UiAction),
    /// A `!Send` job of the viewport's render poster: kept in the thread-local [`LOCAL_JOBS`] under this key.
    Local(u64),
}

thread_local! {
    static LOCAL_JOBS: RefCell<HashMap<u64, Box<dyn FnOnce()>>> = RefCell::new(HashMap::new());
    static ON_RENDER_POST: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

static NEXT_LOCAL_KEY: AtomicU64 = AtomicU64::new(1);

/// C# `QueueDispatcher`: a queue per priority (Send, Normal, Render, Background), run by [`QueueDispatcher::drain`]; FIFO inside a priority. The
/// viewport's render jobs (`!Send`) share the queue through [`RigPoster`], so a layout pass posted before a continuation runs before it.
pub struct QueueDispatcher {
    queue: Mutex<BTreeMap<(UiPriority, u64), Entry>>,
    seq: AtomicU64,
    posted: Mutex<Vec<UiPriority>>,
}

impl QueueDispatcher {
    pub fn new() -> Arc<QueueDispatcher> {
        Arc::new(QueueDispatcher {
            queue: Mutex::new(BTreeMap::new()),
            seq: AtomicU64::new(0),
            posted: Mutex::new(Vec::new()),
        })
    }

    /// C# `PostedPriorities`: the priority of every post made through `UiDispatcher` (not the viewport's render jobs).
    pub fn posted_priorities(&self) -> Vec<UiPriority> {
        self.posted.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Jobs waiting.
    pub fn pending(&self) -> usize {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    /// Drops every waiting job without running it.
    pub fn clear(&self) {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner).clear();
        LOCAL_JOBS.with(|jobs| jobs.borrow_mut().clear());
    }

    fn push(&self, priority: UiPriority, entry: Entry) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert((priority, seq), entry);
    }

    fn post_local(&self, job: Box<dyn FnOnce()>, priority: UiPriority) {
        let key = NEXT_LOCAL_KEY.fetch_add(1, Ordering::SeqCst);
        LOCAL_JOBS.with(|jobs| jobs.borrow_mut().insert(key, job));
        self.push(priority, Entry::Local(key));
    }

    /// Runs the best job (lowest priority value, then oldest); false when the queue is empty. `before` runs just before the job.
    pub fn run_one(&self, before: &dyn Fn()) -> bool {
        let next = {
            let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
            let key = queue.keys().next().copied();
            key.and_then(|key| queue.remove(&key))
        };
        let Some(entry) = next else { return false };
        before();
        match entry {
            Entry::Action(action) => action(),
            Entry::Local(key) => {
                if let Some(job) = LOCAL_JOBS.with(|jobs| jobs.borrow_mut().remove(&key)) {
                    job();
                }
            }
        }
        true
    }

    /// C# `Drain`: runs jobs until the queue is empty (a job may post more); bounded so a loop fails the test instead of hanging it.
    pub fn drain(&self, before: &dyn Fn()) {
        for _ in 0..10_000 {
            if !self.run_one(before) {
                return;
            }
        }
        panic!("the dispatcher queue did not settle after 10000 jobs");
    }
}

impl UiScheduler for QueueDispatcher {
    fn post(&self, action: UiAction) {
        self.post_with_priority(action, UiPriority::Normal);
    }

    fn invoke(&self, action: UiAction) -> UiCompletion {
        self.invoke_with_priority(action, UiPriority::Normal)
    }
}

impl UiDispatcher for QueueDispatcher {
    fn check_access(&self) -> bool {
        true
    }

    fn post_with_priority(&self, action: UiAction, priority: UiPriority) {
        self.posted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(priority);
        if priority == UiPriority::Render {
            // C# `Viewer.PropertyChanged += InvalidateLayout`: a Render post follows a viewer change, so the layout pass is queued before it.
            let hook = ON_RENDER_POST.with(|hook| hook.borrow().clone());
            if let Some(hook) = hook {
                hook();
            }
        }
        self.push(priority, Entry::Action(action));
    }

    fn invoke_with_priority(&self, action: UiAction, priority: UiPriority) -> UiCompletion {
        let (sender, completion) = UiCompletion::channel();
        self.post_with_priority(
            Box::new(move || {
                action();
                let _ = sender.send(Ok(()));
            }),
            priority,
        );
        completion
    }
}

/// The viewport's `RenderPoster` on the same queue (Render priority).
pub struct RigPoster(pub Arc<QueueDispatcher>);

impl RenderPoster for RigPoster {
    fn post_render(&self, job: Box<dyn FnOnce()>) {
        self.0.post_local(job, UiPriority::Render);
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// RigSurface (the C# ViewportImageSurface, which 14c ports)
// ---------------------------------------------------------------------------------------------------------------------------------------------

/// `IImageSurface` + `IFitSurface` + the frame router over a real `ViewportController`. `capture()` builds the Fit snapshot from the viewer (immutable
/// borrow, taken only for the four reads) and the controller, like `ViewportImageSurface.Capture`.
pub struct RigSurface {
    viewport: Rc<ViewportController>,
    viewer: ViewerHandle,
    rig_content: Rc<RigContent>,
    router: Rc<RefCell<Option<FrameRouter>>>,
}

impl RigSurface {
    /// A surface over `viewport` for the native tests (the window is real there).
    pub fn new(viewport: Rc<ViewportController>, viewer: ViewerHandle, rig_content: Rc<RigContent>) -> RigSurface {
        RigSurface {
            viewport,
            viewer,
            rig_content,
            router: Rc::new(RefCell::new(None)),
        }
    }
}

impl ImageSurface for RigSurface {
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
        self.viewport.viewport_width()
    }
    fn viewport_height(&self) -> f64 {
        self.viewport.viewport_height()
    }
    fn extent_width(&self) -> f64 {
        self.viewport.extent_width()
    }
    fn extent_height(&self) -> f64 {
        self.viewport.extent_height()
    }
    fn drag_threshold(&self) -> (f64, f64) {
        self.viewport.drag_threshold()
    }
    fn scroll_to(&mut self, horizontal: f64, vertical: f64) {
        self.viewport.scroll_to(horizontal, vertical);
    }
    fn update_layout(&mut self) {
        // NO content refresh here: this runs inside a pure call that holds the viewer (see the module doc).
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
    fn source_size(&self) -> Option<AppSizeD> {
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
    fn hook_render_frame(&mut self, mut handler: RenderFrameHandler) -> RenderHookId {
        // The router is called after the controller's own handler, once per frame (D-W8B4I-5). It is read at call time and cloned out of the cell first.
        let router = Rc::clone(&self.router);
        self.viewport.hook_render_frame(Box::new(move |ticks| {
            handler(ticks);
            let current = router.borrow().clone();
            if let Some(current) = current {
                current(ticks);
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

impl FitSurface for RigSurface {
    fn viewport_size(&self) -> (f64, f64) {
        self.viewport.viewport_size()
    }
    fn is_loaded(&self) -> bool {
        self.viewport.is_loaded()
    }
    fn update_layout(&mut self) {
        // The Fit path holds no viewer borrow (the adapter takes it only inside `reset_fit`), so the content may be re-read.
        self.rig_content.refresh(&self.viewer);
        self.viewport.update_layout();
    }
    fn update_fit_size(&mut self) {
        self.viewport.update_fit_size();
    }
    fn capture(&self) -> ViewportSnapshot {
        let layout = self.viewport.layout();
        let (zoom, stretch, max_width, max_height) = {
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
            max_image_width: max_width,
            max_image_height: max_height,
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

impl InputSurface for RigSurface {
    fn set_frame_router(&mut self, router: Option<FrameRouter>) {
        *self.router.borrow_mut() = router;
    }
}

/// The cached `ViewportContent` the controller's content callback returns.
pub struct RigContent {
    cache: Cell<ViewportContent>,
    pub bitmap: Cell<(f64, f64)>,
    /// Optional live host read, used to detect viewer guards held across viewport callbacks.
    pub before_read: RefCell<Option<Rc<dyn Fn()>>>,
    /// While set, [`RigContent::refresh`] does nothing (a pure call holds the viewer mutably and posts jobs; the cache stays as it was).
    pub suspended: Cell<bool>,
}

impl RigContent {
    /// A content cache for a bitmap of `bitmap` pixels, filled from `viewer`.
    pub fn new(viewer: &ViewerHandle, bitmap: (f64, f64)) -> Rc<RigContent> {
        let content = Rc::new(RigContent {
            cache: Cell::new(ViewportContent::empty()),
            bitmap: Cell::new(bitmap),
            before_read: RefCell::new(None),
            suspended: Cell::new(false),
        });
        content.refresh(viewer);
        content
    }

    /// The cached content (what the viewport's content callback returns).
    pub fn current(&self) -> ViewportContent {
        let before_read = self.before_read.borrow().clone();
        if let Some(before_read) = before_read {
            before_read();
        }
        self.cache.get()
    }

    /// C# `ContentFromViewer`: re-reads the viewer (never call this while a pure call holds it).
    pub fn refresh(&self, viewer: &ViewerHandle) {
        if self.suspended.get() {
            return;
        }
        let (bitmap_width, bitmap_height) = self.bitmap.get();
        let viewer = viewer.borrow();
        self.cache.set(ViewportContent {
            stretch: viewer.stretch(),
            image_width: viewer.image_width(),
            image_height: viewer.image_height(),
            max_image_width: viewer.max_image_width(),
            max_image_height: viewer.max_image_height(),
            bitmap_width,
            bitmap_height,
            is_fit: viewer.is_fit(),
        });
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// Rig
// ---------------------------------------------------------------------------------------------------------------------------------------------

/// Edits the settings before the rig is built.
pub type ConfigureFn = Box<dyn Fn(&mut AppSettings)>;

/// Construction options of [`Rig`] (the C# `InputRig` constructor arguments).
pub struct RigOptions {
    pub client_width: f64,
    pub client_height: f64,
    pub image_width: i32,
    pub image_height: i32,
    pub dpi_scale: f64,
    /// The DPI scale the WINDOW reports when it differs from the viewer's (C# `SizedShellWindow.DpiScale` is always 1.0 in the G-INPUT replay).
    pub window_dpi_scale: Option<f64>,
    pub configure: Option<ConfigureFn>,
}

impl Default for RigOptions {
    fn default() -> Self {
        Self {
            client_width: 1000.0,
            client_height: 600.0,
            image_width: 3000,
            image_height: 2000,
            dpi_scale: 1.0,
            window_dpi_scale: None,
            configure: None,
        }
    }
}

/// C# `InputRig`: window + viewport + input without an HWND.
pub struct Rig {
    pub settings: Rc<RefCell<Arc<AppSettings>>>,
    pub window: Rc<InputWindow>,
    pub dispatcher: Arc<QueueDispatcher>,
    pub clock: Rc<FakeHostClock>,
    pub host: Rc<RecordingHost>,
    pub viewer: ViewerHandle,
    pub viewport: Rc<ViewportController>,
    pub log: Arc<RecordingLog>,
    pub input: ShellInput,
    pub modifiers: Rc<Cell<KeyModifiers>>,
    pub message_time: Rc<Cell<i32>>,
    pub tick_count: Rc<Cell<i32>>,
    pub wheel_device: Rc<Cell<WheelDeviceHint>>,
    pub timestamp: Rc<Cell<i64>>,
    /// The cursor position in screen pixels the viewport reads (`GetCursorPos`); `None` = unreadable.
    pub mouse: Rc<Cell<Option<PointD>>>,
    pub content: Rc<RigContent>,
    /// The surface handed to `ShellInput` (shared, so a test can read it too).
    pub surface: Rc<RefCell<RigSurface>>,
}

impl Rig {
    pub fn new() -> Rig {
        Rig::with(RigOptions::default())
    }

    pub fn configured(configure: impl Fn(&mut AppSettings) + 'static) -> Rig {
        Rig::with(RigOptions {
            configure: Some(Box::new(configure)),
            ..RigOptions::default()
        })
    }

    pub fn with(options: RigOptions) -> Rig {
        let mut settings = AppSettings::default();
        if let Some(configure) = &options.configure {
            configure(&mut settings);
        }
        let settings = Rc::new(RefCell::new(Arc::new(settings)));
        let window = InputWindow::new(
            options.client_width,
            options.client_height,
            options.window_dpi_scale.unwrap_or(options.dpi_scale),
        );
        let dispatcher = QueueDispatcher::new();
        let clock = FakeHostClock::new();
        let host = RecordingHost::new();
        let log = Arc::new(RecordingLog::new());

        let mut viewer_state = ViewerState::new();
        let _ = viewer_state.set_dpi_scale(options.dpi_scale);
        viewer_state.set_zoom_step(f64::from(settings.borrow().keyboard_zoom_step_percent) / 100.0);
        let _ = viewer_state.set_source_size(options.image_width, options.image_height, true);
        let viewer = ViewerHandle::new(viewer_state);

        let content = Rc::new(RigContent {
            cache: Cell::new(ViewportContent::empty()),
            bitmap: Cell::new((f64::from(options.image_width), f64::from(options.image_height))),
            before_read: RefCell::new(None),
            suspended: Cell::new(false),
        });
        content.refresh(&viewer);

        let timestamp = Rc::new(Cell::new(0_i64));
        let mouse: Rc<Cell<Option<PointD>>> = Rc::new(Cell::new(None));
        let mouse_for_viewport = Rc::clone(&mouse);
        let content_for_viewport = Rc::clone(&content);
        let window_for_fit = Rc::clone(&window);
        let viewer_for_fit = viewer.clone();
        let timestamp_for_viewport = Rc::clone(&timestamp);
        let viewport = ViewportController::new(
            Rc::clone(&window) as Rc<dyn ShellWindowApi>,
            Rc::new(RigPoster(Arc::clone(&dispatcher))),
            Rc::clone(&clock) as Rc<dyn pr_platform_win::host::frame_clock::base::HostFrameClock>,
            Rc::new(move || content_for_viewport.current()),
            Rc::new(move || {
                // C# `UpdateFitSize`: `Viewer.UpdateViewport(ViewportSize)`.
                let size = window_for_fit.client_size.get();
                let sane = |value: f64| if value.is_finite() && value > 0.0 { value } else { 0.0 };
                let _ = viewer_for_fit
                    .borrow_mut()
                    .update_viewport(sane(size.width), sane(size.height), false);
            }),
            ViewportControllerOptions {
                drag_metrics_pixels: Box::new(|| (4, 4)),
                timestamp: Box::new(move || timestamp_for_viewport.get()),
                cursor_screen_pixel: Box::new(move || mouse_for_viewport.get()),
                ..ViewportControllerOptions::default()
            },
        );

        let surface = Rc::new(RefCell::new(RigSurface {
            viewport: Rc::clone(&viewport),
            viewer: viewer.clone(),
            rig_content: Rc::clone(&content),
            router: Rc::new(RefCell::new(None)),
        }));

        let modifiers = Rc::new(Cell::new(KeyModifiers::NONE));
        let message_time = Rc::new(Cell::new(0_i32));
        let tick_count = Rc::new(Cell::new(0_i32));
        let wheel_device = Rc::new(Cell::new(WheelDeviceHint::Unknown));
        let environment = ShellInputEnvironment {
            modifiers: Box::new({
                let modifiers = Rc::clone(&modifiers);
                move || modifiers.get()
            }),
            message_time: Box::new({
                let message_time = Rc::clone(&message_time);
                move || message_time.get()
            }),
            tick_count: Box::new({
                let tick_count = Rc::clone(&tick_count);
                move || tick_count.get()
            }),
            wheel_device: Box::new({
                let wheel_device = Rc::clone(&wheel_device);
                move || wheel_device.get()
            }),
            double_click_time_ms: Box::new(|| 500),
            double_click_size_pixels: Box::new(|_| (4, 4)),
        };

        let settings_for_input = Rc::clone(&settings);
        let input = ShellInput::new(ShellInputDeps {
            window: Rc::clone(&window) as Rc<dyn ShellWindowApi>,
            surface: Rc::clone(&surface) as Rc<RefCell<dyn InputSurface>>,
            viewer: viewer.clone(),
            settings: Rc::new(move || Arc::clone(&settings_for_input.borrow())),
            host: Rc::clone(&host) as Rc<dyn ShellInputHost>,
            dispatcher: Arc::clone(&dispatcher) as Arc<dyn UiDispatcher>,
            environment: Rc::new(environment),
            log: Arc::clone(&log) as Arc<dyn pr_core::abstractions::Log>,
            ticks_per_ms: pr_core::diagnostics::perf::STOPWATCH_FREQUENCY as f64 / 1000.0,
        });

        // C# `Viewer.PropertyChanged += InvalidateLayout`: a Render-priority post (a continuation) follows a viewer change.
        let viewport_for_hook = Rc::clone(&viewport);
        let content_for_hook = Rc::clone(&content);
        let viewer_for_hook = viewer.clone();
        ON_RENDER_POST.with(|hook| {
            *hook.borrow_mut() = Some(Rc::new(move || {
                content_for_hook.refresh(&viewer_for_hook);
                viewport_for_hook.invalidate_layout();
            }));
        });

        let rig = Rig {
            settings,
            window,
            dispatcher,
            clock,
            host,
            viewer,
            viewport,
            log,
            input,
            modifiers,
            message_time,
            tick_count,
            wheel_device,
            timestamp,
            mouse,
            content,
            surface,
        };
        // C# `Viewer.PropertyChanged += InvalidateLayout`: one more pass lets the Fit size converge.
        rig.viewport.invalidate_layout();
        rig.drain();
        rig
    }

    /// Replaces the settings object (a NEW `Arc`, like `SettingsSource::begin_edit`) after applying `edit` to a copy.
    pub fn update_settings(&self, edit: impl FnOnce(&mut AppSettings)) {
        let mut copy = (**self.settings.borrow()).clone();
        edit(&mut copy);
        *self.settings.borrow_mut() = Arc::new(copy);
    }

    /// C# `ContentFromViewer`: re-reads the viewer into the viewport's content cache.
    pub fn refresh_content(&self) {
        self.content.refresh(&self.viewer);
    }

    /// C# `Drain`: the layout pass and the continuations, until the queue is empty. The content is refreshed before every job.
    pub fn drain(&self) {
        self.refresh_content();
        self.dispatcher.drain(&|| self.refresh_content());
    }

    /// C# `Fit()`: Fit through the input, until it converged.
    pub fn fit(&self) {
        self.input.apply_fit();
        self.drain();
    }

    /// C# `Send`: runs the message through the real handlers, then drains.
    pub fn send(&self, msg: u32, wparam: usize, lparam: Lparam) -> (bool, Lresult) {
        let outcome = self.window.dispatch(msg, wparam, lparam);
        self.drain();
        outcome
    }

    /// C# `Pack(x, y)`: `MAKELPARAM` of two signed 16-bit coordinates.
    pub fn pack(x: i32, y: i32) -> Lparam {
        ((((y as u16) as usize) << 16) | ((x as u16) as usize)) as Lparam
    }

    /// C# `Mouse`.
    pub fn mouse(&self, msg: u32, x: i32, y: i32, key_flags: usize) -> (bool, Lresult) {
        self.send(msg, key_flags, Rig::pack(x, y))
    }

    /// C# `Wheel`: `wParam` = delta in the high word, `MK_CONTROL` (8) in the low word; `lParam` = SCREEN pixels.
    pub fn wheel(&self, msg: u32, delta: i32, x: i32, y: i32, ctrl: bool) -> (bool, Lresult) {
        let wparam = (((delta as u16) as usize) << 16) | if ctrl { 0x0008 } else { 0 };
        self.send(msg, wparam, Rig::pack(x, y))
    }

    /// C# `KeyDown(virtualKey, ...)`.
    pub fn key_down_vk(
        &self,
        virtual_key: i32,
        repeat: bool,
        system: bool,
        extended: bool,
        scan_code: u32,
    ) -> (bool, Lresult) {
        let mut lparam: usize = 1 | ((scan_code as usize) << 16);
        if repeat {
            lparam |= 0x4000_0000;
        }
        if extended {
            lparam |= 0x0100_0000;
        }
        self.send(
            if system { WM_SYSKEYDOWN } else { WM_KEYDOWN },
            virtual_key as usize,
            lparam as Lparam,
        )
    }

    /// C# `KeyDown(KeyId, repeat, system)`.
    pub fn key_down(&self, key: KeyId, repeat: bool, system: bool) -> (bool, Lresult) {
        self.key_down_vk(key_mapping::to_virtual_key(key), repeat, system, false, 0)
    }

    /// The viewport's current offsets.
    pub fn offsets(&self) -> (f64, f64) {
        (self.viewport.horizontal_offset(), self.viewport.vertical_offset())
    }

    /// `ShellInput::dispose` then the viewport (C# `Dispose`).
    pub fn dispose(&self) {
        self.input.dispose();
        self.viewport.dispose();
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        // The thread-local hook captures this rig's viewport: never leave it for the next test on the same thread.
        ON_RENDER_POST.with(|hook| *hook.borrow_mut() = None);
        LOCAL_JOBS.with(|jobs| jobs.borrow_mut().clear());
    }
}
