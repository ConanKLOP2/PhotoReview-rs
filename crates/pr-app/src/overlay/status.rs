//! Port of `PhotoReview.Shell.Win32/Overlay/StatusPanel.cs` (WP-23, C-11 area): the bottom-left status panel.
//!
//! Baseline `d92d6aae`, 92 lines. Owned by W8-15e (contract `contracts/W8-15e.md`).
//! Test kind F: hand-written expectations from `OverlayPartsTests` (see the contract), no golden.
//!
//! C# `StatusPanel : PanelElement` is composition in Rust (D-W8-6, contract section "Tree shape"): the struct owns
//! its typed children (status line, EXIF line, skipped row) and implements `ElementNode` itself, so the typed handles
//! the shell needs (`view_list`, `status`, `exif`) stay reachable. Its `PanelElement` (W8-15d) carries only the box
//! style (background, corner radius, padding, orientation) and the base state.
//!
//! Traps (contract section "Traps"): T1 the EXIF line sits exactly 2 DIP below the status line (`margin top 2`);
//! T2 the skipped warning keeps its own fixed size 13, never the model font size; T3 `apply` never touches the skipped
//! warning style or the view-list button style; T4 `apply_localization` is the only writer of button text.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use pr_l10n::localizer::Localizer;

use crate::input::primitives::{PointD, SizeD};
use crate::overlay::button::ButtonElement;
use crate::overlay::contracts::OverlayLayoutContext;
use crate::overlay::element::ElementNode;
use crate::overlay::model::OverlayModel;
use crate::overlay::panel::PanelElement;
use crate::overlay::text::TextElement;
use crate::services::render::DrawContext;
use crate::services::render::text::TextStyle;

/// C# `StatusPanel.PanelId`.
pub const PANEL_ID: &str = "StatusPanel";
/// C# `StatusPanel.StatusTextId`.
pub const STATUS_TEXT_ID: &str = "StatusText";
/// C# `StatusPanel.ExifTextId`.
pub const EXIF_TEXT_ID: &str = "ExifInfoText";
/// C# `StatusPanel.SkippedRowId`.
pub const SKIPPED_ROW_ID: &str = "SkippedRow";
/// C# `StatusPanel.SkippedViewListId`.
pub const SKIPPED_VIEW_LIST_ID: &str = "SkippedViewListButton";
/// C# literal `"SkippedWarningText"` (the warning text id, not a `const` in C#).
pub const SKIPPED_WARNING_ID: &str = "SkippedWarningText";

/// C# `StatusPanel`: the status line, the EXIF line and the skipped-files row, in that order (`Add(Status).Add(Exif)
/// .Add(SkippedRow)`). Style: `HorizontalAlignment = Left`, `VerticalAlignment = Bottom`, margin 8, padding (6, 4),
/// corner radius 4, background from `PanelElement` (`Dark.Overlay`).
#[derive(Debug)]
pub struct StatusPanel {
    /// Box style and base state (`ElementBase`). Holds no children: the children are the typed fields below.
    panel: PanelElement,
    /// C# `Status`: `TextElement(StatusTextId)`, trimming `CharacterEllipsis`.
    status: TextElement,
    /// C# `Exif`: `TextElement(ExifTextId)`, trimming `CharacterEllipsis`, margin `(0, 2, 0, 0)`, colour `Dark.TextSecondary`.
    exif: TextElement,
    /// C# `SkippedRow`: horizontal row (margin `(0, 4, 0, 0)`, transparent background), children warning then view list.
    skipped_row: SkippedRow,
}

/// C# `SkippedRow` (a `PanelElement` built inside `StatusPanel`): horizontal row holding the warning text and the
/// "View list" button. Own type in Rust because the button and text must stay typed (see the module doc).
#[derive(Debug)]
pub struct SkippedRow {
    /// Box style: orientation `Horizontal`, margin `(0, 4, 0, 0)`, background `ColorF(0, 0, 0, 0)`.
    panel: PanelElement,
    /// C# `SkippedWarning`: `TextElement("SkippedWarningText")`, `TextStyle("Segoe UI", 13)`, colour `Dark.Warning`,
    /// vertical alignment `Center`.
    warning: TextElement,
    /// C# `ViewList`: `ButtonElement(SkippedViewListId)`, padding `(8, 2)`, margin `(8, 0, 0, 0)`, background
    /// `Dark.Input`, border `Dark.BorderToolbar`, corner radius 0, vertical alignment `Center`.
    view_list: ButtonElement,
}

