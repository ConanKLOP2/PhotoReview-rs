//! Pure tests of W8-15h: `ShellOverlay` options, constructor, data entry points, layout, hover, hit testing, auto-hide and fading.
//! Port of the click-free half of `ShellOverlayBehaviorTests` and three tests of `OverlayPartsTests` (baseline `d92d6aae`). The fake clock and timestamp
//! run at 1 tick = 1 ms (`Rig::advance`); no sleeps, no OS.

#[path = "common/overlay_fakes.rs"]
mod overlay_fakes;
#[path = "common/overlay_layout_fixture.rs"]
mod overlay_layout_fixture;

use std::cell::Cell;
use std::rc::{Rc, Weak};

use overlay_layout_fixture::{FAR_AWAY, Rig, center_of};
use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::overlay::element::ElementNode;
use pr_app::overlay::model::OverlayModel;
use pr_platform_win::shell_overlay::overlay::layout::effective_dpi;
use pr_platform_win::shell_overlay::overlay::state::{
    FADE_IN_MS, FADE_OUT_MS, HOT_ZONE_MARGIN, OverlayButton, ShellOverlay,
};

fn rig() -> Rig {
    Rig::new(|_| {}, true)
}

fn assert_close(actual: f32, expected: f32, tolerance: f32) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected} +/- {tolerance}, got {actual}"
    );
}

// Constants ------------------------------------------------------------------------------------------------------------

#[test]
fn constants_match_the_csharp_class() {
    // C#: ShellOverlay.HotZoneMargin / FadeInMs / FadeOutMs
    assert_eq!(HOT_ZONE_MARGIN, 24.0);
    assert_eq!(FADE_IN_MS, 150);
    assert_eq!(FADE_OUT_MS, 200);
}

// Toolbar auto-hide ----------------------------------------------------------------------------------------------------

// C#: ShellOverlayBehaviorTests.Toolbar_StartsVisibleAndClickable
#[test]
fn toolbar_starts_visible_and_clickable() {
    let rig = rig();
    assert_eq!(rig.overlay.toolbar_opacity(), 1.0);
    assert!(rig.overlay.toolbar_hit_test_visible());
    assert!(rig.overlay.with_toolbar(|toolbar| toolbar.base().is_visible));
}

// C#: ShellOverlayBehaviorTests.Toolbar_HidesExactlyAfterTheConfiguredDelay_ThenFadesOutOver200Ms
#[test]
fn toolbar_hides_exactly_after_the_configured_delay_then_fades_out_over_200_ms() {
    let rig = rig();
    rig.move_to(FAR_AWAY);

    rig.advance(1499);
    assert_eq!(rig.overlay.toolbar_opacity(), 1.0);
    assert!(rig.overlay.toolbar_hit_test_visible());

    rig.advance(1); // t = 1500: the timer elapses, the fade starts and the toolbar is click-through at once
    assert!(!rig.overlay.toolbar_hit_test_visible());
    assert!(rig.overlay.is_animating());

    rig.advance(1); // first frame of the tween: the opacity is still the start value
    assert_close(rig.overlay.toolbar_opacity(), 1.0, 0.001);
    rig.advance(100);
    assert_close(rig.overlay.toolbar_opacity(), 0.5, 0.02);
    rig.advance(100);
    assert_eq!(rig.overlay.toolbar_opacity(), 0.0);
    assert!(!rig.overlay.is_animating());
}

// C#: ShellOverlayBehaviorTests.Toolbar_FadeIn_Takes150Ms_WhenTheMouseReturnsToTheHotZone
#[test]
fn toolbar_fade_in_takes_150_ms_when_the_mouse_returns_to_the_hot_zone() {
    let rig = rig();
    rig.move_to(FAR_AWAY);
    rig.advance(1500 + 1 + 200);
    assert_eq!(rig.overlay.toolbar_opacity(), 0.0);

    rig.move_to(PointD { x: 20.0, y: 20.0 });
    assert!(rig.overlay.toolbar_hit_test_visible());
    rig.advance(1); // first frame
    rig.advance(75);
    assert_close(rig.overlay.toolbar_opacity(), 0.5, 0.02);
    rig.advance(75);
    assert_eq!(rig.overlay.toolbar_opacity(), 1.0);
}

// C#: ShellOverlayBehaviorTests.Toolbar_HotZone_IsTheToolbarBoxInflatedBy24OnEverySide
#[test]
fn toolbar_hot_zone_is_the_toolbar_box_inflated_by_24_on_every_side() {
    let rig = rig();
    let bx = rig.toolbar_bounds();
    let right = bx.x + bx.width + HOT_ZONE_MARGIN;
    let bottom = bx.y + bx.height + HOT_ZONE_MARGIN;

    rig.move_to(PointD { x: right, y: bottom }); // on the edge: inside
    rig.advance(5000);
    assert_eq!(rig.overlay.toolbar_opacity(), 1.0);

    rig.move_to(PointD {
        x: right + 0.5,
        y: 20.0,
    }); // outside
    rig.advance(1500 + 1 + 200);
    assert_eq!(rig.overlay.toolbar_opacity(), 0.0);

    rig.move_to(PointD {
        x: 20.0,
        y: bottom + 0.5,
    }); // outside vertically: still hidden
    rig.advance(300);
    assert_eq!(rig.overlay.toolbar_opacity(), 0.0);

    rig.move_to(PointD {
        x: bx.x - HOT_ZONE_MARGIN,
        y: bx.y - HOT_ZONE_MARGIN,
    }); // top-left corner: inside
    rig.advance(151);
    assert_eq!(rig.overlay.toolbar_opacity(), 1.0);
}

