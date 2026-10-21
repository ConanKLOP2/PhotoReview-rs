//! Port of `PhotoReview.Shell.Win32/ShellMainWindow.cs` lines 1-226 (baseline `d92d6aae`): the options record, the constants, the fields, the constructor,
//! `RegisterFeatures`, the plain properties, `AttachRenderer` and `OpenInitialAsync`. Unit W8-13g. Contract: `contracts/W8-13g.md`.
//!
//! This file declares [`MainWindowInner`] with ALL fields of all the `ShellMainWindow` partials of units 13g..13k (the per-feature state lives in a small
//! struct owned by the feature file: `FrameState`, `ScrollBarState`, `ViewingState`, `WindowingState`); the feature files add `impl MainWindowInner`
//! blocks. Features of OTHER units (input 14c, menus 14f, overlay 15j, keep-warm 16d) plug in through [`MainWindowFeature`] and [`FeatureOptions`]
//! and use the `pub(crate)` accessors below (D-W8B4M-3, -5).
//!
//! Interim seams (D-W8B4M-4, narrowed by the B4 reconciliation D-W8B4X-2): the collaborators of units that are ported AFTER this one stay behind small traits
//! ([`PresentationHubPort`] for `ShellPresentationHub` 16i, [`StartupCloakPort`] and [`PlacementSavePort`] for `StartupCloak` and `ShellPlacement.Save` 17b). The
//! collaborators of units ported BEFORE this one (16h `ViewportController`, 16d `KeepWarmSetup`) are used as the concrete types (D-W8B4X-2).

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::cmp::{Ordering as CmpOrdering, Reverse};
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use pr_app::services::viewport_size_source::ViewportSizeSource;
use pr_app::viewmodels::main::MainViewModel;
use pr_app::viewmodels::main::ViewerHandle;
use pr_app::viewport::render_geometry::{ColorF, color_from_rgb};
use pr_app::windowing::contracts::WindowShowState;
use pr_core::abstractions::{Log, UiDispatcher, UiPriority};
use pr_core::diagnostics::perf::{ShellPerfMarks, startup_mark};
use pr_imaging::decode_types::PlatformImage;
use pr_imaging::pixels::PixelBuffer;

use super::frame::FrameState;
use super::scrollbars::ScrollBarState;
use super::viewing::ViewingState;
use super::windowing::{ShellWindowingSeams, WindowingState};
use crate::host::contracts::{EventHandlerId, ShellWindowApi, ShellWindowEvent};
use crate::host::error::HostError;
use crate::host::frame_clock::base::HostFrameClock;
use crate::host::shell_window::ShellWindow;
use crate::host::ui_dispatcher::Win32UiDispatcher;
use crate::render::device::RenderFault;
use crate::render::image_cache::policy::{StripeScheduler, StripeToken};
use crate::render::image_cache::upload::{D2dGpuImageCache, ResourcesBitmapDevice};
use crate::render::surface::{D2dRenderSurface, SurfaceOptions};
use crate::shell_viewing::keep_warm::{KeepWarmRegistration, KeepWarmSetup, KeepWarmWindow, register_keep_warm};
use crate::shell_viewing::presentation_sink::ImageChangedHandler;
use crate::shell_viewing::viewport_controller::{RenderPoster, ViewportController};
use crate::win32::structs::Hwnd;

/// C# `DefaultWidthDip`: the size (DIP) before there is a saved placement (`MainWindow.xaml` Width).
pub const DEFAULT_WIDTH_DIP: f64 = 1200.0;
/// C# `DefaultHeightDip`.
pub const DEFAULT_HEIGHT_DIP: f64 = 800.0;
/// C# `PreviewQualityMultiplier`: images are decoded for the screen at the machine dpi with 15 % to spare.
pub const PREVIEW_QUALITY_MULTIPLIER: f64 = 1.15;
/// C# `FallbackViewportWidth`: the decode box uses this while the viewport is not measured (width <= 1).
pub(super) const FALLBACK_VIEWPORT_WIDTH: f64 = 2200.0;
/// C# `FallbackViewportHeight`.
pub(super) const FALLBACK_VIEWPORT_HEIGHT: f64 = 1400.0;
/// `ObjectName` of the `ObjectDisposedException` the C# window throws.
const DISPOSED_OBJECT_NAME: &str = "PhotoReview.Shell.Win32.ShellMainWindow";

/// C# `CanvasColor` = `Dark.Canvas` #101010 (`new ColorF(0x10 / 255f, 0x10 / 255f, 0x10 / 255f, 1f)`, f32 division).
pub fn canvas_color() -> ColorF {
    color_from_rgb(0x10, 0x10, 0x10, 0xFF)
}

/// C# `Action<string> mark` (`PhotoReviewPerf.StartupMark`): records a startup phase by name.
pub type StartupMark = Box<dyn Fn(&str)>;

/// The view model as the window and its features share it. The C# object was a plain reference; `MainViewModel` has `&mut self` methods (D-W8-1), so
/// the shared handle is `Rc<RefCell<..>>`. RULE (D-W8B4M-2): a borrow is never held across a call that can render, close or re-enter; use
/// [`MainWindow::update_view_model`] to run a command and forward its `ChangeSet` with the borrow already released.
pub type SharedViewModel = Rc<RefCell<MainViewModel>>;

/// Identifies one live main window in the UI-thread registry (D-W8B4M-6). Posted `Send` actions carry the id, never an `Rc`.
pub type MainWindowId = u64;

// ---- options -------------------------------------------------------------------------------------------------------------------------------------

/// Typed bag for the option records that other units own (`SettingsStore`, `IZoomPromptService`, `ShellInputEnvironment` of 14c; `KeepWarmOptions`,
/// `KeepWarmWork`, `FrameLogPath`, `KeepWarmTimestamp` of 16d). C# had one record with all of them; the owning unit inserts ONE value of its own type and
/// reads it back in its `register_*` (D-W8B4M-5). At most one value per type.
#[derive(Default)]
pub struct FeatureOptions {
    map: RefCell<HashMap<TypeId, Rc<dyn Any>>>,
}

impl fmt::Debug for FeatureOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FeatureOptions")
            .field("entries", &self.map.borrow().len())
            .finish()
    }
}

impl FeatureOptions {
    /// Stores `value` (replacing an earlier value of the same type).
    pub fn insert<T: Any>(&mut self, value: T) {
        self.map.get_mut().insert(TypeId::of::<T>(), Rc::new(value));
    }

    /// The value of type `T`, if one was inserted (shared, `Rc`).
    pub fn get<T: Any>(&self) -> Option<Rc<T>> {
        let entry = self.map.borrow().get(&TypeId::of::<T>()).cloned()?;
        entry.downcast::<T>().ok()
    }

    /// Removes and returns the value of type `T` (D-W8B4X-5): for one-shot values that cannot be shared or cloned (16d `KeepWarmRegistration` holds a
    /// `Box<dyn FnOnce>`). `None` when no value was inserted, when it was taken already, or when an `Rc` handed out by [`get`](Self::get) is still alive.
    pub fn take<T: Any>(&self) -> Option<T> {
        let mut map = self.map.borrow_mut();
        // A shared entry stays where it is: the caller of `get` still holds it.
        if map
            .get(&TypeId::of::<T>())
            .is_none_or(|entry| Rc::strong_count(entry) != 1)
        {
            return None;
        }
        let entry = map.remove(&TypeId::of::<T>())?;
        drop(map);
        Rc::try_unwrap(entry.downcast::<T>().ok()?).ok()
    }
}

