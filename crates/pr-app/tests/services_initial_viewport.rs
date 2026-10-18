//! Tests of `pr_app::services::initial_viewport` (contract `contracts/W8-04c.md`, rules R-IV-1..9, traps T-3, T-4, T-5).
//!
//! Port of `InitialViewportPredictorTests.cs`. The Win32 facts come from a fake `MonitorProbe` with fixed values. The native
//! `PredictDecodeBox_DefaultSize_IsQuantisedBox` test is not here: it needs the real probe (`pr-platform-win`, W8-17).

use std::cell::RefCell;

use pr_app::services::initial_viewport::{MonitorId, MonitorProbe, PixelRect, predict_client_size, predict_decode_box};
use pr_app::windowing::contracts::{SHOW_MAXIMIZED, SHOW_NORMAL, WindowPlacementData};
use pr_imaging::adaptive_preview::calculate_target_decode_box;
use pr_imaging::decode_types::DecodeBox;

/// 96 dpi (100 %): 8 px sizing border, 31 px top inset = 8 border + 23 caption.
const INSETS_96: PixelRect = PixelRect {
    left: -8,
    top: -31,
    right: 8,
    bottom: 8,
};
/// 144 dpi (150 %): 11 px border, 45 px top inset = 11 border + 34 caption.
const INSETS_144: PixelRect = PixelRect {
    left: -11,
    top: -45,
    right: 11,
    bottom: 11,
};

/// Fixed Win32 facts. `calls` records which probe methods ran, in order, so a test can prove which monitor was chosen.
struct FakeProbe {
    nearest: Option<MonitorId>,
    primary: Option<MonitorId>,
    work_area: Option<PixelRect>,
    dpi: u32,
    insets: Option<PixelRect>,
    areas: Vec<PixelRect>,
    calls: RefCell<Vec<&'static str>>,
}

impl FakeProbe {
    /// One monitor (`MonitorId(1)`) at 96 dpi, work area 1920 x 1032, the 96 dpi insets, and that monitor as nearest and primary.
    fn single_monitor() -> Self {
        let work = PixelRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1032,
        };
        Self {
            nearest: Some(MonitorId(1)),
            primary: Some(MonitorId(1)),
            work_area: Some(work),
            dpi: 96,
            insets: Some(INSETS_96),
            areas: vec![work],
            calls: RefCell::new(Vec::new()),
        }
    }

    fn called(&self, name: &'static str) -> bool {
        self.calls.borrow().contains(&name)
    }
}

impl MonitorProbe for FakeProbe {
    fn monitor_nearest_rect(&self, _rect: PixelRect) -> Option<MonitorId> {
        self.calls.borrow_mut().push("nearest");
        self.nearest
    }

    fn monitor_primary(&self) -> Option<MonitorId> {
        self.calls.borrow_mut().push("primary");
        self.primary
    }

    fn monitor_work_area(&self, _monitor: MonitorId) -> Option<PixelRect> {
        self.calls.borrow_mut().push("work_area");
        self.work_area
    }

    fn dpi_for(&self, _monitor: MonitorId) -> u32 {
        self.calls.borrow_mut().push("dpi");
        self.dpi
    }

    fn frame_insets(&self, _dpi: u32) -> Option<PixelRect> {
        self.calls.borrow_mut().push("insets");
        self.insets
    }

    fn work_areas(&self) -> Vec<PixelRect> {
        self.calls.borrow_mut().push("work_areas");
        self.areas.clone()
    }
}

/// A saved placement with the given show command and normal bounds (the other fields are not read by the predictor).
fn placement(show_command: i32, left: i32, top: i32, right: i32, bottom: i32) -> WindowPlacementData {
    WindowPlacementData {
        length: 0,
        flags: 0,
        show_command,
        min_x: 0,
        min_y: 0,
        max_x: 0,
        max_y: 0,
        normal_left: left,
        normal_top: top,
        normal_right: right,
        normal_bottom: bottom,
    }
}

