//! Pure tests of W8-15i input: press/release, wheel, keyboard, button clicks and the Tools menu of `ShellOverlay`.
//! Port of the click, key and menu half of `ShellOverlayBehaviorTests` and three tests of `OverlayPartsTests` (baseline `d92d6aae`). The fake clock runs at
//! 1 tick = 1 ms; the Tools menu is a `FakeMenuHost`. No sleeps, no OS. The four C# tests about faulting commands live in W8-15j (contract item 6).

#[path = "common/overlay_fakes.rs"]
mod overlay_fakes;
#[path = "common/overlay_layout_fixture.rs"]
mod overlay_layout_fixture;
#[path = "common/overlay_main_window_fixture.rs"]
mod overlay_main_window_fixture;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use overlay_main_window_fixture::{FAR_AWAY, InputRig, MenuAnswer};
use pr_app::input::primitives::PointD;
use pr_app::overlay::tools_menu::TOOLS_IDS;
use pr_platform_win::shell_overlay::messages::OverlayInput;
use pr_platform_win::shell_overlay::native::{VK_RETURN, VK_SPACE, VK_TAB};
use pr_platform_win::shell_overlay::overlay::state::OverlayButton;

const VK_A: i32 = 0x41;
const TOOLBAR_BUTTONS: [OverlayButton; 4] = [
    OverlayButton::OpenFolder,
    OverlayButton::Settings,
    OverlayButton::Fit,
    OverlayButton::Tools,
];

fn rig() -> InputRig {
    InputRig::new(true, false)
}

/// A rig whose status panel shows the skipped row (the "View list" button is visible).
fn rig_with_skipped() -> InputRig {
    let rig = rig();
    rig.full_model();
    rig.overlay.arrange(
        pr_app::input::primitives::SizeD {
            width: 1000.0,
            height: 700.0,
        },
        1.0,
    );
    rig
}

fn is_pressed(rig: &InputRig, button: OverlayButton) -> bool {
    match button {
        OverlayButton::ViewList => rig.overlay.with_status(|s| s.view_list().is_pressed),
        other => rig.overlay.with_toolbar(|t| {
            let index = [
                OverlayButton::OpenFolder,
                OverlayButton::Settings,
                OverlayButton::Fit,
                OverlayButton::Tools,
            ]
            .iter()
            .position(|b| *b == other)
            .unwrap();
            t.buttons()[index].is_pressed
        }),
    }
}

// Buttons --------------------------------------------------------------------------------------------------------------

// C#: ShellOverlayBehaviorTests.Buttons_ClickInvokesTheMatchingCommand
#[test]
fn buttons_click_invokes_the_matching_command() {
    let rig = rig_with_skipped();
    for (button, expected) in [
        (OverlayButton::OpenFolder, "OpenFolder"),
        (OverlayButton::Settings, "Settings"),
        (OverlayButton::Fit, "Fit"),
        (OverlayButton::ViewList, "Skipped"),
    ] {
        rig.calls.borrow_mut().clear();
        rig.click(button);
        assert_eq!(rig.log(), vec![expected.to_owned()], "{button:?}");
    }
}

// C#: ShellOverlayBehaviorTests.Buttons_ReleasingOutsideTheButtonDoesNotClick
#[test]
fn buttons_releasing_outside_the_button_does_not_click() {
    let rig = rig();
    let point = rig.center(OverlayButton::Fit);
    assert!(rig.overlay.pointer_down(point));
    assert_eq!(rig.overlay.pressed_button(), Some(OverlayButton::Fit));
    assert!(rig.overlay.pointer_up(FAR_AWAY));
    assert!(rig.log().is_empty());
    assert_eq!(rig.overlay.pressed_button(), None);
    assert!(!is_pressed(&rig, OverlayButton::Fit));
}

