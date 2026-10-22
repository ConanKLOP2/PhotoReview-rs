//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 463-502 (`ArmSmokeExit`, baseline `d92d6aae`): the window side of the `--smoke` run. Unit W8-17e.
//! Contract: `contracts/W8-17e.md`. The latch and the trigger choice are `pr_app::startup::smoke`.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::rc::Rc;

use pr_platform_win::host::main_window::MainWindow;

use super::state::StartupState;

impl StartupState {
    /// C# `ArmSmokeExit(main)`: smoke closes right after the frame that has the image (or the first frame when nothing is opened) and exits 0; a run that sees no such frame within
    /// `options.smoke_timeout` closes with `EXIT_SMOKE_TIMEOUT`. ONE `Arc<SmokeLatch>`; `close_once(code)` = `if !latch.try_close() { return }`, then hand `code` to the UI thread through ONE `ui_hop::<i32>` created on the UI thread (its sender lives in an
    /// `Arc<Mutex<Option<UiHopSender<i32>>>>`, taken by `close_once`; `Err(HopLost)` is ignored). The state is `!Send` (its `forced_exit` is a `Cell`): `close_once` may run on the timeout WORKER and
    /// must not touch it; the CONTINUATION (UI thread, captures `Weak<StartupState>` and `Weak<MainWindow>`) does `forced_exit.set(Some(code))` FIRST, then, when the window is not destroyed,
    /// `close_secondary_windows()` and `main.window().dispose()`, then `MessageLoop::quit(code)`.
    /// The trigger is `smoke_trigger(launch.initial_path())`: `FirstPresented` -> `main.subscribe_first_presented(close_once(0))` and, when `main.first_present_done()` is already true, call it now;
    /// `FirstImagePresented` -> `main.subscribe_first_image_presented(close_once(0))`. The timeout is one thread (`PhotoReview.SmokeTimeout`) that waits on a `WindowCancel` for the limit and calls
    /// `close_once(EXIT_SMOKE_TIMEOUT)` unless cancelled; `window.subscribe(ShellWindowEvent::Closed, ..)` cancels it (C# `timeout.Dispose()`). No polling, no sleep.
    pub(super) fn arm_smoke_exit(self: &Rc<Self>, main: &Rc<MainWindow>) {
        todo!()
    }
}