// C#: OverlayPartsTests.Toolbar_HotZoneIs24DipRegardlessOfTheConstant
#[test]
fn toolbar_hot_zone_is_24_dip_regardless_of_the_constant() {
    let rig = rig();
    let bx = rig.toolbar_bounds();
    rig.move_to(PointD {
        x: bx.x + bx.width + 24.0,
        y: 20.0,
    });
    rig.advance(5000);
    assert!(rig.overlay.toolbar_hit_test_visible());

    rig.move_to(PointD {
        x: bx.x + bx.width + 24.5,
        y: 20.0,
    });
    rig.advance(1500);
    assert!(!rig.overlay.toolbar_hit_test_visible());

    rig.move_to(PointD {
        x: bx.x - 24.0,
        y: bx.y - 24.0,
    });
    assert!(rig.overlay.toolbar_hit_test_visible());
    rig.move_to(PointD {
        x: bx.x - 24.5,
        y: bx.y - 24.0,
    });
    rig.advance(1500);
    assert!(!rig.overlay.toolbar_hit_test_visible());
}

// C#: ShellOverlayBehaviorTests.Toolbar_CountdownDoesNotRestartOnLaterMovesOutsideTheHotZone
#[test]
fn toolbar_countdown_does_not_restart_on_later_moves_outside_the_hot_zone() {
    let rig = rig();
    rig.move_to(FAR_AWAY);
    rig.advance(1000);
    rig.move_to(PointD { x: 700.0, y: 500.0 }); // still outside the hot zone
    rig.advance(499);
    assert!(rig.overlay.toolbar_hit_test_visible());
    rig.advance(1);
    assert!(!rig.overlay.toolbar_hit_test_visible());
}

fn delay_case(configured: i32, effective: i64) {
    let rig = Rig::new(|s| s.toolbar_auto_hide_delay_ms = configured, true);
    rig.move_to(FAR_AWAY);
    rig.advance(effective - 1);
    assert!(rig.overlay.toolbar_hit_test_visible());
    rig.advance(1);
    assert!(!rig.overlay.toolbar_hit_test_visible());
}

// C#: ShellOverlayBehaviorTests.Toolbar_DelayComesFromSettingsClampedToItsRange (500, 500)
#[test]
fn toolbar_delay_comes_from_settings_500_is_500() {
    delay_case(500, 500);
}

// C#: ShellOverlayBehaviorTests.Toolbar_DelayComesFromSettingsClampedToItsRange (0, 1): WPF Math.Max(1, delay)
#[test]
fn toolbar_delay_comes_from_settings_0_is_1() {
    delay_case(0, 1);
}

// C#: ShellOverlayBehaviorTests.Toolbar_DelayComesFromSettingsClampedToItsRange (99999, 10000)
#[test]
fn toolbar_delay_comes_from_settings_99999_is_clamped_to_10000() {
    delay_case(99999, 10000);
}

// C#: OverlayPartsTests.Toolbar_ZeroDelayStillWaitsForALaterFrame_NotTheFrameAtTheSameInstant
#[test]
fn toolbar_zero_delay_still_waits_for_a_later_frame_not_the_frame_at_the_same_instant() {
    let rig = Rig::new(|s| s.toolbar_auto_hide_delay_ms = 0, true);
    rig.move_to(FAR_AWAY);
    rig.clock.tick(rig.now.get()); // a frame at the very instant the timer was set: Math.Max(1, delay) means not hidden yet
    assert!(rig.overlay.toolbar_hit_test_visible());
    rig.advance(1);
    assert!(!rig.overlay.toolbar_hit_test_visible());
}

// C#: ShellOverlayBehaviorTests.Toolbar_NeverHidesWhenAutoHideIsOff
#[test]
fn toolbar_never_hides_when_auto_hide_is_off() {
    let rig = Rig::new(|s| s.toolbar_auto_hide = false, true);
    rig.move_to(FAR_AWAY);
    rig.advance(20_000);
    assert_eq!(rig.overlay.toolbar_opacity(), 1.0);
}

// C#: ShellOverlayBehaviorTests.Toolbar_StaysVisibleWithoutAFolder
#[test]
fn toolbar_stays_visible_without_a_folder() {
    let rig = Rig::new(|_| {}, false);
    rig.move_to(FAR_AWAY);
    rig.advance(20_000);
    assert_eq!(rig.overlay.toolbar_opacity(), 1.0);
}

// C#: ShellOverlayBehaviorTests.Toolbar_ClosingTheFolderBringsItBack
#[test]
fn toolbar_closing_the_folder_brings_it_back() {
    let rig = rig();
    rig.move_to(FAR_AWAY);
    rig.advance(1500 + 1 + 200);
    assert_eq!(rig.overlay.toolbar_opacity(), 0.0);

    rig.update(|model| model.has_images = false);
    rig.advance(1 + 150);
    assert_eq!(rig.overlay.toolbar_opacity(), 1.0);
}

