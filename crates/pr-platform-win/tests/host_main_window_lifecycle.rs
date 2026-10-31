//! Tests of W8-13h (N): `MainWindow` change fan-out, layout helpers, closing, closed, `release_renderer` and `dispose`. Ports of the window-level twins of
//! `ShellStartupTests` / `ShellWindowingTests` (baseline `d92d6aae`) plus the new tests of the contract. The fan-out order is pinned without a window through
//! `fan_out_changes` (pure); everything else runs a real `MainWindow` on a real UI thread with a hidden (never shown, never activated) shell window.
//!
//! The native tests are `#[ignore = "native: needs desktop"]`. They build a whole `MainWindow`, whose constructor also runs the stubs of 13i, 13j and 13k:
//! they run as soon as those units are merged. Every wait is bounded; no sleeps.
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

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use main_window_fixture::{
    BuildOptions, EventLog, RecordingFeature, WarpHook, build, built, dispatcher, finish, new_order, run, start_ui,
    take_built, wait_until,
};
use pr_app::viewmodels::compare::CompareProperty;
use pr_app::viewmodels::main::{ChangeSet, ImageChangingEventArgs, MainProperty};
use pr_app::viewmodels::viewer_state::{ViewerChanges, ViewerProperty};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::host::contracts::{ShellWindowApi, ShellWindowEvent, new_rect};
use pr_platform_win::host::frame_clock::base::HostFrameClock;
use pr_platform_win::host::main_window::core::{MainWindowError, MainWindowOptions};
use pr_platform_win::host::main_window::lifecycle::{
    ChangeSteps, bitmap_size, fan_out_changes, fan_out_viewer, target_decode_box_for, view_model_wake,
};
use pr_platform_win::host::pending_panic;
use shell_window_fixture::{send_dpi_changed, window_rect};

// ---- pure: the fan-out ------------------------------------------------------------------------------------------------------------------------

/// A hook that runs inside `notify_features`.
type Reenter = Box<dyn Fn(&Steps)>;

/// A recording `ChangeSteps`.
struct Steps {
    log: RefCell<Vec<String>>,
    disposed: Cell<bool>,
    fit: Cell<bool>,
    /// Called from `notify_features` (a feature that re-enters).
    reenter: RefCell<Option<Reenter>>,
}

impl Steps {
    fn new() -> Self {
        Self {
            log: RefCell::new(Vec::new()),
            disposed: Cell::new(false),
            fit: Cell::new(true),
            reenter: RefCell::new(None),
        }
    }

    fn push(&self, step: &str) {
        self.log.borrow_mut().push(step.to_owned());
    }

    fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

impl ChangeSteps for Steps {
    fn is_disposed(&self) -> bool {
        self.disposed.get()
    }
    fn on_image_changing(&self, is_file_change: bool) {
        self.push(&format!("image_changing({is_file_change})"));
    }
    fn drain_pending_images(&self) {
        self.push("drain_images");
    }
    fn viewer_is_fit(&self) -> bool {
        self.fit.get()
    }
    fn post_update_fit_size(&self) {
        self.push("post_fit");
    }
    fn refresh_title(&self) {
        self.push("title");
    }
    fn on_navigation_changed(&self) {
        self.push("navigation");
    }
    fn notify_current_index(&self) {
        self.push("index_hooks");
    }
    fn on_viewer_fullscreen_changed(&self) {
        self.push("fullscreen");
    }
    fn on_viewer_changed(&self) {
        self.push("viewer");
    }
    fn on_compare_changed(&self) {
        self.push("compare");
    }
    fn notify_features(&self, _changes: &ChangeSet) {
        self.push("features");
        if let Some(reenter) = self.reenter.borrow().as_ref() {
            reenter(self);
        }
    }
}

fn everything() -> ChangeSet {
    let mut changes = ChangeSet::default();
    changes.main.insert(MainProperty::CurrentImage);
    changes.main.insert(MainProperty::FolderTitle);
    changes.main.insert(MainProperty::CurrentIndex);
    changes.viewer.insert(ViewerProperty::IsFullscreen);
    changes.viewer.insert(ViewerProperty::Zoom);
    changes.compare.insert(CompareProperty::IsVisible);
    changes.image_changing = Some(ImageChangingEventArgs { is_file_change: true });
    changes
}

// C#: the raise order of one view-model call: ImageChanging, the hub's ImageChanged (+ the posted fit update for CurrentImage), FolderTitle, CurrentIndex,
// the viewer (windowing handler of IsFullscreen subscribed in RegisterFeatures BEFORE the core's OnViewerChanged), compare, the features.
#[test]
fn apply_changes_fans_out_in_the_documented_order() {
    let steps = Steps::new();
    fan_out_changes(&steps, &everything());
    assert_eq!(
        steps.log(),
        [
            "image_changing(true)",
            "drain_images",
            "post_fit",
            "title",
            "navigation",
            "index_hooks",
            "fullscreen",
            "viewer",
            "compare",
            "features"
        ]
    );
}

#[test]
fn apply_changes_posts_the_fit_update_only_for_a_current_image_in_fit() {
    let steps = Steps::new();
    steps.fit.set(false);
    let mut changes = ChangeSet::default();
    changes.main.insert(MainProperty::CurrentImage);
    fan_out_changes(&steps, &changes);
    assert_eq!(steps.log(), ["drain_images", "features"]);

    let fit = Steps::new();
    let mut other = ChangeSet::default();
    other.main.insert(MainProperty::TotalFiles);
    fan_out_changes(&fit, &other);
    assert_eq!(
        fit.log(),
        ["drain_images", "features"],
        "no CurrentImage, no fit update"
    );
}

#[test]
fn apply_changes_with_an_empty_set_does_nothing_but_the_image_queue_and_the_feature_hook() {
    let steps = Steps::new();
    fan_out_changes(&steps, &ChangeSet::default());
    assert_eq!(steps.log(), ["drain_images", "features"]);
}

#[test]
fn apply_changes_after_dispose_is_a_no_op() {
    let steps = Steps::new();
    steps.disposed.set(true);
    fan_out_changes(&steps, &everything());
    let mut viewer = ViewerChanges::EMPTY;
    viewer.insert(ViewerProperty::IsFullscreen);
    fan_out_viewer(&steps, viewer);
    assert!(steps.log().is_empty());
}

#[test]
fn apply_viewer_changes_runs_the_viewer_step_only() {
    let steps = Steps::new();
    fan_out_viewer(&steps, ViewerChanges::EMPTY);
    assert!(steps.log().is_empty(), "an empty viewer set does nothing");

    let mut zoom = ViewerChanges::EMPTY;
    zoom.insert(ViewerProperty::Zoom);
    fan_out_viewer(&steps, zoom);
    assert_eq!(steps.log(), ["viewer"]);

    let mut fullscreen = ViewerChanges::EMPTY;
    fullscreen.insert(ViewerProperty::IsFullscreen);
    fan_out_viewer(&steps, fullscreen);
    assert_eq!(steps.log(), ["viewer", "fullscreen", "viewer"]);
}

// T1 (re-entrancy): a feature that applies an (empty) change set from its hook does not recurse for ever or panic.
#[test]
fn apply_changes_reentered_by_a_feature_does_not_recurse() {
    let steps = Steps::new();
    let depth = Rc::new(Cell::new(0));
    let inner_depth = Rc::clone(&depth);
    *steps.reenter.borrow_mut() = Some(Box::new(move |steps: &Steps| {
        if inner_depth.get() == 0 {
            inner_depth.set(1);
            fan_out_changes(steps, &ChangeSet::default());
        }
    }));
    fan_out_changes(&steps, &everything());
    assert_eq!(steps.log().iter().filter(|step| *step == "features").count(), 2);
}

// C#: UpdateTargetDecodeBox `width > 1 ? width : FallbackViewportWidth` (2200 x 1400), PreviewQualityMultiplier 1.15
#[test]
fn decode_box_uses_the_fallbacks_below_one_dip() {
    let expected_fallback =
        pr_imaging::adaptive_preview::calculate_target_decode_box(2200.0, 1400.0, 1.0, 1.15).unwrap();
    assert_eq!(target_decode_box_for(1.0, 1.0, 1.0).unwrap(), expected_fallback);
    assert_eq!(target_decode_box_for(0.0, -5.0, 1.0).unwrap(), expected_fallback);
    // Strictly greater than 1 DIP is measured (T8): 1.5 is used as it is, not replaced.
    let measured = pr_imaging::adaptive_preview::calculate_target_decode_box(1.5, 1400.0, 1.0, 1.15).unwrap();
    assert_eq!(target_decode_box_for(1.5, 1.0, 1.0).unwrap(), measured);
    assert_ne!(measured, expected_fallback);
    // Real sizes: DIP times dpi times 1.15, quantised by the policy.
    let box_150 = pr_imaging::adaptive_preview::calculate_target_decode_box(1200.0, 800.0, 1.5, 1.15).unwrap();
    assert_eq!(target_decode_box_for(1200.0, 800.0, 1.5).unwrap(), box_150);
    // An invalid dpi is an error (the window logs it and keeps the old box).
    assert!(target_decode_box_for(1200.0, 800.0, f64::NAN).is_err());
}

// C#: ContentFromViewer `image is { IsDisposed: false } ? image.Width : 0`
#[test]
fn content_from_viewer_reads_the_bitmap_only_when_alive() {
    assert_eq!(bitmap_size(None), (0.0, 0.0));
    let mut buffer = PixelBuffer::allocate(7, 3, PixelLayout::Pbgra32).unwrap();
    assert_eq!(bitmap_size(Some(&buffer)), (7.0, 3.0));
    buffer.dispose();
    assert_eq!(
        bitmap_size(Some(&buffer)),
        (0.0, 0.0),
        "a disposed bitmap counts as none"
    );
}

// ---- native helpers -------------------------------------------------------------------------------------------------------------------------------

/// (USER objects, GDI objects, kernel handles) of this process.
fn resource_counts() -> (u32, u32, u32) {
    use pr_platform_win::win32::dialog_native::{GR_GDIOBJECTS, GR_USEROBJECTS, kernel32, user32};
    let process = kernel32::get_current_process();
    let mut handles = 0_u32;
    // SAFETY: `handles` is a live local u32 that the OS fills in; the pseudo-handle of the current process is valid for the call.
    let _ = unsafe {
        windows::Win32::System::Threading::GetProcessHandleCount(
            windows::Win32::Foundation::HANDLE(process as *mut core::ffi::c_void),
            &mut handles,
        )
    };
    (
        user32::get_gui_resources(process, GR_USEROBJECTS as u32),
        user32::get_gui_resources(process, GR_GDIOBJECTS as u32),
        handles,
    )
}

// ---- native: the change entry point -------------------------------------------------------------------------------------------------------

// D-W8B4M-2: `update_view_model` releases the view-model borrow before the fan-out (a feature that borrows the view model in its hook does not panic).
#[test]
#[ignore = "native: needs desktop"]
fn update_view_model_releases_the_borrow_before_the_fan_out() {
    let ui = start_ui();
    let (seen_label, features_ran) = run(&ui, || {
        let b = build(BuildOptions::default());
        let log = new_order();
        let feature = RecordingFeature::new("f", Rc::clone(&log));
        let view_model = Rc::clone(b.window.view_model());
        let observed: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let slot = Rc::clone(&observed);
        *feature.on_changes_hook.borrow_mut() = Some(Box::new(move |_changes| {
            // `borrow_mut` would panic with `BorrowMutError` if the closure's borrow were still alive.
            let vm = view_model.borrow_mut();
            *slot.borrow_mut() = vm.instance_label().map(str::to_owned);
            drop(vm);
        }));
        b.window.add_feature(feature);
        b.window
            .update_view_model(|vm| vm.set_instance_label(Some("AGENT CHECK".to_owned())));
        let label = observed.borrow().clone();
        let ran = log.borrow().iter().any(|entry| entry == "f:on_changes");
        (label, ran)
    });
    assert_eq!(seen_label.as_deref(), Some("AGENT CHECK"));
    assert!(features_ran);
    finish(ui);
}

// Forwarded open while a modal command holds the view model (review ST-SHELL M1): `open_path` used to `borrow_mut` and panic (the dispatcher swallowed
// it, the image never opened). Now the path is queued and opened when the command returns. C# has no equivalent (its view model is not a RefCell).
#[test]
#[ignore = "native: needs desktop"]
fn open_path_during_a_modal_command_is_queued_and_opened_after_it_returns() {
    let ui = start_ui();
    let (status_inside, status_after) = run(&ui, || {
        let b = build(BuildOptions::default());
        let mut inside = String::new();
        // The closure runs with the view model mutably borrowed, like `show_settings`. A path that is not a valid open input answers with the
        // "no valid input" status, which proves the queued call ran.
        b.window.update_view_model(|vm| {
            b.window.open_path("?:\no-such-photo.jpg");
            inside = vm.status_text();
            vm.set_instance_label(None)
        });
        let after = b.window.view_model().borrow().status_text();
        (inside, after)
    });
    assert!(
        status_inside.is_empty(),
        "nothing is opened while the command still holds the view model"
    );
    assert!(
        !status_after.is_empty(),
        "the queued path was opened after the modal command returned"
    );
    finish(ui);
}

// T1 (re-entrancy) through the real window: a feature applies an empty set from its hook.
#[test]
#[ignore = "native: needs desktop"]
fn apply_changes_on_the_window_reentered_by_a_feature_does_not_recurse() {
    let ui = start_ui();
    let calls = run(&ui, || {
        let b = build(BuildOptions::default());
        let log = new_order();
        let feature = RecordingFeature::new("f", Rc::clone(&log));
        let window_inner = built();
        let reentered = Rc::new(Cell::new(false));
        let flag = Rc::clone(&reentered);
        *feature.on_changes_hook.borrow_mut() = Some(Box::new(move |_changes| {
            if !flag.replace(true) {
                window_inner.window.apply_changes(&ChangeSet::default());
            }
        }));
        b.window.add_feature(feature);
        b.window.apply_changes(&ChangeSet::default());
        log.borrow().iter().filter(|entry| *entry == "f:on_changes").count()
    });
    assert_eq!(calls, 2);
    finish(ui);
}

// C#: OnViewModelPropertyChanged(FolderTitle) -> SetTitle; C#: ShellWindowingTests.Title_FollowsTheViewModelAndCarriesTheInstanceLabel
#[test]
#[ignore = "native: needs desktop"]
fn title_follows_the_view_model_and_carries_the_instance_label() {
    let ui = start_ui();
    let (with_label, plain_vm_title, cleared, cleared_vm_title) = run(&ui, || {
        let b = build(BuildOptions::default());
        b.window
            .update_view_model(|vm| vm.set_instance_label(Some("AGENT CHECK".to_owned())));
        let with_label = b.title();
        let plain_vm_title = b.window.view_model().borrow().folder_title().to_owned();
        b.window.update_view_model(|vm| vm.set_instance_label(None));
        (
            with_label,
            plain_vm_title,
            b.title(),
            b.window.view_model().borrow().folder_title().to_owned(),
        )
    });
    assert_eq!(with_label, plain_vm_title);
    assert!(with_label.starts_with("[AGENT CHECK] "), "{with_label}");
    assert_eq!(cleared, cleared_vm_title);
    assert!(!cleared.starts_with('['));
    finish(ui);
}

// D-W8B4M-6: the inbox wake of the view model reaches the window; two wakes while one is queued give one pump.
#[test]
#[ignore = "native: needs desktop"]
fn view_model_wake_coalesces_and_drops_when_no_window() {
    let ui = start_ui();
    // No window yet: the wake is dropped (no panic), the flag clears again.
    run(&ui, || {
        let wake = view_model_wake(dispatcher());
        wake();
        wake();
    });
    wait_until(&ui, "the first wake to be served", || {
        !dispatcher().has_high_priority_work()
    });
    run(&ui, || {
        let b = build(BuildOptions::default());
        b.window.add_feature(RecordingFeature::new("w", Rc::clone(&b.order)));
        let wake = view_model_wake(dispatcher());
        wake();
        wake();
        wake();
        // Not run inline.
        assert_eq!(log_count(&b.order, "w:on_changes"), 0);
    });
    wait_until(&ui, "the wake to be served", || !dispatcher().has_high_priority_work());
    let pumps = run(&ui, || log_count(&built().order, "w:on_changes"));
    assert_eq!(pumps, 1, "three wakes while one was queued: one pump, one fan-out");
    finish(ui);
}

fn log_count(log: &EventLog, entry: &str) -> usize {
    log.borrow().iter().filter(|known| *known == entry).count()
}

// A pump that arrives while another runs (a nested message loop inside a handler) returns at once.
#[test]
#[ignore = "native: needs desktop"]
fn pump_view_model_ignores_a_re_entrant_call() {
    let ui = start_ui();
    let (pumps, order) = run(&ui, || {
        let b = build(BuildOptions::default());
        let log = new_order();
        let feature = RecordingFeature::new("p", Rc::clone(&log));
        let handle = built();
        let reentered = Rc::new(Cell::new(false));
        let flag = Rc::clone(&reentered);
        *feature.on_changes_hook.borrow_mut() = Some(Box::new(move |_changes| {
            if !flag.replace(true) {
                // The nested pump returns at once; the running pump goes round once more instead.
                handle.window.pump_view_model();
            }
        }));
        b.window.add_feature(feature);
        b.window.pump_view_model();
        (log_count(&log, "p:on_changes"), log.borrow().clone())
    });
    // First round (the nested call is refused) + one more round asked for by the refused call.
    assert_eq!(pumps, 2, "{order:?}");
    finish(ui);
}

// ---- native: closing, closed, dispose ------------------------------------------------------------------------------------------------------

// C#: ShellStartupTests.Closing_ReleasesTheRenderSurface_WhileTheWindowIsStillAlive (a Closing handler subscribed AFTER the window's runs after it)
#[test]
#[ignore = "native: needs desktop"]
fn closing_releases_the_render_surface_while_the_window_is_still_alive() {
    let ui = start_ui();
    let (released, alive, closing_flag, order) = run(&ui, || {
        let b = build(BuildOptions::default());
        b.window.attach_renderer(Some(&WarpHook::new(10, 10))).expect("attach");
        assert!(b.window.has_surface());
        let observed: Rc<RefCell<Option<(bool, bool, bool)>>> = Rc::new(RefCell::new(None));
        let slot = Rc::clone(&observed);
        let handle = built();
        b.shell.subscribe_closing(Box::new(move |_args| {
            *slot.borrow_mut() = Some((
                handle.window.has_surface() || handle.window.image_cache().is_some(),
                !handle.shell.is_destroyed(),
                handle.window.is_closing_or_closed(),
            ));
        }));
        b.shell.close();
        let seen = observed.borrow().expect("Closing was raised");
        (!seen.0, seen.1, seen.2, b.order.borrow().clone())
    });
    assert!(released, "the surface and the cache are released when Closing starts");
    assert!(alive, "the HWND is still alive in Closing (T4)");
    assert!(closing_flag, "IsClosingOrClosed is set before the renderer is released");
    assert!(
        order.contains(&"cloak.stop".to_owned()),
        "Closed stops the cloak: {order:?}"
    );
    finish(ui);
}

// C#: ShellStartupTests.Close_WhileAFileActionHoldsTheGate_KeepsTheWindowUntilTheActionIsDone
#[test]
#[ignore = "native: needs desktop"]
fn close_while_a_file_action_holds_the_gate_keeps_the_window_until_the_action_is_done() {
    let ui = start_ui();
    // The gate is held by an exclusive turn the test owns (the real C# test held it with a blocking confirm dialog).
    let (kept_open, closing_after_request, destroyed_after_request) = run(&ui, || {
        let b = build(BuildOptions::default());
        let turn: Rc<RefCell<Option<pr_app::coordinators::file_action_gate::GateTurn>>> = Rc::new(RefCell::new(None));
        let slot = Rc::clone(&turn);
        assert!(b.gate.run_exclusive(move |held| *slot.borrow_mut() = Some(held)));
        assert!(b.window.view_model().borrow().is_file_action_in_progress());
        TURN.with(|cell| *cell.borrow_mut() = turn.borrow_mut().take());
        b.shell.close(); // WM_CLOSE equivalent: Closing is raised, the handler cancels
        let first = (
            !b.shell.is_destroyed(),
            b.window.is_closing_or_closed(),
            b.shell.is_destroyed(),
        );
        b.shell.close(); // a second request while waiting: no second waiter
        (first.0, first.1, first.2)
    });
    assert!(kept_open, "the window is kept");
    assert!(
        !closing_after_request,
        "IsClosingOrClosed stays false while the gate is held (R7-7)"
    );
    assert!(!destroyed_after_request);

    // The action is done: the turn ends, the gate posts its waiter through its scheduler, the waiter closes the window.
    let posted = run(&ui, || {
        TURN.with(|cell| cell.borrow_mut().take()).expect("the turn").finish();
        built().scheduler.run_pending()
    });
    assert!(posted >= 1, "the gate posted its waiter");
    wait_until(&ui, "the window to close by itself", || {
        built().window.is_closing_or_closed() && built().shell.is_destroyed()
    });
    finish(ui);
}

thread_local! {
    static TURN: RefCell<Option<pr_app::coordinators::file_action_gate::GateTurn>> = const { RefCell::new(None) };
}

// C#: ShellStartupTests.DpiChange_WithTheSameDipSize_StillUpdatesTheViewerScaleAndDecodeBoxInTheSameTurn
// (and ShellWindowingTests.DpiChange_UpdatesViewerScaleAndDecodeBoxInTheSameTurn)
#[test]
#[ignore = "native: needs desktop"]
fn dpi_change_with_the_same_dip_size_still_updates_the_viewer_scale_and_decode_box_in_the_same_turn() {
    let ui = start_ui();
    let size_events = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&size_events);
    let (before_scale, after_scale, window_scale, decode_box, expected_box) = run(&ui, move || {
        let b = build(BuildOptions {
            window: pr_platform_win::host::shell_window::ShellWindowOptions {
                width_dip: 400.0,
                height_dip: 300.0,
                ..Default::default()
            },
            ..BuildOptions::default()
        });
        let hwnd = b.shell.hwnd();
        let before_scale = b.window.view_model().borrow().viewer().borrow().dpi_scale();
        let client = b.shell.client_size_pixels();
        let outer = window_rect(hwnd);
        let (frame_w, frame_h) = (
            (outer.right - outer.left) - client.0,
            (outer.bottom - outer.top) - client.1,
        );
        // A monitor with twice the dpi: the suggested rect has twice the client pixels, so the client size in DIP does NOT change and only DpiChanged fires.
        let suggested = new_rect(
            outer.left,
            outer.top,
            outer.left + frame_w + client.0 * 2,
            outer.top + frame_h + client.1 * 2,
        );
        b.shell.subscribe(
            ShellWindowEvent::ClientSizeChanged,
            Box::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        let new_dpi = (before_scale * 96.0 * 2.0).round() as i32;
        send_dpi_changed(hwnd, new_dpi, suggested);
        let after_scale = b.window.view_model().borrow().viewer().borrow().dpi_scale();
        let (vw, vh) = b.window.viewport().viewport_size();
        (
            before_scale,
            after_scale,
            b.shell.dpi_scale(),
            b.size_source.target_decode_box(),
            target_decode_box_for(vw, vh, b.shell.dpi_scale()).unwrap(),
        )
    });
    assert_eq!(
        size_events.load(Ordering::SeqCst),
        0,
        "the DIP size is unchanged: no ClientSizeChanged"
    );
    assert!(
        (after_scale - before_scale * 2.0).abs() < 1e-9,
        "{before_scale} -> {after_scale}"
    );
    assert_eq!(after_scale, window_scale);
    assert_eq!(
        decode_box, expected_box,
        "the decode box follows the new dpi in the same turn"
    );
    assert_ne!(decode_box, DecodeBox::UNBOUNDED);
    finish(ui);
}

// T7 and the C# `_disposed` rule: dispose twice, drop after dispose.
#[test]
#[ignore = "native: needs desktop"]
fn dispose_is_idempotent_and_unsubscribes_everything() {
    let ui = start_ui();
    let (order, subscribers, stops, has_surface, destroyed) = run(&ui, || {
        let b = build(BuildOptions::default());
        b.window.attach_renderer(Some(&WarpHook::new(10, 10))).expect("attach");
        let log = Rc::clone(&b.order);
        b.window.add_feature(RecordingFeature::new("one", Rc::clone(&log)));
        b.window.add_feature(RecordingFeature::new("two", Rc::clone(&log)));
        b.window.dispose();
        b.window.dispose();
        drop(b);
        let handle = take_built();
        let hub = Rc::clone(&handle.hub);
        let cloak = Rc::clone(&handle.cloak);
        let shell = Rc::clone(&handle.shell);
        let has_surface = handle.window.has_surface();
        drop(handle); // Drop after dispose does nothing more
        (
            log.borrow().clone(),
            hub.subscriber_count(),
            cloak.stops.get(),
            has_surface,
            shell.is_destroyed(),
        )
    });
    // Features are disposed in reverse registration order, once; then the renderer is released (features first).
    let tail: Vec<&str> = order
        .iter()
        .map(String::as_str)
        .skip_while(|entry| *entry != "hub.unsubscribe")
        .collect();
    assert_eq!(
        tail,
        [
            "hub.unsubscribe",
            "two:dispose",
            "one:dispose",
            "cloak.stop",
            "two:release_renderer",
            "one:release_renderer",
            "one:drop",
            "two:drop"
        ]
    );
    assert_eq!(subscribers, 0);
    assert_eq!(stops, 1, "the second dispose does nothing");
    assert!(!has_surface);
    assert!(destroyed);
    finish(ui);
}

// Dropping the window while the thread unwinds must not resume a captured panic (that would abort the process).
#[test]
#[ignore = "native: needs desktop"]
fn dispose_while_panicking_does_not_resume_the_captured_panic() {
    let ui = start_ui();
    let (unwound, still_pending) = run(&ui, || {
        build(BuildOptions::default());
        let handle = take_built();
        pending_panic::capture(Box::new("captured in the window procedure"));
        let outcome = catch_unwind(AssertUnwindSafe(move || {
            let _window = handle;
            panic!("test unwinding");
        }));
        let still_pending = pending_panic::has_pending();
        // Clear the slot so the fixture's own cleanup does not resume it.
        let _ = catch_unwind(pending_panic::rethrow_pending);
        (outcome.is_err(), still_pending)
    });
    assert!(unwound);
    assert!(
        still_pending,
        "the captured panic was left in its slot, not resumed during unwinding"
    );
    finish(ui);
}

// C#: OnClosed: IsClosingOrClosed, cloak.Stop(), CloseSession(), preload controller stopped
#[test]
#[ignore = "native: needs desktop"]
fn closed_stops_the_cloak_and_closes_the_session_and_stops_the_preload() {
    let ui = start_ui();
    let (stops, closed, preload_events, closing) = run(&ui, || {
        let b = build(BuildOptions::default());
        b.shell.close();
        (
            b.cloak.stops.get(),
            b.window.view_model().borrow().is_closed(),
            b.preload.events(),
            b.window.is_closing_or_closed(),
        )
    });
    assert!(stops >= 1, "the cloak is stopped when the window closes");
    assert!(closed, "the session is closed");
    assert!(
        preload_events.iter().any(|event| event == "cancel"),
        "{preload_events:?}"
    );
    assert!(closing);
    finish(ui);
}

// C#: SavePlacement runs in Closing BEFORE the renderer is released (the R7-10 geometry of F11 is 13k)
#[test]
#[ignore = "native: needs desktop"]
fn closing_saves_the_placement_through_the_port_before_the_renderer_is_released() {
    let ui = start_ui();
    let (saves, order, hwnd) = run(&ui, || {
        let b = build(BuildOptions {
            options: MainWindowOptions {
                placement_file: Some(std::env::temp_dir().join("pr-13h-placement-never-written.json")),
                prefer_warp: true,
                activate: false,
                ..MainWindowOptions::default()
            },
            ..BuildOptions::default()
        });
        b.window.add_feature(RecordingFeature::new("f", Rc::clone(&b.order)));
        let hwnd = b.shell.hwnd();
        b.shell.close();
        (b.placement.saves.borrow().clone(), b.order.borrow().clone(), hwnd)
    });
    assert_eq!(saves.len(), 1);
    assert_eq!(saves[0].hwnd, hwnd);
    assert!(saves[0].file.ends_with("pr-13h-placement-never-written.json"));
    assert_eq!(saves[0].restore_state, None, "not in F11: nothing to restore");
    assert_eq!(saves[0].restore_bounds, None);
    let save_at = order.iter().position(|entry| entry == "placement.save").unwrap();
    let release_at = order.iter().position(|entry| entry == "f:release_renderer").unwrap();
    assert!(save_at < release_at, "{order:?}");
    finish(ui);
}

// C#: IForwardedOpenTarget.Activate on a destroyed window throws InvalidOperationException("The main window is already destroyed.")
#[test]
#[ignore = "native: needs desktop"]
fn activate_on_a_destroyed_window_is_an_error() {
    let ui = start_ui();
    let (result, minimized) = run(&ui, || {
        let b = build(BuildOptions::default());
        b.shell.close();
        (b.window.activate(), b.window.is_minimized())
    });
    assert_eq!(result, Err(MainWindowError::WindowDestroyed));
    assert!(!minimized, "a hidden window is not iconic");
    finish(ui);
}

// T5: handlers subscribed after the window's own run after it (Closing), and the core's Closed runs before a later one.
#[test]
#[ignore = "native: needs desktop"]
fn closing_handlers_run_in_subscription_order() {
    let ui = start_ui();
    let order = run(&ui, || {
        let b = build(BuildOptions::default());
        b.window.attach_renderer(Some(&WarpHook::new(10, 10))).expect("attach");
        b.window.add_feature(RecordingFeature::new("f", Rc::clone(&b.order)));
        let closing_log = Rc::clone(&b.order);
        b.shell.subscribe_closing(Box::new(move |_args| {
            closing_log.borrow_mut().push("startup.closing".to_owned())
        }));
        let closed_log = Rc::clone(&b.order);
        b.shell.subscribe(
            ShellWindowEvent::Closed,
            Box::new(move || closed_log.borrow_mut().push("startup.closed".to_owned())),
        );
        b.shell.close();
        b.order.borrow().clone()
    });
    let at = |name: &str| {
        order
            .iter()
            .position(|entry| entry == name)
            .unwrap_or_else(|| panic!("{name} in {order:?}"))
    };
    assert!(
        at("f:release_renderer") < at("startup.closing"),
        "the core's on_closing runs first: {order:?}"
    );
    assert!(
        at("cloak.stop") < at("startup.closed"),
        "the core's on_closed runs first: {order:?}"
    );
    finish(ui);
}

/// One build / attach / close / drop cycle (UI thread).
fn one_cycle() {
    let b = build(BuildOptions::default());
    b.window.attach_renderer(Some(&WarpHook::new(10, 10))).expect("attach");
    b.shell.close();
    drop(b);
    drop(take_built());
    end_fixture_window();
}

/// The fixture's own part of a cycle: a bare hidden shell window and its timer clock (their cost is the baseline of the leak test).
fn bare_window_cycle() {
    drop(shell_window_fixture::create_window(Default::default()));
    end_fixture_window();
}

fn end_fixture_window() {
    // The fixture's timer clock owns a kernel handle: dispose it like the 13f leak test does.
    shell_window_fixture::clock().dispose();
    shell_window_fixture::dispose_window();
}

// Twin of the window half of `Startup_ShutdownIsClean_NoWindowDispatcherOrGuiObjectLeak`: 20 build/close cycles keep the GUI and handle counts bounded.
// The kernel handle count is compared with 20 cycles of a BARE shell window (the 13e/13f window and clock cost one handle per cycle on their own, which is
// not this unit's): the main window must add nothing on top. The process handle count is noisy (runs differed by up to 7 over 20 cycles), so the bound is
// loose: it catches a leak of a handle or more per cycle only when it comes ON TOP of the bare window's; the strict checks are the USER and GDI counts.
#[test]
#[ignore = "native: needs desktop"]
fn repeated_open_close_cycles_do_not_grow_gui_or_handle_counts() {
    let ui = start_ui();
    run(&ui, one_cycle); // warm-up: class registration, static caches
    run(&ui, bare_window_cycle);
    let baseline_start = run(&ui, resource_counts);
    for _ in 0..20 {
        run(&ui, bare_window_cycle);
    }
    let baseline_end = run(&ui, resource_counts);
    let before = run(&ui, resource_counts);
    for _ in 0..20 {
        run(&ui, one_cycle);
    }
    let after = run(&ui, resource_counts);
    assert!(
        i64::from(after.0) - i64::from(before.0) <= 2,
        "USER objects {} -> {}",
        before.0,
        after.0
    );
    assert!(
        i64::from(after.1) - i64::from(before.1) <= 2,
        "GDI objects {} -> {}",
        before.1,
        after.1
    );
    let bare_growth = i64::from(baseline_end.2) - i64::from(baseline_start.2);
    let growth = i64::from(after.2) - i64::from(before.2);
    assert!(
        growth <= bare_growth + 12,
        "handles {} -> {} over 20 cycles; a bare window grows by {bare_growth}",
        before.2,
        after.2
    );
    finish(ui);
}