// C#: ShellOverlayBehaviorTests.ClickThrough_HiddenToolbarAndHiddenInfoLetTheImageHaveTheClick
#[test]
fn click_through_hidden_toolbar_and_hidden_info_let_the_image_have_the_click() {
    let rig = rig_with_skipped();
    let fit = rig.center(OverlayButton::Fit);
    let view_list = rig.center(OverlayButton::ViewList);
    rig.overlay.pointer_moved(FAR_AWAY);
    rig.advance(1500 + 3000 + 400);
    assert!(!rig.overlay.toolbar_hit_test_visible());
    assert!(!rig.overlay.info_hit_test_visible());
    assert!(!rig.overlay.pointer_down(fit));
    assert!(!rig.overlay.pointer_down(view_list));
    assert!(!rig.overlay.is_pointer_down());
    assert!(!rig.overlay.pointer_up(fit));
    assert!(rig.log().is_empty());
}

// C#: ShellOverlayBehaviorTests.Panels_AbsorbTheClickButEmptySpaceDoesNot
#[test]
fn panels_absorb_the_click_but_empty_space_does_not() {
    let rig = rig();
    let bar = rig.bounds_of("ToolbarPanel");
    let fit = rig.bounds_of(OverlayButton::Fit.element_id());
    // A point inside the toolbar panel but outside every button: the panel absorbs it.
    let gap = PointD {
        x: bar.x + 1.0,
        y: bar.y + bar.height / 2.0,
    };
    assert!(fit.x > gap.x);
    assert!(rig.overlay.pointer_down(gap));
    assert!(rig.overlay.is_pointer_down());
    assert_eq!(rig.overlay.pressed_button(), None);
    assert!(rig.overlay.pointer_up(gap));
    assert!(!rig.overlay.is_pointer_down());
    assert!(rig.log().is_empty());

    assert!(!rig.overlay.pointer_down(FAR_AWAY));
    assert!(!rig.overlay.is_pointer_down());
}

// C#: ShellOverlayBehaviorTests.PointerCancel_DropsThePressWithoutClicking
#[test]
fn pointer_cancel_drops_the_press_without_clicking() {
    let rig = rig();
    let point = rig.center(OverlayButton::Settings);
    assert!(rig.overlay.pointer_down(point));
    assert!(is_pressed(&rig, OverlayButton::Settings));
    let before = rig.render_requests.get();
    rig.overlay.pointer_cancel();
    assert!(rig.render_requests.get() > before);
    assert!(!rig.overlay.is_pointer_down());
    assert_eq!(rig.overlay.pressed_button(), None);
    assert!(!is_pressed(&rig, OverlayButton::Settings));
    assert!(!rig.overlay.pointer_up(point)); // T1: a release after a cancel is not the overlay's
    assert!(rig.log().is_empty());
}

// C#: ShellOverlayBehaviorTests.AfterDispose_InputAndUpdatesAreIgnored (input half)
#[test]
fn after_dispose_input_is_ignored() {
    let rig = rig();
    let point = rig.center(OverlayButton::Fit);
    rig.overlay.dispose();
    assert!(!rig.overlay.pointer_down(point));
    assert!(!rig.overlay.pointer_up(point));
    assert!(!rig.overlay.key_down(VK_TAB, false, false));
    assert!(!rig.overlay.hit_test(point));
    assert!(rig.log().is_empty());
}

// new: pointer_cancel_after_dispose_does_not_panic
#[test]
fn pointer_cancel_after_dispose_does_not_panic() {
    let rig = rig();
    assert!(rig.overlay.pointer_down(rig.center(OverlayButton::Fit)));
    rig.overlay.dispose();
    rig.overlay.pointer_cancel();
    assert!(!rig.overlay.is_pointer_down());
    assert_eq!(rig.overlay.pressed_button(), None);
}

// new: release_clears_pressed_before_the_click
#[test]
fn release_clears_pressed_before_the_click() {
    let rig = rig();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let weak = rig.weak_overlay();
    let log = Rc::clone(&seen);
    *rig.command_hook.borrow_mut() = Some(Rc::new(move |_| {
        let overlay = weak.upgrade().unwrap();
        log.borrow_mut()
            .push((overlay.pressed_button(), overlay.is_pointer_down()));
    }));
    rig.click(OverlayButton::Fit);
    assert_eq!(*seen.borrow(), vec![(None, false)]);
    assert!(!is_pressed(&rig, OverlayButton::Fit));
}

