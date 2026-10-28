//! The whole-shell test rig of unit W8-20b (`contracts/W8-20b.md`): the Rust twin of the C# `ShellRun` + `TestJpeg` + `ShellFrames` helpers of `tests/PhotoReview.Shell.Integration.Tests/Startup/`
//! (baseline `d92d6aae`). It starts `ShellStartup::run` for real on its own STA thread (a hidden desktop when the lead runs the tests through `tools/run-tests-hidden.ps1`, WARP, a temp data
//! root, a unique instance prefix, no perf listener, no process hooks, no activation) and exposes what a test may do to it: send input messages from OTHER threads, run a closure on the UI
//! thread, wait for a CONDITION on the state the window reports after every frame (a `Condvar`, never a sleep or a poll), and close it.
//!
//! Self-contained on purpose (AGENTS.md "Mutation testing and parallel agents": helper files get a name unique to the unit): it does NOT include `startup_fixture.rs` of 17d/17e, which those units
//! edit; the two rigs only share the public API of `pr_shell::startup` (`ShellStartup::run`, `ShellStartupOptions`, `GraphBuilder`). Included by every `ui_smoke*.rs` with
//! `#[path = "common/ui_smoke_fixture.rs"] mod ui_smoke_fixture;`.
//!
//! No unsafe code: messages go through ui_driver. Each native test re-executes in an isolated child process.
#![allow(dead_code)] // Each smoke binary uses a different subset of this shared rig.
#![cfg(windows)]

use pr_core::abstractions::{NullLog, UiScheduler};
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::host::ui_dispatcher::Win32UiDispatcher;
use pr_platform_win::shell_startup::apartment::StaApartment;
use pr_platform_win::ui_driver;
use pr_shell::startup::graph::{GraphOverrides, production_graph_builder_with};
use pr_shell::startup::run::ShellStartup;
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::{PoisonError, mpsc};

thread_local! { static PROBE: RefCell<Option<UiProbe>> = const { RefCell::new(None) }; }
thread_local! {
    static SERVICES: RefCell<Option<Rc<pr_shell::startup::composition_root::ShellServices>>> = const { RefCell::new(None) };
    static SETTINGS: RefCell<Option<Arc<pr_core::settings::store::SettingsStore>>> = const { RefCell::new(None) };
}
static ABANDONED: Mutex<Option<String>> = Mutex::new(None);

/// Re-executes precisely one test with process-local environment overrides, avoiding unsafe global environment mutation.
pub fn isolated_test(name: &str, body: impl FnOnce()) {
    if std::env::var("PHOTOREVIEW_RS_UI_SMOKE_CHILD").as_deref() == Ok(name) {
        assert!(std::env::var_os("PHOTOREVIEW_RS_DATA_ROOT").is_some());
        assert!(std::env::var_os("PHOTOREVIEW_RS_INSTANCE_PREFIX").is_some());
        body();
        return;
    }
    let root = TempRoot::new("w8-20b-process");
    let prefix = format!(
        "PhotoReviewRsTest{}",
        root.path().file_name().unwrap().to_string_lossy()
    );
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--ignored", "--nocapture", "--test-threads=1"])
        .env("PHOTOREVIEW_RS_UI_SMOKE_CHILD", name)
        .env("PHOTOREVIEW_RS_DATA_ROOT", root.path().join("data"))
        .env("PHOTOREVIEW_RS_ISOLATE_CONFIG", "1")
        .env("PHOTOREVIEW_RS_INSTANCE_PREFIX", prefix)
        .env("PHOTOREVIEW_RS_NOWARM", "1")
        .env_remove("PHOTOREVIEW_RS_PERF_TRACE")
        .env_remove("PHOTOREVIEW_RS_FRAME_LOG")
        .spawn()
        .unwrap();
    // A channel wait bounds the OS wait without sleep polling. The timeout path only terminates our child.
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let _ = tx.send(child.wait());
    });
    match rx.recv_timeout(Duration::from_secs(300)) {
        Ok(status) => {
            waiter.join().unwrap();
            assert!(status.unwrap().success(), "isolated test {name} failed");
        }
        Err(error) => {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .status();
            panic!("isolated test {name} exceeded 300 s: {error}");
        }
    }
}
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use pr_core::abstractions::RecycleBin;
use pr_core::time::DateTimeUtc;
use pr_platform_win::host::main_window::core::SharedViewModel;
use pr_platform_win::shell_viewing::composer::PresentationComposer;
use pr_platform_win::shell_viewing::viewport_controller::ViewportController;
use pr_platform_win::win32::structs::Hwnd;
use pr_shell::startup::graph::GraphBuilder;
use pr_shell::startup::options::ShellStartupOptions;
use pr_test_support::TempRoot;

