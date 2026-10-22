//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 84-216 (the class summary, `Run`, `Dispose`, `RunLoop`, `RunStartupGuardedAsync`), 360-392 (the second half of
//! `StartupCoreAsync`), 596-628 (`OnForwardedOpen`, `ObserveAsync`) and 630-640 (`LogUnhandledForced`) (baseline `d92d6aae`), and `Program.cs` `Main` (16 lines, the call). Unit W8-17e.
//! Contract: `contracts/W8-17e.md`.
//!
//! [`ShellStartup::run`] is what the Rust `main` (W8-20a) calls: it replaces `App.StartupCoreAsync` with a Win32 message loop and no WPF `Application`. Sequence (perf marks keep the names of the
//! WPF build): `appStartup`; DPI PerMonitorV2; warm the settings parser and the graphics on workers; `ShellLaunch`; dispatcher + `MessageLoop` of the UI thread; everything after that runs as a chain
//! of UI-thread steps (no async): services (`servicesBuilt`), Explorer prefetch, placement prefetch, settings + language on a worker (`settingsFileLoaded`, `languageLoaded`, `settingsLoaded`),
//! instance lock + hand-off (`instanceLock`) [W8-17d, `phase_one.rs`]; early decode only AFTER the lock (`earlyDecodeStarted`), hidden window + cloak + placement before `ShowWindow`
//! (`placementRestoredBeforeShow`, `windowShown`), view model + Direct2D surface (`mainWindowConstructed`), first frame (`firstPresent`, `windowRevealed`), first image; journal reconcile (INV-6) AFTER the
//! lock and AFTER the window is shown, before the loop takes its first input [W8-17e].
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::rc::Rc;
use std::sync::Arc;

use pr_core::abstractions::Log;

use super::error::StartupError;
use super::options::ShellStartupOptions;
use super::phase_one::PhaseOneOutcome;
use super::state::StartupState;

/// The entry point of the shell (C# `ShellStartup.Run`). A unit type: the state of a run is [`StartupState`].
#[derive(Debug, Clone, Copy)]
pub struct ShellStartup;

impl ShellStartup {
    /// C# `Run(args, options)` + `RunLoop()`: runs until the main window closes and returns the process exit code (`0` normal, `EXIT_STARTUP_FAILED` (1) when the start-up failed or the loop faulted
    /// `MAX_LOOP_FAULTS` times, `EXIT_SMOKE_TIMEOUT` (7) for a smoke run without a frame, the forced code of a smoke close). Call it on the process's MAIN thread: it owns the dispatcher and the loop.
    /// Never panics. In order:
    ///
    /// 1. `ShellLaunch::parse(args, &PhysicalFileSystem, &*global_log())` (C# `Run`, before the object exists); `options` as given (`ShellStartupOptions::default()` is the real run).
    /// 2. `process_start = Instant::now()` (the entry of `RunLoop`, C# `Stopwatch.GetTimestamp()`); perf listener: `options.start_perf_listener` -> `PerfCsvListener::try_start_from_environment(Some(version),
    ///    &|| local_time_parts())` (`shell_startup::local_time`), stored in the state; `mark("appStartup")` (mark = `options.mark` or `startup_mark`).
    /// 3. `ensure_per_monitor_v2(None)`; `warm_settings_parser(Arc<LocalizerSettingsTexts of the built-in English localizer>, mark)`; `warm_graphics(mark)` only when `options.warm_graphics`. The join
    ///    handles are dropped (fire and forget, C# `_ =`).
    /// 4. `Win32UiDispatcher::new(global_log())` and `MessageLoop::new(Rc::new(dispatcher.clone()))`: an `Err(HostError)` BEFORE the loop exists is logged `AppLog::error_forced("Startup failed", ..)` and the
    ///    run returns `EXIT_STARTUP_FAILED` (no box: nothing can show one yet; the C# `Main` crashed).
    /// 5. `router = ForwardedOpenRouter::new()` + `attach_to_current_thread()` (D-W8B6-15 e); `hub = Rc::new(ShellPresentationHub::new())`; `StartupState::new(..)`.
    /// 6. `install_process_hooks(..)` when `options.install_process_hooks` (see there).
    /// 7. The FIRST step is posted, not run inline (C# `dispatcher.Post(() => _ = RunStartupGuardedAsync())`): `ui_hop(ui, |_| state.begin())` and `send(())`.
    /// 8. The loop (C# lines 175-194): `loop { match catch_unwind(AssertUnwindSafe(|| message_loop.run())) { Ok(Ok(code)) => { exit_code = code; break }, Ok(Err(HostError)) => log forced + `exit_code =
    ///    EXIT_STARTUP_FAILED`, break, Err(payload) => { log_unhandled_forced("UI loop exception", payload); if counter.record(MAX_LOOP_FAULTS) == GiveUp { exit_code = EXIT_STARTUP_FAILED; break } } } }`.
    ///    The `catch_unwind` around `run` is THE ONE panic catch of the UI thread (D-W8B6-25).
    /// 9. `forced_exit` (when set) replaces the exit code; `run_shutdown(&mut StateSteps::new(state.clone()))`; then `dispatcher.shutdown()` (C# `using var dispatcher`: LAST, so late posts of workers are dropped, D-W8B6-T9); return the code. Every `ui_hop` continuation of the run captures a `Weak<StartupState>` (an undelivered hop must not keep the state, and through it the dispatcher, alive after `run` returns).
    pub fn run<S: AsRef<str>>(args: &[S], options: ShellStartupOptions) -> i32 {
        todo!()
    }
}

