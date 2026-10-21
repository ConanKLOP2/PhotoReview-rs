//! Port of `PhotoReview.Shell.Win32/ShellMainWindow.cs` lines 1-226 (baseline `d92d6aae`): the options record, the constants, the fields, the constructor,
//! `RegisterFeatures`, the plain properties, `AttachRenderer` and `OpenInitialAsync`. Unit W8-13g. Contract: `contracts/W8-13g.md`.
//!
//! This file declares [`MainWindowInner`] with ALL fields of all the `ShellMainWindow` partials of units 13g..13k (the per-feature state lives in a small
//! struct owned by the feature file: `FrameState`, `ScrollBarState`, `ViewingState`, `WindowingState`); the feature files add `impl MainWindowInner`
//! blocks. Features of OTHER units (input 14c, menus 14f, overlay 15j, keep-warm 16d) plug in through [`MainWindowFeature`] and [`FeatureOptions`]
//! and use the `pub(crate)` accessors below (D-W8B4M-3, -5).
//!
//! Interim seams (D-W8B4M-4): the collaborators that other batch-B4/B6 units own are named by small traits here ([`MainViewport`] for
//! `ViewportController` 16h, [`PresentationHubPort`] for `ShellPresentationHub` 16i, [`StartupCloakPort`] and [`PlacementSavePort`] for `StartupCloak` and
//! `ShellPlacement.Save` 17b, [`KeepWarmPort`] for `KeepWarmController` 16d). The owning unit implements the trait for its type, or the lead replaces the
//! trait by the type once it is on master. Sizes cross the seams as `f64` DIP.

#![allow(unused_variables, dead_code)] // W8-13g stub: the porter removes this line when the last `todo!()` is gone.

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};

use pr_app::input::primitives::PointD;
use pr_app::services::viewport_size_source::ViewportSizeSource;
use pr_app::viewmodels::main::MainViewModel;
use pr_app::viewport::render_geometry::ColorF;
use pr_app::viewport::types::ViewerStretchMode;
use pr_app::windowing::contracts::WindowShowState;
use pr_core::abstractions::{Log, UiPriority};
use pr_imaging::decode_types::PlatformImage;
use pr_imaging::pixels::PixelBuffer;

use super::frame::FrameState;
use super::scrollbars::ScrollBarState;
use super::viewing::ViewingState;
use super::windowing::{ShellWindowingSeams, WindowingState};
use crate::host::contracts::EventHandlerId;
use crate::host::error::HostError;
use crate::host::frame_clock::base::HostFrameClock;
use crate::host::shell_window::ShellWindow;
use crate::host::ui_dispatcher::Win32UiDispatcher;
use crate::render::device::RenderFault;
use crate::render::image_cache::policy::{StripeScheduler, StripeToken};
use crate::render::image_cache::upload::{D2dGpuImageCache, ResourcesBitmapDevice};
use crate::render::surface::{D2dRenderSurface, SurfaceOptions};
use crate::shell_viewing::frame_log::FrameLog;
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

/// C# `CanvasColor` = `Dark.Canvas` #101010 (`new ColorF(0x10 / 255f, 0x10 / 255f, 0x10 / 255f, 1f)`, f32 division).
pub fn canvas_color() -> ColorF {
    todo!("W8-13g")
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
    map: HashMap<TypeId, Rc<dyn Any>>,
}

impl fmt::Debug for FeatureOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FeatureOptions")
            .field("entries", &self.map.len())
            .finish()
    }
}

impl FeatureOptions {
    /// Stores `value` (replacing an earlier value of the same type).
    pub fn insert<T: Any>(&mut self, value: T) {
        todo!("W8-13g")
    }

    /// The value of type `T`, if one was inserted.
    pub fn get<T: Any>(&self) -> Option<Rc<T>> {
        todo!("W8-13g")
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
        todo!("W8-13g")
    }
}

// ---- seams to other units' types ---------------------------------------------------------------------------------------------------------------

