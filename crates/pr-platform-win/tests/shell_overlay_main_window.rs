//! Tests of W8-15j: `MainWindowOverlay` (`ShellMainWindow.Overlay.cs`, baseline `d92d6aae`) and `ViewModelOverlayCommands`.
//!
//! The C# file has no unit test of its own; `ShellOverlayIntegrationTests` (a whole shell on WARP) covers it. Their LOGIC is ported here with fakes of the
//! window, the view model, the poster and the wake (every test is marked `// new:` with the C# test it stands in for); two of them also run against a real
//! hidden `ShellWindow` (`#[ignore = "native: needs desktop"]`). The other integration tests need the full shell and belong to W8-20b.
//!
//! The four C# tests about commands that THROW (`ToolsMenu_WhenACommandThrowsSynchronously_...`, `FailedAsyncCommand_...`, `SynchronouslyThrowingCommand_...`)
//! have no Rust twin: the command methods are infallible (contract W8-15j item 7, "none caught here"); the failure channel that remains is the Tools menu
//! (`menu_error_reaches_report_action_failed_as_toolsmenu`) and `on_command_failed`.
#![cfg(windows)]

#[path = "common/overlay_fakes.rs"]
mod overlay_fakes;
#[path = "common/overlay_shell_fixture.rs"]
mod overlay_shell_fixture;
#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use overlay_fakes::RecordingDrawContext;
use overlay_shell_fixture::{CountingWake, QueuePoster, Rig, RigOptions, ShellTips, ShellVm, only, timeline, xy};
use pr_app::input::primitives::PointD;
use pr_app::overlay::tools_menu::OverlayCommands;
use pr_app::viewmodels::compare::CompareProperty;
use pr_app::viewmodels::info_overlay::InfoOverlayProperty;
use pr_app::viewmodels::main::state::{ChangeSet, MainProperty};
use pr_app::viewmodels::viewer_state::ViewerProperty;
use pr_core::abstractions::UiPriority;
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_platform_win::host::contracts::{ShellWindowApi, ShellWindowEvent, WindowMessage, WindowMessageHandler};
use pr_platform_win::host::frame_clock::HostFrameClock;
use pr_platform_win::host::main_window::core::MainWindowFeature;
use pr_platform_win::shell_overlay::main_window::{
    MainWindowOverlay, OverlayDeps, OverlayPending, OverlayViewModel, OverlayWindowPort, UiWake,
    ViewModelOverlayCommands, client_origin_dip,
};
use pr_platform_win::shell_overlay::native::{OverlayOs, Win32OverlayOs};
use pr_platform_win::shell_overlay::overlay::state::OverlayButton;
use pr_platform_win::shell_overlay::tooltips::{LocalPoster, OverlayTooltips, TooltipFactory};
use pr_platform_win::win32::messages::{WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE};
use pr_platform_win::win32::structs::Lresult;
use shell_window_fixture::{client_origin, create_window, dispose_window, send, send_dpi_changed, window, window_rect};
use ui_thread::UiThread;

fn navigation(property: MainProperty) -> ChangeSet {
    let mut changes = ChangeSet::default();
    changes.main.insert(property);
    changes
}

fn settings_with_opacity(rig: &Rig, percent: i32) {
    let store = rig.store.as_ref().expect("rig with a store");
    let mut settings = (*store.current()).clone();
    settings.toolbar_opacity_percent = percent;
    store.save(settings).expect("save");
}

/// Answers every message with a fixed value (a handler that was in the chain before the overlay).
struct Always(Lresult);

impl WindowMessageHandler for Always {
    fn try_handle(&self, _message: &WindowMessage) -> Option<Lresult> {
        Some(self.0)
    }
}

// ---- attach ----

// new: ToolbarIsDrawnFromTheFirstFrame_AtTheTopLeft_AndGoneAfterClose (the model is read once, before any frame)
#[test]
fn attach_updates_the_overlay_with_the_view_model_model_once() {
    let rig = Rig::new();
    assert_eq!(rig.vm.model_reads.get(), 1);
    assert_eq!(rig.ov().overlay().model().status_text, "1 / 3");
    assert!(rig.ov().overlay().model().has_images);
}

