//! Tests of W8-14c (F): `ViewportImageSurface` (C# `ViewportImageSurface`), the viewport hooks (C# `CreateViewportOptions`, `InputNativeTests`) and
//! `MainWindowInput` (C# `ShellMainWindow.Input.cs`), baseline `d92d6aae`.
//!
//! The surface tests run on a scripted `ViewportControllerApi`. The wiring tests run a REAL `MainViewModel` (`pr-app/tests/support/main_env.rs`), a REAL
//! `ViewportController` and a REAL `ShellInput`, over the fake window, clock and dispatcher of the W8-14b fixture; only the services trait is a fake
//! (`common/input_feature_fixture.rs`). Nothing here needs a desktop. The fake window has no real HWND, so the drop target registration is refused and
//! logged: that is the expected path of these tests.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/shell_input_dispatch_fixture.rs"]
mod fixture;
#[path = "common/input_feature_fixture.rs"]
mod input_feature_fixture;
mod support {
    #[path = "../../../pr-app/tests/support/main_env.rs"]
    pub mod main_env;
    #[path = "../../../pr-app/tests/support/presenter_fakes.rs"]
    pub mod presenter_fakes;
    #[path = "../../../pr-app/tests/support/preview_fakes.rs"]
    pub mod preview_fakes;
}
#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use input_feature_fixture::{FakeController, FakePrompt, FeatureRig};
use pr_app::input::key_id::KeyId;
use pr_app::input::primitives::{PointD, SizeD};
use pr_app::input::review_command::{ReviewCommand, ReviewCommandType};
use pr_app::input::surface::{ImageSurface, RenderHookId};
use pr_app::viewmodels::main::{ChangeSet, ViewerHandle};
use pr_app::viewmodels::viewer_state::{ViewerProperty, ViewerState};
use pr_app::viewport::fit::FitSurface;
use pr_app::viewport::types::{ViewerStretchMode, ViewportLayout};
use pr_core::abstractions::DisplayTiming;
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::shell_input::dispatch::InputSurface;
use pr_platform_win::shell_input::main_window::{MainWindowInput, create_viewport_input_hooks};
use pr_platform_win::shell_input::native;
use pr_platform_win::shell_input::surface::{ViewportControllerApi, ViewportImageSurface};
use pr_test_support::LogLevel;

// ---- ViewportImageSurface ----------------------------------------------------------------------------------------------------------------------

fn surface_over(controller: &Rc<FakeController>) -> (ViewportImageSurface, ViewerHandle) {
    let viewer = ViewerHandle::new(ViewerState::new());
    let surface = ViewportImageSurface::new(Rc::clone(controller) as Rc<dyn ViewportControllerApi>, viewer.clone());
    (surface, viewer)
}

fn scripted_layout() -> ViewportLayout {
    ViewportLayout {
        viewport_width: 800.0,
        viewport_height: 600.0,
        extent_width: 3000.0,
        extent_height: 2000.0,
        horizontal_bar_visible: true,
        vertical_bar_visible: false,
        ..ViewportLayout::default()
    }
}

// C#: ViewportImageSurface.Capture (fields of MainWindow.CaptureViewportSnapshot, T89)
#[test]
fn capture_builds_the_snapshot_from_the_viewer_and_the_controller() {
    let controller = FakeController::new();
    controller.layout.set(scripted_layout());
    controller.image_actual.set((1234.0, 567.0));
    controller.horizontal_offset.set(11.0);
    controller.vertical_offset.set(22.0);
    let (surface, viewer) = surface_over(&controller);
    let _ = viewer.borrow_mut().set_source_size(3000, 2000, true);
    let _ = viewer.borrow_mut().set_zoom(2.5);
    let (zoom, stretch, max_width, max_height) = {
        let viewer = viewer.borrow();
        (
            viewer.zoom(),
            viewer.stretch(),
            viewer.max_image_width(),
            viewer.max_image_height(),
        )
    };

    let snapshot = surface.capture();

    assert_eq!(zoom, snapshot.zoom);
    assert_eq!(2.5, snapshot.zoom);
    assert_eq!(stretch, snapshot.stretch);
    assert_eq!(max_width, snapshot.max_image_width);
    assert_eq!(max_height, snapshot.max_image_height);
    assert_eq!(1234.0, snapshot.actual_image_width);
    assert_eq!(567.0, snapshot.actual_image_height);
    assert_eq!(3000.0, snapshot.extent_width);
    assert_eq!(2000.0, snapshot.extent_height);
    assert_eq!(800.0, snapshot.viewport_width);
    assert_eq!(600.0, snapshot.viewport_height);
    assert_eq!(11.0, snapshot.horizontal_offset);
    assert_eq!(22.0, snapshot.vertical_offset);
    assert!(snapshot.horizontal_scroll_bar_visible);
    assert!(!snapshot.vertical_scroll_bar_visible);
}

