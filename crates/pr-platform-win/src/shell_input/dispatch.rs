//! Port of `PhotoReview.Shell.Win32/Input/ShellInput.cs` (420 lines, baseline `d92d6aae`): `IShellInputHost` and `ShellInput`. Unit W8-14b.
//! Contract: `contracts/W8-14b.md`.
//!
//! `ShellInput` owns the pointer controller, the shortcut router, the click counter and the operation counter, registers the keyboard, pointer
//! and wheel handlers (14a) on the window, and holds the decision order of `MainWindow.Window_KeyDown` (arrow pan, Esc, button Space/Enter, router,
//! command) and the mouse-region rules. The pure controllers are in `pr_app::input` (`PointerInputController`, `ShortcutRouter`) and are driven here
//! through `PointerContext` (D-W8-1, D-W8-2): no `await`; a render-pass wait is a `PointerStep::AwaitRender` that this unit posts through the
//! `UiDispatcher` at Render priority.
//!
//! Rust shape (D-W8B4I-1): `ShellInput` is a handle over an `Rc` core; the three handlers hold the core through the port traits of `handlers`.
//! The core's mutable state is in `RefCell`s that are borrowed only while a pure controller call runs; commands the controller asks for
//! (`next`, `previous`, `apply_fit`) and commands the router resolves are recorded and performed AFTER the borrows are released, because the host
//! can re-enter the handler chain (`SendMessage`, `WM_CAPTURECHANGED` from `ReleaseCapture`).
//!
//! No `unsafe` in this unit: everything is `Rc`/`RefCell`/traits; the OS reads are 14a's (`native`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use pr_app::input::key_id::KeyId;
use pr_app::input::mouse_gestures::resolve_middle_click;
use pr_app::input::operation_version::ViewportOperationVersion;
use pr_app::input::pointer::PointerInputController;
use pr_app::input::pointer::{PointerCommands, PointerContext, PointerContinuation, PointerStep, PointerViewer};
use pr_app::input::primitives::{KeyModifiers, PointD, PointerButton, RectD};
use pr_app::input::review_command::{ReviewCommand, ReviewCommandType};
use pr_app::input::shortcut_router::{RouteState, ShortcutRouter};
use pr_app::input::surface::ImageSurface;
use pr_app::input::touchpad_gestures::WheelInput;
use pr_app::misc::viewport_convergence::is_stable_viewport;
use pr_app::misc::window_helpers;
use pr_app::viewmodels::main::ViewerHandle;
use pr_app::viewmodels::viewer_state::ViewerUpdate;
use pr_app::viewport::fit::{
    FitContinuation, FitStep, FitSurface, FitViewer, OperationVersion, apply_fit_resume, apply_fit_start,
};
use pr_app::viewport::types::ViewportSnapshot;
use pr_core::abstractions::{Log, UiDispatcher, UiPriority};
use pr_core::diagnostics::perf::PhotoReviewPerf;
use pr_core::model::enums::InitialViewMode;
use pr_core::settings::app_settings::AppSettings;

use super::click_counter::ClickCounter;
use super::environment::ShellInputEnvironment;
use super::handlers::{
    KeyInputPort, KeyboardMessageHandler, PointerInputPort, PointerMessageHandler, WheelInputPort, WheelMessageHandler,
};
use super::native::dpi_of;
use crate::host::contracts::{EventHandlerId, PointD as HostPointD, ShellWindowApi, ShellWindowEvent};

/// A hit test over a DIP point (C# `Func<PointD, bool>`).
pub type PointTest = Rc<dyn Fn(PointD) -> bool>;

/// A parameterless predicate (C# `Func<bool>`).
pub type Predicate = Rc<dyn Fn() -> bool>;

/// A parameterless notification handler (C# `EventHandler` of `PointerActivity`).
pub type ActivityHandler = Rc<dyn Fn()>;

/// The router a surface calls with the rendering time of every frame while the kinetic glide is hooked (`None` = the frame carried no time).
pub type FrameRouter = Rc<dyn Fn(Option<i64>)>;

/// `IImageSurface` plus `IFitSurface` of one object (C# `ViewportImageSurface`), plus the frame router the pure pointer controller cannot use itself:
/// it hooks a no-op render handler (`ImageSurface::hook_render_frame`), so the shell routes the frames to
/// `PointerInputController::on_kinetic_frame` (D-W8B4I-5).
pub trait InputSurface: ImageSurface + FitSurface {
    /// Installs (or clears) the router; the surface calls it once per frame of the viewport's frame clock while at least one hook is registered.
    fn set_frame_router(&mut self, router: Option<FrameRouter>);
}

/// C# `IShellInputHost`: what `ShellInput` needs from the main window besides the viewport. `&self` everywhere: the host is re-entered (a command can
/// close the window, which sends messages back into the handler chain). Commands run synchronously on the UI thread (D-W8-2); the C# `Task` results
/// are gone.
pub trait ShellInputHost {
    /// C# `HasImages`.
    fn has_images(&self) -> bool;
    /// C# `IsFullscreen`.
    fn is_fullscreen(&self) -> bool;
    /// C# `IsCompareVisible`.
    fn is_compare_visible(&self) -> bool;
    /// C# `CurrentHasComparePair`.
    fn current_has_compare_pair(&self) -> bool;
    /// C# `CurrentHasCapturePair`.
    fn current_has_capture_pair(&self) -> bool;
    /// C# `NextAsync()`: show the next image (the view model call, applied by the host).
    fn next(&self);
    /// C# `PreviousAsync()`.
    fn previous(&self);
    /// C# `CancelDuplicateCheck()`: Esc during a duplicate check cancels it and swallows the key (Q-R25); false when none runs.
    fn cancel_duplicate_check(&self) -> bool;
    /// C# `ExecuteAsync(command)`: the commands that are not viewport commands (folder navigation, file actions, fullscreen, Close, OpenFolder,
    /// CustomZoom, Refresh ...). `Err` is reported by `ShellInput` through [`ShellInputHost::report_action_failed`].
    fn execute(&self, command: ReviewCommand) -> Result<(), Box<dyn Error>>;
    /// C# `ReportActionFailed(actionName, exception)`: log and show the "action failed" status.
    fn report_action_failed(&self, action_name: &str, error: &dyn Error);
    /// C# `Tr.MainMenuZoom` (the localized "Zoom"), read at call time because the language can change at run time.
    fn zoom_action_name(&self) -> String;
}

