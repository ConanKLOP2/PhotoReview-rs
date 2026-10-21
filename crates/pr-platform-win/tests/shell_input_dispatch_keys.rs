//! Tests of W8-14b: the keyboard half of `ShellInput` (C# `KeyboardInputTests`, baseline `d92d6aae`): decision order of `HandleKeyDown`, command execution.
//!
//! Test kind F: the pure controllers of `pr_app` run for real, the window, the host, the frame clock and the dispatcher are fakes
//! (`common/shell_input_dispatch_fixture.rs`, the Rust twin of `InputTestSupport.cs`); the viewport is the REAL `ViewportController`.
#![cfg(windows)]

#[path = "common/shell_input_dispatch_fixture.rs"]
mod fixture;
#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use std::cell::Cell;
use std::rc::Rc;

use fixture::{LogLevel, Rig};
use pr_app::input::key_id::KeyId;
use pr_app::input::primitives::KeyModifiers;
use pr_app::input::review_command::{ReviewCommand, ReviewCommandType};
use pr_platform_win::shell_input::dispatch::Predicate;

fn cmd(kind: ReviewCommandType) -> ReviewCommand {
    ReviewCommand::new(kind)
}

/// A rig whose layout has converged at Fit. The C# rig's `Viewer.PropertyChanged -> InvalidateLayout` re-runs the layout after the first pass
/// (`UpdateFitSize` changes the viewer); the Rust fixture has no change event, so one more pass is requested by hand. Without it the first-pass
/// extent is the unfitted 3000x2000 and an arrow key would pan instead of navigating.
fn fitted_rig() -> Rig {
    let rig = Rig::new();
    rig.viewport.invalidate_layout();
    rig.drain();
    rig
}

fn predicate(f: impl Fn() -> bool + 'static) -> Option<Predicate> {
    Some(Rc::new(f))
}

// ---- key -> command mapping from the settings ----

// C#: KeyboardInputTests.DefaultShortcuts_AreResolvedFromTheVirtualKey_AndReachTheHost
#[test]
fn default_shortcuts_are_resolved_from_the_virtual_key_and_reach_the_host() {
    let cases = [
        (KeyId::RIGHT, ReviewCommandType::Next),
        (KeyId::LEFT, ReviewCommandType::Previous),
        (KeyId::DELETE, ReviewCommandType::Recycle),
        (KeyId::SPACE, ReviewCommandType::Skip),
        (KeyId::F11, ReviewCommandType::Fullscreen),
        (KeyId::C, ReviewCommandType::ToggleCompare),
        (KeyId::HOME, ReviewCommandType::FirstImage),
        (KeyId::END, ReviewCommandType::LastImage),
        (KeyId::PAGE_DOWN, ReviewCommandType::NextFolder),
        (KeyId::PAGE_UP, ReviewCommandType::PreviousFolder),
        (KeyId::I, ReviewCommandType::ToggleInfoOverlay),
        (KeyId::K, ReviewCommandType::ToggleKeepZoom),
        (KeyId::D3, ReviewCommandType::CustomZoom),
        (KeyId::R, ReviewCommandType::Refresh),
    ];
    for (key, expected) in cases {
        let rig = fitted_rig();
        rig.host.current_has_compare_pair.set(true);

        let (handled, _) = rig.key_down(key, false, false);

        assert!(handled, "{key:?}");
        assert_eq!(vec![cmd(expected)], rig.host.commands_snapshot(), "{key:?}");
    }
}

// C#: KeyboardInputTests.ViewportCommands_RunOnTheControllerNotTheHost
#[test]
fn viewport_commands_run_on_the_controller_not_the_host() {
    let rig = Rig::new();
    rig.fit();
    assert!(rig.viewer.borrow().is_fit());

    rig.key_down(KeyId::D1, false, false); // ZoomActualSize
    assert!(!rig.viewer.borrow().is_fit());
    assert!((rig.viewer.borrow().zoom() - 1.0).abs() < 1e-9);
    rig.key_down(KeyId::F, false, false); // ToggleFit
    rig.drain();

    assert!(rig.viewer.borrow().is_fit());
    assert!(rig.host.commands_snapshot().is_empty());
}

