//! Pure production SecondaryWindowDialogs builder/result tests.
use pr_app::coordinators::zoom_detail::ThreadRunner;
use pr_app::presenters::benchmark::BenchmarkSessionFactory;
use pr_app::services::contracts::ClipboardService;
use pr_core::abstractions::{NullLog, ShellIntegration, ShellMenuState};
use pr_core::settings::app_settings::EnglishSettingsTexts;
use pr_platform_win::dialogs::file_dialog::NativeShellFileDialog;
use pr_platform_win::dialogs::folder_dialog::NativeShellFolderDialog;
use pr_platform_win::dialogs::secondary_window_port::SecondaryWindowDialogs;
use pr_platform_win::shell_startup::locale::{
    WindowsNumberFormatting, WindowsRecoveryTimeFormatter, group_separator, local_long_time_text,
};
use pr_shell::startup::secondary_dialogs::{
    BenchmarkDialogSeams, DialogFactorySeams, DialogHostParts, DialogServices, ProductionSecondaryWindowDialogs,
};
use pr_test_support::FakeFileSystem;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

struct Clip(RefCell<Vec<String>>);
impl ClipboardService for Clip {
    fn try_set_text(&self, s: &str) -> bool {
        self.0.borrow_mut().push(s.into());
        true
    }
}
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
fn services() -> Rc<DialogServices> {
    let sessions: BenchmarkSessionFactory = Arc::new(|_, _| unreachable!("factory is not invoked"));
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
        shell_integration: Arc::new(NoIntegration),
        physical_memory_bytes: 0,
        runner: Arc::new(ThreadRunner),
        benchmark_sessions: sessions,
    })
}
fn host(clipboard: Rc<dyn ClipboardService>) -> DialogHostParts {
    DialogHostParts {
        clipboard,
        file_dialog: Rc::new(NativeShellFileDialog::new()),
        folder_dialog: Rc::new(NativeShellFolderDialog::new()),
        log: Arc::new(NullLog),
        version: None,
        localizer: Box::new(pr_l10n::localizer::Localizer::current),
        numbers: Rc::new(WindowsNumberFormatting),
        time: Rc::new(WindowsRecoveryTimeFormatter),
        group_separator: group_separator(),
        current_time_text: Arc::new(local_long_time_text),
    }
}
fn factory() -> ProductionSecondaryWindowDialogs {
    ProductionSecondaryWindowDialogs::new(
        services(),
        host(Rc::new(Clip(RefCell::new(vec![])))),
        DialogFactorySeams::default(),
    )
}

#[test]
fn benchmark_dialog_seams_have_a_default_and_clone() {
    let seams = BenchmarkDialogSeams::default();
    let cloned = seams.clone();
    assert!(
        cloned.sessions.is_none()
            && cloned.scan.is_none()
            && cloned.save.is_none()
            && cloned.pick_folder.is_none()
            && cloned.open_in_explorer.is_none()
    );
}
#[test]
fn dialog_factory_seams_have_a_default() {
    let seams = DialogFactorySeams::default();
    assert!(
        seams.chrome.is_none()
            && seams.warnings.is_none()
            && seams.recovery_messages.is_none()
            && seams.recovery_explorer.is_none()
            && !seams.recovery_no_auto_check
    );
}
#[test]
fn port_show_recovery_without_a_journal_opens_nothing() {
    assert!(factory().recovery(0).is_none());
}
#[test]
fn settings_without_a_store_is_none() {
    assert!(
        factory()
            .settings(pr_core::abstractions::SettingsTarget::Default, Rc::new(|_, _| None))
            .is_none()
    );
}

#[test]
#[ignore = "blocked: W8-18q: ActionProfilesDialog::new is still todo!() on master"]
fn action_profiles_finish_returns_list_only_for_ok() {
    let prepared = factory().action_profiles(&[]).expect("builder always returns a dialog");
    assert_eq!((prepared.finish)(1), Some(vec![]));
    let prepared = factory().action_profiles(&[]).unwrap();
    assert_eq!((prepared.finish)(2), None);
}
#[test]
#[ignore = "blocked: W8-18e/18h..18k: NativeDialog core and ListDialog are still todo!() on master"]
fn batch_review_finish_is_true_only_for_ok() {
    let prepared = factory().batch_review(&[]).unwrap();
    assert!((prepared.finish)(1));
    let prepared = factory().batch_review(&[]).unwrap();
    assert!(!(prepared.finish)(2));
}
#[test]
#[ignore = "blocked: W8-18p: ZoomPromptDialog::new is still todo!() on master"]
fn custom_zoom_finish_returns_value_only_for_ok() {
    let prepared = factory().custom_zoom(125).unwrap();
    assert_eq!((prepared.finish)(1), None);
    let prepared = factory().custom_zoom(125).unwrap();
    assert_eq!((prepared.finish)(2), None);
}
#[test]
#[ignore = "blocked: W8-18e/18h..18k/18r: NativeDialog core and DiagnosticsDialog are still todo!() on master"]
fn skipped_files_and_diagnostics_finish_return_unit() {
    let prepared = factory().skipped_files(&[]).unwrap();
    assert_eq!((prepared.finish)(2), ());
    let prepared = factory().diagnostics().unwrap();
    assert_eq!((prepared.finish)(2), ());
}
