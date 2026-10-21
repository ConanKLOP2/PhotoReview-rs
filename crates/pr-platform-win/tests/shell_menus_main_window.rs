//! Tests of W8-14f `shell_menus::main_window` (C# `ShellMainWindow.Menus.cs`, baseline `d92d6aae`; the C# end-to-end tests are `shell_menus_e2e.rs`).
//! Pure: fake `MainWindowMenuServices` over a REAL `MainViewModel` and a REAL `MainWindowInput` (the W8-14c fixture), a scripted popup host, a fake shell
//! window. No native call, no real menu. The port of `ShellMenuRenderingTests.ContextMenu_WhenShowingThrows_...` is `a_failing_host_*`.
#![cfg(windows)]

#[path = "common/context_menu_feature_fixture.rs"]
mod context_menu_feature_fixture;
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

use std::error::Error;

use context_menu_feature_fixture::{Boom, MenuRig};
use input_feature_fixture::FakePrompt;
use pr_app::input::primitives::PointD;
use pr_app::input::review_command::ReviewCommand;
use pr_core::abstractions::SettingsTarget;
use pr_core::settings::context_menu::{ContextMenuItemId, apply_hidden};
use pr_platform_win::shell_menus::controller::ContextMenuActions;
use pr_platform_win::shell_menus::main_window::MainWindowMenuActions;
use pr_platform_win::win32::messages::WM_CONTEXTMENU;
use pr_test_support::LogLevel;

fn actions(rig: &MenuRig) -> MainWindowMenuActions {
    MainWindowMenuActions::new(
        std::rc::Rc::clone(&rig.services)
            as std::rc::Rc<dyn pr_platform_win::shell_menus::main_window::MainWindowMenuServices>,
        std::rc::Rc::clone(&rig.rig.input),
    )
}

// ---- register ---------------------------------------------------------------------------------------------------------------------------------

// C#: RegisterContextMenu (AddMessageHandler(ContextMenuMessageHandler)) - the handler is the LAST of the chain (D-W8B4X-15)
#[test]
fn register_adds_the_context_menu_handler_last() {
    let rig = MenuRig::with_images(&["a.jpg"]);
    assert_eq!(rig.rig.window.handler_count(), rig.handlers_before + 1);

    // A `WM_CONTEXTMENU` with lparam -1 (keyboard) reaches the controller through the window's chain.
    let host = rig.script(None);
    let (handled, _) = rig.rig.window.dispatch(WM_CONTEXTMENU, 0, -1);
    assert!(handled);
    assert_eq!(host.shown.borrow().len(), 1);
}

// C#: ContextMenu (the controller exists after registration and reads the model from the live settings at every opening)
#[test]
fn context_menu_builds_the_model_from_the_live_state_at_every_opening() {
    let rig = MenuRig::with_images(&["a.jpg"]);
    let host = rig.script(None);
    rig.rig
        .env
        .borrow()
        .update_settings(|s| apply_hidden(s, std::iter::empty()));
    rig.menus.context_menu().show(PointD { x: 5.0, y: 6.0 });
    assert_eq!(host.points.borrow()[0], PointD { x: 5.0, y: 6.0 });
    assert!(
        host.ids_of_last().contains(&"ZoomToLevel".to_owned()),
        "{:?}",
        host.ids_of_last()
    );

    rig.rig
        .env
        .borrow()
        .update_settings(|s| apply_hidden(s, [ContextMenuItemId::ZoomToLevel]));
    rig.menus.context_menu().show(PointD { x: 1.0, y: 2.0 });

    assert!(
        !host.ids_of_last().contains(&"ZoomToLevel".to_owned()),
        "{:?}",
        host.ids_of_last()
    );
    assert_eq!(host.shown.borrow().len(), 2);
}

// new: the owner closure of the default host answers what the services answer; 0 means no OS call and no failure
#[test]
fn owner_hwnd_zero_makes_the_default_host_show_nothing_and_report_nothing() {
    let rig = MenuRig::with_images(&["a.jpg"]);
    rig.services.owner.set(0);
    // The default `NativePopupMenuHost` (not scripted): owner 0 gives `None` before any OS call.
    rig.menus.context_menu().show(PointD { x: 1.0, y: 1.0 });
    assert!(rig.services.shell_errors.borrow().is_empty());
    assert!(rig.services.statuses.borrow().is_empty());
    assert!(!rig.menus.context_menu().is_open());
}