// C#: KeyboardInputTests.CustomShortcutFromSettings_IsHonoured_AndRebuildSwapsTheTable
#[test]
fn custom_shortcut_from_settings_is_honoured_and_rebuild_swaps_the_table() {
    let rig = Rig::configured(|s| s.shortcuts.skip = "J".to_owned());
    assert!(rig.key_down(KeyId::J, false, false).0);
    assert_eq!(vec![ReviewCommand::SKIP], rig.host.commands_snapshot());
    rig.host.commands.borrow_mut().clear();
    assert!(!rig.key_down(KeyId::SPACE, false, false).0); // Space is no longer Skip

    rig.update_settings(|s| s.shortcuts.skip = "Q".to_owned());
    let settings = rig.settings.borrow().clone();
    rig.input.rebuild_shortcuts(&settings);
    assert!(rig.key_down(KeyId::Q, false, false).0);
    assert!(!rig.key_down(KeyId::J, false, false).0);
    assert_eq!(vec![ReviewCommand::SKIP], rig.host.commands_snapshot());
}

// C#: KeyboardInputTests.ActionProfileShortcut_RunsThatProfileIndex
#[test]
fn action_profile_shortcut_runs_that_profile_index() {
    let rig = Rig::new();
    let settings = rig.settings.borrow().clone();
    let (index, shortcut) = settings
        .actions
        .iter()
        .enumerate()
        .map(|(i, a)| (i, a.shortcut.clone()))
        .find(|(_, shortcut)| !shortcut.is_empty())
        .expect("a default action with a shortcut");
    let key = pr_app::services::shortcut_key_name::try_parse(Some(&shortcut)).expect("the shortcut parses");

    rig.key_down(key, false, false);

    assert_eq!(vec![ReviewCommand::action(index as i32)], rig.host.commands_snapshot());
}

// C#: KeyboardInputTests.UnboundKey_IsNotHandled_SoWindowsKeepsIt
#[test]
fn unbound_key_is_not_handled_so_windows_keeps_it() {
    let rig = Rig::new();
    let (handled, _) = rig.key_down(KeyId::F9, false, false);
    assert!(!handled);
    assert!(rig.host.commands_snapshot().is_empty());
}

// C#: KeyboardInputTests.NoImages_StillResolvesCommands_ButTheRouterDecides
#[test]
fn no_images_still_resolves_commands_but_the_router_decides() {
    let rig = Rig::new();
    rig.host.has_images.set(false);
    // Fullscreen/OpenFolder/Close need no image; Next: the router returns null without images.
    assert!(rig.key_down(KeyId::F11, false, false).0);
    assert!(!rig.key_down(KeyId::RIGHT, false, false).0);
    assert_eq!(vec![ReviewCommand::FULLSCREEN], rig.host.commands_snapshot());
}

// ---- modifiers ----

// C#: KeyboardInputTests.Modifiers_AreReadFromTheEnvironment_ShiftForcesThePicker_CtrlRequiredForUndo
#[test]
fn modifiers_are_read_from_the_environment_shift_forces_the_picker_ctrl_required_for_undo() {
    let rig = Rig::new();

    rig.modifiers.set(KeyModifiers::SHIFT);
    rig.key_down(KeyId::M, false, false);
    rig.modifiers.set(KeyModifiers::NONE);
    rig.key_down(KeyId::M, false, false);
    rig.key_down(KeyId::Z, false, false); // Undo needs Ctrl
    rig.modifiers.set(KeyModifiers::CONTROL);
    rig.key_down(KeyId::Z, false, false);

    assert_eq!(
        vec![
            ReviewCommand::move_to_folder(true),
            ReviewCommand::move_to_folder(false),
            ReviewCommand::UNDO
        ],
        rig.host.commands_snapshot()
    );
}

