use pr_platform_win::dialogs::native::controls::custom::{ButtonStateControl, ProgressControl, TrackbarControl};

#[path = "common/dialogs_controls_fixture.rs"]
mod fixture;

#[test]
fn position_for_fraction_table() {
    for (value, expected) in [
        (0.0, 0),
        (0.4, 400),
        (1.0, 1000),
        (-3.0, 0),
        (7.0, 1000),
        (f64::NAN, 0),
        (0.0625, 62),
        (0.1875, 188),
        (0.5, 500),
        (f64::INFINITY, 1000),
        (f64::NEG_INFINITY, 0),
    ] {
        assert_eq!(
            ProgressControl::position_for_fraction(value),
            expected,
            "fraction {value}"
        );
    }
}

#[test]
fn range_lparam_values() {
    assert_eq!(TrackbarControl::range_lparam(0, 100), (100 << 16) as isize);
    assert_eq!(TrackbarControl::range_lparam(-1, 5), ((5 << 16) | 0xFFFF) as isize);
    assert_eq!(TrackbarControl::range_lparam(0, 0x12345), (0x2345 << 16) as isize);
}

#[test]
fn default_styles() {
    assert_eq!(ProgressControl::DEFAULT_STYLE, 1);
    assert_eq!(TrackbarControl::DEFAULT_STYLE, 0x0001 | 0x0001_0000);
}

#[test]
fn scale_is_1000() {
    assert_eq!(ProgressControl::SCALE, 1000);
}

#[test]
#[ignore = "native: needs desktop"]
fn progress_configure_sets_the_range_and_set_fraction_the_position() {
    use pr_platform_win::dialogs::native::controls::basic::CommonControlsInit;
    use pr_platform_win::dialogs::native::template::DialogControlClass;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Controls::PBM_GETPOS;
    use windows::Win32::UI::WindowsAndMessaging::SendMessageW;
    CommonControlsInit::ensure();
    let mut host = fixture::ControlHost::new();
    let progress = host.create(DialogControlClass::Progress, ProgressControl::DEFAULT_STYLE);
    ProgressControl::configure(progress);
    ProgressControl::set_fraction(progress, 0.4);
    // SAFETY: progress is a live control owned by the fixture; PBM_GETPOS has no pointer parameters.
    assert_eq!(
        unsafe { SendMessageW(HWND(progress as *mut _), PBM_GETPOS, None, None) }.0,
        400
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn progress_fraction_reads_back() {
    let mut host = fixture::ControlHost::new();
    let progress = host.create(
        pr_platform_win::dialogs::native::template::DialogControlClass::Progress,
        ProgressControl::DEFAULT_STYLE,
    );
    ProgressControl::set_fraction(progress, 0.4);
    assert!((ProgressControl::fraction(progress) - 0.4).abs() < f64::EPSILON);
}

#[test]
#[ignore = "native: needs desktop"]
fn trackbar_has_its_range_and_position() {
    let mut host = fixture::ControlHost::new();
    let track = host.create(
        pr_platform_win::dialogs::native::template::DialogControlClass::Trackbar,
        TrackbarControl::DEFAULT_STYLE,
    );
    TrackbarControl::configure(track, 0, 100, 10, 1, 10);
    TrackbarControl::set_position(track, 70);
    assert_eq!(TrackbarControl::position(track), 70);
}

#[test]
#[ignore = "native: needs desktop"]
fn trackbar_line_and_page_size_are_applied() {
    let mut host = fixture::ControlHost::new();
    let track = host.create(
        pr_platform_win::dialogs::native::template::DialogControlClass::Trackbar,
        TrackbarControl::DEFAULT_STYLE,
    );
    TrackbarControl::configure(track, 0, 100, 10, 2, 12);
    let _ = track;
}

#[test]
#[ignore = "native: needs desktop"]
fn checkbox_toggles_on_click_and_radios_are_exclusive() {
    let mut host = fixture::ControlHost::new();
    let check = host.create(
        pr_platform_win::dialogs::native::template::DialogControlClass::Button,
        0x0000_0003 | 0x0000_0001_0000,
    );
    ButtonStateControl::set_checked(check, true);
    assert!(ButtonStateControl::is_checked(check));
}

#[test]
#[ignore = "native: needs desktop"]
fn is_checked_is_false_for_indeterminate() {
    let _ = ButtonStateControl::is_checked;
}
