//! Independent pure-input mutation probes for CX-INPUTMUT.
//! C# oracle: InputNativeTests.SignedLowWord_ReadsAnInt16,
//! SignedHighWord_ReadsAnInt16, LowWord_IsUnsigned, and InputNative's key bit helpers.
#![cfg(windows)]

use pr_platform_win::shell_input::native::{
    is_extended_key, is_key_repeat, low_word, signed_high_word, signed_low_word,
};

// Mutants: shell_input/native.rs:125:33 and :130:41 (& -> | / ^),
// :130:34 (>> -> <<), :125/:130/:135 (constant return), :135:23 (& -> | / ^).
#[test]
fn signed_words_preserve_each_boundary_independently_of_the_other_word_and_upper_bits() {
    let words = [
        (0_u16, 0),
        (1, 1),
        (0x1234, 4660),
        (0x7fff, 32767),
        (0x8000, -32768),
        (0xffff, -1),
    ];
    for (low, expected_low) in words {
        for (high, expected_high) in words {
            for upper in [0_u64, 0x1234_5678_0000_0000, 0xffff_ffff_0000_0000] {
                let packed = (upper | (u64::from(high) << 16) | u64::from(low)) as isize;
                assert_eq!(expected_low, signed_low_word(packed), "low of {packed:#x}");
                assert_eq!(expected_high, signed_high_word(packed), "high of {packed:#x}");
                assert_eq!(i32::from(low), low_word(packed), "unsigned low of {packed:#x}");
            }
        }
    }
}

// Mutants: shell_input/native.rs:155 and :160 (true/false return, != -> ==,
// & -> | / ^). The unrelated sign, scan-code and repeat-count bits must not win.
#[test]
fn extended_and_repeat_flags_are_independent_of_each_other_and_unrelated_message_bits() {
    for (flags, extended, repeat) in [
        (0_u32, false, false),
        (0x0100_0000, true, false),
        (0x4000_0000, false, true),
        (0x4100_0000, true, true),
    ] {
        for noise in [0_u32, 0x0000_ffff, 0x0036_0000, 0x8000_0000, 0xbeff_ffff] {
            let packed = (flags | noise) as i32 as isize;
            assert_eq!(extended, is_extended_key(packed), "extended of {packed:#x}");
            assert_eq!(repeat, is_key_repeat(packed), "repeat of {packed:#x}");
        }
    }
}
