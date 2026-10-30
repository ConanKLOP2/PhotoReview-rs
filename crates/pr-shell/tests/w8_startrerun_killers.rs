//! cl-startrerun mutation killers for `pr-shell` startup files (hidden dispatcher only, fake instance prefix, no real window, no user data).
#![cfg(windows)]
#[path = "common/composition_fixture.rs"]
#[allow(dead_code)]
mod composition_fixture;
#[path = "common/startup_fixture.rs"]
mod startup_fixture;

use pr_app::startup::{forwarded_open::new_forward_coalescer, instance::InstanceGate};
use pr_core::{abstractions::NullLog, model::InstanceMode};
use pr_shell::startup::instance::{ScopeGate, create_scope};
use pr_test_support::QueuedUiScheduler;
use startup_fixture::{StartupFixture, on_ui};
use std::sync::Arc;

fn fake_prefix(tag: &str) -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("PhotoReviewRsTest{tag}{stamp:032x}")
}

fn gate_over(mode: InstanceMode, prefix: &str) -> ScopeGate {
    let ui = Arc::new(QueuedUiScheduler::new());
    let log = Arc::new(NullLog);
    let coalescer = new_forward_coalescer(ui, log.clone());
    let scope = create_scope(mode, coalescer, log.clone(), prefix).unwrap();
    ScopeGate::new(scope, log)
}

#[test]
fn scope_gate_reports_its_mode_acquires_once_and_debug_names_the_type() {
    // Mutants instance.rs:48 Debug, :62 `mode` -> Default, :67 `try_acquire` -> Ok(true)/Ok(false).
    on_ui(|| {
        let prefix = fake_prefix("A");
        let first = gate_over(InstanceMode::PerFolder, &prefix);
        let second = gate_over(InstanceMode::PerFolder, &prefix);
        assert_eq!(first.mode(), InstanceMode::PerFolder);
        assert!(format!("{first:?}").contains("ScopeGate"));
        let folder = "C:\\PhotoReviewRsTestFolder";
        assert!(first.try_acquire(Some(folder)).unwrap(), "first owner acquires");
        assert!(!second.try_acquire(Some(folder)).unwrap(), "second is refused");
    });
}

#[test]
fn scope_gate_forward_without_an_owner_is_not_delivered() {
    // Mutant instance.rs:75 `forward_to_owner` -> Ok(true): nobody owns this fake prefix, so nothing can be delivered.
    on_ui(|| {
        let gate = gate_over(InstanceMode::SingleWindow, &fake_prefix("B"));
        assert!(!gate.forward_to_owner(None, &[]).unwrap());
    });
}

#[test]
fn scope_gate_forward_reaches_a_running_owner() {
    // Mutant instance.rs:75 `forward_to_owner` -> Ok(false): an owner holds the fake prefix, so the hand-off is delivered.
    on_ui(|| {
        let prefix = fake_prefix("C");
        let owner = gate_over(InstanceMode::SingleWindow, &prefix);
        let other = gate_over(InstanceMode::SingleWindow, &prefix);
        assert!(owner.try_acquire(None).unwrap());
        assert!(!other.try_acquire(None).unwrap());
        assert!(other.forward_to_owner(None, &[]).unwrap());
    });
}

#[test]
fn startup_state_debug_names_the_type() {
    // Mutant state.rs:101 `<Debug for StartupState>::fmt` -> Ok(Default::default()).
    on_ui(|| {
        let rig = StartupFixture::new(pr_shell::startup::options::ShellStartupOptions::default());
        assert!(format!("{:?}", rig.state).contains("StartupState"));
    });
}

#[test]
fn graph_inputs_and_startup_graph_debug_name_their_types() {
    // Mutants graph.rs:48 `<Debug for GraphInputs>::fmt`, :85 `<Debug for StartupGraph>::fmt`.
    on_ui(|| {
        use pr_core::app_paths::AppPaths;
        use pr_platform_win::{
            host::{message_loop::MessageLoop, ui_dispatcher::Win32UiDispatcher},
            shell_viewing::presentation_sink::ShellPresentationHub,
        };
        use pr_shell::startup::{
            composition_root::{ShellHostServices, ShellServices, ShellServicesOptions},
            graph::{GraphInputs, GraphOverrides, production_graph_builder_with},
        };
        use std::rc::Rc;
        let (fake_services, root, _) = composition_fixture::shared_services();
        let paths = AppPaths::new(root.path(), None, true);
        let log = Arc::new(NullLog);
        let dispatcher = Win32UiDispatcher::new(log.clone()).unwrap();
        let message_loop = Rc::new(MessageLoop::new(Rc::new(dispatcher.clone())).unwrap());
        let hub = Rc::new(ShellPresentationHub::new());
        let services = Rc::new(ShellServices::build(
            ShellHostServices {
                dispatcher: dispatcher.clone(),
                owner_window: Rc::new(|| 0),
                message_loop: Some(message_loop.clone()),
            },
            hub.clone(),
            log.clone(),
            ShellServicesOptions::default(),
        ));
        let builder = production_graph_builder_with(GraphOverrides {
            paths: Some(paths),
            log: Some(log.clone()),
            recycle_bin: Some(fake_services.recycle_bin()),
            explorer_order: Some(fake_services.explorer_order()),
            clock: Some(fake_services.clock()),
            decoder_wrapper: None,
        });
        let inputs = GraphInputs {
            services,
            dispatcher: dispatcher.clone(),
            message_loop,
            hub,
            log,
            mark: Arc::new(|_| {}),
        };
        assert!(format!("{inputs:?}").contains("GraphInputs"));
        let graph = builder(inputs).unwrap();
        assert!(format!("{graph:?}").contains("StartupGraph"));
        drop(graph);
        dispatcher.shutdown();
    });
}