// C#: ShellOverlayBehaviorTests.Toolbar_VisibleOpacityFollowsTheSettingAndNeverGoesBelowTheMinimum
#[test]
fn toolbar_visible_opacity_follows_the_setting_and_never_goes_below_the_minimum() {
    let rig = Rig::new(|s| s.toolbar_opacity_percent = 60, true);
    rig.advance(1 + 150);
    assert_close(rig.overlay.toolbar_opacity(), 0.6, 0.001);

    rig.set(|s| s.toolbar_opacity_percent = 5); // clamped to 20 %
    rig.overlay.settings_changed();
    rig.advance(1 + 150);
    assert_close(rig.overlay.toolbar_opacity(), 0.2, 0.001);
}

// C#: ShellOverlayBehaviorTests.Toolbar_SettingsChangeToAutoHideOffBringsItBackAtOnce
#[test]
fn toolbar_settings_change_to_auto_hide_off_brings_it_back_at_once() {
    let rig = rig();
    rig.move_to(FAR_AWAY);
    rig.advance(1500 + 1 + 200);
    rig.set(|s| s.toolbar_auto_hide = false);
    rig.overlay.settings_changed();
    rig.advance(1 + 150);
    assert_eq!(rig.overlay.toolbar_opacity(), 1.0);
    assert!(rig.overlay.toolbar_hit_test_visible());
}

// Info overlay ---------------------------------------------------------------------------------------------------------

// C#: ShellOverlayBehaviorTests.InfoOverlay_HidesAfterTheIdleDelayAndBecomesClickThrough
#[test]
fn info_overlay_hides_after_the_idle_delay_and_becomes_click_through() {
    let rig = rig();
    rig.advance(2999);
    assert!(rig.overlay.info_hit_test_visible());
    rig.advance(1);
    assert!(!rig.overlay.info_hit_test_visible());
    rig.advance(1 + 200);
    assert_eq!(rig.overlay.info_opacity(), 0.0);
}

// C#: ShellOverlayBehaviorTests.InfoOverlay_ActivityRestartsTheCountdown
#[test]
fn info_overlay_activity_restarts_the_countdown() {
    let rig = rig();
    rig.advance(2000);
    rig.move_to(FAR_AWAY); // activity
    rig.advance(2999);
    assert!(rig.overlay.info_hit_test_visible());
    rig.advance(1);
    assert!(!rig.overlay.info_hit_test_visible());
}

// C#: ShellOverlayBehaviorTests.InfoOverlay_NavigationAndKeysAreActivity_AndTheyBringItBack
// (the `KeyDown` line is W8-15i's; a second `note_activity` stands for the key here)
#[test]
fn info_overlay_navigation_and_keys_are_activity_and_they_bring_it_back() {
    let rig = rig();
    rig.advance(3000 + 1 + 200);
    assert_eq!(rig.overlay.info_opacity(), 0.0);

    rig.overlay.note_activity(); // image change
    rig.advance(1 + 150);
    assert_eq!(rig.overlay.info_opacity(), 1.0);

    rig.advance(3000 + 1 + 200);
    assert_eq!(rig.overlay.info_opacity(), 0.0);
    rig.overlay.note_activity(); // a key
    rig.advance(1 + 150);
    assert_eq!(rig.overlay.info_opacity(), 1.0);
}

// C#: ShellOverlayBehaviorTests.InfoOverlay_AttentionMessageKeepsItVisible_AndTheDelayStartsWhenTheMessageEnds
#[test]
fn info_overlay_attention_message_keeps_it_visible_and_the_delay_starts_when_the_message_ends() {
    let rig = rig();
    rig.update(|m| {
        m.status_needs_attention = true;
        m.status_text = "Dang tai...".to_owned();
    });
    rig.advance(10_000);
    assert!(rig.overlay.info_hit_test_visible());

    rig.update(|m| {
        m.status_needs_attention = false;
        m.status_text = String::new();
    });
    rig.advance(2999);
    assert!(rig.overlay.info_hit_test_visible()); // counted from the end of the message, not from the last activity
    rig.advance(1);
    assert!(!rig.overlay.info_hit_test_visible());
}

// C#: ShellOverlayBehaviorTests.InfoOverlay_CompareAndInactiveWindowAndNoFolderForceItVisible
#[test]
fn info_overlay_compare_and_inactive_window_and_no_folder_force_it_visible() {
    let rig = rig();
    rig.update(|m| m.compare_visible = true);
    rig.advance(10_000);
    assert!(rig.overlay.info_hit_test_visible());
    rig.update(|m| m.compare_visible = false);
    rig.advance(3000 + 1 + 200);
    assert_eq!(rig.overlay.info_opacity(), 0.0);

    rig.window_active.set(false); // a dialog or another application holds the focus
    rig.overlay.window_deactivated();
    rig.advance(1 + 150);
    assert_eq!(rig.overlay.info_opacity(), 1.0);
    rig.advance(20_000);
    assert_eq!(rig.overlay.info_opacity(), 1.0);

    rig.window_active.set(true);
    rig.overlay.window_activated();
    rig.advance(3000 + 1 + 200);
    assert_eq!(rig.overlay.info_opacity(), 0.0);

    rig.update(|m| m.has_images = false);
    rig.advance(1 + 150);
    assert_eq!(rig.overlay.info_opacity(), 1.0);
}