/// Everything `ShellInput::new` needs (the C# constructor arguments plus the services the Rust controllers take per call).
pub struct ShellInputDeps {
    /// C# `window`.
    pub window: Rc<dyn ShellWindowApi>,
    /// C# `viewport` wrapped as `ViewportImageSurface` (W8-14c), shared with the main window's own use (Fit, refresh).
    pub surface: Rc<RefCell<dyn InputSurface>>,
    /// C# `viewer`.
    pub viewer: ViewerHandle,
    /// C# `Func<AppSettings> settings` (read at each use).
    pub settings: Rc<dyn Fn() -> Arc<AppSettings>>,
    /// C# `host`.
    pub host: Rc<dyn ShellInputHost>,
    /// The UI dispatcher: `Render` priority for the continuation of a zoom or Fit pass, `Normal` for the rest. The actions are `Send`, so they carry
    /// the id of this `ShellInput` and a plain-data continuation and find the `!Send` core again through a UI-thread registry (D-W8B4I-3, the technique
    /// of `host::main_window::core::with_window`).
    pub dispatcher: Arc<dyn UiDispatcher>,
    /// C# `environment` (default: the real Win32 reads).
    pub environment: Rc<ShellInputEnvironment>,
    /// C# `ILog? log`.
    pub log: Arc<dyn Log>,
    /// C# `Stopwatch.Frequency / 1000.0`: ticks of `ImageSurface::timestamp` per millisecond (`PointerInputController::new`).
    pub ticks_per_ms: f64,
}

impl fmt::Debug for ShellInputDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellInputDeps").finish_non_exhaustive()
    }
}

/// The mutable controllers; borrowed only for the duration of one pure call (never across the host or the OS).
struct InputState {
    /// C# `Pointer`.
    pointer: PointerInputController,
    /// C# `_router`.
    router: ShortcutRouter,
    /// C# `_version`: shared by the pointer controller and the Fit driver.
    versions: ViewportOperationVersion,
    /// C# `Clicks`.
    clicks: ClickCounter,
}

/// The shared core behind the [`ShellInput`] handle; the three handlers of 14a hold it as `Rc<dyn ...Port>`.
struct InputCore {
    /// The id the posted continuations carry (key of the UI-thread registry).
    id: u64,
    deps: ShellInputDeps,
    state: RefCell<InputState>,
    /// The settings object the router was last built from (D-W8B4I-7: a different `Arc` at a key press rebuilds the table).
    router_settings: RefCell<Arc<AppSettings>>,
    /// C# `OverlayHitTest`.
    overlay_hit_test: RefCell<Option<PointTest>>,
    /// C# `ActivationKeyOwned`.
    activation_key_owned: RefCell<Option<Predicate>>,
    /// C# `EscapeInterceptor`.
    escape_interceptor: RefCell<Option<Predicate>>,
    /// C# `PointerActivity` handlers.
    pointer_activity: RefCell<Vec<(EventHandlerId, ActivityHandler)>>,
    next_activity_id: Cell<EventHandlerId>,
    /// C# `LastPointerPosition`.
    last_pointer_position: Cell<Option<PointD>>,
    /// A pure pointer call is running: a nested `WM_CAPTURECHANGED` (from our own `ReleaseCapture`) is remembered instead of re-entering (D-W8B4I-4).
    in_pointer_call: Cell<bool>,
    capture_lost_while_busy: Cell<bool>,
    /// The `Deactivated` subscription, removed by `dispose`.
    deactivated_subscription: Cell<Option<EventHandlerId>>,
    disposed: Cell<bool>,
}

/// C# `ShellInput`: the input of the shell window. A cheap handle (`Clone` shares the core); `dispose` is explicit (C# `IDisposable`), `Drop` of the
/// last handle does nothing else.
#[derive(Clone)]
pub struct ShellInput {
    core: Rc<InputCore>,
}

impl fmt::Debug for ShellInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellInput").finish_non_exhaustive()
    }
}

/// A process-wide counter of input ids; the id travels in the `Send` continuation actions (D-W8B4I-3).
static NEXT_INPUT_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// UI-thread registry: input id to the (weak) core. A posted continuation finds the `!Send` core again through it (the technique of
    /// `host::main_window::core::with_window`). Entries are removed by `dispose` and by `Drop` of the core.
    static INPUTS: RefCell<HashMap<u64, Weak<InputCore>>> = RefCell::new(HashMap::new());
}

/// Runs `f` on the live core with this id on the CURRENT thread; `None` when it was disposed or dropped (or lives on another thread).
fn with_input<R>(id: u64, f: impl FnOnce(&InputCore) -> R) -> Option<R> {
    // The registry borrow ends before `f` runs: `f` may re-enter the registry (a nested post, a dispose).
    let core = INPUTS.with(|inputs| inputs.borrow().get(&id).and_then(Weak::upgrade))?;
    Some(f(&core))
}

fn unregister_input(id: u64) {
    // `try_with`: the thread-local may already be gone when the last handle drops during thread teardown.
    let _ = INPUTS.try_with(|inputs| {
        if let Ok(mut map) = inputs.try_borrow_mut() {
            map.remove(&id);
        }
    });
}

/// What a controller call asked the shell to do outside the call (recorded by [`ShellPointerCommands`], performed after every borrow is released).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerEffect {
    Next,
    Previous,
    ApplyFit,
}

/// C# `PointerCommands`: `has_images` is the one query allowed inside a call; the rest only records (D-W8-2 "posts the request").
struct ShellPointerCommands<'a> {
    host: &'a dyn ShellInputHost,
    effects: Vec<PointerEffect>,
}

impl PointerCommands for ShellPointerCommands<'_> {
    fn has_images(&self) -> bool {
        self.host.has_images()
    }

    fn next(&mut self) {
        self.effects.push(PointerEffect::Next);
    }

    fn previous(&mut self) {
        self.effects.push(PointerEffect::Previous);
    }

    fn apply_fit(&mut self) {
        self.effects.push(PointerEffect::ApplyFit);
    }
}

/// Resets `in_pointer_call` when a pure call ends, also when it panics.
struct BusyScope<'a>(&'a Cell<bool>);