// new: T1. The viewer is borrowed for the four reads only; the controller is read with no borrow held.
#[test]
fn capture_does_not_hold_the_viewer_borrow_while_reading_the_controller() {
    let controller = FakeController::new();
    let (surface, viewer) = surface_over(&controller);
    let probed = Rc::new(Cell::new(false));
    let (probe_viewer, probe_flag) = (viewer.clone(), Rc::clone(&probed));
    *controller.on_image_actual_width.borrow_mut() = Some(Box::new(move || {
        // A held `borrow()` would make this panic.
        let _ = probe_viewer.borrow_mut().set_zoom(3.0);
        probe_flag.set(true);
    }));

    let _ = surface.capture();

    assert!(probed.get(), "the probe ran inside image_actual_width");
    assert_eq!(3.0, viewer.borrow().zoom());
}

// new: every member of the C# adapter forwards to the controller (pure forwarding), and nothing else is called.
#[test]
fn surface_forwards_every_member() {
    let controller = FakeController::new();
    controller.layout.set(scripted_layout());
    controller.viewport_size.set((7.0, 8.0));
    controller.horizontal_offset.set(3.0);
    controller.vertical_offset.set(4.0);
    controller.drag_threshold.set((5.0, 6.0));
    controller.image_origin.set(PointD { x: 9.0, y: 10.0 });
    controller.image_actual.set((11.0, 12.0));
    controller.source_size.set(Some(SizeD {
        width: 13.0,
        height: 14.0,
    }));
    controller.timestamp.set(15);
    controller.display_timing.set(Some(DisplayTiming {
        last_vblank: 1,
        refresh_period: 2,
    }));
    controller.pointer_position.set(Some(PointD { x: 16.0, y: 17.0 }));
    let (mut surface, _viewer) = surface_over(&controller);

    assert!(ImageSurface::is_loaded(&surface));
    assert_eq!(3.0, surface.horizontal_offset());
    assert_eq!(4.0, surface.vertical_offset());
    assert_eq!(800.0, surface.viewport_width());
    assert_eq!(600.0, surface.viewport_height());
    assert_eq!(3000.0, surface.extent_width());
    assert_eq!(2000.0, surface.extent_height());
    assert_eq!((5.0, 6.0), surface.drag_threshold());
    surface.scroll_to(1.5, 2.5);
    ImageSurface::update_layout(&mut surface);
    surface.yield_to_render(Box::new(|| {}));
    assert_eq!(PointD { x: 9.0, y: 10.0 }, surface.image_origin());
    assert_eq!(
        PointD { x: 21.0, y: 32.0 },
        surface.to_image_element(PointD { x: 20.0, y: 30.0 })
    );
    assert_eq!(11.0, surface.image_actual_width());
    assert_eq!(12.0, surface.image_actual_height());
    assert_eq!(
        Some(SizeD {
            width: 13.0,
            height: 14.0
        }),
        surface.source_size()
    );
    surface.capture_mouse();
    surface.release_mouse_capture();
    surface.set_pan_cursor(true);
    surface.set_pan_cursor(false);
    assert_eq!(15, surface.timestamp());
    assert_eq!(1, surface.display_timing().map_or(0, |timing| timing.last_vblank));
    assert_eq!(Some(PointD { x: 16.0, y: 17.0 }), surface.pointer_position());
    // FitSurface members.
    assert_eq!((7.0, 8.0), FitSurface::viewport_size(&surface));
    assert!(FitSurface::is_loaded(&surface));
    FitSurface::update_layout(&mut surface);
    surface.update_fit_size();
    surface.scroll_home();

    let calls = controller.calls_snapshot();
    let expected = [
        "is_loaded",
        "horizontal_offset",
        "vertical_offset",
        "layout",
        "layout",
        "layout",
        "layout",
        "drag_threshold",
        "scroll_to(1.5,2.5)",
        "update_layout",
        "yield_to_render",
        "image_origin",
        "to_image_element(20,30)",
        "image_actual_width",
        "image_actual_height",
        "source_size",
        "capture_mouse",
        "release_mouse_capture",
        "set_pan_cursor(true)",
        "set_pan_cursor(false)",
        "timestamp",
        "display_timing",
        "pointer_position",
        "viewport_size",
        "is_loaded",
        "update_layout",
        "update_fit_size",
        "scroll_home",
    ];
    assert_eq!(expected.to_vec(), calls);
    assert_eq!(
        1,
        controller.yielded.borrow().len(),
        "the continuation reached the controller"
    );
}