// ---- auto-repeat ----

// C#: KeyboardInputTests.AutoRepeat_IsDroppedForFileActions_ButKeptForNavigationAndZoom
#[test]
fn auto_repeat_is_dropped_for_file_actions_but_kept_for_navigation_and_zoom() {
    let rig = fitted_rig();

    let recycle = rig.key_down(KeyId::DELETE, true, false);
    let next = rig.key_down(KeyId::RIGHT, true, false);
    let fullscreen = rig.key_down(KeyId::F11, true, false);
    let info = rig.key_down(KeyId::I, true, false);

    assert!(recycle.0); // the key is still swallowed
    assert!(next.0);
    assert!(fullscreen.0);
    assert!(info.0);
    assert_eq!(vec![ReviewCommand::NEXT], rig.host.commands_snapshot());
}

// C#: KeyboardInputTests.FirstPress_OfAFileAction_Runs
#[test]
fn first_press_of_a_file_action_runs() {
    let rig = Rig::new();
    rig.key_down(KeyId::DELETE, false, false);
    rig.key_down(KeyId::DELETE, true, false);
    assert_eq!(vec![ReviewCommand::RECYCLE], rig.host.commands_snapshot());
}

// ---- Esc ----

// C#: KeyboardInputTests.Escape_ExitsFullscreen_ElseClosesTheWindow
#[test]
fn escape_exits_fullscreen_else_closes_the_window() {
    let rig = Rig::new();
    rig.key_down(KeyId::ESCAPE, false, false);
    rig.host.is_fullscreen.set(true);
    rig.key_down(KeyId::ESCAPE, false, false);

    assert_eq!(
        vec![ReviewCommand::CLOSE, ReviewCommand::EXIT_FULLSCREEN],
        rig.host.commands_snapshot()
    );
}

// C#: KeyboardInputTests.Escape_ClosesTheToolsPopupFirst_ThenCancelsADuplicateCheck_BeforeAnythingElse
#[test]
fn escape_closes_the_tools_popup_first_then_cancels_a_duplicate_check_before_anything_else() {
    let rig = Rig::new();
    let popup = Rc::new(Cell::new(true));
    let popup_for_interceptor = Rc::clone(&popup);
    rig.input.set_escape_interceptor(predicate(move || {
        let was = popup_for_interceptor.get();
        popup_for_interceptor.set(false);
        was
    }));
    rig.host.duplicate_check_running.set(true);
    rig.host.is_fullscreen.set(true);

    assert!(rig.key_down(KeyId::ESCAPE, false, false).0); // closes the popup, leaves the duplicate check alone
    assert!(rig.host.duplicate_check_running.get());
    assert!(rig.key_down(KeyId::ESCAPE, false, false).0); // cancels the hashing
    assert!(!rig.host.duplicate_check_running.get());
    assert!(rig.host.commands_snapshot().is_empty());
    assert!(rig.key_down(KeyId::ESCAPE, false, false).0); // leaves fullscreen
    assert_eq!(vec![ReviewCommand::EXIT_FULLSCREEN], rig.host.commands_snapshot());
}

// C#: KeyboardInputTests.EscapeInterceptors_AreNotConsultedForOtherKeys
#[test]
fn escape_interceptors_are_not_consulted_for_other_keys() {
    let rig = fitted_rig();
    let calls = Rc::new(Cell::new(0_u32));
    let calls_for_interceptor = Rc::clone(&calls);
    rig.input.set_escape_interceptor(predicate(move || {
        calls_for_interceptor.set(calls_for_interceptor.get() + 1);
        true
    }));
    rig.host.duplicate_check_running.set(true);

    rig.key_down(KeyId::RIGHT, false, false);

    assert_eq!(0, calls.get());
    assert!(rig.host.duplicate_check_running.get());
    assert_eq!(vec![ReviewCommand::NEXT], rig.host.commands_snapshot());
}