impl<'a> BusyScope<'a> {
    fn enter(flag: &'a Cell<bool>) -> Self {
        flag.set(true);
        Self(flag)
    }
}

impl Drop for BusyScope<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// `FitSurface` over the shared surface cell: every call takes its own short borrow, because the Fit driver is called while the `cancel_pan`
/// closure needs the same surface (it runs before any other surface call, never inside one).
struct SurfaceFitAdapter<'a>(&'a RefCell<dyn InputSurface>);

impl FitSurface for SurfaceFitAdapter<'_> {
    fn viewport_size(&self) -> (f64, f64) {
        FitSurface::viewport_size(&*self.0.borrow())
    }

    fn is_loaded(&self) -> bool {
        FitSurface::is_loaded(&*self.0.borrow())
    }

    fn update_layout(&mut self) {
        FitSurface::update_layout(&mut *self.0.borrow_mut());
    }

    fn update_fit_size(&mut self) {
        FitSurface::update_fit_size(&mut *self.0.borrow_mut());
    }

    fn capture(&self) -> ViewportSnapshot {
        FitSurface::capture(&*self.0.borrow())
    }

    fn scroll_home(&mut self) {
        FitSurface::scroll_home(&mut *self.0.borrow_mut());
    }
}

/// Each operation borrows the shared viewer only for that operation. Source-size and synchronous
/// layout callbacks then observe the live updated viewer without colliding with a held guard.
struct SharedViewerAdapter<'a>(&'a ViewerHandle);

impl FitViewer for SharedViewerAdapter<'_> {
    fn reset_fit(&mut self, viewport_width: f64, viewport_height: f64) {
        FitViewer::reset_fit(&mut *self.0.borrow_mut(), viewport_width, viewport_height);
    }
}

macro_rules! shared_viewer_update {
    ($name:ident($($arg:ident: $kind:ty),* $(,)?) -> $result:ty) => {
        fn $name(&mut self, $($arg: $kind),*) -> $result {
            self.0.borrow_mut().$name($($arg),*)
        }
    };
}

impl PointerViewer for SharedViewerAdapter<'_> {
    fn is_fit(&self) -> bool {
        self.0.borrow().is_fit()
    }
    fn zoom(&self) -> f64 {
        self.0.borrow().zoom()
    }
    shared_viewer_update!(set_zoom(value: f64) -> ViewerUpdate);
    shared_viewer_update!(zoom_to_actual_size() -> ViewerUpdate);
    shared_viewer_update!(zoom_to_fit_width() -> ViewerUpdate);
    shared_viewer_update!(zoom_to_fit_height() -> ViewerUpdate);
    shared_viewer_update!(zoom_in() -> ViewerUpdate);
    shared_viewer_update!(zoom_out() -> ViewerUpdate);
    shared_viewer_update!(wheel_zoom(delta: i32) -> ViewerUpdate);
    shared_viewer_update!(update_viewport(width: f64, height: f64, force: bool) -> ViewerUpdate);
    shared_viewer_update!(
        apply_initial_view_mode(
            mode: InitialViewMode,
            width: f64,
            height: f64,
            click_zoom_percent: i32,
            keep_zoom: bool,
        ) -> Option<ViewerUpdate>
    );
}

/// Maps `ViewportOperationVersion` (`i64`) to `viewport::fit::OperationVersion` (`u64`). The cast only reinterprets the bits: equality and
/// supersession are preserved, including across the wrap at `i64::MAX`.
struct VersionAdapter<'a>(&'a mut ViewportOperationVersion);

impl OperationVersion for VersionAdapter<'_> {
    #[allow(clippy::cast_sign_loss)] // bit reinterpretation, see the type doc
    fn next(&mut self) -> u64 {
        self.0.next_version() as u64
    }

    #[allow(clippy::cast_sign_loss)] // bit reinterpretation, see the type doc
    fn current(&self) -> u64 {
        self.0.current() as u64
    }
}

/// The handle the window's message handlers hold: a `Weak` of the core, so the window never keeps the input alive (trap T8). A dead core answers
/// "not handled" and does nothing.
struct WeakPorts(Weak<InputCore>);

impl KeyInputPort for WeakPorts {
    fn handle_key_down(&self, key: KeyId, is_system_key: bool, is_repeat: bool, message_time: i32) -> bool {
        self.0
            .upgrade()
            .is_some_and(|core| core.handle_key_down_impl(key, is_system_key, is_repeat, message_time))
    }

    fn message_time(&self) -> i32 {
        self.0
            .upgrade()
            .map_or(0, |core| (core.deps.environment.message_time)())
    }
}

impl PointerInputPort for WeakPorts {
    fn handle_pointer_down(&self, button: PointerButton, pixel_x: i32, pixel_y: i32) -> bool {
        self.0
            .upgrade()
            .is_some_and(|core| core.handle_pointer_down_impl(button, pixel_x, pixel_y))
    }

    fn handle_pointer_move(&self, pixel_x: i32, pixel_y: i32, left_button_pressed: bool) {
        if let Some(core) = self.0.upgrade() {
            core.handle_pointer_move_impl(pixel_x, pixel_y, left_button_pressed);
        }
    }

    fn handle_left_button_up(&self, pixel_x: i32, pixel_y: i32) {
        if let Some(core) = self.0.upgrade() {
            core.handle_left_button_up_impl(pixel_x, pixel_y);
        }
    }

    fn handle_capture_lost(&self) {
        if let Some(core) = self.0.upgrade() {
            core.handle_capture_lost_impl();
        }
    }
}

impl WheelInputPort for WeakPorts {
    fn screen_to_client_dip(&self, screen_pixel: PointD) -> PointD {
        match self.0.upgrade() {
            Some(core) => WheelInputPort::screen_to_client_dip(&*core, screen_pixel),
            None => screen_pixel,
        }
    }

    fn handle_vertical_wheel(&self, delta: i32, ctrl: bool, client_dip: PointD) -> bool {
        self.0
            .upgrade()
            .is_some_and(|core| core.handle_vertical_wheel_impl(delta, ctrl, client_dip))
    }

