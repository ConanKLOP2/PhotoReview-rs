//! Pure contract tests for the secondary window port.
#[rustfmt::skip]
use pr_platform_win::dialogs::secondary_window_port::{try_read_recovery_entries, Win32SecondaryWindowPort};
use pr_core::abstractions::{BatchReviewItem, SettingsTarget, SkippedEntry};
use pr_core::abstractions::{Log, NullLog};
use pr_core::settings::shortcuts::ReviewAction;
use pr_platform_win::dialogs::dialog_service::{SecondaryWindowKind, SecondaryWindowPort};
use pr_platform_win::dialogs::native::dialog::NativeDialog;
use pr_platform_win::dialogs::secondary_window_port::{
    ActionProfilesOpener, PortOptions, PreparedDialog, SecondaryWindowDialogs, SecondaryWindowRouter,
};
use pr_platform_win::win32::structs::Hwnd;
use std::cell::RefCell;
use std::error::Error;
use std::io;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

fn show_error(_: &str, _: &str) {}

// C#: SecondaryWindowRoutingTests.Win32Port_Supports_ExactlyTheNativeWindows
#[test]
fn supports_matches_the_golden() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/secondary-window-supports.json")).unwrap();
    let cases = golden["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 50);
    let kinds = [
        SecondaryWindowKind::Settings,
        SecondaryWindowKind::Recovery,
        SecondaryWindowKind::Diagnostics,
        SecondaryWindowKind::Benchmark,
        SecondaryWindowKind::SkippedFiles,
        SecondaryWindowKind::BatchReview,
        SecondaryWindowKind::ZoomPrompt,
        SecondaryWindowKind::ActionProfiles,
        SecondaryWindowKind::ExternalEditors,
        SecondaryWindowKind::RenamePrompt,
    ];
    for row in cases {
        if row["kind"] == "kinds" {
            assert_eq!(
                serde_json::json!(kinds.iter().map(|kind| format!("{kind:?}")).collect::<Vec<_>>()),
                row["names"]
            );
            assert_eq!(
                serde_json::json!(kinds.iter().map(|kind| *kind as i32).collect::<Vec<_>>()),
                row["values"]
            );
            continue;
        }
        let enabled = match row["settings_enabled"].as_str().unwrap() {
            "true" => Some(true),
            "false" => Some(false),
            "default" => None,
            other => panic!("unknown enabled flag {other}"),
        };
        let port = Win32SecondaryWindowPort::new(
            Rc::new(NoDialogs::default()),
            Arc::new(NullLog),
            PortOptions {
                settings_enabled: enabled.map(|value| Box::new(move || value) as Box<dyn Fn() -> bool>),
                ..Default::default()
            },
        );
        if row["kind"] == "initial" {
            assert_eq!(port.open_count(), row["open_count"].as_u64().unwrap() as usize);
            assert_eq!(port.is_benchmark_open(), row["is_benchmark_open"].as_bool().unwrap());
            assert_eq!(
                port.settings_enabled(),
                row["settings_enabled_value"].as_bool().unwrap()
            );
            continue;
        }
        let name = row["window"].as_str().unwrap();
        let kind = kinds.iter().find(|kind| format!("{kind:?}") == name).copied().unwrap();
        let actual = match row["kind"].as_str().unwrap() {
            "supports" => port.supports(kind),
            "router_supports" => SecondaryWindowRouter::new(port).supports(kind),
            other => panic!("unhandled golden kind {other}"),
        };
        assert_eq!(actual, row["value"].as_bool().unwrap(), "{}", row["name"]);
    }
}

// C#: SecondaryWindowRoutingTests.Win32Port_RejectsAnotherThread_BecauseWindowsHaveThreadAffinity
#[test]
#[rustfmt::skip]
fn win32_secondary_window_port_is_not_send() {
    trait AmbiguousIfSend<A> { fn probe() {} }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}
    <Win32SecondaryWindowPort as AmbiguousIfSend<_>>::probe();
}

// C#: RecoveryDialogDataSafetyTests.ReadingTheJournal_ReportsIoAndAccessErrors_ThroughTheCallback
#[test]
fn try_read_recovery_entries_returns_success_without_error_callback() {
    let result = try_read_recovery_entries(|| Ok(vec![1_u8, 2]), &NullLog, &show_error);
    assert_eq!(result, Some(vec![1, 2]));
}

// C#: RecoveryDialogDataSafetyTests.ReadingTheJournal_ReportsIoAndAccessErrors_ThroughTheCallback
#[test]
fn try_read_recovery_entries_not_found_is_reported() {
    assert_reported(io::ErrorKind::NotFound);
}