// new: RealMouseMessages_ClickTheFitButton_AndTheOverlaySwallowsThePress (the handler is in FRONT of what was there)
#[test]
fn attach_inserts_the_message_handler_first() {
    let rig = Rig::with(RigOptions {
        existing_handler: Some(Rc::new(Always(99))),
        ..RigOptions::default()
    });
    assert_eq!(
        only(
            &rig.timeline,
            &["vm.overlay_model", "window.add_handler", "window.insert_first"]
        ),
        ["window.add_handler", "vm.overlay_model", "window.insert_first"],
        "the model is applied BEFORE the handler goes in, and the handler is inserted, not added"
    );
    assert_eq!(rig.window.chain.borrow().len(), 2);
    rig.draw();
    let fit = rig.bounds_of(OverlayButton::Fit);
    let (x, y) = ((fit.x + fit.width / 2.0) as i16, (fit.y + fit.height / 2.0) as i16);
    assert_eq!(
        rig.window.dispatch(WM_LBUTTONDOWN, 1, xy(x, y)),
        Some(0),
        "the overlay answers first"
    );
    assert_eq!(rig.window.dispatch(WM_LBUTTONUP, 0, xy(x, y)), Some(0));
    assert_eq!(
        rig.window.dispatch(WM_MOUSEMOVE, 0, xy(900, 650)),
        Some(99),
        "a message the overlay does not take falls through to the older handler"
    );
}

// new: Close_UnsubscribesTheOverlayFromTheStaticLocalizerEvent / Close_UnsubscribesFromTheSettingsStoreAndTheViewModel (window part)
#[test]
fn attach_and_dispose_balance_the_window_subscriptions() {
    let rig = Rig::new();
    assert_eq!(rig.window.subscriber_count(), 2);
    rig.ov().dispose();
    assert_eq!(rig.window.subscriber_count(), 0);
    assert_eq!(rig.window.unsubscribe_calls.get(), 2);
}

// new: (the Activated / Deactivated wiring of AttachOverlay)
#[test]
fn window_activated_counts_as_activity_and_is_unsubscribed_after_dispose() {
    let rig = Rig::new();
    rig.settle();
    let before = rig.clock.frame_requests.get();
    rig.window.fire(ShellWindowEvent::Activated);
    assert_eq!(rig.clock.frame_requests.get(), before + 1);
    rig.window.fire(ShellWindowEvent::Deactivated); // must not panic
    rig.ov().dispose();
    rig.window.fire(ShellWindowEvent::Activated);
    assert_eq!(rig.clock.frame_requests.get(), before + 1, "nobody listens any more");
}

// new: (DeferredTooltips are posted at Background priority, the control is created by that job)
#[test]
fn deferred_tooltips_are_created_at_background_priority() {
    let rig = Rig::new();
    assert_eq!(rig.poster.posted.borrow().as_slice(), [UiPriority::Background]);
    assert_eq!(rig.tips.sync_count.get(), 0, "no control yet");
    rig.draw();
    assert_eq!(
        rig.tips.sync_count.get(),
        0,
        "the frame before the job only remembers the regions"
    );
    rig.poster.run_all();
    assert_eq!(
        rig.tips.sync_count.get(),
        1,
        "the job creates the control and hands it the pending list"
    );
}

// ---- drawing ----

// new: ToolbarIsDrawnFromTheFirstFrame_AtTheTopLeft_AndGoneAfterClose
#[test]
fn draw_overlay_arranges_with_the_window_size_and_dpi_then_draws() {
    let rig = Rig::new();
    rig.window.dpi.set(1.5);
    rig.window.size.set(pr_app::input::primitives::SizeD {
        width: 1000.0,
        height: 700.0,
    });
    rig.poster.run_all();
    let dc = rig.draw();
    assert!(!dc.calls.is_empty(), "something was drawn");
    assert!(!dc.fills.is_empty());
    let fit = rig.bounds_of(OverlayButton::Fit);
    assert!(
        fit.x > 0.0 && fit.x + fit.width <= 1000.0,
        "arranged inside the window: {fit:?}"
    );
    assert!(fit.y + fit.height <= 700.0);
    assert_eq!(
        rig.tips.dpi.get(),
        1.5,
        "the tooltip regions were synchronised with the window's DPI"
    );
}