/// C# `ShellMainWindowOptions` (the part owned by 13g..13k; defaults = the real run, tests change one field).
pub struct MainWindowOptions {
    /// `PreferWarp` (env `PHOTOREVIEW_SHELL_WARP=1` is read by the startup code): WARP instead of the GPU.
    pub prefer_warp: bool,
    /// `PlacementFile` (window-placement.json, written on close); `None` = do not save (smoke, tests).
    pub placement_file: Option<PathBuf>,
    /// `Activate`: take the focus when shown (smoke and tests: false). Also gates the launch keep-warm in `on_frame_presented`.
    pub activate: bool,
    /// `Windowing` (W8-13k): the F11/monitor/DWM seams; `None` = real Win32.
    pub windowing: Option<ShellWindowingSeams>,
    /// Options of the units 14c and 16d (see [`FeatureOptions`]).
    pub extensions: FeatureOptions,
}

impl fmt::Debug for MainWindowOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainWindowOptions")
            .field("prefer_warp", &self.prefer_warp)
            .field("placement_file", &self.placement_file)
            .field("activate", &self.activate)
            .finish_non_exhaustive()
    }
}

impl Default for MainWindowOptions {
    /// `PreferWarp = false`, `PlacementFile = None`, `Activate = true`, no seams, no extensions.
    fn default() -> Self {
        Self {
            prefer_warp: false,
            placement_file: None,
            activate: true,
            windowing: None,
            extensions: FeatureOptions::default(),
        }
    }
}

// ---- seams to other units' types ---------------------------------------------------------------------------------------------------------------

/// `ViewportContent` is owned by 16h (`shell_viewing::viewport_controller`); the window only re-exports it (D-W8B4X-2). The interim twin and the
/// `MainViewport` trait of the first 13g draft are gone: 16h is ported BEFORE this unit, so the window holds the concrete `Rc<ViewportController>`.
pub use crate::shell_viewing::viewport_controller::ViewportContent;

/// What the composition root's factory gets to build the `ViewportController`: the window collaborators it needs and the two window-owned callbacks of the C#
/// constructor (`ContentFromViewer`, `UpdateFitSize`).
pub struct ViewportCallbacks {
    /// The native window (`Rc<ShellWindow>` coerced).
    pub window: Rc<dyn ShellWindowApi>,
    /// The frame clock of the window.
    pub frame_clock: Rc<dyn HostFrameClock>,
    /// The window's poster (`UiLocalPoster` implements 16h's `RenderPoster`).
    pub poster: UiLocalPoster,
    /// C# `ContentFromViewer`.
    pub content: Rc<dyn Fn() -> ViewportContent>,
    /// C# `UpdateFitSize`.
    pub update_fit_size: Rc<dyn Fn()>,
}

impl fmt::Debug for ViewportCallbacks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewportCallbacks").finish_non_exhaustive()
    }
}

/// Builds the viewport controller from the window callbacks. The composition root writes it as
/// `|cb| ViewportController::new(cb.window, Rc::new(cb.poster), cb.frame_clock, cb.content, cb.update_fit_size, options_with(create_viewport_input_hooks(..)))`
/// (the C# constructor called `CreateViewportOptions` of `ShellMainWindow.Input.cs` = 14c itself; the factory removes the edge 13g -> 14c, D-W8B4M-4).
/// The factory must leave `ViewportControllerOptions::scroll_bar_thickness` at its default (D-W8B4X-7).
pub type ViewportFactory = Box<dyn FnOnce(ViewportCallbacks) -> Rc<ViewportController>>;

/// A hub image handler: the new image (`None` = empty folder or compare) and C# `isFileChange`. The type is 16i's `ImageChangedHandler` (D-W8B4X-2).
pub type HubImageHandler = ImageChangedHandler;

/// `ShellPresentationHub` (W8-16i, `shell_viewing::presentation_sink`), the members the window calls. INTERIM trait (16i implements it for the hub, or the lead
/// replaces `Rc<dyn PresentationHubPort>` by `Rc<ShellPresentationHub>`). The hub calls `ImageChanged` handlers INSIDE the view model's sink replay, i.e. while
/// the view model is borrowed: the window's handler therefore only QUEUES the image and `apply_changes` handles it after the borrow is gone (D-W8B4M-7).
pub trait PresentationHubPort {
    /// C# `CurrentImage`.
    fn current_image(&self) -> Option<PlatformImage>;
    /// C# `ImageChanged += handler`; the id unsubscribes.
    fn subscribe_image_changed(&self, handler: HubImageHandler) -> u64;
    /// C# `ImageChanged -= handler`.
    fn unsubscribe_image_changed(&self, id: u64);
    /// C# `NoteFramePresented(containedImage)`.
    fn note_frame_presented(&self, contained_image: bool);
}

/// `StartupCloak` (W8-17b): the two members the window calls.
pub trait StartupCloakPort {
    /// C# `Stop()`: idempotent; the safety deadline must not keep the window alive.
    fn stop(&self);
    /// C# `NoteFirstPresent()`: reveal the window.
    fn note_first_present(&self);
}

/// `ShellPlacement.Save(hwnd, file, store, native, restoreState, log, restoreBounds)` (W8-17b) with the store, the native placement and the log bound.
pub trait PlacementSavePort {
    /// `true` when the placement was written. `restore_state`/`restore_bounds` are the state and Normal rect from BEFORE F11 (R7-10), `None` otherwise.
    fn save(
        &self,
        hwnd: Hwnd,
        file: &Path,
        restore_state: Option<WindowShowState>,
        restore_bounds: Option<(i32, i32, i32, i32)>,
    ) -> bool;
}

/// A unit that plugs into the window (C# `partial class` members for input, menus, overlay, keep-warm). Registered with
/// [`MainWindow::add_feature`]. Every hook defaults to nothing. Order: `release_renderer` runs first in `release_renderer()`, `dispose` at the place of
/// C# `DisposeOverlay()` in `Dispose()`, both in reverse registration order; `draw_overlay`, `on_changes`, `on_current_index_changed` and
/// `on_scroll_bar_press` run in registration order.
pub trait MainWindowFeature {
    /// Name for logs.
    fn name(&self) -> &'static str;
    /// C# `DisposeKeepWarm()`: stop threads that own graphics devices (called from `release_renderer`, window still alive).
    fn release_renderer(&self) {}
    /// C# `DisposeOverlay()`: unhook everything that touches the view model or the window (called from `dispose`).
    fn dispose(&self) {}
    /// C# `DrawOverlay(context)` (15j): the HUD, drawn after the scroll bars in the SAME frame.
    fn draw_overlay(&self, _context: &mut dyn pr_app::services::render::DrawContext) {}
    /// Called by `apply_changes` after the window's own handling, with the whole change set (overlay refresh, menus).
    fn on_changes(&self, _changes: &pr_app::viewmodels::main::ChangeSet) {}
    /// C# `_onViewModelChangedForInput` for `CurrentIndex` (14c: `NoteCurrentIndexChanged`).
    fn on_current_index_changed(&self, _index: i32) {}
    /// C# `onPress: () => _input?.Pointer.OnWindowPreviewMouseDown()` (14c): a press on a scroll bar.
    fn on_scroll_bar_press(&self) {}
    /// Down-cast support (the owning unit finds its own object again).
    fn as_any(&self) -> &dyn Any;
}

