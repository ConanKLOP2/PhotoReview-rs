//! Port of `PhotoReview.Shell.Win32/Overlay/ToolbarPanel.cs` (WP-23): the top-left toolbar (open folder, settings,
//! fit, tools).
//!
//! Baseline `d92d6aae`, 117 lines. Owned by W8-15e (contract `contracts/W8-15e.md`). Test kind F.
//!
//! Composition, not inheritance (contract section "Tree shape"): the struct owns the four buttons as typed fields and
//! implements `ElementNode`; the `PanelElement` (W8-15d) carries the box style only. `buttons()` is the Tab order.
//!
//! Traps (contract section "Traps"): T1 the Tools button is 34 x 32 fixed and its padding is zero, so the toolbar
//! height is 40 (32 + 4 + 4); T2 the open-folder button has no left margin, the other three have margin left 5;
//! T3 `move_focus` wraps to "no button" (`false`) past either end, it never wraps around; T4 `focused_index` and each
//! button's `is_focused` are kept in step on every focus change (C# `SetFocus`).

use pr_l10n::localizer::Localizer;

use crate::input::primitives::RectD;
use crate::input::primitives::{PointD, SizeD};
use crate::overlay::button::ButtonElement;
use crate::overlay::contracts::OverlayElement;
use crate::overlay::contracts::OverlayLayoutContext;
use crate::overlay::element::{
    ElementBase, ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment,
};
use crate::overlay::panel::PanelElement;
use crate::services::render::DrawContext;
use crate::services::render::text::TextStyle;

/// C# `ToolbarPanel.PanelId`.
pub const PANEL_ID: &str = "ToolbarPanel";
/// C# `ToolbarPanel.OpenFolderId`.
pub const OPEN_FOLDER_ID: &str = "OpenFolderButton";
/// C# `ToolbarPanel.SettingsId`.
pub const SETTINGS_ID: &str = "SettingsButton";
/// C# `ToolbarPanel.FitId`.
pub const FIT_ID: &str = "FitButton";
/// C# `ToolbarPanel.ToolsId`.
pub const TOOLS_ID: &str = "ToolsButton";

/// Glyph of the open-folder button: C# `"\U0001F4C2"` (U+1F4C2, open file folder).
pub const OPEN_FOLDER_GLYPH: &str = "\u{1F4C2}";
/// Glyph of the settings button: C# `"⚙"` (U+2699, no variation selector).
pub const SETTINGS_GLYPH: &str = "\u{2699}";
/// Glyph of the tools button: C# `"⋮"` (U+22EE, vertical ellipsis).
pub const TOOLS_GLYPH: &str = "\u{22EE}";

/// C# `ToolbarPanel : PanelElement`. Style: `HorizontalAlignment = Left`, `VerticalAlignment = Top`, margin 8,
/// padding (6, 4), corner radius 4, orientation `Horizontal`. Background from `PanelElement` (`Dark.Overlay`).
#[derive(Debug)]
pub struct ToolbarPanel {
    /// Box style and base state. Holds no children (the four buttons below are the children).
    panel: PanelElement,
    /// C# `OpenFolder`: content `📂` (glyph above), size 16, padding `(8, 2)`, margin zero. Localised tooltip and
    /// automation name (`MAIN_TOOLBAR_OPEN_FOLDER_*`).
    open_folder: ButtonElement,
    /// C# `Settings`: content `⚙`, size 12, padding `(8, 4)`, margin `(5, 0, 0, 0)`. Localised tooltip and automation
    /// name (`MAIN_TOOLBAR_SETTINGS_*`).
    settings: ButtonElement,
    /// C# `Fit`: content from `MAIN_TOOLBAR_FIT` (set by `apply_localization`, empty before), size 12, padding `(8, 4)`,
    /// margin `(5, 0, 0, 0)`. Localised content, tooltip and automation name (`MAIN_TOOLBAR_FIT*`).
    fit: ButtonElement,
    /// C# `Tools`: content `⋮`, size 18, padding zero, margin `(5, 0, 0, 0)`, `FixedWidth = 34`, `FixedHeight = 32`.
    /// Localised tooltip and automation name (`MAIN_TOOLBAR_TOOLS_*`).
    tools: ButtonElement,
    /// C# `_focusedIndex`: index into `buttons()`, `None` = no button holds the keyboard (C# `-1`).
    focused_index: Option<usize>,
}