// C#: ShellOverlayBehaviorTests.InfoOverlay_AutoHideOffNeverHides_AndTheToolbarDelayIsIndependent
#[test]
fn info_overlay_auto_hide_off_never_hides_and_the_toolbar_delay_is_independent() {
    let rig = Rig::new(|s| s.info_overlay_auto_hide = false, true);
    rig.move_to(FAR_AWAY);
    rig.advance(20_000);
    assert_eq!(rig.overlay.info_opacity(), 1.0);
    assert!(!rig.overlay.toolbar_hit_test_visible()); // the toolbar still hides after its own delay
}

// C#: OverlayPartsTests.InfoOverlay_StopsItsIdleTimer_WhenSomethingStartsKeepingItVisible
#[test]
fn info_overlay_stops_its_idle_timer_when_something_starts_keeping_it_visible() {
    let rig = rig();
    let running = rig.clock.subscriber_count();
    assert!(running >= 1, "the idle countdown is running");
    assert!(rig.overlay.is_frame_subscribed());
    rig.update(|m| m.compare_visible = true);
    assert!(
        rig.clock.subscriber_count() < running,
        "the frame loop must rest when nothing is left to hide"
    );
    assert!(!rig.overlay.is_frame_subscribed());
}

// Hover ----------------------------------------------------------------------------------------------------------------

fn button_center(rig: &Rig, button: OverlayButton) -> PointD {
    center_of(rig.bounds_of(button.element_id()))
}

fn hot(rig: &Rig) -> [bool; 4] {
    rig.overlay.with_toolbar(|toolbar| {
        let [a, b, c, d] = toolbar.buttons();
        [a.is_hot, b.is_hot, c.is_hot, d.is_hot]
    })
}

// C#: ShellOverlayBehaviorTests.Buttons_HoverFollowsThePointer_AndLeavingClearsIt
#[test]
fn buttons_hover_follows_the_pointer_and_leaving_clears_it() {
    let rig = rig();
    rig.move_to(button_center(&rig, OverlayButton::Fit));
    assert_eq!(hot(&rig), [false, false, true, false]);

    rig.move_to(button_center(&rig, OverlayButton::Settings));
    assert_eq!(hot(&rig), [false, true, false, false]);

    rig.overlay.pointer_left();
    assert_eq!(hot(&rig), [false, false, false, false]);
}

#[test]
fn hover_requests_a_render_only_when_the_hot_button_changes() {
    // new: PointerMoved requests a render only when `_hot` changes (the fade requests are separate and gated).
    let rig = rig();
    rig.move_to(PointD { x: 20.0, y: 20.0 });
    let before = rig.render_requests.get();
    rig.move_to(button_center(&rig, OverlayButton::Fit));
    assert_eq!(rig.render_requests.get(), before + 1);
    rig.move_to(button_center(&rig, OverlayButton::Fit));
    assert_eq!(rig.render_requests.get(), before + 1);
    rig.overlay.pointer_left();
    assert_eq!(rig.render_requests.get(), before + 2);
    rig.overlay.pointer_left(); // nothing hot any more
    assert_eq!(rig.render_requests.get(), before + 2);
}

#[test]
fn pointer_left_keeps_the_hot_zone_flag() {
    // new: C# `PointerLeft` does not touch `_toolbarMouseInsideHotZone` (no MouseMove in WPF): the toolbar keeps its state.
    let rig = rig();
    rig.move_to(button_center(&rig, OverlayButton::Fit));
    rig.overlay.pointer_left();
    rig.advance(5000);
    assert_eq!(rig.overlay.toolbar_opacity(), 1.0);
}

#[test]
fn the_view_list_button_gets_hover_too() {
    // new: `button_mut` reaches the skipped-row button inside the info layer.
    let rig = rig();
    rig.update(|m| {
        m.status_panel_visible = true;
        m.has_skipped_entries = true;
        m.skipped_warning_text = "3".to_owned();
    });
    rig.overlay.arrange(
        SizeD {
            width: 1000.0,
            height: 700.0,
        },
        1.0,
    );
    let point = center_of(rig.bounds_of(OverlayButton::ViewList.element_id()));
    assert_eq!(rig.overlay.hit_button(point), Some(OverlayButton::ViewList));
    rig.move_to(point);
    assert!(rig.overlay.with_status(|status| status.view_list().is_hot));
    rig.overlay.pointer_left();
    assert!(!rig.overlay.with_status(|status| status.view_list().is_hot));
}

// Model ----------------------------------------------------------------------------------------------------------------