/// The bound of every wait of the rig (C# `ShellRun.DefaultWait`): 60 s. A wait that expires fails the test with [`UiSmokeRun::diagnostics`] in its message.
pub const BOUND: Duration = Duration::from_secs(60);

/// How the rig starts the shell.
pub enum OpenTarget {
    /// The first photo FILE of the generated folder (C# `[jpeg]`): opens its folder at that file.
    FirstPhoto,
    /// The generated folder itself.
    PhotosFolder,
    /// No argument: the empty window.
    Nothing,
    /// These arguments as they are (`--smoke`, a second path...).
    Arguments(Vec<String>),
}

/// A caller's graph builder: gets the temp paths and the temp bin, returns the builder (through `production_graph_builder_with(GraphOverrides { .. })`).
pub type CustomGraphFn = Box<dyn FnOnce(&AppPathsForRig) -> GraphBuilder + Send>;

/// The graph the shell runs on.
pub enum GraphChoice {
    /// `production_graph_builder()` (W8-09b) over the temp `AppPaths`, with `TempFolderRecycleBin` as the Recycle Bin: the normal choice.
    Production,
    /// A caller's builder (the stale-preview test wraps the decoder factory; the keep-warm test injects the work): built from the temp paths by the closure.
    Custom(CustomGraphFn),
}

/// What a `GraphChoice::Custom` closure needs to build a graph over the rig's temp folders.
#[derive(Debug, Clone)]
pub struct AppPathsForRig {
    /// `pr_core::app_paths::AppPaths::new(root, Some(<root>/data), true)`: the config, journal, sessions, log, cache and placement files all live under the temp root.
    pub paths: pr_core::app_paths::AppPaths,
    /// The Recycle Bin the graph must use: [`TempFolderRecycleBin`] (NEVER the real bin).
    pub bin: Arc<TempFolderRecycleBin>,
}

/// A last-moment edit of the start-up options (see `SmokeConfig::configure`).
pub type ConfigureFn = Box<dyn FnOnce(&mut ShellStartupOptions) + Send>;

/// Everything a test configures.
pub struct SmokeConfig {
    /// How many copies of the fixture JPEG are written to `<root>/photos` as `<name_prefix>00.jpg`, `<name_prefix>01.jpg`, ... (zero-padded to two digits).
    pub photo_count: usize,
    /// The file-name prefix of the copies: `"s"` by default (`s00.jpg`), `"p"` for the `ShellViewingTests` ports (`p00.jpg`, as in C# `MakeFolder`).
    pub name_prefix: &'static str,
    /// The file name inside `tests/fixtures/turbojpeg/` that is copied (found through `pr_test_support::golden::repo_root()`): `parity-400x300.jpg` by default, `parity-640x480.jpg` for the zoom
    /// test (the larger one, so Fit and 100 % differ).
    pub fixture: &'static str,
    /// What the command line is.
    pub open: OpenTarget,
    /// Written to `AppPaths::config_file` BEFORE the start (settings JSON in the C# format, e.g. `{"ConfirmBeforeDelete":true}`); `None` writes nothing (defaults).
    pub config_json: Option<String>,
    /// The graph.
    pub graph: GraphChoice,
    /// A last-moment edit of the options (a fake `journal_recovery`, a shorter `smoke_timeout`...). The rig has ALREADY set: `instance_name_prefix` = `PhotoReviewRsTest` + 10 hex digits of a GUID,
    /// `prefer_warp = true`, `activate = false`, `warm_graphics = false`, `install_process_hooks = false`, `start_perf_listener = false`, and wrapped `mark` and `on_main_window_ready`
    /// (the rig's wrappers call the closure's, so the edit may add its own).
    pub configure: Option<ConfigureFn>,
    /// `true` activates the comctl32 v6 manifest on the shell thread (the real exe embeds it, a test exe does not), so `TaskDialogIndirect` exists and the dialogs of the shell (failure box, recovery question) really show.
    pub task_dialogs: bool,
}