impl StatusPanel {
    /// C# constructor `StatusPanel()`: builds the tree with the styles above, then `ApplyLocalization()` (the button
    /// content and automation name come from `texts`). Does not apply a model: the fields keep their defaults until
    /// `apply` (C# default `IsVisible = true` for the panel, the children are `TextElement` / `ButtonElement` defaults).
    pub fn new(texts: &Localizer) -> Self {
        todo!()
    }

    /// C# `Status` (the status line; the shell reads `Bounds`).
    pub fn status(&self) -> &TextElement {
        todo!()
    }

    /// C# `Exif` (the EXIF line).
    pub fn exif(&self) -> &TextElement {
        todo!()
    }

    /// C# `SkippedWarning` (through the row; the warning text is `skipped_row().warning()`).
    pub fn skipped_row(&self) -> &SkippedRow {
        todo!()
    }

    /// C# `ViewList` (the "View list" button). Read access for `Bounds`, `IsHot`, `IsPressed`.
    pub fn view_list(&self) -> &ButtonElement {
        todo!()
    }

    /// C# `ViewList` for the host that clicks it (`ButtonElement::click`) and sets `IsHot` / `IsPressed` / `IsFocused`.
    pub fn view_list_mut(&mut self) -> &mut ButtonElement {
        todo!()
    }

    /// C# `Apply(OverlayModel)`: `IsVisible = model.status_panel_visible`; status line `IsVisible = file_info_visible`,
    /// `Text = status_text`, `Style = (Segoe UI, font_size)`; EXIF `IsVisible = exif_visible`, `Text = exif_text`,
    /// `Style = (Segoe UI, exif_font_size)`; `skipped_row` `IsVisible = has_skipped_entries`; warning
    /// `Text = skipped_warning_text`. Nothing else is written (trap T2, T3).
    pub fn apply(&mut self, model: &OverlayModel) {
        todo!()
    }

    /// C# `ApplyLocalization()`: `view_list.content = MainSkippedViewList`, `view_list.automation_name =
    /// MainSkippedViewListAutomationName` (keys `pr_l10n::keys::MAIN_SKIPPED_VIEW_LIST` and
    /// `MAIN_SKIPPED_VIEW_LIST_AUTOMATION_NAME`). Called by the constructor and on every language change.
    pub fn apply_localization(&mut self, texts: &Localizer) {
        todo!()
    }
}

impl SkippedRow {
    /// The warning text (C# `SkippedWarning`).
    pub fn warning(&self) -> &TextElement {
        todo!()
    }

    /// The "View list" button (C# `ViewList`, read side).
    pub fn view_list(&self) -> &ButtonElement {
        todo!()
    }

    /// The "View list" button (write side).
    pub fn view_list_mut(&mut self) -> &mut ButtonElement {
        todo!()
    }
}

/// Element tree of the status panel. Children in C# order: status, EXIF, skipped row. Layout and paint are the
/// `PanelElement` stack rules (W8-15d helpers) applied to the three typed children; `find` / `find_at` walk the same
/// order (C# `Find` / `FindAt` of `PanelElement`: self first, then the first match in the subtree).
impl ElementNode for StatusPanel {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn base(&self) -> &crate::overlay::element::ElementBase {
        todo!()
    }

    fn base_mut(&mut self) -> &mut crate::overlay::element::ElementBase {
        todo!()
    }

    fn measure_core(&mut self, available: SizeD, context: &OverlayLayoutContext<'_>) -> SizeD {
        todo!()
    }

    fn arrange_children(&mut self, context: &OverlayLayoutContext<'_>) {
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

/// Element tree of the skipped row: children are the warning text, then the view-list button.
impl ElementNode for SkippedRow {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn base(&self) -> &crate::overlay::element::ElementBase {
        todo!()
    }

    fn base_mut(&mut self) -> &mut crate::overlay::element::ElementBase {
        todo!()
    }

    fn measure_core(&mut self, available: SizeD, context: &OverlayLayoutContext<'_>) -> SizeD {
        todo!()
    }

    fn arrange_children(&mut self, context: &OverlayLayoutContext<'_>) {
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

/// The family of every text in this panel (C# `private const string Family = "Segoe UI"`).
const FAMILY: &str = "Segoe UI";

/// `new TextStyle(Family, size)` with the C# default bold `false`.
fn segoe_ui(size_dip: f64) -> TextStyle {
    todo!()
}
