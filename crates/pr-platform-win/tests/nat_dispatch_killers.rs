//! Killers for the survivors of the whole-file native mutation run of `shell_input/dispatch.rs` (lines 1..821, lane E): the `Debug` of `ShellInputDeps`, the
//! registry removal (P-431 seam `registered_input_count`), the `SurfaceFitAdapter` / `VersionAdapter` forwarders (the fixture's `FitSpy` records what the
//! Fit driver asked of the surface), `rebuild_shortcuts`, `note_current_index_changed`, `pointer_press_tunnel`, the pointer-activity subscription ids,
//! the `is_inside_viewport` edges and the invalidation of `run_pointer`. Test kind F, no sleeps.
#![cfg(windows)]

#[path = "common/shell_input_dispatch_fixture.rs"]
mod fixture;
#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use fixture::Rig;
use pr_app::input::key_id::KeyId;
use pr_app::input::primitives::{PointD, PointerButton};
use pr_app::input::review_command::ReviewCommand;
use pr_app::services::shortcut_key_name::try_parse;
use pr_core::abstractions::{Log, UiDispatcher};
use pr_core::settings::app_settings::AppSettings;
use pr_platform_win::host::contracts::{ShellCursor, ShellWindowApi};
use pr_platform_win::shell_input::dispatch::{InputSurface, ShellInputDeps, ShellInputHost, registered_input_count};
use pr_platform_win::shell_input::environment::ShellInputEnvironment;
use pr_platform_win::shell_input::native::MK_LBUTTON;
use pr_platform_win::win32::messages::{WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL};

const MKL: usize = MK_LBUTTON as usize;

fn calls(rig: &Rig) -> Vec<&'static str> {
    rig.surface.borrow().fit_spy.calls.borrow().clone()
}

fn clear_calls(rig: &Rig) {
    rig.surface.borrow().fit_spy.calls.borrow_mut().clear();
}

fn kinetic_rig() -> Rig {
    let rig = Rig::configured(|settings| settings.kinetic_pan_enabled = true);
    rig.key_down(KeyId::D1, false, false); // actual size, so an arrow key pans
    rig.drain();
    rig
}

// dispatch.rs 141:9: `Debug for ShellInputDeps` prints the type name and elides the Rc fields.
#[test]
fn shell_input_deps_debug_names_the_type_and_elides_the_fields() {
    let rig = Rig::new();
    let settings = Rc::clone(&rig.settings);
    let deps = ShellInputDeps {
        window: Rc::clone(&rig.window) as Rc<dyn ShellWindowApi>,
        surface: Rc::clone(&rig.surface) as Rc<RefCell<dyn InputSurface>>,
        viewer: rig.viewer.clone(),
        settings: Rc::new(move || Arc::clone(&settings.borrow())),
        host: Rc::clone(&rig.host) as Rc<dyn ShellInputHost>,
        dispatcher: Arc::clone(&rig.dispatcher) as Arc<dyn UiDispatcher>,
        environment: Rc::new(ShellInputEnvironment::default()),
        log: Arc::clone(&rig.log) as Arc<dyn Log>,
        ticks_per_ms: 1.0,
    };
    assert_eq!("ShellInputDeps { .. }", format!("{deps:?}"));
}

// dispatch.rs 215:5 (`unregister_input -> ()`) and 656:9 (`Drop for InputCore -> ()`): the registry entry goes away with `dispose` and with the last drop.
#[test]
fn dispose_removes_the_registry_entry() {
    let before = registered_input_count();
    let rig = Rig::new();
    assert_eq!(before + 1, registered_input_count());
    rig.input.dispose();
    assert_eq!(before, registered_input_count());
}

#[test]
fn dropping_an_undisposed_input_removes_its_registry_entry() {
    let before = registered_input_count();
    let rig = Rig::new();
    assert_eq!(before + 1, registered_input_count());
    drop(rig);
    assert_eq!(before, registered_input_count());
}

// dispatch.rs 276..296 (`SurfaceFitAdapter`) and 351/356 (`VersionAdapter`): the Fit driver's order and its answers reach the surface.
#[test]
fn apply_fit_runs_the_pass_in_order_with_the_surface_size() {
    let rig = Rig::new();
    clear_calls(&rig);
    {
        let surface = rig.surface.borrow();
        surface.fit_spy.viewport_size.set(Some((1234.0, 567.0)));
        surface.fit_spy.skip_update_fit_size.set(true);
        surface.fit_spy.skip_update_layout.set(true);
    }
    rig.input.apply_fit();
    assert_eq!(
        vec!["viewport_size", "is_loaded", "update_layout", "update_fit_size"],
        calls(&rig)
    );
    {
        let viewer = rig.viewer.borrow();
        assert_eq!(1234.0, viewer.max_image_width(), "reset_fit received the surface width");
        assert_eq!(
            567.0,
            viewer.max_image_height(),
            "reset_fit received the surface height"
        );
    }
    rig.surface.borrow().fit_spy.skip_update_fit_size.set(false);
    rig.surface.borrow().fit_spy.skip_update_layout.set(false);
    rig.surface.borrow().fit_spy.viewport_size.set(None);
    rig.drain();
    let all = calls(&rig);
    assert!(all.contains(&"capture"), "the resume captures the surface: {all:?}");
    assert_eq!(
        vec!["update_layout", "scroll_home"],
        all[all.len() - 2..].to_vec(),
        "the final section lays out and scrolls home: {all:?}"
    );
}

