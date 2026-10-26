//! Shared fixture of the main-window tests (W8-13g creates it; 13h extends it, 13i..13k may reuse it): a Win32 UI thread, a hidden shell window, fake
//! collaborators (hub, cloak, placement) and a real `MainViewModel` from `pr-app/tests/support/main_env.rs`.
//!
//! The including test crate declares, at its root: `#[path = ".."] mod file_action_fakes;`, `mod support { pub mod main_env; pub mod presenter_fakes;
//! pub mod preview_fakes; }` (paths into `crates/pr-app/tests`), `ui_thread`, `shell_window_fixture` and this file. `main_env.rs` compiles in
//! `pr-platform-win` as is (no Cargo change). Nothing here shows or activates a window; every window is destroyed by [`finish`] (or by the drop of
//! [`Built`]).
//!
//! Everything but the pure helpers lives on the UI thread: [`start_ui`] builds a `Win32UiDispatcher` there, [`run`] sends a closure to it (a panic in
//! the closure is re-raised on the test thread), [`build`] makes the window and keeps it in a thread-local ([`built`]).
#![allow(dead_code)] // shared by several test crates, each of which uses a different part

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use pr_app::coordinators::file_action_gate::FileActionGate;
use pr_app::services::viewport_size_source::ViewportSizeSource;
use pr_app::viewmodels::main::{ChangeSet, ViewerHandle};
use pr_app::windowing::contracts::WindowShowState;
use pr_core::abstractions::{Log, NullLog, UiScheduler};
use pr_imaging::decode_types::PlatformImage;
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::host::frame_clock::base::HostFrameClock;
use pr_platform_win::host::main_window::core::{
    HubImageHandler, MainWindow, MainWindowDeps, MainWindowFeature, MainWindowOptions, PlacementSavePort,
    PresentationHubPort, RenderSurfaceFactoryHook, StartupCloakPort, ViewportCallbacks,
};
use pr_platform_win::host::shell_window::{ShellWindow, ShellWindowOptions};
use pr_platform_win::host::ui_dispatcher::Win32UiDispatcher;
use pr_platform_win::render::device::RenderFault;
use pr_platform_win::render::surface::{D2dRenderSurface, D2dRenderSurfaceFactory, SurfaceOptions};
use pr_platform_win::shell_viewing::viewport_controller::{ViewportController, ViewportControllerOptions};
use pr_platform_win::win32::structs::Hwnd;
use pr_test_support::{QueuedUiScheduler, RecordingLog, TempRoot};

use crate::shell_window_fixture::{clock, create_window, dispose_window};
use crate::support::main_env::{EventPreload, MainEnv, ManualRunner};
use crate::ui_thread::{BOUND, UiParts, UiThread};

/// An ordered text log shared by the fakes (UI thread only).
pub type EventLog = Rc<RefCell<Vec<String>>>;

/// A new empty [`EventLog`].
pub fn new_order() -> EventLog {
    Rc::new(RefCell::new(Vec::new()))
}

// ---- the UI thread ----------------------------------------------------------------------------------------------------------------------------

/// A UI thread with a real `Win32UiDispatcher` (message-only window; nothing is shown).
pub fn start_ui() -> UiThread {
    UiThread::start_with(|| {
        let log: Arc<dyn Log> = Arc::new(NullLog);
        let dispatcher = Win32UiDispatcher::new(log).expect("dispatcher");
        let poster = dispatcher.clone();
        UiParts {
            dispatcher: Rc::new(dispatcher),
            post: Arc::new(move |task| poster.post(task)),
        }
    })
}

/// The dispatcher of the calling UI thread.
pub fn dispatcher() -> Win32UiDispatcher {
    Win32UiDispatcher::from_current_thread().expect("a Win32UiDispatcher on this thread")
}

/// Runs `task` on the UI thread and returns its result; a panic in `task` is re-raised HERE (the dispatcher would swallow it and the call would hang).
pub fn run<R: Send + 'static>(ui: &UiThread, task: impl FnOnce() -> R + Send + 'static) -> R {
    let outcome: Result<R, Box<dyn Any + Send>> = ui.invoke(move || catch_unwind(AssertUnwindSafe(task)));
    match outcome {
        Ok(value) => value,
        Err(payload) => resume_unwind(payload),
    }
}

/// Waits (bounded, no sleep) until `condition` is true; every probe is a round trip through the UI thread, which keeps serving its queues.
pub fn wait_until(ui: &UiThread, what: &str, condition: impl Fn() -> bool + Clone + Send + 'static) {
    let deadline = Instant::now() + BOUND;
    loop {
        let probe = condition.clone();
        if run(ui, probe) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::yield_now();
    }
}