/// Interim twin of `shell_viewing::viewport_controller::ViewportContent` (W8-16h; C# `ViewportContent` record): what `MainImage` took from `ViewerState`
/// and the drawn bitmap. 16h's type wins; the move is a rename.
#[derive(Debug, Clone, Copy)]
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

/// The members of `ViewportController` (C#, W8-16h, `shell_viewing::viewport_controller`) that the main window calls. INTERIM: 16h implements this trait for its
/// controller, or the lead replaces `Rc<dyn MainViewport>` by `Rc<ViewportController>` once 16h is on master (the scroll bars, W8-16f, take the concrete
/// controller anyway). UI thread only, all `&self`. Coordinates are DIP; the point type is `pr_app`'s (what 16h returns).
pub trait MainViewport {
    /// C# `ViewportSize`: `(width, height)` DIP of the visible image area (after the scroll bars).
    fn viewport_size(&self) -> (f64, f64);
    /// C# `InvalidateLayout()`: queue one coalesced layout pass at `UiPriority::Render`.
    fn invalidate_layout(&self);
    /// C# `IsLayoutPosted`.
    fn is_layout_posted(&self) -> bool;
    /// C# `UpdateLayout()`: run the posted layout now.
    fn update_layout(&self);
    /// C# `ImageOrigin`.
    fn image_origin(&self) -> PointD;
    /// C# `ImageActualWidth`.
    fn image_actual_width(&self) -> f64;
    /// C# `ImageActualHeight`.
    fn image_actual_height(&self) -> f64;
    /// C# `LayoutUpdated += handler`; the id unsubscribes.
    fn subscribe_layout_updated(&self, handler: Rc<dyn Fn()>) -> u64;
    /// C# `LayoutUpdated -= handler`.
    fn unsubscribe_layout_updated(&self, id: u64);
    /// C# `Dispose()`: idempotent.
    fn dispose(&self);
}

/// The two window-owned callbacks of the C# `ViewportController` constructor (`ContentFromViewer`, `UpdateFitSize`), in the `Rc<dyn Fn>` form 16h takes.
pub struct ViewportCallbacks {
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
/// `|callbacks| ViewportController::new(window, poster, clock, callbacks.content, callbacks.update_fit_size, options_with(create_viewport_input_hooks(..)))`
/// (the C# constructor called `CreateViewportOptions` of `ShellMainWindow.Input.cs` = 14c itself; the factory removes the edge 13g -> 14c, D-W8B4M-4).
pub type ViewportFactory = Box<dyn FnOnce(ViewportCallbacks) -> Rc<dyn MainViewport>>;

/// A hub image handler (16i `ImageChangedHandler`): the new image (`None` = empty folder or compare) and C# `isFileChange`.
pub type HubImageHandler = Rc<dyn Fn(Option<&PlatformImage>, bool)>;

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

/// `KeepWarmController` (W8-16d), the one member `on_frame_presented` calls.
pub trait KeepWarmPort {
    /// C# `WarmAtLaunch()`.
    fn warm_at_launch(&self);
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
    fn draw_overlay(&self, context: &mut dyn pr_app::services::render::DrawContext) {}
    /// Called by `apply_changes` after the window's own handling, with the whole change set (overlay refresh, menus).
    fn on_changes(&self, changes: &pr_app::viewmodels::main::ChangeSet) {}
    /// C# `_onViewModelChangedForInput` for `CurrentIndex` (14c: `NoteCurrentIndexChanged`).
    fn on_current_index_changed(&self, index: i32) {}
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
        todo!("W8-13g")
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
        todo!("W8-13g")
    }

    /// `post` after `delay`: a timer thread waits (a bounded channel wait, no polling) and then posts a `Send` wake job; never blocks the UI thread. A window
    /// that is gone before the delay ends makes the job a no-op.
    pub fn post_after(&self, delay: std::time::Duration, priority: UiPriority, job: LocalJob) {
        todo!("W8-13g")
    }
}