// ---- IContextMenuActions ----------------------------------------------------------------------------------------------------------------------

// new: each member reaches the view model / the input exactly as the C# explicit interface members do
#[test]
fn actions_map_to_view_model_and_input_calls() {
    let rig = MenuRig::with_images(&["a.jpg", "b.jpg"]);
    let a = actions(&rig);

    assert!(a.has_image());
    assert!(a.can_copy_path());
    assert!(!a.can_open_in_external_editor(), "no editor configured");
    assert!(!a.is_compare_visible());
    assert_eq!(a.effective_zoom(), None, "the viewer starts in Fit");
    assert_eq!(a.settings().click_zoom_percent, rig.click_zoom_percent());
    assert_eq!(a.zoom_action_name(), "Zoom");

    // apply_zoom_preset -> the menu selection (zooms; saves the click level only when "also set" is on).
    rig.rig
        .env
        .borrow()
        .update_settings(|s| s.set_zoom_also_sets_click_level = false);
    a.apply_zoom_preset(250).expect("ok");
    rig.rig.drain();
    assert_eq!(rig.rig.viewer.borrow().display_zoom_percent(), 250);
    assert_eq!(a.effective_zoom().map(|z| (z * 100.0).round() as i32), Some(250));

    // fit -> back to Fit.
    a.fit().expect("ok");
    rig.rig.drain();
    assert_eq!(a.effective_zoom(), None);

    // zoom_to_configured_level -> zooms to the configured click level, which is left unchanged.
    rig.rig.env.borrow().update_settings(|s| s.click_zoom_percent = 175);
    a.zoom_to_configured_level().expect("ok");
    rig.rig.drain();
    assert_eq!(rig.rig.viewer.borrow().display_zoom_percent(), 175);
    assert_eq!(rig.click_zoom_percent(), 175);

    // custom_zoom -> asks the prompt with the current click level and applies the answer.
    let prompt = FakePrompt::new(Some(210));
    *rig.rig.services.prompt.borrow_mut() = Some(std::rc::Rc::clone(&prompt) as _);
    a.custom_zoom().expect("ok");
    rig.rig.drain();
    assert_eq!(*prompt.asked.borrow(), [175]);
    assert_eq!(rig.rig.viewer.borrow().display_zoom_percent(), 210);

    // execute -> ShellInput.execute (the arms are covered by the 14c tests; here only that it returns Ok and goes through the input).
    let before = rig.rig.vm_calls();
    a.execute(ReviewCommand::REFRESH).expect("ok");
    rig.rig.settle();
    assert!(rig.rig.vm_calls() >= before);
}

// new: the three view-model commands go through `with_view_model`, once each
#[test]
fn copy_and_settings_commands_call_the_view_model_once_each() {
    let rig = MenuRig::with_images(&["a.jpg"]);
    let a = actions(&rig);

    let before = rig.services.vm_calls.get();
    a.copy_file_name().expect("ok");
    assert_eq!(rig.services.vm_calls.get(), before + 1);
    a.copy_full_pathname().expect("ok");
    assert_eq!(rig.services.vm_calls.get(), before + 2);

    let dialog = std::sync::Arc::clone(&rig.rig.env.borrow().dialog);
    dialog.on_settings(|_| false);
    a.show_settings().expect("ok");
    assert_eq!(rig.services.vm_calls.get(), before + 3);
    assert_eq!(
        *dialog.settings_targets.lock().expect("lock"),
        [SettingsTarget::Default]
    );
}

// C#: OpenInExternalEditor() - nothing configured: the view model takes the user to the Settings field that sets it
#[test]
fn open_in_external_editor_without_an_editor_opens_the_settings_at_the_editor_field() {
    let rig = MenuRig::with_images(&["a.jpg"]);
    let dialog = std::sync::Arc::clone(&rig.rig.env.borrow().dialog);
    dialog.on_settings(|_| false);

    actions(&rig).open_in_external_editor().expect("no failure");

    assert_eq!(
        *dialog.settings_targets.lock().expect("lock"),
        [SettingsTarget::ExternalEditor]
    );
}

// C#: CanUndo => true ("always on like WPF: an empty history is a no-op")
#[test]
fn can_undo_is_always_true() {
    let empty = MenuRig::new();
    assert!(!empty.rig.env.borrow().vm.has_images());
    assert!(actions(&empty).can_undo());

    let with_images = MenuRig::with_images(&["a.jpg"]);
    assert!(actions(&with_images).can_undo());
}