/// Destroys the window (drop of the fixture + `dispose_window`) and ends the UI thread (the dispatcher's message-only window dies with the thread; shutting it
/// down first would drop the posted quit). Always call it last.
pub fn finish(ui: UiThread) {
    run(&ui, || {
        BUILT.with(|slot| slot.borrow_mut().take());
        dispose_window();
    });
    ui.dispose().expect("the UI thread ends cleanly");
}

// ---- fakes ---------------------------------------------------------------------------------------------------------------------------------------

/// The hub port: keeps the subscribers (so the test can call them) and the `note_frame_presented` calls.
pub struct FakeHub {
    current: RefCell<Option<PlatformImage>>,
    handlers: RefCell<Vec<(u64, HubImageHandler)>>,
    next_id: Cell<u64>,
    /// `note_frame_presented(contained_image)` calls.
    pub presented: RefCell<Vec<bool>>,
    /// Shared order log (`hub.subscribe` / `hub.unsubscribe`).
    pub order: EventLog,
    /// Called inside `subscribe_image_changed` (what did the window look like at that point of the constructor?).
    pub on_subscribe: RefCell<Option<Box<dyn Fn()>>>,
}

impl FakeHub {
    pub fn new(order: EventLog) -> Self {
        Self {
            current: RefCell::new(None),
            handlers: RefCell::new(Vec::new()),
            next_id: Cell::new(1),
            presented: RefCell::new(Vec::new()),
            order,
            on_subscribe: RefCell::new(None),
        }
    }

    /// How many `ImageChanged` subscribers there are.
    pub fn subscriber_count(&self) -> usize {
        self.handlers.borrow().len()
    }

    /// Calls every subscriber like the real hub does.
    pub fn raise(&self, image: Option<&PlatformImage>, is_file_change: bool) {
        *self.current.borrow_mut() = image.cloned();
        let handlers: Vec<HubImageHandler> = self.handlers.borrow().iter().map(|(_, h)| Rc::clone(h)).collect();
        for handler in handlers {
            handler(image, is_file_change);
        }
    }
}

impl std::fmt::Debug for FakeHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeHub").finish_non_exhaustive()
    }
}

impl PresentationHubPort for FakeHub {
    fn current_image(&self) -> Option<PlatformImage> {
        self.current.borrow().clone()
    }

    fn subscribe_image_changed(&self, handler: HubImageHandler) -> u64 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.handlers.borrow_mut().push((id, handler));
        self.order.borrow_mut().push("hub.subscribe".to_owned());
        if let Some(hook) = self.on_subscribe.borrow().as_ref() {
            hook();
        }
        id
    }

    fn unsubscribe_image_changed(&self, id: u64) {
        self.handlers.borrow_mut().retain(|(known, _)| *known != id);
        self.order.borrow_mut().push("hub.unsubscribe".to_owned());
    }

    fn note_frame_presented(&self, contained_image: bool) {
        self.presented.borrow_mut().push(contained_image);
    }
}

/// The cloak port: counts `stop` and `note_first_present`.
pub struct FakeCloak {
    pub stops: Cell<u32>,
    pub first_presents: Cell<u32>,
    pub order: EventLog,
}

impl std::fmt::Debug for FakeCloak {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeCloak").finish_non_exhaustive()
    }
}

impl StartupCloakPort for FakeCloak {
    fn stop(&self) {
        self.stops.set(self.stops.get() + 1);
        self.order.borrow_mut().push("cloak.stop".to_owned());
    }

    fn note_first_present(&self) {
        self.first_presents.set(self.first_presents.get() + 1);
    }
}

/// One `PlacementSavePort::save` call.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedPlacement {
    pub hwnd: Hwnd,
    pub file: PathBuf,
    pub restore_state: Option<WindowShowState>,
    pub restore_bounds: Option<(i32, i32, i32, i32)>,
    /// `has_surface()` of the main window at the moment of the call is NOT recorded here: see the order log (`placement.save`).
    pub window_alive: bool,
}

/// The placement port: records the calls.
pub struct FakePlacement {
    pub saves: RefCell<Vec<SavedPlacement>>,
    pub order: EventLog,
}

impl std::fmt::Debug for FakePlacement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakePlacement").finish_non_exhaustive()
    }
}

impl PlacementSavePort for FakePlacement {
    fn save(
        &self,
        hwnd: Hwnd,
        file: &Path,
        restore_state: Option<WindowShowState>,
        restore_bounds: Option<(i32, i32, i32, i32)>,
    ) -> bool {
        self.order.borrow_mut().push("placement.save".to_owned());
        self.saves.borrow_mut().push(SavedPlacement {
            hwnd,
            file: file.to_path_buf(),
            restore_state,
            restore_bounds,
            window_alive: hwnd != 0,
        });
        true
    }
}

/// A hook of [`RecordingFeature::on_changes`].
pub type ChangesHook = Box<dyn Fn(&ChangeSet)>;

