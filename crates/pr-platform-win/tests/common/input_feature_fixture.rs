//! Fixture of the W8-14c tests (`tests/shell_input_surface.rs`): a scripted `ViewportControllerApi` (for the surface), fake `MainWindowInputServices`
//! over a REAL `MainViewModel` (`pr-app/tests/support/main_env.rs`) and a REAL `ViewportController` on the fakes of the W8-14b fixture.
//!
//! The including test crate declares at its root: `file_action_fakes`, `support::{main_env, presenter_fakes, preview_fakes}`, `fixture`
//! (`common/shell_input_dispatch_fixture.rs`) and `viewing_fakes`.
#![allow(dead_code)] // shared by the modules of one test crate, each of which uses a different part

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::input::primitives::{PointD, SizeD};
use pr_app::input::surface::{RenderFrameHandler, RenderHookId};
use pr_app::services::contracts::ZoomPromptService;
use pr_app::viewmodels::main::{ChangeSet, MainViewModel, SettingsSource, ViewerHandle};
use pr_app::viewport::types::ViewportLayout;
use pr_core::abstractions::{DisplayTiming, Log, UiDispatcher};
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::host::frame_clock::base::HostFrameClock;
use pr_platform_win::shell_input::environment::ShellInputEnvironment;
use pr_platform_win::shell_input::main_window::{InputActionNames, MainWindowInput, MainWindowInputServices};
use pr_platform_win::shell_input::surface::ViewportControllerApi;
use pr_platform_win::shell_viewing::viewport_controller::{ViewportController, ViewportControllerOptions};
use pr_test_support::RecordingLog;

use crate::fixture::{InputWindow, QueueDispatcher, RigContent, RigPoster};
use crate::support::main_env::MainEnv;
use crate::viewing_fakes::FakeHostClock;

// ---------------------------------------------------------------------------------------------------------------------------------------------
// FakeController
// ---------------------------------------------------------------------------------------------------------------------------------------------

/// A scripted `ViewportControllerApi`: every call is logged as `name` or `name(args)`; the values are set by the test.
pub struct FakeController {
    pub calls: RefCell<Vec<String>>,
    pub layout: Cell<ViewportLayout>,
    pub viewport_size: Cell<(f64, f64)>,
    pub loaded: Cell<bool>,
    pub horizontal_offset: Cell<f64>,
    pub vertical_offset: Cell<f64>,
    pub drag_threshold: Cell<(f64, f64)>,
    pub image_origin: Cell<PointD>,
    pub image_actual: Cell<(f64, f64)>,
    pub source_size: Cell<Option<SizeD>>,
    pub timestamp: Cell<i64>,
    pub display_timing: Cell<Option<DisplayTiming>>,
    pub pointer_position: Cell<Option<PointD>>,
    /// Runs inside `image_actual_width()` (the borrow probe of `capture`).
    pub on_image_actual_width: RefCell<Option<Box<dyn Fn()>>>,
    hooks: RefCell<Vec<(RenderHookId, RenderFrameHandler)>>,
    next_hook: Cell<u64>,
    /// The continuations of `yield_to_render`.
    pub yielded: RefCell<Vec<Box<dyn FnOnce()>>>,
}

impl FakeController {
    pub fn new() -> Rc<FakeController> {
        Rc::new(FakeController {
            calls: RefCell::new(Vec::new()),
            layout: Cell::new(ViewportLayout::default()),
            viewport_size: Cell::new((0.0, 0.0)),
            loaded: Cell::new(true),
            horizontal_offset: Cell::new(0.0),
            vertical_offset: Cell::new(0.0),
            drag_threshold: Cell::new((4.0, 4.0)),
            image_origin: Cell::new(PointD { x: 0.0, y: 0.0 }),
            image_actual: Cell::new((0.0, 0.0)),
            source_size: Cell::new(None),
            timestamp: Cell::new(0),
            display_timing: Cell::new(None),
            pointer_position: Cell::new(None),
            on_image_actual_width: RefCell::new(None),
            hooks: RefCell::new(Vec::new()),
            next_hook: Cell::new(1),
            yielded: RefCell::new(Vec::new()),
        })
    }

    fn log(&self, entry: impl Into<String>) {
        self.calls.borrow_mut().push(entry.into());
    }

