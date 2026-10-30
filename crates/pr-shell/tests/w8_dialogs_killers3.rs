//! Pure constructor/result guards for secondary dialogs; no native window is shown.
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
fn factory_debug_keeps_each_service_type_name() {
    // secondary_dialogs.rs:92,122,143,166,179: replace Debug::fmt with Ok(Default).
    assert_eq!(format!("{:?}", services()), "DialogServices { .. }");
    assert_eq!(
        format!("{:?}", host(Rc::new(Clip(RefCell::new(vec![]))))),
        "DialogHostParts { .. }"
    );
    assert_eq!(
        format!("{:?}", BenchmarkDialogSeams::default()),
        "BenchmarkDialogSeams { .. }"
    );
    assert_eq!(
        format!("{:?}", DialogFactorySeams::default()),
        "DialogFactorySeams { .. }"
    );
    assert_eq!(format!("{:?}", factory()), "ProductionSecondaryWindowDialogs { .. }");
}

#[test]
fn prepared_results_preserve_actions_and_accept_only_ok() {
    // secondary_dialogs.rs:328,337,383,386: None replacement and == -> !=.
    // C#: Win32SecondaryWindowPort.ShowActionProfiles / ShowBatchReview.
    let factory = factory();
    for id in [0, 1, 2, -1] {
        let prepared = factory.action_profiles(&[]).unwrap();
        assert_eq!((prepared.finish)(id), (id == 1).then(Vec::new));
        let prepared = factory.batch_review(&[]).unwrap();
        assert_eq!((prepared.finish)(id), id == 1);
    }
}

#[test]
fn optional_services_and_windowless_builders_keep_their_contract() {
    // secondary_dialogs.rs:201,242,295,343,371,391: Option return replacements.
    // C#: Win32SecondaryWindowPort builder bodies; no modal is entered.
    let factory = factory();
    assert!(
        factory
            .settings(pr_core::abstractions::SettingsTarget::Default, Rc::new(|_, _| None))
            .is_none()
    );
    assert!(factory.recovery(0).is_none());
    assert_eq!(factory.benchmark(None).unwrap().hwnd(), 0);
    assert!(factory.diagnostics().is_some());
    assert!(factory.skipped_files(&[]).is_some());
    for id in [1, 2] {
        let zoom = factory.custom_zoom(125).unwrap();
        assert_eq!((zoom.finish)(id), None);
    }
}

#[test]
fn recovery_follow_up_preserves_error_precedence_and_stale_count() {
    use pr_app::presenters::recovery_window::RecoveryDismissResult;
    use pr_platform_win::dialogs::recovery::dismiss::{DismissFollowUp, follow_up};
    // recovery/dismiss.rs:26-38: return replacement / delete ! / skipped count replacement.
    // C#: RecoveryDialog.Dismiss.
    for has_window in [false, true] {
        let error = RecoveryDismissResult {
            error: Some("failure-marker".into()),
            skipped_count: 7,
        };
        assert_eq!(
            follow_up(&error, has_window),
            DismissFollowUp::ShowError("failure-marker".into())
        );
    }
    for skipped in [0, 1, 7] {
        let ok = RecoveryDismissResult {
            error: None,
            skipped_count: skipped,
        };
        assert_eq!(follow_up(&ok, false), DismissFollowUp::Stop);
        assert_eq!(follow_up(&ok, true), DismissFollowUp::Refresh { skipped });
    }
}

#[test]
fn recovery_retry_notice_close_precedes_recheck() {
    use pr_app::presenters::recovery_window::RecoveryRetryNotice;
    use pr_platform_win::dialogs::recovery::restore::{AfterNotice, after_notice, cursor_id};
    // recovery/restore.rs:59,70: return replacement and branch deletion.
    // C#: RecoveryDialog.RetryAsync / SetBusy.
    for (close_window, recheck, expected) in [
        (true, true, AfterNotice::Close),
        (true, false, AfterNotice::Close),
        (false, true, AfterNotice::Recheck),
        (false, false, AfterNotice::Nothing),
    ] {
        assert_eq!(
            after_notice(&RecoveryRetryNotice {
                title: String::new(),
                message: String::new(),
                is_warning: false,
                close_window,
                recheck
            }),
            expected
        );
    }
    assert_eq!(cursor_id(false), 32512);
    assert_eq!(cursor_id(true), 32514);
}

#[test]
fn recovery_selection_carries_identity_and_top_index_has_exclusive_upper_bound() {
    use pr_platform_win::dialogs::recovery::list::{carried_selection, top_index_to_restore};
    use std::collections::HashSet;
    // recovery/list.rs:76-89: return replacement, > / < boundaries and && -> ||.
    // C#: RecoveryDialog.RefreshEntries.
    let selected = HashSet::from(["removed".to_owned(), "keep".to_owned()]);
    let visible = ["new", "keep", "other", "keep"].map(str::to_owned);
    assert_eq!(carried_selection(&selected, &visible), [1, 3]);
    assert!(carried_selection(&HashSet::new(), &visible).is_empty());
    assert!(carried_selection(&selected, &[]).is_empty());
    for (top, count, expected) in [
        (-1, 5, None),
        (0, 5, None),
        (1, 5, Some(1)),
        (4, 5, Some(4)),
        (5, 5, None),
        (6, 5, None),
        (1, 0, None),
    ] {
        assert_eq!(top_index_to_restore(top, count), expected);
    }
}