/// C# `IRenderSurfaceFactoryHook`: where `attach_renderer` gets its surface (tests: WARP offscreen or a faulting factory). The concrete surface type is
/// needed (the window calls `resources()`, `begin_draw_context`), so this is not `pr_app`'s `RenderSurfaceFactory`.
pub trait RenderSurfaceFactoryHook {
    /// C# `Create(hwnd, options)`.
    fn create(&self, hwnd: Hwnd, options: SurfaceOptions) -> Result<D2dRenderSurface, RenderFault>;
}

/// The GPU image cache of the window: the budgeted policy cache over the Direct2D backend (W8-12f/12g).
pub type WindowImageCache = D2dGpuImageCache<ResourcesBitmapDevice>;

// ---- errors -----------------------------------------------------------------------------------------------------------------------------------------

/// What the window's own calls can fail with (C# `ObjectDisposedException`, `InvalidOperationException`, and the surface creation errors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MainWindowError {
    /// C# `ObjectDisposedException.ThrowIf(_disposed, this)`.
    Disposed,
    /// `"The renderer is already attached."`.
    RendererAlreadyAttached,
    /// `"The main window is already destroyed."` (`IForwardedOpenTarget.Activate`).
    WindowDestroyed,
    /// `"Scroll bars are not registered."`, `"Input is not registered."`-style access to a feature that is not there.
    FeatureNotRegistered { feature: &'static str },
    /// The surface (`CreateWindowSurface` or the hook) failed: C# lets the `COMException`/`InvalidOperationException` escape from `AttachRenderer`.
    Render(RenderFault),
    /// A host call failed.
    Host(HostError),
}

impl fmt::Display for MainWindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disposed => write!(
                f,
                "Cannot access a disposed object.\r\nObject name: '{DISPOSED_OBJECT_NAME}'."
            ),
            Self::RendererAlreadyAttached => f.write_str("The renderer is already attached."),
            Self::WindowDestroyed => f.write_str("The main window is already destroyed."),
            Self::FeatureNotRegistered { feature } => {
                // C# texts: "Scroll bars are not registered.", "Input is not registered.", "The context menu is not registered.".
                let verb = if feature.ends_with('s') { "are" } else { "is" };
                write!(f, "{feature} {verb} not registered.")
            }
            Self::Render(fault) => fmt::Display::fmt(fault, f),
            Self::Host(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl std::error::Error for MainWindowError {}

impl From<RenderFault> for MainWindowError {
    fn from(fault: RenderFault) -> Self {
        Self::Render(fault)
    }
}

impl From<HostError> for MainWindowError {
    fn from(error: HostError) -> Self {
        Self::Host(error)
    }
}

// ---- construction arguments --------------------------------------------------------------------------------------------------------------------

/// The collaborators of the C# constructor `ShellMainWindow(window, dispatcher, frameClock, viewModel, sizeSource, hub, cloak, placementStore,
/// placementNative, displayClock, log, mark, options)` (13 positional parameters; `displayClock`, `placementStore` and `placementNative` moved into the
/// `viewport_factory` and the `placement` port). None can be null in Rust, so the `ArgumentNullException` checks have no twin.
pub struct MainWindowDeps {
    /// The native window (C# `ShellWindow`); the main window disposes it last, in `dispose`.
    pub window: Rc<ShellWindow>,
    /// The UI-thread dispatcher (C# `Win32UiDispatcher`); a cheap clone.
    pub dispatcher: Win32UiDispatcher,
    /// The frame clock of the window (C# `IFrameClock frameClock`), also given to the composer.
    pub frame_clock: Rc<dyn HostFrameClock>,
    /// The view model (shared with the composition root and the other features).
    pub view_model: SharedViewModel,
    /// Where the decode box and the viewport size are published for worker threads.
    pub size_source: Arc<ViewportSizeSource>,
    /// C# `ShellPresentationHub`.
    pub hub: Rc<dyn PresentationHubPort>,
    /// C# `StartupCloak`.
    pub cloak: Rc<dyn StartupCloakPort>,
    /// C# `ShellPlacement.Save` with its store, native and log.
    pub placement: Rc<dyn PlacementSavePort>,
    /// C# `ILog`.
    pub log: Arc<dyn Log>,
    /// C# `mark` (`PhotoReviewPerf.StartupMark` by default): `None` = `pr_core::diagnostics::perf::startup_mark`.
    pub mark: Option<StartupMark>,
    /// Builds the `ViewportController` (see [`ViewportFactory`]).
    pub viewport_factory: ViewportFactory,
    /// C# `ShellMainWindowOptions`.
    pub options: MainWindowOptions,
}

impl fmt::Debug for MainWindowDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainWindowDeps").finish_non_exhaustive()
    }
}

// ---- posting UI-thread closures -------------------------------------------------------------------------------------------------------------------

/// A closure for the UI thread that is not `Send` (it reaches `Rc` state).
pub type LocalJob = Box<dyn FnOnce()>;

/// Posts [`LocalJob`]s to the UI thread. The dispatcher only takes `Send` actions, so non-`Send` closures wait in a thread-local FIFO per priority and ONE
/// `Send` wake job per queued closure runs them (the idea of `StripeScheduler`, D-W8B3S-2). Units 15j (`LocalPoster`), 16c (`ScheduleOnUi`), 16h (`RenderPoster`)
/// and 16d (`PrefetchHost`) each declared a trait of this shape; one implementation serves them all (D-W8B4M-11).
pub trait LocalPoster {
    /// Runs `job` on the UI thread after the current message, ordered by `priority`; never inline.
    fn post(&self, priority: UiPriority, job: LocalJob);
}

/// Process-wide window id counter (`MainWindowId`).
static NEXT_WINDOW_ID: AtomicU64 = AtomicU64::new(1);
/// Token of a delayed local job.
static NEXT_DELAYED_TOKEN: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// The UI thread's non-`Send` jobs, one FIFO per priority slot (see [`priority_slot`]).
    static LOCAL_JOBS: RefCell<[VecDeque<LocalJob>; 4]> =
        const { RefCell::new([VecDeque::new(), VecDeque::new(), VecDeque::new(), VecDeque::new()]) };
    /// The UI thread's delayed jobs, waiting for their timer (token -> job).
    static DELAYED_JOBS: RefCell<HashMap<u64, LocalJob>> = RefCell::new(HashMap::new());
    /// The live main windows of this thread, oldest first (D-W8B4M-6).
    static LIVE: RefCell<Vec<(MainWindowId, Weak<MainWindowInner>)>> = const { RefCell::new(Vec::new()) };
}

/// Index of `priority` in `LOCAL_JOBS`.
fn priority_slot(priority: UiPriority) -> usize {
    match priority {
        UiPriority::Send => 0,
        UiPriority::Normal => 1,
        UiPriority::Render => 2,
        UiPriority::Background => 3,
    }
}

/// The wake job of one queued local job: runs the OLDEST job of `priority` (FIFO inside a priority).
fn run_next_local_job(priority: UiPriority) {
    // The job runs with no borrow held (it may post more jobs).
    let job = LOCAL_JOBS
        .try_with(|queues| queues.borrow_mut()[priority_slot(priority)].pop_front())
        .ok()
        .flatten();
    if let Some(job) = job {
        job();
    }
}

/// The wake job of a delayed local job.
fn run_delayed_job(token: u64) {
    let job = DELAYED_JOBS
        .try_with(|jobs| jobs.borrow_mut().remove(&token))
        .ok()
        .flatten();
    if let Some(job) = job {
        job();
    }
}

/// One entry of the timer thread's queue: ordered by due time, then by arrival.
struct TimerEntry {
    due: Instant,
    seq: u64,
    action: Box<dyn FnOnce() + Send>,
}

impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.due == other.due && self.seq == other.seq
    }
}
impl Eq for TimerEntry {}
impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}
impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        self.due.cmp(&other.due).then(self.seq.cmp(&other.seq))
    }
}

/// The process-wide timer thread behind [`UiLocalPoster::post_after`]: one thread, a heap of due times and a condition variable (a bounded wait, no
/// polling). Its actions are `Send` closures that only post a wake job; no `Rc` ever reaches this thread.
struct TimerQueue {
    heap: Mutex<BinaryHeap<Reverse<TimerEntry>>>,
    changed: Condvar,
    next_seq: AtomicU64,
}

impl TimerQueue {
    fn schedule(&self, delay: Duration, action: Box<dyn FnOnce() + Send>) {
        let entry = TimerEntry {
            due: Instant::now() + delay,
            seq: self.next_seq.fetch_add(1, Ordering::Relaxed),
            action,
        };
        self.heap
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Reverse(entry));
        self.changed.notify_one();
    }

    fn run(&self) {
        let mut heap = self.heap.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            let now = Instant::now();
            match heap.peek().map(|entry| entry.0.due) {
                None => heap = self.changed.wait(heap).unwrap_or_else(PoisonError::into_inner),
                Some(due) if due <= now => {
                    if let Some(Reverse(entry)) = heap.pop() {
                        drop(heap);
                        (entry.action)();
                        heap = self.heap.lock().unwrap_or_else(PoisonError::into_inner);
                    }
                }
                Some(due) => {
                    heap = self
                        .changed
                        .wait_timeout(heap, due - now)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
            }
        }
    }
}

/// The timer queue, with its thread started on first use.
fn timer_queue() -> &'static TimerQueue {
    static QUEUE: OnceLock<&'static TimerQueue> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let queue: &'static TimerQueue = Box::leak(Box::new(TimerQueue {
            heap: Mutex::new(BinaryHeap::new()),
            changed: Condvar::new(),
            next_seq: AtomicU64::new(0),
        }));
        // A failure to start the thread leaves delayed jobs unrun (later `post_after` calls still queue): there is no better answer on a UI thread.
        let _ = std::thread::Builder::new()
            .name("PhotoReviewRs UI timer".to_owned())
            .spawn(move || queue.run());
        queue
    })
}

/// The one implementation of [`LocalPoster`] over a [`Win32UiDispatcher`]. `Clone`, `!Send`. FIFO inside one priority. A job that panics unwinds into the
/// dispatcher's action runner (which catches and logs it, D-W8B3H-10); the jobs behind it still run (each wake job pops one job).
#[derive(Clone)]
pub struct UiLocalPoster {
    dispatcher: Win32UiDispatcher,
}

impl fmt::Debug for UiLocalPoster {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UiLocalPoster").finish_non_exhaustive()
    }
}

impl UiLocalPoster {
    /// A poster over `dispatcher`; must be created on the UI thread (it uses the thread-local queues of that thread).
    pub fn new(dispatcher: Win32UiDispatcher) -> Self {
        Self { dispatcher }
    }

    /// `post` after `delay`: a timer thread waits (a bounded channel wait, no polling) and then posts a `Send` wake job; never blocks the UI thread. A window
    /// that is gone before the delay ends makes the job a no-op.
    pub fn post_after(&self, delay: std::time::Duration, priority: UiPriority, job: LocalJob) {
        let token = NEXT_DELAYED_TOKEN.fetch_add(1, Ordering::Relaxed);
        DELAYED_JOBS.with(|jobs| jobs.borrow_mut().insert(token, job));
        let dispatcher = self.dispatcher.clone();
        timer_queue().schedule(
            delay,
            Box::new(move || {
                // Only a token crosses threads: the job itself stays in the UI thread's table.
                dispatcher.post_with_priority(Box::new(move || run_delayed_job(token)), priority);
            }),
        );
    }
}

impl LocalPoster for UiLocalPoster {
    fn post(&self, priority: UiPriority, job: LocalJob) {
        LOCAL_JOBS.with(|queues| queues.borrow_mut()[priority_slot(priority)].push_back(job));
        self.dispatcher
            .post_with_priority(Box::new(move || run_next_local_job(priority)), priority);
    }
}

/// 16h's `RenderPoster` seam (D-W8B4X-6): `post_render(job)` = `self.post(UiPriority::Render, job)`. The thread-local queues are per thread, so every clone of the
/// poster is FIFO-ordered with the others.
impl RenderPoster for UiLocalPoster {
    fn post_render(&self, job: Box<dyn FnOnce()>) {
        self.post(UiPriority::Render, job);
    }
}

/// A coalescing wake for a `Send` caller (15j `UiWake`, the view-model inbox `WakeFn`): the returned closure posts at most ONE `Send` job at `priority` while
/// one is queued; the job calls `f` on the window found by `id` (`None` = the current window of the thread). A wake that finds no live window is dropped.
pub(crate) fn coalesced_wake(
    dispatcher: Win32UiDispatcher,
    id: Option<MainWindowId>,
    priority: UiPriority,
    f: fn(&MainWindowInner),
) -> Arc<dyn Fn() + Send + Sync> {
    let queued = Arc::new(AtomicBool::new(false));
    Arc::new(move || {
        if queued.swap(true, Ordering::AcqRel) {
            return;
        }
        let queued = Arc::clone(&queued);
        dispatcher.post_with_priority(
            Box::new(move || {
                // Cleared BEFORE `f` runs: a wake that arrives while `f` works queues a fresh job.
                queued.store(false, Ordering::Release);
                match id {
                    Some(id) => with_window(id, f),
                    None => with_current_window(f),
                };
            }),
            priority,
        );
    })
}

// ---- the UI-thread registry ------------------------------------------------------------------------------------------------------------------------

/// Looks up a live window by id on the CURRENT thread (a `thread_local` map of `Weak<MainWindowInner>`) and runs `f` on it; `None` when the window is
/// gone or the id belongs to another thread. This is how a `Send` action posted to the dispatcher (a stripe job, a cache-cleared notification, the
/// deferred-close waiter, the view-model pump wake) reaches the `!Send` window (D-W8B4M-6).
pub(crate) fn with_window<R>(id: MainWindowId, f: impl FnOnce(&MainWindowInner) -> R) -> Option<R> {
    let inner = LIVE.try_with(|live| find_live(&live.borrow(), id)).ok().flatten()?;
    if inner.is_disposed() {
        return None;
    }
    Some(f(&inner))
}

/// [`with_window`] for "the main window of this thread" (the application has exactly one: the newest live registration).
pub(crate) fn with_current_window<R>(f: impl FnOnce(&MainWindowInner) -> R) -> Option<R> {
    let inner = LIVE
        .try_with(|live| newest_live(&live.borrow(), |window| !window.is_disposed()))
        .ok()
        .flatten()?;
    Some(f(&inner))
}

/// The live entry `id` of a registry list.
fn find_live<T>(entries: &[(MainWindowId, Weak<T>)], id: MainWindowId) -> Option<Rc<T>> {
    entries
        .iter()
        .find(|(known, _)| *known == id)
        .and_then(|(_, weak)| weak.upgrade())
}

/// The newest live entry of a registry list that `accept`s.
fn newest_live<T>(entries: &[(MainWindowId, Weak<T>)], accept: impl Fn(&T) -> bool) -> Option<Rc<T>> {
    entries
        .iter()
        .rev()
        .find_map(|(_, weak)| weak.upgrade().filter(|window| accept(window)))
}

