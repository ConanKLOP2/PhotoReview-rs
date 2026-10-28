//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 463-502 (`ArmSmokeExit`, baseline `d92d6aae`): the window side of the `--smoke` run. Unit W8-17e.
//! Contract: `contracts/W8-17e.md`. The latch and the trigger choice are `pr_app::startup::smoke`.

use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};

use pr_app::startup::options::EXIT_SMOKE_TIMEOUT;
use pr_app::startup::smoke::{SmokeLatch, SmokeTrigger, smoke_trigger};
use pr_app::startup::ui_hop::{UiHopSender, ui_hop};
use pr_core::instance::coalescer::WindowCancel;
use pr_platform_win::host::contracts::{ShellWindowApi, ShellWindowEvent};
use pr_platform_win::host::main_window::MainWindow;
use pr_platform_win::host::message_loop::MessageLoop;

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
    #[allow(
        dead_code,
        reason = "called by create_main_window (window.rs, lane shell-a) once it lands"
    )]
    pub(super) fn arm_smoke_exit(self: &Rc<Self>, main: &Rc<MainWindow>) {
        let latch = Arc::new(SmokeLatch::new());
        let slot: Arc<Mutex<Option<UiHopSender<i32>>>> = Arc::new(Mutex::new(None));
        let weak_state = Rc::downgrade(self);
        let weak_main = Rc::downgrade(main);
        // Created here, on the UI thread; the continuation runs on the UI thread too.
        let sender = ui_hop::<i32>(self.ui.clone(), move |result| {
            let Ok(code) = result else {
                return;
            };
            let Some(state) = weak_state.upgrade() else {
                return;
            };
            state.forced_exit.set(Some(code));
            if let Some(main) = weak_main.upgrade() {
                if !main.window().is_destroyed() {
                    state.close_secondary_windows();
                    main.window().dispose();
                }
            }
            MessageLoop::quit(code);
        });
        *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);

        let close_once: Arc<dyn Fn(i32) + Send + Sync> = Arc::new(move |code| {
            if !latch.try_close() {
                return;
            }
            let sender = slot.lock().unwrap_or_else(PoisonError::into_inner).take();
            if let Some(sender) = sender {
                sender.send(code);
            }
        });

        match smoke_trigger(self.launch.initial_path()) {
            SmokeTrigger::FirstPresented => {
                let close = close_once.clone();
                main.subscribe_first_presented(Box::new(move || close(0)));
                if main.first_present_done() {
                    close_once(0);
                }
            }
            SmokeTrigger::FirstImagePresented => {
                let close = close_once.clone();
                main.subscribe_first_image_presented(Box::new(move || close(0)));
            }
        }

        let limit = self.options.borrow().smoke_timeout;
        let cancel = Arc::new(WindowCancel::new());
        let waiter_cancel = cancel.clone();
        let timeout_close = close_once;
        let _timeout = std::thread::Builder::new()
            .name("PhotoReview.SmokeTimeout".into())
            .spawn(move || {
                if !waiter_cancel.wait_timeout(limit) {
                    timeout_close(EXIT_SMOKE_TIMEOUT);
                }
            });
        main.window()
            .subscribe(ShellWindowEvent::Closed, Box::new(move || cancel.cancel()));
    }
}