// C#: ShellOverlayBehaviorTests.Model_DrivesVisibilityTextAndFontOfEveryPanel
#[test]
fn model_drives_visibility_text_and_font_of_every_panel() {
    let rig = rig();
    rig.update(|m| {
        m.status_panel_visible = true;
        m.file_info_visible = true;
        m.status_text = "IMG_1.JPG".to_owned();
        m.font_size = 14.0;
        m.exif_font_size = 13.0;
        m.exif_visible = true;
        m.exif_text = "ISO 100".to_owned();
        m.folder_info_visible = true;
        m.folder_info_text = "Trip".to_owned();
        m.zoom_indicator_visible = true;
        m.zoom_indicator_text = "150%".to_owned();
        m.capture_pair_visible = true;
        m.capture_pair_text = "JPEG+RAW".to_owned();
    });

    rig.overlay.with_status(|status| {
        assert!(status.base().is_visible);
        assert_eq!(status.status().text(), "IMG_1.JPG");
        assert_eq!(status.status().style().font_size_dip, 14.0);
        assert_eq!(status.exif().style().font_size_dip, 13.0);
        assert_eq!(status.exif().text(), "ISO 100");
    });
    assert_eq!(
        rig.overlay
            .with_folder_info(|panel| panel.text().map(|t| t.text().to_owned())),
        Some("Trip".to_owned())
    );
    assert_eq!(
        rig.overlay.with_zoom_indicator(|panel| panel.text().text().to_owned()),
        "150%"
    );
    assert_eq!(
        rig.overlay
            .with_capture_pair(|panel| panel.text().map(|t| t.text().to_owned())),
        Some("JPEG+RAW".to_owned())
    );
    assert!(rig.overlay.with_capture_pair(|panel| panel.base().is_visible));

    rig.update(|m| {
        m.status_panel_visible = false;
        m.folder_info_visible = false;
        m.zoom_indicator_visible = false;
        m.capture_pair_visible = false;
    });
    assert!(!rig.overlay.with_status(|p| p.base().is_visible));
    assert!(!rig.overlay.with_folder_info(|p| p.base().is_visible));
    assert!(!rig.overlay.with_zoom_indicator(|p| p.base().is_visible));
    assert!(!rig.overlay.with_capture_pair(|p| p.base().is_visible));
}

#[test]
fn the_info_layer_holds_the_four_panels_in_order_under_the_expected_id() {
    // new: constructor wiring (C# `InfoLayer.Add(Status).Add(FolderInfo).Add(ZoomIndicator).Add(CapturePair)`, host order layer then toolbar).
    let rig = rig();
    let ids: Vec<String> = rig.overlay.with_info_layer(|layer| {
        assert_eq!(layer.base().id(), "InfoOverlayHost");
        layer.children().iter().map(|c| c.base().id().to_owned()).collect()
    });
    assert_eq!(
        ids,
        [
            "StatusPanel",
            "FolderInfoPanel",
            "ZoomIndicatorPanel",
            "CapturePairBadge"
        ]
    );
    assert!(rig.overlay.with_toolbar(|t| t.base().is_visible));
}

#[test]
fn update_with_equal_model_still_requests_a_render() {
    // new: C# `Update` always ends with RequestRender.
    let rig = rig();
    let before = rig.render_requests.get();
    rig.overlay.update(rig.overlay.model());
    assert_eq!(rig.render_requests.get(), before + 1);
}

#[test]
fn settings_changed_and_apply_localization_request_a_render() {
    // new: C# `SettingsChanged` and `ApplyLocalization` end with RequestRender.
    let rig = rig();
    let before = rig.render_requests.get();
    rig.overlay.settings_changed();
    assert!(rig.render_requests.get() > before);
    let before = rig.render_requests.get();
    rig.overlay
        .apply_localization(&pr_l10n::localizer::built_in_english_localizer());
    assert_eq!(rig.render_requests.get(), before + 1);
}

#[test]
fn window_deactivated_forces_the_info_overlay_visible_through_the_settings_flag() {
    // new: with auto-hide off nothing hides; with an inactive window the overlay stays up even after a long idle.
    let rig = rig();
    rig.window_active.set(false);
    rig.overlay.window_deactivated();
    rig.advance(10_000);
    assert_eq!(rig.overlay.info_opacity(), 1.0);
}

// Layout ---------------------------------------------------------------------------------------------------------------

#[test]
fn effective_dpi_table() {
    // new: C# `_lastDpi = dpiScale <= 0 ? 1 : dpiScale` (NaN is kept)
    assert_eq!(effective_dpi(1.0), 1.0);
    assert_eq!(effective_dpi(1.5), 1.5);
    assert_eq!(effective_dpi(0.0), 1.0);
    assert_eq!(effective_dpi(-0.0), 1.0);
    assert_eq!(effective_dpi(-2.0), 1.0);
    assert_eq!(effective_dpi(0.25), 0.25);
    assert!(effective_dpi(f64::NAN).is_nan());
}

#[test]
fn arrange_stores_the_client_and_the_effective_dpi() {
    // new: `last_client` / `last_dpi` after `arrange`
    let rig = rig();
    assert_eq!(
        rig.overlay.last_client(),
        SizeD {
            width: 1000.0,
            height: 700.0
        }
    );
    assert_eq!(rig.overlay.last_dpi(), 1.0);
    rig.overlay.arrange(
        SizeD {
            width: 640.0,
            height: 480.0,
        },
        2.0,
    );
    assert_eq!(
        rig.overlay.last_client(),
        SizeD {
            width: 640.0,
            height: 480.0
        }
    );
    assert_eq!(rig.overlay.last_dpi(), 2.0);
    rig.overlay.arrange(
        SizeD {
            width: 640.0,
            height: 480.0,
        },
        0.0,
    );
    assert_eq!(rig.overlay.last_dpi(), 1.0);
}