/// Adds a window to the registry of this thread (dead entries are dropped on the way).
fn register_window(id: MainWindowId, window: Weak<MainWindowInner>) {
    let _ = LIVE.try_with(|live| {
        let mut live = live.borrow_mut();
        live.retain(|(_, weak)| weak.strong_count() > 0);
        live.push((id, window));
    });
}

/// Removes a window from the registry of this thread.
pub(super) fn unregister_window(id: MainWindowId) {
    let _ = LIVE.try_with(|live| live.borrow_mut().retain(|(known, _)| *known != id));
}

/// C# `Win32UiDispatcher.Post(work)` / `Post(work, priority)` for a window-bound action: posts a `Send` closure that calls [`with_window`] and, when the
/// window is alive and not disposed, `f`.
pub(crate) fn post_to_window(
    dispatcher: &Win32UiDispatcher,
    id: MainWindowId,
    priority: UiPriority,
    f: fn(&MainWindowInner),
) {
    dispatcher.post_with_priority(
        Box::new(move || {
            with_window(id, f);
        }),
        priority,
    );
}

/// The `StripeScheduler` of the GPU cache (D-W8B3S-2): posts a `Send` job at `UiPriority::Background` that calls `with_window(id, |w| w.run_stripe(token))`.
pub(crate) struct DispatcherStripeScheduler {
    pub(crate) dispatcher: Win32UiDispatcher,
    pub(crate) window: MainWindowId,
}

impl fmt::Debug for DispatcherStripeScheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DispatcherStripeScheduler")
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}

impl StripeScheduler for DispatcherStripeScheduler {
    /// Never runs the stripe synchronously.
    fn schedule_stripe(&self, token: StripeToken) {
        let window = self.window;
        self.dispatcher.post_with_priority(
            Box::new(move || {
                with_window(window, |w| w.run_stripe(token));
            }),
            UiPriority::Background,
        );
    }
}

// ---- the window ---------------------------------------------------------------------------------------------------------------------------------------

/// All state of the main window (C# `ShellMainWindow` fields of every partial). `Rc`-shared with the closures of the window events; the closures hold a
/// `Weak` (same rule as `ShellWindowInner`, D-W8B3H-18). Fields are `pub(super)` for the feature files of this module; other units use the accessors of
/// `impl MainWindow` / `impl MainWindowInner` below.
// The fields `poster`, `frame_clock`, `frame`, `scroll_bars` and `viewing` are read by the units 13i/13j/13k, whose stubs are not ported yet.
#[allow(dead_code)]
pub struct MainWindowInner {
    pub(super) id: MainWindowId,
    pub(super) weak_self: Weak<MainWindowInner>,
    pub(super) window: Rc<ShellWindow>,
    pub(super) dispatcher: Win32UiDispatcher,
    pub(super) poster: UiLocalPoster,
    pub(super) frame_clock: Rc<dyn HostFrameClock>,
    pub(super) view_model: SharedViewModel,
    /// The shared viewer handle, cloned once from the view model (the layout callbacks read the viewer without borrowing the view model).
    pub(super) viewer: ViewerHandle,
    pub(super) size_source: Arc<ViewportSizeSource>,
    /// The last viewport size published for `size_source` (the accessor installed in the constructor reads it; `Send + Sync` as the accessor type demands).
    pub(super) published_viewport: Arc<Mutex<(f64, f64)>>,
    pub(super) hub: Rc<dyn PresentationHubPort>,
    pub(super) cloak: Rc<dyn StartupCloakPort>,
    pub(super) placement: Rc<dyn PlacementSavePort>,
    pub(super) options: MainWindowOptions,
    pub(super) log: Arc<dyn Log>,
    pub(super) mark: StartupMark,
    pub(super) viewport: Rc<ViewportController>,
    /// Ids of the subscriptions made on `window` by this struct (constructor), removed by `dispose`.
    pub(super) window_subscriptions: RefCell<Vec<EventHandlerId>>,
    /// Id of the `layout_updated` subscription on `viewport`.
    pub(super) layout_subscription: Cell<Option<u64>>,
    /// Id of the `ImageChanged` subscription on the hub.
    pub(super) hub_subscription: Cell<Option<u64>>,
    /// C# `_surface`. Borrowed mutably for the whole of one frame (`present`): nothing called while drawing may touch it (D-W8B4M-8).
    pub(super) surface: RefCell<Option<D2dRenderSurface>>,
    /// C# `_cache`.
    pub(super) cache: RefCell<Option<Rc<WindowImageCache>>>,
    /// C# `_image`: the bitmap the viewport lays out and `present` draws (the composer's `Current`).
    pub(super) image: RefCell<Option<Arc<PixelBuffer>>>,
    /// C# `_firstPresentDone`.
    pub(super) first_present_done: Cell<bool>,
    /// C# `_closeWhenFileActionDone`.
    pub(super) close_when_file_action_done: Cell<bool>,
    /// C# `_disposed`.
    pub(super) disposed: Cell<bool>,
    /// C# `IsClosingOrClosed`.
    pub(super) is_closing_or_closed: Cell<bool>,
    /// `pump_view_model` is running (a nested message loop must not re-enter it).
    pub(super) pumping: Cell<bool>,
    /// A `pump_view_model` call was refused while `pumping`: the running pump loops once more.
    pub(super) pump_again: Cell<bool>,
    /// Features of other units (input, menus, overlay, keep-warm), in registration order.
    pub(super) features: RefCell<Vec<Rc<dyn MainWindowFeature>>>,
    /// C# `_keepAlive` + `_keepWarm` + `_frameLog` as 16d returns them from `register_keep_warm`; `release_renderer` calls `KeepWarmSetup::dispose` FIRST (D-W8B4X-4).
    pub(super) keep_warm: RefCell<Option<KeepWarmSetup>>,
    /// W8-13i state (`Present`, first-image events, scroll bars).
    pub(super) frame: FrameState,
    pub(super) scroll_bars: ScrollBarState,
    /// W8-13j state (composer, prefetcher, pacer, stats).
    pub(super) viewing: ViewingState,
    /// W8-13k state (fullscreen, display-change handler).
    pub(super) windowing: WindowingState,
}

impl fmt::Debug for MainWindowInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainWindowInner")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

