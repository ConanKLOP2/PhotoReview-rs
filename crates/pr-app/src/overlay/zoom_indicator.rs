//! Port of `PhotoReview.Shell.Win32/Overlay/ZoomIndicatorPanel.cs` (WP-23): the top-right zoom percentage.
//!
//! Baseline `d92d6aae`, 38 lines. Owned by W8-15e (contract `contracts/W8-15e.md`). Test kind F.
//!
//! Traps (contract section "Traps"): T1 the text is never trimmed (C# sets no trimming, unlike the status line);
//! T2 `automation_name` is read by the host for the accessibility tree and set only by `apply_localization`.

use pr_l10n::localizer::Localizer;

use crate::input::primitives::{PointD, RectD, SizeD};
use crate::overlay::contracts::OverlayElement;
use crate::overlay::contracts::OverlayLayoutContext;
use crate::overlay::element::{
    ElementBase, ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment,
};
use crate::overlay::model::OverlayModel;
use crate::overlay::panel::PanelElement;
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
        panel.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Right;
        panel.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
        panel.base_mut().margin = OverlayThickness::uniform(8.0);
        panel.padding = OverlayThickness::symmetric(6.0, 4.0);
        panel.corner_radius = 4.0;
        let mut this = Self {
            panel,
            text: TextElement::new(TEXT_ID).unwrap_or_else(|_| unreachable!("static id")),
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
            bold: false,
        });
    }

    /// C# `ApplyLocalization()`: `automation_name = MainZoomIndicatorAutomationName` (key
    /// `pr_l10n::keys::MAIN_ZOOM_INDICATOR_AUTOMATION_NAME`). Called by the constructor and on language change.
    pub fn apply_localization(&mut self, texts: &Localizer) {
        self.automation_name = texts.get(pr_l10n::keys::MAIN_ZOOM_INDICATOR_AUTOMATION_NAME)
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

    fn measure_core(&mut self, available: SizeD, context: &OverlayLayoutContext<'_>) -> SizeD {
        let s = self.text.measure(available, context);
        SizeD {
            width: s.width + self.panel.padding.horizontal(),
            height: s.height + self.panel.padding.vertical(),
        }
    }

    fn arrange_children(&mut self, context: &OverlayLayoutContext<'_>) {
        let b = self.base().bounds();
        self.text.arrange_in(
            RectD {
                x: b.x + self.panel.padding.left,
                y: b.y + self.panel.padding.top,
                width: (b.width - self.panel.padding.horizontal()).max(0.0),
                height: (b.height - self.panel.padding.vertical()).max(0.0),
            },
            context,
        )
    }

    fn render_core(&self, dc: &mut dyn DrawContext) {
        if self.panel.background.a > 0.0 {
            dc.fill_rounded_rectangle(self.base().bounds(), self.panel.corner_radius, self.panel.background)
        }
        self.text.render(dc)
    }

    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        if self.hit_test(point) {
            self.text.find_at(point).or(Some(self))
        } else {
            None
        }
    }

    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        if self.id() == id {
            Some(self)
        } else {
            self.text.find(id)
        }
    }

    fn release_resources(&mut self) {
        self.text.release_resources()
    }
}