// ---- view-model change sets ----

// new: (OnOverlaySourceChanged: CurrentIndex / CurrentImage are activity)
#[test]
fn navigation_change_set_is_activity() {
    let rig = Rig::new();
    rig.settle();
    for (round, property) in [MainProperty::CurrentIndex, MainProperty::CurrentImage]
        .into_iter()
        .enumerate()
    {
        let before = rig.clock.frame_requests.get();
        rig.ov().on_view_model_changed(&navigation(property));
        assert_eq!(
            rig.clock.frame_requests.get(),
            before + 1,
            "{property:?} restarts the info overlay's countdown"
        );
        assert_eq!(rig.poster.waiting(), [UiPriority::Render], "round {round}");
        rig.poster.run_all();
    }
}

// new: (OnOverlaySourceChanged for the three sources: main, InfoOverlay, Compare)
#[test]
fn non_navigation_change_set_only_marks_dirty() {
    let rig = Rig::new();
    rig.settle();
    let mut sets = Vec::new();
    sets.push(navigation(MainProperty::StatusText));
    let mut info = ChangeSet::default();
    info.overlay.insert(InfoOverlayProperty::FontSize);
    sets.push(info);
    let mut compare = ChangeSet::default();
    compare.compare.insert(CompareProperty::IsVisible);
    sets.push(compare);
    for changes in &sets {
        let before = rig.clock.frame_requests.get();
        rig.ov().on_view_model_changed(changes);
        assert_eq!(rig.clock.frame_requests.get(), before, "not a navigation: no activity");
        assert_eq!(rig.poster.waiting(), [UiPriority::Render]);
        rig.poster.run_all();
    }
}

// new: (the C# overlay did not listen to the viewer)
#[test]
fn viewer_only_change_set_does_nothing() {
    let rig = Rig::new();
    rig.settle();
    let mut changes = ChangeSet::default();
    changes.viewer.insert(ViewerProperty::Zoom);
    changes.catalog_changed = true;
    let before = rig.clock.frame_requests.get();
    rig.ov().on_view_model_changed(&changes);
    rig.ov().on_view_model_changed(&ChangeSet::default());
    assert!(rig.poster.waiting().is_empty());
    assert_eq!(rig.clock.frame_requests.get(), before);
    assert!(rig.timeline.borrow().is_empty());
}

// ---- dirty coalescing ----

// new: (MarkOverlayModelDirty: one Render-priority post, many changes give one update)
#[test]
fn model_dirty_is_coalesced_and_posted_at_render_priority() {
    let rig = Rig::new();
    rig.settle();
    let reads = rig.vm.model_reads.get();
    for _ in 0..3 {
        rig.ov().mark_model_dirty();
    }
    assert_eq!(rig.poster.waiting(), [UiPriority::Render], "one post for three changes");
    rig.vm.model.borrow_mut().status_text = "2 / 3".to_owned();
    rig.poster.run_all();
    assert_eq!(
        rig.vm.model_reads.get(),
        reads + 1,
        "one update, with the model read at that time"
    );
    assert_eq!(rig.ov().overlay().model().status_text, "2 / 3");
    rig.ov().mark_model_dirty();
    assert_eq!(
        rig.poster.waiting(),
        [UiPriority::Render],
        "after the job ran a new change posts again"
    );
}

// new: (RefreshOverlayModel clears the flag FIRST so a change made inside the update can post again)
#[test]
fn refresh_clears_the_flag_before_updating() {
    let rig = Rig::new();
    rig.settle();
    let weak = Rc::downgrade(&rig.ov());
    rig.on_next_invalidate(move || {
        if let Some(overlay) = weak.upgrade() {
            overlay.mark_model_dirty();
        }
    });
    rig.ov().mark_model_dirty();
    assert!(rig.poster.run_next());
    assert_eq!(
        rig.poster.waiting(),
        [UiPriority::Render],
        "the render request made inside the update posted a second refresh"
    );
}