#[test]
fn layout_is_independent_of_dpi() {
    // C#: ShellOverlayGoldenTests.ToolbarKeepsItsBox_WhenOnlyTheDpiChanges (with the fake renderer)
    let rig = rig();
    let client = SizeD {
        width: 1000.0,
        height: 700.0,
    };
    rig.overlay.arrange(client, 1.0);
    let at_one = rig.toolbar_bounds();
    let fit_at_one = rig.bounds_of("FitButton");
    rig.overlay.arrange(client, 1.5);
    assert_eq!(rig.toolbar_bounds(), at_one);
    assert_eq!(rig.bounds_of("FitButton"), fit_at_one);
    assert_eq!(rig.overlay.last_dpi(), 1.5);
}

#[test]
fn element_bounds_finds_nested_elements_and_nothing_else() {
    // new: `element_bounds` is `Host.Find(id).Bounds`
    let rig = rig();
    assert!(rig.overlay.element_bounds("FitButton").is_some());
    assert!(rig.overlay.element_bounds("StatusPanel").is_some());
    assert!(rig.overlay.element_bounds("NoSuchElement").is_none());
    let bx: RectD = rig.toolbar_bounds();
    assert_eq!((bx.x, bx.y), (8.0, 8.0));
}

// Hit testing ----------------------------------------------------------------------------------------------------------

#[test]
fn hit_button_maps_only_the_five_buttons() {
    // new: T9 identity of buttons; a text or panel hit is not a button
    let rig = rig();
    rig.update(|m| {
        m.zoom_indicator_visible = true;
        m.zoom_indicator_text = "100%".to_owned();
    });
    rig.overlay.arrange(
        SizeD {
            width: 1000.0,
            height: 700.0,
        },
        1.0,
    );
    for (button, id) in [
        (OverlayButton::OpenFolder, "OpenFolderButton"),
        (OverlayButton::Settings, "SettingsButton"),
        (OverlayButton::Fit, "FitButton"),
        (OverlayButton::Tools, "ToolsButton"),
    ] {
        assert_eq!(button.element_id(), id);
        assert_eq!(OverlayButton::from_element_id(id), Some(button));
        assert_eq!(rig.overlay.hit_button(button_center(&rig, button)), Some(button));
        assert!(rig.overlay.hit_test(button_center(&rig, button)));
    }
    assert_eq!(OverlayButton::ViewList.element_id(), "SkippedViewListButton");
    assert_eq!(
        OverlayButton::from_element_id("SkippedViewListButton"),
        Some(OverlayButton::ViewList)
    );
    assert_eq!(OverlayButton::from_element_id("ToolbarPanel"), None);
    assert_eq!(OverlayButton::from_element_id(""), None);
    assert_eq!(OverlayButton::from_element_id("fitbutton"), None);

    let zoom = center_of(rig.bounds_of("ZoomIndicatorPanel"));
    assert!(rig.overlay.hit_test(zoom));
    assert_eq!(rig.overlay.hit_button(zoom), None);
    let empty = PointD { x: 500.0, y: 350.0 };
    assert!(!rig.overlay.hit_test(empty));
    assert_eq!(rig.overlay.hit_button(empty), None);
}

// C#: ShellOverlayBehaviorTests.ClickThrough_HiddenToolbarAndHiddenInfoLetTheImageHaveTheClick (the hit-test half)
#[test]
fn click_through_hidden_toolbar_and_hidden_info_let_the_image_have_the_click() {
    let rig = rig();
    rig.update(|m| {
        m.status_panel_visible = true;
        m.file_info_visible = true;
        m.status_text = "a.jpg".to_owned();
    });
    rig.overlay.arrange(
        SizeD {
            width: 1000.0,
            height: 700.0,
        },
        1.0,
    );
    let toolbar_point = button_center(&rig, OverlayButton::Fit);
    let status_point = center_of(rig.bounds_of("StatusPanel"));
    assert!(rig.overlay.hit_test(toolbar_point));
    assert!(rig.overlay.hit_test(status_point));

    rig.move_to(FAR_AWAY);
    rig.advance(4000);
    assert!(!rig.overlay.hit_test(toolbar_point));
    assert!(!rig.overlay.hit_test(status_point));
    assert_eq!(rig.overlay.hit_button(toolbar_point), None);
    assert!(rig.commands.borrow().is_empty());
}

#[test]
fn a_fading_out_toolbar_is_click_through_from_the_start_of_the_fade() {
    // new: T2 `Fade` sets `is_hit_test_visible = visible` at once
    let rig = rig();
    let fit = button_center(&rig, OverlayButton::Fit);
    rig.move_to(FAR_AWAY);
    rig.advance(1500);
    assert!(rig.overlay.toolbar_opacity() > 0.0);
    assert!(!rig.overlay.hit_test(fit));
}