// new: D-W8B4I-5. The router runs after the controller-side handler, with the same ticks; a router set AFTER the hook does not reach that hook
// (it is captured at hook time).
#[test]
fn frame_router_is_called_after_the_controller_handler_with_the_ticks() {
    let controller = FakeController::new();
    let (mut surface, _viewer) = surface_over(&controller);
    let order = Rc::new(RefCell::new(Vec::<String>::new()));
    let router_order = Rc::clone(&order);
    surface.set_frame_router(Some(Rc::new(move |ticks| {
        router_order.borrow_mut().push(format!("router:{ticks:?}"));
    })));
    let handler_order = Rc::clone(&order);
    let id = surface.hook_render_frame(Box::new(move |ticks| {
        handler_order.borrow_mut().push(format!("handler:{ticks:?}"));
    }));

    controller.fire_frame(Some(1234));
    controller.fire_frame(None);

    assert_eq!(
        vec!["handler:Some(1234)", "router:Some(1234)", "handler:None", "router:None"],
        *order.borrow()
    );

    // A hook made before the router existed never calls it.
    let late = Rc::new(RefCell::new(Vec::<String>::new()));
    let (mut other, _viewer) = surface_over(&controller);
    let late_handler = Rc::clone(&late);
    let _ = other.hook_render_frame(Box::new(move |_| late_handler.borrow_mut().push("handler".to_owned())));
    let late_router = Rc::clone(&late);
    other.set_frame_router(Some(Rc::new(move |_| {
        late_router.borrow_mut().push("router".to_owned())
    })));
    order.borrow_mut().clear();
    surface.unhook_render_frame(id);
    controller.fire_frame(Some(5));
    assert_eq!(vec!["handler"], *late.borrow());
    assert!(order.borrow().is_empty(), "an unhooked handler is not called");
    assert_eq!(1, controller.hook_count());
}

// new: the id of `hook_render_frame` is the controller's id (the pointer controller unhooks with it).
#[test]
fn unhook_forwards_the_controller_id() {
    let controller = FakeController::new();
    let (mut surface, _viewer) = surface_over(&controller);
    let id = surface.hook_render_frame(Box::new(|_| {}));
    surface.unhook_render_frame(id);
    assert!(
        controller
            .calls_snapshot()
            .contains(&format!("unhook_render_frame({})", id.0))
    );
    assert_eq!(RenderHookId(1), id);
}

// ---- hooks -------------------------------------------------------------------------------------------------------------------------------------