#[test]
fn a_fit_on_an_unloaded_surface_stops_after_the_loaded_check() {
    let rig = Rig::new();
    rig.window.loaded.set(false);
    clear_calls(&rig);
    rig.input.apply_fit();
    assert_eq!(vec!["viewport_size", "is_loaded"], calls(&rig));
}

#[test]
fn every_fit_gets_a_fresh_version_that_stays_current() {
    let rig = Rig::new();
    for round in 1..=3 {
        clear_calls(&rig);
        rig.input.apply_fit();
        let first = calls(&rig);
        assert_eq!(
            vec!["viewport_size", "is_loaded", "update_layout", "update_fit_size"],
            first,
            "round {round}"
        );
        rig.drain();
        assert!(
            calls(&rig).contains(&"scroll_home"),
            "round {round} ends with the final section"
        );
    }
}

// dispatch.rs 509:9: `rebuild_shortcuts` installs the table of the given settings.
#[test]
fn rebuild_shortcuts_binds_the_keys_of_the_given_settings() {
    let rig = Rig::new();
    let key = try_parse(Some("F9")).expect("F9");
    assert!(!rig.key_down(key, false, false).0, "unbound before");
    let mut settings = AppSettings::default();
    settings.shortcuts.refresh = "F9".to_owned();
    rig.input.rebuild_shortcuts(&settings);
    let (handled, _) = rig.key_down(key, false, false);
    assert!(handled);
    assert_eq!(vec![ReviewCommand::REFRESH], rig.host.commands_snapshot());
}

// dispatch.rs 518:9: a changed index stops a glide, the same index does not.
#[test]
fn note_current_index_changed_stops_a_glide_only_for_a_new_index() {
    let rig = kinetic_rig();
    rig.input.note_current_index_changed(777);
    rig.key_down(KeyId::RIGHT, false, false); // starts a glide
    assert_eq!(1, rig.clock.subscriber_count());
    rig.input.note_current_index_changed(777);
    assert_eq!(
        1,
        rig.clock.subscriber_count(),
        "the same index (a full-resolution swap) keeps the glide"
    );
    rig.input.note_current_index_changed(778);
    assert_eq!(0, rig.clock.subscriber_count(), "navigation stops the glide");
}

// dispatch.rs 543:9: the window-level press tunnel stops a glide.
#[test]
fn pointer_press_tunnel_stops_a_glide() {
    let rig = kinetic_rig();
    rig.key_down(KeyId::RIGHT, false, false); // starts a glide
    assert_eq!(1, rig.clock.subscriber_count());
    rig.input.pointer_press_tunnel();
    assert_eq!(0, rig.clock.subscriber_count());
}

// dispatch.rs 620:43 (`id + 1`) and 630:47 (`!=` in `retain`).
#[test]
fn pointer_activity_subscriptions_get_distinct_ids_and_unsubscribe_only_themselves() {
    let rig = Rig::new();
    let counters: Vec<Rc<Cell<u32>>> = (0..3).map(|_| Rc::new(Cell::new(0))).collect();
    let ids: Vec<_> = counters
        .iter()
        .map(|counter| {
            let counter = Rc::clone(counter);
            rig.input
                .subscribe_pointer_activity(Rc::new(move || counter.set(counter.get() + 1)))
        })
        .collect();
    assert!(ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2], "{ids:?}");
    rig.mouse(WM_MOUSEMOVE, 500, 300, 0);
    assert_eq!(vec![1, 1, 1], counters.iter().map(|c| c.get()).collect::<Vec<_>>());
    rig.input.unsubscribe_pointer_activity(ids[0]);
    rig.mouse(WM_MOUSEMOVE, 501, 300, 0);
    assert_eq!(vec![1, 2, 2], counters.iter().map(|c| c.get()).collect::<Vec<_>>());
    rig.input.unsubscribe_pointer_activity(ids[2] + 1000); // an unknown id is a no-op
    rig.mouse(WM_MOUSEMOVE, 502, 300, 0);
    assert_eq!(vec![1, 3, 3], counters.iter().map(|c| c.get()).collect::<Vec<_>>());
}