#[test]
fn hit_test_prefers_the_toolbar_over_the_info_layer_where_they_overlap() {
    // new: the toolbar root is on top. A small client makes the status panel (bottom left) and the zoom indicator reach the toolbar.
    let rig = rig();
    rig.update(|m| {
        m.status_panel_visible = true;
        m.file_info_visible = true;
        m.status_text = "A long status line that is wider than the window".to_owned();
        m.zoom_indicator_visible = true;
        m.zoom_indicator_text = "100%".to_owned();
        m.folder_info_visible = true;
        m.folder_info_text = "Folder".to_owned();
    });
    let client = SizeD {
        width: 260.0,
        height: 60.0,
    };
    rig.overlay.arrange(client, 1.0);
    let infos: Vec<RectD> = ["StatusPanel", "FolderInfoPanel", "ZoomIndicatorPanel"]
        .iter()
        .map(|id| rig.bounds_of(id))
        .collect();
    let toolbar_buttons: Vec<(OverlayButton, RectD)> = [
        OverlayButton::OpenFolder,
        OverlayButton::Settings,
        OverlayButton::Fit,
        OverlayButton::Tools,
    ]
    .iter()
    .map(|b| (*b, rig.bounds_of(b.element_id())))
    .collect();
    let mut overlap = None;
    for (button, rect) in &toolbar_buttons {
        let center = center_of(*rect);
        if infos
            .iter()
            .any(|i| center.x >= i.x && center.x <= i.x + i.width && center.y >= i.y && center.y <= i.y + i.height)
        {
            overlap = Some((*button, center));
            break;
        }
    }
    let (button, point) = overlap.expect("the small client makes an info panel cover a toolbar button");
    assert_eq!(rig.overlay.hit_button(point), Some(button));
}

// Dispose --------------------------------------------------------------------------------------------------------------

// C#: ShellOverlayBehaviorTests.Dispose_StopsTimersAndAnimationsAndReleasesTextLayouts
#[test]
fn dispose_stops_timers_and_animations_and_releases_text_layouts() {
    let rig = rig();
    assert!(rig.text.live() > 0);
    rig.move_to(FAR_AWAY);
    assert!(rig.clock.subscriber_count() > 0);
    assert!(rig.overlay.is_frame_subscribed());

    rig.overlay.dispose();
    assert_eq!(rig.clock.subscriber_count(), 0);
    assert!(!rig.overlay.is_frame_subscribed());
    assert_eq!(rig.text.live(), 0);
    assert!(rig.tooltips.disposed.get());
    rig.advance(5000); // nothing listens to the clock any more
    let unsubscribes = rig.clock.unsubscribe_calls.get();
    rig.overlay.dispose(); // idempotent
    assert_eq!(rig.clock.unsubscribe_calls.get(), unsubscribes);
}

#[test]
fn dispose_is_idempotent_and_releases_the_subscription() {
    // new: a second dispose and the final drop do nothing more
    let rig = rig();
    rig.move_to(FAR_AWAY);
    let subscribes = rig.clock.subscribe_calls.get();
    rig.overlay.dispose();
    rig.overlay.dispose();
    assert_eq!(rig.clock.subscribe_calls.get(), subscribes);
    assert_eq!(rig.clock.subscriber_count(), 0);
    let clock = Rc::clone(&rig.clock);
    drop(rig);
    assert_eq!(clock.subscriber_count(), 0);
}

#[test]
fn dropping_the_last_reference_disposes_the_overlay() {
    // new: `Drop` calls `dispose` (layouts released, subscription gone, tooltips disposed)
    let rig = rig();
    rig.move_to(FAR_AWAY);
    let (clock, text, tooltips) = (Rc::clone(&rig.clock), Rc::clone(&rig.text), Rc::clone(&rig.tooltips));
    drop(rig);
    assert_eq!(clock.subscriber_count(), 0);
    assert_eq!(text.live(), 0);
    assert!(tooltips.disposed.get());
}

// C#: ShellOverlayBehaviorTests.AfterDispose_InputAndUpdatesAreIgnored (the non-input half)
#[test]
fn after_dispose_updates_and_settings_changes_are_ignored() {
    let rig = rig();
    let point = button_center(&rig, OverlayButton::Fit);
    rig.overlay.dispose();
    let renders = rig.render_requests.get();
    assert!(!rig.overlay.hit_test(point));
    assert_eq!(rig.overlay.hit_button(point), None);
    rig.overlay.update(OverlayModel::default());
    assert!(
        rig.overlay.model().has_images,
        "an update after dispose does not change the model"
    );
    rig.overlay.settings_changed();
    rig.overlay.pointer_moved(point);
    rig.overlay.note_activity();
    rig.overlay.window_deactivated();
    rig.overlay.window_activated();
    rig.overlay
        .apply_localization(&pr_l10n::localizer::built_in_english_localizer());
    assert_eq!(rig.render_requests.get(), renders);
    assert!(rig.commands.borrow().is_empty());
    assert!(!rig.overlay.is_frame_subscribed());
}

// Frame wiring ---------------------------------------------------------------------------------------------------------

#[test]
fn frame_subscription_exists_only_while_something_runs() {
    // new: D-W8B4O-8, one subscription while the animator or a timer runs
    let rig = Rig::new(|_| {}, false);
    assert!(!rig.overlay.is_frame_subscribed());
    assert_eq!(rig.clock.subscriber_count(), 0);

    rig.update(|m| m.has_images = true); // the info overlay may hide now: its idle timer runs
    assert!(rig.overlay.is_frame_subscribed());
    assert_eq!(rig.clock.subscriber_count(), 1);

    rig.advance(3000 + 1 + 200); // timer elapsed, fade finished
    assert!(!rig.overlay.is_frame_subscribed());
    assert_eq!(rig.clock.subscriber_count(), 0);
    assert_eq!(rig.clock.subscribe_calls.get(), 1);
    assert_eq!(rig.clock.unsubscribe_calls.get(), 1);

    rig.move_to(FAR_AWAY); // both timers again, one subscription
    assert_eq!(rig.clock.subscriber_count(), 1);
    assert_eq!(rig.clock.subscribe_calls.get(), 2);
}