// ---- cross-thread hand-over ----

// new: SettingsSave_ReevaluatesAutoHide_AndTheToolbarFollowsTheMouse (SettingsStore.Changed from another thread)
#[test]
fn settings_saved_from_another_thread_wait_for_run_pending() {
    let rig = Rig::with(RigOptions {
        store: true,
        ..RigOptions::default()
    });
    rig.settle();
    let store = Arc::clone(rig.store.as_ref().unwrap());
    std::thread::spawn(move || {
        let mut settings = (*store.current()).clone();
        settings.toolbar_opacity_percent = 55;
        store.save(settings).expect("save");
    })
    .join()
    .unwrap();
    assert_eq!(rig.wake.count(), 1, "the handler woke the UI thread once");
    assert!(
        rig.vm.set_settings_calls.borrow().is_empty(),
        "nothing runs on the saving thread"
    );
    assert!(rig.timeline.borrow().is_empty());
    assert!(rig.poster.waiting().is_empty());

    rig.ov().run_pending();
    let calls = rig.vm.set_settings_calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].toolbar_opacity_percent, 55);
    drop(calls);
    assert_eq!(rig.poster.waiting(), [UiPriority::Render], "the model is refreshed too");
}

// new: (latest wins; older settings are dropped)
#[test]
fn only_the_latest_pending_settings_are_applied() {
    let rig = Rig::with(RigOptions {
        store: true,
        ..RigOptions::default()
    });
    rig.settle();
    settings_with_opacity(&rig, 55);
    settings_with_opacity(&rig, 65);
    assert_eq!(rig.wake.count(), 2);
    rig.ov().run_pending();
    let calls = rig.vm.set_settings_calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].toolbar_opacity_percent, 65);
}

// new: SettingsSave_ReevaluatesAutoHide (the view model must see the new settings before the overlay re-evaluates)
#[test]
fn run_pending_applies_view_model_settings_before_the_overlay_reevaluates() {
    let rig = Rig::with(RigOptions {
        store: true,
        ..RigOptions::default()
    });
    rig.settle();
    settings_with_opacity(&rig, 55);
    rig.ov().run_pending();
    let log = rig.timeline.borrow().clone();
    let set = log
        .iter()
        .position(|e| e == "vm.set_settings")
        .expect("set_settings ran");
    let read = log
        .iter()
        .position(|e| e == "vm.settings")
        .expect("the overlay re-read the settings");
    assert!(set < read, "set_settings must come first: {log:?}");
}

// new: (run_pending drains the settings first, then the language flag)
#[test]
fn run_pending_handles_settings_before_language() {
    let rig = Rig::with(RigOptions {
        store: true,
        ..RigOptions::default()
    });
    rig.settle();
    Localizer::set_current(built_in_english_localizer());
    settings_with_opacity(&rig, 55);
    assert_eq!(rig.wake.count(), 2);
    rig.ov().run_pending();
    assert_eq!(
        only(&rig.timeline, &["vm.set_settings", "vm.refresh_text"]),
        ["vm.set_settings", "vm.refresh_text"]
    );
    rig.timeline.borrow_mut().clear();
    rig.ov().run_pending();
    assert!(rig.timeline.borrow().is_empty(), "both slots were taken");
}

// new: LanguageSwitch_UpdatesToolbarTextOnTheUiThread / LanguageSwitch_RefreshesTheViewModelTextTheOverlayShows
#[test]
fn language_change_refreshes_view_model_text_then_overlay_then_model() {
    let rig = Rig::new();
    rig.settle();
    Localizer::set_current(built_in_english_localizer());
    assert_eq!(rig.wake.count(), 1);
    assert!(rig.timeline.borrow().is_empty(), "the event thread touched nothing");
    rig.ov().run_pending();
    assert_eq!(
        only(&rig.timeline, &["vm.refresh_text", "invalidate", "vm.overlay_model"]),
        ["vm.refresh_text", "invalidate", "vm.overlay_model", "invalidate"],
        "text first, then apply_localization (a render request), then the model (a render request)"
    );
}