// ---- focus: Space/Enter belong to a button ----

// C#: KeyboardInputTests.SpaceAndEnter_BelongToAFocusedButton_ButOtherKeysStillWork
#[test]
fn space_and_enter_belong_to_a_focused_button_but_other_keys_still_work() {
    for key in [KeyId::SPACE, KeyId::ENTER] {
        let rig = fitted_rig();
        rig.input.set_activation_key_owned(predicate(|| true));

        assert!(!rig.key_down(key, false, false).0, "{key:?}");
        assert!(rig.key_down(KeyId::RIGHT, false, false).0, "{key:?}");
        rig.input.set_activation_key_owned(predicate(|| false));
        rig.key_down(KeyId::SPACE, false, false);

        assert_eq!(
            vec![ReviewCommand::NEXT, ReviewCommand::SKIP],
            rig.host.commands_snapshot(),
            "{key:?}"
        );
    }
}

// ---- system keys ----

// C#: KeyboardInputTests.SysKeyDown_ResolvesThroughTheSystemKey_AndAltF4IsLeftToWindows
#[test]
fn sys_key_down_resolves_through_the_system_key_and_alt_f4_is_left_to_windows() {
    let rig = Rig::configured(|s| s.shortcuts.fullscreen = "F10".to_owned());
    rig.modifiers.set(KeyModifiers::ALT);

    assert!(!rig.key_down(KeyId::F4, false, true).0); // Alt+F4: DefWindowProc closes the window
    assert!(rig.key_down(KeyId::F10, false, true).0);
    assert_eq!(vec![ReviewCommand::FULLSCREEN], rig.host.commands_snapshot());
}

// C#: KeyboardInputTests.SysKeyDown_WithAnArrow_DoesNotPan
#[test]
fn sys_key_down_with_an_arrow_does_not_pan() {
    let rig = Rig::new();
    rig.key_down(KeyId::D1, false, false);
    rig.modifiers.set(KeyModifiers::ALT);
    let before = rig.viewport.horizontal_offset();

    rig.key_down(KeyId::RIGHT, false, true);

    assert_eq!(before, rig.viewport.horizontal_offset());
}

// ---- arrow pan ----

// C#: KeyboardInputTests.ArrowPan_StepIsAPercentOfTheShorterViewportSide_SameOnBothAxes
#[test]
fn arrow_pan_step_is_a_percent_of_the_shorter_viewport_side_same_on_both_axes() {
    for (percent, step) in [(10, 59.0), (25, 147.5), (100, 590.0)] {
        let rig = Rig::configured(move |s| {
            s.arrow_pan_step_percent = percent;
            s.kinetic_pan_enabled = false;
        });
        rig.key_down(KeyId::D1, false, false);
        rig.drain();
        assert_eq!(
            (990.0, 590.0),
            (rig.viewport.viewport_width(), rig.viewport.viewport_height()),
            "{percent}"
        );

        let (x0, y0) = rig.offsets();

        assert!(rig.key_down(KeyId::RIGHT, false, false).0, "{percent}");
        assert!(
            (rig.viewport.horizontal_offset() - (x0 + step)).abs() < 1e-6,
            "{percent}"
        );
        assert!(rig.key_down(KeyId::DOWN, false, false).0, "{percent}");
        assert!((rig.viewport.vertical_offset() - (y0 + step)).abs() < 1e-6, "{percent}");
        assert!(rig.key_down(KeyId::LEFT, false, false).0, "{percent}");
        assert!((rig.viewport.horizontal_offset() - x0).abs() < 1e-6, "{percent}");
        assert!(rig.key_down(KeyId::UP, false, false).0, "{percent}");
        assert!((rig.viewport.vertical_offset() - y0).abs() < 1e-6, "{percent}");
        assert!(rig.host.commands_snapshot().is_empty(), "{percent}");
    }
}

