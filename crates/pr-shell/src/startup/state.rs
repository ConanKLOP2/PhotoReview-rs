//! Port of the FIELDS of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 108-135 (baseline `7eaee2a2`): the state that the C# `ShellStartup` object carries through the start-up and the
//! shutdown. Unit W8-17d owns this file and declares EVERY field of both units (17e must not need to add one; if it does it appends to this file, after 17d is merged). Contract:
//! `contracts/W8-17d.md`.
//!
//! One `Rc<StartupState>` lives on the UI thread for the whole run. The C# object had `lock`-free fields touched from the UI thread plus a few `Interlocked.Exchange` ones (`_main`,
//! `_coalescer`, `_services`, `_instanceScope`, `_perf`: the shutdown may run twice and must dispose each once); here every such field is a `RefCell<Option<..>>` that `take()`s. The
//! methods of the state are split over the sibling files by unit: `phase_one.rs` (17d: services, settings, instance lock), `window.rs`, `recovery.rs`, `smoke.rs`, `shutdown.rs`,
//! `run.rs` (17e). Workers never see the state (it is `!Send`); they get a `UiHopSender` or a clone of an `Arc`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use pr_app::startup::context::{ShellStartupContext, StartupTask};
use pr_app::startup::early_decode::{EarlyDecodePlan, MarkFn};
use pr_app::startup::forwarded_open::ForwardedOpenRouter;
use pr_app::startup::launch::ShellLaunch;
use pr_app::windowing::placement_store::JsonWindowPlacementStore;
use pr_core::abstractions::UiScheduler;
use pr_core::diagnostics::perf_csv::PerfCsvListener;
use pr_core::instance::coalescer::ForwardedOpenCoalescer;
use pr_core::settings::app_settings::AppSettings;
use pr_platform_win::host::frame_clock::TimerFrameClock;
use pr_platform_win::host::main_window::MainWindow;
use pr_platform_win::host::message_loop::MessageLoop;
use pr_platform_win::host::ui_dispatcher::Win32UiDispatcher;
use pr_platform_win::instance_scope::InstanceScope;
use pr_platform_win::shell_startup::cloak::StartupCloak;
use pr_platform_win::shell_viewing::presentation_sink::ShellPresentationHub;

use super::composition_root::ShellServices;
use super::graph::StartupGraph;
use super::options::ShellStartupOptions;

/// The slot that holds the early-decode preparation task: filled by the settings WORKER (it starts the preparation as soon as the settings file is read), taken by the UI thread.
pub type EarlyDecodeSlot = Arc<Mutex<Option<StartupTask<Option<EarlyDecodePlan>>>>>;