    fn handle_horizontal_wheel(&self, delta: i32, ctrl: bool, client_dip: PointD) -> bool {
        self.0
            .upgrade()
            .is_some_and(|core| core.handle_horizontal_wheel_impl(delta, ctrl, client_dip))
    }
}

impl ShellInput {
    /// C# constructor: builds the controllers (`PointerInputController::new(ticks_per_ms)`, `ShortcutRouter::from_settings` of the current settings),
    /// installs the frame router on the surface, subscribes `Deactivated` (stops the glide) and adds the keyboard, pointer and wheel handlers to the
    /// window, in that order.
    pub fn new(deps: ShellInputDeps) -> ShellInput {
        let settings = (deps.settings)();
        let state = InputState {
            pointer: PointerInputController::new(deps.ticks_per_ms),
            router: ShortcutRouter::from_settings(&settings.shortcuts, &settings.actions),
            versions: ViewportOperationVersion::new(),
            clicks: ClickCounter::new(),
        };
        let id = NEXT_INPUT_ID.fetch_add(1, Ordering::Relaxed);
        let core = Rc::new(InputCore {
            id,
            deps,
            state: RefCell::new(state),
            router_settings: RefCell::new(settings),
            overlay_hit_test: RefCell::new(None),
            activation_key_owned: RefCell::new(None),
            escape_interceptor: RefCell::new(None),
            pointer_activity: RefCell::new(Vec::new()),
            next_activity_id: Cell::new(1),
            last_pointer_position: Cell::new(None),
            in_pointer_call: Cell::new(false),
            capture_lost_while_busy: Cell::new(false),
            deactivated_subscription: Cell::new(None),
            disposed: Cell::new(false),
        });
        INPUTS.with(|inputs| inputs.borrow_mut().insert(id, Rc::downgrade(&core)));

        // The frame router (D-W8B4I-5): a Weak, so the surface never keeps the input alive.
        let weak = Rc::downgrade(&core);
        let router: FrameRouter = Rc::new(move |ticks| {
            if let Some(core) = weak.upgrade() {
                core.on_render_frame(ticks);
            }
        });
        core.deps.surface.borrow_mut().set_frame_router(Some(router));

        // `Deactivated` stops the glide (it does not end a pan, like C#).
        let weak = Rc::downgrade(&core);
        let subscription = core.deps.window.subscribe(
            ShellWindowEvent::Deactivated,
            Box::new(move || {
                if let Some(core) = weak.upgrade() {
                    core.stop_kinetic_impl();
                }
            }),
        );
        core.deactivated_subscription.set(Some(subscription));

        // Keyboard, pointer, wheel, in that order.
        let ports = Rc::new(WeakPorts(Rc::downgrade(&core)));
        core.deps
            .window
            .add_message_handler(Rc::new(KeyboardMessageHandler::new(
                Rc::clone(&ports) as Rc<dyn KeyInputPort>
            )));
        core.deps.window.add_message_handler(Rc::new(PointerMessageHandler::new(
            Rc::clone(&ports) as Rc<dyn PointerInputPort>
        )));
        core.deps
            .window
            .add_message_handler(Rc::new(WheelMessageHandler::new(ports as Rc<dyn WheelInputPort>)));
        ShellInput { core }
    }

    /// C# `Dispose()`: idempotent; unsubscribes `Deactivated`, clears the frame router, `Pointer.OnWindowClosed()` (stops the glide, ends a pan).
    pub fn dispose(&self) {
        let core = &self.core;
        if core.disposed.replace(true) {
            return;
        }
        if let Some(subscription) = core.deactivated_subscription.take() {
            core.deps.window.unsubscribe(subscription);
        }
        core.deps.surface.borrow_mut().set_frame_router(None);
        core.with_pointer_surface(|pointer, surface| pointer.on_window_closed(surface));
        unregister_input(core.id);
    }

    /// C# `RebuildShortcuts(settings)` (R2-F-27: only when the settings change).
    pub fn rebuild_shortcuts(&self, settings: &AppSettings) {
        self.core
            .state
            .borrow_mut()
            .router
            .rebuild(&settings.shortcuts, &settings.actions);
    }

    /// C# `NoteCurrentIndexChanged(index)`: `Pointer.OnCurrentIndexChanged` (navigation stops a glide; a full-resolution swap of the same image does not).
    pub fn note_current_index_changed(&self, index: i32) {
        self.core
            .with_pointer_surface(|pointer, surface| pointer.on_current_index_changed(surface, index));
    }

    /// C# `OnZoomModeChanged` (the `ViewerState.ZoomModeChanged` handler): stops the glide. The Rust viewer has no event: this runs when a pointer
    /// call reports `ViewerUpdate::zoom_mode_changed` and when the main window sees a zoom or stretch bit in a view-model `ChangeSet` (14c).
    pub fn on_zoom_mode_changed(&self) {
        self.core.stop_kinetic_impl();
    }

    /// C# `ApplyInitialViewAsync()`: `Pointer.apply_initial_view(settings.initial_view_mode, settings.click_zoom_percent)`.
    pub fn apply_initial_view(&self) {
        let core = &self.core;
        let step = core.run_pointer(|pointer, ctx| {
            let mode = ctx.settings.initial_view_mode;
            let percent = ctx.settings.click_zoom_percent;
            pointer.apply_initial_view(ctx, mode, percent)
        });
        core.post_pointer_step(step);
    }

    /// C# `Pointer.OnWindowPreviewMouseDown()` (D-W8B4X-8): the window-level press tunnel without a mouse message, called by the main window's scroll-bar press hook
    /// (`MainWindowFeature::on_scroll_bar_press`, 14c's `InputFeature`). Runs `pointer.on_window_preview_mouse_down(surface)` (stops a kinetic glide and cancels a
    /// pending fit), under the same borrow discipline as `handle_pointer_down`.
    pub fn pointer_press_tunnel(&self) {
        self.core.pointer_press_tunnel_impl();
    }

    /// C# `ApplyFitAsync()`: `apply_fit_start(.., cancel_pan = Pointer.cancel_pan)`; an `AwaitRender` continuation is posted at Render priority and
    /// resumed with `apply_fit_resume`.
    pub fn apply_fit(&self) {
        self.core.apply_fit_impl();
    }