impl Default for SmokeConfig {
    /// Three photos of `parity-400x300.jpg`, `OpenTarget::FirstPhoto`, no config file, `GraphChoice::Production`, no edit.
    fn default() -> Self {
        Self {
            photo_count: 3,
            name_prefix: "s",
            fixture: "parity-400x300.jpg",
            open: OpenTarget::FirstPhoto,
            config_json: None,
            graph: GraphChoice::Production,
            configure: None,
            task_dialogs: false,
        }
    }
}

/// What the window reported after the LAST frame (the rig's render subscription refreshes it on the UI thread and notifies the `Condvar`). Plain data: `Send`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiSnapshot {
    /// Frames rendered since the main window was ready.
    pub frames: u64,
    /// `MainWindow::first_image_presented()`.
    pub first_image_presented: bool,
    /// `MainViewModel::current_index()` (`-1` when no image).
    pub current_index: i32,
    /// `MainViewModel::total_files()`.
    pub total_files: usize,
    /// `MainViewModel::folder_title()`.
    pub folder_title: String,
    /// `MainViewModel::status_text()`.
    pub status_text: String,
    /// The zoom of the viewer in percent (100.0 = one source pixel per device pixel); `0.0` when no image.
    pub zoom_percent: f64,
    /// `true` while the viewer is in Fit mode.
    pub is_fit: bool,
    /// The full path of the image the view model shows now, `None` when none.
    pub current_path: Option<String>,
    pub toolbar_opacity: f32,
    pub toolbar_hit_test_visible: bool,
    pub settled: bool,
    pub drawn_buffer_id: Option<i64>,
}

/// UI-thread handles the rig keeps for closures passed to [`UiSmokeRun::on_ui`] (a thread-local filled by the `on_main_window_ready` hook, never shared with another thread).
pub struct UiProbe {
    pub settings_store: Arc<pr_core::settings::store::SettingsStore>,
    pub services: Rc<pr_shell::startup::composition_root::ShellServices>,
    /// The main window handle.
    pub hwnd: Hwnd,
    /// The shared view model of the window.
    pub view_model: SharedViewModel,
    /// The viewport controller (scroll offsets, extents, layout).
    pub viewport: Rc<ViewportController>,
    /// The main window itself (`Rc<MainWindow>`): S2 needs `with_surface` (fault hooks), `render_now`, `image_cache`, `has_surface` and `window().insert_message_handler_first(..)`. REQUIRES a change of
    /// `ShellStartupOptions::on_main_window_ready` from `FnOnce(&MainWindow)` to `FnOnce(&Rc<MainWindow>)` (a lead decision on the 17d/17e stub, see the contract "Requirements on other units", R2): a
    /// `&MainWindow` cannot be kept after the hook returns and `MainWindow` is not `Clone`.
    pub main_window: Rc<pr_platform_win::host::main_window::core::MainWindow>,
    /// `MainWindow::composer()` read when the window was ready (`None` if the surface was not attached yet; the S2/S3 tests wait for the first image first). Slice S3 reads `pending()`,
    /// `current()` and `last_drawn()` from it, as the C# helper `IsSettled` does.
    pub composer: Option<Rc<PresentationComposer>>,
}

/// A running shell. `Drop` closes it (see [`UiSmokeRun::close_and_wait`]), joins the thread with a 15 s bound and removes the temp root; a thread that does not exit marks the whole test
/// process as "abandoned" (a static flag) so every later [`UiSmokeRun::start`] fails at once naming the first hung run (C# `ShellRun.s_abandoned`).
pub struct UiSmokeRun {
    root: TempRoot,
    shared: Arc<(Mutex<RunState>, Condvar)>,
    thread: Option<JoinHandle<()>>,
    photos: Vec<PathBuf>,
    bin: Arc<TempFolderRecycleBin>,
}

/// What the UI thread and the test thread share.
#[derive(Default)]
struct RunState {
    hwnd: Hwnd,
    snapshot: UiSnapshot,
    marks: Vec<String>,
    main_ready: bool,
    exit_code: Option<i32>,
    error: Option<String>,
    dispatcher: Option<Win32UiDispatcher>,
}