/// The box the predictor must return for a client size, through the same policy function it calls (R-IV-5).
fn expected_box(client_width: i32, client_height: i32, scale: f64, quality: f64) -> Option<DecodeBox> {
    calculate_target_decode_box(
        f64::from(client_width) / scale,
        f64::from(client_height) / scale,
        scale,
        quality,
    )
    .ok()
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> PixelRect {
    PixelRect {
        left,
        top,
        right,
        bottom,
    }
}

// C#: InitialViewportPredictorTests.PredictClientSize_Maximized96Dpi_KeepsWorkAreaWidthAndLosesOnlyTheCaption
#[test]
fn predict_client_size_maximized_96dpi_keeps_work_area_width_and_loses_only_the_caption() {
    let size = predict_client_size(true, rect(100, 100, 1300, 900), rect(0, 0, 1920, 1032), INSETS_96);
    assert_eq!(size, Some((1920, 1032 - 23)));
}

// C#: InitialViewportPredictorTests.PredictClientSize_Maximized144Dpi_KeepsWorkAreaWidthAndLosesOnlyTheCaption
#[test]
fn predict_client_size_maximized_144dpi_keeps_work_area_width_and_loses_only_the_caption() {
    let size = predict_client_size(true, rect(0, 0, 0, 0), rect(0, 0, 2880, 1548), INSETS_144);
    assert_eq!(size, Some((2880, 1548 - 34)));
}

// C#: InitialViewportPredictorTests.PredictClientSize_Maximized_IgnoresNormalBounds
#[test]
fn predict_client_size_maximized_ignores_normal_bounds() {
    let work = rect(0, 0, 1920, 1032);
    let small = predict_client_size(true, rect(10, 10, 200, 200), work, INSETS_96);
    let large = predict_client_size(true, rect(0, 0, 5000, 5000), work, INSETS_96);
    assert_eq!(small, large);
}

// C#: InitialViewportPredictorTests.PredictClientSize_Normal96Dpi_IsBoundsMinusFrame
#[test]
fn predict_client_size_normal_96dpi_is_bounds_minus_frame() {
    let size = predict_client_size(false, rect(100, 100, 1300, 900), rect(0, 0, 1920, 1032), INSETS_96);
    assert_eq!(size, Some((1200 - 16, 800 - 39)));
}

// C#: InitialViewportPredictorTests.PredictClientSize_Normal144Dpi_IsBoundsMinusFrame
#[test]
fn predict_client_size_normal_144dpi_is_bounds_minus_frame() {
    let size = predict_client_size(false, rect(-300, 50, 1500, 1250), rect(0, 0, 2880, 1548), INSETS_144);
    assert_eq!(size, Some((1800 - 22, 1200 - 56)));
}

// C#: InitialViewportPredictorTests.PredictClientSize_EmptyClient_ReturnsNull (Theory, 4 cases)
#[test]
fn predict_client_size_empty_client_returns_none() {
    let cases = [
        (false, rect(0, 0, 16, 39)),  // normal: bounds exactly the frame
        (false, rect(0, 0, 10, 10)),  // normal: smaller than the frame
        (true, rect(0, 0, 1920, 23)), // maximized: work area exactly the caption
        (true, rect(0, 0, 0, 0)),     // maximized: empty work area
    ];
    for (maximized, area) in cases {
        let size = predict_client_size(maximized, area, area, INSETS_96);
        assert_eq!(size, None, "maximized {maximized}, area {area:?}");
    }
}

// Contract test (R-IV-5): with no saved placement the primary monitor is used and the default size is placed at (0, 0).
// C#: InitialViewportPredictorTests (predictor, placement null path); the default 1200 x 800 at 96 dpi is 1184 x 761 of client.
#[test]
fn predict_decode_box_none_placement_uses_primary_and_default_size() {
    let probe = FakeProbe::single_monitor();

    let result = predict_decode_box(None, 1200.0, 800.0, 1.0, &probe);

    assert!(probe.called("primary"), "the primary monitor is used");
    assert!(!probe.called("nearest"), "no nearest lookup without a placement");
    assert_eq!(result, expected_box(1200 - 16, 800 - 39, 1.0, 1.0));
}

// Contract test (R-IV-5): an on-screen normal placement uses the nearest monitor and the saved bounds.
#[test]
fn predict_decode_box_on_screen_normal_uses_nearest_monitor_and_saved_bounds() {
    let probe = FakeProbe::single_monitor();
    let saved = placement(SHOW_NORMAL, 100, 100, 1300, 900);

    let result = predict_decode_box(Some(&saved), 1200.0, 800.0, 1.0, &probe);

    assert!(probe.called("nearest"), "the nearest monitor of the saved bounds");
    assert!(!probe.called("primary"));
    assert_eq!(result, expected_box(1200 - 16, 800 - 39, 1.0, 1.0));
}

// C# rule: an off-screen normal rect is ignored; the default size is used on the primary monitor.
#[test]
fn predict_decode_box_off_screen_normal_falls_back_to_default_size() {
    let probe = FakeProbe::single_monitor();
    let saved = placement(SHOW_NORMAL, 5000, 5000, 6000, 6000);

    let result = predict_decode_box(Some(&saved), 1200.0, 800.0, 1.0, &probe);

    assert!(
        probe.called("primary"),
        "an off-screen placement falls back to the primary monitor"
    );
    assert_eq!(result, expected_box(1200 - 16, 800 - 39, 1.0, 1.0));
}

// Contract test (R-IV-2 through the full path): a maximized placement takes the work area width and loses the caption.
#[test]
fn predict_decode_box_maximized_uses_work_area() {
    let probe = FakeProbe::single_monitor();
    let saved = placement(SHOW_MAXIMIZED, 100, 100, 1300, 900);

    let result = predict_decode_box(Some(&saved), 1200.0, 800.0, 1.0, &probe);

    assert_eq!(result, expected_box(1920, 1032 - 23, 1.0, 1.0));
}

// Contract test (R-IV-9): a `None` from the monitor, the work area, or the insets gives `None`.
#[test]
fn predict_decode_box_no_monitor_or_work_area_is_none() {
    let no_monitor = FakeProbe {
        nearest: None,
        primary: None,
        ..FakeProbe::single_monitor()
    };
    assert_eq!(predict_decode_box(None, 1200.0, 800.0, 1.0, &no_monitor), None);

    let no_work_area = FakeProbe {
        work_area: None,
        ..FakeProbe::single_monitor()
    };
    let maximized = placement(SHOW_MAXIMIZED, 100, 100, 1300, 900);
    assert_eq!(
        predict_decode_box(Some(&maximized), 1200.0, 800.0, 1.0, &no_work_area),
        None
    );
}

// Contract test (R-IV-9): no frame insets gives `None`.
#[test]
fn predict_decode_box_insets_none_is_none() {
    let probe = FakeProbe {
        insets: None,
        ..FakeProbe::single_monitor()
    };
    assert_eq!(predict_decode_box(None, 1200.0, 800.0, 1.0, &probe), None);
}

// Contract test (T-4): the default size rounds half to even. Scale 1.0 (96 dpi), default width 384.5 gives 384, not 385.
// The insets are zero here so the client width equals the rounded default width.
#[test]
fn predict_decode_box_default_size_rounds_half_to_even() {
    let probe = FakeProbe {
        insets: Some(rect(0, 0, 0, 0)),
        ..FakeProbe::single_monitor()
    };

    let result = predict_decode_box(None, 384.5, 800.0, 1.0, &probe);

    assert_eq!(result, expected_box(384, 800, 1.0, 1.0));
}

// Contract test (R-IV-5): the quality multiplier is passed to the policy unchanged.
#[test]
fn predict_decode_box_quality_multiplier_is_passed_through() {
    let probe = FakeProbe::single_monitor();

    let result = predict_decode_box(None, 1200.0, 800.0, 1.5, &probe);

    assert_eq!(result, expected_box(1200 - 16, 800 - 39, 1.0, 1.5));
}

// Contract test (R-IV-9, Q4): a policy error (quality multiplier 0.0 or NaN) gives `None`.
#[test]
fn predict_decode_box_policy_error_is_none() {
    let probe = FakeProbe::single_monitor();
    assert_eq!(predict_decode_box(None, 1200.0, 800.0, 0.0, &probe), None);
    assert_eq!(predict_decode_box(None, 1200.0, 800.0, f64::NAN, &probe), None);
}

// Contract test (T-3): `width()` wraps like C# unchecked `int` arithmetic and does not panic in a debug build.
#[test]
fn pixel_rect_width_wraps_without_panic() {
    let wide = rect(i32::MIN, 0, i32::MAX, 0);
    assert_eq!(wide.width(), i32::MAX.wrapping_sub(i32::MIN));
}

// Added by the porter (mutation check M1, R-IV-4): a zero width with a positive height is not a prediction, in both forms.
#[test]
fn predict_client_size_zero_width_with_height_returns_none() {
    let normal = predict_client_size(false, rect(0, 0, 16, 100), rect(0, 0, 1920, 1032), INSETS_96);
    assert_eq!(normal, None, "normal: bounds width equals the frame width");
    let maximized = predict_client_size(true, rect(0, 0, 0, 0), rect(0, 0, 0, 1032), INSETS_96);
    assert_eq!(maximized, None, "maximized: work area width is zero");
}

// Added by the porter (mutation check M2, T-4): 1280.5 rounds to 1280 (half to even), not 1281. The quantum of 128 makes the two
// results differ (1280 stays 1280, 1281 quantises to 1408), so the rounding rule is observable in the decode box.
#[test]
fn predict_decode_box_default_size_ties_to_even_is_observable() {
    let probe = FakeProbe {
        insets: Some(rect(0, 0, 0, 0)),
        ..FakeProbe::single_monitor()
    };

    let result = predict_decode_box(None, 1280.5, 800.0, 1.0, &probe);

    assert_eq!(result, Some(DecodeBox::new(1280, 896)));
}