// C#: RecoveryDialogDataSafetyTests.ReadingTheJournal_ReportsIoAndAccessErrors_ThroughTheCallback
#[test]
fn try_read_recovery_entries_permission_denied_is_reported() {
    assert_reported(io::ErrorKind::PermissionDenied);
}

// C#: RecoveryDialogDataSafetyTests.ReadingTheJournal_ReportsIoAndAccessErrors_ThroughTheCallback
#[test]
fn try_read_recovery_entries_other_error_is_reported() {
    assert_reported(io::ErrorKind::Other);
}

#[derive(Default)]
struct RecordingLog(std::sync::Mutex<Vec<String>>);

#[rustfmt::skip]
impl Log for RecordingLog {
    fn enabled(&self) -> bool { true }
    fn info(&self, _: &str) {}
    fn warn(&self, _: &str) {}
    fn error(&self, message: &str, _: Option<&(dyn Error + 'static)>) {
        if let Ok(mut messages) = self.0.lock() { messages.push(message.to_owned()); }
    }
}

#[rustfmt::skip]
fn assert_reported(kind: io::ErrorKind) {
    let log = RecordingLog::default();
    let shown = std::cell::RefCell::new(None);
    let result = try_read_recovery_entries::<u8>(
        || Err(io::Error::new(kind, "test read failure")),
        &log,
        &|title, message| *shown.borrow_mut() = Some((title.to_owned(), message.to_owned())),
    );
    assert_eq!(result, None);
    assert_eq!(log.0.lock().map(|m| m.as_slice().to_vec()).unwrap_or_default(),
        vec!["Could not read the operation journal for the Recovery window"]);
    assert!(shown.into_inner().is_some());
}

#[derive(Default)]
struct NoDialogs {
    recovery_calls: std::cell::Cell<usize>,
    diagnostics_calls: std::cell::Cell<usize>,
    skipped_calls: std::cell::Cell<usize>,
}

impl SecondaryWindowDialogs for NoDialogs {
    fn settings(&self, _: SettingsTarget, _: ActionProfilesOpener) -> Option<PreparedDialog<bool>> {
        None
    }
    fn recovery(&self, _: Hwnd) -> Option<PreparedDialog<()>> {
        self.recovery_calls.set(self.recovery_calls.get() + 1);
        None
    }
    fn benchmark(&self, _: Option<&Path>) -> Option<NativeDialog> {
        None
    }
    fn action_profiles(&self, _: &[ReviewAction]) -> Option<PreparedDialog<Option<Vec<ReviewAction>>>> {
        None
    }
    fn diagnostics(&self) -> Option<PreparedDialog<()>> {
        self.diagnostics_calls.set(self.diagnostics_calls.get() + 1);
        None
    }
    fn skipped_files(&self, _: &[SkippedEntry]) -> Option<PreparedDialog<()>> {
        self.skipped_calls.set(self.skipped_calls.get() + 1);
        None
    }
    fn batch_review(&self, _: &[BatchReviewItem]) -> Option<PreparedDialog<bool>> {
        None
    }
    fn custom_zoom(&self, _: i32) -> Option<PreparedDialog<Option<i32>>> {
        None
    }
}

// C#: SecondaryWindowRoutingTests.WindowsTheWin32PortSupports_GoToIt_AndNeverTouchTheFallback
#[test]
fn active_window_commands_reach_the_factory_and_disposed_commands_do_not() {
    let factory = Rc::new(NoDialogs::default());
    let port = Win32SecondaryWindowPort::new(
        Rc::clone(&factory) as Rc<dyn SecondaryWindowDialogs>,
        Arc::new(NullLog),
        PortOptions::default(),
    );
    port.show_recovery(0);
    port.show_diagnostics(0);
    port.show_skipped_files(0, &[]);
    assert_eq!(factory.recovery_calls.get(), 1);
    assert_eq!(factory.diagnostics_calls.get(), 1);
    assert_eq!(factory.skipped_calls.get(), 1);
    port.dispose();
    port.show_recovery(0);
    port.show_diagnostics(0);
    port.show_skipped_files(0, &[]);
    assert_eq!(factory.recovery_calls.get(), 1);
    assert_eq!(factory.diagnostics_calls.get(), 1);
    assert_eq!(factory.skipped_calls.get(), 1);
}

// C#: SettingsPortTests.SettingsIsSupported_OnlyWhenEnabled
#[test]
fn settings_enabled_option_and_supports_table_are_exact() {
    for (setting, enabled) in [(true, true), (false, false)] {
        let port = Win32SecondaryWindowPort::new(
            Rc::new(NoDialogs::default()),
            Arc::new(NullLog),
            PortOptions {
                settings_enabled: Some(Box::new(move || setting)),
                ..PortOptions::default()
            },
        );
        assert_eq!(port.settings_enabled(), enabled);
        assert_eq!(port.supports(SecondaryWindowKind::Settings), enabled);
        for kind in [
            SecondaryWindowKind::Recovery,
            SecondaryWindowKind::Diagnostics,
            SecondaryWindowKind::Benchmark,
            SecondaryWindowKind::SkippedFiles,
            SecondaryWindowKind::BatchReview,
            SecondaryWindowKind::ZoomPrompt,
            SecondaryWindowKind::ActionProfiles,
        ] {
            assert!(port.supports(kind), "{kind:?}");
        }
        assert!(port.supports(SecondaryWindowKind::ExternalEditors));
        assert!(port.supports(SecondaryWindowKind::RenamePrompt)); // W9-R8: flipped, C# Supports lists RenamePrompt
        assert_eq!(port.open_count(), 0);
        assert!(!port.is_benchmark_open());
    }
}

#[derive(Default)]
struct SpyPort {
    allow_settings: bool,
    calls: RefCell<Vec<&'static str>>,
}

impl SpyPort {
    fn hit(&self, name: &'static str) {
        self.calls.borrow_mut().push(name);
    }
}

impl SecondaryWindowPort for SpyPort {
    fn supports(&self, kind: SecondaryWindowKind) -> bool {
        kind != SecondaryWindowKind::Settings || self.allow_settings
    }
    fn show_settings(&self, _: Hwnd, _: SettingsTarget) -> bool {
        self.hit("settings");
        true
    }
    fn show_recovery(&self, _: Hwnd) {
        self.hit("recovery");
    }
    fn show_diagnostics(&self, _: Hwnd) {
        self.hit("diagnostics");
    }
    fn show_benchmark(&self, _: Hwnd, _: Option<&Path>) {
        self.hit("benchmark");
    }
    fn show_skipped_files(&self, _: Hwnd, _: &[SkippedEntry]) {
        self.hit("skipped");
    }
    fn show_batch_review(&self, _: Hwnd, _: &[BatchReviewItem]) -> bool {
        self.hit("batch");
        true
    }
    fn prompt_custom_zoom(&self, _: Hwnd, _: i32) -> Option<i32> {
        self.hit("zoom");
        Some(37)
    }
    fn show_action_profiles(&self, _: Hwnd, _: &[ReviewAction]) -> Option<Vec<ReviewAction>> {
        self.hit("actions");
        Some(Vec::new())
    }
    fn dispose(&self) {
        self.hit("dispose");
    }
}

// C#: SecondaryWindowRoutingTests.WhenTheWin32PortLacksSettings_ItFallsBackToTheTemporaryPort_WhileWin32WindowsStillWork
#[test]
fn router_routes_supported_windows_and_returns_cancelled_for_unsupported_settings() {
    let spy = Rc::new(SpyPort {
        allow_settings: false,
        ..SpyPort::default()
    });
    let router = SecondaryWindowRouter::new(spy.clone());
    assert!(router.supports(SecondaryWindowKind::Settings));
    assert!(!router.show_settings(0, SettingsTarget::Default));
    router.show_recovery(0);
    router.show_diagnostics(0);
    router.show_benchmark(0, None);
    router.show_skipped_files(0, &[]);
    assert!(router.show_batch_review(0, &[]));
    assert_eq!(router.prompt_custom_zoom(0, 12), Some(37));
    assert_eq!(router.show_action_profiles(0, &[]), Some(Vec::new()));
    assert_eq!(
        &*spy.calls.borrow(),
        &[
            "recovery",
            "diagnostics",
            "benchmark",
            "skipped",
            "batch",
            "zoom",
            "actions"
        ]
    );
    router.dispose();
    assert_eq!(spy.calls.borrow().last(), Some(&"dispose"));
}

// C#: SecondaryWindowRoutingTests.Settings_GoesToTheWin32Port_WhenItSupportsIt
#[test]
fn router_forwards_settings_when_the_native_port_supports_it() {
    let spy = Rc::new(SpyPort {
        allow_settings: true,
        ..SpyPort::default()
    });
    let router = SecondaryWindowRouter::new(spy.clone());
    assert!(router.show_settings(0, SettingsTarget::Default));
    assert_eq!(&*spy.calls.borrow(), &["settings"]);
}