impl UiSmokeRun {
    /// Creates the temp root and the photos, writes the config file, builds the options (see [`SmokeConfig::configure`]) and spawns the thread named `UiSmokeRun`, which: enters
    /// `StaApartment::enter()`, runs `ShellStartup::run(&args, options)` inside `catch_unwind` (a panic becomes `RunState::error`), stores the exit code, and notifies. Returns at once; the
    /// shell comes up in the background. `Err` when a previous run was abandoned, or the photos or the config file could not be written (message names the path).
    pub fn start(config: SmokeConfig) -> Result<UiSmokeRun, String> {
        if let Some(reason) = ABANDONED.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
            return Err(format!("previous run abandoned: {reason}"));
        }
        let prefix = std::env::var("PHOTOREVIEW_RS_INSTANCE_PREFIX")
            .map_err(|_| "use isolated_test: instance prefix missing")?;
        if std::env::var_os("PHOTOREVIEW_RS_DATA_ROOT").is_none()
            || std::env::var("PHOTOREVIEW_RS_ISOLATE_CONFIG").as_deref() != Ok("1")
        {
            return Err("use isolated_test: data isolation missing".into());
        }
        let root = TempRoot::new("w8-20b-ui");
        let data = root.path().join("data");
        let paths = pr_core::app_paths::AppPaths::new(root.path(), Some(&data.to_string_lossy()), true);
        let bin = Arc::new(TempFolderRecycleBin::new(root.path().join("bin")));
        let photos_dir = root.path().join("photos");
        std::fs::create_dir_all(&photos_dir).map_err(|e| e.to_string())?;
        let bytes = std::fs::read(
            pr_test_support::golden::repo_root()
                .join("tests/fixtures/turbojpeg")
                .join(config.fixture),
        )
        .map_err(|e| e.to_string())?;
        let mut photos = Vec::new();
        for index in 0..config.photo_count {
            let path = photos_dir.join(format!("{}{:02}.jpg", config.name_prefix, index));
            std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            photos.push(path);
        }
        // A pre-existing isolated config also prevents first-run C# config import.
        std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
        std::fs::write(&paths.config_file, config.config_json.as_deref().unwrap_or("{}")).map_err(|e| e.to_string())?;
        let args = match config.open {
            OpenTarget::FirstPhoto => vec![
                photos
                    .first()
                    .ok_or("FirstPhoto needs a photo")?
                    .to_string_lossy()
                    .into_owned(),
            ],
            OpenTarget::PhotosFolder => vec![photos_dir.to_string_lossy().into_owned()],
            OpenTarget::Nothing => Vec::new(),
            OpenTarget::Arguments(args) => args,
        };
        let task_dialogs = config.task_dialogs;
        let shared = Arc::new((Mutex::new(RunState::default()), Condvar::new()));
        let state = shared.clone();
        let rig_paths = AppPathsForRig {
            paths,
            bin: bin.clone(),
        };
        let thread = std::thread::Builder::new()
            .name("UiSmokeRun".into())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    let _sta = StaApartment::enter().map_err(|e| e.to_string())?;
                    // The real exe carries the comctl32 v6 manifest; the test exe does not, so a test that shows a task dialog (`SmokeConfig::task_dialogs`) activates it here.
                    let _visual_styles = if task_dialogs {
                        Some(
                            pr_platform_win::win32::activation_context::ActivationScope::from_manifest_text(
                                pr_platform_win::win32::comctl::SHELL_MANIFEST,
                            )?,
                        )
                    } else {
                        None
                    };
                    let mut options = ShellStartupOptions {
                        instance_name_prefix: prefix,
                        prefer_warp: true,
                        activate: false,
                        warm_graphics: false,
                        install_process_hooks: false,
                        start_perf_listener: false,
                        journal_recovery: Some(Box::new(|_, _, _| Vec::new())),
                        ..Default::default()
                    };
                    options.graph_builder = Some(match config.graph {
                        GraphChoice::Production => production_graph_builder_with(GraphOverrides {
                            paths: Some(rig_paths.paths),
                            recycle_bin: Some(rig_paths.bin),
                            log: Some(Arc::new(NullLog)),
                            ..Default::default()
                        }),
                        GraphChoice::Custom(builder) => builder(&rig_paths),
                    });
                    if let Some(configure) = config.configure {
                        configure(&mut options);
                    }
                    let builder = options.graph_builder.take().ok_or("rig requires isolated graph")?;
                    options.graph_builder = Some(Box::new(move |inputs| {
                        SERVICES.with(|slot| *slot.borrow_mut() = Some(inputs.services.clone()));
                        let graph = builder(inputs)?;
                        SETTINGS.with(|slot| *slot.borrow_mut() = Some(graph.settings_store.clone()));
                        Ok(graph)
                    }));
                    let old_mark = options.mark.take();
                    let marks = state.clone();
                    options.mark = Some(Arc::new(move |name| {
                        marks
                            .0
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .marks
                            .push(name.to_string());
                        marks.1.notify_all();
                        if let Some(mark) = &old_mark {
                            mark(name);
                        }
                    }));
                    let old_ready = options.on_main_window_ready.take();
                    let ready = state.clone();
                    options.on_main_window_ready = Some(Box::new(move |window| {
                        let probe = UiProbe {
                            settings_store: SETTINGS.with(|slot| slot.borrow().as_ref().unwrap().clone()),
                            services: SERVICES.with(|slot| slot.borrow().as_ref().unwrap().clone()),
                            hwnd: window.window().hwnd(),
                            view_model: window.view_model().clone(),
                            viewport: window.viewport().clone(),
                            main_window: window.clone(),
                            composer: window.composer(),
                        };
                        {
                            let mut run = ready.0.lock().unwrap_or_else(PoisonError::into_inner);
                            run.hwnd = probe.hwnd;
                            run.dispatcher = Some(window.dispatcher().clone());
                            run.main_ready = true;
                        }
                        PROBE.with(|slot| *slot.borrow_mut() = Some(probe));
                        let frames = ready.clone();
                        let weak = Rc::downgrade(window);
                        window.window().subscribe_render(Box::new(move |_| {
                            if let Some(window) = weak.upgrade() {
                                let snapshot = take_snapshot(&window);
                                let mut run = frames.0.lock().unwrap_or_else(PoisonError::into_inner);
                                let count = run.snapshot.frames + 1;
                                run.snapshot = snapshot;
                                run.snapshot.frames = count;
                                frames.1.notify_all();
                            }
                        }));
                        ready.1.notify_all();
                        if let Some(hook) = old_ready {
                            hook(window);
                        }
                    }));
                    Ok::<_, String>(ShellStartup::run(&args, options))
                }));
                PROBE.with(|slot| *slot.borrow_mut() = None);
                SETTINGS.with(|slot| *slot.borrow_mut() = None);
                SERVICES.with(|slot| *slot.borrow_mut() = None);
                let mut run = state.0.lock().unwrap_or_else(PoisonError::into_inner);
                match result {
                    Ok(Ok(code)) => run.exit_code = Some(code),
                    Ok(Err(error)) => run.error = Some(error),
                    Err(payload) => run.error = Some(pr_core::abstractions::panic_message(&*payload)),
                }
                run.dispatcher = None;
                state.1.notify_all();
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            root,
            shared,
            thread: Some(thread),
            photos,
            bin,
        })
    }

    /// The photos written by `start`, in name order.
    pub fn photos(&self) -> &[PathBuf] {
        &self.photos
    }

    /// The temp root.
    pub fn root(&self) -> &std::path::Path {
        self.root.path()
    }

    /// The folder of the bin: items sent to it are moved here, never to the real Recycle Bin.
    pub fn bin(&self) -> &TempFolderRecycleBin {
        &self.bin
    }

    /// Waits (a `Condvar`, [`BOUND`]) until the main window is ready (the `on_main_window_ready` hook ran) and the first image is presented; `Err(diagnostics)` on expiry or when the run ended first.
    pub fn wait_first_image(&self) -> Result<UiSnapshot, String> {
        self.wait_until("first image", |s| s.first_image_presented)
    }

    /// Waits (a `Condvar`, [`BOUND`]) until `predicate(&snapshot)` is true for a snapshot taken after a frame; returns that snapshot. The predicate is evaluated under the lock: it must be quick and
    /// must not call the rig. `Err` names `what` and carries [`UiSmokeRun::diagnostics`].
    pub fn wait_until(&self, what: &str, predicate: impl Fn(&UiSnapshot) -> bool) -> Result<UiSnapshot, String> {
        let run = self.shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        let (run, timed) = self
            .shared
            .1
            .wait_timeout_while(run, BOUND, |run| {
                !predicate(&run.snapshot) && run.exit_code.is_none() && run.error.is_none()
            })
            .unwrap_or_else(PoisonError::into_inner);
        if predicate(&run.snapshot) && !timed.timed_out() {
            return Ok(run.snapshot.clone());
        }
        drop(run);
        Err(format!("{what}: {}", self.diagnostics()))
    }

    /// The last snapshot, without waiting.
    pub fn snapshot(&self) -> UiSnapshot {
        self.shared
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot
            .clone()
    }

    /// `ui_driver::post_key(hwnd, vk, true)` then `post_key(hwnd, vk, false)`. `Err` when the window does not exist yet or the post failed.
    pub fn key(&self, virtual_key: u16) -> Result<(), String> {
        self.posted(
            ui_driver::post_key(self.hwnd(), virtual_key, true) && ui_driver::post_key(self.hwnd(), virtual_key, false),
        )
    }

    /// `ui_driver::post_mouse_wheel` at the centre of the client area (read from the UI thread once and cached).
    pub fn wheel(&self, delta: i16) -> Result<(), String> {
        let (x, y) = self.on_ui(|p| {
            let (w, h) = p.main_window.window().client_size_pixels();
            (w / 2, h / 2)
        })?;
        self.posted(ui_driver::post_mouse_wheel(self.hwnd(), x, y, delta))
    }

    /// `ui_driver::post_mouse_move` / `post_left_button` at a client point in pixels.
    pub fn mouse_move(&self, client_x: i32, client_y: i32) -> Result<(), String> {
        self.posted(ui_driver::post_mouse_move(self.hwnd(), client_x, client_y))
    }

    /// Press and release the left button at a client point in pixels.
    pub fn click(&self, client_x: i32, client_y: i32) -> Result<(), String> {
        self.posted(
            ui_driver::post_left_button(self.hwnd(), client_x, client_y, true)
                && ui_driver::post_left_button(self.hwnd(), client_x, client_y, false),
        )
    }

    /// Runs `f` on the UI thread (posted through the `UiScheduler` of the window's dispatcher) with the [`UiProbe`] and returns its result; waits with `recv_timeout(BOUND)`. `Err` when the UI
    /// thread is gone or the closure panicked (the panic text is in the message).
    pub fn on_ui<R: Send + 'static>(&self, f: impl FnOnce(&UiProbe) -> R + Send + 'static) -> Result<R, String> {
        let dispatcher = {
            self.shared
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .dispatcher
                .clone()
        };
        let dispatcher = dispatcher.ok_or_else(|| self.diagnostics())?;
        let (tx, rx) = mpsc::channel();
        dispatcher.post(Box::new(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                PROBE.with(|slot| f(slot.borrow().as_ref().expect("UI probe ready")))
            }))
            .map_err(|payload| pr_core::abstractions::panic_message(&*payload));
            let _ = tx.send(result);
        }));
        rx.recv_timeout(BOUND)
            .map_err(|e| format!("on_ui: {e}; {}", self.diagnostics()))?
    }

    /// Posts `WM_CLOSE` to the main window (the close button) without waiting.
    pub fn request_close(&self) -> Result<(), String> {
        self.posted(ui_driver::post_close(self.hwnd()))
    }

    /// `request_close`, then waits (a `Condvar`, [`BOUND`]) for `ShellStartup::run` to return and gives its exit code. `Err` on expiry (the message carries the diagnostics).
    pub fn close_and_wait(&self) -> Result<i32, String> {
        self.request_close()?;
        self.wait_exit()
    }

    /// Waits for the run to end by itself (a smoke run, a start-up failure) and gives its exit code.
    pub fn wait_exit(&self) -> Result<i32, String> {
        let run = self.shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        let (run, _) = self
            .shared
            .1
            .wait_timeout_while(run, BOUND, |s| s.exit_code.is_none() && s.error.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(code) = run.exit_code {
            return Ok(code);
        }
        drop(run);
        Err(self.diagnostics())
    }

    /// The perf marks seen so far, in order (`appStartup`, `servicesBuilt`, ... `firstPresent`).
    pub fn marks(&self) -> Vec<String> {
        self.shared
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .marks
            .clone()
    }

    /// One line per fact a failing test needs (C# `ShellRun.Diagnostics` + `HangReport`): marks, error, exit code, snapshot, whether the thread is alive, the tail (6000 bytes) of the run's own
    /// `app.log`.
    pub fn diagnostics(&self) -> String {
        let run = self.shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        format!(
            "root={} marks={:?} exit={:?} error={:?} snapshot={:?}",
            self.root.path().display(),
            run.marks,
            run.exit_code,
            run.error,
            run.snapshot
        )
    }
}

