//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 253-359 (the first half of `StartupCoreAsync`, baseline `d92d6aae`): services, error log, prefetches, settings and language
//! load (C-28 input), logging switch, forward coalescer and the single-instance lock. Unit W8-17d. Contract: `contracts/W8-17d.md`.
//!
//! The C# method awaits twice (`await settingsLoad`, `await SecondInstanceHandoff.TryForwardAsync`) and continues on the UI thread. Rust has no async (D-W8-2, R0003): the same sequence is a
//! step machine whose two continuations are `startup::ui_hop` hops. EVERY path ends in exactly one call of the `done` callback, on the UI thread: `Ok(Proceed)` (this process holds the lock, or
//! runs `--smoke`: go on to the window, W8-17e), `Ok(SecondInstanceExit)` (the request was handed over or answered with a box: quit with code 0) or `Err` (the start-up failed: W8-17e's
//! `fail_startup`). Each step runs under `error::catch_step`, so a panic is an `Err(Panicked)` and never leaves a window-less process holding the mutex (R2-F-04).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::rc::Rc;

use pr_app::startup::settings_load::LoadedSettings;
use pr_app::startup::ui_hop::HopLost;

use super::error::StartupError;
use super::state::StartupState;

/// How the first half ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseOneOutcome {
    /// Go on: this process holds the instance lock (or is a `--smoke` run, which takes none).
    Proceed,
    /// Another instance owns the request: quit with exit code 0 (C# `_exitCode = 0; MessageLoop.Quit(0)`). The caller does the quit.
    SecondInstanceExit,
}

/// The continuation of the first half (UI thread, called exactly once).
pub type PhaseOneDone = Box<dyn FnOnce(Result<PhaseOneOutcome, StartupError>)>;

impl StartupState {
    /// C# lines 253-299, then (through the hop) 300-359. In order, all on the UI thread unless said otherwise:
    ///
    /// 1. **Services** (255-260): `ShellServices::build(ShellHostServices { dispatcher, owner_window: owner closure over a `Weak` of `self.main` (0 once the slot is empty or the window destroyed, D-W8B5-1),
    ///    message_loop: Some(loop) }, hub, log, options.services.take())`, then the graph: `options.graph_builder.take().unwrap_or_else(production_graph_builder)(GraphInputs { .. })`
    ///    (an `Err` is `StartupError::Graph(text)`); `services.bind_dialog_services(graph.dialog_services.clone())` (D-W8B6-15 c; the first bind wins); store both in the state; `mark("servicesBuilt")`.
    /// 2. **Error log** (262-264): `error_log::configure(Some(config_file.parent().join(FILE_NAME)))` (`parent` of `graph.config_file`, an empty path when it has none).
    /// 3. **Explorer order** (266-271): when `launch.launch_folder` is `Some` and not empty: `graph.explorer_order.prefetch(Path::new(folder), EXPLORER_PREFETCH_TIMEOUT)` (non-blocking).
    /// 4. **Placement** (273-277): smoke -> no prefetch (`None`); else `placement_store.prefetch(&graph.window_placement_file.to_string_lossy())` -> `Some(PrefetchHandle)`.
    /// 5. **Log switch** (281): `store.subscribe_changed(Box::new(move |s| { AppLog::global().set_enabled(logging_enabled(s, &diag)); Ok(()) }))` (`diag` = `graph::diag_options()`).
    /// 6. **Localization mode** (282-283): `localization.lock().set_mode(LocalizationService::parse_mode(&args))` (a poisoned mutex is recovered).
    /// 7. **Settings worker** (284-299, `Task.Run`): `ui_hop(ui, continuation)` gives the sender; a named thread (`PhotoReview.SettingsLoad`) runs `settings_load::load_settings_and_language(store,
    ///    localization, mark, after_file_loaded)` under `catch_unwind` and sends the `Result<LoadedSettings, StartupLoadError>`. `after_file_loaded` (C# 291-294) starts the early-decode
    ///    preparation when `launch.initial_file` is `Some`: it takes the placement handle (smoke: none) and spawns `StartupTask::spawn("PhotoReview.EarlyDecodePlan", ..)` that joins the handle and calls
    ///    `early_decode::prepare_early_decode(placement.as_ref(), previews, &Win32MonitorProbe, DEFAULT_WIDTH_DIP, DEFAULT_HEIGHT_DIP, PREVIEW_QUALITY_MULTIPLIER, &*log)`, and stores the task in
    ///    `self.early_decode` (the SLOT, so the UI thread finds it after the hop). The placement handle NOT consumed (no initial file) goes to the context: it lives in an `Arc<Mutex<Option<PrefetchHandle>>>` created in step 7, captured by `after_file_loaded` (an `Fn`, it `take()`s) and by the hop continuation (which `take()`s what is left for `build_startup_context`). Every continuation of this half captures a `Weak<StartupState>` (upgrade failure = the run is over: do nothing). `start_phase_one` is `pub` because the integration tests of `crates/pr-shell/tests/` (separate crate) call it; the other steps stay `pub(super)`.
    /// 8. The continuation ([`on_settings_loaded`](Self::on_settings_loaded)) runs on the UI thread when the worker sent its result.
    ///
    /// `done` receives the outcome of the whole half.
    pub fn start_phase_one(self: &Rc<Self>, done: PhaseOneDone) {
        todo!()
    }

