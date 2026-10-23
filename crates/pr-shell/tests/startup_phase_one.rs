//! W8-17d hidden-dispatcher failure paths. Full run-loop tests wait for 17e.
#![cfg(windows)]
#[path = "common/startup_fixture.rs"]
mod startup_fixture;
use pr_shell::startup::{error::StartupError, options::ShellStartupOptions};
use startup_fixture::{StartupFixture, on_ui};
use std::{cell::RefCell, rc::Rc};

#[test]
#[ignore = "native: needs desktop"]
fn phase_one_failure_in_the_graph_builder_calls_done_once_with_err() {
    // New: ShellStartup.RunStartupGuardedAsync failure before any worker is started.
    on_ui(|| {
        let options = ShellStartupOptions {
            install_process_hooks: false,
            graph_builder: Some(Box::new(|_| Err(std::io::Error::other("graph failed").into()))),
            ..ShellStartupOptions::default()
        };
        let rig = StartupFixture::new(options);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sink = calls.clone();
        rig.state
            .start_phase_one(Box::new(move |result| sink.borrow_mut().push(result)));
        assert_eq!(calls.borrow().len(), 1, "synchronous failure, exactly once");
        assert!(matches!(&calls.borrow()[0], Err(StartupError::Graph(m)) if m == "graph failed"));
        assert!(!rig.root.join("operations.jsonl").exists());
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn phase_one_a_panicking_graph_builder_is_reported_once_without_leaking_state() {
    // New: R2-F-04, guarded startup must not unwind out of the dispatcher.
    on_ui(|| {
        let options = ShellStartupOptions {
            graph_builder: Some(Box::new(|_| panic!("graph panicked"))),
            install_process_hooks: false,
            ..ShellStartupOptions::default()
        };
        let rig = StartupFixture::new(options);
        let state = Rc::downgrade(&rig.state);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sink = calls.clone();
        rig.state
            .start_phase_one(Box::new(move |result| sink.borrow_mut().push(result)));
        assert_eq!(calls.borrow().len(), 1);
        assert!(matches!(&calls.borrow()[0], Err(StartupError::Panicked(m)) if m == "graph panicked"));
        drop(rig);
        assert!(state.upgrade().is_none());
    });
}