#[test]
fn run_pending_with_nothing_pending_is_a_no_op() {
    let rig = Rig::new();
    rig.settle();
    rig.ov().run_pending();
    assert!(rig.timeline.borrow().is_empty());
    assert!(rig.poster.waiting().is_empty());
}

// ---- dispose ----

// new: Close_UnsubscribesTheOverlayFromTheStaticLocalizerEvent, Close_UnsubscribesFromTheSettingsStoreAndTheViewModel
#[test]
fn after_dispose_everything_is_ignored() {
    let rig = Rig::with(RigOptions {
        store: true,
        ..RigOptions::default()
    });
    rig.settle();
    rig.ov().mark_model_dirty(); // a refresh job is waiting when the window closes
    let reads = rig.vm.model_reads.get();
    rig.ov().dispose();
    assert!(rig.tips.disposed.get(), "dispose reaches the shared tooltips");
    rig.timeline.borrow_mut().clear();
    let wakes = rig.wake.count();

    rig.poster.run_all();
    assert_eq!(
        rig.vm.model_reads.get(),
        reads,
        "the posted refresh finds the overlay disposed"
    );

    let before = rig.clock.frame_requests.get();
    rig.ov().on_view_model_changed(&navigation(MainProperty::CurrentIndex));
    rig.ov().mark_model_dirty();
    assert!(rig.poster.waiting().is_empty());
    assert_eq!(rig.clock.frame_requests.get(), before);

    settings_with_opacity(&rig, 55);
    Localizer::set_current(built_in_english_localizer());
    assert_eq!(
        rig.wake.count(),
        wakes,
        "the store and the localizer no longer reach the overlay"
    );
    rig.ov().run_pending();
    rig.ov().on_language_changed();
    rig.ov().on_command_failed("Fit", "boom");
    assert!(
        rig.timeline.borrow().is_empty(),
        "no view-model call after dispose: {:?}",
        rig.timeline.borrow()
    );
    assert!(rig.vm.failures.borrow().is_empty());
    assert!(rig.vm.set_settings_calls.borrow().is_empty());

    let dc = rig.draw();
    assert!(dc.calls.is_empty(), "a disposed overlay draws nothing");
}

#[test]
fn dispose_is_idempotent() {
    let rig = Rig::new();
    rig.settle();
    rig.ov().dispose();
    rig.ov().dispose();
    assert_eq!(
        rig.window.unsubscribe_calls.get(),
        2,
        "the second dispose unsubscribes nothing"
    );
}

// new: Close_UnsubscribesTheOverlayFromTheStaticLocalizerEvent (T6: dropping without dispose still unsubscribes)
#[test]
fn dropping_without_dispose_still_unsubscribes_the_localizer() {
    let rig = Rig::new();
    Localizer::set_current(built_in_english_localizer());
    assert_eq!(
        rig.wake.count(),
        1,
        "control: the subscription works while the overlay lives"
    );
    rig.drop_overlay();
    Localizer::set_current(built_in_english_localizer());
    assert_eq!(rig.wake.count(), 1, "the subscription died with the overlay");
}

// ---- Tools menu anchor and DPI ----

// new: ToolsMenuAnchor_IsInScreenDipsOfTheCurrentWindowDpi
#[test]
fn client_origin_dip_table() {
    type Row = ((i32, i32), f64, (f64, f64));
    let table: [Row; 8] = [
        ((300, 150), 1.0, (300.0, 150.0)),
        ((300, 150), 1.5, (200.0, 100.0)),
        ((300, 150), 2.0, (150.0, 75.0)),
        ((300, 150), 0.0, (300.0, 150.0)),
        ((300, 150), -1.0, (300.0, 150.0)),
        ((-300, -150), 2.0, (-150.0, -75.0)),
        ((0, 0), 2.0, (0.0, 0.0)),
        ((300, 150), 0.5, (600.0, 300.0)),
    ];
    for (origin, dpi, (x, y)) in table {
        assert_eq!(client_origin_dip(origin, dpi), PointD { x, y }, "{origin:?} at {dpi}");
    }
    let nan = client_origin_dip((1, 2), f64::NAN);
    assert!(
        nan.x.is_nan() && nan.y.is_nan(),
        "NaN is not <= 0: it stays NaN like the C# guard"
    );
}