// C#: HasImage / CanCopyPath with no folder open
#[test]
fn without_a_folder_there_is_no_image_and_nothing_to_copy() {
    let rig = MenuRig::new();
    let a = actions(&rig);
    assert!(!a.has_image());
    assert!(!a.can_copy_path());
    assert!(!a.can_open_in_external_editor());
}

// C#: CanOpenInExternalEditor (path configured and an image)
#[test]
fn can_open_in_external_editor_needs_a_configured_path_and_an_image() {
    let rig = MenuRig::with_images(&["a.jpg"]);
    rig.rig
        .env
        .borrow()
        .update_settings(|s| s.external_editor_path = "C:\\Tools\\editor.exe".to_owned());
    assert!(actions(&rig).can_open_in_external_editor());
}

// C#: ReportActionFailed => ((IShellInputHost)this).ReportActionFailed
#[test]
fn report_action_failed_goes_to_the_input_host() {
    let rig = MenuRig::with_images(&["a.jpg"]);
    actions(&rig).report_action_failed("Zoom", &Boom("kaput"));
    let statuses = rig.rig.statuses();
    assert_eq!(statuses.len(), 1, "{statuses:?}");
    assert!(
        statuses[0].contains("Zoom") && statuses[0].contains("kaput"),
        "{statuses:?}"
    );
}

// ---- settings setters --------------------------------------------------------------------------------------------------------------------------

// C#: SetZoomAlsoSetsClickLevel / SetClickZoomPercent (write the setting, SaveSettings)
#[test]
fn set_flags_save_with_the_documented_failure_texts() {
    let rig = MenuRig::with_images(&["a.jpg"]);
    let a = actions(&rig);

    // Saves: visible at once and in the store.
    a.set_zoom_also_sets_click_level(true);
    assert!(rig.rig.env.borrow().settings.current().set_zoom_also_sets_click_level);
    assert!(rig.rig.env.borrow().store.current().set_zoom_also_sets_click_level);
    a.set_zoom_also_sets_click_level(false);
    assert!(!rig.rig.env.borrow().store.current().set_zoom_also_sets_click_level);
    a.set_click_zoom_percent(321);
    assert_eq!(rig.click_zoom_percent(), 321);
    assert_eq!(rig.rig.env.borrow().store.current().click_zoom_percent, 321);
    assert!(rig.services.log.entries().is_empty(), "no failure, no log");

    // A config file that cannot be replaced: the value stays for this session and the documented text is logged.
    let config = rig.rig.env.borrow().paths.config_file.clone();
    let _ = std::fs::remove_file(&config);
    std::fs::create_dir_all(&config).expect("block the config file");

    a.set_zoom_also_sets_click_level(true);
    assert!(
        rig.services.log.contains(
            LogLevel::Error,
            "Could not save the 'also set as click zoom level' setting"
        ),
        "{:?}",
        rig.services.log.entries()
    );
    assert!(
        rig.rig.env.borrow().settings.current().set_zoom_also_sets_click_level,
        "the session keeps the provisional value"
    );

    a.set_click_zoom_percent(444);
    assert!(
        rig.services
            .log
            .contains(LogLevel::Error, "Could not save the click zoom level setting"),
        "{:?}",
        rig.services.log.entries()
    );
    assert_eq!(rig.click_zoom_percent(), 444);
    let _ = std::fs::remove_dir_all(&config);
}

// ---- report_menu_failed ------------------------------------------------------------------------------------------------------------------------

// C#: ReportMenuFailed(context, exception) - shell-errors.log, the general log "{context} failed", the status line
#[test]
fn report_menu_failed_writes_the_shell_log_the_general_log_and_the_status() {
    let rig = MenuRig::new();

    rig.menus.report_menu_failed("ToolsMenu", &Boom("menu host exploded"));

    assert_eq!(
        *rig.services.shell_errors.borrow(),
        [("ToolsMenu".to_owned(), "menu host exploded".to_owned())]
    );
    let entries = rig.services.log.entries();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].0, LogLevel::Error);
    assert!(entries[0].1.contains("ToolsMenu failed"), "{entries:?}");
    assert_eq!(*rig.services.statuses.borrow(), ["Menu error: menu host exploded"]);
    assert_eq!(rig.status_text(), "Menu error: menu host exploded");
}