// C#: InputNativeTests.ViewportOptions_DragMetricsFollowTheWindowDpi_AndThePointerIsTheRealCursor (96/144/192 dpi, then 96 again)
#[test]
fn hooks_follow_the_window_dpi_at_each_call() {
    let window = fixture::InputWindow::new(800.0, 600.0, 1.0);
    let hooks = create_viewport_input_hooks(Rc::clone(&window) as Rc<dyn ShellWindowApi>);

    for (scale, dpi) in [(1.0, 96_u32), (1.5, 144), (2.0, 192), (1.0, 96)] {
        window.dpi_scale.set(scale);
        assert_eq!(
            native::drag_metrics_pixels(dpi),
            (hooks.drag_metrics_pixels)(),
            "dpi scale {scale}"
        );
    }
    assert_eq!(native::cursor_screen_pixel(), (hooks.cursor_screen_pixel)());
}

// ---- execute table -----------------------------------------------------------------------------------------------------------------------------

fn images_rig(names: &[&str]) -> (FeatureRig, Vec<String>) {
    let mut env = support::main_env::MainEnv::new();
    let (folder, _paths) = env.images("shots", names);
    env.open(&folder);
    (
        FeatureRig::with_env(env),
        names.iter().map(|name| (*name).to_owned()).collect(),
    )
}

fn execute(rig: &FeatureRig, kind: ReviewCommandType) {
    let host = rig.input.input_host();
    host.execute(ReviewCommand::new(kind))
        .expect("execute never fails here");
    rig.settle();
}

fn index(rig: &FeatureRig) -> i32 {
    rig.env.borrow().vm.current_index()
}

// new (C# ExecuteAsync switch): each arm reaches the view model exactly once; the arms that have an observable effect assert it.
#[test]
fn execute_table_maps_each_command_to_one_view_model_call() {
    let (rig, _) = images_rig(&["a.jpg", "b.jpg", "c.jpg"]);
    assert_eq!(0, index(&rig));
    let overlay_before = rig.env.borrow().settings.current().show_info_overlay;

    // Navigation arms.
    let before = rig.vm_calls();
    execute(&rig, ReviewCommandType::LastImage);
    assert_eq!((before + 1, 2), (rig.vm_calls(), index(&rig)), "LastImage");
    execute(&rig, ReviewCommandType::FirstImage);
    assert_eq!(0, index(&rig), "FirstImage");
    execute(&rig, ReviewCommandType::Next);
    assert_eq!(1, index(&rig), "Next");
    execute(&rig, ReviewCommandType::Previous);
    assert_eq!(0, index(&rig), "Previous");
    execute(&rig, ReviewCommandType::Skip);
    assert_eq!(1, index(&rig), "Skip");
    assert_eq!(before + 5, rig.vm_calls());

    // Fullscreen.
    let before = rig.vm_calls();
    execute(&rig, ReviewCommandType::Fullscreen);
    assert!(rig.viewer.borrow().is_fullscreen(), "Fullscreen");
    execute(&rig, ReviewCommandType::ExitFullscreen);
    assert!(!rig.viewer.borrow().is_fullscreen(), "ExitFullscreen");
    assert_eq!(before + 2, rig.vm_calls());

    // Info overlay: the view model flips and saves the setting.
    execute(&rig, ReviewCommandType::ToggleInfoOverlay);
    assert_eq!(!overlay_before, rig.env.borrow().settings.current().show_info_overlay);

    // The arms without an observable effect in this environment: one view-model call each.
    for kind in [
        ReviewCommandType::Undo,
        ReviewCommandType::ToggleCompare,
        ReviewCommandType::ToggleCaptureMember,
        ReviewCommandType::Recycle,
        ReviewCommandType::NextFolder,
        ReviewCommandType::PreviousFolder,
        ReviewCommandType::OpenFolder,
    ] {
        let before = rig.vm_calls();
        execute(&rig, kind);
        assert_eq!(before + 1, rig.vm_calls(), "{kind:?}");
    }
    let before = rig.vm_calls();
    rig.input
        .input_host()
        .execute(ReviewCommand::move_to_folder(true))
        .expect("move");
    rig.input
        .input_host()
        .execute(ReviewCommand::copy_to_folder(false))
        .expect("copy");
    assert_eq!(before + 2, rig.vm_calls(), "MoveToFolder, CopyToFolder");
    let before = rig.vm_calls();
    rig.input
        .input_host()
        .execute(ReviewCommand::action(0))
        .expect("run action");
    assert_eq!(before + 1, rig.vm_calls(), "RunAction");
    assert!(
        rig.statuses().is_empty(),
        "no arm reported a failure: {:?}",
        rig.statuses()
    );
}