    /// C# lines 300-359 after `await settingsLoad`. `Err(HopLost)` (the worker panicked or could not start) is `StartupError::Worker`, `Ok(Err(e))` is `StartupError::Settings(e)`. Then, in order:
    ///
    /// 1. `settings_load::apply_loaded_settings` (localization published, mark `settingsLoaded`); keep `loaded.settings` in `self.settings`.
    /// 2. **Context** (C-16, 300-303): `self.context = Some(build_startup_context(&launch, &settings, unconsumed_placement_handle, process_start))`.
    /// 3. **Logging** (307-316): `AppLog::global().set_enabled(logging_enabled(..))`; `startup_log_lines(&args, &diag)`: the first line through `AppLog::info` when enabled; the DIAG line, when present,
    ///    through `AppLog::global().write_forced(|| AppLog::global().error(line, None))`.
    /// 4. **Coalescer** (329-330, also for smoke): `new_forward_coalescer(ui.clone(), log.clone())`, stored; `self.router.attach_to_current_thread()` was done by `run` (D-W8B6-15 e).
    /// 5. **Instance lock** (331-354): smoke -> none. Else `instance::create_scope(settings.instance_mode, coalescer, log, &options.instance_name_prefix)` stored in `self.instance` (Q-R18: the mode comes
    ///    from the settings just read, BEFORE any lock), `ScopeGate::new(scope, log)`, `try_take_lock(&gate, launch.launch_folder.as_deref())`: `Ok(true)` -> step 6; `Ok(false)` -> the HAND-OFF: a hop
    ///    and a worker (`PhotoReview.Handoff`) that run `gate.forward_to_owner(folder, launch.existing_paths())` (blocks up to 1.5 s) and send the `Result<bool, InstanceStartError>`; its
    ///    continuation (`on_handoff_done`) maps `Err` to `StartupError::Instance`, `Ok(forwarded)` to `handoff_decision(forwarded, gate.mode())`: `ExitSilently` -> no box; `ExitWithMessage(kind)` ->
    ///    `services.dialog_service.show_message(startup_box_title(..), instance_message_text(kind, &Localizer::current()))`; then `done(Ok(SecondInstanceExit))`. The scope stays in the state
    ///    (the shutdown disposes it).
    /// 6. `if instance.is_some() { mark("instanceLock") }`, then `done(Ok(Proceed))`.
    ///
    /// Panics in any step are `StartupError::Panicked` through `catch_step`.
    pub(super) fn on_settings_loaded(
        self: &Rc<Self>,
        result: Result<Result<LoadedSettings, pr_app::startup::settings_load::StartupLoadError>, HopLost>,
        done: PhaseOneDone,
    ) {
        todo!()
    }
}
