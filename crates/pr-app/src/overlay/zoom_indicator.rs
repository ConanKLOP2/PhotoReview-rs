//! Port of `PhotoReview.Shell.Win32/Overlay/ZoomIndicatorPanel.cs` (WP-23): the top-right zoom percentage.
//!
//! Baseline `d92d6aae`, 38 lines. Owned by W8-15e (contract `contracts/W8-15e.md`). Test kind F.
//!
//! Traps (contract section "Traps"): T1 the text is never trimmed (C# sets no trimming, unlike the status line);
//! T2 `automation_name` is read by the host for the accessibility tree and set only by `apply_localization`.

use pr_l10n::keys::MAIN_ZOOM_INDICATOR_AUTOMATION_NAME;
use pr_l10n::localizer::Localizer;

use crate::input::primitives::{PointD, SizeD};
use crate::overlay::contracts::OverlayLayoutContext;
use crate::overlay::element::{
    ElementBase, ElementNode, OverlayElement, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment,
};
use crate::overlay::model::OverlayModel;
use crate::overlay::panel::{
    PanelElement, arrange_stack, find_at_children, find_in_children, measure_stack, render_stack, text_element,
};
use crate::overlay::text::TextElement;
use crate::services::render::DrawContext;
use crate::services::render::text::TextStyle;

/// C# `ZoomIndicatorPanel.PanelId`.
pub const PANEL_ID: &str = "ZoomIndicatorPanel";
/// C# `ZoomIndicatorPanel.TextId`.
pub const TEXT_ID: &str = "ZoomIndicatorText";

/// C# `ZoomIndicatorPanel : PanelElement`. Style: `HorizontalAlignment = Right`, `VerticalAlignment = Top`, margin 8,
/// padding (6, 4), corner radius 4, background from `PanelElement`. One child: the percentage text.
#[derive(Debug)]
pub struct ZoomIndicatorPanel {
    /// Box style and base state. Holds no children (the text below is the only child).
    panel: PanelElement,
    /// C# `Text`: `TextElement("ZoomIndicatorText")`, default style and colour (no trimming).
    text: TextElement,
    /// C# `AutomationName` (`AutomationProperties.Name`), set by `apply_localization`; `""` until then.
    automation_name: String,
}

impl ZoomIndicatorPanel {
    /// C# constructor `ZoomIndicatorPanel()`: builds the tree, then `ApplyLocalization()` with `texts`.
    pub fn new(texts: &Localizer) -> Self {
        let mut panel = PanelElement::new(PANEL_ID);
        {
            let base = panel.base_mut();
            base.horizontal_alignment = OverlayHorizontalAlignment::Right;
            base.vertical_alignment = OverlayVerticalAlignment::Top;
            base.margin = OverlayThickness::uniform(8.0);
        }
        panel.padding = OverlayThickness::symmetric(6.0, 4.0);
        panel.corner_radius = 4.0;
        let mut this = Self {
            panel,
            text: text_element(TEXT_ID),
            automation_name: String::new(),
        };
        this.apply_localization(texts);
        this
    }

    /// C# `Text` (the percentage text; the shell reads `Bounds`).
    pub fn text(&self) -> &TextElement {
        &self.text
    }

    /// C# `AutomationName`.
    pub fn automation_name(&self) -> &str {
        &self.automation_name
    }

    /// C# `Apply(OverlayModel)`: `IsVisible = model.zoom_indicator_visible`; `Text.Text = model.zoom_indicator_text`;
    /// `Text.Style = (Segoe UI, model.font_size)`. Nothing else is written.
    pub fn apply(&mut self, model: &OverlayModel) {
        self.panel.base_mut().is_visible = model.zoom_indicator_visible;
        self.text.set_text(&model.zoom_indicator_text);
        self.text.set_style(TextStyle {
            font_family: "Segoe UI".to_owned(),
            font_size_dip: model.font_size,
            ..TextStyle::default()
        });
    }

    /// C# `ApplyLocalization()`: `automation_name = MainZoomIndicatorAutomationName` (key
    /// `pr_l10n::keys::MAIN_ZOOM_INDICATOR_AUTOMATION_NAME`). Called by the constructor and on language change.
    pub fn apply_localization(&mut self, texts: &Localizer) {
        self.automation_name = texts.get(MAIN_ZOOM_INDICATOR_AUTOMATION_NAME);
    }
}

/// Element tree: the single child is the percentage text. Layout and paint are the `PanelElement` stack rules
/// (W8-15d helpers).
impl ElementNode for ZoomIndicatorPanel {
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

    fn measure_core(&mut self, available: SizeD, ctx: &OverlayLayoutContext<'_>) -> SizeD {
        let style = self.panel.style();
        let mut children: [&mut dyn ElementNode; 1] = [&mut self.text];
        measure_stack(style, available, &mut children, ctx)
    }

    fn arrange_children(&mut self, ctx: &OverlayLayoutContext<'_>) {
        let style = self.panel.style();
        let bounds = self.panel.base().bounds();
        let mut children: [&mut dyn ElementNode; 1] = [&mut self.text];
        arrange_stack(style, bounds, &mut children, ctx);
    }

    fn render_core(&self, dc: &mut dyn DrawContext) {
        let children: [&dyn ElementNode; 1] = [&self.text];
        render_stack(self.panel.style(), self.panel.base().bounds(), &children, dc);
    }

    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        if !self.hit_test(point) {
            return None;
        }
        let children: [&dyn ElementNode; 1] = [&self.text];
        find_at_children(&children, point).or(Some(self as &dyn ElementNode))
    }

    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        if self.panel.base().id() == id {
            return Some(self);
        }
        let children: [&dyn ElementNode; 1] = [&self.text];
        find_in_children(&children, id)
    }

    fn release_resources(&mut self) {
        self.text.release_resources();
    }
}