/// C# `ShellStartup` instance fields. See the module doc.
#[allow(dead_code)] // Phase-two fields are consumed by the separately ported W8-17e.
pub struct StartupState {
    // ---- given by `run` (17e) ----------------------------------------------------------------------------------------------------------------------------------------
    /// C# `_args`.
    pub(super) args: Vec<String>,
    /// C# `_launch`.
    pub(super) launch: ShellLaunch,
    /// C# `_options` (the `Option` seams are `take()`n by the step that uses them: graph builder, services, cloak seam, windowing, keep-warm, journal recovery, ready callback).
    pub(super) options: RefCell<ShellStartupOptions>,
    /// C# `_mark`.
    pub(super) mark: MarkFn,
    /// C# `_startTimestamp` (taken at the entry of `run`).
    pub(super) process_start: Instant,
    /// C# `_dispatcher`.
    pub(super) dispatcher: Win32UiDispatcher,
    /// `Arc::new(dispatcher.clone())` as the scheduler the workers post to.
    pub(super) ui: Arc<dyn UiScheduler>,
    /// C# `_loop`.
    pub(super) message_loop: Rc<MessageLoop>,
    /// The hub (`new ShellPresentationHub()` of `StartupCoreAsync` line 256).
    pub(super) hub: Rc<ShellPresentationHub>,
    /// C# `_forwardHandler` + `_hasPendingForward` + `_pendingForwardPath` (17a).
    pub(super) router: Rc<ForwardedOpenRouter>,
    /// C# `_placementStore = new()`: `JsonWindowPlacementStore::new(Arc::new(PhysicalFileSystem), Arc::new(Win32MonitorLayout))`.
    pub(super) placement_store: Arc<JsonWindowPlacementStore>,
    // ---- phase one (17d) ----------------------------------------------------------------------------------------------------------------------------------------------
    /// C# `_services` (the host half): `None` before `servicesBuilt` and after the shutdown.
    pub(super) services: RefCell<Option<Rc<ShellServices>>>,
    /// C# `_services` (the shared graph half).
    pub(super) graph: RefCell<Option<StartupGraph>>,
    /// The settings the load produced (C# `appSettings`).
    pub(super) settings: RefCell<Option<Arc<AppSettings>>>,
    /// C# `Context` (C-16), built when the settings arrived.
    pub(super) context: RefCell<Option<ShellStartupContext>>,
    /// C# `_instanceScope`: `None` in smoke mode and after the shutdown.
    pub(super) instance: RefCell<Option<Arc<InstanceScope>>>,
    /// C# `_coalescer`.
    pub(super) coalescer: RefCell<Option<Arc<ForwardedOpenCoalescer>>>,
    /// C# `earlyDecode` local: the preparation task (see [`EarlyDecodeSlot`]).
    pub(super) early_decode: EarlyDecodeSlot,
    /// Shared single-use placement handle: the worker takes it for prediction, otherwise the context takes it.
    pub(super) placement_load: Arc<Mutex<Option<pr_app::windowing::contracts::PrefetchHandle>>>,
    // ---- phase two, run and shutdown (17e) --------------------------------------------------------------------------------------------------------------------------
    /// C# `_main`. A SHARED slot of an `Rc<MainWindow>`: the owner closures of the dialogs, the forwarded-open target and the overlay view model hold a `Weak` of the SLOT, read "no window"
    /// as `0` / "closing", and CLONE the `Rc<MainWindow>` out of it (a short borrow) before they call the window, because a call may open a modal dialog and the window may be closed and the slot
    /// emptied while it runs (D-W8B6-28). The shutdown calls `MainWindow::dispose()` (idempotent) and then drops the slot's `Rc`.
    pub(super) main: Rc<RefCell<Option<Rc<MainWindow>>>>,
    /// C# `_cloak`.
    pub(super) cloak: RefCell<Option<Rc<StartupCloak>>>,
    /// C# `_frameClock`.
    pub(super) frame_clock: RefCell<Option<Rc<TimerFrameClock>>>,
    /// C# `_perf`.
    pub(super) perf: RefCell<Option<PerfCsvListener>>,
    /// C# `_exitCode`.
    pub(super) exit_code: Cell<i32>,
    /// C# `_forcedExit`: wins over `exit_code` when set.
    pub(super) forced_exit: Cell<Option<i32>>,
}

impl std::fmt::Debug for StartupState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartupState").finish_non_exhaustive()
    }
}

impl StartupState {
    /// The state of one run: the given parts as they are, every `RefCell<Option<_>>` empty, `exit_code` 0, `forced_exit` `None`, `ui = Arc::new(dispatcher.clone())`, a fresh
    /// `EarlyDecodeSlot`, a fresh `main` slot and a `JsonWindowPlacementStore` over `PhysicalFileSystem` and `Win32MonitorLayout`.
    #[allow(
        clippy::too_many_arguments,
        reason = "one argument per C# field that `run` produces; a builder would only rename them"
    )]
    pub fn new(
        args: Vec<String>,
        launch: ShellLaunch,
        options: ShellStartupOptions,
        mark: MarkFn,
        process_start: Instant,
        dispatcher: Win32UiDispatcher,
        message_loop: Rc<MessageLoop>,
        hub: Rc<ShellPresentationHub>,
        router: Rc<ForwardedOpenRouter>,
    ) -> Rc<StartupState> {
        let ui = Arc::new(dispatcher.clone());
        Rc::new(Self {
            args,
            launch,
            options: RefCell::new(options),
            mark,
            process_start,
            dispatcher,
            ui,
            message_loop,
            hub,
            router,
            placement_store: Arc::new(JsonWindowPlacementStore::new(
                Arc::new(pr_core::io::physical::PhysicalFileSystem),
                Arc::new(pr_platform_win::host::main_window::windowing::Win32MonitorLayout),
            )),
            services: RefCell::new(None),
            graph: RefCell::new(None),
            settings: RefCell::new(None),
            context: RefCell::new(None),
            instance: RefCell::new(None),
            coalescer: RefCell::new(None),
            early_decode: Arc::new(Mutex::new(None)),
            placement_load: Arc::new(Mutex::new(None)),
            main: Rc::new(RefCell::new(None)),
            cloak: RefCell::new(None),
            frame_clock: RefCell::new(None),
            perf: RefCell::new(None),
            exit_code: Cell::new(0),
            forced_exit: Cell::new(None),
        })
    }
}
