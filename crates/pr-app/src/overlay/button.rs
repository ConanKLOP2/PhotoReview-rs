//! Port of `PhotoReview.Shell.Win32/Overlay/ButtonElement.cs` (177 lines, baseline d92d6aae; contract `contracts/W8-15c.md`).
//!
//! Toolbar button: state-dependent background (`Button`, `ButtonHover`, `ButtonPressed` of the dark palette), the content centred
//! on one line, a focus border and a click. It never captures the mouse: the host sets `is_hot` / `is_pressed` (W8-14c wires the
//! pointer). `click` replaces the C# `Clicked` event: the caller passes the sink, which receives the element id.

use std::fmt;

use crate::input::primitives::{PointD, RectD, SizeD};
use crate::overlay::contracts::OverlayElement;
use crate::overlay::element::{
    ElementBase, ElementError, ElementNode, OverlayLayoutContext, OverlayThickness, net_max,
};
use crate::overlay::palette;
use crate::overlay::text::style_same;
use crate::services::render::error::RenderError;
use crate::services::render::surface::DrawContext;
use crate::services::render::text::{TextLayout, TextStyle, TextTrimming};
use crate::viewport::render_geometry::ColorF;

/// C# `DarkPalette.Get("Dark.AccentHover")` (`#4A8BE5`): the focus border colour. Not a named function of `overlay::palette`.
const ACCENT_HOVER_ARGB: u32 = 0xFF4A_8BE5;
/// C# `DarkPalette.Get("Dark.TextMuted")` (`#9A9A9A`): the text colour of a disabled button. Not a named function of `overlay::palette`.
const TEXT_MUTED_ARGB: u32 = 0xFF9A_9A9A;

/// `ButtonElement` (C# `public class ButtonElement : OverlayElement`).
///
/// Defaults (C# initialisers and constructor): `content = ""`, `style = TextStyle::default()`, `padding = (8, 4)` (C# `new
/// OverlayThickness(8, 4)`), `border_thickness = 1`, `corner_radius = 2`, `is_enabled = true`, `is_hot = is_pressed = is_focused =
/// is_checked = false`, `foreground = TEXT_BRIGHT`, `background = BUTTON` (toolbar sets `INPUT`), `border_color = BORDER_STRONG`,
/// `fixed_width = fixed_height = NaN` (content size), `tool_tip = ""`, `automation_name = ""`.
pub struct ButtonElement {
    base: ElementBase,
    content: String,
    style: TextStyle,
    /// C# `_layout`: native layout lease, `None` until measured and after `drop_layout`.
    layout: Option<Box<dyn TextLayout>>,
    /// C# `_layoutOwner`: identity of the renderer that made `layout` (`OverlayLayoutContext::text_renderer_id`).
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
        if id.is_empty() {
            return Err(ElementError::EmptyId);
        }
        Ok(Self {
            base: ElementBase::new(id, true),
            content: String::new(),
            style: TextStyle::default(),
            layout: None,
            layout_owner: None,
            padding: OverlayThickness::symmetric(8.0, 4.0),
            border_thickness: 1.0,
            corner_radius: 2.0,
            is_enabled: true,
            is_hot: false,
            is_pressed: false,
            is_focused: false,
            is_checked: false,
            foreground: palette::text_bright(),
            background: palette::button(),
            border_color: palette::border_strong(),
            fixed_width: f64::NAN,
            fixed_height: f64::NAN,
            tool_tip: String::new(),
            automation_name: String::new(),
        })
    }

    /// C# `Content` getter.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// C# `Content` setter. Ordinal equality: an equal string changes nothing; a changed one calls `drop_layout`.
    pub fn set_content(&mut self, value: &str) {
        if self.content != value {
            value.clone_into(&mut self.content);
            self.drop_layout();
        }
    }

    /// C# `Style` getter.
    pub fn style(&self) -> &TextStyle {
        &self.style
    }

    /// C# `Style` setter. `style_same` (NaN-aware) equal: nothing; otherwise `drop_layout`.
    pub fn set_style(&mut self, value: TextStyle) {
        if !style_same(&self.style, &value) {
            self.style = value;
            self.drop_layout();
        }
    }

    /// C# `bool Click()`: calls `on_clicked(self.id())` and returns `true` when the button is enabled, visible and `opacity > 0`
    /// (C# checks `!IsEnabled || !IsVisible || Opacity <= 0f` first and returns `false` without calling the sink). Takes `&self`:
    /// C# `Click` changes no state.
    pub fn click(&self, on_clicked: &mut dyn FnMut(&str)) -> bool {
        if !self.is_enabled || !self.base.is_visible || self.base.opacity() <= 0.0 {
            return false;
        }
        on_clicked(self.base.id());
        true
    }

    /// C# `DropLayout()`: drop the lease, `layout_owner = None`.
    fn drop_layout(&mut self) {
        self.layout = None;
        self.layout_owner = None;
    }

    /// C# `EnsureLayout(renderer, width)`: when there is no layout or the renderer identity changed, drop the old lease, then
    /// `ctx.text.create_layout(content, style, f64::INFINITY, TextTrimming::None)` and record `layout_owner`. The width argument
    /// of C# is unused (the button is one line that never wraps or trims); the call happens even for an empty content.
    fn ensure_layout(&mut self, context: &OverlayLayoutContext<'_>) -> Result<(), RenderError> {
        let owner = context.text_renderer_id;
        if self.layout.is_some() && self.layout_owner == Some(owner) {
            return Ok(());
        }
        self.drop_layout();
        let layout = context
            .text
            .create_layout(&self.content, &self.style, f64::INFINITY, TextTrimming::None)?;
        self.layout = Some(layout);
        self.layout_owner = Some(owner);
        Ok(())
    }
}