// dispatch.rs 686:53 and 686:91: `0 <= x < viewport_width`, `0 <= y < viewport_height` (the far edge is outside).
#[test]
fn a_horizontal_wheel_is_taken_inside_the_viewport_and_not_on_its_far_edges() {
    let rig = Rig::new();
    let width = rig.viewport.viewport_width();
    let height = rig.viewport.viewport_height();
    assert_eq!(width.fract(), 0.0, "an integral viewport");
    assert_eq!(height.fract(), 0.0, "an integral viewport");
    let (width, height) = (width as i32, height as i32);
    assert!(rig.wheel(WM_MOUSEHWHEEL, 120, width - 1, 300, false).0, "last column");
    assert!(
        !rig.wheel(WM_MOUSEHWHEEL, 120, width, 300, false).0,
        "x == viewport_width"
    );
    assert!(rig.wheel(WM_MOUSEHWHEEL, 120, 500, height - 1, false).0, "last row");
    assert!(
        !rig.wheel(WM_MOUSEHWHEEL, 120, 500, height, false).0,
        "y == viewport_height"
    );
}

// dispatch.rs 756:12 (`delete !` in `run_pointer`): only a pointer call that changed the viewer invalidates the window.
#[test]
fn a_zoom_invalidates_the_window_and_a_plain_move_does_not() {
    let rig = Rig::new();
    let wparam_wheel = (120_usize << 16) | 0x0008; // ctrl + one notch
    let before = rig.window.invalidations.get();
    rig.window.dispatch(WM_MOUSEWHEEL, wparam_wheel, Rig::pack(500, 300));
    assert!(
        rig.window.invalidations.get() > before,
        "a wheel zoom changes the viewer"
    );
    rig.drain();
    let before = rig.window.invalidations.get();
    rig.window.dispatch(WM_MOUSEMOVE, 0, Rig::pack(400, 300));
    assert_eq!(
        before,
        rig.window.invalidations.get(),
        "a move without a button changes nothing"
    );
}

// ---- pointer move / release routing (dispatch.rs 1030, 1033, 1045) ----

fn pan_rig() -> Rig {
    let rig = Rig::configured(|settings| settings.kinetic_pan_enabled = false);
    rig.key_down(KeyId::D1, false, false); // actual size: the image is larger than the viewport, so a drag pans
    rig.drain();
    rig
}

// dispatch.rs 1030:40 (`&&` -> `||`) and 1030:43 (`delete !`): a pan that holds the capture goes on over an overlay element.
#[test]
fn a_captured_pan_continues_over_an_overlay_element() {
    let rig = pan_rig();
    rig.input
        .set_overlay_hit_test(Some(Rc::new(|p: PointD| p.x < 100.0 && p.y < 60.0)));
    assert!(rig.mouse(WM_LBUTTONDOWN, 500, 300, 0).0);
    assert!(rig.window.captured.get());
    let before = rig.offsets();
    rig.mouse(WM_MOUSEMOVE, 50, 30, MKL); // inside the overlay rectangle, the capture is held
    assert_ne!(before, rig.offsets(), "the pan follows the pointer over the overlay");
}

// dispatch.rs 1033:84 (`delete !` on the compare check): without a capture the move is routed only over the image while the compare view is hidden.
#[test]
fn a_pan_move_without_the_capture_is_routed_over_the_image_unless_compare_is_visible() {
    let rig = pan_rig();
    assert!(rig.mouse(WM_LBUTTONDOWN, 500, 300, 0).0);
    rig.window.captured.set(false); // the capture is gone but the pan state is still on
    let start = rig.offsets();
    rig.mouse(WM_MOUSEMOVE, 450, 300, MKL);
    let moved = rig.offsets();
    assert_ne!(start, moved, "over the image, compare hidden: the move pans");
    rig.host.is_compare_visible.set(true);
    rig.mouse(WM_MOUSEMOVE, 400, 300, MKL);
    assert_eq!(
        moved,
        rig.offsets(),
        "compare visible: the move is not routed to the pan"
    );
}

fn release_resets_the_pan_cursor(rig: &Rig, x: i32, y: i32) -> bool {
    rig.window.cursor.set(None);
    rig.mouse(WM_LBUTTONUP, x, y, 0);
    rig.window.cursor.get() == Some(ShellCursor::Arrow)
}

