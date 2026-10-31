//! Independent inline probes for InputNative / KeyboardMessageHandler (C# baseline 7eaee2a2).
#![cfg(windows)]

use pr_app::input::primitives::KeyModifiers;
use pr_platform_win::shell_input::handlers::KeyboardMessageHandler;
use pr_platform_win::shell_input::native::{VK_LSHIFT, VK_RSHIFT, VK_SHIFT, modifiers_from_key_states};

// C#: InputNative.ReadModifiers / InputNativeTests. native.rs:61-71:
// default return, < comparisons, | -> &, || -> &&. Zero and toggled-only states are up.
#[test]
fn modifiers_distinguish_all_pressed_combinations_from_zero_and_toggle_states() {
    let flags = [
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SHIFT,
        KeyModifiers::WINDOWS,
        KeyModifiers::WINDOWS,
    ];
    for pressed in 0_u32..32 {
        let expected = flags
            .iter()
            .enumerate()
            .fold(KeyModifiers::NONE, |result, (bit, flag)| {
                if pressed & (1 << bit) == 0 {
                    result
                } else {
                    result | *flag
                }
            });
        for up in [0, 1, i16::MAX] {
            for down in [-1, i16::MIN] {
                let states: [i16; 5] = std::array::from_fn(|bit| if pressed & (1 << bit) == 0 { up } else { down });
                assert_eq!(
                    modifiers_from_key_states(states[0], states[1], states[2], states[3], states[4]),
                    expected,
                    "pressed={pressed:05b}, states={states:?}"
                );
            }
        }
    }
}

// C#: KeyboardMessageHandler.ResolveVirtualKey. handlers.rs:85-89:
// constant returns, != -> ==, & -> |/^, >> -> <<, == -> !=.
#[test]
fn resolve_shift_ignores_every_bit_outside_the_scan_code_byte() {
    for scan in 0_isize..=255 {
        let expected = if scan == 0x36 { VK_RSHIFT } else { VK_LSHIFT };
        for noise in [0, 0xffff, 0x4100_ffff, -0x100_0000] {
            let packed = (scan << 16) | noise;
            assert_eq!(KeyboardMessageHandler::resolve_virtual_key(VK_SHIFT, packed), expected);
            for key in [0, 1, -1, 0x41, VK_LSHIFT, VK_RSHIFT] {
                assert_eq!(KeyboardMessageHandler::resolve_virtual_key(key, packed), key);
            }
        }
    }
}
