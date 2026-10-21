//! Tests of W8-14a (F): `ClickCounter`, the pure helpers of `InputNative`, `ShellInputEnvironment`, and the three message handlers decoded against a
//! recording port (`tests/common/shell_input_fixture.rs`). C# tests: `ClickCounterTests`, `InputNativeTests`, and the decoding tests of
//! `KeyboardInputTests` / `PointerInputTests` (baseline `d92d6aae`). The few tests that call the OS are `#[ignore = "native: needs desktop"]`:
//! they only read the keyboard state of their own thread, never show a window and never move the cursor.
#![cfg(windows)]

#[path = "common/shell_input_fixture.rs"]
mod shell_input_fixture;

use std::rc::Rc;

use pr_app::input::key_id::KeyId;
use pr_app::input::key_mapping;
use pr_app::input::primitives::{KeyModifiers, PointerButton};
use pr_platform_win::shell_input::click_counter::ClickCounter;
use pr_platform_win::shell_input::environment::ShellInputEnvironment;
use pr_platform_win::shell_input::handlers::{KeyboardMessageHandler, PointerMessageHandler, WheelMessageHandler};
use pr_platform_win::shell_input::native::{
    self, KEY_EXTENDED_BIT, KEY_REPEAT_BIT, MK_CONTROL, MK_LBUTTON, SCAN_CODE_RIGHT_SHIFT, VK_CONTROL, VK_LSHIFT,
    VK_MENU, VK_RSHIFT, VK_SHIFT, dpi_of, is_extended_key, is_key_repeat, low_word, modifiers_from_key_states,
    signed_high_word, signed_low_word,
};
use pr_platform_win::win32::messages::{
    WM_CAPTURECHANGED, WM_CHAR, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK,
    WM_MBUTTONDOWN, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use shell_input_fixture::{Call, RecordingInput, high_low_wparam, key_lparam, point_lparam, send};

// ---------------------------------------------------------------------------------------------------------------------------------------------
// ClickCounter (C# ClickCounterTests)
// ---------------------------------------------------------------------------------------------------------------------------------------------

const TIME: i32 = 500; // GetDoubleClickTime default
const SLOP: i32 = 4; // SM_CXDOUBLECLK / SM_CYDOUBLECLK in the tests (half = 2)

fn register(counter: &mut ClickCounter, button: PointerButton, x: i32, y: i32, t: i32) -> i32 {
    counter.register(button, x, y, t, TIME, SLOP, SLOP)
}

// C#: ClickCounterTests.FirstClick_IsOne_AndTheSecondWithinTimeAndSlopIsTwo_ThirdIsThree
#[test]
fn first_click_is_one_and_the_second_within_time_and_slop_is_two_third_is_three() {
    let mut counter = ClickCounter::new();
    assert_eq!(1, register(&mut counter, PointerButton::Left, 100, 100, 1000));
    assert_eq!(2, register(&mut counter, PointerButton::Left, 100, 100, 1100));
    assert_eq!(3, register(&mut counter, PointerButton::Left, 101, 101, 1200));
}

// C#: ClickCounterTests.TimeWindow_IsInclusive
#[test]
fn time_window_is_inclusive() {
    for (elapsed, expected) in [(500, 2), (501, 1), (0, 2), (-1, 1)] {
        let mut counter = ClickCounter::new();
        register(&mut counter, PointerButton::Left, 10, 10, 1000);
        assert_eq!(
            expected,
            register(&mut counter, PointerButton::Left, 10, 10, 1000 + elapsed),
            "elapsed {elapsed}"
        );
    }
}

// C#: ClickCounterTests.SlopRectangle_IsCenteredOnTheFirstClick_BothAxes
#[test]
fn slop_rectangle_is_centered_on_the_first_click_both_axes() {
    for (dx, dy, expected) in [
        (2, 0, 2),
        (-2, 0, 2),
        (0, 2, 2),
        (0, -2, 2),
        (3, 0, 1),
        (-3, 0, 1),
        (0, 3, 1),
        (0, -3, 1),
    ] {
        let mut counter = ClickCounter::new();
        register(&mut counter, PointerButton::Left, 50, 50, 1000);
        assert_eq!(
            expected,
            register(&mut counter, PointerButton::Left, 50 + dx, 50 + dy, 1100),
            "dx {dx} dy {dy}"
        );
    }
}

// New: the two axes use their OWN slop (width for x, height for y) and the half is an integer division.
#[test]
fn slop_width_applies_to_x_and_slop_height_to_y_with_an_integer_half() {
    let mut counter = ClickCounter::new();
    counter.register(PointerButton::Left, 50, 50, 1000, TIME, 8, 2);
    assert_eq!(
        2,
        counter.register(PointerButton::Left, 54, 51, 1100, TIME, 8, 2),
        "x within 8/2, y within 2/2"
    );
    let mut counter = ClickCounter::new();
    counter.register(PointerButton::Left, 50, 50, 1000, TIME, 8, 2);
    assert_eq!(
        1,
        counter.register(PointerButton::Left, 50, 52, 1100, TIME, 8, 2),
        "y beyond 2/2"
    );
    let mut counter = ClickCounter::new();
    counter.register(PointerButton::Left, 50, 50, 1000, TIME, 8, 2);
    assert_eq!(
        1,
        counter.register(PointerButton::Left, 55, 50, 1100, TIME, 8, 2),
        "x beyond 8/2"
    );
    // 5 / 2 == 2: a slop of 5 allows 2 pixels, not 3.
    let mut counter = ClickCounter::new();
    counter.register(PointerButton::Left, 50, 50, 1000, TIME, 5, 5);
    assert_eq!(2, counter.register(PointerButton::Left, 52, 48, 1100, TIME, 5, 5));
    let mut counter = ClickCounter::new();
    counter.register(PointerButton::Left, 50, 50, 1000, TIME, 5, 5);
    assert_eq!(1, counter.register(PointerButton::Left, 53, 50, 1100, TIME, 5, 5));
}

// C#: ClickCounterTests.ADifferentButton_StartsANewChain
#[test]
fn a_different_button_starts_a_new_chain() {
    let mut counter = ClickCounter::new();
    register(&mut counter, PointerButton::Left, 10, 10, 1000);
    assert_eq!(1, register(&mut counter, PointerButton::Middle, 10, 10, 1050));
    assert_eq!(2, register(&mut counter, PointerButton::Middle, 10, 10, 1100));
    assert_eq!(1, register(&mut counter, PointerButton::Left, 10, 10, 1150));
}

// C#: ClickCounterTests.ASlowSecondClick_RestartsTheChain_AndTheNextOneCountsFromIt
#[test]
fn a_slow_second_click_restarts_the_chain_and_the_next_one_counts_from_it() {
    let mut counter = ClickCounter::new();
    register(&mut counter, PointerButton::Left, 10, 10, 0);
    assert_eq!(1, register(&mut counter, PointerButton::Left, 10, 10, 2000));
    assert_eq!(2, register(&mut counter, PointerButton::Left, 10, 10, 2100));
}

// C#: ClickCounterTests.ChainContinuesFromTheLastClick_NotFromTheFirst
#[test]
fn chain_continues_from_the_last_click_not_from_the_first() {
    let mut counter = ClickCounter::new();
    register(&mut counter, PointerButton::Left, 10, 10, 0);
    register(&mut counter, PointerButton::Left, 10, 10, 400);
    // 800 ms from the first click, 400 ms from the previous one.
    assert_eq!(3, register(&mut counter, PointerButton::Left, 10, 10, 800));
}

// New: the position reference moves with every click too (a drifting chain breaks once it is more than half the slop from the LAST click).
#[test]
fn chain_position_is_measured_from_the_last_click_not_from_the_first() {
    let mut counter = ClickCounter::new();
    register(&mut counter, PointerButton::Left, 10, 10, 0);
    assert_eq!(2, register(&mut counter, PointerButton::Left, 12, 12, 100));
    // 4 pixels from the first click but 2 from the last one.
    assert_eq!(3, register(&mut counter, PointerButton::Left, 14, 14, 200));
}

// New: a click that breaks the chain becomes the new reference for time AND position.
#[test]
fn a_click_that_breaks_the_chain_becomes_the_new_reference_point() {
    let mut counter = ClickCounter::new();
    register(&mut counter, PointerButton::Left, 10, 10, 0);
    assert_eq!(1, register(&mut counter, PointerButton::Left, 100, 100, 100));
    assert_eq!(2, register(&mut counter, PointerButton::Left, 101, 99, 200));
}

// C#: ClickCounterTests.TickCountWrap_StillCountsAsConsecutive
#[test]
fn tick_count_wrap_still_counts_as_consecutive() {
    let mut counter = ClickCounter::new();
    register(&mut counter, PointerButton::Left, 10, 10, i32::MAX - 50);
    // 151 ms apart across the overflow point.
    assert_eq!(2, register(&mut counter, PointerButton::Left, 10, 10, i32::MIN + 100));
}

// C#: ClickCounterTests.Reset_AndSeed_ControlTheChain
#[test]
fn reset_and_seed_control_the_chain() {
    let mut counter = ClickCounter::new();
    register(&mut counter, PointerButton::Left, 10, 10, 0);
    counter.reset();
    assert_eq!(1, register(&mut counter, PointerButton::Left, 10, 10, 10));

    counter.seed(PointerButton::Left, 10, 10, 100, 4);
    assert_eq!(5, register(&mut counter, PointerButton::Left, 10, 10, 100));
}

// New: a fresh counter and a default one have no chain (the first click at time 0 on (0, 0) is a 1, not a continuation).
#[test]
fn a_new_counter_has_no_chain() {
    let mut counter = ClickCounter::default();
    assert_eq!(1, register(&mut counter, PointerButton::Left, 0, 0, 0));
    let mut counter = ClickCounter::new();
    assert_eq!(1, register(&mut counter, PointerButton::Left, 0, 0, 0));
    assert_eq!(2, register(&mut counter, PointerButton::Left, 0, 0, 0));
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// InputNative pure helpers (C# InputNativeTests)
// ---------------------------------------------------------------------------------------------------------------------------------------------

// C#: InputNativeTests.DpiOf_RoundsTheScaleToWholeDpi
#[test]
fn dpi_of_rounds_the_scale_to_whole_dpi() {
    for (scale, expected) in [
        (1.0, 96),
        (1.25, 120),
        (1.5, 144),
        (2.0, 192),
        (0.0, 1),
        (0.5, 48),
        (0.004, 1),
        (f64::NAN, 1),
        (-3.0, 1),
    ] {
        assert_eq!(expected, dpi_of(scale), "scale {scale}");
    }
}

// New: the C# `Math.Round` is banker's rounding (ties to even), not away from zero.
#[test]
fn dpi_of_rounds_ties_to_even() {
    let two_and_a_half = 2.5 / 96.0;
    assert_eq!(2.5, two_and_a_half * 96.0, "the test scale must hit the tie exactly");
    assert_eq!(2, dpi_of(two_and_a_half));
    let three_and_a_half = 3.5 / 96.0;
    assert_eq!(3.5, three_and_a_half * 96.0);
    assert_eq!(4, dpi_of(three_and_a_half));
}

// C#: InputNativeTests.SignedLowWord_ReadsAnInt16
#[test]
fn signed_low_word_reads_an_int16() {
    for (value, expected) in [
        (0x0000_0064_isize, 100),
        (0x0000_FFFF, -1),
        (0x0000_8000, -32768),
        (0x7FFF, 32767),
    ] {
        assert_eq!(expected, signed_low_word(value), "{value:#x}");
    }
    // The high word is ignored, also when the whole value is negative (mask first, T2).
    assert_eq!(100, signed_low_word(0x1234_0064));
    assert_eq!(-1, signed_low_word(-1));
    assert_eq!(-2, signed_low_word(-2));
    assert_eq!(0, signed_low_word(-65536));
}

// C#: InputNativeTests.SignedHighWord_ReadsAnInt16 (the `WheelDelta` half is `wheel_message::delta`)
#[test]
fn signed_high_word_reads_an_int16() {
    for (value, expected) in [
        (0x0064_0000_isize, 100),
        (0xFFFF_0000_u32 as i32 as isize, -1),
        (0x8000_0000_u32 as i32 as isize, -32768),
        (0x7FFF_0000, 32767),
    ] {
        assert_eq!(expected, signed_high_word(value), "{value:#x}");
        assert_eq!(expected, pr_app::input::wheel_message::delta(value as i64));
    }
    // The low word and bits above bit 31 are ignored.
    assert_eq!(100, signed_high_word(0x0064_FFFF));
    assert_eq!(100, signed_high_word(0x5_0064_1234));
    assert_eq!(-1, signed_high_word(-1));
    assert_eq!(0, signed_high_word(0xFFFF));
}

// C#: InputNativeTests.LowWord_IsUnsigned
#[test]
fn low_word_is_unsigned() {
    assert_eq!(0xFFFF, low_word(0x1_FFFF));
    assert_eq!(0xFFFF, low_word(-1));
    assert_eq!(0, low_word(0x1_0000));
    assert_eq!(0x8000, low_word(0x8000));
}

// New: each key state maps to its own flag; both Windows keys give the same flag; only a negative state is "down" (the low "toggled" bit is not).
#[test]
fn modifiers_from_key_states_maps_each_key_to_its_flag() {
    let down = i16::MIN; // 0x8000: the high bit
    let up = 0;
    assert_eq!(KeyModifiers::NONE, modifiers_from_key_states(up, up, up, up, up));
    assert_eq!(KeyModifiers::CONTROL, modifiers_from_key_states(down, up, up, up, up));
    assert_eq!(KeyModifiers::ALT, modifiers_from_key_states(up, down, up, up, up));
    assert_eq!(KeyModifiers::SHIFT, modifiers_from_key_states(up, up, down, up, up));
    assert_eq!(KeyModifiers::WINDOWS, modifiers_from_key_states(up, up, up, down, up));
    assert_eq!(KeyModifiers::WINDOWS, modifiers_from_key_states(up, up, up, up, down));
    assert_eq!(KeyModifiers::WINDOWS, modifiers_from_key_states(up, up, up, down, down));
    assert_eq!(
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        modifiers_from_key_states(down, up, down, up, up)
    );
    assert_eq!(
        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT | KeyModifiers::WINDOWS,
        modifiers_from_key_states(-1, -128, -2, -3, up)
    );
    // The toggled bit alone (state 1) and zero are not "down".
    assert_eq!(KeyModifiers::NONE, modifiers_from_key_states(1, 1, 1, 1, 1));
    // The expected bits are the C# `ModifierKeys` values.
    assert_eq!(1, KeyModifiers::ALT.0);
    assert_eq!(2, KeyModifiers::CONTROL.0);
    assert_eq!(4, KeyModifiers::SHIFT.0);
    assert_eq!(8, KeyModifiers::WINDOWS.0);
}

// New: the key constants of the C# `InputNative` and the two `lParam` bits, with positive and negative (sign-extended) values.
#[test]
fn key_bits_table() {
    assert_eq!(
        (0x10, 0x11, 0x12, 0xA0, 0xA1),
        (VK_SHIFT, VK_CONTROL, VK_MENU, VK_LSHIFT, VK_RSHIFT)
    );
    assert_eq!((0x5B, 0x5C), (native::VK_LWIN, native::VK_RWIN));
    assert_eq!((1, 8, 0x36), (MK_LBUTTON, MK_CONTROL, SCAN_CODE_RIGHT_SHIFT));
    assert_eq!((0x4000_0000, 0x0100_0000), (KEY_REPEAT_BIT, KEY_EXTENDED_BIT));

    // (lparam, extended, repeat)
    let rows: [(isize, bool, bool); 8] = [
        (0, false, false),
        (0x4000_0000, false, true),
        (0x0100_0000, true, false),
        (0x4100_0001, true, true),
        (0x8000_0000_u32 as isize, false, false),
        (0xC000_0001_u32 as i32 as isize, false, true),
        (0xFFFF_FFFF_u32 as i32 as isize, true, true),
        (0x3EFF_FFFF, false, false),
    ];
    for (lparam, extended, repeat) in rows {
        assert_eq!(extended, is_extended_key(lparam), "extended {lparam:#x}");
        assert_eq!(repeat, is_key_repeat(lparam), "repeat {lparam:#x}");
    }
    assert!(is_extended_key(-1) && is_key_repeat(-1));
}

// New: the tick count and the message time are 32-bit values of the same (wrapping) clock; the pure part is the C# `Math.Max(1, ...)`.
#[test]
fn default_environment_members_are_real_closures() {
    let env = ShellInputEnvironment::default();
    // Type check of every member.
    let _: &dyn Fn() -> KeyModifiers = &*env.modifiers;
    let _: &dyn Fn() -> i32 = &*env.message_time;
    let _: &dyn Fn() -> i32 = &*env.tick_count;
    let _: &dyn Fn() -> pr_app::input::touchpad_gestures::WheelDeviceHint = &*env.wheel_device;
    let _: &dyn Fn(u32) -> (i32, i32) = &*env.double_click_size_pixels;
    // Harmless real reads.
    assert!((env.double_click_time_ms)() > 0);
    assert_eq!(
        ((env.double_click_size_pixels)(96)).0 > 0,
        native::double_click_size_pixels(96).0 > 0
    );

    // Struct-update syntax replaces single members.
    let env = ShellInputEnvironment {
        message_time: Box::new(|| 1000),
        double_click_size_pixels: Box::new(|dpi| (dpi as i32, 4)),
        ..Default::default()
    };
    assert_eq!(1000, (env.message_time)());
    assert_eq!((144, 4), (env.double_click_size_pixels)(144));
    assert!(format!("{env:?}").contains("ShellInputEnvironment"));
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// KeyboardMessageHandler (C# KeyboardInputTests, decoding half)
// ---------------------------------------------------------------------------------------------------------------------------------------------

fn vk_key(virtual_key: usize) -> KeyId {
    key_mapping::from_virtual_key(virtual_key as i32, false)
}

fn keyboard_rig() -> (Rc<RecordingInput>, KeyboardMessageHandler) {
    let port = Rc::new(RecordingInput::new());
    let handler = KeyboardMessageHandler::new(port.clone());
    (port, handler)
}

// C#: KeyboardInputTests.ShiftSide_IsTakenFromTheScanCode
#[test]
fn resolve_virtual_key_takes_the_shift_side_from_the_scan_code() {
    for (virtual_key, scan_code, expected) in [
        (0x10, 0x2A_i64, 0xA0),
        (0x10, 0x36, 0xA1),
        (0x11, 0x36, 0x11),
        (0x41, 0x1E, 0x41),
    ] {
        assert_eq!(
            expected,
            KeyboardMessageHandler::resolve_virtual_key(virtual_key, (scan_code << 16) as isize),
            "vk {virtual_key:#x} scan {scan_code:#x}"
        );
    }
    // Only bits 16..23 count: the extended and repeat bits and the repeat count do not change the answer.
    let flags = 0x0100_0000 | 0x4000_0000 | 1;
    assert_eq!(
        VK_RSHIFT,
        KeyboardMessageHandler::resolve_virtual_key(VK_SHIFT, (0x36 << 16) | flags)
    );
    assert_eq!(
        VK_LSHIFT,
        KeyboardMessageHandler::resolve_virtual_key(VK_SHIFT, (0x35 << 16) | flags)
    );
    assert_eq!(
        VK_LSHIFT,
        KeyboardMessageHandler::resolve_virtual_key(VK_SHIFT, 0x0000_0036)
    );
    assert_eq!(VK_LSHIFT, KeyboardMessageHandler::resolve_virtual_key(VK_SHIFT, 0));
    // A sign-extended (negative) lParam.
    assert_eq!(
        VK_RSHIFT,
        KeyboardMessageHandler::resolve_virtual_key(VK_SHIFT, (0xC136_0001_u32 as i32) as isize)
    );
    // Only VK_SHIFT is rewritten: a synthetic VK_LSHIFT / VK_RSHIFT is kept (T5).
    assert_eq!(
        VK_LSHIFT,
        KeyboardMessageHandler::resolve_virtual_key(VK_LSHIFT, 0x36 << 16)
    );
    assert_eq!(
        VK_RSHIFT,
        KeyboardMessageHandler::resolve_virtual_key(VK_RSHIFT, 0x2A << 16)
    );
}

// C#: KeyboardInputTests.RightShift_MapsToTheRightShiftKeyId
#[test]
fn right_shift_maps_to_the_right_shift_key_id() {
    assert_eq!(KeyId::RIGHT_SHIFT, key_mapping::from_virtual_key(0xA1, false));
    let (port, handler) = keyboard_rig();
    // WM_KEYDOWN always carries VK_SHIFT; the scan code says which side.
    assert_eq!(
        None,
        send(&handler, WM_KEYDOWN, VK_SHIFT as usize, key_lparam(0x36, false, false))
    );
    assert_eq!(
        None,
        send(&handler, WM_KEYDOWN, VK_SHIFT as usize, key_lparam(0x2A, false, false))
    );
    let keys: Vec<KeyId> = port
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::KeyDown { key, .. } => Some(key),
            _ => None,
        })
        .collect();
    assert_eq!(vec![KeyId::RIGHT_SHIFT, KeyId::LEFT_SHIFT], keys);
}

// C#: KeyboardInputTests.KeyUpAndChar_OfAHandledKeyAreSwallowed_ButNotOfAnUnhandledOne
#[test]
fn a_handled_key_swallows_its_key_up_and_char_but_an_unhandled_one_does_not() {
    let (port, handler) = keyboard_rig();
    let (f9, right, left) = (0x78, 0x27, 0x25);
    port.handled_keys.borrow_mut().push(vk_key(right));
    let lparam = key_lparam(0x1E, false, false);

    // F9: not handled.
    assert_eq!(None, send(&handler, WM_KEYDOWN, f9, lparam));
    assert_eq!(None, send(&handler, WM_CHAR, 'x' as usize, 0));
    assert_eq!(None, send(&handler, WM_KEYUP, f9, 0));

    // Right: handled.
    assert_eq!(Some(0), send(&handler, WM_KEYDOWN, right, lparam));
    assert_eq!(Some(0), send(&handler, WM_CHAR, 'x' as usize, 0));
    assert_eq!(Some(0), send(&handler, WM_SYSCHAR, 'x' as usize, 0));
    // Another key released: neither swallowed nor does it lose the mark.
    assert_eq!(None, send(&handler, WM_KEYUP, left, 0));
    assert_eq!(Some(0), send(&handler, WM_KEYUP, right, 0));
    // Only once.
    assert_eq!(None, send(&handler, WM_KEYUP, right, 0));
}

// New: the system flavours of the same rules, and an unhandled key down resets both the char answer and the pending key-up swallow.
#[test]
fn sys_key_up_is_swallowed_like_key_up_and_an_unhandled_key_down_resets_the_swallow_state() {
    let (port, handler) = keyboard_rig();
    let (right, f9) = (0x27, 0x78);
    port.handled_keys.borrow_mut().push(vk_key(right));
    let lparam = key_lparam(0x4D, true, false);

    assert_eq!(Some(0), send(&handler, WM_SYSKEYDOWN, right, lparam));
    assert_eq!(Some(0), send(&handler, WM_SYSKEYUP, right, 0));
    assert_eq!(None, send(&handler, WM_SYSKEYUP, right, 0));

    // Handled, then an unhandled key goes down: the chars of the new key are not swallowed, and the first key's up is no longer swallowed.
    assert_eq!(Some(0), send(&handler, WM_KEYDOWN, right, lparam));
    assert_eq!(None, send(&handler, WM_KEYDOWN, f9, lparam));
    assert_eq!(None, send(&handler, WM_CHAR, 'x' as usize, 0));
    assert_eq!(None, send(&handler, WM_SYSCHAR, 'x' as usize, 0));
    assert_eq!(None, send(&handler, WM_KEYUP, right, 0));

    // The last handled key replaces the previous mark.
    port.handled_keys.borrow_mut().push(vk_key(0x25));
    assert_eq!(Some(0), send(&handler, WM_KEYDOWN, right, lparam));
    assert_eq!(Some(0), send(&handler, WM_KEYDOWN, 0x25, lparam));
    assert_eq!(None, send(&handler, WM_KEYUP, right, 0));
    assert_eq!(Some(0), send(&handler, WM_KEYUP, 0x25, 0));
}

// New: "no handled key" is -1, not a real key code: a key up of virtual key 1 (VK_LBUTTON) is not swallowed after an unhandled key down nor
// after the swallow was used up (guards the two `-1` resets).
#[test]
fn the_no_handled_key_marker_never_matches_a_real_virtual_key() {
    let (port, handler) = keyboard_rig();
    let lparam = key_lparam(0x1E, false, false);
    assert_eq!(None, send(&handler, WM_KEYUP, 1, 0), "fresh handler");
    assert_eq!(None, send(&handler, WM_KEYDOWN, 0x78, lparam)); // unhandled
    assert_eq!(None, send(&handler, WM_KEYUP, 1, 0), "after an unhandled key down");

    port.handled_keys.borrow_mut().push(vk_key(0x27));
    assert_eq!(Some(0), send(&handler, WM_KEYDOWN, 0x27, lparam));
    assert_eq!(Some(0), send(&handler, WM_KEYUP, 0x27, 0));
    assert_eq!(None, send(&handler, WM_KEYUP, 1, 0), "after the swallow was used");
}

// New: a handled Shift is swallowed on key up by the RAW wParam (VK_SHIFT), not the resolved VK_RSHIFT, because the key-up message carries VK_SHIFT.
#[test]
fn a_handled_shift_key_up_is_matched_by_the_raw_wparam() {
    let (port, handler) = keyboard_rig();
    port.handled_keys.borrow_mut().push(KeyId::RIGHT_SHIFT);
    assert_eq!(
        Some(0),
        send(&handler, WM_KEYDOWN, VK_SHIFT as usize, key_lparam(0x36, false, false))
    );
    assert_eq!(
        None,
        send(&handler, WM_KEYUP, VK_RSHIFT as usize, key_lparam(0x36, false, false))
    );
    assert_eq!(
        Some(0),
        send(&handler, WM_KEYUP, VK_SHIFT as usize, key_lparam(0x36, false, false))
    );
}

// New: virtual key, extended bit, repeat bit and the environment message time all reach the port.
#[test]
fn key_down_decodes_virtual_key_extended_bit_repeat_bit_and_message_time() {
    let (port, handler) = keyboard_rig();
    port.message_time.set(4242);

    send(&handler, WM_KEYDOWN, 0x41, key_lparam(0x1E, false, false));
    send(&handler, WM_KEYDOWN, 0x41, key_lparam(0x1E, false, true));
    send(&handler, WM_KEYDOWN, VK_CONTROL as usize, key_lparam(0x1D, true, false));
    send(
        &handler,
        WM_KEYDOWN,
        VK_CONTROL as usize,
        key_lparam(0x1D, false, false),
    );
    send(&handler, WM_KEYDOWN, VK_MENU as usize, key_lparam(0x38, true, true));
    // A sign-extended (negative) lParam still decodes (T2).
    send(&handler, WM_KEYDOWN, 0x41, 0xC100_0000_u32 as i32 as isize);
    port.message_time.set(-7);
    send(&handler, WM_KEYDOWN, 0x41, 0);

    let key = |is_repeat: bool, message_time: i32, key: KeyId| Call::KeyDown {
        key,
        is_system_key: false,
        is_repeat,
        message_time,
    };
    assert_eq!(
        vec![
            key(false, 4242, KeyId::A),
            key(true, 4242, KeyId::A),
            key(false, 4242, KeyId::RIGHT_CTRL),
            key(false, 4242, vk_key(VK_CONTROL as usize)),
            key(true, 4242, KeyId::RIGHT_ALT),
            key(true, 4242, KeyId::A),
            key(false, -7, KeyId::A),
        ],
        port.calls()
    );
    assert_ne!(KeyId::RIGHT_CTRL, vk_key(VK_CONTROL as usize));
}

// New (C# HandleKeyDown(isSystemKey: msg == WmSysKeyDown)).
#[test]
fn sys_key_down_sets_the_system_flag() {
    let (port, handler) = keyboard_rig();
    send(&handler, WM_SYSKEYDOWN, 0x46, key_lparam(0x21, false, false));
    send(&handler, WM_KEYDOWN, 0x46, key_lparam(0x21, false, false));
    let flags: Vec<bool> = port
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::KeyDown { is_system_key, .. } => Some(is_system_key),
            _ => None,
        })
        .collect();
    assert_eq!(vec![true, false], flags);
}

