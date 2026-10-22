//! The whole-shell test rig of unit W8-20b (`contracts/W8-20b.md`): the Rust twin of the C# `ShellRun` + `TestJpeg` + `ShellFrames` helpers of `tests/PhotoReview.Shell.Integration.Tests/Startup/`
//! (baseline `d92d6aae`). It starts `ShellStartup::run` for real on its own STA thread (a hidden desktop when the lead runs the tests through `tools/run-tests-hidden.ps1`, WARP, a temp data
//! root, a unique instance prefix, no perf listener, no process hooks, no activation) and exposes what a test may do to it: send input messages from OTHER threads, run a closure on the UI
//! thread, wait for a CONDITION on the state the window reports after every frame (a `Condvar`, never a sleep or a poll), and close it.
//!
//! Self-contained on purpose (AGENTS.md "Mutation testing and parallel agents": helper files get a name unique to the unit): it does NOT include `startup_fixture.rs` of 17d/17e, which those units
//! edit; the two rigs only share the public API of `pr_shell::startup` (`ShellStartup::run`, `ShellStartupOptions`, `GraphBuilder`). Included by every `ui_smoke*.rs` with
//! `#[path = "common/ui_smoke_fixture.rs"] mod ui_smoke_fixture;`.
//!
//! No `unsafe` here (the rig posts messages through `pr_platform_win::ui_driver`). STUB: every body is `todo!()`; the porter implements them in the order of the contract.
#![allow(unused_variables, dead_code)] // stub: not every test file uses every item; the porter removes the first half of this line with the last `todo!()`
#![cfg(windows)]

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

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
}

impl Default for SmokeConfig {
    /// Three photos of `parity-400x300.jpg`, `OpenTarget::FirstPhoto`, no config file, `GraphChoice::Production`, no edit.
    fn default() -> Self {
        todo!()
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
}

/// UI-thread handles the rig keeps for closures passed to [`UiSmokeRun::on_ui`] (a thread-local filled by the `on_main_window_ready` hook, never shared with another thread).
pub struct UiProbe {
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
#[derive(Debug, Default)]
struct RunState {
    hwnd: Hwnd,
    snapshot: UiSnapshot,
    marks: Vec<String>,
    main_ready: bool,
    exit_code: Option<i32>,
    error: Option<String>,
}

impl UiSmokeRun {
    /// Creates the temp root and the photos, writes the config file, builds the options (see [`SmokeConfig::configure`]) and spawns the thread named `UiSmokeRun`, which: enters
    /// `StaApartment::enter()`, runs `ShellStartup::run(&args, options)` inside `catch_unwind` (a panic becomes `RunState::error`), stores the exit code, and notifies. Returns at once; the
    /// shell comes up in the background. `Err` when a previous run was abandoned, or the photos or the config file could not be written (message names the path).
    pub fn start(config: SmokeConfig) -> Result<UiSmokeRun, String> {
        todo!()
    }

    /// The photos written by `start`, in name order.
    pub fn photos(&self) -> &[PathBuf] {
        todo!()
    }

    /// The temp root.
    pub fn root(&self) -> &std::path::Path {
        todo!()
    }

    /// The folder of the bin: items sent to it are moved here, never to the real Recycle Bin.
    pub fn bin(&self) -> &TempFolderRecycleBin {
        todo!()
    }

    /// Waits (a `Condvar`, [`BOUND`]) until the main window is ready (the `on_main_window_ready` hook ran) and the first image is presented; `Err(diagnostics)` on expiry or when the run ended first.
    pub fn wait_first_image(&self) -> Result<UiSnapshot, String> {
        todo!()
    }

    /// Waits (a `Condvar`, [`BOUND`]) until `predicate(&snapshot)` is true for a snapshot taken after a frame; returns that snapshot. The predicate is evaluated under the lock: it must be quick and
    /// must not call the rig. `Err` names `what` and carries [`UiSmokeRun::diagnostics`].
    pub fn wait_until(&self, what: &str, predicate: impl Fn(&UiSnapshot) -> bool) -> Result<UiSnapshot, String> {
        todo!()
    }

