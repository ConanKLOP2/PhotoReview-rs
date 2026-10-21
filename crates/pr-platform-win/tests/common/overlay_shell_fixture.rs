//! Test rig of W8-15j (`MainWindowOverlay`, the Rust twin of `ShellMainWindow.Overlay.cs`): fakes of the window port, the view model, the poster, the
//! wake, the frame clock, the OS seam, the tooltip control and the Tools menu host, and a `Rig` that attaches an overlay to them.
//!
//! The including test crate must declare `overlay_fakes` (`FakeTextRenderer`, `RecordingDrawContext`) next to this module. Nothing here sleeps or
//! touches the OS. Every `Rig` holds a process-wide lock, because `Localizer::set_current` and its handlers are global: a test that counts wakes must
//! not see the language events of another test's overlay (the Rust twin of the C# "GlobalState" collection).

#![allow(dead_code)] // shared by the pure and the native test files; each uses a subset

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::error::Error;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_app::input::frame_clock::{FrameHandler, FrameHandlerId};
use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::menus::model::MenuItemModel;
use pr_app::overlay::model::OverlayModel;
use pr_app::services::render::DrawContext;
use pr_core::abstractions::{NullLog, SystemClock, UiPriority};
use pr_core::app_paths::AppPaths;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::store::SettingsStore;
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_platform_win::host::contracts::{
    ClosingHandler, EventHandlerId, ShellCursor, ShellWindowApi, ShellWindowEvent, WindowEventHandler, WindowMessage,
    WindowMessageHandler,
};
use pr_platform_win::host::error::HostError;
use pr_platform_win::host::frame_clock::HostFrameClock;
use pr_platform_win::shell_overlay::main_window::{
    MainWindowOverlay, OverlayDeps, OverlayViewModel, OverlayWindowPort, UiWake,
};
use pr_platform_win::shell_overlay::native::OverlayOs;
use pr_platform_win::shell_overlay::overlay::state::{OverlayButton, OverlayMenuHost};
use pr_platform_win::shell_overlay::tooltips::{
    LocalJob, LocalPoster, OverlayTooltip, OverlayTooltips, TooltipFactory,
};
use pr_platform_win::win32::structs::{Hwnd, Lparam, Lresult, Wparam};
use pr_test_support::TempRoot;

use crate::overlay_fakes::{FakeTextRenderer, RecordingDrawContext};

/// Ordered log shared by the fakes (the Rust twin of the C# `List<string> order`).
pub type Timeline = Rc<RefCell<Vec<String>>>;

pub fn timeline() -> Timeline {
    Rc::new(RefCell::new(Vec::new()))
}

/// The entries of `timeline` that equal one of `names`, in order.
pub fn only(timeline: &Timeline, names: &[&str]) -> Vec<String> {
    timeline
        .borrow()
        .iter()
        .filter(|entry| names.contains(&entry.as_str()))
        .cloned()
        .collect()
}

// ---- frame clock ----

/// A `HostFrameClock` that only counts.
#[derive(Default)]
pub struct ShellClock {
    next_id: Cell<u64>,
    pub subscribers: RefCell<Vec<FrameHandlerId>>,
    pub frame_requests: Cell<usize>,
}

impl HostFrameClock for ShellClock {
    fn subscribe(&self, _handler: FrameHandler) -> FrameHandlerId {
        self.next_id.set(self.next_id.get() + 1);
        self.subscribers.borrow_mut().push(self.next_id.get());
        self.next_id.get()
    }
    fn unsubscribe(&self, id: FrameHandlerId) {
        self.subscribers.borrow_mut().retain(|other| *other != id);
    }
    fn request_frame(&self) {
        self.frame_requests.set(self.frame_requests.get() + 1);
    }
    fn is_armed(&self) -> bool {
        false
    }
    fn tick_count(&self) -> i64 {
        0
    }
    fn wait_handle(&self) -> isize {
        0
    }
    fn deadline_timestamp(&self) -> i64 {
        i64::MAX
    }
    fn ticks_until_deadline(&self) -> i64 {
        i64::MAX
    }
    fn on_signaled(&self) -> Result<bool, HostError> {
        Ok(false)
    }
    fn on_deadline(&self) -> Result<bool, HostError> {
        Ok(false)
    }
    fn dispose(&self) {}
}

// ---- window ----

type Subscription = (EventHandlerId, ShellWindowEvent, WindowEventHandler);