// new: pointer_up_over_a_different_button_does_not_click_and_still_swallows
#[test]
fn pointer_up_over_a_different_button_does_not_click_and_still_swallows() {
    let rig = rig();
    assert!(rig.overlay.pointer_down(rig.center(OverlayButton::Fit)));
    assert!(rig.overlay.pointer_up(rig.center(OverlayButton::Settings)));
    assert!(rig.log().is_empty());
}

// Wheel ----------------------------------------------------------------------------------------------------------------

// C#: OverlayPartsTests.Wheel_IsConsumedOverAVisiblePanel_AndPassedThroughElsewhere
#[test]
fn wheel_is_consumed_over_a_visible_panel_and_passed_through_elsewhere() {
    let rig = rig();
    assert!(rig.overlay.wheel(rig.center(OverlayButton::Fit)));
    assert!(!rig.overlay.wheel(FAR_AWAY));
}

// new: wheel_over_a_hidden_info_layer_is_not_consumed
#[test]
fn wheel_over_a_hidden_info_layer_is_not_consumed() {
    let rig = rig_with_skipped();
    let view_list = rig.center(OverlayButton::ViewList);
    assert!(rig.overlay.wheel(view_list));
    rig.overlay.pointer_moved(FAR_AWAY);
    rig.advance(3000 + 400);
    assert!(!rig.overlay.info_hit_test_visible());
    assert!(!rig.overlay.wheel(view_list));
}

// C#: ShellOverlayBehaviorTests.InfoOverlay_NavigationAndKeysAreActivity_AndTheyBringItBack (wheel and key lines)
#[test]
fn wheel_and_keys_are_info_activity_and_bring_the_overlay_back() {
    let rig = rig_with_skipped();
    rig.overlay.pointer_moved(FAR_AWAY);
    rig.advance(3000 + 400);
    assert_eq!(rig.overlay.info_opacity(), 0.0);
    assert!(!rig.overlay.wheel(FAR_AWAY)); // the wheel is activity even when nothing is hit
    assert!(rig.overlay.info_hit_test_visible());
    rig.advance(3000 + 400);
    assert_eq!(rig.overlay.info_opacity(), 0.0);
    assert!(!rig.overlay.key_down(VK_A, false, false)); // any key is activity
    assert!(rig.overlay.info_hit_test_visible());
}

// Keyboard -------------------------------------------------------------------------------------------------------------

// C#: ShellOverlayBehaviorTests.Keyboard_TabWalksTheButtonsAndLeavesAfterTheLast
#[test]
fn keyboard_tab_walks_the_buttons_and_leaves_after_the_last() {
    let rig = rig();
    for (index, _) in TOOLBAR_BUTTONS.iter().enumerate() {
        assert!(rig.overlay.key_down(VK_TAB, false, false));
        assert_eq!(rig.overlay.with_toolbar(|t| t.focused_index()), Some(index));
        assert!(rig.overlay.has_keyboard_focus());
    }
    rig.overlay.key_down(VK_TAB, false, false); // the return value of the last Tab is a C# quirk, not pinned
    assert!(!rig.overlay.has_keyboard_focus());
}

// new: tab_always_swallows_unless_repeating
#[test]
fn tab_always_swallows_unless_repeating() {
    let rig = rig();
    for _ in 0..4 {
        assert!(rig.overlay.key_down(VK_TAB, false, false));
    }
    // The fifth Tab moves the focus out of the toolbar and is still swallowed (C# quirk, contract Q4).
    assert!(rig.overlay.key_down(VK_TAB, false, false));
    assert!(!rig.overlay.has_keyboard_focus());
    // Auto-repeat without focus is left to the image.
    assert!(!rig.overlay.key_down(VK_TAB, false, true));
}

// C#: ShellOverlayBehaviorTests.Keyboard_ShiftTabStartsFromTheLastButton
#[test]
fn keyboard_shift_tab_starts_from_the_last_button() {
    let rig = rig();
    assert!(rig.overlay.key_down(VK_TAB, true, false));
    assert_eq!(rig.overlay.with_toolbar(|t| t.focused_index()), Some(3));
    assert!(rig.overlay.key_down(VK_TAB, true, false));
    assert_eq!(rig.overlay.with_toolbar(|t| t.focused_index()), Some(2));
}

