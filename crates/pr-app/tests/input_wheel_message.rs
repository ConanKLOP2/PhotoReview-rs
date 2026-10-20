//! Port of the wheel-decoding test in `tests/PhotoReview.App.Tests/Input/TouchpadGesturesTests.cs` (unit W8-05a,
//! contract `contracts/W8-05a.md`). The test lives here, not with W8-05e, because it tests `WheelMessageSource`.
//!
//! Code under test: `input::wheel_message` (stub). `delta` and `screen_point` are `todo!()`, so these tests fail until the
//! porter implements them. `Current()` (Win32 `GetCurrentInputMessageSource`) is not ported here (W8-14a).

use pr_app::input::wheel_message::{WM_MOUSE_HWHEEL, delta, screen_point};

// C#: TouchpadGesturesTests.WheelMessageSource_DecodesTheSignedDeltaAndTheScreenPoint
#[test]
fn wheel_message_decodes_signed_delta_and_screen_point() {
    // wParam: high word is -12 (0xFFF4 as a signed 16-bit value), low word is the key flags (8).
    assert_eq!(delta((0xFFF4_i64 << 16) | 0x0008), -12);
    // wParam: high word is +120 (one standard notch).
    assert_eq!(delta(120_i64 << 16), 120);

    // lParam: low word is x = 300, high word is y = -5 (0xFFFB as a signed 16-bit value).
    let point = screen_point((0xFFFB_i64 << 16) | 300);
    assert_eq!(point.x, 300.0);
    assert_eq!(point.y, -5.0); // a monitor left of or above the primary has negative coordinates
}

// new: the WM_MOUSEHWHEEL message number (horizontal wheel / sideways touchpad swipe).
#[test]
fn wm_mouse_hwheel_is_0x020e() {
    assert_eq!(WM_MOUSE_HWHEEL, 0x020E);
}