// C#: KeyboardInputTests.ArrowKeys_AtFit_NavigateInsteadOfPanning
#[test]
fn arrow_keys_at_fit_navigate_instead_of_panning() {
    let rig = Rig::new();
    rig.fit();
    rig.key_down(KeyId::RIGHT, false, false);
    rig.key_down(KeyId::LEFT, false, false);
    assert_eq!(
        vec![ReviewCommand::NEXT, ReviewCommand::PREVIOUS],
        rig.host.commands_snapshot()
    );
}

// C#: KeyboardInputTests.ArrowKey_WithAModifier_OrWhileComparing_DoesNotPan
#[test]
fn arrow_key_with_a_modifier_or_while_comparing_does_not_pan() {
    let rig = Rig::configured(|s| s.kinetic_pan_enabled = false);
    rig.key_down(KeyId::D1, false, false);
    rig.drain();
    let x0 = rig.viewport.horizontal_offset();

    rig.modifiers.set(KeyModifiers::CONTROL);
    rig.key_down(KeyId::RIGHT, false, false);
    rig.modifiers.set(KeyModifiers::NONE);
    rig.host.is_compare_visible.set(true);
    rig.key_down(KeyId::RIGHT, false, false);

    assert_eq!(x0, rig.viewport.horizontal_offset());
}

// C#: KeyboardInputTests.KineticArrowPan_StartsAGlide_AndAnyOtherKeyStopsIt
#[test]
fn kinetic_arrow_pan_starts_a_glide_and_any_other_key_stops_it() {
    let rig = Rig::configured(|s| s.kinetic_pan_enabled = true);
    rig.key_down(KeyId::D1, false, false);
    rig.drain();
    let x0 = rig.viewport.horizontal_offset();

    rig.key_down(KeyId::RIGHT, false, false);
    assert_eq!(1, rig.clock.subscriber_count()); // one frame hook for the glide

    rig.timestamp.set(0);
    rig.clock.tick(0.0);
    rig.timestamp.set(rig.timestamp.get() + 160_000);
    rig.clock.tick(16.0);
    rig.drain();
    assert!(rig.viewport.horizontal_offset() > x0);

    rig.key_down(KeyId::F9, false, false); // any key stops the glide
    assert_eq!(0, rig.clock.subscriber_count());
}

// C#: KeyboardInputTests.ZoomChange_AndDeactivation_StopTheGlide
#[test]
fn zoom_change_and_deactivation_stop_the_glide() {
    let rig = Rig::configured(|s| s.kinetic_pan_enabled = true);
    rig.key_down(KeyId::D1, false, false);
    rig.drain();
    rig.key_down(KeyId::RIGHT, false, false);
    assert_eq!(1, rig.clock.subscriber_count());

    rig.window.raise_deactivated();
    assert_eq!(0, rig.clock.subscriber_count());

    rig.key_down(KeyId::RIGHT, false, false);
    assert_eq!(1, rig.clock.subscriber_count());
    // The Rust viewer has no change event: the main window forwards a zoom-mode change to `on_zoom_mode_changed` (see its doc).
    let _ = rig.viewer.borrow_mut().set_zoom(2.0);
    rig.input.on_zoom_mode_changed();
    assert_eq!(0, rig.clock.subscriber_count());
}

// ---- command failure ----

// C#: KeyboardInputTests.ACommandThatThrows_IsReportedNotCrashed
#[test]
fn a_command_that_throws_is_reported_not_crashed() {
    let rig = Rig::new();
    *rig.host.execute_failure.borrow_mut() = Some("boom".to_owned());

    let (handled, _) = rig.key_down(KeyId::DELETE, false, false);

    assert!(handled);
    assert_eq!(vec!["failed:Recycle:boom".to_owned()], rig.host.log_snapshot());
    assert!(rig.log.contains(LogLevel::Error, "Command Recycle failed"));
}