impl ToolbarPanel {
    /// C# constructor `ToolbarPanel()`: builds the four buttons (styles above, background `Dark.Input`, border
    /// `Dark.BorderToolbar`, corner radius 0, `Style = (Segoe UI, size)`), adds them in Tab order, then
    /// `ApplyLocalization()` with `texts`.
    pub fn new(texts: &Localizer) -> Self {
        let mut panel = PanelElement::new(PANEL_ID);
        panel.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
        panel.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
        panel.base_mut().margin = OverlayThickness::uniform(8.0);
        panel.padding = OverlayThickness::symmetric(6.0, 4.0);
        panel.corner_radius = 4.0;
        panel.orientation = crate::overlay::panel::OverlayOrientation::Horizontal;
        let mut this = Self {
            panel,
            open_folder: create_button(
                OPEN_FOLDER_ID,
                OPEN_FOLDER_GLYPH,
                16.0,
                OverlayThickness::symmetric(8.0, 2.0),
                OverlayThickness::ZERO,
            ),
            settings: create_button(
                SETTINGS_ID,
                SETTINGS_GLYPH,
                12.0,
                OverlayThickness::symmetric(8.0, 4.0),
                OverlayThickness::new(5.0, 0.0, 0.0, 0.0),
            ),
            fit: create_button(
                FIT_ID,
                "",
                12.0,
                OverlayThickness::symmetric(8.0, 4.0),
                OverlayThickness::new(5.0, 0.0, 0.0, 0.0),
            ),
            tools: create_button(
                TOOLS_ID,
                TOOLS_GLYPH,
                18.0,
                OverlayThickness::ZERO,
                OverlayThickness::new(5.0, 0.0, 0.0, 0.0),
            ),
            focused_index: None,
        };
        this.tools.fixed_width = 34.0;
        this.tools.fixed_height = 32.0;
        this.apply_localization(texts);
        this
    }

    /// C# `OpenFolder`.
    pub fn open_folder(&self) -> &ButtonElement {
        &self.open_folder
    }

    /// C# `Settings`.
    pub fn settings(&self) -> &ButtonElement {
        &self.settings
    }

    /// C# `Fit`.
    pub fn fit(&self) -> &ButtonElement {
        &self.fit
    }

    /// C# `Tools`.
    pub fn tools(&self) -> &ButtonElement {
        &self.tools
    }

    /// C# `Buttons` (`IReadOnlyList<ButtonElement>`, Tab order): `[open_folder, settings, fit, tools]`.
    pub fn buttons(&self) -> [&ButtonElement; 4] {
        [&self.open_folder, &self.settings, &self.fit, &self.tools]
    }

    /// Mutable view of the four buttons in Tab order, for the host to click or set hot/pressed state.
    pub fn buttons_mut(&mut self) -> [&mut ButtonElement; 4] {
        [
            &mut self.open_folder,
            &mut self.settings,
            &mut self.fit,
            &mut self.tools,
        ]
    }

    /// C# `FocusedIndex`: index in `buttons()`, `None` when no button holds the keyboard (C# `-1`).
    pub fn focused_index(&self) -> Option<usize> {
        self.focused_index
    }

    /// C# `HasKeyboardFocus` (`_focusedIndex >= 0`): the toolbar holds the keyboard (C# `IsKeyboardFocusWithin`).
    pub fn has_keyboard_focus(&self) -> bool {
        self.focused_index.is_some()
    }

    /// C# `FocusedButton`: the button at `focused_index`, `None` when there is none.
    pub fn focused_button(&self) -> Option<&ButtonElement> {
        self.focused_index.and_then(|i| self.buttons().get(i).copied())
    }

    /// C# `ApplyLocalization()`: sets content, tooltip and automation name of the buttons from `texts`
    /// (`OpenFolder`/`Settings`/`Tools`: tooltip and automation name; `Fit`: content, tooltip, automation name).
    /// Called by the constructor and on every language change. Does not touch the glyph contents of the
    /// open-folder, settings and tools buttons (fixed glyphs).
    pub fn apply_localization(&mut self, texts: &Localizer) {
        use pr_l10n::keys::*;
        self.open_folder.tool_tip = texts.get(MAIN_TOOLBAR_OPEN_FOLDER_TOOL_TIP);
        self.open_folder.automation_name = texts.get(MAIN_TOOLBAR_OPEN_FOLDER_AUTOMATION_NAME);
        self.settings.tool_tip = texts.get(MAIN_TOOLBAR_SETTINGS_TOOL_TIP);
        self.settings.automation_name = texts.get(MAIN_TOOLBAR_SETTINGS_AUTOMATION_NAME);
        self.fit.set_content(&texts.get(MAIN_TOOLBAR_FIT));
        self.fit.tool_tip = texts.get(MAIN_TOOLBAR_FIT_TOOL_TIP);
        self.fit.automation_name = texts.get(MAIN_TOOLBAR_FIT_AUTOMATION_NAME);
        self.tools.tool_tip = texts.get(MAIN_TOOLBAR_TOOLS_TOOL_TIP);
        self.tools.automation_name = texts.get(MAIN_TOOLBAR_TOOLS_AUTOMATION_NAME);
    }