// The `pub(crate)` accessors are the plug-in surface of the units 14c, 14f, 15j, 16d and 13i..13k, which are not merged yet.
#[allow(dead_code)]
impl MainWindowInner {
    /// C# constructor body (lines 84-123), in this order: copy the collaborators; build the viewport through `viewport_factory` (callbacks hold a `Weak`);
    /// install the size accessor on `size_source` (reads `published_viewport`) and publish the initial size; `viewer.set_dpi_scale(window.dpi_scale())`;
    /// `register_features()`; subscribe `Render`, `ClientSizeChanged`, `DpiChanged`, `Closing`, `Closed` on the window (ids into `window_subscriptions`) and `ImageChanged` on the hub;
    /// register in the thread-local registry; `update_target_decode_box(None)`; `window.set_title(vm.folder_title())` (a failure is logged, not thrown);
    /// `initialize_viewing()`.
    pub(super) fn new(deps: MainWindowDeps, weak_self: Weak<MainWindowInner>) -> Self {
        let MainWindowDeps {
            window,
            dispatcher,
            frame_clock,
            view_model,
            size_source,
            hub,
            cloak,
            placement,
            log,
            mark,
            viewport_factory,
            options,
        } = deps;
        let mark: StartupMark = mark.unwrap_or_else(|| Box::new(startup_mark));
        let id = NEXT_WINDOW_ID.fetch_add(1, Ordering::Relaxed);
        let poster = UiLocalPoster::new(dispatcher.clone());
        let viewer = view_model.borrow().viewer().clone();

        // Step 2: the viewport, with callbacks that hold a `Weak` (a window that is gone gives the empty content / does nothing).
        let content_owner = weak_self.clone();
        let fit_owner = weak_self.clone();
        let window_api: Rc<dyn ShellWindowApi> = window.clone();
        let viewport = viewport_factory(ViewportCallbacks {
            window: window_api,
            frame_clock: Rc::clone(&frame_clock),
            poster: poster.clone(),
            content: Rc::new(move || {
                content_owner
                    .upgrade()
                    .map_or_else(ViewportContent::empty, |inner| inner.content_from_viewer())
            }),
            update_fit_size: Rc::new(move || {
                if let Some(inner) = fit_owner.upgrade() {
                    inner.update_fit_size();
                }
            }),
        });
        let initial_size = viewport.viewport_size();

        Self {
            id,
            weak_self,
            window,
            dispatcher,
            poster,
            frame_clock,
            view_model,
            viewer,
            size_source,
            published_viewport: Arc::new(Mutex::new(initial_size)),
            hub,
            cloak,
            placement,
            options,
            log,
            mark,
            viewport,
            window_subscriptions: RefCell::new(Vec::new()),
            layout_subscription: Cell::new(None),
            hub_subscription: Cell::new(None),
            surface: RefCell::new(None),
            cache: RefCell::new(None),
            image: RefCell::new(None),
            first_present_done: Cell::new(false),
            close_when_file_action_done: Cell::new(false),
            disposed: Cell::new(false),
            is_closing_or_closed: Cell::new(false),
            pumping: Cell::new(false),
            pump_again: Cell::new(false),
            features: RefCell::new(Vec::new()),
            keep_warm: RefCell::new(None),
            frame: FrameState::default(),
            scroll_bars: ScrollBarState::default(),
            viewing: ViewingState::default(),
            windowing: WindowingState::default(),
        }
    }

    /// C# `RegisterFeatures()` (lines 130-137). ORDER IS BEHAVIOUR: scroll bars BEFORE input (the bars' mouse handler must see the press before the
    /// image does), then input, windowing, context menu, keep-warm. Each unit adds exactly ONE line at the end; on a merge conflict keep all lines
    /// (the C# rule). Lines of units that are not on master are comments until they merge.
    pub(super) fn register_features(&self) {
        self.register_scroll_bars(); // 13i: BEFORE input (the bars' mouse handler must see the press first)
        self.register_input(); // 14c
        self.register_windowing(); // 13k
        self.register_context_menu(); // 14f
        self.register_keep_warm(); // 16d
    }

    /// Steps 3 to 11 of the constructor (they need the finished `Rc`, which `Rc::new_cyclic` only gives AFTER `new`): size accessor and layout subscription, the
    /// viewer scale, `register_features`, the window subscriptions (ORDER: render, client size, dpi, closing, closed), the hub subscription, the registry,
    /// the decode box, the title and `initialize_viewing`. Holds no `RefCell` borrow across a call into the window (T3).
    pub(super) fn wire(&self) {
        // Step 3: worker threads read the published size (`ViewportSizeFn` is `Send + Sync`, the viewport is not, D-W8B4M-12).
        let published = Arc::clone(&self.published_viewport);
        self.size_source.set_viewport_size_fn(Arc::new(move || {
            *published.lock().unwrap_or_else(PoisonError::into_inner)
        }));
        let weak = self.weak_self.clone();
        let layout_id = self.viewport.subscribe_layout_updated(Rc::new(move || {
            if let Some(inner) = weak.upgrade() {
                inner.publish_viewport_size();
            }
        }));
        self.layout_subscription.set(Some(layout_id));

        // Step 4: the changes are dropped, as in C# (nothing is subscribed yet).
        let dpi = self.window.dpi_scale();
        let _ = self.viewer.borrow_mut().set_dpi_scale(dpi);

        // Step 5.
        self.register_features();

        // Step 6: the order of the subscriptions is observable (features subscribed above, so their `Closed` runs before `on_closed`).
        let mut ids = Vec::with_capacity(5);
        let weak = self.weak_self.clone();
        ids.push(self.window.subscribe_render(Box::new(move |_tick| {
            if let Some(inner) = weak.upgrade() {
                inner.render_frame();
            }
        })));
        let weak = self.weak_self.clone();
        ids.push(self.window.subscribe(
            ShellWindowEvent::ClientSizeChanged,
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner.on_client_size_changed();
                }
            }),
        ));
        let weak = self.weak_self.clone();
        ids.push(self.window.subscribe(
            ShellWindowEvent::DpiChanged,
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner.on_dpi_changed();
                }
            }),
        ));
        let weak = self.weak_self.clone();
        ids.push(self.window.subscribe_closing(Box::new(move |args| {
            if let Some(inner) = weak.upgrade() {
                inner.on_closing(args);
            }
        })));
        let weak = self.weak_self.clone();
        ids.push(self.window.subscribe(
            ShellWindowEvent::Closed,
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner.on_closed();
                }
            }),
        ));
        self.window_subscriptions.borrow_mut().extend(ids);

        // Step 7: the hub handler only queues the image (it runs inside a view-model call); `apply_changes` handles the queue.
        let weak = self.weak_self.clone();
        let hub_id = self.hub.subscribe_image_changed(Rc::new(move |image, is_file_change| {
            if let Some(inner) = weak.upgrade() {
                if !inner.disposed.get() {
                    inner.on_hub_image_changed(image, is_file_change);
                }
            }
        }));
        self.hub_subscription.set(Some(hub_id));

        // Step 8.
        register_window(self.id, self.weak_self.clone());

        // Step 9.
        self.update_target_decode_box(None);

        // Step 10: C# `SetTitle` throws only for a refused `SetWindowText`, and a constructor that throws leaks the window: log instead.
        let title = self.view_model.borrow().folder_title().to_owned();
        if let Err(error) = self.window.set_title(&title) {
            self.log.error("SetWindowText failed", Some(&error));
        }

        // Step 11.
        self.initialize_viewing();
    }

    /// Publishes the viewport size for the accessor that `size_source` holds (worker threads read it; the viewport itself is `!Send`).
    pub(super) fn publish_viewport_size(&self) {
        let size = self.viewport.viewport_size();
        *self.published_viewport.lock().unwrap_or_else(PoisonError::into_inner) = size;
    }

    /// C# `RegisterKeepWarm()` (16d), the 5th line of `register_features`: the [`KeepWarmRegistration`] is TAKEN from the options (it holds a `FnOnce`) or built from
    /// `prefer_warp` alone; `prefer_warp` always comes from the window options. The whole setup is stored: `release_renderer` disposes it FIRST (D-W8B4X-4).
    fn register_keep_warm(&self) {
        let mut registration = self
            .options
            .extensions
            .take::<KeepWarmRegistration>()
            .unwrap_or(KeepWarmRegistration {
                frame_log_path: None,
                keep_warm: None,
                prefer_warp: false,
                work: None,
                timestamp: None,
            });
        registration.prefer_warp = self.options.prefer_warp;
        let window: Rc<dyn KeepWarmWindow> = self.window.clone();
        let setup = register_keep_warm(&window, registration);
        self.set_keep_warm(setup);
    }

    /// Accessor: C# `Window`.
    pub(crate) fn window(&self) -> &Rc<ShellWindow> {
        &self.window
    }

    /// Accessor: C# `_viewModel.Viewer` (a clone of the shared handle; borrow it briefly).
    pub(crate) fn viewer(&self) -> pr_app::viewmodels::main::ViewerHandle {
        self.viewer.clone()
    }

    /// C# `RefreshView`'s tail and many handlers: `viewport.invalidate_layout()` then `window.invalidate()`.
    pub(crate) fn invalidate_layout_and_window(&self) {
        self.viewport.invalidate_layout();
        self.window.invalidate();
    }

    /// Accessor: C# `Dispatcher`.
    pub(crate) fn dispatcher(&self) -> &Win32UiDispatcher {
        &self.dispatcher
    }

    /// Accessor: C# `ViewModel`.
    pub(crate) fn view_model(&self) -> &SharedViewModel {
        &self.view_model
    }

    /// Accessor: C# `Viewport`.
    pub(crate) fn viewport(&self) -> &Rc<ViewportController> {
        &self.viewport
    }

    /// Accessor: the poster for non-`Send` UI-thread closures (see [`UiLocalPoster`]).
    pub(crate) fn poster(&self) -> &UiLocalPoster {
        &self.poster
    }

    /// Accessor: the registry id.
    pub(crate) fn id(&self) -> MainWindowId {
        self.id
    }

    /// Accessor: `Log`.
    pub(crate) fn log(&self) -> &Arc<dyn Log> {
        &self.log
    }

    /// Accessor: the options (features read their [`FeatureOptions`] entry here).
    pub(crate) fn options(&self) -> &MainWindowOptions {
        &self.options
    }

    /// C# `_disposed`.
    pub(crate) fn is_disposed(&self) -> bool {
        self.disposed.get()
    }

    /// C# `_mark(name)`.
    pub(crate) fn mark(&self, name: &str) {
        (self.mark)(name);
    }

    /// Registers `feature` (see [`MainWindowFeature`]).
    pub(crate) fn add_feature(&self, feature: Rc<dyn MainWindowFeature>) {
        self.features.borrow_mut().push(feature);
    }

    /// Runs `f` on the registered feature of concrete type `T` (found through `as_any`); `None` when no such feature is registered.
    pub(crate) fn with_feature<T: 'static, R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        // The list is cloned out so that `f` may register features or call back into the window.
        let features: Vec<Rc<dyn MainWindowFeature>> = self.features.borrow().clone();
        features
            .iter()
            .find_map(|feature| feature.as_any().downcast_ref::<T>())
            .map(f)
    }

    /// Called by the 5th `register_features` line with the result of 16d's `register_keep_warm` (C# `_keepAlive`, `_keepWarm`, `_frameLog`).
    pub(crate) fn set_keep_warm(&self, setup: KeepWarmSetup) {
        *self.keep_warm.borrow_mut() = Some(setup);
    }

    /// Runs one prefetch stripe on the cache (the job of [`DispatcherStripeScheduler`]); a missing cache or an error is logged and ignored.
    pub(crate) fn run_stripe(&self, token: StripeToken) {
        let cache = self.cache.try_borrow().ok().and_then(|cache| cache.clone());
        let Some(cache) = cache else {
            return;
        };
        if let Err(error) = cache.run_stripe(token) {
            self.log.error("Image upload stripe failed", Some(&error));
        }
    }
}

