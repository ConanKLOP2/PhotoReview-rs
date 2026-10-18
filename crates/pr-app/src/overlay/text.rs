//! Port of `PhotoReview.Shell.Win32/Overlay/TextElement.cs` (114 lines, baseline d92d6aae; contract `contracts/W8-15c.md`).
//!
//! `TextBlock`: one string, one style, one colour. `TextTrimming::CharacterEllipsis` cuts with an ellipsis when the text is wider
//! than the offered width. The layout lease is kept until the text, the style, the trimming or the offered width changes, or until
//! `release_resources` runs (C# `Invalidate`). Never reads the clock, the file system or the window.
//!
//! Stub: every body is `todo!()`. Signatures are final (contract "Public API"). The `allow` line goes away with the last `todo!()`
//! (contracts/README.md D11).

#![allow(unused_variables, dead_code, unused_imports)] // stub only: removed by the porter when the bodies are filled

use std::fmt;

use crate::input::primitives::{PointD, SizeD};
use crate::overlay::element::{ElementBase, ElementError, ElementNode, OverlayLayoutContext};
use crate::services::render::error::RenderError;
use crate::services::render::surface::DrawContext;
use crate::services::render::text::{TextLayout, TextStyle, TextTrimming};
use crate::viewport::render_geometry::ColorF;

/// `TextElement` (C# `public class TextElement : OverlayElement`).
///
/// Defaults (C# field initialisers and constructor): `text = ""`, `style = TextStyle::default()` (Segoe UI 12, not bold),
/// `trimming = TextTrimming::None`, `color = palette TEXT_BRIGHT` (C# `DarkPalette.TextBright`), `is_hit_test_visible = false`
/// (C# constructor sets `IsHitTestVisible = false`), no layout (`layout = None`, `layout_owner = None`, `layout_width = NaN`).
pub struct TextElement {
    base: ElementBase,
    text: String,
    style: TextStyle,
    trimming: TextTrimming,
    /// C# `_layout`: the native layout lease, `None` before the first measure and after every invalidation.
    layout: Option<Box<dyn TextLayout>>,
    /// C# `_layoutOwner`: identity of the renderer that created `layout` (see `OverlayLayoutContext::text_renderer_id`).
    layout_owner: Option<u64>,
    /// C# `_layoutWidth`: offered width of the current layout, `NaN` when there is none.
    layout_width: f64,
    /// C# `Color` (`Alpha` straight, `DarkPalette.TextBright` by default).
    pub color: ColorF,
}

impl TextElement {
    /// C# `new TextElement(id)`. `Err(ElementError::EmptyId)` for an empty id (C# `ArgumentException`).
    pub fn new(id: &str) -> Result<Self, ElementError> {
        todo!()
    }

    /// C# `Text` getter.
    pub fn text(&self) -> &str {
        todo!()
    }

    /// C# `Text` setter. Ordinal equality (`string.Equals(_text, value, Ordinal)`): an equal string changes nothing;
    /// a changed one drops the layout at once (C# `Invalidate`).
    pub fn set_text(&mut self, value: &str) {
        todo!()
    }

    /// C# `Style` getter.
    pub fn style(&self) -> &TextStyle {
        todo!()
    }

    /// C# `Style` setter. Equal style (`style_same`, NaN-aware) changes nothing; otherwise `invalidate`.
    pub fn set_style(&mut self, value: TextStyle) {
        todo!()
    }

    /// C# `Trimming` getter.
    pub fn trimming(&self) -> TextTrimming {
        todo!()
    }

    /// C# `Trimming` setter: a different value calls `invalidate`.
    pub fn set_trimming(&mut self, value: TextTrimming) {
        todo!()
    }

    /// C# `internal ITextLayout? Layout` (the layout held after Measure, `None` before it). Used by the parts' tests and the
    /// render path; `pub(crate)` only.
    pub(crate) fn layout(&self) -> Option<&dyn TextLayout> {
        todo!()
    }

    /// C# `Invalidate()`: drop the lease, forget the owner and the width (`layout_width = NaN`).
    fn invalidate(&mut self) {
        todo!()
    }
}

/// C# `TextStyle` record equality (`_style != value`): family ordinal, size with `double.Equals` (NaN equals NaN, `0.0` equals
/// `-0.0`), bold. Shared with `ButtonElement` (`overlay::button`). Not a derived `PartialEq`: the derived one says NaN != NaN.
pub(crate) fn style_same(a: &TextStyle, b: &TextStyle) -> bool {
    todo!()
}

impl fmt::Debug for TextElement {
    /// Manual: `Box<dyn TextLayout>` has no `Debug`. Prints the id, text, style, trimming and whether a layout is held.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl ElementNode for TextElement {
    /// Leaf `FindAt` (inherited from C# `OverlayElement`): `Some(self)` when `hit_test(point)`, else `None`.
    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        todo!()
    }

    /// Leaf `Find`: `Some(self)` when the id matches (ordinal), else `None`.
    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn base(&self) -> &ElementBase {
        todo!()
    }

    fn base_mut(&mut self) -> &mut ElementBase {
        todo!()
    }

    /// C# `MeasureCore(available, context)`. Reuses the layout while the renderer identity, the offered width (IEEE `!=`, so a
    /// NaN width re-measures every time, as C# does) and the text/style/trimming are unchanged; otherwise drops the old lease
    /// and asks `context.text` for a new one with `max_width = available.width`. Returns the layout size (the base clamps it to
    /// `MaxWidth`).
    fn measure_core(&mut self, available: SizeD, context: &OverlayLayoutContext<'_>) -> SizeD {
        todo!()
    }

    /// C# `RenderCore(dc)`: nothing when there is no layout or the text is empty; otherwise `PushClip(Bounds)`, `DrawText` at
    /// `(Bounds.X, Bounds.Y)` with `color`, `PopClip`.
    fn render_core(&self, dc: &mut dyn DrawContext) {
        todo!()
    }

    /// C# `ReleaseResources() => Invalidate()`.
    fn release_resources(&mut self) {
        todo!()
    }
}
