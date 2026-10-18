//! Port of `PhotoReview.Shell.Win32/Overlay/DarkPalette.cs` (WP-17): the dark palette of the Direct2D overlay. The values are
//! the `Dark.*` brushes of `PhotoReview.App.WpfWindows/Themes/DarkPalette.xaml` at baseline `d92d6aae` (the XAML is WPF-only
//! and not ported; its brushes are carried in the W8-15b golden input `overlay-palette.json`). Owned by W8-15b; pure.
//!
//! Colours are straight alpha (C# `#AARRGGBB`): `from_argb` maps each 8-bit channel `c` to `c as f32 / 255.0`.
//! `ColorF` is the Shell.Rendering colour (W8-12a, `pr_app::services::render::ColorF`, `Copy`, four f32 channels).

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use crate::services::render::ColorF;

/// C# `Table`: every `Dark.*` key with its ARGB value, in C# declaration order (35 rows, keys unique).
/// Duplicate keys are a bug (C# `Dictionary.Add` throws in the static constructor); the test `table_keys_are_unique` guards it.
pub const TABLE: &[(&str, u32)] = &[
    ("Dark.Text", 0xFFE5E5E5),
    ("Dark.TextStrong", 0xFFFFFFFF),
    ("Dark.TextMuted", 0xFF9A9A9A),
    ("Dark.Input", 0xFF252525),
    ("Dark.Panel", 0xFF1E1E1E),
    ("Dark.Border", 0xFF3C3C3C),
    ("Dark.BorderStrong", 0xFF5A5A5A),
    ("Dark.Button", 0xFF2D2D2D),
    ("Dark.ButtonHover", 0xFF383838),
    ("Dark.ButtonPressed", 0xFF444444),
    ("Dark.Accent", 0xFF3A7BD5),
    ("Dark.AccentHover", 0xFF4A8BE5),
    ("Dark.Window", 0xFF171717),
    ("Dark.Backdrop", 0xFF181818),
    ("Dark.Canvas", 0xFF101010),
    ("Dark.Tooltip", 0xFF202020),
    ("Dark.Surface", 0xFF2A2A2A),
    ("Dark.RowAlt", 0xFF242424),
    ("Dark.Overlay", 0xB0181818),
    ("Dark.Selection", 0xFF3A5A80),
    ("Dark.SuccessBackground", 0xFF1E3B24),
    ("Dark.ErrorBackground", 0xFF3B1E1E),
    ("Dark.WindowText", 0xFFF5F5F5),
    ("Dark.TextBright", 0xFFEEEEEE),
    ("Dark.TextSoft", 0xFFCFCFCF),
    ("Dark.TextSecondary", 0xFFBBBBBB),
    ("Dark.TextSubtle", 0xFFAAAAAA),
    ("Dark.TextFaint", 0xFF777777),
    ("Dark.Warning", 0xFFFFC857),
    ("Dark.Caution", 0xFFF0B040),
    ("Dark.ErrorText", 0xFFFF9A9A),
    ("Dark.BorderSubtle", 0xFF3A3A3A),
    ("Dark.BorderToolbar", 0xFF454545),
    ("Dark.GridLine", 0xFF333333),
    ("Dark.ScrollTrack", 0xFF161616),
];

/// C# `FromArgb(uint argb)`: straight alpha. `a = (argb >> 24) / 255`, then `r`, `g`, `b` from bits 16, 8, 0. Each channel is
/// `((argb >> shift) & 0xFF) as f32 / 255.0` (one f32 division, bit-exact with C# `int / 255f`).
pub const fn from_argb(argb: u32) -> ColorF {
    todo!()
}

/// C# `Get(key)` with the C# exception turned into `None`: exact, ordinal, case-sensitive match on [`TABLE`]. No trimming, no
/// case folding. C# throws `KeyNotFoundException` for an unknown key; the port returns `None` and the caller decides (the
/// callers in the overlay pass constant keys, so `None` is a programming error there).
pub fn get(key: &str) -> Option<ColorF> {
    todo!()
}

/// C# `Overlay` (`Dark.Overlay`, `#B0181818`).
pub fn overlay() -> ColorF {
    from_argb(0xB018_1818)
}
/// C# `TextBright` (`Dark.TextBright`, `#EEEEEE`).
pub fn text_bright() -> ColorF {
    from_argb(0xFFEE_EEEE)
}
/// C# `TextSecondary` (`Dark.TextSecondary`, `#BBBBBB`).
pub fn text_secondary() -> ColorF {
    from_argb(0xFFBB_BBBB)
}
/// C# `Warning` (`Dark.Warning`, `#FFC857`).
pub fn warning() -> ColorF {
    from_argb(0xFFFF_C857)
}
/// C# `Button` (`Dark.Button`, `#2D2D2D`).
pub fn button() -> ColorF {
    from_argb(0xFF2D_2D2D)
}
/// C# `ButtonHover` (`Dark.ButtonHover`, `#383838`).
pub fn button_hover() -> ColorF {
    from_argb(0xFF38_3838)
}
/// C# `ButtonPressed` (`Dark.ButtonPressed`, `#444444`).
pub fn button_pressed() -> ColorF {
    from_argb(0xFF44_4444)
}
/// C# `Tooltip` (`Dark.Tooltip`, `#202020`).
pub fn tooltip() -> ColorF {
    from_argb(0xFF20_2020)
}
/// C# `BorderStrong` (`Dark.BorderStrong`, `#5A5A5A`).
pub fn border_strong() -> ColorF {
    from_argb(0xFF5A_5A5A)
}