#[test]
fn starting_a_timer_requests_a_frame() {
    // new: `FrameDelayTimer::start` is always followed by `request_frame`; a fade tween too
    let rig = Rig::new(|_| {}, false);
    let before = rig.clock.frame_requests.get();
    rig.update(|m| m.has_images = true);
    assert!(rig.clock.frame_requests.get() > before);
    let before = rig.clock.frame_requests.get();
    rig.advance(3000); // the timer elapses and the info fade starts
    assert!(rig.clock.frame_requests.get() > before);
}

#[test]
fn frame_order_is_animator_then_toolbar_timer_then_info_timer() {
    // new: D-W8B4O-8. Both timers elapse in the same frame (t = 1500); a fade started by a timer gets its start timestamp one frame LATER, because the
    // animator already ran in that frame. If the timers ran first, the tween would start at t = 1500 and be finished at t = 1700.
    let rig = Rig::new(|s| s.info_overlay_auto_hide_delay_ms = 1500, true);
    rig.move_to(FAR_AWAY); // starts both countdowns at the same instant, both 1500 ms
    rig.advance(1500);
    assert!(!rig.overlay.toolbar_hit_test_visible());
    assert!(!rig.overlay.info_hit_test_visible());
    assert_eq!(rig.overlay.toolbar_opacity(), 1.0);
    assert_eq!(rig.overlay.info_opacity(), 1.0);

    rig.advance(200); // t = 1700: with the right order the tween started at 1501, so 199 of 200 ms have run
    assert_close(rig.overlay.toolbar_opacity(), 0.005, 0.001);
    assert_close(rig.overlay.info_opacity(), 0.005, 0.001);
    rig.advance(1);
    assert_eq!(rig.overlay.toolbar_opacity(), 0.0);
    assert_eq!(rig.overlay.info_opacity(), 0.0);
    assert!(!rig.overlay.is_frame_subscribed());
}

#[test]
fn repeated_evaluation_with_the_same_target_requests_no_render() {
    // new: T2/T6 `FadeTargetGate`: re-evaluating the visibility with an unchanged target starts nothing and requests nothing
    let rig = rig();
    rig.move_to(PointD { x: 20.0, y: 20.0 });
    rig.advance(10);
    let renders = rig.render_requests.get();
    let frames = rig.clock.frame_requests.get();
    rig.move_to(PointD { x: 21.0, y: 21.0 });
    rig.move_to(PointD { x: 22.0, y: 22.0 });
    assert_eq!(rig.render_requests.get(), renders);
    assert_eq!(rig.clock.frame_requests.get(), frames + 2); // only the info activity timers (two moves)
}

#[test]
fn the_fade_target_is_a_new_value_when_the_setting_changes() {
    // new: the gate key is the (widened f32) target: a different opacity setting is a different target and fades again
    let rig = Rig::new(|s| s.toolbar_opacity_percent = 60, true);
    rig.advance(1 + 150);
    assert_close(rig.overlay.toolbar_opacity(), 0.6, 0.001);
    let renders = rig.render_requests.get();
    rig.overlay.settings_changed(); // same percent: nothing new for the toolbar
    let same = rig.render_requests.get() - renders;
    rig.set(|s| s.toolbar_opacity_percent = 80);
    let renders = rig.render_requests.get();
    rig.overlay.settings_changed();
    let different = rig.render_requests.get() - renders;
    assert!(different > same, "a new target starts a fade and requests renders");
    rig.advance(1 + 150);
    assert_close(rig.overlay.toolbar_opacity(), 0.8, 0.001);
}

#[test]
fn no_borrow_error_when_request_render_reenters() {
    // new: T1 / D-W8B4O-7. `request_render` calls back into the overlay while an operation is in progress.
    let rig = rig();
    let slot: Rc<std::cell::RefCell<Weak<ShellOverlay>>> = Rc::new(std::cell::RefCell::new(Weak::new()));
    *slot.borrow_mut() = Rc::downgrade(&rig.overlay);
    let reentries = Rc::new(Cell::new(0usize));
    {
        let slot = Rc::clone(&slot);
        let reentries = Rc::clone(&reentries);
        *rig.on_render.borrow_mut() = Some(Box::new(move || {
            if let Some(overlay) = slot.borrow().upgrade() {
                let _ = overlay.hit_test(PointD { x: 20.0, y: 20.0 });
                let _ = overlay.model();
                let _ = overlay.toolbar_opacity();
                overlay.with_toolbar(|t| assert!(t.base().is_visible));
                let _ = overlay.last_client();
                reentries.set(reentries.get() + 1);
            }
        }));
    }
    rig.update(|m| m.status_text = "x".to_owned());
    rig.move_to(button_center(&rig, OverlayButton::Fit));
    rig.move_to(FAR_AWAY);
    rig.overlay.settings_changed();
    rig.advance(1500 + 1 + 200);
    rig.move_to(PointD { x: 20.0, y: 20.0 });
    rig.advance(1 + 150);
    rig.overlay.window_deactivated();
    rig.overlay.pointer_left();
    assert!(reentries.get() > 5);
    assert_eq!(rig.overlay.toolbar_opacity(), 1.0);
}
