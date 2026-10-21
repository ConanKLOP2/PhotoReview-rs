//! Native (ignored) tests: a real `ShellWindow` with hidden state only (never shown, never activated), and the G-INPUT replay.
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::input::key_id::KeyId;
use pr_app::input::key_mapping;
use pr_app::input::review_command::ReviewCommand;
use pr_app::viewmodels::main::ViewerHandle;
use pr_app::viewmodels::viewer_state::ViewerState;
use pr_core::abstractions::UiDispatcher;
use pr_core::settings::app_settings::AppSettings;
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::host::frame_clock::base::HostFrameClock;
use pr_platform_win::host::shell_window::ShellWindowOptions;
use pr_platform_win::shell_input::dispatch::{InputSurface, ShellInput, ShellInputDeps, ShellInputHost};
use pr_platform_win::shell_input::environment::ShellInputEnvironment;
use pr_platform_win::shell_viewing::viewport_controller::{ViewportController, ViewportControllerOptions};
use pr_platform_win::win32::messages::WM_KEYDOWN;
use pr_test_support::{RecordingLog, golden};

use crate::fixture::{QueueDispatcher, RecordingHost, RigContent, RigPoster, RigSurface};
use crate::shell_window_fixture::{clock, create_window, dispose_window, send, window};
use crate::ui_thread::UiThread;

// new: the three handlers are registered on a REAL window and the keyboard handler answers a `WM_KEYDOWN` sent to the HWND (SendMessage, no foreground API).
#[test]
#[ignore = "native: needs desktop"]
fn shell_input_registers_three_handlers_in_order_on_a_real_window() {
    let ui = UiThread::start();
    let hwnd = ui.invoke(|| create_window(ShellWindowOptions::default()).hwnd());
    let commands = ui.invoke(move || {
        let real = window();
        let host = RecordingHost::new();
        let viewer = ViewerHandle::new(ViewerState::new());
        let content = RigContent::new(&viewer, (3000.0, 2000.0));
        let dispatcher = QueueDispatcher::new();
        let viewport = ViewportController::new(
            Rc::clone(&real) as Rc<dyn ShellWindowApi>,
            Rc::new(RigPoster(Arc::clone(&dispatcher))),
            clock() as Rc<dyn HostFrameClock>,
            {
                let content = Rc::clone(&content);
                Rc::new(move || content.current())
            },
            Rc::new(|| {}),
            ViewportControllerOptions::default(),
        );
        let surface = Rc::new(RefCell::new(RigSurface::new(viewport, viewer.clone(), content)));
        let input = ShellInput::new(ShellInputDeps {
            window: Rc::clone(&real) as Rc<dyn ShellWindowApi>,
            surface: surface as Rc<RefCell<dyn InputSurface>>,
            viewer,
            settings: Rc::new(|| Arc::new(AppSettings::default())),
            host: Rc::clone(&host) as Rc<dyn ShellInputHost>,
            dispatcher: dispatcher as Arc<dyn UiDispatcher>,
            environment: Rc::new(ShellInputEnvironment::default()),
            log: Arc::new(RecordingLog::new()),
            ticks_per_ms: 10_000.0,
        });
        // Right = the default "Next" shortcut; the key handler is the first of the three.
        let vk = key_mapping::to_virtual_key(KeyId::RIGHT);
        let result = send(hwnd, WM_KEYDOWN, vk as usize, 1);
        input.dispose();
        (host.commands_snapshot(), result)
    });
    assert_eq!(vec![ReviewCommand::NEXT], commands.0);
    assert_eq!(0, commands.1, "a handled key answers 0");
    ui.invoke(dispose_window);
}

// G-INPUT: replays every recorded script through real messages. The golden `input-scripts` is not synced into `tests/golden` yet (lead action:
// `tools/sync-from-csharp.ps1`), and the C# replay engine (`InputGoldenReplay`) has no Rust twin before it: the test skips with a message.
#[test]
#[ignore = "native: needs desktop"]
fn g_input_every_recorded_script_replays_through_real_messages() {
    let path = golden::repo_root()
        .join("tests")
        .join("golden")
        .join("input-scripts.json");
    if !path.is_file() {
        eprintln!(
            "SKIPPED: tests/golden/input-scripts.json is not synced yet (tools/sync-from-csharp.ps1); G-INPUT replay not run"
        );
        return;
    }
    panic!(
        "tests/golden/input-scripts.json exists now: port InputGoldenReplay (C# tests/PhotoReview.Shell.Tests) and replace this panic"
    );
}