// new: ToolsMenuAnchor_IsInScreenDipsOfTheCurrentWindowDpi
#[test]
fn client_origin_follows_the_window_dpi_at_call_time() {
    let rig = Rig::new();
    rig.os.origin.set((300, 150));
    assert_eq!(rig.ov().client_origin_on_screen_dip(), PointD { x: 300.0, y: 150.0 });
    rig.window.dpi.set(2.0);
    assert_eq!(rig.ov().client_origin_on_screen_dip(), PointD { x: 150.0, y: 75.0 });
    rig.window.dpi.set(1.5);
    assert_eq!(rig.ov().client_origin_on_screen_dip(), PointD { x: 200.0, y: 100.0 });
}

// new: ToolsMenuAnchor_IsInScreenDipsOfTheCurrentWindowDpi (the overlay passes it as the menu anchor)
#[test]
fn tools_menu_anchor_adds_the_client_origin_in_dips() {
    let rig = Rig::new();
    rig.window.dpi.set(2.0);
    rig.os.origin.set((300, 150));
    rig.draw();
    rig.click(OverlayButton::Tools);
    let tools = rig.bounds_of(OverlayButton::Tools);
    assert_eq!(
        rig.menus.anchors.borrow().as_slice(),
        [PointD {
            x: 150.0 + tools.x,
            y: 75.0 + tools.y + tools.height
        }]
    );
}

// new: (T2: the mouse lParam pixels are divided by the DPI of the moment of the message)
#[test]
fn message_pixels_follow_the_window_dpi_between_two_messages() {
    let rig = Rig::new();
    rig.window.dpi.set(2.0);
    rig.draw();
    let fit = rig.bounds_of(OverlayButton::Fit);
    let (x, y) = (
        ((fit.x + fit.width / 2.0) * 2.0) as i16,
        ((fit.y + fit.height / 2.0) * 2.0) as i16,
    );
    assert_eq!(rig.window.dispatch(WM_LBUTTONDOWN, 1, xy(x, y)), Some(0));
    assert_eq!(rig.window.dispatch(WM_LBUTTONUP, 0, xy(x, y)), Some(0));
    assert_eq!(rig.fit_calls.get(), 1);
    rig.window.dpi.set(1.0); // the same pixels are now twice as far in DIP: no longer over the button
    assert_eq!(rig.window.dispatch(WM_LBUTTONDOWN, 1, xy(x, y)), None);
}

// ---- commands ----

// new: (ViewModelOverlayCommands: nine one-line forwards)
#[test]
fn view_model_overlay_commands_map_one_to_one() {
    let vm = ShellVm::new(timeline());
    let fits = Rc::new(Cell::new(0usize));
    let mut commands = ViewModelOverlayCommands::new(Rc::clone(&vm) as Rc<dyn OverlayViewModel>, {
        let fits = Rc::clone(&fits);
        Box::new(move || fits.set(fits.get() + 1))
    });
    commands.open_folder();
    commands.show_settings();
    commands.show_skipped_files();
    commands.show_recovery();
    commands.show_diagnostics();
    commands.show_benchmark();
    commands.clear_cache();
    commands.remove_duplicates(true);
    commands.remove_duplicates(false);
    assert_eq!(
        vm.commands.borrow().as_slice(),
        [
            "pick_and_open_folder",
            "show_settings",
            "show_skipped_files",
            "show_recovery",
            "show_diagnostics",
            "show_benchmark",
            "clear_cache",
            "remove_duplicates(true)",
            "remove_duplicates(false)",
        ]
    );
    assert_eq!(fits.get(), 0, "Fit is not a view-model call");
    commands.fit();
    commands.fit();
    assert_eq!(fits.get(), 2);
    assert_eq!(vm.commands.borrow().len(), 9, "Fit touched nothing on the view model");
    assert!(format!("{commands:?}").contains("ViewModelOverlayCommands"));
}