impl Drop for UiSmokeRun {
    /// See the type doc. Never panics (a failing test must report its own failure, not a double panic).
    fn drop(&mut self) {
        let _ = self.request_close();
        let run = self.shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        let (run, _) = self
            .shared
            .1
            .wait_timeout_while(run, Duration::from_secs(45), |s| {
                s.exit_code.is_none() && s.error.is_none()
            })
            .unwrap_or_else(PoisonError::into_inner);
        let ended = run.exit_code.is_some() || run.error.is_some();
        drop(run);
        if let Some(thread) = self.thread.take() {
            if ended {
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(thread.join());
                });
                if rx.recv_timeout(Duration::from_secs(15)).is_ok() {
                    return;
                }
            }
            *ABANDONED.lock().unwrap_or_else(PoisonError::into_inner) = Some(self.diagnostics());
        }
    }
}

/// A Recycle Bin for tests that really moves the file out of its folder (so the folder listing, the catalog and the decode see the file go) but into a folder of the temp root, never into the user's
/// Recycle Bin (AGENTS.md "Recycle Bin"). `Send + Sync`.
#[derive(Debug)]
pub struct TempFolderRecycleBin {
    bin_dir: PathBuf,
    items: Mutex<Vec<(PathBuf, PathBuf)>>,
}