/// C# `AppDomain.CurrentDomain.UnhandledException` (lines 320-321): `std::panic::set_hook` chained to the previous hook, which writes `AppLog::global().error_forced("Unhandled panic", &UnhandledError(info
/// text))` (payload text through `describe_payload`, plus the location) BEFORE calling the previous hook. Forced = written even when logging is off, flushed (R2-F-12: this is a crash report). A hook that
/// panics itself is guarded (`catch_unwind`: "a broken logger must not turn a handled error into another one", C# `LogUnhandledForced`). `UnobservedTaskException` (line 322-326) has no Rust twin: a
/// worker's panic is caught and reported at its own thread (R0003), recorded as a `Different` row (D-W8B6-25). Called ONCE per process; a second call is a no-op (`OnceLock`).
pub fn install_process_hooks(log: Arc<dyn Log>) {
    todo!()
}

/// C# `LogUnhandledForced(message, exceptionObject)` (lines 630-640): `AppLog::global().error_forced(message, &UnhandledError(describe_payload(payload)))`, with a panic of the logger itself swallowed.
pub fn log_unhandled_forced(message: &str, payload: &(dyn std::any::Any + Send)) {
    todo!()
}

impl StartupState {
    /// C# `RunStartupGuardedAsync()` (lines 205-216) as the posted first step: `start_phase_one(done)` where `done = move |result| state.after_phase_one(result)`, wrapped in `catch_step` (a panic or an
    /// `Err` is `fail_startup`).
    pub(super) fn begin(self: &Rc<Self>) {
        todo!()
    }

    /// The seam between the halves of `StartupCoreAsync`: `Ok(Proceed)` -> `catch_step(|| self.phase_two())`, an `Err` of it -> `fail_startup`; `Ok(SecondInstanceExit)` -> `exit_code.set(0)` and
    /// `MessageLoop::quit(0)` (C# lines 350-352); `Err(e)` -> `fail_startup(&e)`.
    pub(super) fn after_phase_one(self: &Rc<Self>, result: Result<PhaseOneOutcome, StartupError>) {
        todo!()
    }

    /// C# lines 360-392 (the second half of `StartupCoreAsync`), UI thread, in order:
    ///
    /// 1. **Early decode** (361-364): when the early-decode slot holds a task AND `launch.initial_file` is `Some`: spawn a worker (`PhotoReview.EarlyDecodeStart`) that `join`s the task and runs
    ///    `early_decode::start_early_decode(plan, &graph.size_source, &settings, file, &mark)`. Only HERE, after the lock: a refused second launch never decodes (it returned from phase one).
    /// 2. **Window** (366-367): [`create_main_window`](Self::create_main_window)`?`; `options.on_main_window_ready.take()` is called with `&main` (an `&Rc<MainWindow>`, D-W8B6-52) (UI thread).
    /// 3. **Open the first path** (370-372): `main.open_initial(launch.initial_path(), launch.initial_file.is_some())`. The C# observed the returned task (`"Open initial path failed"`); the Rust call is
    ///    synchronous and infallible, the view model reports its own failures.
    /// 4. **Forwarded request that arrived while starting** (374-381): `router.take_pending()` -> `router.on_forwarded_open(path)` once.
    /// 5. **Journal** (383-386): `start_journal_recovery()` unless smoke.
    /// 6. **Repair notice** (388-391): `settings_load::repair_notice(&store, &LocalizerSettingsTexts::new(Localizer::current()))` -> `services.dialog_service.show_message(app_title, text)` (modal
    ///    information box, once; a valid config shows nothing).
    pub(super) fn phase_two(self: &Rc<Self>) -> Result<(), StartupError> {
        todo!()
    }

    /// C# `FailStartup(ex)`: `run_fail_startup(&mut StateSteps::new(self.clone()), error)`.
    pub(super) fn fail_startup(self: &Rc<Self>, error: &StartupError) {
        todo!()
    }
}
