//! P-343: `join_logged` (the consumer of `StartupTask::join` in `startup::run`) writes a swallowed worker failure to the log at info level and changes nothing else.
use std::sync::Mutex;

use pr_app::startup::context::StartupTask;
use pr_core::abstractions::Log;
use pr_shell::startup::run::join_logged;

#[derive(Default)]
struct RecordingLog {
    infos: Mutex<Vec<String>>,
    others: Mutex<usize>,
}

impl Log for RecordingLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, message: &str) {
        self.infos.lock().unwrap().push(message.to_owned());
    }
    fn warn(&self, _message: &str) {
        *self.others.lock().unwrap() += 1;
    }
    fn error(&self, _message: &str, _error: Option<&(dyn std::error::Error + 'static)>) {
        *self.others.lock().unwrap() += 1;
    }
}

// new
#[test]
fn join_logged_writes_an_info_line_when_the_worker_panicked() {
    let log = RecordingLog::default();
    let task = StartupTask::spawn("startup-task-join-log-panic", || -> u8 { panic!("boom") });
    assert_eq!(join_logged(task, "early decode plan", &log), None);
    let infos = log.infos.lock().unwrap();
    assert_eq!(infos.len(), 1);
    assert!(infos[0].contains("early decode plan"), "{infos:?}");
    assert_eq!(*log.others.lock().unwrap(), 0, "low level only");
}

// new
#[test]
fn join_logged_is_silent_and_returns_the_value_when_the_worker_succeeds() {
    let log = RecordingLog::default();
    let task = StartupTask::spawn("startup-task-join-log-ok", || 7u8);
    assert_eq!(join_logged(task, "early decode plan", &log), Some(7));
    // A task whose value is itself `None` (no plan could be built) is a normal answer, not a failure.
    let none = StartupTask::spawn("startup-task-join-log-none", || None::<u8>);
    assert_eq!(join_logged(none, "early decode plan", &log), Some(None));
    assert!(log.infos.lock().unwrap().is_empty());
    assert_eq!(*log.others.lock().unwrap(), 0);
}