impl TempFolderRecycleBin {
    /// A bin under `bin_dir` (created lazily by the first send).
    pub fn new(bin_dir: PathBuf) -> Self {
        Self {
            bin_dir,
            items: Mutex::new(Vec::new()),
        }
    }

    /// The original paths sent so far, in order.
    pub fn sent(&self) -> Vec<PathBuf> {
        self.items
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(original, _)| original.clone())
            .collect()
    }
}

impl RecycleBin for TempFolderRecycleBin {
    /// `std::fs::rename` of `path` to `<bin_dir>/<n>-<file name>` (n = number of items sent so far), recording `(original, stored)`; creates `bin_dir` first.
    fn send_to_recycle_bin(&self, path: &std::path::Path) -> std::io::Result<()> {
        let mut items = self.items.lock().unwrap_or_else(PoisonError::into_inner);
        std::fs::create_dir_all(&self.bin_dir)?;
        let name = path
            .file_name()
            .ok_or_else(|| std::io::Error::other("missing photo name"))?;
        let stored = self.bin_dir.join(format!("{}-{}", items.len(), name.to_string_lossy()));
        std::fs::rename(path, &stored)?;
        items.push((path.to_path_buf(), stored));
        Ok(())
    }

    /// Finds the LAST record of `original_path`, checks that `expected_size` equals the stored file's length (the last write time is not compared: `expected_last_write_utc` is accepted), moves it back
    /// when the original path is free (`Ok(true)`), `Ok(false)` when there is no such record, the size differs or the path is occupied; `Err` only for an I/O error of the move.
    fn try_restore(
        &self,
        original_path: &std::path::Path,
        expected_size: u64,
        expected_last_write_utc: DateTimeUtc,
    ) -> std::io::Result<bool> {
        let _ = expected_last_write_utc;
        let mut items = self.items.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(index) = items.iter().rposition(|(original, _)| original == original_path) else {
            return Ok(false);
        };
        let stored = &items[index].1;
        if original_path.exists() || std::fs::metadata(stored)?.len() != expected_size {
            return Ok(false);
        }
        std::fs::rename(stored, original_path)?;
        items.remove(index);
        Ok(true)
    }
}