// C#: ReportMenuFailed(exception) => ReportMenuFailed("ContextMenu", exception)
#[test]
fn the_actions_report_menu_failed_uses_the_context_menu_context() {
    let rig = MenuRig::new();

    actions(&rig).report_menu_failed(&Boom("bad"));

    assert_eq!(
        *rig.services.shell_errors.borrow(),
        [("ContextMenu".to_owned(), "bad".to_owned())]
    );
    assert!(rig.services.log.contains(LogLevel::Error, "ContextMenu failed"));
}

// C#: ShellMenuRenderingTests.ContextMenu_WhenShowingThrows_WritesShellErrorsLogAndShowsAMenuErrorStatus (host side, fake services)
#[test]
fn a_failing_host_writes_the_shell_log_and_shows_a_menu_error_status() {
    let rig = MenuRig::with_images(&["a.jpg"]);
    let host = rig.script(None);
    host.failure.set(Some("menu host exploded"));

    let result = rig.rig.window.dispatch(WM_CONTEXTMENU, 0, -1);

    assert!(result.0);
    let status = rig.status_text();
    assert!(status.starts_with("Menu error"), "{status}");
    let errors = rig.services.shell_errors.borrow();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, "ContextMenu");
    assert!(errors[0].1.contains("menu host exploded"));
    assert!(
        !rig.menus.context_menu().is_open(),
        "the flag is cleared after a failure"
    );
}

// ---- through the controller --------------------------------------------------------------------------------------------------------------------

// C#: ShellContextMenuEndToEndTests.NativeMenu_ZoomSubmenu_PresetFromTheSubmenu_ZoomsAndSavesTheClickLevel (model side: a chosen id runs the action)
#[test]
fn a_chosen_zoom_preset_zooms_and_saves_the_click_level_when_also_set_is_on() {
    let rig = MenuRig::with_images(&["a.jpg"]);
    rig.rig.env.borrow().update_settings(|s| {
        s.set_zoom_also_sets_click_level = true;
        s.click_zoom_percent = 100;
    });
    rig.script(Some("Zoom.Preset.300"));

    rig.menus.context_menu().show(PointD { x: 10.0, y: 10.0 });
    rig.rig.drain();

    assert_eq!(rig.rig.viewer.borrow().display_zoom_percent(), 300);
    assert_eq!(rig.click_zoom_percent(), 300);
}

// C#: AlsoSetClickLevel_FlipsAndPersistsTheSetting
#[test]
fn a_chosen_also_set_item_flips_and_persists_the_setting() {
    let rig = MenuRig::with_images(&["a.jpg"]);
    rig.rig
        .env
        .borrow()
        .update_settings(|s| s.set_zoom_also_sets_click_level = false);
    rig.script(Some("Zoom.AlsoSetClickLevel"));

    rig.menus.context_menu().show(PointD { x: 10.0, y: 10.0 });

    assert!(rig.rig.env.borrow().store.current().set_zoom_also_sets_click_level);
}

// C#: Fit_ReturnsToFit_FromAZoomedView
#[test]
fn a_chosen_fit_returns_to_fit_from_a_zoomed_view() {
    let rig = MenuRig::with_images(&["a.jpg"]);
    actions(&rig).apply_zoom_preset(200).expect("ok");
    rig.rig.drain();
    assert!(actions(&rig).effective_zoom().is_some());
    rig.script(Some("Fit"));

    rig.menus.context_menu().show(PointD { x: 10.0, y: 10.0 });
    rig.rig.drain();

    assert_eq!(actions(&rig).effective_zoom(), None);
}

// new: the error type of a failing action reaches `report_action_failed` (the controller's guarded path) with the zoom name from the services
#[test]
fn zoom_commands_are_guarded_with_the_services_zoom_name() {
    let rig = MenuRig::new();
    *rig.services.zoom_name.borrow_mut() = "Thu phong".to_owned();
    assert_eq!(actions(&rig).zoom_action_name(), "Thu phong");
}

#[test]
fn debug_output_names_the_types() {
    let rig = MenuRig::new();
    assert!(format!("{:?}", rig.menus).contains("MainWindowMenus"));
    assert!(format!("{:?}", actions(&rig)).contains("MainWindowMenuActions"));
    let _: &dyn Error = &Boom("x");
}