// C#: OverlayPartsTests.Tab_AutoRepeatDoesNotMoveTheFocus
#[test]
fn tab_auto_repeat_does_not_move_the_focus() {
    let rig = rig();
    assert!(!rig.overlay.key_down(VK_TAB, false, true)); // no focus: the image gets the Tab
    assert!(!rig.overlay.has_keyboard_focus());
    assert!(rig.overlay.key_down(VK_TAB, false, false));
    assert!(rig.overlay.key_down(VK_TAB, false, true)); // focus held: swallowed, not moved
    assert_eq!(rig.overlay.with_toolbar(|t| t.focused_index()), Some(0));
}

// C#: ShellOverlayBehaviorTests.Keyboard_SpaceAndEnterActivateTheFocusedButton_AutoRepeatIgnored
#[test]
fn keyboard_space_and_enter_activate_the_focused_button_auto_repeat_ignored() {
    for key in [VK_SPACE, VK_RETURN] {
        let rig = rig();
        rig.overlay.key_down(VK_TAB, false, false);
        rig.overlay.key_down(VK_TAB, false, false);
        rig.overlay.key_down(VK_TAB, false, false); // Fit
        assert!(rig.overlay.key_down(key, false, true)); // repeat: swallowed, no click
        assert!(rig.log().is_empty());
        assert!(rig.overlay.key_down(key, false, false));
        assert_eq!(rig.log(), vec!["Fit".to_owned()]);
    }
}

// C#: ShellOverlayBehaviorTests.Keyboard_SpaceWithoutFocusIsLeftToTheImage
#[test]
fn keyboard_space_without_focus_is_left_to_the_image() {
    let rig = rig();
    assert!(!rig.overlay.key_down(VK_SPACE, false, false));
    assert!(!rig.overlay.key_down(VK_RETURN, false, false));
    assert!(rig.log().is_empty());
}

// C#: ShellOverlayBehaviorTests.Keyboard_FocusInsideKeepsAHiddenToolbarVisible_AndBringsItBack
#[test]
fn keyboard_focus_inside_keeps_a_hidden_toolbar_visible_and_brings_it_back() {
    let rig = rig();
    rig.overlay.pointer_moved(FAR_AWAY);
    rig.advance(1500 + 1 + 200);
    assert_eq!(rig.overlay.toolbar_opacity(), 0.0);
    assert!(rig.overlay.key_down(VK_TAB, false, false));
    assert!(rig.overlay.toolbar_hit_test_visible());
    rig.advance(5000);
    assert_eq!(rig.overlay.toolbar_opacity(), 1.0); // focus keeps it up past the hide delay
}

// C#: ShellOverlayBehaviorTests.Keyboard_ClickingTheImageTakesTheFocusBack
#[test]
fn keyboard_clicking_the_image_takes_the_focus_back() {
    let rig = rig();
    rig.overlay.key_down(VK_TAB, false, false);
    assert!(rig.overlay.has_keyboard_focus());
    assert!(!rig.overlay.pointer_down(FAR_AWAY)); // T7: image click
    assert!(!rig.overlay.has_keyboard_focus());
    // A press on a panel also takes the focus.
    rig.overlay.key_down(VK_TAB, false, false);
    assert!(rig.overlay.pointer_down(rig.center(OverlayButton::Settings)));
    assert!(!rig.overlay.has_keyboard_focus());
}

// Tools menu -----------------------------------------------------------------------------------------------------------

// C#: ShellOverlayBehaviorTests.ToolsButton_ShowsTheToolsMenuBelowTheButton_InScreenDips
#[test]
fn tools_button_shows_the_tools_menu_below_the_button_in_screen_dips() {
    let rig = rig();
    rig.click(OverlayButton::Tools);
    let shown = rig.menus.shown.borrow();
    assert_eq!(shown.len(), 1);
    let bar = rig.bounds_of(OverlayButton::Tools.element_id());
    // The fixture's client origin on screen is (100, 50).
    assert_eq!(
        shown[0].1,
        PointD {
            x: 100.0 + bar.x,
            y: 50.0 + bar.y + bar.height
        }
    );
    let ids: Vec<&str> = shown[0].0.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, TOOLS_IDS.to_vec());
}