/// Windows virtual-key codes the tests use (`winuser.h`): the rig has no `windows` crate dependency.
pub mod vk {
    /// `VK_ESCAPE`.
    pub const ESCAPE: u16 = 0x1B;
    /// `VK_SPACE`.
    pub const SPACE: u16 = 0x20;
    /// `VK_END`.
    pub const END: u16 = 0x23;
    /// `VK_HOME`.
    pub const HOME: u16 = 0x24;
    /// `VK_LEFT`.
    pub const LEFT: u16 = 0x25;
    /// `VK_RIGHT`.
    pub const RIGHT: u16 = 0x27;
    /// `VK_DELETE`.
    pub const DELETE: u16 = 0x2E;
    /// `VK_ADD` (numpad plus).
    pub const ADD: u16 = 0x6B;
    /// `VK_SUBTRACT` (numpad minus).
    pub const SUBTRACT: u16 = 0x6D;
    /// `F` (the Fit key): the virtual key of a letter is its upper-case ASCII code.
    pub const F: u16 = 0x46;
    /// `D1`: the digit key `1` (zoom 100 %).
    pub const D1: u16 = 0x31;
}

impl UiSmokeRun {
    fn hwnd(&self) -> Hwnd {
        self.shared.0.lock().unwrap_or_else(PoisonError::into_inner).hwnd
    }
    fn posted(&self, ok: bool) -> Result<(), String> {
        if ok {
            Ok(())
        } else {
            Err(format!("message refused: {}", self.diagnostics()))
        }
    }
}
fn take_snapshot(window: &pr_platform_win::host::main_window::core::MainWindow) -> UiSnapshot {
    let vm = window.view_model().borrow();
    let viewer = vm.viewer().borrow();
    let composer = window.composer();
    let pixels = vm.current_image().and_then(|image| image.as_pixels());
    let settled = composer.as_ref().is_some_and(|c| {
        c.pending().is_none()
            && pixels.is_some_and(|p| {
                c.current().is_some_and(|current| Arc::ptr_eq(p, &current))
                    && c.last_drawn().is_some_and(|(drawn, _, _)| Arc::ptr_eq(p, &drawn))
            })
    });
    let drawn_buffer_id = composer.and_then(|c| c.last_drawn()).map(|(p, _, _)| p.id());
    let (toolbar_opacity, toolbar_hit_test_visible) = window
        .with_feature::<pr_platform_win::shell_overlay::main_window::MainWindowOverlay, _>(|o| {
            (o.overlay().toolbar_opacity(), o.overlay().toolbar_hit_test_visible())
        })
        .unwrap_or_default();
    UiSnapshot {
        toolbar_opacity,
        toolbar_hit_test_visible,
        settled,
        drawn_buffer_id,
        frames: 0,
        first_image_presented: window.first_image_presented(),
        current_index: vm.current_index(),
        total_files: vm.total_files(),
        folder_title: vm.folder_title().to_owned(),
        status_text: vm.status_text(),
        zoom_percent: (if viewer.is_fit() {
            viewer.fit_zoom()
        } else {
            viewer.zoom()
        }) * 100.0,
        is_fit: viewer.is_fit(),
        current_path: vm.catalog().current().map(|e| e.path().to_owned()),
    }
}