    /// C# `MoveFocus(bool forward)` (Tab / Shift+Tab): from no focus, forward goes to index 0 and backward to the last
    /// index; otherwise `index + 1` or `index - 1`; an index outside `0..4` means "no button" (trap T3). Calls
    /// `set_focus` with the result. Returns `true` when a button still holds the keyboard.
    pub fn move_focus(&mut self, forward: bool) -> bool {
        let next = match self.focused_index {
            None => {
                if forward {
                    Some(0)
                } else {
                    Some(3)
                }
            }
            Some(i) if forward && i < 3 => Some(i + 1),
            Some(i) if !forward && i > 0 => Some(i - 1),
            _ => None,
        };
        self.set_focus(next);
        self.focused_index.is_some()
    }

    /// C# `ClearFocus()`: `set_focus(None)`.
    pub fn clear_focus(&mut self) {
        self.set_focus(None)
    }

    /// C# `SetFocus(int index)` (private): stores `focused_index` and sets `is_focused` on each button to
    /// `i == index` (trap T4).
    fn set_focus(&mut self, index: Option<usize>) {
        self.focused_index = index;
        for (i, button) in self.buttons_mut().into_iter().enumerate() {
            button.is_focused = Some(i) == index;
        }
    }
}

/// Element tree of the toolbar. Children in C# order: open folder, settings, fit, tools (the `Add` loop over `Buttons`).
/// Layout and paint are the `PanelElement` stack rules (W8-15d helpers, horizontal orientation).
impl ElementNode for ToolbarPanel {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn base(&self) -> &ElementBase {
        self.panel.base()
    }

    fn base_mut(&mut self) -> &mut ElementBase {
        self.panel.base_mut()
    }

    fn measure_core(&mut self, available: SizeD, context: &OverlayLayoutContext<'_>) -> SizeD {
        let mut w = self.panel.padding.horizontal();
        let mut h: f64 = 0.0;
        for b in self.buttons_mut() {
            let s = b.measure(available, context);
            w += s.width;
            h = h.max(s.height);
        }
        SizeD {
            width: w,
            height: h + self.panel.padding.vertical(),
        }
    }

    fn arrange_children(&mut self, context: &OverlayLayoutContext<'_>) {
        let bounds = self.base().bounds();
        let mut x = bounds.x + self.panel.padding.left;
        let y = bounds.y + self.panel.padding.top;
        let inner_height = (bounds.height - self.panel.padding.vertical()).max(0.0);
        for b in self.buttons_mut() {
            let d = b.base().desired_size();
            b.arrange_in(
                RectD {
                    x,
                    y,
                    width: d.width,
                    height: inner_height,
                },
                context,
            );
            x += d.width;
        }
    }

    fn render_core(&self, dc: &mut dyn DrawContext) {
        use crate::overlay::contracts::OverlayElement;
        if self.panel.background.a > 0.0 {
            dc.fill_rounded_rectangle(self.base().bounds(), self.panel.corner_radius, self.panel.background);
        }
        for b in self.buttons() {
            b.render(dc);
        }
    }

    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        for b in self.buttons().into_iter().rev() {
            if let Some(v) = b.find_at(point) {
                return Some(v);
            }
        }
        if self.hit_test(point) { Some(self) } else { None }
    }

    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        if self.id() == id {
            return Some(self);
        }
        for b in self.buttons() {
            if let Some(v) = b.find(id) {
                return Some(v);
            }
        }
        None
    }

    fn release_resources(&mut self) {
        for b in self.buttons_mut() {
            b.release_resources();
        }
    }
}

/// The family of every button text in the toolbar (C# `private const string Family = "Segoe UI"`).
const FAMILY: &str = "Segoe UI";

/// `new TextStyle(Family, size)` with the C# default bold `false`.
fn segoe_ui(size_dip: f64) -> TextStyle {
    TextStyle {
        font_family: FAMILY.to_owned(),
        font_size_dip: size_dip,
        bold: false,
    }
}

/// C# `CreateButton(id, content, size, padding, margin)`: the shared button style (`Style`, `Padding`, `Margin`,
/// `Background = Dark.Input`, `BorderColor = Dark.BorderToolbar`, `CornerRadius = 0`). Content is set by the caller.
fn create_button(
    id: &str,
    content: &str,
    size_dip: f64,
    padding: crate::overlay::element::OverlayThickness,
    margin: crate::overlay::element::OverlayThickness,
) -> ButtonElement {
    let mut b = ButtonElement::new(id).unwrap_or_else(|_| unreachable!("static ids are nonempty"));
    b.set_content(content);
    b.set_style(segoe_ui(size_dip));
    b.padding = padding;
    b.base_mut().margin = margin;
    b.background = crate::overlay::palette::from_argb(0xFF25_2525);
    b.border_color = crate::overlay::palette::from_argb(0xFF45_4545);
    b.corner_radius = 0.0;
    b
}