// new: the arms that never touch the view model, and the viewport kinds that arrive here by mistake do nothing.
#[test]
fn close_refresh_keep_zoom_and_custom_zoom_do_not_run_through_with_view_model_blindly() {
    let (rig, _) = images_rig(&["a.jpg"]);
    let host = rig.input.input_host();

    let before = rig.vm_calls();
    host.execute(ReviewCommand::new(ReviewCommandType::Close))
        .expect("close");
    assert_eq!(before, rig.vm_calls(), "Close never borrows the view model");
    assert_eq!(1, rig.services.closes.get());

    host.execute(ReviewCommand::new(ReviewCommandType::ToggleKeepZoom))
        .expect("keep");
    assert_eq!(
        before,
        rig.vm_calls(),
        "ToggleKeepZoom goes through the settings source"
    );

    host.execute(ReviewCommand::new(ReviewCommandType::Refresh))
        .expect("refresh");
    assert_eq!(before + 1, rig.vm_calls());
    assert_eq!(1, rig.services.invalidations.get());

    // CustomZoom without a prompt: nothing.
    host.execute(ReviewCommand::new(ReviewCommandType::CustomZoom))
        .expect("custom");

    for kind in [
        ReviewCommandType::ToggleFit,
        ReviewCommandType::ZoomIn,
        ReviewCommandType::ZoomOut,
        ReviewCommandType::ZoomActualSize,
        ReviewCommandType::ClickZoom,
        ReviewCommandType::FitWidth,
        ReviewCommandType::FitHeight,
        ReviewCommandType::FitWidth2,
    ] {
        host.execute(ReviewCommand::new(kind)).expect("viewport kind");
    }
    assert_eq!(
        before + 1,
        rig.vm_calls(),
        "the viewport kinds never reach the view model"
    );
}

// new: `report_action_failed` logs `"{name} failed"` and shows the services' text; the zoom name is read at call time.
#[test]
fn guarded_arms_report_and_swallow_but_unguarded_arms_return_the_error() {
    let (rig, _) = images_rig(&["a.jpg"]);
    let host = rig.input.input_host();
    let boom = std::io::Error::other("boom");

    host.report_action_failed("Move to folder", &boom);

    assert!(rig.services.log.contains(LogLevel::Error, "Move to folder failed"));
    assert_eq!(vec!["action-failed:Move to folder:boom".to_owned()], rig.statuses());
    assert_eq!("Zoom", host.zoom_action_name());

    // A guarded arm that does not fail returns Ok and reports nothing; the unguarded arms return `Ok` too (their view-model calls cannot fail).
    for kind in [
        ReviewCommandType::NextFolder,
        ReviewCommandType::PreviousFolder,
        ReviewCommandType::OpenFolder,
        ReviewCommandType::CustomZoom,
        ReviewCommandType::Undo,
        ReviewCommandType::FirstImage,
    ] {
        assert!(host.execute(ReviewCommand::new(kind)).is_ok(), "{kind:?}");
    }
    assert_eq!(1, rig.statuses().len(), "nothing but the explicit report");
}

// new: queries and commands read the view model; `RunAction` with an index outside the list does not panic and still reaches the view model once.
#[test]
fn run_action_with_an_index_outside_the_list_is_one_view_model_call() {
    let (rig, _) = images_rig(&["a.jpg"]);
    let host = rig.input.input_host();
    let before = rig.vm_calls();

    host.execute(ReviewCommand::action(99)).expect("out of range");
    host.execute(ReviewCommand::action(-1)).expect("negative");

    assert_eq!(before + 2, rig.vm_calls());
    assert!(host.has_images());
    assert!(!host.is_compare_visible());
    assert!(!host.current_has_capture_pair());
    assert!(!host.cancel_duplicate_check());
}