    /// C# `ExecuteAsync(command)` (public: menus and buttons run the same path as keys): the viewport commands run on the controllers
    /// (`ZoomActualSize`, `ToggleFit`, `ZoomIn`, `ZoomOut`, `ClickZoom`, `FitWidth`/`FitWidth2` with `misc::window_helpers::fit_width_anchor_for`,
    /// `FitHeight`), the rest goes to `ShellInputHost::execute`. An `Err` is logged and reported with the command name.
    pub fn execute(&self, command: ReviewCommand) {
        self.core.execute_guarded(command);
    }

    /// C# `Input.Pointer.SetClickZoomLevelAsync(percent)` (used by the Zoom menu, 14c and 14f): `set_click_zoom_level` plus its continuation.
    pub fn set_click_zoom_level(&self, percent: i32) {
        let core = &self.core;
        let step = core.run_pointer(|pointer, ctx| pointer.set_click_zoom_level(ctx, percent));
        core.post_pointer_step(step);
    }

    /// C# `Pointer.StopKinetic()` (Refresh and `Deactivated` use it).
    pub fn stop_kinetic(&self) -> bool {
        self.core.stop_kinetic_impl()
    }

    /// C# `OverlayHitTest` setter (WP-23 hook): true when the DIP point is over an overlay element that takes the mouse.
    pub fn set_overlay_hit_test(&self, test: Option<PointTest>) {
        *self.core.overlay_hit_test.borrow_mut() = test;
    }

    /// C# `ActivationKeyOwned` setter: a focused button or compare cell owns Space/Enter.
    pub fn set_activation_key_owned(&self, owned: Option<Predicate>) {
        *self.core.activation_key_owned.borrow_mut() = owned;
    }

    /// C# `EscapeInterceptor` setter: Esc closes the Tools popup first (R7-6); true = handled.
    pub fn set_escape_interceptor(&self, interceptor: Option<Predicate>) {
        *self.core.escape_interceptor.borrow_mut() = interceptor;
    }

    /// C# `PointerActivity +=`: mouse move or horizontal wheel (the auto-hide of the overlay).
    pub fn subscribe_pointer_activity(&self, handler: ActivityHandler) -> EventHandlerId {
        let id = self.core.next_activity_id.get();
        self.core.next_activity_id.set(id + 1);
        self.core.pointer_activity.borrow_mut().push((id, handler));
        id
    }

    /// C# `PointerActivity -=`; an unknown id is a no-op.
    pub fn unsubscribe_pointer_activity(&self, id: EventHandlerId) {
        self.core
            .pointer_activity
            .borrow_mut()
            .retain(|(existing, _)| *existing != id);
    }

    /// C# `LastPointerPosition`: the last mouse-move position in client DIP.
    pub fn last_pointer_position(&self) -> Option<PointD> {
        self.core.last_pointer_position.get()
    }

    /// The handler of 14a's keyboard decode (`Rc<dyn KeyInputPort>` over the shared core).
    pub fn key_port(&self) -> Rc<dyn KeyInputPort> {
        Rc::clone(&self.core) as Rc<dyn KeyInputPort>
    }

    /// The pointer port.
    pub fn pointer_port(&self) -> Rc<dyn PointerInputPort> {
        Rc::clone(&self.core) as Rc<dyn PointerInputPort>
    }

    /// The wheel port.
    pub fn wheel_port(&self) -> Rc<dyn WheelInputPort> {
        Rc::clone(&self.core) as Rc<dyn WheelInputPort>
    }
}

impl Drop for InputCore {
    fn drop(&mut self) {
        unregister_input(self.id);
    }
}

impl InputCore {
    /// C# `PixelsToDip` / `ClientDip(x, y)`: pixels over `window.dpi_scale()`.
    fn client_dip(&self, pixel_x: i32, pixel_y: i32) -> PointD {
        let scale = self.deps.window.dpi_scale();
        PointD {
            x: f64::from(pixel_x) / scale,
            y: f64::from(pixel_y) / scale,
        }
    }

    /// C# `IsOverImage(point)`: inside the image rectangle (`surface.image_origin()`, `image_actual_width/height`), `RectD::contains`.
    fn is_over_image(&self, point: PointD) -> bool {
        let surface = self.deps.surface.borrow();
        let origin = surface.image_origin();
        RectD {
            x: origin.x,
            y: origin.y,
            width: surface.image_actual_width(),
            height: surface.image_actual_height(),
        }
        .contains(point)
    }

    /// C# `IsInsideViewport(point)`: `0 <= x < viewport_width && 0 <= y < viewport_height`.
    fn is_inside_viewport(&self, point: PointD) -> bool {
        let surface = self.deps.surface.borrow();
        point.x >= 0.0 && point.y >= 0.0 && point.x < surface.viewport_width() && point.y < surface.viewport_height()
    }

    /// C# `IsOverOverlay(point)`.
    fn is_over_overlay(&self, point: PointD) -> bool {
        // The hook is cloned out first: it may call back into the input (set another hook).
        let test = self.overlay_hit_test.borrow().clone();
        test.is_some_and(|test| test(point))
    }

    /// Calls a parameterless predicate hook; unset = false.
    fn call_predicate(slot: &RefCell<Option<Predicate>>) -> bool {
        let predicate = slot.borrow().clone();
        predicate.is_some_and(|predicate| predicate())
    }

    /// C# `PointerActivity?.Invoke`: the handlers are cloned out of the list first, no borrow is held while they run.
    fn raise_pointer_activity(&self) {
        let handlers: Vec<ActivityHandler> = self
            .pointer_activity
            .borrow()
            .iter()
            .map(|(_, handler)| Rc::clone(handler))
            .collect();
        for handler in handlers {
            handler();
        }
    }