/// A hook of [`FakeHub::on_subscribe`] / [`BuildOptions::on_hub_subscribe`].
pub type HubHook = Box<dyn Fn(&HubProbe)>;

/// A feature that records its hooks in an order log (and its own drop).
pub struct RecordingFeature {
    pub name: &'static str,
    pub log: EventLog,
    /// Called from `on_changes` with the change set (a test hooks re-entrancy or borrow checks here).
    pub on_changes_hook: RefCell<Option<ChangesHook>>,
}

impl RecordingFeature {
    pub fn new(name: &'static str, log: EventLog) -> Rc<Self> {
        Rc::new(Self {
            name,
            log,
            on_changes_hook: RefCell::new(None),
        })
    }
}

impl std::fmt::Debug for RecordingFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordingFeature")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Drop for RecordingFeature {
    fn drop(&mut self) {
        self.log.borrow_mut().push(format!("{}:drop", self.name));
    }
}

impl MainWindowFeature for RecordingFeature {
    fn name(&self) -> &'static str {
        self.name
    }

    fn release_renderer(&self) {
        self.log.borrow_mut().push(format!("{}:release_renderer", self.name));
    }

    fn dispose(&self) {
        self.log.borrow_mut().push(format!("{}:dispose", self.name));
    }

    fn on_changes(&self, changes: &ChangeSet) {
        self.log.borrow_mut().push(format!("{}:on_changes", self.name));
        if let Some(hook) = self.on_changes_hook.borrow().as_ref() {
            hook(changes);
        }
    }

    fn on_current_index_changed(&self, index: i32) {
        self.log.borrow_mut().push(format!("{}:index={index}", self.name));
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Surface hook: a WARP offscreen surface of `width` x `height` (a size different from the window's client forces the resize branch).
pub struct WarpHook {
    pub width: i32,
    pub height: i32,
    pub calls: Cell<u32>,
    pub options: RefCell<Vec<SurfaceOptions>>,
}

impl WarpHook {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            calls: Cell::new(0),
            options: RefCell::new(Vec::new()),
        }
    }
}

impl RenderSurfaceFactoryHook for WarpHook {
    fn create(&self, _hwnd: Hwnd, options: SurfaceOptions) -> Result<D2dRenderSurface, RenderFault> {
        self.calls.set(self.calls.get() + 1);
        self.options.borrow_mut().push(options);
        D2dRenderSurfaceFactory.create_offscreen(self.width, self.height, 1.0)
    }
}

/// Surface hook that fails `failures` times and then delegates to a WARP offscreen surface.
pub struct FlakyHook {
    pub failures: Cell<u32>,
    pub width: i32,
    pub height: i32,
}

impl RenderSurfaceFactoryHook for FlakyHook {
    fn create(&self, _hwnd: Hwnd, _options: SurfaceOptions) -> Result<D2dRenderSurface, RenderFault> {
        if self.failures.get() > 0 {
            self.failures.set(self.failures.get() - 1);
            return Err(RenderFault::InvalidOperation {
                message: "injected surface failure",
            });
        }
        D2dRenderSurfaceFactory.create_offscreen(self.width, self.height, 1.0)
    }
}

// ---- the built window -------------------------------------------------------------------------------------------------------------------------

/// What [`build`] makes (UI thread only).
pub struct Built {
    pub window: MainWindow,
    pub shell: Rc<ShellWindow>,
    pub hub: Rc<FakeHub>,
    pub cloak: Rc<FakeCloak>,
    pub placement: Rc<FakePlacement>,
    /// Marks the window recorded (`d3dDeviceReady`, `openPathBegin`, ...), in order.
    pub marks: EventLog,
    /// Factory/hub/cloak/placement events in order.
    pub order: EventLog,
    pub size_source: Arc<ViewportSizeSource>,
    pub log: Arc<RecordingLog>,
    pub preload: Arc<EventPreload>,
    pub gate: FileActionGate,
    pub scheduler: Arc<QueuedUiScheduler>,
    pub runner: Arc<ManualRunner>,
    pub photos: PathBuf,
    /// Dropped LAST: the folder of the view model's collaborators.
    root: TempRoot,
}

impl std::fmt::Debug for Built {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Built").finish_non_exhaustive()
    }
}

/// What a hub-subscription hook can look at.
pub struct HubProbe {
    pub size_source: Arc<ViewportSizeSource>,
    pub viewer: ViewerHandle,
    pub viewport: Rc<ViewportController>,
    pub shell: Rc<ShellWindow>,
}

/// How [`build`] sets things up.
pub struct BuildOptions {
    pub options: MainWindowOptions,
    pub window: ShellWindowOptions,
    /// Runs inside the hub subscription of the constructor (see [`FakeHub::on_subscribe`]): what did the window look like at that point?
    pub on_hub_subscribe: Option<HubHook>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            options: MainWindowOptions {
                // The real run activates; a test never takes the focus.
                activate: false,
                // WARP: no GPU keep-warm thread in tests.
                prefer_warp: true,
                ..MainWindowOptions::default()
            },
            window: ShellWindowOptions {
                width_dip: 640.0,
                height_dip: 480.0,
                ..ShellWindowOptions::default()
            },
            on_hub_subscribe: None,
        }
    }
}

