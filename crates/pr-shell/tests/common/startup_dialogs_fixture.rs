//! Temp-root shell startup fixture for W8-17c native integration checks.
//! The dispatcher creates a private hidden Win32 message window used as the owner; a STATIC HWND cannot be created here because this crate must not contain unsafe FFI.
use pr_app::coordinators::zoom_detail::ThreadRunner;
use pr_app::presenters::benchmark::BenchmarkSessionFactory;
use pr_core::abstractions::{Log, NullLog, ShellIntegration, ShellMenuState};
use pr_core::settings::app_settings::EnglishSettingsTexts;
use pr_platform_win::host::message_loop::MessageLoop;
use pr_platform_win::host::ui_dispatcher::Win32UiDispatcher;
use pr_platform_win::shell_viewing::presentation_sink::ShellPresentationHub;
use pr_platform_win::win32::structs::Hwnd;
use pr_shell::startup::composition_root::{ShellHostServices, ShellServices, ShellServicesOptions};
use pr_shell::startup::secondary_dialogs::DialogServices;
use pr_test_support::{FakeFileSystem, TempRoot};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug)]
struct NoIntegration;
impl ShellIntegration for NoIntegration {
    fn get_state(&self, _: &str) -> ShellMenuState {
        ShellMenuState::NotRegistered
    }
    fn register(&self, _: &str) -> std::io::Result<()> {
        Ok(())
    }
    fn unregister(&self) -> std::io::Result<()> {
        Ok(())
    }
}
pub struct StartupDialogsFixture {
    pub root: TempRoot,
    pub message_loop: Rc<MessageLoop>,
    pub owner: Hwnd,
    pub services: ShellServices,
}
impl StartupDialogsFixture {
    pub fn new() -> Self {
        let root = TempRoot::new("startup-dialogs");
        let log: Arc<dyn Log> = Arc::new(NullLog);
        let dispatcher = Win32UiDispatcher::new(log.clone()).expect("native UI dispatcher");
        let message_loop = Rc::new(MessageLoop::new(Rc::new(dispatcher.clone())).expect("shell message loop"));
        let owner = dispatcher.message_window();
        let owner_for_closure = owner;
        let services = ShellServices::build(
            ShellHostServices {
                dispatcher: dispatcher.clone(),
                owner_window: Rc::new(move || owner_for_closure),
                message_loop: Some(message_loop.clone()),
            },
            Rc::new(ShellPresentationHub::new()),
            log,
            ShellServicesOptions::default(),
        );
        let benchmark_sessions: BenchmarkSessionFactory =
            Arc::new(|_, _| unreachable!("test benchmark seam is not called"));
        let graph = Rc::new(DialogServices {
            settings_store: None,
            localization: None,
            update_checker: None,
            decoder_factory: None,
            journal: None,
            recovery_retry: None,
            file_system: Arc::new(FakeFileSystem::new()),
            metrics: None,
            explorer_snapshot: None,
            settings_texts: Arc::new(EnglishSettingsTexts),
            shell_integration: Arc::new(NoIntegration),
            physical_memory_bytes: 0,
            runner: Arc::new(ThreadRunner),
            benchmark_sessions,
        });
        assert!(services.bind_dialog_services(graph));
        Self {
            root,
            message_loop,
            owner,
            services,
        }
    }
}