// new: T2. `Close` calls the services with the view model free (a real window sends messages back into the handler chain).
#[test]
fn close_runs_outside_the_view_model_borrow() {
    let rig = FeatureRig::new();

    rig.input
        .input_host()
        .execute(ReviewCommand::new(ReviewCommandType::Close))
        .expect("close");

    assert_eq!(vec![true], *rig.services.close_outside_borrow.borrow());
}

// ---- zoom helpers ------------------------------------------------------------------------------------------------------------------------------

fn clicks(rig: &FeatureRig) -> i32 {
    rig.env.borrow().settings.current().click_zoom_percent
}

// C#: C-12 (SetZoomAlsoSetsClickLevel decides whether the menu zoom is saved as the click level)
#[test]
fn apply_zoom_menu_selection_saves_only_when_also_set_is_on() {
    let rig = images_rig(&["a.jpg"]).0;
    rig.env.borrow().update_settings(|s| {
        s.set_zoom_also_sets_click_level = false;
        s.click_zoom_percent = 100;
    });

    rig.input.apply_zoom_menu_selection(250);
    rig.drain();
    assert_eq!(100, clicks(&rig), "off: zooms, saves nothing");
    assert_eq!(100, rig.env.borrow().store.current().click_zoom_percent);
    assert_eq!(250, rig.viewer.borrow().display_zoom_percent(), "the zoom was applied");

    rig.env
        .borrow()
        .update_settings(|s| s.set_zoom_also_sets_click_level = true);
    rig.input.apply_zoom_menu_selection(300);
    rig.drain();
    assert_eq!(300, clicks(&rig), "on: the level is saved");
    assert_eq!(
        300,
        rig.env.borrow().store.current().click_zoom_percent,
        "and it reached the store"
    );
    assert_eq!(300, rig.viewer.borrow().display_zoom_percent());
}

// C#: ApplyClickZoomLevelAsync ("zooms even when the save failed")
#[test]
fn apply_click_zoom_level_zooms_even_when_the_save_fails() {
    let rig = images_rig(&["a.jpg"]).0;
    rig.env.borrow().update_settings(|s| s.click_zoom_percent = 100);
    // A directory where the config file goes: the save cannot replace it.
    let config = rig.env.borrow().paths.config_file.clone();
    let _ = std::fs::remove_file(&config);
    std::fs::create_dir_all(&config).expect("block the config file");

    rig.input.apply_click_zoom_level(275);
    rig.drain();

    assert!(
        rig.services
            .log
            .contains(LogLevel::Error, "Could not save the click zoom level setting"),
        "{:?}",
        rig.services.log.entries()
    );
    assert_eq!(275, clicks(&rig), "the session keeps the value (provisional override)");
    assert_eq!(275, rig.viewer.borrow().display_zoom_percent(), "and still zooms");
    let _ = std::fs::remove_dir_all(&config);
}

// C#: ToggleKeepZoomAcrossImages (flips and saves)
#[test]
fn toggle_keep_zoom_flips_and_saves() {
    let rig = FeatureRig::new();
    let initial = rig.env.borrow().settings.current().keep_zoom_across_images;

    rig.input.toggle_keep_zoom_across_images();
    assert_eq!(!initial, rig.env.borrow().settings.current().keep_zoom_across_images);
    assert_eq!(
        !initial,
        rig.env.borrow().store.current().keep_zoom_across_images,
        "saved"
    );

    rig.input.toggle_keep_zoom_across_images();
    assert_eq!(initial, rig.env.borrow().settings.current().keep_zoom_across_images);
    assert!(!rig.services.log.contains(LogLevel::Error, "keep-zoom"));
}