impl LocalPoster for UiLocalPoster {
    fn post(&self, priority: UiPriority, job: LocalJob) {
        todo!("W8-13g")
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
    todo!("W8-13g")
}

// ---- the UI-thread registry ------------------------------------------------------------------------------------------------------------------------

/// Looks up a live window by id on the CURRENT thread (a `thread_local` map of `Weak<MainWindowInner>`) and runs `f` on it; `None` when the window is
/// gone or the id belongs to another thread. This is how a `Send` action posted to the dispatcher (a stripe job, a cache-cleared notification, the
/// deferred-close waiter, the view-model pump wake) reaches the `!Send` window (D-W8B4M-6).
pub(crate) fn with_window<R>(id: MainWindowId, f: impl FnOnce(&MainWindowInner) -> R) -> Option<R> {
    todo!("W8-13g")
}

/// [`with_window`] for "the main window of this thread" (the application has exactly one: the newest live registration).
pub(crate) fn with_current_window<R>(f: impl FnOnce(&MainWindowInner) -> R) -> Option<R> {
    todo!("W8-13g")
}

/// C# `Win32UiDispatcher.Post(work)` / `Post(work, priority)` for a window-bound action: posts a `Send` closure that calls [`with_window`] and, when the
/// window is alive and not disposed, `f`.
pub(crate) fn post_to_window(
    dispatcher: &Win32UiDispatcher,
    id: MainWindowId,
    priority: UiPriority,
    f: fn(&MainWindowInner),
) {
    todo!("W8-13g")
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
        todo!("W8-13g")
    }
}

// ---- the window ---------------------------------------------------------------------------------------------------------------------------------------

/// All state of the main window (C# `ShellMainWindow` fields of every partial). `Rc`-shared with the closures of the window events; the closures hold a
/// `Weak` (same rule as `ShellWindowInner`, D-W8B3H-18). Fields are `pub(super)` for the feature files of this module; other units use the accessors of
/// `impl MainWindow` / `impl MainWindowInner` below.
pub struct MainWindowInner {
    pub(super) id: MainWindowId,
    pub(super) weak_self: Weak<MainWindowInner>,
    pub(super) window: Rc<ShellWindow>,
    pub(super) dispatcher: Win32UiDispatcher,
    pub(super) poster: UiLocalPoster,
    pub(super) frame_clock: Rc<dyn HostFrameClock>,
    pub(super) view_model: SharedViewModel,
    pub(super) size_source: Arc<ViewportSizeSource>,
    /// The last viewport size published for `size_source` (the accessor installed in the constructor reads it; `Send + Sync` as the accessor type demands).
    pub(super) published_viewport: Arc<Mutex<(f64, f64)>>,
    pub(super) hub: Rc<dyn PresentationHubPort>,
    pub(super) cloak: Rc<dyn StartupCloakPort>,
    pub(super) placement: Rc<dyn PlacementSavePort>,
    pub(super) options: MainWindowOptions,
    pub(super) log: Arc<dyn Log>,
    pub(super) mark: StartupMark,
    pub(super) viewport: Rc<dyn MainViewport>,
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
    /// Features of other units (input, menus, overlay, keep-warm), in registration order.
    pub(super) features: RefCell<Vec<Rc<dyn MainWindowFeature>>>,
    /// C# `_keepWarm` (set by 16d's `register_keep_warm`).
    pub(super) keep_warm: RefCell<Option<Rc<dyn KeepWarmPort>>>,
    /// C# `_frameLog` (set by 16d's `register_keep_warm`, closed by `dispose_keep_warm`).
    pub(super) frame_log: RefCell<Option<FrameLog>>,
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

impl MainWindowInner {
    /// C# constructor body (lines 84-123), in this order: copy the collaborators; build the viewport through `viewport_factory` (callbacks hold a `Weak`);
    /// install the size accessor on `size_source` (reads `published_viewport`) and publish the initial size; `viewer.set_dpi_scale(window.dpi_scale())`;
    /// `register_features()`; subscribe `Render`, `ClientSizeChanged`, `DpiChanged`, `Closing`, `Closed` on the window (ids into `window_subscriptions`) and `ImageChanged` on the hub;
    /// register in the thread-local registry; `update_target_decode_box(None)`; `window.set_title(vm.folder_title())` (a failure is logged, not thrown);
    /// `initialize_viewing()`.
    pub(super) fn new(deps: MainWindowDeps, weak_self: Weak<MainWindowInner>) -> Self {
        todo!("W8-13g")
    }