/// The main window (C# `ShellMainWindow`, the owner handle). `Drop` is C# `Dispose()` (idempotent; the rethrow of a captured panic is skipped while
/// panicking, like `ShellWindow`). UI thread only (`!Send`).
pub struct MainWindow {
    pub(super) inner: Rc<MainWindowInner>,
}

impl fmt::Debug for MainWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainWindow")
            .field("id", &self.inner.id)
            .finish_non_exhaustive()
    }
}

impl MainWindow {
    /// C# constructor. Builds the inner state with `Rc::new_cyclic` and runs [`MainWindowInner::new`].
    pub fn new(deps: MainWindowDeps) -> MainWindow {
        let inner = Rc::new_cyclic(|weak| MainWindowInner::new(deps, weak.clone()));
        // The owner exists before the wiring runs: a panic in it drops the owner, which disposes what was wired so far.
        let window = MainWindow { inner };
        window.inner.wire();
        window
    }

    /// C# `Window`.
    pub fn window(&self) -> &Rc<ShellWindow> {
        &self.inner.window
    }

    /// Diagnostics for the ownership rule (T1): the number of strong references to the shared state. It is 1 while the owner lives, because every closure the
    /// window gives away holds a `Weak`.
    pub fn strong_count(&self) -> usize {
        Rc::strong_count(&self.inner)
    }

    /// C# `Hwnd` (`0` after the window was destroyed or before `WM_NCCREATE`).
    pub fn hwnd(&self) -> Hwnd {
        self.inner.window.hwnd()
    }

    /// C# `ViewModel`.
    pub fn view_model(&self) -> &SharedViewModel {
        &self.inner.view_model
    }

    /// C# `Viewport`.
    pub fn viewport(&self) -> &Rc<ViewportController> {
        &self.inner.viewport
    }

    /// C# `Dispatcher`.
    pub fn dispatcher(&self) -> &Win32UiDispatcher {
        &self.inner.dispatcher
    }

    /// C# `FirstPresentDone`: true after the first frame was presented.
    pub fn first_present_done(&self) -> bool {
        self.inner.first_present_done.get()
    }

    /// C# `IsClosingOrClosed`.
    pub fn is_closing_or_closed(&self) -> bool {
        self.inner.is_closing_or_closed.get()
    }

    /// C# `Surface != null`: the surface exists (between `attach_renderer` and `release_renderer`). The surface itself is not handed out (it is
    /// borrowed for a whole frame); tests use [`MainWindow::with_surface`].
    pub fn has_surface(&self) -> bool {
        // A surface that is being drawn on (borrowed) exists.
        self.inner
            .surface
            .try_borrow()
            .map_or(true, |surface| surface.is_some())
    }

    /// C# `Surface` for diagnostics and tests: runs `f` on the surface; `None` when there is none or it is being drawn on (`try_borrow_mut` fails).
    pub fn with_surface<R>(&self, f: impl FnOnce(&mut D2dRenderSurface) -> R) -> Option<R> {
        let mut surface = self.inner.surface.try_borrow_mut().ok()?;
        surface.as_mut().map(f)
    }

    /// C# `ImageCache`.
    pub fn image_cache(&self) -> Option<Rc<WindowImageCache>> {
        self.inner.cache.try_borrow().ok().and_then(|cache| cache.clone())
    }

    /// Registers a feature of another unit (see [`MainWindowFeature`]).
    pub fn add_feature(&self, feature: Rc<dyn MainWindowFeature>) {
        self.inner.add_feature(feature);
    }

