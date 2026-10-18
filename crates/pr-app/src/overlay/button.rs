//! Port of `PhotoReview.Shell.Win32/Overlay/ButtonElement.cs` (177 lines, baseline d92d6aae; contract `contracts/W8-15c.md`).
//!
//! Toolbar button: state-dependent background (`Button`, `ButtonHover`, `ButtonPressed` of the dark palette), the content centred
//! on one line, a focus border and a click. It never captures the mouse: the host sets `is_hot` / `is_pressed` (W8-14c wires the
//! pointer). `click` replaces the C# `Clicked` event: the caller passes the sink, which receives the element id.
//!
//! Stub: every body is `todo!()`. Signatures are final (contract "Public API"). The `allow` line goes away with the last `todo!()`
//! (contracts/README.md D11).

#![allow(unused_variables, dead_code, unused_imports)] // stub only: removed by the porter when the bodies are filled

use std::fmt;

use crate::input::primitives::{PointD, SizeD};
use crate::overlay::element::{ElementBase, ElementError, ElementNode, OverlayLayoutContext, OverlayThickness};
use crate::overlay::text::style_same;
use crate::services::render::error::RenderError;
use crate::services::render::surface::DrawContext;
use crate::services::render::text::{TextLayout, TextStyle};
use crate::viewport::render_geometry::ColorF;

/// `ButtonElement` (C# `public class ButtonElement : OverlayElement`).
///
/// Defaults (C# initialisers and constructor): `content = ""`, `style = TextStyle::default()`, `padding = (8, 4)` (C# `new
/// OverlayThickness(8, 4)`), `border_thickness = 1`, `corner_radius = 2`, `is_enabled = true`, `is_hot = is_pressed = is_focused =
/// is_checked = false`, `foreground = TEXT_BRIGHT`, `background = BUTTON` (toolbar sets `INPUT`), `border_color = BORDER_STRONG`,
/// `fixed_width = fixed_height = NaN` (content size), `tool_tip = ""`, `automation_name = ""`. Palette names: `overlay::palette`
/// (W8-15b); `ACCENT_HOVER` is C# `DarkPalette.Get("Dark.AccentHover")`, `TEXT_MUTED` is `Get("Dark.TextMuted")`.
pub struct ButtonElement {
    base: ElementBase,
    content: String,
    style: TextStyle,
    /// C# `_layout`: native layout lease, `None` until measured and after `drop_layout`.
    layout: Option<Box<dyn TextLayout>>,
    /// C# `_layoutOwner`: id of the renderer that made `layout` (`OverlayLayoutContext::text_renderer_id`).
    layout_owner: Option<u64>,
    /// C# `Padding` (`OverlayThickness`), default `(8, 4)`.
    pub padding: OverlayThickness,
    /// C# `BorderThickness`, DIP, default `1`. Counted in the size (`chrome = 2 * border_thickness`).
    pub border_thickness: f64,
    /// C# `CornerRadius`, default `2`. `> 0` fills a rounded rectangle, otherwise a plain one.
    pub corner_radius: f64,
    /// C# `IsEnabled`, default `true`.
    pub is_enabled: bool,
    /// C# `IsHot` (pointer over the button, set by the host).
    pub is_hot: bool,
    /// C# `IsPressed` (primary button down on the button, set by the host).
    pub is_pressed: bool,
    /// C# `IsFocused` (keyboard focus: the border uses `ACCENT_HOVER`).
    pub is_focused: bool,
    /// C# `IsChecked` (ToggleButton on, e.g. the Tools button while its menu is open): the background is `BUTTON_PRESSED`.
    pub is_checked: bool,
    /// C# `Foreground`, default `TEXT_BRIGHT`.
    pub foreground: ColorF,
    /// C# `Background`, default `BUTTON`.
    pub background: ColorF,
    /// C# `BorderColor`, default `BORDER_STRONG`.
    pub border_color: ColorF,
    /// C# `FixedWidth` (XAML `Width`, outer size including the border), `NaN` = content size.
    pub fixed_width: f64,
    /// C# `FixedHeight` (XAML `Height`), `NaN` = content size.
    pub fixed_height: f64,
    /// C# `ToolTip` text, `""` = no tooltip (consumed by W8-15g).
    pub tool_tip: String,
    /// C# `AutomationProperties.Name`, `""` = unset (consumed by the UIA package).
    pub automation_name: String,
}