// C#: ShellOverlayBehaviorTests.ToolsMenu_ChosenItemRunsItsCommand
#[test]
fn tools_menu_chosen_item_runs_its_command() {
    for (id, expected) in [
        ("Tools.Recovery", "Recovery"),
        ("Tools.Diagnostics", "Diagnostics"),
        ("Tools.Benchmark", "Benchmark"),
        ("Tools.ClearCache", "ClearCache"),
        ("Tools.RemoveNumberedDuplicates", "RemoveNumbered"),
        ("Tools.RemoveOriginalDuplicates", "RemoveOriginal"),
    ] {
        let rig = rig();
        *rig.menus.answer.borrow_mut() = MenuAnswer::Chosen(id);
        rig.click(OverlayButton::Tools);
        assert_eq!(rig.log(), vec![expected.to_owned()], "{id}");
    }
}

// C#: ShellOverlayBehaviorTests.ToolsMenu_DuplicateRemovalIsDisabledAndNoOpWithoutImages
#[test]
fn tools_menu_duplicate_removal_is_disabled_and_no_op_without_images() {
    let rig = InputRig::new(false, false);
    *rig.menus.answer.borrow_mut() = MenuAnswer::Chosen("Tools.RemoveNumberedDuplicates");
    rig.click(OverlayButton::Tools);
    let shown = rig.menus.shown.borrow();
    let enabled = |id: &str| shown[0].0.iter().find(|(i, _)| i == id).unwrap().1;
    assert!(!enabled("Tools.RemoveNumberedDuplicates"));
    assert!(!enabled("Tools.RemoveOriginalDuplicates"));
    assert!(enabled("Tools.Recovery"));
    assert!(rig.log().is_empty());
}

// C#: ShellOverlayBehaviorTests.ToolsMenu_WithoutAHostTheButtonDoesNothing
#[test]
fn tools_menu_without_a_host_the_button_does_nothing() {
    use pr_platform_win::shell_overlay::overlay::state::{ShellOverlay, ShellOverlayOptions};
    let overlay = ShellOverlay::new(ShellOverlayOptions {
        text: Rc::new(overlay_fakes::FakeTextRenderer::new()),
        text_renderer_id: 1,
        texts: pr_l10n::localizer::built_in_english_localizer(),
        clock: Rc::new(overlay_layout_fixture::FakeFrameClock::default()),
        settings: Box::new(|| std::sync::Arc::new(pr_core::settings::app_settings::AppSettings::default())),
        commands: Box::new(overlay_layout_fixture::FakeCommands {
            calls: Rc::new(RefCell::new(Vec::new())),
        }),
        request_render: Box::new(|| {}),
        is_window_active: Box::new(|| true),
        menus: None,
        tooltips: None,
        client_origin_on_screen_dip: None,
        on_command_failed: None,
        on_menu_failed: None,
        timestamp: Some(Box::new(|| 0)),
        ticks_per_second: Some(1000),
        animator_ticks_per_second: Some(1000),
    });
    overlay.arrange(
        pr_app::input::primitives::SizeD {
            width: 1000.0,
            height: 700.0,
        },
        1.0,
    );
    let tools = overlay_layout_fixture::center_of(overlay.element_bounds("ToolsButton").unwrap());
    assert!(overlay.pointer_down(tools));
    assert!(overlay.pointer_up(tools));
    assert!(!overlay.is_tools_menu_open());
    assert!(!overlay.with_toolbar(|t| t.tools().is_checked));
}

// C#: ShellOverlayBehaviorTests.ToolsMenu_ReentrantClickIsIgnoredWhileOpen (T2)
#[test]
fn tools_menu_reentrant_click_is_ignored_while_open() {
    let rig = rig();
    let weak = rig.weak_overlay();
    let point = rig.center(OverlayButton::Tools);
    let open_inside = Rc::new(Cell::new(false));
    let seen = Rc::clone(&open_inside);
    *rig.menus.while_open.borrow_mut() = Some(Rc::new(move || {
        let overlay = weak.upgrade().unwrap();
        seen.set(overlay.is_tools_menu_open());
        overlay.pointer_down(point);
        overlay.pointer_up(point); // the Tools button again, from inside the modal menu
    }));
    rig.click(OverlayButton::Tools);
    assert!(open_inside.get());
    assert_eq!(rig.menus.shown.borrow().len(), 1);
    assert!(!rig.overlay.is_tools_menu_open());
}

