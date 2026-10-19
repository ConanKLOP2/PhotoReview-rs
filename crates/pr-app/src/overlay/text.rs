//! Port of `PhotoReview.Shell.Win32/Overlay/TextElement.cs` (114 lines, baseline d92d6aae; contract `contracts/W8-15c.md`).
//!
//! `TextBlock`: one string, one style, one colour. `TextTrimming::CharacterEllipsis` cuts with an ellipsis when the text is wider
//! than the offered width. The layout lease is kept until the text, the style, the trimming or the offered width changes, or until
//! `release_resources` runs (C# `Invalidate`). Never reads the clock, the file system or the window.

use std::fmt;

use crate::input::primitives::{PointD, SizeD, f64_same};
use crate::overlay::contracts::OverlayElement;
use crate::overlay::element::{ElementBase, ElementError, ElementNode, OverlayLayoutContext};
use crate::overlay::palette;
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
    /// C# `_layoutOwner`: identity of the renderer that created `layout` (`OverlayLayoutContext::text_renderer_id`).
    layout_owner: Option<u64>,
    /// C# `_layoutWidth`: offered width of the current layout, `NaN` when there is none.
    layout_width: f64,
    /// C# `Color` (`Alpha` straight, `DarkPalette.TextBright` by default).
    pub color: ColorF,
}

impl TextElement {
    /// C# `new TextElement(id)`. `Err(ElementError::EmptyId)` for an empty id (C# `ArgumentException`).
    pub fn new(id: &str) -> Result<Self, ElementError> {
        if id.is_empty() {
            return Err(ElementError::EmptyId);
        }
        Ok(Self {
            base: ElementBase::new(id, false),
            text: String::new(),
            style: TextStyle::default(),
            trimming: TextTrimming::None,
            layout: None,
            layout_owner: None,
            layout_width: f64::NAN,
            color: palette::text_bright(),
        })
    }

    /// C# `Text` getter.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// C# `Text` setter. Ordinal equality (`string.Equals(_text, value, Ordinal)`): an equal string changes nothing;
    /// a changed one drops the layout at once (C# `Invalidate`).
    pub fn set_text(&mut self, value: &str) {
        if self.text != value {
            value.clone_into(&mut self.text);
            self.invalidate();
        }
    }

    /// C# `Style` getter.
    pub fn style(&self) -> &TextStyle {
        &self.style
    }

    /// C# `Style` setter. Equal style (`style_same`, NaN-aware) changes nothing; otherwise `invalidate`.
    pub fn set_style(&mut self, value: TextStyle) {
        if !style_same(&self.style, &value) {
            self.style = value;
            self.invalidate();
        }
    }

    /// C# `Trimming` getter.
    pub fn trimming(&self) -> TextTrimming {
        self.trimming
    }

    /// C# `Trimming` setter: a different value calls `invalidate`.
    pub fn set_trimming(&mut self, value: TextTrimming) {
        if self.trimming != value {
            self.trimming = value;
            self.invalidate();
        }
    }

    /// C# `internal ITextLayout? Layout` (the layout held after Measure, `None` before it). Used by the parts' tests and the
    /// render path; `pub(crate)` only.
    #[allow(dead_code)] // C# `Layout` accessor; its first caller (parts tests, render path) lands with W8-15d/15e.
    pub(crate) fn layout(&self) -> Option<&dyn TextLayout> {
        self.layout.as_deref()
    }

    /// C# `Invalidate()`: drop the lease, forget the owner and the width (`layout_width = NaN`).
    fn invalidate(&mut self) {
        self.layout = None;
        self.layout_owner = None;
        self.layout_width = f64::NAN;
    }
}

/// C# `TextStyle` record equality (`_style != value`): family ordinal, size with `double.Equals` (NaN equals NaN, `0.0` equals
/// `-0.0`), bold. Shared with `ButtonElement` (`overlay::button`). Not a derived `PartialEq`: the derived one says NaN != NaN.
pub(crate) fn style_same(a: &TextStyle, b: &TextStyle) -> bool {
    a.font_family == b.font_family && f64_same(a.font_size_dip, b.font_size_dip) && a.bold == b.bold
}

impl fmt::Debug for TextElement {
    /// Manual: `Box<dyn TextLayout>` has no `Debug`. Prints the id, text, style, trimming and whether a layout is held.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextElement")
            .field("id", &self.base.id())
            .field("text", &self.text)
            .field("style", &self.style)
            .field("trimming", &self.trimming)
            .field("has_layout", &self.layout.is_some())
            .finish()
    }
}