    pub fn calls_snapshot(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// Delivers one frame to every hooked handler, oldest first.
    pub fn fire_frame(&self, ticks: Option<i64>) {
        let mut hooks = std::mem::take(&mut *self.hooks.borrow_mut());
        for (_, handler) in &mut hooks {
            handler(ticks);
        }
        let mut current = self.hooks.borrow_mut();
        hooks.append(&mut current);
        *current = hooks;
    }

    pub fn hook_count(&self) -> usize {
        self.hooks.borrow().len()
    }
}

impl ViewportControllerApi for FakeController {
    fn layout(&self) -> ViewportLayout {
        self.log("layout");
        self.layout.get()
    }
    fn viewport_size(&self) -> (f64, f64) {
        self.log("viewport_size");
        self.viewport_size.get()
    }
    fn is_loaded(&self) -> bool {
        self.log("is_loaded");
        self.loaded.get()
    }
    fn update_layout(&self) {
        self.log("update_layout");
    }
    fn update_fit_size(&self) {
        self.log("update_fit_size");
    }
    fn scroll_home(&self) {
        self.log("scroll_home");
    }
    fn invalidate_layout(&self) {
        self.log("invalidate_layout");
    }
    fn horizontal_offset(&self) -> f64 {
        self.log("horizontal_offset");
        self.horizontal_offset.get()
    }
    fn vertical_offset(&self) -> f64 {
        self.log("vertical_offset");
        self.vertical_offset.get()
    }
    fn drag_threshold(&self) -> (f64, f64) {
        self.log("drag_threshold");
        self.drag_threshold.get()
    }
    fn scroll_to(&self, horizontal: f64, vertical: f64) {
        self.log(format!("scroll_to({horizontal},{vertical})"));
    }
    fn image_origin(&self) -> PointD {
        self.log("image_origin");
        self.image_origin.get()
    }
    fn to_image_element(&self, surface_point: PointD) -> PointD {
        self.log(format!("to_image_element({},{})", surface_point.x, surface_point.y));
        PointD {
            x: surface_point.x + 1.0,
            y: surface_point.y + 2.0,
        }
    }
    fn image_actual_width(&self) -> f64 {
        self.log("image_actual_width");
        if let Some(probe) = self.on_image_actual_width.borrow().as_ref() {
            probe();
        }
        self.image_actual.get().0
    }
    fn image_actual_height(&self) -> f64 {
        self.log("image_actual_height");
        self.image_actual.get().1
    }
    fn source_size(&self) -> Option<SizeD> {
        self.log("source_size");
        self.source_size.get()
    }
    fn capture_mouse(&self) {
        self.log("capture_mouse");
    }
    fn release_mouse_capture(&self) {
        self.log("release_mouse_capture");
    }
    fn set_pan_cursor(&self, panning: bool) {
        self.log(format!("set_pan_cursor({panning})"));
    }
    fn hook_render_frame(&self, handler: RenderFrameHandler) -> RenderHookId {
        self.log("hook_render_frame");
        let id = RenderHookId(self.next_hook.get());
        self.next_hook.set(id.0 + 1);
        self.hooks.borrow_mut().push((id, handler));
        id
    }
    fn unhook_render_frame(&self, id: RenderHookId) {
        self.log(format!("unhook_render_frame({})", id.0));
        self.hooks.borrow_mut().retain(|(known, _)| *known != id);
    }
    fn timestamp(&self) -> i64 {
        self.log("timestamp");
        self.timestamp.get()
    }
    fn display_timing(&self) -> Option<DisplayTiming> {
        self.log("display_timing");
        self.display_timing.get()
    }
    fn pointer_position(&self) -> Option<PointD> {
        self.log("pointer_position");
        self.pointer_position.get()
    }
    fn yield_to_render(&self, continuation: Box<dyn FnOnce()>) {
        self.log("yield_to_render");
        self.yielded.borrow_mut().push(continuation);
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// FakePrompt
// ---------------------------------------------------------------------------------------------------------------------------------------------

/// The zoom prompt: answers `answer` and records the percent it was asked with.
pub struct FakePrompt {
    pub answer: Cell<Option<i32>>,
    pub asked: RefCell<Vec<i32>>,
}

impl FakePrompt {
    pub fn new(answer: Option<i32>) -> Rc<FakePrompt> {
        Rc::new(FakePrompt {
            answer: Cell::new(answer),
            asked: RefCell::new(Vec::new()),
        })
    }
}

impl ZoomPromptService for FakePrompt {
    fn prompt_custom_zoom(&self, current_percent: i32) -> Option<i32> {
        self.asked.borrow_mut().push(current_percent);
        self.answer.get()
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// FakeServices
// ---------------------------------------------------------------------------------------------------------------------------------------------

/// `MainWindowInputServices` over a real view model. `with_view_model` counts its calls and keeps the `ChangeSet` of each; nothing applies them (a test
/// settles the environment itself).
pub struct FakeServices {
    pub env: Rc<RefCell<MainEnv>>,
    pub window: Rc<InputWindow>,
    pub viewport: Rc<dyn ViewportControllerApi>,
    pub viewer: ViewerHandle,
    pub prompt: RefCell<Option<Rc<dyn ZoomPromptService>>>,
    pub log: Arc<RecordingLog>,
    pub dispatcher: Arc<QueueDispatcher>,
    pub environment: Rc<ShellInputEnvironment>,
    pub vm_calls: Cell<u32>,
    pub applied: RefCell<Vec<ChangeSet>>,
    pub statuses: RefCell<Vec<String>>,
    pub closes: Cell<u32>,
    /// For each `close_window`: could the environment (the view model) be borrowed mutably at that moment?
    pub close_outside_borrow: RefCell<Vec<bool>>,
    pub invalidations: Cell<u32>,
    pub events: RefCell<Vec<String>>,
}

impl MainWindowInputServices for FakeServices {
    fn window(&self) -> Rc<dyn ShellWindowApi> {
        Rc::clone(&self.window) as Rc<dyn ShellWindowApi>
    }
    fn viewport(&self) -> Rc<dyn ViewportControllerApi> {
        Rc::clone(&self.viewport)
    }
    fn viewer(&self) -> ViewerHandle {
        self.viewer.clone()
    }
    fn settings(&self) -> SettingsSource {
        self.env.borrow().settings.clone()
    }
    fn zoom_prompt(&self) -> Option<Rc<dyn ZoomPromptService>> {
        self.prompt.borrow().clone()
    }
    fn log(&self) -> Arc<dyn Log> {
        Arc::clone(&self.log) as Arc<dyn Log>
    }
    fn dispatcher(&self) -> Arc<dyn UiDispatcher> {
        Arc::clone(&self.dispatcher) as Arc<dyn UiDispatcher>
    }
    fn input_environment(&self) -> Rc<ShellInputEnvironment> {
        Rc::clone(&self.environment)
    }
    fn ticks_per_ms(&self) -> f64 {
        pr_core::diagnostics::perf::STOPWATCH_FREQUENCY as f64 / 1000.0
    }
    fn with_view_model(&self, f: &mut dyn FnMut(&mut MainViewModel) -> ChangeSet) {
        self.vm_calls.set(self.vm_calls.get() + 1);
        let changes = {
            let mut env = self.env.borrow_mut();
            f(&mut env.vm)
        };
        self.applied.borrow_mut().push(changes);
    }
    fn show_status(&self, text: &str) {
        self.statuses.borrow_mut().push(text.to_owned());
    }
    fn folder_open_failed_text(&self, error: &dyn Error) -> String {
        format!("open-failed:{error}")
    }
    fn action_failed_text(&self, action_name: &str, error: &dyn Error) -> String {
        format!("action-failed:{action_name}:{error}")
    }
    fn action_names(&self) -> InputActionNames {
        InputActionNames {
            next_folder: "Next folder".to_owned(),
            previous_folder: "Previous folder".to_owned(),
            move_to_folder: "Move to folder".to_owned(),
            copy_to_folder: "Copy to folder".to_owned(),
            open_folder: "Open folder".to_owned(),
            zoom: "Zoom".to_owned(),
        }
    }
    fn close_window(&self) {
        self.closes.set(self.closes.get() + 1);
        self.close_outside_borrow
            .borrow_mut()
            .push(self.env.try_borrow_mut().is_ok());
    }
    fn invalidate_layout_and_window(&self) {
        self.invalidations.set(self.invalidations.get() + 1);
        self.events.borrow_mut().push("invalidate".to_owned());
        self.viewport.invalidate_layout();
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// FeatureRig
// ---------------------------------------------------------------------------------------------------------------------------------------------

/// A `MainWindowInput` over [`FakeServices`], a real view model and a REAL `ViewportController` (fake window, clock and dispatcher).
pub struct FeatureRig {
    pub services: Rc<FakeServices>,
    pub input: Rc<MainWindowInput>,
    pub env: Rc<RefCell<MainEnv>>,
    pub window: Rc<InputWindow>,
    pub dispatcher: Arc<QueueDispatcher>,
    pub clock: Rc<FakeHostClock>,
    pub viewport: Rc<ViewportController>,
    pub content: Rc<RigContent>,
    pub viewer: ViewerHandle,
}

impl FeatureRig {
    pub fn new() -> FeatureRig {
        FeatureRig::with_env(MainEnv::new())
    }

    pub fn with_env(env: MainEnv) -> FeatureRig {
        let window = InputWindow::new(1000.0, 600.0, 1.0);
        let dispatcher = QueueDispatcher::new();
        let clock = FakeHostClock::new();
        let viewer = env.viewer.clone();
        // The image is 3000 x 2000; the C# rig sets the same source size.
        let _ = viewer.borrow_mut().set_source_size(3000, 2000, true);
        let content = RigContent::new(&viewer, (3000.0, 2000.0));
        let content_for_viewport = Rc::clone(&content);
        let window_for_fit = Rc::clone(&window);
        let viewer_for_fit = viewer.clone();
        let viewport = ViewportController::new(
            Rc::clone(&window) as Rc<dyn ShellWindowApi>,
            Rc::new(RigPoster(Arc::clone(&dispatcher))),
            Rc::clone(&clock) as Rc<dyn HostFrameClock>,
            Rc::new(move || content_for_viewport.current()),
            Rc::new(move || {
                let size = window_for_fit.client_size.get();
                let sane = |value: f64| if value.is_finite() && value > 0.0 { value } else { 0.0 };
                let _ = viewer_for_fit
                    .borrow_mut()
                    .update_viewport(sane(size.width), sane(size.height), false);
            }),
            ViewportControllerOptions {
                drag_metrics_pixels: Box::new(|| (4, 4)),
                ..ViewportControllerOptions::default()
            },
        );
        let env = Rc::new(RefCell::new(env));
        let services = Rc::new(FakeServices {
            env: Rc::clone(&env),
            window: Rc::clone(&window),
            viewport: Rc::clone(&viewport) as Rc<dyn ViewportControllerApi>,
            viewer: viewer.clone(),
            prompt: RefCell::new(None),
            log: Arc::new(RecordingLog::new()),
            dispatcher: Arc::clone(&dispatcher),
            environment: Rc::new(ShellInputEnvironment::default()),
            vm_calls: Cell::new(0),
            applied: RefCell::new(Vec::new()),
            statuses: RefCell::new(Vec::new()),
            closes: Cell::new(0),
            close_outside_borrow: RefCell::new(Vec::new()),
            invalidations: Cell::new(0),
            events: RefCell::new(Vec::new()),
        });
        let input = MainWindowInput::register(Rc::clone(&services) as Rc<dyn MainWindowInputServices>);
        let rig = FeatureRig {
            services,
            input,
            env,
            window,
            dispatcher,
            clock,
            viewport,
            content,
            viewer,
        };
        rig.viewport.invalidate_layout();
        rig.drain();
        rig
    }

    /// The layout pass and the continuations, until the queue is empty (the content is re-read before every job).
    pub fn drain(&self) {
        self.content.refresh(&self.viewer);
        self.dispatcher.drain(&|| self.content.refresh(&self.viewer));
    }

    /// Runs the view model's own queued work (`MainEnv::settle`).
    pub fn settle(&self) {
        self.env.borrow_mut().settle();
    }

    pub fn vm_calls(&self) -> u32 {
        self.services.vm_calls.get()
    }

    pub fn statuses(&self) -> Vec<String> {
        self.services.statuses.borrow().clone()
    }

    pub fn warnings(&self) -> Vec<String> {
        self.services
            .log
            .entries()
            .into_iter()
            .filter(|(level, _)| *level == pr_test_support::LogLevel::Warn)
            .map(|(_, message)| message)
            .collect()
    }
}