// C#: OpenClickZoomCustomDialogAsync (a cancelled prompt changes nothing; an answer is applied)
#[test]
fn custom_zoom_cancelled_changes_nothing() {
    let rig = images_rig(&["a.jpg"]).0;
    rig.env.borrow().update_settings(|s| {
        s.click_zoom_percent = 150;
        s.set_zoom_also_sets_click_level = true;
    });
    let zoom_before = rig.viewer.borrow().zoom();
    let prompt = FakePrompt::new(None);
    *rig.services.prompt.borrow_mut() =
        Some(Rc::clone(&prompt) as Rc<dyn pr_app::services::contracts::ZoomPromptService>);

    rig.input.open_click_zoom_custom_dialog();

    assert_eq!(
        vec![150],
        *prompt.asked.borrow(),
        "asked with the current click percent"
    );
    assert_eq!(150, clicks(&rig));
    assert_eq!(zoom_before, rig.viewer.borrow().zoom());

    prompt.answer.set(Some(200));
    rig.input.open_click_zoom_custom_dialog();
    rig.drain();
    assert_eq!(200, clicks(&rig));
    assert_eq!(200, rig.viewer.borrow().display_zoom_percent());
}

// ---- change fan-out ----------------------------------------------------------------------------------------------------------------------------

fn start_glide(rig: &FeatureRig) {
    rig.env.borrow().update_settings(|s| s.kinetic_pan_enabled = true);
    let key = |key: KeyId| {
        let vk = pr_app::input::key_mapping::to_virtual_key(key);
        rig.window
            .dispatch(pr_platform_win::win32::messages::WM_KEYDOWN, vk as usize, 1);
        rig.drain();
    };
    key(KeyId::D1);
    key(KeyId::RIGHT);
    assert_eq!(1, rig.clock.subscriber_count(), "a glide hooks one frame");
}

fn changes_with(property: ViewerProperty) -> ChangeSet {
    let mut changes = ChangeSet::default();
    changes.viewer.insert(property);
    changes
}

// C#: ViewerState.ZoomModeChanged -> OnZoomModeChanged (the toolbar and the menu change the zoom too)
#[test]
fn view_model_changes_stop_the_glide_on_zoom_or_stretch_bits() {
    for property in [ViewerProperty::Zoom, ViewerProperty::Stretch] {
        let rig = images_rig(&["a.jpg", "b.jpg"]).0;
        start_glide(&rig);

        rig.input
            .on_view_model_changes(&changes_with(ViewerProperty::IsFullscreen));
        assert_eq!(
            1,
            rig.clock.subscriber_count(),
            "an unrelated viewer bit keeps the glide"
        );
        rig.input.on_view_model_changes(&ChangeSet::default());
        assert_eq!(1, rig.clock.subscriber_count(), "an empty set keeps the glide");

        rig.input.on_view_model_changes(&changes_with(property));
        assert_eq!(0, rig.clock.subscriber_count(), "{property:?} stops the glide");
    }
    let _ = ViewerStretchMode::Uniform;
}

// ---- drop ---------------------------------------------------------------------------------------------------------------------------------------

// new: the fake window has no HWND, so OLE refuses the target; the input still works and the warning is logged.
#[test]
fn a_refused_drop_registration_is_a_warning_and_the_input_still_works() {
    let rig = FeatureRig::new();

    assert!(!rig.input.drop_target_registered());
    assert!(rig.input.drop_target_interface().is_none());
    assert_eq!(
        vec!["Drag and drop is unavailable (RegisterDragDrop failed).".to_owned()],
        rig.warnings()
    );
    let (handled, _) = rig.window.dispatch(
        pr_platform_win::win32::messages::WM_KEYDOWN,
        pr_app::input::key_mapping::to_virtual_key(KeyId::F9) as usize,
        1,
    );
    assert!(!handled, "an unassigned key falls through, the handlers are registered");
    assert_eq!(3, rig.window.handler_count(), "keyboard, pointer and wheel handlers");
}

// C#: ReportDropFailure (log + FolderOpenFailed status) and OpenDroppedPathAsync (the view model's open_path)
#[test]
fn drop_failure_status_uses_folder_open_failed_text() {
    let rig = FeatureRig::new();
    let boom = std::io::Error::other("cannot open");

    rig.input.report_drop_failure(&boom);

    assert!(rig.services.log.contains(LogLevel::Error, "Drop open failed"));
    assert_eq!(vec!["open-failed:cannot open".to_owned()], rig.statuses());
}