impl ElementNode for TextElement {
    /// Leaf `FindAt` (inherited from C# `OverlayElement`): `Some(self)` when `hit_test(point)`, else `None`.
    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        if self.hit_test(point) { Some(self) } else { None }
    }

    /// Leaf `Find`: `Some(self)` when the id matches (ordinal), else `None`.
    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        if self.base.id() == id { Some(self) } else { None }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn base(&self) -> &ElementBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut ElementBase {
        &mut self.base
    }

    /// C# `MeasureCore(available, context)`. Reuses the layout while the renderer identity, the offered width (IEEE `!=`, so a
    /// NaN width re-measures every time, as C# does) and the text/style/trimming are unchanged; otherwise drops the old lease
    /// and asks `context.text` for a new one with `max_width = available.width`. Returns the layout size (the base clamps it to
    /// `MaxWidth`). A failed `create_layout` leaves no layout (trap T2); the size is then zero and the next measure retries.
    fn measure_core(&mut self, available: SizeD, context: &OverlayLayoutContext<'_>) -> SizeD {
        let owner = context.text_renderer_id;
        #[allow(clippy::float_cmp)]
        // trap T5: IEEE `!=` on purpose; a NaN width must re-measure every time (C# `!=` on double).
        let width_unchanged = self.layout_width == available.width;
        let reusable = self.layout.is_some() && self.layout_owner == Some(owner) && width_unchanged;
        if !reusable {
            // C# disposes the old lease before `CreateLayout`; a failure keeps no lease (trap T2).
            self.layout = None;
            self.layout_owner = None;
            match context
                .text
                .create_layout(&self.text, &self.style, available.width, self.trimming)
            {
                Ok(layout) => {
                    self.layout = Some(layout);
                    self.layout_owner = Some(owner);
                    self.layout_width = available.width;
                }
                Err(_) => {
                    self.layout_width = f64::NAN;
                }
            }
        }
        self.layout.as_ref().map_or(SizeD::default(), |layout| layout.size())
    }

    /// C# `RenderCore(dc)`: nothing when there is no layout or the text is empty; otherwise `PushClip(Bounds)`, `DrawText` at
    /// `(Bounds.X, Bounds.Y)` with `color`, `PopClip`.
    fn render_core(&self, dc: &mut dyn DrawContext) {
        let Some(layout) = self.layout.as_deref() else {
            return;
        };
        if self.text.is_empty() {
            return;
        }
        let bounds = self.base.bounds();
        dc.push_clip(bounds);
        let _ = dc.draw_text(
            layout,
            PointD {
                x: bounds.x,
                y: bounds.y,
            },
            self.color,
        );
        let popped = dc.pop_clip();
        debug_assert!(popped.is_ok(), "pop_clip must follow push_clip");
    }

    /// C# `ReleaseResources() => Invalidate()`.
    fn release_resources(&mut self) {
        self.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::render::error::RenderError;
    use crate::services::render::text::TextRenderer;

    struct Layout(String);
    impl TextLayout for Layout {
        fn size(&self) -> SizeD {
            SizeD {
                width: 7.0,
                height: 9.0,
            }
        }
        fn text(&self) -> &str {
            &self.0
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    struct Renderer;
    impl TextRenderer for Renderer {
        fn create_layout(
            &self,
            text: &str,
            _style: &TextStyle,
            _max_width: f64,
            _trimming: TextTrimming,
        ) -> Result<Box<dyn TextLayout>, RenderError> {
            Ok(Box::new(Layout(text.to_owned())))
        }
    }

    #[test]
    fn layout_is_none_before_measure_and_the_held_lease_after() {
        let renderer = Renderer;
        let context = OverlayLayoutContext::new(
            SizeD {
                width: 800.0,
                height: 600.0,
            },
            1.0,
            &renderer,
        );
        let mut element = TextElement::new("t").expect("non-empty id");
        element.set_text("abc");
        assert!(element.layout().is_none());
        element.measure(
            SizeD {
                width: 100.0,
                height: 100.0,
            },
            &context,
        );
        assert_eq!(element.layout().map(TextLayout::text), Some("abc"));
        element.release_resources();
        assert!(element.layout().is_none());
    }
}