// new: show_tools_menu_does_not_hold_a_borrow (T2)
#[test]
fn show_tools_menu_does_not_hold_a_borrow() {
    let rig = rig();
    let weak = rig.weak_overlay();
    *rig.menus.while_open.borrow_mut() = Some(Rc::new(move || {
        let overlay = weak.upgrade().unwrap();
        // Messages pumped by the modal loop reach the overlay: none of these may hit a `BorrowMutError`.
        overlay.pointer_moved(PointD { x: 5.0, y: 5.0 });
        overlay.pointer_left();
        overlay.wheel(PointD { x: 5.0, y: 5.0 });
        overlay.key_down(VK_A, false, false);
        overlay.settings_changed();
        let _ = overlay.collect_tooltips();
    }));
    rig.click(OverlayButton::Tools);
    assert_eq!(rig.menus.shown.borrow().len(), 1);
}

// C#: ShellOverlayBehaviorTests.Toolbar_DoesNotHideWhileTheToolsMenuIsOpen_AndCountsDownAfterItCloses
#[test]
fn toolbar_does_not_hide_while_the_tools_menu_is_open_and_counts_down_after_it_closes() {
    let rig = rig();
    let weak = rig.weak_overlay();
    let opacity_inside = Rc::new(Cell::new(-1.0f32));
    let seen = Rc::clone(&opacity_inside);
    // The fixture advances the fake clock from inside the modal menu: 5 s with the mouse far away.
    let rig_now = Rc::clone(&rig.now);
    let clock = Rc::clone(&rig.clock);
    *rig.menus.while_open.borrow_mut() = Some(Rc::new(move || {
        let overlay = weak.upgrade().unwrap();
        overlay.pointer_moved(FAR_AWAY);
        for _ in 0..5000 {
            rig_now.set(rig_now.get() + 1);
            clock.tick(rig_now.get());
        }
        seen.set(overlay.toolbar_opacity());
    }));
    rig.click(OverlayButton::Tools);
    assert_eq!(opacity_inside.get(), 1.0);
    assert!(rig.overlay.toolbar_hit_test_visible());
    rig.advance(1499);
    assert!(rig.overlay.toolbar_hit_test_visible());
    rig.advance(1); // 1500 ms AFTER the menu closed
    assert!(!rig.overlay.toolbar_hit_test_visible());
}

// new: the Tools button is checked while the menu is open and unchecked after
#[test]
fn tools_button_is_checked_while_the_menu_is_open() {
    let rig = rig();
    let weak = rig.weak_overlay();
    let checked_inside = Rc::new(Cell::new(false));
    let seen = Rc::clone(&checked_inside);
    *rig.menus.while_open.borrow_mut() = Some(Rc::new(move || {
        seen.set(weak.upgrade().unwrap().with_toolbar(|t| t.tools().is_checked));
    }));
    rig.click(OverlayButton::Tools);
    assert!(checked_inside.get());
    assert!(!rig.overlay.with_toolbar(|t| t.tools().is_checked));
}

// C#: ShellOverlayBehaviorTests.ToolsMenu_WhenTheHostThrows_ReportsTheFailureAndRestoresTheToolbarState
#[test]
fn tools_menu_when_the_host_fails_reports_the_failure_and_restores_the_toolbar_state() {
    let rig = InputRig::new(true, true);
    *rig.menus.answer.borrow_mut() = MenuAnswer::Fails("boom");
    let before = rig.render_requests.get();
    rig.click(OverlayButton::Tools);
    assert_eq!(*rig.menu_failures.borrow(), vec!["boom".to_owned()]);
    assert!(rig.command_failures.borrow().is_empty());
    assert!(!rig.overlay.is_tools_menu_open());
    assert!(!rig.overlay.with_toolbar(|t| t.tools().is_checked));
    assert!(rig.render_requests.get() > before);
    assert!(rig.log().is_empty());
}