// C#: OpenDroppedPathAsync
#[test]
fn a_dropped_path_opens_through_the_view_model() {
    let env = support::main_env::MainEnv::new();
    let (folder, paths) = env.images("dropped", &["a.jpg", "z.jpg"]);
    let _ = folder;
    let rig = FeatureRig::with_env(env);

    rig.input.open_dropped_path(&paths[1]);
    rig.settle();

    assert_eq!(1, rig.vm_calls());
    let env = rig.env.borrow();
    assert!(env.vm.has_images());
    assert_eq!(1, env.vm.current_index(), "z.jpg is the opened file");
}

// ---- lifecycle ----------------------------------------------------------------------------------------------------------------------------------

// new: idempotent; the Closed subscription is the unit's own (D-W8B4X-16) and is removed by the first close.
#[test]
fn on_window_closed_is_idempotent_and_revokes_the_drop_target() {
    let rig = FeatureRig::new();
    assert_eq!(
        1,
        rig.window
            .subscriber_count(pr_platform_win::host::contracts::ShellWindowEvent::Closed)
    );
    assert_eq!(
        1,
        rig.window
            .subscriber_count(pr_platform_win::host::contracts::ShellWindowEvent::Deactivated)
    );

    rig.window
        .raise(pr_platform_win::host::contracts::ShellWindowEvent::Closed);
    assert!(!rig.input.drop_target_registered());
    assert_eq!(
        0,
        rig.window
            .subscriber_count(pr_platform_win::host::contracts::ShellWindowEvent::Closed)
    );
    assert_eq!(
        0,
        rig.window
            .subscriber_count(pr_platform_win::host::contracts::ShellWindowEvent::Deactivated),
        "ShellInput was disposed"
    );

    rig.input.on_window_closed();
    rig.input.on_window_closed();
    assert!(!rig.input.drop_target_registered());
}

// new: R2-F-27 / D-W8B4I-7. An edit installs a new settings object; the shortcut table follows without an explicit call.
#[test]
fn settings_changed_rebuilds_the_shortcut_table_lazily_and_eagerly() {
    let rig = FeatureRig::new();
    let overlay = rig.env.borrow().settings.current().show_info_overlay;
    rig.env
        .borrow()
        .update_settings(|s| s.shortcuts.toggle_info_overlay = "J".to_owned());
    let send = |key: KeyId| {
        let vk = pr_app::input::key_mapping::to_virtual_key(key);
        rig.window
            .dispatch(pr_platform_win::win32::messages::WM_KEYDOWN, vk as usize, 1);
        rig.drain();
        rig.settle();
    };

    send(KeyId::J);
    assert_ne!(
        overlay,
        rig.env.borrow().settings.current().show_info_overlay,
        "J (new) toggles"
    );

    let current = rig.env.borrow().settings.current();
    rig.input.on_settings_changed(&current);
    send(KeyId::J);
    assert_eq!(
        overlay,
        rig.env.borrow().settings.current().show_info_overlay,
        "J toggles back"
    );
    let _: &MainWindowInput = &rig.input;
    let _: &dyn InputSurface = &ViewportImageSurface::new(
        FakeController::new() as Rc<dyn ViewportControllerApi>,
        ViewerHandle::new(ViewerState::new()),
    );
    let _ = ReviewCommand::NEXT;
}

// new: the production `ViewportControllerApi` (the real controller) forwards the capture calls to the window (mutation: the forwarders did nothing).
#[test]
fn the_real_controller_forwards_capture_and_release_to_the_window() {
    let rig = FeatureRig::new();
    let api = Rc::clone(&rig.services.viewport);

    api.capture_mouse();
    assert!(rig.window.captured.get());
    assert_eq!(1, rig.window.capture_calls.get());

    api.release_mouse_capture();
    assert!(!rig.window.captured.get());
    assert_eq!(1, rig.window.releases.get());

    api.set_pan_cursor(true);
    assert_eq!(
        Some(pr_platform_win::host::contracts::ShellCursor::SizeAll),
        rig.window.cursor.get()
    );
}
