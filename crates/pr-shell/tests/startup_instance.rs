//! W8-17d real named-object guards, no application data or Recycle Bin access.
#![cfg(windows)]
#[path = "common/startup_fixture.rs"]
mod startup_fixture;
use pr_app::startup::{forwarded_open::new_forward_coalescer, instance::InstanceGate};
use pr_core::{abstractions::NullLog, model::InstanceMode};
use pr_shell::startup::instance::{ScopeGate, create_scope};
use pr_test_support::QueuedUiScheduler;
use std::sync::Arc;

#[test]
#[ignore = "native: needs desktop"]
fn a_second_scope_is_refused_and_forwards_to_the_owner() {
    // C#: ShellStartupTests.SecondInstance_ForwardsItsPathToTheRunningOneAndExitsZero (gate half).
    startup_fixture::on_ui(|| {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let prefix = format!("PhotoReviewRsTest{stamp:032x}");
        let ui = Arc::new(QueuedUiScheduler::new());
        let log = Arc::new(NullLog);
        let coalescer = new_forward_coalescer(ui, log.clone());
        let first = create_scope(InstanceMode::SingleWindow, coalescer.clone(), log.clone(), &prefix).unwrap();
        let second = create_scope(InstanceMode::SingleWindow, coalescer, log.clone(), &prefix).unwrap();
        assert!(first.try_acquire(None).unwrap());
        assert!(!second.try_acquire(None).unwrap());
        let gate = ScopeGate::new(second.clone(), log);
        assert_eq!(gate.mode(), InstanceMode::SingleWindow);
        assert!(gate.forward_to_owner(None, &[]).unwrap());
        second.dispose();
        first.dispose();
    });
}