    /// Runs one controller call with scoped state/surface borrows and per-operation viewer access (D-W8B4I-1). In this order: settings read, `f`, state/surface borrows released, a
    /// `WM_CAPTURECHANGED` remembered during the call replayed, then the recorded effects (`next`, `previous`, `apply_fit`), then the viewer update
    /// (`zoom_mode_changed` stops the glide, C# `OnZoomModeChanged`). The caller posts the returned `PointerStep`, if any.
    fn run_pointer<R>(&self, f: impl FnOnce(&mut PointerInputController, &mut PointerContext<'_>) -> R) -> R {
        let settings = (self.deps.settings)();
        let (result, effects) = {
            let _busy = BusyScope::enter(&self.in_pointer_call);
            let mut guard = self.state.borrow_mut();
            let state = &mut *guard;
            let mut viewer = SharedViewerAdapter(&self.deps.viewer);
            let mut surface = self.deps.surface.borrow_mut();
            let mut commands = ShellPointerCommands {
                host: &*self.deps.host,
                effects: Vec::new(),
            };
            let result = {
                let mut ctx = PointerContext {
                    surface: &mut *surface,
                    viewer: &mut viewer,
                    settings: &settings,
                    versions: &mut state.versions,
                    commands: &mut commands,
                    log: &*self.deps.log,
                };
                f(&mut state.pointer, &mut ctx)
            };
            (result, commands.effects)
        };
        self.replay_capture_lost();
        for effect in effects {
            match effect {
                PointerEffect::Next => self.deps.host.next(),
                PointerEffect::Previous => self.deps.host.previous(),
                PointerEffect::ApplyFit => self.apply_fit_impl(),
            }
        }
        let update = self.state.borrow_mut().pointer.take_viewer_update();
        if update.zoom_mode_changed {
            self.stop_kinetic_impl();
        }
        if !update.changes.is_empty() {
            self.deps.window.invalidate();
        }
        result
    }

    /// Like [`InputCore::run_pointer`] for the calls that only need the controller and the surface (`stop_kinetic`, `cancel_pan`, ...): the viewer and
    /// the settings are not touched, so a stop can never collide with a viewer borrow of the host.
    fn with_pointer_surface<R>(&self, f: impl FnOnce(&mut PointerInputController, &mut dyn ImageSurface) -> R) -> R {
        let result = {
            let _busy = BusyScope::enter(&self.in_pointer_call);
            let mut guard = self.state.borrow_mut();
            let mut surface = self.deps.surface.borrow_mut();
            f(&mut guard.pointer, &mut *surface)
        };
        self.replay_capture_lost();
        result
    }

    /// A `WM_CAPTURECHANGED` that arrived while a pure call was running (from our own `ReleaseCapture`) is replayed ONCE after the borrows are gone
    /// (D-W8B4I-4): the outer call already cancelled the pan, so this second `cancel_pan` is idempotent. It is not replayed again if that call
    /// provokes another message (`release_mouse_capture` only releases a capture it holds).
    fn replay_capture_lost(&self) {
        if self.capture_lost_while_busy.replace(false) {
            {
                let _busy = BusyScope::enter(&self.in_pointer_call);
                let mut guard = self.state.borrow_mut();
                let mut surface = self.deps.surface.borrow_mut();
                guard.pointer.on_lost_capture(&mut *surface);
            }
            self.capture_lost_while_busy.set(false);
        }
    }

    fn stop_kinetic_impl(&self) -> bool {
        self.with_pointer_surface(|pointer, surface| pointer.stop_kinetic(surface))
    }

    fn pointer_press_tunnel_impl(&self) {
        self.with_pointer_surface(|pointer, surface| pointer.on_window_preview_mouse_down(surface));
    }

    /// C# `ApplyFitAsync()` up to the render yield.
    fn apply_fit_impl(&self) {
        let step = {
            let _busy = BusyScope::enter(&self.in_pointer_call);
            let mut guard = self.state.borrow_mut();
            let InputState { pointer, versions, .. } = &mut *guard;
            let surface_cell: &RefCell<dyn InputSurface> = &self.deps.surface;
            let mut fit_surface = SurfaceFitAdapter(surface_cell);
            let mut fit_viewer = SharedViewerAdapter(&self.deps.viewer);
            let mut version_adapter = VersionAdapter(versions);
            apply_fit_start(
                &mut fit_surface,
                &mut fit_viewer,
                &mut version_adapter,
                &mut || pointer.cancel_pan(&mut *surface_cell.borrow_mut()),
                is_stable_viewport,
            )
        };
        self.finish_fit_step(step);
    }

    /// The continuation of a Fit pass (`apply_fit_resume`), run from the posted action with fresh borrows.
    fn resume_fit(&self, continuation: FitContinuation) {
        let step = {
            let _busy = BusyScope::enter(&self.in_pointer_call);
            let mut guard = self.state.borrow_mut();
            let mut fit_surface = SurfaceFitAdapter(&self.deps.surface);
            let mut version_adapter = VersionAdapter(&mut guard.versions);
            apply_fit_resume(continuation, &mut fit_surface, &mut version_adapter, is_stable_viewport)
        };
        self.finish_fit_step(step);
    }

    /// After a Fit step: replay a remembered capture loss, honour the `ZoomModeChanged` of `ResetFit` (the `FitViewer` seam returns `()`, so the
    /// viewer keeps it pending), repaint, and post the next pass.
    fn finish_fit_step(&self, step: FitStep) {
        self.replay_capture_lost();
        let zoom_mode_changed = self.deps.viewer.borrow_mut().take_pending_zoom_mode_changed();
        if zoom_mode_changed {
            self.stop_kinetic_impl();
        }
        self.deps.window.invalidate();
        self.post_fit_step(step);
    }

    fn post_fit_step(&self, step: FitStep) {
        if let FitStep::AwaitRender(continuation) = step {
            let id = self.id;
            self.deps.dispatcher.post_with_priority(
                Box::new(move || {
                    with_input(id, |core| core.resume_fit(continuation));
                }),
                UiPriority::Render,
            );
        }
    }

    /// `execute` with the error mapping of `ExecuteGuardedAsync`: an `Err` of the host is logged and reported with the command name.
    fn execute_guarded(&self, command: ReviewCommand) {
        if let Err(error) = self.execute_command(command) {
            let name = format!("{:?}", command.kind);
            self.deps.log.error(&format!("Command {name} failed"), Some(&*error));
            self.deps.host.report_action_failed(&name, &*error);
        }
    }

    /// C# `ExecuteAsync(command)`: the viewport commands on the controllers, everything else on the host (after every borrow was released).
    fn execute_command(&self, command: ReviewCommand) -> Result<(), Box<dyn Error>> {
        match command.kind {
            ReviewCommandType::ZoomActualSize => {
                let step = self.run_pointer(|pointer, ctx| pointer.zoom_actual_size(ctx));
                self.post_pointer_step(step);
            }
            ReviewCommandType::ToggleFit => self.apply_fit_impl(),
            ReviewCommandType::ZoomIn => {
                let step = self.run_pointer(|pointer, ctx| pointer.zoom_in(ctx));
                self.post_pointer_step(step);
            }
            ReviewCommandType::ZoomOut => {
                let step = self.run_pointer(|pointer, ctx| pointer.zoom_out(ctx));
                self.post_pointer_step(step);
            }
            ReviewCommandType::ClickZoom => {
                let step = self.run_pointer(|pointer, ctx| pointer.toggle_click_zoom(ctx));
                self.post_pointer_step(step);
            }
            ReviewCommandType::FitWidth | ReviewCommandType::FitWidth2 => {
                let second = command.kind == ReviewCommandType::FitWidth2;
                let step = self.run_pointer(|pointer, ctx| {
                    let anchor = window_helpers::fit_width_anchor_for(ctx.settings, second);
                    pointer.fit_width(ctx, anchor)
                });
                self.post_pointer_step(step);
            }
            ReviewCommandType::FitHeight => {
                let step = self.run_pointer(|pointer, ctx| pointer.fit_height(ctx));
                self.post_pointer_step(step);
            }
            _ => return self.deps.host.execute(command),
        }
        Ok(())
    }

    /// C# `HandleKeyDown`: the decision order (see the contract), then the command OUTSIDE every borrow.
    fn handle_key_down_impl(&self, key: KeyId, is_system_key: bool, is_repeat: bool, message_time: i32) -> bool {
        let host = &self.deps.host;
        let modifiers = (self.deps.environment.modifiers)();

        // An arrow the controller consumes decides about the glide itself; every other key stops it unconditionally.
        let arrow_consumed = modifiers == KeyModifiers::NONE
            && !host.is_compare_visible()
            && self.run_pointer(|pointer, ctx| pointer.try_pan_by_arrow(ctx, key, is_repeat));
        if !arrow_consumed {
            self.stop_kinetic_impl();
        }

        let perf = PhotoReviewPerf::log();
        if perf.is_enabled() {
            let delay = (self.deps.environment.tick_count)().wrapping_sub(message_time);
            perf.key_input(0, key.name(), f64::from(delay));
        }

        if arrow_consumed {
            return true;
        }
        // R7-6: Esc closes the Tools popup first, not the application.
        if key == KeyId::ESCAPE && Self::call_predicate(&self.escape_interceptor) {
            return true;
        }
        // Q-R25: Esc during a duplicate check cancels it (before leaving fullscreen or closing); the key is swallowed.
        if key == KeyId::ESCAPE && host.cancel_duplicate_check() {
            return true;
        }
        // Space/Enter belong to a focused button or compare cell.
        if (key == KeyId::SPACE || key == KeyId::ENTER) && Self::call_predicate(&self.activation_key_owned) {
            return false;
        }

        self.rebuild_router_if_settings_changed();
        let route_state = RouteState {
            is_fullscreen: host.is_fullscreen(),
            has_image: host.has_images(),
            has_compare_pair: host.current_has_compare_pair(),
            is_compare_visible: host.is_compare_visible(),
            has_capture_pair: host.current_has_capture_pair(),
        };
        let command = self.state.borrow().router.try_resolve(
            if is_system_key { KeyId::SYSTEM } else { key },
            if is_system_key { key } else { KeyId::NONE },
            modifiers,
            route_state,
        );
        let Some(command) = command else {
            return false;
        };
        // R2-F-06: a file action never repeats while the key is held; the key is still swallowed.
        if !(is_repeat && command.kind.ignores_auto_repeat()) {
            self.execute_guarded(command);
        }
        true
    }

    /// D-W8B4I-7: the settings object changed since the router was built: rebuild lazily at the next key.
    fn rebuild_router_if_settings_changed(&self) {
        let current = (self.deps.settings)();
        let mut last = self.router_settings.borrow_mut();
        if !Arc::ptr_eq(&current, &last) {
            self.state
                .borrow_mut()
                .router
                .rebuild(&current.shortcuts, &current.actions);
            *last = current;
        }
    }

    /// C# `HandlePointerDown`.
    fn handle_pointer_down_impl(&self, button: PointerButton, pixel_x: i32, pixel_y: i32) -> bool {
        let environment = &self.deps.environment;
        let time = (environment.message_time)();
        let (slop_width, slop_height) = (environment.double_click_size_pixels)(dpi_of(self.deps.window.dpi_scale()));
        let double_click_time = (environment.double_click_time_ms)();
        // Registered before the routing, so a rejected press still counts toward a later double click (trap T3).
        let click_count = self.state.borrow_mut().clicks.register(
            button,
            pixel_x,
            pixel_y,
            time,
            double_click_time,
            slop_width,
            slop_height,
        );
        // The window-level tunnel runs for EVERY button, also over the overlay and while comparing.
        self.pointer_press_tunnel_impl();

        let point = self.client_dip(pixel_x, pixel_y);
        if self.is_over_overlay(point) || self.deps.host.is_compare_visible() {
            return false;
        }
        match button {
            PointerButton::Left if self.is_over_image(point) => self
                .run_pointer(|pointer, ctx| pointer.on_image_press(ctx, PointerButton::Left, click_count, point, time)),
            PointerButton::Middle if self.is_inside_viewport(point) => {
                if click_count == 1 && self.deps.host.has_images() {
                    let action = (self.deps.settings)().middle_click_action_or_default();
                    if let Some(command) = resolve_middle_click(action) {
                        self.execute_guarded(command);
                    }
                }
                // ImageScroll_PreviewMouseDown: e.Handled = true for every middle press inside the viewport.
                true
            }
            _ => false,
        }
    }

    /// C# `HandlePointerMove`.
    fn handle_pointer_move_impl(&self, pixel_x: i32, pixel_y: i32, left_button_pressed: bool) {
        let point = self.client_dip(pixel_x, pixel_y);
        self.last_pointer_position.set(Some(point));
        self.raise_pointer_activity();
        if self.is_over_overlay(point) && !self.deps.window.has_pointer_capture() {
            return;
        }
        if self.deps.window.has_pointer_capture() || (self.is_over_image(point) && !self.deps.host.is_compare_visible())
        {
            let time = (self.deps.environment.message_time)();
            let _handled =
                self.run_pointer(|pointer, ctx| pointer.on_image_move(ctx, left_button_pressed, point, time));
        }
    }

    /// C# `HandleLeftButtonUp`.
    fn handle_left_button_up_impl(&self, pixel_x: i32, pixel_y: i32) {
        let point = self.client_dip(pixel_x, pixel_y);
        if self.deps.window.has_pointer_capture()
            || (self.is_over_image(point) && !self.is_over_overlay(point) && !self.deps.host.is_compare_visible())
        {
            let time = (self.deps.environment.message_time)();
            let release = self.run_pointer(|pointer, ctx| pointer.on_image_release(ctx, point, time));
            self.post_pointer_step(release.step);
        }
    }

    /// C# `HandleCaptureLost`: `Pointer.on_lost_capture`, or remembered when a pointer call is running (D-W8B4I-4).
    fn handle_capture_lost_impl(&self) {
        // `try_borrow_mut` so a missed flag can never panic: a state that is borrowed is by definition inside a call.
        if self.in_pointer_call.get() || self.state.try_borrow_mut().is_err() {
            self.capture_lost_while_busy.set(true);
            return;
        }
        self.with_pointer_surface(|pointer, surface| pointer.on_lost_capture(surface));
    }

    /// C# `HandleVerticalWheel`.
    fn handle_vertical_wheel_impl(&self, delta: i32, ctrl: bool, client_dip: PointD) -> bool {
        if self.is_over_overlay(client_dip) || self.deps.host.is_compare_visible() {
            return false;
        }
        let environment = &self.deps.environment;
        let message_time = (environment.message_time)();
        // The device hint is valid only while the message is dispatched: read it here, inside the call.
        let input = WheelInput::new(delta, false, ctrl, message_time).with_hint((environment.wheel_device)());
        let step = self.run_pointer(|pointer, ctx| pointer.on_wheel(ctx, input, client_dip));
        self.post_pointer_step(step);
        true
    }

    /// C# `HandleHorizontalWheel`.
    fn handle_horizontal_wheel_impl(&self, delta: i32, ctrl: bool, client_dip: PointD) -> bool {
        if !self.deps.window.is_loaded() || self.deps.host.is_compare_visible() || !self.is_inside_viewport(client_dip)
        {
            return false;
        }
        self.raise_pointer_activity();
        let environment = &self.deps.environment;
        // WM_MOUSEHWHEEL has no message time in the WPF version: the tick count is the time stamp.
        let tick_count = (environment.tick_count)();
        let input = WheelInput::new(delta, true, ctrl, tick_count).with_hint((environment.wheel_device)());
        let step = self.run_pointer(|pointer, ctx| pointer.on_wheel(ctx, input, client_dip));
        self.post_pointer_step(step);
        true
    }

    /// Routes one frame to `Pointer.on_kinetic_frame` while the glide is hooked (the surface's frame router).
    fn on_render_frame(&self, rendering_time_ticks: Option<i64>) {
        let hooked = self
            .state
            .try_borrow()
            .is_ok_and(|state| state.pointer.is_kinetic_hooked());
        if hooked {
            self.run_pointer(|pointer, ctx| pointer.on_kinetic_frame(ctx, rendering_time_ticks));
        }
    }

    /// Posts the continuation of a `PointerStep::AwaitRender` at Render priority (`UiPriority::Render`): the `Send` action holds this input's id and
    /// the continuation, looks the core up in the UI-thread registry and calls `resume` with fresh borrows; a missing core (disposed) drops it. A
    /// continuation that returns another step is posted again.
    fn post_pointer_step(&self, step: PointerStep) {
        if let PointerStep::AwaitRender(continuation) = step {
            let id = self.id;
            self.deps.dispatcher.post_with_priority(
                Box::new(move || {
                    with_input(id, |core| core.resume_pointer(continuation));
                }),
                UiPriority::Render,
            );
        }
    }

    /// The continuation of a zoom pass (`PointerInputController::resume`), run from the posted action with fresh borrows.
    fn resume_pointer(&self, continuation: PointerContinuation) {
        let step = self.run_pointer(|pointer, ctx| pointer.resume(ctx, continuation));
        self.post_pointer_step(step);
    }

    /// The host-point twin conversion (`host::contracts::PointD` to `pr_app` `PointD`) until the layering follow-up removes the twin (D-W8B3H-1).
    fn from_host_point(point: HostPointD) -> PointD {
        PointD { x: point.x, y: point.y }
    }
}

impl KeyInputPort for InputCore {
    fn handle_key_down(&self, key: KeyId, is_system_key: bool, is_repeat: bool, message_time: i32) -> bool {
        self.handle_key_down_impl(key, is_system_key, is_repeat, message_time)
    }