impl ButtonElement {
    /// C# `new ButtonElement(id)`: sets the defaults above. `Err(ElementError::EmptyId)` for an empty id (C# `ArgumentException`).
    pub fn new(id: &str) -> Result<Self, ElementError> {
        todo!()
    }

    /// C# `Content` getter.
    pub fn content(&self) -> &str {
        todo!()
    }

    /// C# `Content` setter. Ordinal equality: an equal string changes nothing; a changed one calls `drop_layout`.
    pub fn set_content(&mut self, value: &str) {
        todo!()
    }

    /// C# `Style` getter.
    pub fn style(&self) -> &TextStyle {
        todo!()
    }

    /// C# `Style` setter. `style_same` (NaN-aware) equal: nothing; otherwise `drop_layout`.
    pub fn set_style(&mut self, value: TextStyle) {
        todo!()
    }

    /// C# `bool Click()`: calls `on_clicked(self.id())` and returns `true` when the button is enabled, visible and `opacity > 0`
    /// (C# checks `!IsEnabled || !IsVisible || Opacity <= 0f` first and returns `false` without calling the sink). Takes `&self`:
    /// C# `Click` changes no state.
    pub fn click(&self, on_clicked: &mut dyn FnMut(&str)) -> bool {
        todo!()
    }

    /// C# `DropLayout()`: drop the lease, `layout_owner = None`.
    fn drop_layout(&mut self) {
        todo!()
    }

    /// C# `EnsureLayout(renderer, width)`: when there is no layout or the renderer identity changed, drop the old lease, then
    /// `ctx.text.create_layout(content, style, f64::INFINITY, TextTrimming::None)` and record `layout_owner`. The width argument
    /// of C# is unused (the button is one line that never wraps or trims); the call happens even for an empty content.
    fn ensure_layout(&mut self, context: &OverlayLayoutContext<'_>) -> Result<(), RenderError> {
        todo!()
    }
}

impl fmt::Debug for ButtonElement {
    /// Manual: `Box<dyn TextLayout>` has no `Debug`. Prints the id, content, style, the enabled/hot/pressed/focused/checked flags
    /// and whether a layout is held.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl ElementNode for ButtonElement {
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

    /// C# `MeasureCore(available, context)`: `EnsureLayout` first (its width argument is ignored), then
    /// `width = fixed_width` if not NaN, else `text.width + padding.horizontal + chrome`; `height` likewise with `text.height +
    /// padding.vertical + chrome`, where `chrome = 2 * border_thickness`. The base clamps the width to `MaxWidth`.
    fn measure_core(&mut self, available: SizeD, context: &OverlayLayoutContext<'_>) -> SizeD {
        todo!()
    }

    /// C# `RenderCore(dc)`, in this order:
    /// 1. background: `!is_enabled` -> `background`; `is_pressed || is_checked` -> `BUTTON_PRESSED`; `is_hot` -> `BUTTON_HOVER`;
    ///    else `background`. `corner_radius > 0` -> `fill_rounded_rectangle(bounds, corner_radius, c)`, else `fill_rectangle`.
    /// 2. border when `border_thickness > 0`: `half = border_thickness / 2`, stroke rect
    ///    `(x + half, y + half, max(0, w - border_thickness), max(0, h - border_thickness))`, colour `ACCENT_HOVER` when
    ///    `is_focused` else `border_color`, `draw_rectangle(stroke, colour, border_thickness)`.
    /// 3. text: nothing when there is no layout or the content is empty. Otherwise the text origin is centred in `bounds`
    ///    (`x = bounds.x + (bounds.w - size.w) / 2`, `y` likewise), `push_clip(bounds)`, `draw_text(layout, origin,
    ///    colour)` with colour `foreground` when enabled else `TEXT_MUTED`, `pop_clip`.
    fn render_core(&self, dc: &mut dyn DrawContext) {
        todo!()
    }

    /// C# `ReleaseResources() => DropLayout()`.
    fn release_resources(&mut self) {
        todo!()
    }
}
