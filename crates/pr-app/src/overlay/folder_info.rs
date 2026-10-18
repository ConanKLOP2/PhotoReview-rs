//! Port of `PhotoReview.Shell.Win32/Overlay/FolderInfoPanel.cs` (39 lines, baseline d92d6aae, WP-23): the folder-info block in the
//! bottom-right corner (`MaxWidth` 640, one line trimmed with an ellipsis: the current folder plus its two neighbours).
//! Composition like `CapturePairBadge` (decision D-15d-1): the panel wraps a `PanelElement` and delegates the `OverlayElement`
//! hooks. Stub: every body is `todo!()`.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use crate::input::primitives::{PointD, SizeD};
use crate::overlay::contracts::OverlayLayoutContext;
use crate::overlay::element::{ElementBase, ElementNode};
use crate::overlay::model::OverlayModel;
use crate::overlay::panel::PanelElement;
use crate::overlay::text::TextElement;
use crate::services::render::surface::DrawContext;
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
        todo!()
    }

    /// C# `Text` property: the child text element (child 0 of the panel). `None` only if the child is not a `TextElement`.
    pub fn text(&self) -> Option<&TextElement> {
        todo!()
    }

    /// C# `AutomationName` (`AutomationProperties.Name`), read by screen readers.
    pub fn automation_name(&self) -> &str {
        todo!()
    }

    /// C# `Apply(OverlayModel)`, in this order: `IsVisible = model.FolderInfoVisible`; `Text.Text = model.FolderInfoText`;
    /// `Text.Style = new TextStyle("Segoe UI", model.FontSize)` (see `CapturePairBadge::apply`).
    pub fn apply(&mut self, model: &OverlayModel) {
        todo!()
    }

    /// C# `internal void ApplyLocalization() => AutomationName = Tr.MainFolderInfoAutomationName;`:
    /// `automation_name = texts.get(AUTOMATION_NAME_KEY)`. Called again whenever the language changes.
    pub fn apply_localization(&mut self, texts: &Localizer) {
        todo!()
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
        todo!()
    }

    fn base_mut(&mut self) -> &mut ElementBase {
        todo!()
    }

    fn measure_core(&mut self, available: SizeD, ctx: &OverlayLayoutContext<'_>) -> SizeD {
        todo!()
    }

    fn arrange_children(&mut self, ctx: &OverlayLayoutContext<'_>) {
        todo!()
    }

    fn render_core(&self, dc: &mut dyn DrawContext) {
        todo!()
    }

    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        todo!()
    }

    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        todo!()
    }

    fn release_resources(&mut self) {
        todo!()
    }
}