// new: RealMouseMessages_ClickTheFitButton_AndTheOverlaySwallowsThePress (fake window)
#[test]
fn fit_command_runs_the_injected_closure() {
    let rig = Rig::new();
    rig.draw();
    rig.click(OverlayButton::Fit);
    assert_eq!(rig.fit_calls.get(), 1);
    assert!(rig.vm.commands.borrow().is_empty());
    rig.click(OverlayButton::OpenFolder);
    rig.click(OverlayButton::Settings);
    assert_eq!(
        rig.vm.commands.borrow().as_slice(),
        ["pick_and_open_folder", "show_settings"]
    );
}

// new: ToolsMenu_WhenTheHostThrows_ReportsTheFailure (the menu error reaches the view model as "ToolsMenu")
#[test]
fn menu_error_reaches_report_action_failed_as_toolsmenu() {
    let rig = Rig::new();
    rig.draw();
    *rig.menus.fail_with.borrow_mut() = Some("menu boom");
    rig.click(OverlayButton::Tools);
    assert_eq!(
        rig.vm.failures.borrow().as_slice(),
        [("ToolsMenu".to_owned(), "menu boom".to_owned())]
    );
    *rig.menus.fail_with.borrow_mut() = None;
    rig.click(OverlayButton::Tools);
    assert_eq!(rig.vm.failures.borrow().len(), 1, "a dismissed menu is not a failure");
}

// new: FailedAsyncCommand_IsReportedWithItsName (the reporting half)
#[test]
fn on_command_failed_is_reported_to_the_view_model() {
    let rig = Rig::new();
    rig.ov().on_command_failed("Clear cache", "disk full");
    assert_eq!(
        rig.vm.failures.borrow().as_slice(),
        [("Clear cache".to_owned(), "disk full".to_owned())]
    );
}

// new: (the owner holds the overlay as a `MainWindowFeature`: draw, change sets, dispose and the down-cast)
#[test]
fn main_window_feature_forwards_to_the_overlay() {
    let rig = Rig::new();
    rig.settle();
    let feature: Rc<dyn MainWindowFeature> = rig.ov();
    assert_eq!(feature.name(), "overlay");
    assert!(feature.as_any().downcast_ref::<MainWindowOverlay>().is_some());

    let mut dc = RecordingDrawContext::new();
    feature.draw_overlay(&mut dc);
    assert!(!dc.calls.is_empty(), "draw_overlay reaches the overlay");

    let before = rig.clock.frame_requests.get();
    feature.on_changes(&navigation(MainProperty::CurrentIndex));
    assert_eq!(rig.clock.frame_requests.get(), before + 1);
    assert_eq!(rig.poster.waiting(), [UiPriority::Render]);

    feature.dispose();
    assert_eq!(rig.window.subscriber_count(), 0);
    assert!(rig.tips.disposed.get());
}

#[test]
fn overlay_pending_is_empty_by_default() {
    let pending = OverlayPending::default();
    assert!(!pending.language_changed.load(std::sync::atomic::Ordering::SeqCst));
    assert!(pending.settings.lock().unwrap().is_none());
    assert!(format!("{pending:?}").contains("OverlayPending"));
}

// ---- native (real hidden ShellWindow) ----

/// Builds the overlay on the current UI thread's `ShellWindow`; returns the pieces the native tests read.
struct NativeOverlay {
    overlay: Rc<MainWindowOverlay>,
    fit_calls: Rc<Cell<usize>>,
}

