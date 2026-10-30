//! cl-mainwin (2026-10-30): mutation killers for the main-window glue (`host/main_window/lifecycle.rs`, `InnerSteps` and the pump guard).
//! Native tests on a hidden, never shown shell window (same fixture as `host_main_window_lifecycle.rs`); every wait is bounded, no sleeps.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/main_window_fixture.rs"]
mod main_window_fixture;
#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
mod support {
    #[path = "../../../pr-app/tests/support/main_env.rs"]
    pub mod main_env;
    #[path = "../../../pr-app/tests/support/presenter_fakes.rs"]
    pub mod presenter_fakes;
    #[path = "../../../pr-app/tests/support/preview_fakes.rs"]
    pub mod preview_fakes;
}
#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use main_window_fixture::{
    BuildOptions, EventLog, RecordingFeature, build, built, finish, new_order, run, start_ui, wait_until,
};
use pr_app::viewmodels::main::{ChangeSet, MainProperty};
use pr_core::abstractions::{UiDispatcher, UiPriority};

fn count(log: &EventLog, entry: &str) -> usize {
    log.borrow().iter().filter(|known| *known == entry).count()
}

/// Posts a `Render` sentinel behind the fit update (same priority, FIFO) and waits for it: everything posted before has run.
fn drain_render_queue(ui: &ui_thread::UiThread) {
    let done = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&done);
    run(ui, move || {
        built()
            .window
            .dispatcher()
            .post_with_priority(Box::new(move || flag.store(true, Ordering::SeqCst)), UiPriority::Render);
    });
    wait_until(ui, "the Render sentinel", move || done.load(Ordering::SeqCst));
}

fn max_width() -> f64 {
    let b = built();
    let view_model = b.window.view_model().borrow();
    view_model.viewer().borrow().max_image_width()
}

fn with_viewer<R>(f: impl FnOnce(&mut pr_app::viewmodels::viewer_state::ViewerState) -> R) -> R {
    let b = built();
    let view_model = b.window.view_model().borrow();
    let mut viewer = view_model.viewer().borrow_mut();
    f(&mut viewer)
}

fn current_image_change() -> ChangeSet {
    let mut changes = ChangeSet::default();
    changes.main.insert(MainProperty::CurrentImage);
    changes
}

// lifecycle.rs:131 `viewer_is_fit -> false` and :135 `post_update_fit_size -> ()`: a new current image in Fit posts the fit update, which re-reads the
// viewport into the viewer (`update_fit_size_if_alive` -> `update_viewport`). The viewer limit is poisoned first so the update is visible.
#[test]
#[ignore = "native: needs desktop"]
fn current_image_in_fit_mode_posts_the_fit_update_that_refreshes_the_viewer_limits() {
    let ui = start_ui();
    run(&ui, || {
        let b = build(BuildOptions::default());
        assert!(with_viewer(|v| v.is_fit()));
        with_viewer(|v| v.set_max_image_width(7.0));
        b.window.apply_changes(&current_image_change());
        assert_eq!(max_width(), 7.0, "the update is posted, not run inline");
    });
    wait_until(&ui, "the fit update to run", || max_width() != 7.0);
    finish(ui);
}

// lifecycle.rs:131 `viewer_is_fit -> true`: outside Fit (zoom 2.0) the same change posts nothing.
#[test]
#[ignore = "native: needs desktop"]
fn current_image_outside_fit_mode_posts_no_fit_update() {
    let ui = start_ui();
    run(&ui, || {
        let b = build(BuildOptions::default());
        with_viewer(|v| v.set_zoom_raw(2.0));
        assert!(!with_viewer(|v| v.is_fit()));
        with_viewer(|v| v.set_max_image_width(7.0));
        b.window.apply_changes(&current_image_change());
    });
    drain_render_queue(&ui);
    let width = run(&ui, max_width);
    assert_eq!(width, 7.0, "no fit update was posted");
    finish(ui);
}

// lifecycle.rs:156 `notify_current_index -> ()`: a CurrentIndex change tells every feature the current index.
#[test]
#[ignore = "native: needs desktop"]
fn current_index_change_notifies_the_features_with_the_index() {
    let ui = start_ui();
    let (hooks, index) = run(&ui, || {
        let b = build(BuildOptions::default());
        let log = new_order();
        b.window.add_feature(RecordingFeature::new("f", Rc::clone(&log)));
        let mut changes = ChangeSet::default();
        changes.main.insert(MainProperty::CurrentIndex);
        b.window.apply_changes(&changes);
        let index = b.window.view_model().borrow().current_index();
        (count(&log, &format!("f:index={index}")), index)
    });
    assert_eq!(hooks, 1, "index {index}");
    finish(ui);
}

// lifecycle.rs:198 `ClearOnDrop::drop -> ()`: the pump guard clears `pumping`, so a second, sequential pump fans out again.
#[test]
#[ignore = "native: needs desktop"]
fn a_second_sequential_pump_runs_after_the_first_one_finished() {
    let ui = start_ui();
    let pumps = run(&ui, || {
        let b = build(BuildOptions::default());
        let log = new_order();
        b.window.add_feature(RecordingFeature::new("p", Rc::clone(&log)));
        b.window.pump_view_model();
        b.window.pump_view_model();
        count(&log, "p:on_changes")
    });
    assert_eq!(pumps, 2);
    finish(ui);
}

// lifecycle.rs:119 `is_disposed -> false`: a disposed window ignores a fan-out (no title write on the destroyed window, no feature call).
#[test]
#[ignore = "native: needs desktop"]
fn a_fan_out_after_dispose_logs_no_error_and_reaches_no_feature() {
    let ui = start_ui();
    let (errors, hooks) = run(&ui, || {
        let b = build(BuildOptions::default());
        let log = new_order();
        b.window.add_feature(RecordingFeature::new("f", Rc::clone(&log)));
        b.window.dispose();
        let before = count(&log, "f:on_changes") + count(&log, "f:index=0");
        let mut changes = ChangeSet::default();
        changes.main.insert(MainProperty::FolderTitle);
        changes.main.insert(MainProperty::CurrentIndex);
        b.window.apply_changes(&changes);
        let after = count(&log, "f:on_changes") + count(&log, "f:index=0");
        (b.log.entries().len(), after - before)
    });
    assert_eq!(hooks, 0);
    assert_eq!(errors, 0, "no log entry after dispose");
    finish(ui);
}

// lifecycle.rs:167 `on_viewer_changed -> ()`: a viewer change queues one layout pass (`invalidate_layout`), which is observable through `is_layout_posted`.
#[test]
#[ignore = "native: needs desktop"]
fn a_viewer_change_queues_a_layout_pass() {
    let ui = start_ui();
    drain_render_queue_after_build(&ui);
    let (before, after) = run(&ui, || {
        let b = built();
        let before = b.window.viewport().is_layout_posted();
        let mut changes = ChangeSet::default();
        changes
            .viewer
            .insert(pr_app::viewmodels::viewer_state::ViewerProperty::Zoom);
        b.window.apply_changes(&changes);
        (before, b.window.viewport().is_layout_posted())
    });
    assert!(!before, "the layout queue was drained");
    assert!(after, "the viewer change posted a layout pass");
    finish(ui);
}

fn drain_render_queue_after_build(ui: &ui_thread::UiThread) {
    run(ui, || {
        build(BuildOptions::default());
    });
    drain_render_queue(ui);
}