    fn message_time(&self) -> i32 {
        (self.deps.environment.message_time)()
    }
}

impl PointerInputPort for InputCore {
    fn handle_pointer_down(&self, button: PointerButton, pixel_x: i32, pixel_y: i32) -> bool {
        self.handle_pointer_down_impl(button, pixel_x, pixel_y)
    }

    fn handle_pointer_move(&self, pixel_x: i32, pixel_y: i32, left_button_pressed: bool) {
        self.handle_pointer_move_impl(pixel_x, pixel_y, left_button_pressed);
    }

    fn handle_left_button_up(&self, pixel_x: i32, pixel_y: i32) {
        self.handle_left_button_up_impl(pixel_x, pixel_y);
    }

    fn handle_capture_lost(&self) {
        self.handle_capture_lost_impl();
    }
}

impl WheelInputPort for InputCore {
    fn screen_to_client_dip(&self, screen_pixel: PointD) -> PointD {
        let host = self.deps.window.screen_to_client_dip(HostPointD {
            x: screen_pixel.x,
            y: screen_pixel.y,
        });
        Self::from_host_point(host)
    }

    fn handle_vertical_wheel(&self, delta: i32, ctrl: bool, client_dip: PointD) -> bool {
        self.handle_vertical_wheel_impl(delta, ctrl, client_dip)
    }

    fn handle_horizontal_wheel(&self, delta: i32, ctrl: bool, client_dip: PointD) -> bool {
        self.handle_horizontal_wheel_impl(delta, ctrl, client_dip)
    }
}