    /// C# `RegisterFeatures()` (lines 130-137). ORDER IS BEHAVIOUR: scroll bars BEFORE input (the bars' mouse handler must see the press before the
    /// image does), then input, windowing, context menu, keep-warm. Each unit adds exactly ONE line at the end; on a merge conflict keep all lines
    /// (the C# rule). Lines of units that are not on master are comments until they merge.
    pub(super) fn register_features(&self) {
        todo!("W8-13g")
    }

    /// Accessor: C# `Window`.
    pub(crate) fn window(&self) -> &Rc<ShellWindow> {
        todo!("W8-13g")
    }

    /// Accessor: C# `_viewModel.Viewer` (a clone of the shared handle; borrow it briefly).
    pub(crate) fn viewer(&self) -> pr_app::viewmodels::main::ViewerHandle {
        todo!("W8-13g")
    }

    /// C# `RefreshView`'s tail and many handlers: `viewport.invalidate_layout()` then `window.invalidate()`.
    pub(crate) fn invalidate_layout_and_window(&self) {
        todo!("W8-13g")
    }

    /// Accessor: C# `Dispatcher`.
    pub(crate) fn dispatcher(&self) -> &Win32UiDispatcher {
        todo!("W8-13g")
    }

    /// Accessor: C# `ViewModel`.
    pub(crate) fn view_model(&self) -> &SharedViewModel {
        todo!("W8-13g")
    }

    /// Accessor: C# `Viewport`.
    pub(crate) fn viewport(&self) -> &Rc<dyn MainViewport> {
        todo!("W8-13g")
    }

    /// Accessor: the poster for non-`Send` UI-thread closures (see [`UiLocalPoster`]).
    pub(crate) fn poster(&self) -> &UiLocalPoster {
        todo!("W8-13g")
    }

    /// Accessor: the registry id.
    pub(crate) fn id(&self) -> MainWindowId {
        todo!("W8-13g")
    }

    /// Accessor: `Log`.
    pub(crate) fn log(&self) -> &Arc<dyn Log> {
        todo!("W8-13g")
    }

    /// Accessor: the options (features read their [`FeatureOptions`] entry here).
    pub(crate) fn options(&self) -> &MainWindowOptions {
        todo!("W8-13g")
    }

    /// C# `_disposed`.
    pub(crate) fn is_disposed(&self) -> bool {
        todo!("W8-13g")
    }

    /// C# `_mark(name)`.
    pub(crate) fn mark(&self, name: &str) {
        todo!("W8-13g")
    }

    /// Registers `feature` (see [`MainWindowFeature`]).
    pub(crate) fn add_feature(&self, feature: Rc<dyn MainWindowFeature>) {
        todo!("W8-13g")
    }

    /// Runs `f` on the registered feature of concrete type `T` (found through `as_any`); `None` when no such feature is registered.
    pub(crate) fn with_feature<T: 'static, R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        todo!("W8-13g")
    }

    /// Called by 16d's `register_keep_warm`: stores the controller port and the frame log (C# `_keepWarm`, `_frameLog`).
    pub(crate) fn set_keep_warm(&self, keep_warm: Option<Rc<dyn KeepWarmPort>>, frame_log: Option<FrameLog>) {
        todo!("W8-13g")
    }

    /// Runs one prefetch stripe on the cache (the job of [`DispatcherStripeScheduler`]); a missing cache or an error is logged and ignored.
    pub(crate) fn run_stripe(&self, token: StripeToken) {
        todo!("W8-13g")
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
        todo!("W8-13g")
    }