// dispatch.rs 1045:43, 1045:46, 1045:75, 1045:78: without a capture a release reaches the pan controller only over the image, not over an overlay element
// and not while the compare view is visible (the controller's `cancel_pan` resets the pan cursor, which shows that it ran).
#[test]
fn a_release_without_the_capture_reaches_the_controller_only_over_the_bare_image() {
    let rig = Rig::new();
    rig.fit();
    assert!(!rig.window.captured.get());
    assert!(
        release_resets_the_pan_cursor(&rig, 500, 300),
        "over the image, no overlay, compare hidden"
    );
    assert!(!release_resets_the_pan_cursor(&rig, 5, 300), "left of the image");
    rig.input
        .set_overlay_hit_test(Some(Rc::new(|p: PointD| p.x > 400.0 && p.x < 600.0)));
    assert!(
        !release_resets_the_pan_cursor(&rig, 500, 300),
        "over an overlay element"
    );
    rig.input.set_overlay_hit_test(None);
    rig.host.is_compare_visible.set(true);
    assert!(!release_resets_the_pan_cursor(&rig, 500, 300), "compare visible");
    rig.host.is_compare_visible.set(false);
    assert!(release_resets_the_pan_cursor(&rig, 500, 300), "back to the bare image");
}

// ---- the public port handles (dispatch.rs 1133..1173): the window talks to the core through `WeakPorts`, the main window through these ----

// dispatch.rs 1133:9 and 1137:9.
#[test]
fn the_key_port_answers_for_bound_and_unbound_keys_and_reads_the_message_time() {
    let rig = Rig::new();
    let port = rig.input.key_port();
    rig.message_time.set(4242);
    assert_eq!(4242, port.message_time());
    assert!(
        port.handle_key_down(KeyId::R, false, false, 1),
        "a bound key is handled"
    );
    assert_eq!(vec![ReviewCommand::REFRESH], rig.host.commands_snapshot());
    assert!(
        !port.handle_key_down(KeyId::F8, false, false, 2),
        "an unbound key is not handled"
    );
}

// dispatch.rs 1143:9, 1147:9, 1151:9, 1155:9.
#[test]
fn the_pointer_port_presses_pans_releases_and_loses_the_capture() {
    let rig = pan_rig();
    let port = rig.input.pointer_port();
    assert!(
        !port.handle_pointer_down(PointerButton::Left, -10, 300),
        "outside the image: not handled"
    );
    assert!(
        port.handle_pointer_down(PointerButton::Left, 500, 300),
        "on the image: handled"
    );
    assert!(rig.window.captured.get());
    rig.drain();
    let before = rig.offsets();
    port.handle_pointer_move(450, 300, true);
    rig.drain();
    assert_ne!(before, rig.offsets(), "the move pans");
    port.handle_left_button_up(450, 300);
    assert!(!rig.window.captured.get(), "the release ends the pan");
    rig.window.cursor.set(Some(ShellCursor::SizeAll));
    assert!(port.handle_pointer_down(PointerButton::Left, 500, 300));
    rig.window.cursor.set(Some(ShellCursor::SizeAll));
    port.handle_capture_lost();
    assert_eq!(
        Some(ShellCursor::Arrow),
        rig.window.cursor.get(),
        "a lost capture cancels the pan"
    );
}

// dispatch.rs 1169:9 and 1173:9.
#[test]
fn the_wheel_port_handles_the_wheel_over_the_viewport_only() {
    let rig = Rig::new();
    let port = rig.input.wheel_port();
    assert!(port.handle_vertical_wheel(120, false, PointD { x: 500.0, y: 300.0 }));
    rig.host.is_compare_visible.set(true);
    assert!(
        !port.handle_vertical_wheel(120, false, PointD { x: 500.0, y: 300.0 }),
        "compare visible"
    );
    rig.host.is_compare_visible.set(false);
    assert!(port.handle_horizontal_wheel(120, false, PointD { x: 500.0, y: 300.0 }));
    assert!(
        !port.handle_horizontal_wheel(120, false, PointD { x: 5000.0, y: 300.0 }),
        "outside the viewport"
    );
}

fn layouts(rig: &Rig) -> usize {
    calls(rig).iter().filter(|call| **call == "update_layout").count()
}

// dispatch.rs `VersionAdapter::next -> 0` (a constant never supersedes anything): a second Fit started before the first one resumed ends the first one
// at its next version check, so the first pass adds only its start layout.
#[test]
fn a_newer_fit_supersedes_the_pending_one() {
    let rig = Rig::new();
    clear_calls(&rig);
    rig.input.apply_fit();
    rig.drain();
    let single = layouts(&rig);
    assert!(single >= 2, "a Fit lays out at least twice: {single}");
    clear_calls(&rig);
    rig.input.apply_fit();
    rig.input.apply_fit();
    rig.drain();
    assert_eq!(single + 1, layouts(&rig), "{:?}", calls(&rig));
}