/// Accesses the thread-local probe from callbacks already running on the shell UI thread.
pub fn with_probe<R>(f: impl FnOnce(&UiProbe) -> R) -> R {
    PROBE.with(|slot| f(slot.borrow().as_ref().expect("UI probe ready")))
}

impl UiSmokeRun {
    /// Posts work without waiting, allowing the test thread to operate a modal dialog.
    pub fn post_on_ui<R: Send + 'static>(
        &self,
        f: impl FnOnce(&UiProbe) -> R + Send + 'static,
    ) -> Result<mpsc::Receiver<Result<R, String>>, String> {
        let dispatcher = {
            self.shared
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .dispatcher
                .clone()
        }
        .ok_or("UI not ready")?;
        let (tx, rx) = mpsc::channel();
        dispatcher.post(Box::new(move || {
            let result =
                catch_unwind(AssertUnwindSafe(|| with_probe(f))).map_err(|p| pr_core::abstractions::panic_message(&*p));
            let _ = tx.send(result);
        }));
        Ok(rx)
    }
    pub fn wait_for_owned_window(&self, what: &str) -> Result<Hwnd, String> {
        self.wait_owned(what, true)?.ok_or_else(|| self.diagnostics())
    }
    pub fn wait_no_owned_window(&self) -> Result<(), String> {
        self.wait_owned("owned windows closed", false).map(|_| ())
    }
    fn wait_owned(&self, what: &str, present: bool) -> Result<Option<Hwnd>, String> {
        let deadline = Instant::now() + BOUND;
        loop {
            let owned = ui_driver::first_window_owned_by(self.hwnd());
            if owned.is_some() == present {
                return Ok(owned);
            }
            if Instant::now() >= deadline {
                return Err(format!("{what}: {}", self.diagnostics()));
            }
            let run = self.shared.0.lock().unwrap_or_else(PoisonError::into_inner);
            let _wait = self
                .shared
                .1
                .wait_timeout(
                    run,
                    Duration::from_millis(50).min(deadline.saturating_duration_since(Instant::now())),
                )
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}