/// C# fake `IShellWindow` + `InsertMessageHandlerFirst`: a message-handler chain that can be dispatched, event subscriptions that can be raised, and the
/// counters the tests read.
pub struct PortWindow {
    pub timeline: Timeline,
    pub dpi: Cell<f64>,
    pub size: Cell<SizeD>,
    pub active: Cell<bool>,
    pub handle: Hwnd,
    next_id: Cell<u64>,
    subscriptions: RefCell<Vec<Subscription>>,
    pub chain: RefCell<Vec<Rc<dyn WindowMessageHandler>>>,
    pub invalidates: Cell<usize>,
    pub unsubscribe_calls: Cell<usize>,
    /// Runs inside every `invalidate` (re-entrancy tests).
    pub on_invalidate: RefCell<Option<Rc<dyn Fn()>>>,
    captured: Cell<bool>,
}

impl PortWindow {
    pub fn new(timeline: Timeline) -> Rc<PortWindow> {
        Rc::new(PortWindow {
            timeline,
            dpi: Cell::new(1.0),
            size: Cell::new(SizeD {
                width: 1000.0,
                height: 700.0,
            }),
            active: Cell::new(true),
            handle: 7,
            next_id: Cell::new(0),
            subscriptions: RefCell::new(Vec::new()),
            chain: RefCell::new(Vec::new()),
            invalidates: Cell::new(0),
            unsubscribe_calls: Cell::new(0),
            on_invalidate: RefCell::new(None),
            captured: Cell::new(false),
        })
    }

    /// Raises `event` for every current subscriber.
    pub fn fire(&self, event: ShellWindowEvent) {
        for (_, kind, handler) in self.subscriptions.borrow_mut().iter_mut() {
            if std::mem::discriminant(kind) == std::mem::discriminant(&event) {
                handler();
            }
        }
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscriptions.borrow().len()
    }

    /// The window procedure's chain: the first handler that answers wins.
    pub fn dispatch(&self, msg: u32, wparam: Wparam, lparam: Lparam) -> Option<Lresult> {
        let message = WindowMessage {
            hwnd: self.handle,
            msg,
            wparam,
            lparam,
        };
        let chain = self.chain.borrow().clone();
        chain.iter().find_map(|handler| handler.try_handle(&message))
    }
}

impl ShellWindowApi for PortWindow {
    fn hwnd(&self) -> Hwnd {
        self.handle
    }
    fn dpi_scale(&self) -> f64 {
        self.dpi.get()
    }
    fn client_size_dip(&self) -> SizeD {
        self.size.get()
    }
    fn is_active(&self) -> bool {
        self.active.get()
    }
    fn is_loaded(&self) -> bool {
        true
    }
    fn subscribe(&self, event: ShellWindowEvent, handler: WindowEventHandler) -> EventHandlerId {
        self.next_id.set(self.next_id.get() + 1);
        self.subscriptions
            .borrow_mut()
            .push((self.next_id.get(), event, handler));
        self.next_id.get()
    }
    fn subscribe_closing(&self, _handler: ClosingHandler) -> EventHandlerId {
        0
    }
    fn unsubscribe(&self, id: EventHandlerId) {
        self.unsubscribe_calls.set(self.unsubscribe_calls.get() + 1);
        self.subscriptions.borrow_mut().retain(|(other, _, _)| *other != id);
    }
    fn add_message_handler(&self, handler: Rc<dyn WindowMessageHandler>) {
        self.timeline.borrow_mut().push("window.add_handler".to_owned());
        self.chain.borrow_mut().push(handler);
    }
    fn insert_message_handler_first(&self, handler: Rc<dyn WindowMessageHandler>) {
        self.timeline.borrow_mut().push("window.insert_first".to_owned());
        self.chain.borrow_mut().insert(0, handler);
    }
    fn is_destroyed(&self) -> bool {
        false
    }
    fn invalidate(&self) {
        self.invalidates.set(self.invalidates.get() + 1);
        self.timeline.borrow_mut().push("invalidate".to_owned());
        let hook = self.on_invalidate.borrow().clone();
        if let Some(hook) = hook {
            hook();
        }
    }
    fn set_title(&self, _title: &str) -> Result<(), HostError> {
        Ok(())
    }
    fn set_cursor(&self, _cursor: ShellCursor) {}
    fn capture_pointer(&self) {
        self.captured.set(true);
    }
    fn release_pointer(&self) {
        self.captured.set(false);
    }
    fn has_pointer_capture(&self) -> bool {
        self.captured.get()
    }
    fn screen_to_client_dip(&self, screen_pixel: PointD) -> PointD {
        PointD {
            x: screen_pixel.x / self.dpi.get(),
            y: screen_pixel.y / self.dpi.get(),
        }
    }
    fn close(&self) {}
}