    /// C# `Window`.
    pub fn window(&self) -> &Rc<ShellWindow> {
        todo!("W8-13g")
    }

    /// C# `Hwnd` (`0` after the window was destroyed or before `WM_NCCREATE`).
    pub fn hwnd(&self) -> Hwnd {
        todo!("W8-13g")
    }

    /// C# `ViewModel`.
    pub fn view_model(&self) -> &SharedViewModel {
        todo!("W8-13g")
    }

    /// C# `Viewport`.
    pub fn viewport(&self) -> &Rc<dyn MainViewport> {
        todo!("W8-13g")
    }

    /// C# `Dispatcher`.
    pub fn dispatcher(&self) -> &Win32UiDispatcher {
        todo!("W8-13g")
    }

    /// C# `FirstPresentDone`: true after the first frame was presented.
    pub fn first_present_done(&self) -> bool {
        todo!("W8-13g")
    }

    /// C# `IsClosingOrClosed`.
    pub fn is_closing_or_closed(&self) -> bool {
        todo!("W8-13g")
    }

    /// C# `Surface != null`: the surface exists (between `attach_renderer` and `release_renderer`). The surface itself is not handed out (it is
    /// borrowed for a whole frame); tests use [`MainWindow::with_surface`].
    pub fn has_surface(&self) -> bool {
        todo!("W8-13g")
    }

    /// C# `Surface` for diagnostics and tests: runs `f` on the surface; `None` when there is none or it is being drawn on (`try_borrow_mut` fails).
    pub fn with_surface<R>(&self, f: impl FnOnce(&mut D2dRenderSurface) -> R) -> Option<R> {
        todo!("W8-13g")
    }

    /// C# `ImageCache`.
    pub fn image_cache(&self) -> Option<Rc<WindowImageCache>> {
        todo!("W8-13g")
    }

    /// Registers a feature of another unit (see [`MainWindowFeature`]).
    pub fn add_feature(&self, feature: Rc<dyn MainWindowFeature>) {
        todo!("W8-13g")
    }

    /// C# `AttachRenderer(factory)`: once, after the window has an HWND. Order: not disposed (else `Disposed`), not attached (else
    /// `RendererAlreadyAttached`); `RenderSurfaceOptions { prefer_warp }`; surface from the hook or `D2dRenderSurface::create_window_surface(hwnd, ..)`;
    /// marks `d3dDeviceReady` and `swapChainReady` (both, once); if the client size is positive and differs from the surface, `surface.resize(w, h,
    /// dpi_scale)`; store the surface; `cache = D2dGpuImageCache::new(ResourcesBitmapDevice::new(surface.resources()), DispatcherStripeScheduler, surface
    /// pixel size)`; `attach_viewing()`; `window.set_surface_ready(true)`.
    pub fn attach_renderer(&self, factory: Option<&dyn RenderSurfaceFactoryHook>) -> Result<(), MainWindowError> {
        todo!("W8-13g")
    }

    /// C# `OpenInitialAsync(initialPath, isFile)`: sets the decode box from the REAL client area (when positive), and for a non-blank path: mark
    /// `openPathBegin`; for a file the prediction marks (`earlyDecodeBoxMatched`/`earlyDecodeBoxMismatch` when a startup prediction exists) and
    /// `prewarm_initial_image` (mark `initialDecodeStarted` when it started a task); `set_defer_next_preload_kick(true)`; `vm.open_path(path)`; the returned
    /// `ChangeSet` goes through `apply_changes`. Blank or `None` path: nothing more. The C# `Task` is the view model's own step machine (`pump`).
    pub fn open_initial(&self, initial_path: Option<&str>, is_file: bool) {
        todo!("W8-13g")
    }
}

impl Drop for MainWindow {
    /// `self.inner.dispose_inner(!std::thread::panicking())` (13h): a captured panic is not resumed while the thread is already unwinding.
    fn drop(&mut self) {
        todo!("W8-13g")
    }
}
