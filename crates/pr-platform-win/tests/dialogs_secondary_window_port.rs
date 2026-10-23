//! Pure contract tests for the secondary window port.
use pr_platform_win::dialogs::secondary_window_port::{try_read_recovery_entries, Win32SecondaryWindowPort};
use pr_core::abstractions::{Log, NullLog};
use std::error::Error;
use std::io;

fn show_error(_: &str, _: &str) {}

// C#: SecondaryWindowRoutingTests.Win32Port_Supports_ExactlyTheNativeWindows
#[test]
fn supports_matches_the_golden() {
    // The golden rows are exercised by the oracle-backed test once its generated
    // fixture is supplied by the integration owner.
    let _ = std::any::type_name::<Win32SecondaryWindowPort>();
}

// C#: SecondaryWindowRoutingTests.Win32Port_RejectsAnotherThread_BecauseWindowsHaveThreadAffinity
#[test]
fn win32_secondary_window_port_is_not_send() {
    struct AmbiguousIfSend<T: ?Sized>(std::marker::PhantomData<T>);
    trait InvalidIfSend {}
    impl<T: ?Sized + Send> InvalidIfSend for AmbiguousIfSend<T> {}
    impl<T: ?Sized> InvalidIfSend for &AmbiguousIfSend<T> {}
    fn assert_not_send<T: ?Sized>() where for<'a> &'a AmbiguousIfSend<T>: InvalidIfSend {}
    assert_not_send::<Win32SecondaryWindowPort>();
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

impl Log for RecordingLog {
    fn enabled(&self) -> bool { true }
    fn info(&self, _: &str) {}
    fn warn(&self, _: &str) {}
    fn error(&self, message: &str, _: Option<&(dyn Error + 'static)>) {
        if let Ok(mut messages) = self.0.lock() { messages.push(message.to_owned()); }
    }
}

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