impl fmt::Debug for ButtonElement {
    /// Manual: `Box<dyn TextLayout>` has no `Debug`. Prints the id, content, style, the enabled/hot/pressed/focused/checked flags
    /// and whether a layout is held.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ButtonElement")
            .field("id", &self.base.id())
            .field("content", &self.content)
            .field("style", &self.style)
            .field("is_enabled", &self.is_enabled)
            .field("is_hot", &self.is_hot)
            .field("is_pressed", &self.is_pressed)
            .field("is_focused", &self.is_focused)
            .field("is_checked", &self.is_checked)
            .field("has_layout", &self.layout.is_some())
            .finish()
    }
}

impl ElementNode for ButtonElement {
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

    /// C# `MeasureCore(available, context)`: `EnsureLayout` first (its width argument is ignored), then
    /// `width = fixed_width` if not NaN, else `text.width + padding.horizontal + chrome`; `height` likewise with `text.height +
    /// padding.vertical + chrome`, where `chrome = 2 * border_thickness`. The base clamps the width to `MaxWidth`.
    /// A failed `create_layout` keeps no layout and counts the text as zero-sized (the next measure retries, trap T2).
    fn measure_core(&mut self, _available: SizeD, context: &OverlayLayoutContext<'_>) -> SizeD {
        let text = match self.ensure_layout(context) {
            Ok(()) => self.layout.as_ref().map_or(SizeD::default(), |layout| layout.size()),
            Err(_) => SizeD::default(),
        };
        let chrome = 2.0 * self.border_thickness;
        let width = if self.fixed_width.is_nan() {
            text.width + self.padding.horizontal() + chrome
        } else {
            self.fixed_width
        };
        let height = if self.fixed_height.is_nan() {
            text.height + self.padding.vertical() + chrome
        } else {
            self.fixed_height
        };
        SizeD { width, height }
    }

    /// C# `RenderCore(dc)`, in this order:
    /// 1. background: `!is_enabled` -> `background`; `is_pressed || is_checked` -> `BUTTON_PRESSED`; `is_hot` -> `BUTTON_HOVER`;
    ///    else `background`. `corner_radius > 0` -> rounded fill, else plain fill.
    /// 2. border when `border_thickness > 0`: `half = border_thickness / 2`, stroke rect
    ///    `(x + half, y + half, max(0, w - border_thickness), max(0, h - border_thickness))`, colour `ACCENT_HOVER` when
    ///    `is_focused` else `border_color`.
    /// 3. text: nothing when there is no layout or the content is empty. Otherwise the origin is centred in `bounds`,
    ///    clip to `bounds`, draw in `foreground` (enabled) or `TEXT_MUTED` (disabled), clip pop.
    fn render_core(&self, dc: &mut dyn DrawContext) {
        let bounds = self.base.bounds();

        let background = if !self.is_enabled {
            self.background
        } else if self.is_pressed || self.is_checked {
            palette::button_pressed()
        } else if self.is_hot {
            palette::button_hover()
        } else {
            self.background
        };
        if self.corner_radius > 0.0 {
            dc.fill_rounded_rectangle(bounds, self.corner_radius, background);
        } else {
            dc.fill_rectangle(bounds, background);
        }

        if self.border_thickness > 0.0 {
            let half = self.border_thickness / 2.0;
            let stroke = RectD {
                x: bounds.x + half,
                y: bounds.y + half,
                width: net_max(0.0, bounds.width - self.border_thickness),
                height: net_max(0.0, bounds.height - self.border_thickness),
            };
            let colour = if self.is_focused {
                palette::from_argb(ACCENT_HOVER_ARGB)
            } else {
                self.border_color
            };
            dc.draw_rectangle(stroke, colour, self.border_thickness);
        }

        let Some(layout) = self.layout.as_deref() else {
            return;
        };
        if self.content.is_empty() {
            return;
        }
        let size = layout.size();
        let origin = PointD {
            x: bounds.x + (bounds.width - size.width) / 2.0,
            y: bounds.y + (bounds.height - size.height) / 2.0,
        };
        let colour = if self.is_enabled {
            self.foreground
        } else {
            palette::from_argb(TEXT_MUTED_ARGB)
        };
        dc.push_clip(bounds);
        let _ = dc.draw_text(layout, origin, colour);
        let popped = dc.pop_clip();
        debug_assert!(popped.is_ok(), "pop_clip must follow push_clip");
    }

    /// C# `ReleaseResources() => DropLayout()`.
    fn release_resources(&mut self) {
        self.drop_layout();
    }
}