impl OverlayWindowPort for PortWindow {
    fn insert_message_handler_first(&self, handler: Rc<dyn WindowMessageHandler>) {
        ShellWindowApi::insert_message_handler_first(self, handler);
    }
}

// ---- view model ----

/// The fake view model: every call is logged on the timeline, the model and the settings are plain cells.
pub struct ShellVm {
    pub timeline: Timeline,
    pub model: RefCell<OverlayModel>,
    pub settings: RefCell<Arc<AppSettings>>,
    pub model_reads: Cell<usize>,
    pub set_settings_calls: RefCell<Vec<Arc<AppSettings>>>,
    pub commands: RefCell<Vec<String>>,
    pub failures: RefCell<Vec<(String, String)>>,
    /// Runs inside every `overlay_model` (re-entrancy tests).
    pub on_model: RefCell<Option<Rc<dyn Fn()>>>,
}

impl ShellVm {
    pub fn new(timeline: Timeline) -> Rc<ShellVm> {
        let settings = AppSettings {
            toolbar_auto_hide: true,
            toolbar_auto_hide_delay_ms: 1500,
            info_overlay_auto_hide: true,
            info_overlay_auto_hide_delay_ms: 3000,
            toolbar_opacity_percent: 100,
            ..AppSettings::default()
        };
        Rc::new(ShellVm {
            timeline,
            model: RefCell::new(OverlayModel {
                has_images: true,
                status_text: "1 / 3".to_owned(),
                ..OverlayModel::default()
            }),
            settings: RefCell::new(Arc::new(settings)),
            model_reads: Cell::new(0),
            set_settings_calls: RefCell::new(Vec::new()),
            commands: RefCell::new(Vec::new()),
            failures: RefCell::new(Vec::new()),
            on_model: RefCell::new(None),
        })
    }

    fn note(&self, entry: &str) {
        self.timeline.borrow_mut().push(entry.to_owned());
    }

    fn command(&self, name: &str) {
        self.commands.borrow_mut().push(name.to_owned());
    }
}

impl OverlayViewModel for ShellVm {
    fn overlay_model(&self) -> OverlayModel {
        self.model_reads.set(self.model_reads.get() + 1);
        self.note("vm.overlay_model");
        let hook = self.on_model.borrow().clone();
        if let Some(hook) = hook {
            hook();
        }
        self.model.borrow().clone()
    }
    fn settings(&self) -> Arc<AppSettings> {
        self.note("vm.settings");
        Arc::clone(&self.settings.borrow())
    }
    fn set_settings(&self, settings: Arc<AppSettings>) {
        self.note("vm.set_settings");
        self.set_settings_calls.borrow_mut().push(Arc::clone(&settings));
        *self.settings.borrow_mut() = settings;
    }
    fn refresh_localized_text(&self) {
        self.note("vm.refresh_text");
    }
    fn report_action_failed(&self, action_name: &str, message: &str) {
        self.note("vm.report_failed");
        self.failures
            .borrow_mut()
            .push((action_name.to_owned(), message.to_owned()));
    }
    fn pick_and_open_folder(&self) {
        self.command("pick_and_open_folder");
    }
    fn show_settings(&self) {
        self.command("show_settings");
    }
    fn show_skipped_files(&self) {
        self.command("show_skipped_files");
    }
    fn show_recovery(&self) {
        self.command("show_recovery");
    }
    fn show_diagnostics(&self) {
        self.command("show_diagnostics");
    }
    fn show_benchmark(&self) {
        self.command("show_benchmark");
    }
    fn clear_cache(&self) {
        self.command("clear_cache");
    }
    fn remove_duplicates(&self, numbered: bool) {
        self.command(if numbered {
            "remove_duplicates(true)"
        } else {
            "remove_duplicates(false)"
        });
    }
}

// ---- wake, poster, os, tooltips, menus ----

#[derive(Default)]
pub struct CountingWake {
    pub count: AtomicUsize,
}

impl UiWake for CountingWake {
    fn wake(&self) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }
}

