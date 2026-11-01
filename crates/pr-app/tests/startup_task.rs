//! `StartupTask` (C# `Task<T>` from `Task.Run`): result taken once; a panicking worker answers `None` (BACKLOG G13, pr-164 L6).
use pr_app::startup::context::StartupTask;

#[test]
fn join_returns_the_worker_result() {
    assert_eq!(StartupTask::spawn("startup-task-ok", || 7).join(), Some(7));
}

#[test]
fn join_is_none_when_the_worker_panics() {
    assert_eq!(
        StartupTask::spawn("startup-task-panic", || -> u8 { panic!("boom") }).join(),
        None
    );
}