    /// The last snapshot, without waiting.
    pub fn snapshot(&self) -> UiSnapshot {
        todo!()
    }

    /// `ui_driver::post_key(hwnd, vk, true)` then `post_key(hwnd, vk, false)`. `Err` when the window does not exist yet or the post failed.
    pub fn key(&self, virtual_key: u16) -> Result<(), String> {
        todo!()
    }

    /// `ui_driver::post_mouse_wheel` at the centre of the client area (read from the UI thread once and cached).
    pub fn wheel(&self, delta: i16) -> Result<(), String> {
        todo!()
    }

    /// `ui_driver::post_mouse_move` / `post_left_button` at a client point in pixels.
    pub fn mouse_move(&self, client_x: i32, client_y: i32) -> Result<(), String> {
        todo!()
    }

    /// Press and release the left button at a client point in pixels.
    pub fn click(&self, client_x: i32, client_y: i32) -> Result<(), String> {
        todo!()
    }

    /// Runs `f` on the UI thread (posted through the `UiScheduler` of the window's dispatcher) with the [`UiProbe`] and returns its result; waits with `recv_timeout(BOUND)`. `Err` when the UI
    /// thread is gone or the closure panicked (the panic text is in the message).
    pub fn on_ui<R: Send + 'static>(&self, f: impl FnOnce(&UiProbe) -> R + Send + 'static) -> Result<R, String> {
        todo!()
    }

    /// Posts `WM_CLOSE` to the main window (the close button) without waiting.
    pub fn request_close(&self) -> Result<(), String> {
        todo!()
    }

    /// `request_close`, then waits (a `Condvar`, [`BOUND`]) for `ShellStartup::run` to return and gives its exit code. `Err` on expiry (the message carries the diagnostics).
    pub fn close_and_wait(&self) -> Result<i32, String> {
        todo!()
    }

    /// Waits for the run to end by itself (a smoke run, a start-up failure) and gives its exit code.
    pub fn wait_exit(&self) -> Result<i32, String> {
        todo!()
    }

    /// The perf marks seen so far, in order (`appStartup`, `servicesBuilt`, ... `firstPresent`).
    pub fn marks(&self) -> Vec<String> {
        todo!()
    }

    /// One line per fact a failing test needs (C# `ShellRun.Diagnostics` + `HangReport`): marks, error, exit code, snapshot, whether the thread is alive, the tail (6000 bytes) of the run's own
    /// `app.log`.
    pub fn diagnostics(&self) -> String {
        todo!()
    }
}

impl Drop for UiSmokeRun {
    /// See the type doc. Never panics (a failing test must report its own failure, not a double panic).
    fn drop(&mut self) {
        todo!()
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
        todo!()
    }

    /// The original paths sent so far, in order.
    pub fn sent(&self) -> Vec<PathBuf> {
        todo!()
    }
}

impl RecycleBin for TempFolderRecycleBin {
    /// `std::fs::rename` of `path` to `<bin_dir>/<n>-<file name>` (n = number of items sent so far), recording `(original, stored)`; creates `bin_dir` first.
    fn send_to_recycle_bin(&self, path: &std::path::Path) -> std::io::Result<()> {
        todo!()
    }

    /// Finds the LAST record of `original_path`, checks that `expected_size` equals the stored file's length (the last write time is not compared: `expected_last_write_utc` is accepted), moves it back
    /// when the original path is free (`Ok(true)`), `Ok(false)` when there is no such record, the size differs or the path is occupied; `Err` only for an I/O error of the move.
    fn try_restore(
        &self,
        original_path: &std::path::Path,
        expected_size: u64,
        expected_last_write_utc: DateTimeUtc,
    ) -> std::io::Result<bool> {
        todo!()
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