// new: show_tools_menu_menu_error_falls_back_to_on_command_failed
#[test]
fn show_tools_menu_menu_error_falls_back_to_on_command_failed() {
    let rig = InputRig::new(true, false);
    *rig.menus.answer.borrow_mut() = MenuAnswer::Fails("no menu");
    rig.click(OverlayButton::Tools);
    assert_eq!(
        *rig.command_failures.borrow(),
        vec![("Tools menu".to_owned(), "no menu".to_owned())]
    );
    assert!(!rig.overlay.is_tools_menu_open());
}

// C#: ShellOverlayBehaviorTests.ToolsMenu_WhenACommandThrowsSynchronously_ReportsItInsteadOfPropagating (retargeted, contract item 6)
#[test]
fn tools_menu_executes_the_chosen_command_and_reports_nothing_itself() {
    let rig = rig();
    *rig.menus.answer.borrow_mut() = MenuAnswer::Chosen("Tools.Diagnostics");
    rig.click(OverlayButton::Tools);
    assert_eq!(rig.log(), vec!["Diagnostics".to_owned()]);
    assert!(rig.command_failures.borrow().is_empty());
}

// new: the tools state is restored BEFORE the chosen command runs
#[test]
fn tools_state_is_restored_before_the_chosen_command_runs() {
    let rig = rig();
    *rig.menus.answer.borrow_mut() = MenuAnswer::Chosen("Tools.Recovery");
    let weak = rig.weak_overlay();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&seen);
    *rig.command_hook.borrow_mut() = Some(Rc::new(move |_| {
        let overlay = weak.upgrade().unwrap();
        log.borrow_mut().push(overlay.is_tools_menu_open());
    }));
    rig.click(OverlayButton::Tools);
    assert_eq!(*seen.borrow(), vec![false]);
}

// new: a chosen item is dropped when the overlay was disposed while the menu was open
#[test]
fn a_chosen_item_is_dropped_when_the_overlay_is_disposed_while_the_menu_is_open() {
    let rig = rig();
    *rig.menus.answer.borrow_mut() = MenuAnswer::Chosen("Tools.Recovery");
    let weak = rig.weak_overlay();
    *rig.menus.while_open.borrow_mut() = Some(Rc::new(move || weak.upgrade().unwrap().dispose()));
    rig.click(OverlayButton::Tools);
    assert!(rig.log().is_empty());
}

// new: a command that clicks another button from inside itself is dropped (try_borrow_mut)
#[test]
fn a_reentrant_command_click_is_dropped() {
    let rig = rig();
    let weak = rig.weak_overlay();
    let fit = rig.center(OverlayButton::Fit);
    let settings = rig.center(OverlayButton::Settings);
    *rig.command_hook.borrow_mut() = Some(Rc::new(move |name| {
        if name == "Fit" {
            let overlay = weak.upgrade().unwrap();
            overlay.pointer_down(settings);
            overlay.pointer_up(settings);
        }
    }));
    assert!(rig.overlay.pointer_down(fit));
    assert!(rig.overlay.pointer_up(fit));
    assert_eq!(rig.log(), vec!["Fit".to_owned()]); // the nested Settings click found the commands cell borrowed
}

// OverlayInput trait ---------------------------------------------------------------------------------------------------

// new: impl_overlay_input_forwards_every_method
#[test]
fn impl_overlay_input_forwards_every_method() {
    let rig = rig();
    let input: &dyn OverlayInput = &*rig.overlay;
    let fit = rig.center(OverlayButton::Fit);
    input.pointer_moved(fit);
    assert!(rig.overlay.with_toolbar(|t| t.fit().is_hot));
    input.pointer_left();
    assert!(!rig.overlay.with_toolbar(|t| t.fit().is_hot));
    assert!(input.pointer_down(fit));
    assert!(input.is_pointer_down());
    input.pointer_cancel();
    assert!(!input.is_pointer_down());
    assert!(input.pointer_down(fit));
    assert!(input.pointer_up(fit));
    assert_eq!(rig.log(), vec!["Fit".to_owned()]);
    assert!(input.wheel(fit));
    assert!(input.key_down(VK_TAB, false, false));
    assert!(!input.key_down(VK_A, false, false));
}