// New: messages the keyboard handler does not know are not handled and never reach the port.
#[test]
fn keyboard_handler_ignores_other_messages() {
    let (port, handler) = keyboard_rig();
    port.handled_keys.borrow_mut().push(vk_key(0x27));
    send(&handler, WM_KEYDOWN, 0x27, 0);
    port.clear();
    for msg in [WM_MOUSEMOVE, WM_LBUTTONDOWN, WM_MOUSEWHEEL, 0, 0x0007] {
        assert_eq!(None, send(&handler, msg, 0x27, 0), "msg {msg:#x}");
    }
    assert!(port.calls().is_empty());
    assert!(format!("{handler:?}").contains("KeyboardMessageHandler"));
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// PointerMessageHandler (C# PointerInputTests, decoding half)
// ---------------------------------------------------------------------------------------------------------------------------------------------

fn pointer_rig() -> (Rc<RecordingInput>, PointerMessageHandler) {
    let port = Rc::new(RecordingInput::new());
    let handler = PointerMessageHandler::new(port.clone());
    (port, handler)
}

// New: all the press messages of the C# table, signed coordinates, and the answer follows the port (`Some(0)` only for a handled press).
#[test]
fn pointer_handler_decodes_buttons_and_signed_coordinates() {
    let table = [
        (WM_LBUTTONDOWN, PointerButton::Left),
        (WM_LBUTTONDBLCLK, PointerButton::Left),
        (WM_MBUTTONDOWN, PointerButton::Middle),
        (WM_MBUTTONDBLCLK, PointerButton::Middle),
        (WM_RBUTTONDOWN, PointerButton::Right),
        (WM_RBUTTONDBLCLK, PointerButton::Right),
    ];
    for (msg, button) in table {
        for (x, y) in [(120, 340), (-1, -1), (-32768, 32767), (0, -300)] {
            let (port, handler) = pointer_rig();
            port.pointer_down_result.set(true);
            assert_eq!(
                Some(0),
                send(&handler, msg, 0, point_lparam(x, y)),
                "msg {msg:#x} ({x}, {y})"
            );
            port.pointer_down_result.set(false);
            assert_eq!(
                None,
                send(&handler, msg, 0, point_lparam(x, y)),
                "unhandled msg {msg:#x}"
            );
            assert_eq!(
                vec![Call::PointerDown(button, x, y), Call::PointerDown(button, x, y)],
                port.calls()
            );
        }
    }
    // The same coordinates from a sign-extended lParam.
    let (port, handler) = pointer_rig();
    send(&handler, WM_LBUTTONDOWN, 0, 0xFFFF_FFFF_u32 as i32 as isize);
    assert_eq!(vec![Call::PointerDown(PointerButton::Left, -1, -1)], port.calls());
}

// New: WM_MOUSEMOVE reads MK_LBUTTON from the low word of wParam; other flags (and the high word) do not count.
#[test]
fn mouse_move_decodes_coordinates_and_the_left_button_flag() {
    let (port, handler) = pointer_rig();
    port.pointer_down_result.set(true);
    // wParam: 0x0001 MK_LBUTTON, 0x0002 MK_RBUTTON, 0x0008 MK_CONTROL, 0x0010 MK_MBUTTON.
    for (wparam, expected) in [
        (0x0001_usize, true),
        (0x0009, true),
        (0x0000, false),
        (0x0002, false),
        (0x0008, false),
        (0x0010, false),
        (0x0001_0000, false),
    ] {
        port.clear();
        assert_eq!(
            None,
            send(&handler, WM_MOUSEMOVE, wparam, point_lparam(-5, 77)),
            "wparam {wparam:#x}"
        );
        assert_eq!(
            vec![Call::PointerMove(-5, 77, expected)],
            port.calls(),
            "wparam {wparam:#x}"
        );
    }
}

// C#: PointerInputTests.RightAndExtraButtons_AreNotHandled_ButStopAGlide (the handler half)
#[test]
fn x_buttons_pick_xbutton1_or_xbutton2_from_the_high_word_and_never_handle() {
    for msg in [WM_XBUTTONDOWN, WM_XBUTTONDBLCLK] {
        for (which, button) in [
            (1, PointerButton::XButton1),
            (2, PointerButton::XButton2),
            (0, PointerButton::XButton2),
            (3, PointerButton::XButton2),
        ] {
            let (port, handler) = pointer_rig();
            port.pointer_down_result.set(true); // even a "handled" answer is ignored (C# `return false`)
            // The low word carries key flags that must not disturb the button.
            let wparam = high_low_wparam(which, 0x0001);
            assert_eq!(
                None,
                send(&handler, msg, wparam, point_lparam(30, -40)),
                "msg {msg:#x} which {which}"
            );
            assert_eq!(vec![Call::PointerDown(button, 30, -40)], port.calls());
        }
    }
    // XBUTTONUP is not decoded at all.
    let (port, handler) = pointer_rig();
    assert_eq!(
        None,
        send(&handler, WM_XBUTTONUP, high_low_wparam(1, 0), point_lparam(1, 2))
    );
    assert!(port.calls().is_empty());
}

// C#: PointerInputTests.RightAndExtraButtons_AreNotHandled_ButStopAGlide (left up, move, capture changed)
#[test]
fn left_up_move_and_capture_changed_never_handle() {
    let (port, handler) = pointer_rig();
    port.pointer_down_result.set(true);
    assert_eq!(None, send(&handler, WM_LBUTTONUP, 0, point_lparam(-9, 14)));
    assert_eq!(None, send(&handler, WM_MOUSEMOVE, 1, point_lparam(3, 4)));
    assert_eq!(None, send(&handler, WM_CAPTURECHANGED, 0, 0x1234));
    assert_eq!(
        vec![
            Call::LeftButtonUp(-9, 14),
            Call::PointerMove(3, 4, true),
            Call::CaptureLost
        ],
        port.calls()
    );
}

// New: messages the pointer handler does not know never reach the port (the right button release and the keyboard included).
#[test]
fn pointer_handler_ignores_other_messages() {
    let (port, handler) = pointer_rig();
    port.pointer_down_result.set(true);
    for msg in [WM_RBUTTONUP, WM_KEYDOWN, WM_MOUSEWHEEL, WM_MOUSEHWHEEL, 0] {
        assert_eq!(None, send(&handler, msg, 0, point_lparam(1, 1)), "msg {msg:#x}");
    }
    assert!(port.calls().is_empty());
    assert!(format!("{handler:?}").contains("PointerMessageHandler"));
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// WheelMessageHandler (C# PointerInputTests, decoding half)
// ---------------------------------------------------------------------------------------------------------------------------------------------

fn wheel_rig() -> (Rc<RecordingInput>, WheelMessageHandler) {
    let port = Rc::new(RecordingInput::new());
    let handler = WheelMessageHandler::new(port.clone());
    (port, handler)
}

// C#: PointerInputTests.Wheel_ScreenCoordinates_AreConvertedToTheClientAtTheWindowDpi, Wheel_Delta_IsSigned_NegativeZoomsOut,
// Wheel_CtrlFlag_FromWParam_TogglesBetweenZoomAndNavigate (the handler halves)
#[test]
fn wheel_handler_converts_the_screen_point_through_the_port_and_reads_delta_and_ctrl() {
    // Screen (-1820, 530) with the client origin at (-1920, 500) and 2 pixels per DIP: client (50, 15) DIP.
    for (msg, vertical) in [(WM_MOUSEWHEEL, true), (WM_MOUSEHWHEEL, false)] {
        for (delta, flags, ctrl) in [
            (120, 0, false),
            (-120, 0, false),
            (-240, MK_CONTROL as u32, true),
            (360, MK_CONTROL as u32 | 0x0001, true),
            (1, 0x0001, false),
        ] {
            let (port, handler) = wheel_rig();
            port.origin.set((-1920.0, 500.0));
            port.scale.set(2.0);
            port.wheel_result.set(true);
            let result = send(&handler, msg, high_low_wparam(delta, flags), point_lparam(-1820, 530));
            assert_eq!(Some(0), result, "handled {msg:#x} delta {delta}");
            let wheel = if vertical {
                Call::VerticalWheel {
                    delta,
                    ctrl,
                    client: (50.0, 15.0),
                }
            } else {
                Call::HorizontalWheel {
                    delta,
                    ctrl,
                    client: (50.0, 15.0),
                }
            };
            assert_eq!(
                vec![Call::ScreenToClient(-1820.0, 530.0), wheel],
                port.calls(),
                "msg {msg:#x} delta {delta} flags {flags:#x}"
            );

            // An unhandled wheel goes on.
            port.wheel_result.set(false);
            assert_eq!(
                None,
                send(&handler, msg, high_low_wparam(delta, flags), point_lparam(-1820, 530))
            );
        }
    }
}

// New: the same bits arrive sign-extended (a 64-bit negative wParam / lParam) and still decode.
#[test]
fn wheel_handler_reads_sign_extended_parameters() {
    let (port, handler) = wheel_rig();
    port.wheel_result.set(true);
    let wparam = (0xFF88_0008_u32 as i32) as isize as usize; // delta -120, MK_CONTROL
    let lparam = 0xFFF6_FFFB_u32 as i32 as isize; // screen (-5, -10)
    assert_eq!(Some(0), send(&handler, WM_MOUSEWHEEL, wparam, lparam));
    assert_eq!(
        vec![
            Call::ScreenToClient(-5.0, -10.0),
            Call::VerticalWheel {
                delta: -120,
                ctrl: true,
                client: (-5.0, -10.0)
            }
        ],
        port.calls()
    );
}

// New.
#[test]
fn wheel_handler_ignores_other_messages() {
    let (port, handler) = wheel_rig();
    port.wheel_result.set(true);
    for msg in [WM_MOUSEMOVE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_CHAR, 0] {
        assert_eq!(
            None,
            send(&handler, msg, high_low_wparam(120, 0), point_lparam(1, 1)),
            "msg {msg:#x}"
        );
    }
    assert!(
        port.calls().is_empty(),
        "the port is not even asked to convert the point"
    );
    assert!(format!("{handler:?}").contains("WheelMessageHandler"));
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// Native (read the OS; hidden or no windows; the cursor is never moved)
// ---------------------------------------------------------------------------------------------------------------------------------------------

mod native_tests {
    use super::*;
    use windows::Win32::System::SystemInformation::GetTickCount64;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardState, SetKeyboardState};
    use windows::Win32::UI::WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW, PostThreadMessageW, WM_APP};

    /// Restores the keyboard state of this thread when dropped (C# `finally { SetKeyboardState(saved); }`).
    struct KeyboardStateGuard([u8; 256]);

    impl KeyboardStateGuard {
        fn save() -> Self {
            let mut state = [0u8; 256];
            // SAFETY: `state` is a live 256-byte buffer, the size `GetKeyboardState` writes.
            unsafe { GetKeyboardState(&mut state) }.expect("GetKeyboardState");
            Self(state)
        }
    }

    impl Drop for KeyboardStateGuard {
        fn drop(&mut self) {
            // SAFETY: the saved buffer is a live 256-byte array.
            let _ = unsafe { SetKeyboardState(&self.0) };
        }
    }

    fn set_state(state: &[u8; 256]) {
        // SAFETY: `state` is a live 256-byte buffer.
        unsafe { SetKeyboardState(state) }.expect("SetKeyboardState");
    }

    // C#: InputNativeTests.ReadModifiers_MapsEachKeyStateToItsFlag
    #[test]
    #[ignore = "native: needs desktop"]
    fn read_modifiers_maps_each_key_state_to_its_flag() {
        let _guard = KeyboardStateGuard::save();
        for (virtual_key, expected) in [
            (0x11_usize, KeyModifiers::CONTROL),
            (0x12, KeyModifiers::ALT),
            (0x10, KeyModifiers::SHIFT),
            (0x5B, KeyModifiers::WINDOWS),
            (0x5C, KeyModifiers::WINDOWS),
        ] {
            let mut state = [0u8; 256];
            state[virtual_key] = 0x80;
            set_state(&state);
            assert_eq!(expected, native::read_modifiers(), "vk {virtual_key:#x}");

            state[virtual_key] = 0;
            state[0x11] = 0x80;
            state[0x10] = 0x80;
            set_state(&state);
            assert_eq!(KeyModifiers::CONTROL | KeyModifiers::SHIFT, native::read_modifiers());
        }
    }

    // C#: InputNativeTests.ReadModifiers_NoKeyDown_IsNone
    #[test]
    #[ignore = "native: needs desktop"]
    fn read_modifiers_no_key_down_is_none() {
        let _guard = KeyboardStateGuard::save();
        set_state(&[0u8; 256]);
        assert_eq!(KeyModifiers::NONE, native::read_modifiers());
    }

    // C#: InputNativeTests.SystemMetrics_ComeFromWindows_ScaledByDpi
    #[test]
    #[ignore = "native: needs desktop"]
    fn system_metrics_come_from_windows_scaled_by_dpi() {
        let (drag_x, drag_y) = native::drag_metrics_pixels(96);
        let (drag_x2, _) = native::drag_metrics_pixels(192);
        assert!(drag_x > 0 && drag_y > 0);
        assert!(drag_x2 >= drag_x);

        let (click_w, click_h) = native::double_click_size_pixels(96);
        assert!(click_w > 0 && click_h > 0);
        assert!(native::double_click_time_ms() >= 200);
    }

    // C#: InputNativeTests.CursorScreenPixel_IsTheCursorPosition_OrNullWhenWindowsCannotSayIt
    #[test]
    #[ignore = "native: needs desktop"]
    fn cursor_screen_pixel_is_the_cursor_position_or_none() {
        // On a hidden desktop `GetCursorPos` fails: then it must be `None`, not (0, 0).
        let direct = pr_platform_win::win32::comctl::user32::get_cursor_pos();
        let read = native::cursor_screen_pixel();
        assert_eq!(direct.is_some(), read.is_some());
        if let (Some(point), Some(read)) = (direct, read) {
            // The user may move the mouse between the two reads: only exact equality of the mapping is asserted for a still cursor.
            assert!((read.x - f64::from(point.x)).abs() < 400.0 && (read.y - f64::from(point.y)).abs() < 400.0);
        }
    }

    // New: `message_time` and `tick_count` are the same clock (D-W8B4I-15): a message posted to this thread is stamped with `GetMessageTime`
    // right now, and the tick count read right after differs by (wrap-aware) a small amount, never by a different origin.
    #[test]
    #[ignore = "native: needs desktop"]
    fn tick_count_and_message_time_share_a_clock() {
        // SAFETY: plain integer arguments; the posted thread message has no payload.
        unsafe { PostThreadMessageW(GetCurrentThreadId(), WM_APP, Default::default(), Default::default()) }
            .expect("PostThreadMessageW");
        let mut msg = MSG::default();
        // SAFETY: `msg` is a live local MSG that the OS fills; no window filter.
        let found = unsafe { PeekMessageW(&mut msg, None, WM_APP, WM_APP, PM_REMOVE) };
        assert!(found.as_bool(), "the posted message must be in the queue");

        let message_time = native::message_time();
        let tick_count = native::tick_count();
        // SAFETY: no arguments.
        let wide = unsafe { GetTickCount64() } as u32 as i32;
        let behind = tick_count.wrapping_sub(message_time);
        assert!(
            (0..2000).contains(&behind),
            "tick {tick_count} message {message_time}: {behind} ms apart"
        );
        let drift = wide.wrapping_sub(tick_count);
        assert!(
            (0..2000).contains(&drift),
            "tick count is the GetTickCount64 clock: {drift}"
        );
    }

    // New: a plain wheel device read outside a wheel message is `Unknown` or `Touchpad`, never a panic (the value is only valid during dispatch).
    #[test]
    #[ignore = "native: needs desktop"]
    fn current_wheel_device_outside_a_message_is_a_valid_hint() {
        use pr_app::input::touchpad_gestures::WheelDeviceHint;
        let hint = native::current_wheel_device();
        assert!(hint == WheelDeviceHint::Unknown || hint == WheelDeviceHint::Touchpad);
    }
}