impl CountingWake {
    pub fn count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

/// A `LocalPoster` that queues: nothing runs until the test says so. Jobs posted while jobs run are run by the same `run_all`.
#[derive(Default)]
pub struct QueuePoster {
    jobs: RefCell<VecDeque<(UiPriority, LocalJob)>>,
    pub posted: RefCell<Vec<UiPriority>>,
}

impl LocalPoster for QueuePoster {
    fn post(&self, priority: UiPriority, job: LocalJob) {
        self.posted.borrow_mut().push(priority);
        self.jobs.borrow_mut().push_back((priority, job));
    }
}

impl QueuePoster {
    /// Priorities of the jobs waiting now.
    pub fn waiting(&self) -> Vec<UiPriority> {
        self.jobs.borrow().iter().map(|(priority, _)| *priority).collect()
    }

    /// Runs the oldest job; false when none waits.
    pub fn run_next(&self) -> bool {
        let next = self.jobs.borrow_mut().pop_front();
        match next {
            Some((_, job)) => {
                job();
                true
            }
            None => false,
        }
    }

    /// Runs jobs until none waits (including those the jobs post).
    pub fn run_all(&self) {
        while self.run_next() {}
    }
}

pub struct ShellOs {
    pub origin: Cell<(i32, i32)>,
}

impl OverlayOs for ShellOs {
    fn request_mouse_leave(&self, _hwnd: Hwnd) {}
    fn is_key_down(&self, _virtual_key: i32) -> bool {
        false
    }
    fn client_origin_on_screen(&self, _hwnd: Hwnd) -> (i32, i32) {
        self.origin.get()
    }
}

#[derive(Default)]
pub struct ShellTips {
    pub sync_count: Cell<usize>,
    pub dpi: Cell<f64>,
    pub tool_count: Cell<usize>,
    pub disposed: Cell<bool>,
}

impl OverlayTooltips for ShellTips {
    fn sync(&self, tools: &[OverlayTooltip], dpi_scale: f64) {
        self.sync_count.set(self.sync_count.get() + 1);
        self.dpi.set(dpi_scale);
        self.tool_count.set(tools.len());
    }
    fn relay_mouse(&self, _message: u32, _wparam: Wparam, _lparam: Lparam) {}
    fn dispose(&self) {
        self.disposed.set(true);
    }
}

/// The Tools menu host: records the anchor and either dismisses or fails.
#[derive(Default)]
pub struct ShellMenus {
    pub anchors: RefCell<Vec<PointD>>,
    pub fail_with: RefCell<Option<&'static str>>,
}

impl OverlayMenuHost for ShellMenus {
    fn show(&self, _items: &[MenuItemModel], screen_point_dip: PointD) -> Result<Option<String>, Box<dyn Error>> {
        self.anchors.borrow_mut().push(screen_point_dip);
        match *self.fail_with.borrow() {
            Some(text) => Err(text.into()),
            None => Ok(None),
        }
    }
}

// ---- rig ----

static GLOBAL_STATE: Mutex<()> = Mutex::new(());

/// What a `Rig` is built with.
#[derive(Default)]
pub struct RigOptions {
    /// A real `SettingsStore` over a temp folder (`Rig::store`).
    pub store: bool,
    /// A handler already in the window's chain BEFORE `attach` (so "inserted first" is observable).
    pub existing_handler: Option<Rc<dyn WindowMessageHandler>>,
}

pub struct Rig {
    overlay: RefCell<Option<Rc<MainWindowOverlay>>>,
    pub timeline: Timeline,
    pub window: Rc<PortWindow>,
    pub vm: Rc<ShellVm>,
    pub wake: Arc<CountingWake>,
    pub poster: Rc<QueuePoster>,
    pub os: Rc<ShellOs>,
    pub clock: Rc<ShellClock>,
    pub tips: Rc<ShellTips>,
    pub menus: Rc<ShellMenus>,
    pub fit_calls: Rc<Cell<usize>>,
    pub store: Option<Arc<SettingsStore>>,
    restore_localizer: Arc<Localizer>,
    _root: TempRoot,
    _global: MutexGuard<'static, ()>,
}

impl Rig {
    pub fn new() -> Rig {
        Rig::with(RigOptions::default())
    }

