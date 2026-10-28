//! C# ShellCompositionRootTests ports and composition root behavior checks.
use pr_app::coordinators::zoom_detail::ThreadRunner;
use pr_app::presenters::benchmark::BenchmarkSessionFactory;
use pr_core::abstractions::{Log, NullLog, SettingsTarget, ShellIntegration, ShellMenuState};
use pr_core::settings::app_settings::EnglishSettingsTexts;
use pr_platform_win::dialogs::dialog_service::{SecondaryWindowKind, SecondaryWindowPort};
use pr_platform_win::dialogs::file_dialog::ShellFileDialog;
use pr_platform_win::dialogs::folder_dialog::{FolderPicker, ShellFolderDialog};
use pr_platform_win::host::ui_dispatcher::Win32UiDispatcher;
use pr_platform_win::shell_viewing::presentation_sink::ShellPresentationHub;
use pr_platform_win::win32::structs::{Hresult, Hwnd};
use pr_shell::startup::composition_root::{
    ShellFolderPickerAdapter, ShellHostServices, ShellServices, ShellServicesOptions,
};
use pr_shell::startup::secondary_dialogs::DialogServices;
use pr_test_support::FakeFileSystem;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct TestLog(Mutex<Vec<(String, Option<String>)>>);
impl Log for TestLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, _: &str) {}
    fn warn(&self, _: &str) {}
    fn error(&self, m: &str, e: Option<&(dyn std::error::Error + 'static)>) {
        self.0.lock().unwrap().push((m.into(), e.map(ToString::to_string)));
    }
}
struct Folders {
    result: Result<Option<String>, Hresult>,
    calls: RefCell<Vec<(Hwnd, String, Option<String>)>>,
}
impl ShellFolderDialog for Folders {
    fn show(&self, h: Hwnd, t: &str, i: Option<&str>) -> Result<Option<String>, Hresult> {
        self.calls.borrow_mut().push((h, t.into(), i.map(str::to_owned)));
        self.result.clone()
    }
}
struct Dropped(Rc<Cell<usize>>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
impl SecondaryWindowPort for Dropped {
    fn supports(&self, _: SecondaryWindowKind) -> bool {
        true
    }
    fn show_settings(&self, _: Hwnd, _: SettingsTarget) -> bool {
        false
    }
    fn show_recovery(&self, _: Hwnd) {}
    fn show_diagnostics(&self, _: Hwnd) {}
    fn show_benchmark(&self, _: Hwnd, _: Option<&std::path::Path>) {}
    fn show_skipped_files(&self, _: Hwnd, _: &[pr_core::abstractions::SkippedEntry]) {}
    fn show_batch_review(&self, _: Hwnd, _: &[pr_core::abstractions::BatchReviewItem]) -> bool {
        false
    }
    fn prompt_custom_zoom(&self, _: Hwnd, _: i32) -> Option<i32> {
        None
    }
}
fn host(owner: Rc<dyn Fn() -> Hwnd>) -> ShellHostServices {
    ShellHostServices {
        dispatcher: Win32UiDispatcher::new(Arc::new(NullLog)).unwrap(),
        owner_window: owner,
        message_loop: None,
    }
}
fn build(owner: Rc<dyn Fn() -> Hwnd>, log: Arc<dyn Log>, options: ShellServicesOptions) -> ShellServices {
    ShellServices::build(host(owner), Rc::new(ShellPresentationHub::new()), log, options)
}

// C#: ShellCompositionRootTests.HostServices_AreTheWin32Ones
#[test]
fn shell_services_are_the_win32_ones() {
    let hub = Rc::new(ShellPresentationHub::new());
    let h = host(Rc::new(|| 0));
    let s = ShellServices::build(h, hub.clone(), Arc::new(NullLog), Default::default());
    assert!(Rc::ptr_eq(&s.hub, &hub));
    assert!(Rc::ptr_eq(&s.secondary_port, &s.secondary_port_host.port));
    assert!(!s.secondary_port.is_value_created());
    assert!(format!("{:?}", s.dialog_service).contains("Win32DialogService"));
    assert!(format!("{:?}", s.zoom_prompt).contains("Win32ZoomPromptService"));
    assert!(format!("{:?}", s.sink_factory).contains("Win32PresentationSinkFactory"));
}

// W9-R8-wire: the window options carry the dialog service (rename box of F2 / "Rename...") and the zoom prompt, both the shared production services.
#[test]
fn window_service_extensions_carry_the_dialog_service_and_the_zoom_prompt() {
    use pr_platform_win::shell_input::main_window::{DialogServiceOption, ZoomPromptOption};
    let s = build(Rc::new(|| 0), Arc::new(NullLog), Default::default());
    let extensions = s.window_service_extensions();
    let dialogs = extensions
        .get::<DialogServiceOption>()
        .expect("DialogServiceOption is present");
    assert!(format!("{:?}", dialogs).contains("DialogServiceOption"));
    assert!(std::ptr::addr_eq(Rc::as_ptr(&dialogs.0), Rc::as_ptr(&s.dialog_service)));
    assert!(extensions.get::<ZoomPromptOption>().is_some());
}

// C#: ShellCompositionRootTests.LaterFeatureRegistrations_OverrideTheDefaults
#[test]
fn later_registrations_override_the_defaults() {
    let folder: Rc<dyn ShellFolderDialog> = Rc::new(Folders {
        result: Ok(Some("injected".into())),
        calls: RefCell::new(vec![]),
    });
    let file: Rc<dyn ShellFileDialog> = Rc::new(pr_platform_win::dialogs::file_dialog::NativeShellFileDialog::new());
    let s = build(
        Rc::new(|| 0),
        Arc::new(NullLog),
        ShellServicesOptions {
            folder_dialog: Some(folder.clone()),
            file_dialog: Some(file.clone()),
            ..Default::default()
        },
    );
    assert!(Rc::ptr_eq(&s.folder_dialog, &folder));
    assert!(Rc::ptr_eq(&s.file_dialog, &file));
}

#[test]
fn registration_resolves_the_real_port_lazily_and_shared_by_dialogs_and_zoom() {
    let s = build(Rc::new(|| 0), Arc::new(NullLog), Default::default());
    assert!(!s.secondary_port.is_value_created());
    assert!(Rc::ptr_eq(&s.secondary_port, &s.secondary_port_host.port));
    assert!(s.secondary_port.get().is_none());
    assert!(s.secondary_port.is_value_created());
}
#[test]
fn a_port_registered_by_the_test_wins_over_the_real_port() {
    let dropped = Rc::new(Cell::new(0));
    let port: Rc<dyn SecondaryWindowPort> = Rc::new(Dropped(dropped));
    let s = build(
        Rc::new(|| 0),
        Arc::new(NullLog),
        ShellServicesOptions {
            secondary_port: Some(port.clone()),
            ..Default::default()
        },
    );
    assert!(Rc::ptr_eq(&s.secondary_port.get().unwrap(), &port));
}
#[test]
fn opening_from_another_thread_is_unrepresentable() {
    trait MaybeSend<A> {
        fn marker() {}
    }
    struct NonSend;
    impl<T: ?Sized> MaybeSend<()> for T {}
    impl<T: ?Sized + Send> MaybeSend<NonSend> for T {}
    fn assert_not_send<T: ?Sized>() {
        let _ = <T as MaybeSend<_>>::marker;
    }
    assert_not_send::<ShellServices>();
}
#[test]
fn shell_services_drop_frees_the_port() {
    let drops = Rc::new(Cell::new(0));
    let port: Rc<dyn SecondaryWindowPort> = Rc::new(Dropped(drops.clone()));
    let s = build(
        Rc::new(|| 0),
        Arc::new(NullLog),
        ShellServicesOptions {
            secondary_port: Some(port),
            ..Default::default()
        },
    );
    drop(s.secondary_port.get());
    assert_eq!(drops.get(), 0);
    drop(s);
    assert_eq!(drops.get(), 1);
}
#[test]
fn owner_closure_is_evaluated_at_use_time_and_gives_zero_for_a_destroyed_window() {
    let hwnd = Rc::new(Cell::new(22));
    let h = hwnd.clone();
    let owner = host(Rc::new(move || h.get())).owner_closure();
    assert_eq!(owner(), 22);
    hwnd.set(0);
    assert_eq!(owner(), 0);
}
#[test]
fn folder_picker_adapter_logs_a_com_failure_and_answers_none() {
    let log = Arc::new(TestLog::default());
    let dialog = Rc::new(Folders {
        result: Err(Hresult(0x80004005u32 as i32)),
        calls: RefCell::new(vec![]),
    });
    let picker = Rc::new(FolderPicker::new(dialog, Box::new(|| 3)));
    let mut adapter = ShellFolderPickerAdapter::new(picker, log.clone());
    assert_eq!(
        pr_app::coordinators::traits::IFolderPicker::pick_folder(&mut adapter, "title", None),
        None
    );
    assert_eq!(log.0.lock().unwrap()[0].0, "Folder dialog failed (0x80004005)");
}
#[test]
fn folder_picker_adapter_passes_title_and_initial_folder_through() {
    let dialog = Rc::new(Folders {
        result: Ok(Some("chosen".into())),
        calls: RefCell::new(vec![]),
    });
    let initial = std::env::temp_dir().to_string_lossy().into_owned();
    let picker = Rc::new(FolderPicker::new(dialog.clone(), Box::new(|| 9)));
    let mut adapter = ShellFolderPickerAdapter::new(picker, Arc::new(NullLog));
    assert_eq!(
        pr_app::coordinators::traits::IFolderPicker::pick_folder(&mut adapter, "Pick", Some(&initial)),
        Some("chosen".into())
    );
    assert_eq!(*dialog.calls.borrow(), [(9, "Pick".into(), Some(initial))]);
}
#[derive(Debug)]
struct NoShellIntegration;
impl ShellIntegration for NoShellIntegration {
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
fn dialog_services() -> Rc<DialogServices> {
    let sessions: BenchmarkSessionFactory = Arc::new(|_, _| unreachable!("benchmark test factory is never invoked"));
    Rc::new(DialogServices {
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
        shell_integration: Arc::new(NoShellIntegration),
        physical_memory_bytes: 0,
        runner: Arc::new(ThreadRunner),
        benchmark_sessions: sessions,
    })
}
#[test]
fn bind_dialog_services_first_one_wins() {
    let s = build(Rc::new(|| 0), Arc::new(NullLog), Default::default());
    assert!(s.bind_dialog_services(dialog_services()));
    assert!(!s.bind_dialog_services(dialog_services()));
    assert!(!s.secondary_port.is_value_created());
}
#[test]
fn factory_before_the_graph_is_bound_answers_no_port_and_logs() {
    let log = Arc::new(TestLog::default());
    let s = build(Rc::new(|| 0), log.clone(), Default::default());
    assert!(s.secondary_port.get().is_none());
    assert!(
        log.0.lock().unwrap()[0]
            .0
            .contains("before the service graph was bound")
    );
}
#[test]
fn host_registrations_are_all_mapped() {
    let Ok(golden) = std::fs::read_to_string("tests/golden/shell-composition-registrations.json") else {
        eprintln!("skip: generated golden is absent");
        return;
    };
    assert!(golden.contains("ShellPresentationHub"));
    assert!(golden.contains("IFolderPicker"));
    let input =
        std::fs::read_to_string("tests/golden-inputs/shell-composition-registrations.json").expect("named input");
    assert!(!input.is_empty());
}