#[test]
fn provided_store_and_journal_build_unshown_dialogs() {
    use pr_core::app_paths::AppPaths;
    use pr_core::file_actions::journal::OperationJournal;
    use pr_core::settings::store::SettingsStore;
    use pr_test_support::{FakeClock, TempRoot};
    // secondary_dialogs.rs:201,242: replace factory result with None.
    // C#: Win32SecondaryWindowPort.ShowSettings / ShowRecovery.
    let root = TempRoot::new("cx-dlgs-services");
    let paths = AppPaths::new(root.path(), Some(root.path().to_str().unwrap()), true);
    let fs = Arc::new(FakeFileSystem::new());
    let clock = Arc::new(FakeClock::new());
    let mut services = services();
    let mutable = Rc::get_mut(&mut services).unwrap();
    mutable.settings_store = Some(Arc::new(SettingsStore::new(
        paths.clone(),
        fs.clone(),
        Arc::new(NullLog),
        clock.clone(),
        Arc::new(EnglishSettingsTexts),
    )));
    mutable.journal = Some(Arc::new(OperationJournal::new(&paths, fs, clock)));
    let factory = ProductionSecondaryWindowDialogs::new(
        services,
        host(Rc::new(Clip(RefCell::new(vec![])))),
        DialogFactorySeams {
            recovery_no_auto_check: true,
            ..Default::default()
        },
    );
    let settings = factory
        .settings(pr_core::abstractions::SettingsTarget::Default, Rc::new(|_, _| None))
        .unwrap();
    assert_eq!(settings.dialog.hwnd(), 0);
    assert!(!(settings.finish)(2));
    let recovery = factory.recovery(0).unwrap();
    assert_eq!(recovery.dialog.hwnd(), 0);
    (recovery.finish)(2);
}

#[test]
fn benchmark_sort_toggles_only_the_current_ascending_column() {
    use pr_platform_win::dialogs::benchmark::state::{compare_f64_dotnet, next_sort};
    use std::cmp::Ordering;
    // benchmark/state.rs:139-145,197: constant return / == -> != / && -> ||.
    // C#: BenchmarkDialog.CompareRows / SortBy.
    for current_column in [0, 1, 4] {
        for direction in [0, 1, 2] {
            for clicked in [0, 1, 4] {
                let expected = if current_column == clicked && direction == 1 {
                    2
                } else {
                    1
                };
                assert_eq!(next_sort((current_column, direction), clicked), (clicked, expected));
            }
        }
    }
    for (a, b, expected) in [
        (f64::NAN, f64::NAN, Ordering::Equal),
        (f64::NAN, 0.0, Ordering::Less),
        (0.0, f64::NAN, Ordering::Greater),
        (-0.0, 0.0, Ordering::Equal),
        (-1.0, 3.0, Ordering::Less),
        (3.0, -1.0, Ordering::Greater),
        (f64::INFINITY, 4.0, Ordering::Greater),
    ] {
        assert_eq!(compare_f64_dotnet(a, b), expected);
    }
}
mod router_rename {
    use std::path::Path;
    use std::rc::Rc;

    use pr_core::abstractions::RenamePromptRequest;
    use pr_core::abstractions::{BatchReviewItem, SettingsTarget, SkippedEntry};
    use pr_platform_win::dialogs::dialog_service::{SecondaryWindowKind, SecondaryWindowPort};
    use pr_platform_win::dialogs::secondary_window_port::SecondaryWindowRouter;
    use std::sync::Arc;

    struct Native {
        supported: bool,
    }

    impl SecondaryWindowPort for Native {
        fn show_settings(&self, _: isize, _: SettingsTarget) -> bool {
            false
        }
        fn show_recovery(&self, _: isize) {}
        fn show_diagnostics(&self, _: isize) {}
        fn show_benchmark(&self, _: isize, _: Option<&Path>) {}
        fn show_skipped_files(&self, _: isize, _: &[SkippedEntry]) {}
        fn show_batch_review(&self, _: isize, _: &[BatchReviewItem]) -> bool {
            false
        }
        fn prompt_custom_zoom(&self, _: isize, _: i32) -> Option<i32> {
            None
        }
        fn supports(&self, _: SecondaryWindowKind) -> bool {
            self.supported
        }
        fn prompt_rename(&self, owner: isize, request: &RenamePromptRequest) -> Option<String> {
            assert_eq!(owner, 73);
            assert_eq!(request.initial_name, "input");
            assert_eq!(request.extension, ".jpg");
            Some("renamed-marker".into())
        }
    }

    #[test]
    fn router_rename_forwards_supported_and_cancels_unsupported() {
        // secondary_window_port.rs:554,571,651: Option replacements / supports false.
        // C#: SecondaryWindowRouter.Pick / Supports / PromptRename.
        let request = RenamePromptRequest {
            initial_name: "input".into(),
            extension: ".jpg".into(),
            validate: Arc::new(|_, _| None),
        };
        for supported in [false, true] {
            let router = SecondaryWindowRouter::new(Rc::new(Native { supported }));
            assert!(router.supports(SecondaryWindowKind::RenamePrompt));
            assert_eq!(
                router.prompt_rename(73, &request),
                supported.then(|| "renamed-marker".into())
            );
        }
    }
}
