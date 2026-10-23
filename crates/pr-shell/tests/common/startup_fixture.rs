//! W8-17d bounded UI-thread fixture; extended by 17e for the full window/run chain.
use pr_app::startup::{forwarded_open::ForwardedOpenRouter, launch::ShellLaunch};
use pr_core::abstractions::NullLog;
use pr_platform_win::host::{message_loop::MessageLoop, ui_dispatcher::Win32UiDispatcher};
use pr_platform_win::shell_viewing::presentation_sink::ShellPresentationHub;
use pr_shell::startup::{options::ShellStartupOptions, state::StartupState};
use pr_test_support::{FakeFileSystem, TempRoot};
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

pub fn on_ui(body: impl FnOnce() + Send + 'static) {
    let (tx, rx) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
        tx.send(result).unwrap();
    });
    let result = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("bounded startup UI thread");
    thread.join().unwrap();
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}

#[allow(dead_code)] // The instance-gate test reuses only on_ui; 17e extends this shared fixture.
pub struct StartupFixture {
    pub root: TempRoot,
    pub state: Rc<StartupState>,
    dispatcher: Win32UiDispatcher,
}
#[allow(dead_code)] // Not every startup test needs a dispatcher/state fixture.
impl StartupFixture {
    pub fn new(options: ShellStartupOptions) -> Self {
        let root = TempRoot::new("startup-phase-one");
        let dispatcher = Win32UiDispatcher::new(Arc::new(NullLog)).unwrap();
        let message_loop = Rc::new(MessageLoop::new(Rc::new(dispatcher.clone())).unwrap());
        let launch = ShellLaunch::parse(&["--smoke"], &FakeFileSystem::new(), &NullLog);
        let state = StartupState::new(
            vec!["--smoke".into()],
            launch,
            options,
            Arc::new(|_| {}),
            Instant::now(),
            dispatcher.clone(),
            message_loop,
            Rc::new(ShellPresentationHub::new()),
            ForwardedOpenRouter::new(),
        );
        Self {
            root,
            state,
            dispatcher,
        }
    }
}
impl Drop for StartupFixture {
    fn drop(&mut self) {
        self.dispatcher.shutdown();
    }
}