fn attach_to_real_window() -> NativeOverlay {
    let w = create_window(pr_platform_win::host::shell_window::ShellWindowOptions::default());
    let clock = shell_window_fixture::clock();
    let vm = ShellVm::new(timeline());
    let poster = Rc::new(QueuePoster::default());
    let fit_calls = Rc::new(Cell::new(0usize));
    let tips = Rc::new(ShellTips::default());
    let factory: TooltipFactory = Box::new(move || Ok(tips as Rc<dyn OverlayTooltips>));
    let overlay = MainWindowOverlay::attach(OverlayDeps {
        window: w as Rc<dyn OverlayWindowPort>,
        clock: clock as Rc<dyn HostFrameClock>,
        view_model: vm as Rc<dyn OverlayViewModel>,
        text: Rc::new(overlay_fakes::FakeTextRenderer::new()),
        text_renderer_id: 1,
        texts: built_in_english_localizer(),
        menus: None,
        settings_store: None,
        poster: Rc::clone(&poster) as Rc<dyn LocalPoster>,
        wake: Arc::new(CountingWake::default()) as Arc<dyn UiWake>,
        fit: {
            let fit_calls = Rc::clone(&fit_calls);
            Box::new(move || fit_calls.set(fit_calls.get() + 1))
        },
        tooltip_factory: Some(factory),
        os: Rc::new(Win32OverlayOs) as Rc<dyn OverlayOs>,
        log: Arc::new(pr_core::abstractions::NullLog),
    });
    poster.run_all();
    NativeOverlay { overlay, fit_calls }
}

// new: RealMouseMessages_ClickTheFitButton_AndTheOverlaySwallowsThePress
#[test]
#[ignore = "native: needs desktop"]
fn real_window_mouse_messages_click_fit_and_the_press_is_swallowed() {
    let ui = UiThread::start();
    let (fit_calls, far_press_swallowed, near_press_swallowed) = ui.invoke(|| {
        let native = attach_to_real_window();
        let w = window();
        let mut dc = RecordingDrawContext::new();
        native.overlay.draw_overlay(&mut dc);
        let dpi = w.dpi_scale();
        let fit = native
            .overlay
            .overlay()
            .element_bounds("FitButton")
            .expect("fit button");
        let (x, y) = (
            ((fit.x + fit.width / 2.0) * dpi).round() as i16,
            ((fit.y + fit.height / 2.0) * dpi).round() as i16,
        );
        let hwnd = w.hwnd();
        send(hwnd, WM_MOUSEMOVE, 0, xy(x, y));
        send(hwnd, WM_LBUTTONDOWN, 1, xy(x, y));
        let near = native.overlay.overlay().is_pointer_down();
        send(hwnd, WM_LBUTTONUP, 0, xy(x, y));
        let (far_x, far_y) = ((600.0 * dpi) as i16, (400.0 * dpi) as i16);
        send(hwnd, WM_LBUTTONDOWN, 1, xy(far_x, far_y));
        let far = native.overlay.overlay().is_pointer_down();
        send(hwnd, WM_LBUTTONUP, 0, xy(far_x, far_y));
        native.overlay.dispose();
        let fits = native.fit_calls.get();
        dispose_window();
        (fits, far, near)
    });
    assert!(near_press_swallowed, "the press on Fit belongs to the overlay");
    assert_eq!(fit_calls, 1, "the release over Fit ran the injected closure once");
    assert!(!far_press_swallowed, "a press on the image is not the overlay's");
}

// new: ToolsMenuAnchor_IsInScreenDipsOfTheCurrentWindowDpi
#[test]
#[ignore = "native: needs desktop"]
fn tools_menu_anchor_is_in_screen_dips_of_the_current_dpi() {
    let ui = UiThread::start();
    let outcome = ui.invoke(|| {
        let native = attach_to_real_window();
        let w = window();
        let hwnd = w.hwnd();
        send_dpi_changed(hwnd, 192, window_rect(hwnd));
        let scale = w.dpi_scale();
        let origin = client_origin(hwnd);
        let anchor = native.overlay.client_origin_on_screen_dip();
        native.overlay.dispose();
        dispose_window();
        (scale, (origin.x, origin.y), anchor)
    });
    let (scale, (px, py), anchor) = outcome;
    assert_eq!(scale, 2.0);
    if px == 0 && py == 0 {
        eprintln!("skipped: the window sits at the screen origin, the check would prove nothing");
        return;
    }
    assert_eq!(
        anchor,
        PointD {
            x: f64::from(px) / 2.0,
            y: f64::from(py) / 2.0
        }
    );
}
