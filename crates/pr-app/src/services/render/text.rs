//! Port of `PhotoReview.Shell.Rendering/Contracts/TextContracts.cs` (26 lines, C-11 text part, baseline d92d6aae): the DirectWrite
//! text contract. The real renderer and the layout cache are W8-12i (`pr_platform_win::render::text`).
//!
//! No `windows` types (D-W8-3).

use std::any::Any;

use super::error::RenderError;
use crate::input::primitives::{SizeD, f64_same};

/// `TextTrimming` (C# values 0 and 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextTrimming {
    None = 0,
    CharacterEllipsis = 1,
}

/// `TextStyle(string FontFamily = "Segoe UI", double FontSizeDip = 12, bool Bold = false)` (C# record: value equality).
/// No `Eq`/`Hash` (an `f64` field); the layout cache of W8-12i builds its key from the `f32` size like C# (`TextLayoutKey`).
#[derive(Debug, Clone)]
pub struct TextStyle {
    pub font_family: String,
    pub font_size_dip: f64,
    pub bold: bool,
}

impl PartialEq for TextStyle {
    /// C# record equality: `font_family` ordinal (`string.Equals`, case-sensitive), `font_size_dip` with
    /// `EqualityComparer<double>.Default` (`input::primitives::f64_same`, `pub(crate)`, import it: NaN equals NaN, `0.0` equals `-0.0`), `bold`.
    fn eq(&self, other: &Self) -> bool {
        self.font_family == other.font_family
            && f64_same(self.font_size_dip, other.font_size_dip)
            && self.bold == other.bold
    }
}

impl Default for TextStyle {
    /// `font_family = "Segoe UI"`, `font_size_dip = 12.0`, `bold = false`.
    fn default() -> Self {
        Self {
            font_family: "Segoe UI".to_string(),
            font_size_dip: 12.0,
            bold: false,
        }
    }
}

/// `ITextLayout : IDisposable`. A layout is a lease on a cached native layout; dropping it returns the lease (C# `Dispose`).
pub trait TextLayout {
    /// `Size`: measured width and height in DIP.
    fn size(&self) -> SizeD;

    /// `Text`: the text the layout was created from.
    fn text(&self) -> &str;

    /// Upcast for the draw context's concrete-type check (C# `layout is INativeTextLayout`).
    fn as_any(&self) -> &dyn Any;
}

/// `ITextRenderer`: DirectWrite; layouts are cached by (text, style, max width, trimming). UI thread, `&self` (the cache is behind
/// interior mutability, as in the C# class which locks internally only for lease release from other threads).
pub trait TextRenderer {
    /// `CreateLayout(string text, TextStyle style, double maxWidth, TextTrimming trimming)`.
    /// `Err(InvalidFontSize { value_bits })` when `style.font_size_dip` is not a finite positive number;
    /// `Err(Disposed { object: "PhotoReview.Shell.Rendering.DWriteTextRenderer" })` after the renderer was dropped/disposed.
    fn create_layout(
        &self,
        text: &str,
        style: &TextStyle,
        max_width: f64,
        trimming: TextTrimming,
    ) -> Result<Box<dyn TextLayout>, RenderError>;
}
