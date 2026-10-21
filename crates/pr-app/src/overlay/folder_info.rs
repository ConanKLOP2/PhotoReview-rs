//! Port of `PhotoReview.Shell.Win32/Overlay/FolderInfoPanel.cs` (39 lines, baseline d92d6aae, WP-23): the folder-info block in the
//! bottom-right corner (`MaxWidth` 640, one line trimmed with an ellipsis: the current folder plus its two neighbours).
//! Composition like `CapturePairBadge` (decision D-15d-1): the panel wraps a `PanelElement` and delegates the `OverlayElement`
//! hooks.

use crate::input::primitives::{PointD, SizeD};
use crate::overlay::contracts::OverlayLayoutContext;
use crate::overlay::element::{
    ElementBase, ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment,
};
use crate::overlay::model::OverlayModel;
use crate::overlay::panel::{PanelElement, text_element};
use crate::overlay::text::TextElement;
use crate::services::render::surface::DrawContext;
use crate::services::render::text::{TextStyle, TextTrimming};
use pr_l10n::localizer::Localizer;

/// C# `FolderInfoPanel.PanelId`.
pub const PANEL_ID: &str = "FolderInfoPanel";
/// C# `FolderInfoPanel.TextId`.
pub const TEXT_ID: &str = "FolderInfoText";
/// Catalog key of `Tr.MainFolderInfoAutomationName` (`assets/languages/en.json`: "Current folder and the neighbouring image folders").
pub const AUTOMATION_NAME_KEY: &str = "main.folderInfo.automationName";
/// C# `MaxWidth = 640` (DIP).
pub const MAX_WIDTH: f64 = 640.0;

/// The folder-info panel. `panel` holds the text as child 0.
#[derive(Debug)]
pub struct FolderInfoPanel {
    panel: PanelElement,
    automation_name: String,
}

impl FolderInfoPanel {
    /// C# ctor: `HorizontalAlignment = Right`, `VerticalAlignment = Bottom`, `Margin` uniform 8, `Padding = (6, 4)`,
    /// `CornerRadius = 4`, `MaxWidth = MAX_WIDTH`, child 0 = `TextElement(TEXT_ID)` with `Trimming = CharacterEllipsis`,
    /// then `ApplyLocalization()` (the automation name is read from `texts` now).
    pub fn new(texts: &Localizer) -> Self {
        let mut panel = PanelElement::new(PANEL_ID);
        {
            let base = panel.base_mut();
            base.horizontal_alignment = OverlayHorizontalAlignment::Right;
            base.vertical_alignment = OverlayVerticalAlignment::Bottom;
            base.margin = OverlayThickness::uniform(8.0);
            base.max_width = MAX_WIDTH;
        }
        panel.padding = OverlayThickness::symmetric(6.0, 4.0);
        panel.corner_radius = 4.0;
        let mut text = text_element(TEXT_ID);
        text.set_trimming(TextTrimming::CharacterEllipsis);
        panel.add(Box::new(text));
        let mut this = Self {
            panel,
            automation_name: String::new(),
        };
        this.apply_localization(texts);
        this
    }

    /// C# `Text` property: the child text element (child 0 of the panel). `None` only if the child is not a `TextElement`.
    pub fn text(&self) -> Option<&TextElement> {
        self.panel.child::<TextElement>(0)
    }

    /// C# `AutomationName` (`AutomationProperties.Name`), read by screen readers.
    pub fn automation_name(&self) -> &str {
        &self.automation_name
    }

    /// C# `Apply(OverlayModel)`, in this order: `IsVisible = model.FolderInfoVisible`; `Text.Text = model.FolderInfoText`;
    /// `Text.Style = new TextStyle("Segoe UI", model.FontSize)` (see `CapturePairBadge::apply`).
    pub fn apply(&mut self, model: &OverlayModel) {
        self.panel.base_mut().is_visible = model.folder_info_visible;
        if let Some(text) = self.panel.child_mut::<TextElement>(0) {
            text.set_text(&model.folder_info_text);
            text.set_style(TextStyle {
                font_family: "Segoe UI".to_owned(),
                font_size_dip: model.font_size,
                ..TextStyle::default()
            });
        }
    }

    /// C# `internal void ApplyLocalization() => AutomationName = Tr.MainFolderInfoAutomationName;`:
    /// `automation_name = texts.get(AUTOMATION_NAME_KEY)`. Called again whenever the language changes.
    pub fn apply_localization(&mut self, texts: &Localizer) {
        self.automation_name = texts.get(AUTOMATION_NAME_KEY);
    }
}

impl ElementNode for FolderInfoPanel {
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
        self.panel.measure_core(available, ctx)
    }

    fn arrange_children(&mut self, ctx: &OverlayLayoutContext<'_>) {
        self.panel.arrange_children(ctx);
    }

    fn render_core(&self, dc: &mut dyn DrawContext) {
        self.panel.render_core(dc);
    }

    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        self.panel.find_at(point)
    }

    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        self.panel.find(id)
    }

    fn release_resources(&mut self) {
        self.panel.release_resources();
    }
}