    pub fn with(options: RigOptions) -> Rig {
        let global = GLOBAL_STATE.lock().unwrap_or_else(PoisonError::into_inner);
        let restore_localizer = Localizer::current();
        let root = TempRoot::new("overlay-shell");
        let store = options.store.then(|| {
            Arc::new(SettingsStore::new(
                AppPaths::new(&root.join("appdata"), None, false),
                Arc::new(PhysicalFileSystem),
                Arc::new(NullLog),
                Arc::new(SystemClock),
                Arc::new(EnglishSettingsTexts),
            ))
        });
        let timeline = timeline();
        let window = PortWindow::new(Rc::clone(&timeline));
        if let Some(existing) = options.existing_handler {
            window.add_message_handler(existing);
        }
        let vm = ShellVm::new(Rc::clone(&timeline));
        let wake = Arc::new(CountingWake::default());
        let poster = Rc::new(QueuePoster::default());
        let os = Rc::new(ShellOs {
            origin: Cell::new((0, 0)),
        });
        let clock = Rc::new(ShellClock::default());
        let tips = Rc::new(ShellTips::default());
        let menus = Rc::new(ShellMenus::default());
        let fit_calls = Rc::new(Cell::new(0usize));
        let factory: TooltipFactory = {
            let tips = Rc::clone(&tips);
            Box::new(move || Ok(tips as Rc<dyn OverlayTooltips>))
        };
        let overlay = MainWindowOverlay::attach(OverlayDeps {
            window: Rc::clone(&window) as Rc<dyn OverlayWindowPort>,
            clock: Rc::clone(&clock) as Rc<dyn HostFrameClock>,
            view_model: Rc::clone(&vm) as Rc<dyn OverlayViewModel>,
            text: Rc::new(FakeTextRenderer::new()),
            text_renderer_id: 1,
            texts: built_in_english_localizer(),
            menus: Some(Rc::clone(&menus) as Rc<dyn OverlayMenuHost>),
            settings_store: store.clone(),
            poster: Rc::clone(&poster) as Rc<dyn LocalPoster>,
            wake: Arc::clone(&wake) as Arc<dyn UiWake>,
            fit: {
                let fit_calls = Rc::clone(&fit_calls);
                Box::new(move || fit_calls.set(fit_calls.get() + 1))
            },
            tooltip_factory: Some(factory),
            os: Rc::clone(&os) as Rc<dyn OverlayOs>,
            log: Arc::new(NullLog),
        });
        Rig {
            overlay: RefCell::new(Some(overlay)),
            timeline,
            window,
            vm,
            wake,
            poster,
            os,
            clock,
            tips,
            menus,
            fit_calls,
            store,
            restore_localizer,
            _root: root,
            _global: global,
        }
    }

    /// The attached overlay (until [`Rig::drop_overlay`]).
    pub fn ov(&self) -> Rc<MainWindowOverlay> {
        self.overlay.borrow().clone().expect("overlay was dropped")
    }

    /// Drops the rig's only strong reference to the overlay WITHOUT calling `dispose` (T6).
    pub fn drop_overlay(&self) {
        let taken = self.overlay.borrow_mut().take().expect("overlay was dropped");
        assert_eq!(Rc::strong_count(&taken), 1, "nothing else may keep the overlay alive");
    }

    /// Runs the queued jobs (the Background tooltip creation and any refresh) and clears the timeline: the state "just attached and settled".
    pub fn settle(&self) {
        self.poster.run_all();
        self.timeline.borrow_mut().clear();
    }

    /// One frame into a recording context (arranges the overlay at the window's size and DPI).
    pub fn draw(&self) -> RecordingDrawContext {
        let mut dc = RecordingDrawContext::new();
        self.ov().draw_overlay(&mut dc as &mut dyn DrawContext);
        dc
    }

    pub fn bounds_of(&self, button: OverlayButton) -> RectD {
        self.ov()
            .overlay()
            .element_bounds(button.element_id())
            .expect("element exists")
    }

    /// Press and release at the centre of `button` (after a `draw`).
    pub fn click(&self, button: OverlayButton) {
        let rect = self.bounds_of(button);
        let point = PointD {
            x: rect.x + rect.width / 2.0,
            y: rect.y + rect.height / 2.0,
        };
        let overlay = self.ov();
        assert!(overlay.overlay().pointer_down(point), "press must be swallowed");
        assert!(overlay.overlay().pointer_up(point), "release must be swallowed");
    }

    /// Makes the next `invalidate` run `once` (and only once).
    pub fn on_next_invalidate(&self, once: impl Fn() + 'static) {
        let done = Cell::new(false);
        *self.window.on_invalidate.borrow_mut() = Some(Rc::new(move || {
            if !done.replace(true) {
                once();
            }
        }));
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        // The ambient localizer is process-global: put back what the test found (the global lock is still held).
        Localizer::set_current(Arc::clone(&self.restore_localizer));
    }
}

/// `lParam` of a mouse message: low word x, high word y (client pixels).
pub fn xy(x: i16, y: i16) -> isize {
    ((y as u16 as isize) << 16) | (x as u16 as isize)
}