thread_local! {
    static BUILT: RefCell<Option<Rc<Built>>> = const { RefCell::new(None) };
}

/// The window made by [`build`] on this UI thread.
pub fn built() -> Rc<Built> {
    BUILT.with(|slot| slot.borrow().clone()).expect("build() first")
}

/// Makes a hidden shell window, a real `MainViewModel` and the main window over them. UI thread only. The window is NOT shown.
pub fn build(build_options: BuildOptions) -> Rc<Built> {
    build_with_env(build_options, MainEnv::new())
}

/// The same real window with a caller-owned decoder/sink fixture.
pub fn build_with_env(build_options: BuildOptions, env: MainEnv) -> Rc<Built> {
    let BuildOptions {
        options,
        window: window_options,
        on_hub_subscribe,
    } = build_options;
    let shell = create_window(window_options);
    let frame_clock: Rc<dyn HostFrameClock> = clock();
    let MainEnv {
        vm,
        photos,
        runner,
        scheduler,
        preload,
        log,
        gate,
        root,
        ..
    } = env;
    let viewer = vm.viewer().clone();
    let order = new_order();
    let marks = new_order();
    let size_source = Arc::new(ViewportSizeSource::new());
    let hub = Rc::new(FakeHub::new(Rc::clone(&order)));
    let cloak = Rc::new(FakeCloak {
        stops: Cell::new(0),
        first_presents: Cell::new(0),
        order: Rc::clone(&order),
    });
    let placement = Rc::new(FakePlacement {
        saves: RefCell::new(Vec::new()),
        order: Rc::clone(&order),
    });
    let factory_order = Rc::clone(&order);
    let viewport_probe: Rc<RefCell<Option<Rc<ViewportController>>>> = Rc::new(RefCell::new(None));
    if let Some(hook) = on_hub_subscribe {
        let probe = Rc::clone(&viewport_probe);
        let (size_source, viewer, shell) = (Arc::clone(&size_source), viewer.clone(), Rc::clone(&shell));
        *hub.on_subscribe.borrow_mut() = Some(Box::new(move || {
            if let Some(viewport) = probe.borrow().as_ref() {
                hook(&HubProbe {
                    size_source: Arc::clone(&size_source),
                    viewer: viewer.clone(),
                    viewport: Rc::clone(viewport),
                    shell: Rc::clone(&shell),
                });
            }
        }));
    }
    let probe = Rc::clone(&viewport_probe);
    let mark_log = Rc::clone(&marks);
    let window_log: Arc<dyn Log> = log.clone();
    let deps = MainWindowDeps {
        window: Rc::clone(&shell),
        dispatcher: dispatcher(),
        frame_clock,
        view_model: Rc::new(RefCell::new(vm)),
        size_source: Arc::clone(&size_source),
        hub: hub.clone(),
        cloak: cloak.clone(),
        placement: placement.clone(),
        log: window_log,
        mark: Some(Box::new(move |name| mark_log.borrow_mut().push(name.to_owned()))),
        viewport_factory: Box::new(move |callbacks: ViewportCallbacks| {
            factory_order.borrow_mut().push("viewport".to_owned());
            let viewport = ViewportController::new(
                callbacks.window,
                Rc::new(callbacks.poster),
                callbacks.frame_clock,
                callbacks.content,
                callbacks.update_fit_size,
                ViewportControllerOptions::default(),
            );
            *probe.borrow_mut() = Some(Rc::clone(&viewport));
            viewport
        }),
        options,
    };
    let window = MainWindow::new(deps);
    let built = Rc::new(Built {
        window,
        shell,
        hub,
        cloak,
        placement,
        marks,
        order,
        size_source,
        log,
        preload,
        gate,
        scheduler,
        runner,
        photos,
        root,
    });
    BUILT.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&built)));
    built
}

/// Takes the fixture out of its thread-local (to drop the window on purpose). Fails when something else still holds it.
pub fn take_built() -> Built {
    let built = BUILT.with(|slot| slot.borrow_mut().take()).expect("build() first");
    match Rc::try_unwrap(built) {
        Ok(built) => built,
        Err(_) => panic!("the fixture is still shared"),
    }
}

impl Built {
    /// Window text of the hidden window.
    pub fn title(&self) -> String {
        crate::shell_window_fixture::window_text(self.shell.hwnd())
    }

    /// The root folder of the view model's files (for tests that need a real path).
    pub fn root_path(&self) -> &Path {
        self.root.path()
    }
}
