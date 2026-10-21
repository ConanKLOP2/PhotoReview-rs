//! Port of `PhotoReview.Shell.Win32/Menus/MenuOwnerDraw.cs` (59 lines, baseline `d92d6aae`). Unit W8-14e. Contract: `contracts/W8-14e.md`.
//!
//! The pure half of the owner-draw menu: colours by item state and sizes by DPI. The menu paints itself with the dark palette of the app
//! (`pr_app::overlay::palette`), so it does not depend on the undocumented uxtheme ordinals 133/135/136 (on Windows 11 build 26300 they give light text
//! on a white background). The GDI painting is `owner_draw` (W8-14g).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

/// C# `OdsSelected` (`DRAWITEMSTRUCT.itemState`).
pub const ODS_SELECTED: u32 = 0x0001;
/// C# `OdsGrayed`.
pub const ODS_GRAYED: u32 = 0x0002;
/// C# `OdsDisabled`.
pub const ODS_DISABLED: u32 = 0x0004;
/// C# `OdsChecked`.
pub const ODS_CHECKED: u32 = 0x0008;

/// C# `Scale(px96, dpi)`: `max(1, round_ties_even(px96 * dpi / 96))`; `dpi == 0` counts as 96.
pub fn scale(px96: i32, dpi: u32) -> i32 {
    todo!()
}

/// C# `ItemHeight(textHeight, dpi)`: `max(text_height + scale(10), scale(28))` (a touch-friendly floor).
pub fn item_height(text_height: i32, dpi: u32) -> i32 {
    todo!()
}

/// C# `SeparatorHeight(dpi)`: `scale(9)`.
pub fn separator_height(dpi: u32) -> i32 {
    todo!()
}

/// C# `ItemWidth(textWidth, shortcutWidth, dpi)`: `scale(32) + text + (shortcut > 0 ? scale(32) + shortcut : 0) + scale(32)`.
pub fn item_width(text_width: i32, shortcut_width: i32, dpi: u32) -> i32 {
    todo!()
}

/// C# `LeftGutter(dpi)`: `scale(32)` (room for the check mark).
pub fn left_gutter(dpi: u32) -> i32 {
    todo!()
}

/// C# `RightPad(dpi)`: `scale(16)`.
pub fn right_pad(dpi: u32) -> i32 {
    todo!()
}

/// C# `ColorRef(paletteKey)`: the `COLORREF` (`0x00BBGGRR`) of a `Dark.*` palette key; each channel is `round_ties_even(f32 channel * 255f)` clamped to 0..=255.
/// An unknown key gives `0` (the keys are constants; the C# `Get` threw).
pub fn color_ref(palette_key: &str) -> u32 {
    todo!()
}

/// C# `Background` (`Dark.Panel`): also the menu's `hbrBack`, so borders and padding match.
pub fn background() -> u32 {
    todo!()
}

/// C# `Separator` (`Dark.Border`).
pub fn separator() -> u32 {
    todo!()
}

/// C# `ItemBackground(state)`: `Dark.ButtonPressed` while `ODS_SELECTED`, else [`background`].
pub fn item_background(state: u32) -> u32 {
    todo!()
}

/// C# `ItemText(state)`: `Dark.TextFaint` while `ODS_GRAYED` or `ODS_DISABLED`, else `Dark.Text`.
pub fn item_text(state: u32) -> u32 {
    todo!()
}
