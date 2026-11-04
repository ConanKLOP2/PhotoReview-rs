//! M4 mutation killers for `presenters/action_profiles.rs` (X-action-profiles). Hand-written, no golden data.

mod presenters_support;

use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pr_app::misc::app_log::{AppLog, AppLogTarget};
use pr_app::presenters::action_profiles::{ActionProfilesPresenter, try_export, try_import};
use pr_core::abstractions::Log;
use presenters_support::{TempDir, english, mv};

// C# `Remove(action, selectedIndex)`: removing the OPEN row drops the open state, removing another row keeps it.
#[test]
fn remove_clears_the_open_row_only_when_the_open_row_is_removed() {
    let mut presenter = ActionProfilesPresenter::new(&[mv("A", "F1"), mv("B", "F2"), mv("C", "F3")]);
    let (a, b, c) = (
        presenter.row_id(0).unwrap(),
        presenter.row_id(1).unwrap(),
        presenter.row_id(2).unwrap(),
    );

    presenter.open(Some(b));
    assert_eq!(presenter.remove(a, 0), Some(0));
    assert_eq!(presenter.loaded(), Some(b), "removing another row keeps the open row");

    assert_eq!(presenter.remove(b, 0), Some(0));
    assert_eq!(presenter.loaded(), None, "removing the open row closes it");
    assert_eq!(presenter.row_id(0), Some(c));
}

#[derive(Default)]
struct CaptureLog {
    errors: Mutex<Vec<String>>,
}

impl Log for CaptureLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, _message: &str) {}
    fn warn(&self, _message: &str) {}
    fn error(&self, message: &str, _error: Option<&(dyn Error + 'static)>) {
        self.errors.lock().unwrap().push(message.to_owned());
    }
}

impl AppLogTarget for CaptureLog {
    fn set_enabled(&self, _enabled: bool) {}
    fn flush(&self) {}
    fn shutdown(&self) {}
    fn file_path(&self) -> PathBuf {
        PathBuf::from("capture.log")
    }
}

// C# `TryImport` / `TryExport`: an I/O failure is logged to `AppLog` (message + path) before the localized text returns.
// The only test of this binary that replaces the process-wide log instance.
#[test]
fn import_and_export_failures_are_logged_with_the_path() {
    let capture = Arc::new(CaptureLog::default());
    AppLog::global().set_instance(Some(capture.clone()));
    let texts = english();
    let dir = TempDir::new("m4-log");

    let missing = dir.join("missing.json");
    assert!(try_import(&missing, &texts).is_err());
    let blocked = dir.join("no-such-folder").join("x.json");
    assert!(try_export(&blocked, &[mv("A", "F6")], &texts).is_some());
    let bad_utf8 = dir.join("bad.json");
    std::fs::write(&bad_utf8, [0xFFu8, 0xFE, 0x00]).unwrap();
    assert!(try_import(&bad_utf8, &texts).is_err());

    AppLog::global().set_instance(None);
    let errors = capture.errors.lock().unwrap().clone();
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert_eq!(errors[0], format!("Action import failed: {}", missing.display()));
    assert_eq!(errors[1], format!("Action export failed: {}", blocked.display()));
    assert_eq!(errors[2], format!("Action import failed: {}", bad_utf8.display()));
}
