//! Port of `PhotoReview.Shell.Interop/Graphics/DWrite.cs` (251 lines, WP-13b), baseline `d92d6aae`. Unit W8-11h (audit; thin helpers).
//!
//! `IDWriteFactory`, `IDWriteTextFormat`, `IDWriteTextLayout`, the enums and the metrics structs come from `windows-rs`; the 93 slots
//! and `Reserved` placeholders of the C# file are retired. What stays is the creation recipe of `DWriteTextRenderer.Build` (format,
//! layout) and the two metric reads it makes. Text is UTF-16 (`str::encode_utf16`), enough for Vietnamese. Table: `contracts/W8-11h.md`.
//! The ellipsis trimming recipe is `extras::set_character_ellipsis_trimming` (W8-11f).

#![allow(unused_variables, dead_code)] // W8-11h stub: the porter removes this line when the last `todo!()` is gone.

use windows::Win32::Graphics::DirectWrite::{IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout};

use super::com::GfxError;

/// `DWriteTextRenderer.Unbounded`: the width/height that stands for "no limit" (DirectWrite needs a finite number).
pub const UNBOUNDED: f32 = 1_000_000.0;

/// `DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED)` (C# `DWrite.CreateFactory()`; ISOLATED is not offered).
pub fn create_factory() -> Result<IDWriteFactory, GfxError> {
    todo!("W8-11h")
}

/// `factory.CreateTextFormat(family, None /* system collection */, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
/// size_dip, locale)` with `weight = DWRITE_FONT_WEIGHT_BOLD (700)` when `bold`, else `DWRITE_FONT_WEIGHT_REGULAR (400)`.
/// `family` and `locale` are converted to NUL-terminated UTF-16 for the call only. The caller has validated `size_dip`
/// (finite, positive; `TextRenderer::create_layout` rejects the rest, W8-12i).
pub fn create_text_format(
    factory: &IDWriteFactory,
    family: &str,
    bold: bool,
    size_dip: f32,
    locale: &str,
) -> Result<IDWriteTextFormat, GfxError> {
    todo!("W8-11h")
}

/// `factory.CreateTextLayout(text as UTF-16, format, max_width, UNBOUNDED)` (C# passes `key.Text`, its UTF-16 length,
/// `key.MaxWidth`, `Unbounded`). `max_width` is already normalised (`DWriteTextRenderer.NormalizeWidth`, W8-12i).
pub fn create_text_layout(
    factory: &IDWriteFactory,
    text: &str,
    format: &IDWriteTextFormat,
    max_width: f32,
) -> Result<IDWriteTextLayout, GfxError> {
    todo!("W8-11h")
}

/// The three numbers `DWriteTextRenderer.Build` reads from `IDWriteTextLayout::GetMetrics`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutMetrics {
    /// `DWRITE_TEXT_METRICS::width` (the ink-box width of the laid-out text, without trailing whitespace).
    pub width: f32,
    /// `DWRITE_TEXT_METRICS::height`.
    pub height: f32,
    /// `DWRITE_TEXT_METRICS::lineCount`.
    pub line_count: u32,
}

/// `layout.GetMetrics()` reduced to [`LayoutMetrics`].
pub fn layout_metrics(layout: &IDWriteTextLayout) -> Result<LayoutMetrics, GfxError> {
    todo!("W8-11h")
}

/// C# `GetLineMetrics(&line, 1, ..)` then `line.IsTrimmed != 0`: whether the FIRST line was cut by the trimming options. A failing
/// call is `false` (C# `trimmed = hr >= 0 && line.IsTrimmed != 0`), not an error.
#[must_use]
pub fn first_line_is_trimmed(layout: &IDWriteTextLayout) -> bool {
    todo!("W8-11h")
}