    /// Runs `f` on the registered feature of concrete type `T` (D-W8B4X-9): how the composition root (`pr-shell`, W8-17e) reaches `MainWindowInput` for the Fit
    /// closure of the overlay and for the Tools-menu host.
    pub fn with_feature<T: 'static, R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.inner.with_feature(f)
    }

    /// C# `AttachRenderer(factory)`: once, after the window has an HWND. Order: not disposed (else `Disposed`), not attached (else
    /// `RendererAlreadyAttached`); `RenderSurfaceOptions { prefer_warp }`; surface from the hook or `D2dRenderSurface::create_window_surface(hwnd, ..)`;
    /// marks `d3dDeviceReady` and `swapChainReady` (both, once); if the client size is positive and differs from the surface, `surface.resize(w, h,
    /// dpi_scale)`; store the surface; `cache = D2dGpuImageCache::new(ResourcesBitmapDevice::new(surface.resources()), DispatcherStripeScheduler, surface
    /// pixel size)`; `attach_viewing()`; `window.set_surface_ready(true)`.
    pub fn attach_renderer(&self, factory: Option<&dyn RenderSurfaceFactoryHook>) -> Result<(), MainWindowError> {
        let inner = &*self.inner;
        if inner.disposed.get() {
            return Err(MainWindowError::Disposed);
        }
        if inner.surface.try_borrow().map_or(true, |surface| surface.is_some()) {
            return Err(MainWindowError::RendererAlreadyAttached);
        }

        let options = SurfaceOptions {
            prefer_warp: inner.options.prefer_warp,
            ..SurfaceOptions::default()
        };
        let hwnd = inner.window.hwnd();
        let mut surface = match factory {
            Some(hook) => hook.create(hwnd, options)?,
            None => D2dRenderSurface::create_window_surface(hwnd, options)?,
        };
        // C# emits both marks although one call created the device and the swap chain.
        inner.mark(ShellPerfMarks::D3D_DEVICE_READY);
        inner.mark(ShellPerfMarks::SWAP_CHAIN_READY);
        let (width, height) = inner.window.client_size_pixels();
        if width > 0 && height > 0 && (surface.pixel_width() != width || surface.pixel_height() != height) {
            // An `Err` drops the surface (its `Drop` disposes it): nothing is stored and a retry is allowed.
            surface.resize(width, height, inner.window.dpi_scale())?;
        }

        // The cache is built BEFORE anything is stored, so a failure leaves no partial state (T4).
        let device = ResourcesBitmapDevice::new(Rc::clone(surface.resources()));
        let scheduler: Rc<dyn StripeScheduler> = Rc::new(DispatcherStripeScheduler {
            dispatcher: inner.dispatcher.clone(),
            window: inner.id,
        });
        let cache = D2dGpuImageCache::new(device, scheduler, surface.pixel_width(), surface.pixel_height()).map_err(
            |error| {
                inner
                    .log
                    .error("The GPU image cache could not be created", Some(&error));
                MainWindowError::Render(RenderFault::InvalidOperation {
                    message: "The GPU image cache could not be created.",
                })
            },
        )?;
        *inner.surface.borrow_mut() = Some(surface);
        *inner.cache.borrow_mut() = Some(Rc::new(cache));
        inner.attach_viewing();
        inner.window.set_surface_ready(true);
        Ok(())
    }

    /// C# `OpenInitialAsync(initialPath, isFile)`: sets the decode box from the REAL client area (when positive), and for a non-blank path: mark
    /// `openPathBegin`; for a file the prediction marks (`earlyDecodeBoxMatched`/`earlyDecodeBoxMismatch` when a startup prediction exists) and
    /// `prewarm_initial_image` (mark `initialDecodeStarted` when it started a task); `set_defer_next_preload_kick(true)`; `vm.open_path(path)`; the returned
    /// `ChangeSet` goes through `apply_changes`. Blank or `None` path: nothing more. The C# `Task` is the view model's own step machine (`pump`).
    pub fn open_initial(&self, initial_path: Option<&str>, is_file: bool) {
        let inner = &*self.inner;
        let (client_width, client_height) = inner.window.client_size_pixels();
        if client_width > 0 && client_height > 0 {
            let dpi = inner.window.dpi_scale();
            inner.update_target_decode_box(Some((f64::from(client_width) / dpi, f64::from(client_height) / dpi)));
        }
        let Some(path) = initial_path.filter(|path| !path.trim().is_empty()) else {
            return;
        };

        inner.mark("openPathBegin");
        if is_file {
            if let Some(predicted) = inner.size_source.startup_prediction() {
                inner.mark(if predicted == inner.size_source.target_decode_box() {
                    "earlyDecodeBoxMatched"
                } else {
                    "earlyDecodeBoxMismatch"
                });
            }
            // A short borrow: the prewarm only starts a task.
            let started = inner.view_model.borrow().prewarm_initial_image(path).is_some();
            if started {
                inner.mark("initialDecodeStarted");
            }
        }

        // P-1: the first image's frame before the preload.
        inner.view_model.borrow_mut().set_defer_next_preload_kick(true);
        self.open_path(path);
        // The C# `Task` is the view model's step machine: whatever already arrived (including wakes dropped before the window existed) is drained now.
        inner.pump_view_model();
    }
}

impl Drop for MainWindow {
    /// `self.inner.dispose_inner(!std::thread::panicking())` (13h): a captured panic is not resumed while the thread is already unwinding.
    fn drop(&mut self) {
        self.inner.dispose_inner(!std::thread::panicking());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // D-W8B4M-6: the registry finds a window by id only while it is alive.
    #[test]
    fn registry_with_window_finds_only_live_windows() {
        let first = Rc::new(1_i32);
        let second = Rc::new(2_i32);
        let entries = vec![(10, Rc::downgrade(&first)), (11, Rc::downgrade(&second))];
        assert_eq!(find_live(&entries, 10).as_deref(), Some(&1));
        assert_eq!(find_live(&entries, 11).as_deref(), Some(&2));
        assert!(find_live(&entries, 12).is_none(), "unknown id");
        drop(second);
        assert!(find_live(&entries, 11).is_none(), "a dropped window is gone");
        assert_eq!(find_live(&entries, 10).as_deref(), Some(&1));
    }

    // Application model: one window per thread, the newest live registration is "the current window".
    #[test]
    fn newest_live_takes_the_newest_accepted_live_entry() {
        let old = Rc::new(1_i32);
        let middle = Rc::new(2_i32);
        let newest = Rc::new(3_i32);
        let entries = vec![
            (1, Rc::downgrade(&old)),
            (2, Rc::downgrade(&middle)),
            (3, Rc::downgrade(&newest)),
        ];
        assert_eq!(newest_live(&entries, |_| true).as_deref(), Some(&3));
        assert_eq!(
            newest_live(&entries, |value| *value != 3).as_deref(),
            Some(&2),
            "a disposed window is skipped"
        );
        drop(newest);
        assert_eq!(
            newest_live(&entries, |_| true).as_deref(),
            Some(&2),
            "a dropped window is skipped"
        );
        assert!(newest_live(&entries, |_| false).is_none());
        assert!(newest_live::<i32>(&[], |_| true).is_none());
    }

    // The local queues are indexed by priority; the dispatcher serves Send < Normal < Render < Background.
    #[test]
    fn priority_slots_follow_the_dispatcher_order() {
        let priorities = [
            UiPriority::Send,
            UiPriority::Normal,
            UiPriority::Render,
            UiPriority::Background,
        ];
        for (index, priority) in priorities.into_iter().enumerate() {
            assert_eq!(priority_slot(priority), index);
        }
    }

    // The timer heap serves the earliest due time first and keeps arrival order for equal times.
    #[test]
    fn timer_entries_order_by_due_time_then_arrival() {
        let now = Instant::now();
        let entry = |offset_ms: u64, seq: u64| TimerEntry {
            due: now + Duration::from_millis(offset_ms),
            seq,
            action: Box::new(|| {}),
        };
        let mut heap = BinaryHeap::new();
        heap.push(Reverse(entry(20, 0)));
        heap.push(Reverse(entry(10, 2)));
        heap.push(Reverse(entry(10, 1)));
        let order: Vec<(u64, u64)> = std::iter::from_fn(|| heap.pop())
            .map(|Reverse(entry)| ((entry.due - now).as_millis() as u64, entry.seq))
            .collect();
        assert_eq!(order, [(10, 1), (10, 2), (20, 0)]);
    }
}
