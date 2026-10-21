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

use pr_l10n::keys::{MAIN_SKIPPED_VIEW_LIST, MAIN_SKIPPED_VIEW_LIST_AUTOMATION_NAME};
use pr_l10n::localizer::Localizer;

use crate::input::primitives::{PointD, SizeD};
use crate::overlay::button::ButtonElement;
use crate::overlay::contracts::OverlayLayoutContext;
use crate::overlay::element::{
    ElementNode, OverlayElement, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment,
};
use crate::overlay::model::OverlayModel;
use crate::overlay::palette;
use crate::overlay::panel::{
    OverlayOrientation, PanelElement, arrange_stack, button_element, dark_color, find_at_children, find_in_children,
    measure_stack, render_stack, text_element,
};
use crate::overlay::text::TextElement;
use crate::services::render::DrawContext;
use crate::services::render::text::{TextStyle, TextTrimming};
use crate::viewport::render_geometry::ColorF;

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
        let mut panel = PanelElement::new(PANEL_ID);
        {
            let base = panel.base_mut();
            base.horizontal_alignment = OverlayHorizontalAlignment::Left;
            base.vertical_alignment = OverlayVerticalAlignment::Bottom;
            base.margin = OverlayThickness::uniform(8.0);
        }
        panel.padding = OverlayThickness::symmetric(6.0, 4.0);
        panel.corner_radius = 4.0;

        let mut status = text_element(STATUS_TEXT_ID);
        status.set_trimming(TextTrimming::CharacterEllipsis);
        let mut exif = text_element(EXIF_TEXT_ID);
        exif.set_trimming(TextTrimming::CharacterEllipsis);
        exif.base_mut().margin = OverlayThickness::new(0.0, 2.0, 0.0, 0.0);
        exif.color = palette::text_secondary();

        let mut this = Self {
            panel,
            status,
            exif,
            skipped_row: SkippedRow::new(),
        };
        this.apply_localization(texts);
        this
    }

    /// C# `Status` (the status line; the shell reads `Bounds`).
    pub fn status(&self) -> &TextElement {
        &self.status
    }

    /// C# `Exif` (the EXIF line).
    pub fn exif(&self) -> &TextElement {
        &self.exif
    }

    /// C# `SkippedWarning` (through the row; the warning text is `skipped_row().warning()`).
    pub fn skipped_row(&self) -> &SkippedRow {
        &self.skipped_row
    }

    /// C# `ViewList` (the "View list" button). Read access for `Bounds`, `IsHot`, `IsPressed`.
    pub fn view_list(&self) -> &ButtonElement {
        &self.skipped_row.view_list
    }

    /// C# `ViewList` for the host that clicks it (`ButtonElement::click`) and sets `IsHot` / `IsPressed` / `IsFocused`.
    pub fn view_list_mut(&mut self) -> &mut ButtonElement {
        &mut self.skipped_row.view_list
    }

    /// C# `Apply(OverlayModel)`: `IsVisible = model.status_panel_visible`; status line `IsVisible = file_info_visible`,
    /// `Text = status_text`, `Style = (Segoe UI, font_size)`; EXIF `IsVisible = exif_visible`, `Text = exif_text`,
    /// `Style = (Segoe UI, exif_font_size)`; `skipped_row` `IsVisible = has_skipped_entries`; warning
    /// `Text = skipped_warning_text`. Nothing else is written (trap T2, T3).
    pub fn apply(&mut self, model: &OverlayModel) {
        self.panel.base_mut().is_visible = model.status_panel_visible;
        self.status.base_mut().is_visible = model.file_info_visible;
        self.status.set_text(&model.status_text);
        self.status.set_style(segoe_ui(model.font_size));
        self.exif.base_mut().is_visible = model.exif_visible;
        self.exif.set_text(&model.exif_text);
        self.exif.set_style(segoe_ui(model.exif_font_size));
        self.skipped_row.panel.base_mut().is_visible = model.has_skipped_entries;
        self.skipped_row.warning.set_text(&model.skipped_warning_text);
    }

    /// C# `ApplyLocalization()`: `view_list.content = MainSkippedViewList`, `view_list.automation_name =
    /// MainSkippedViewListAutomationName` (keys `pr_l10n::keys::MAIN_SKIPPED_VIEW_LIST` and
    /// `MAIN_SKIPPED_VIEW_LIST_AUTOMATION_NAME`). Called by the constructor and on every language change.
    pub fn apply_localization(&mut self, texts: &Localizer) {
        let view_list = &mut self.skipped_row.view_list;
        view_list.set_content(&texts.get(MAIN_SKIPPED_VIEW_LIST));
        view_list.automation_name = texts.get(MAIN_SKIPPED_VIEW_LIST_AUTOMATION_NAME);
    }
}

impl SkippedRow {
    /// C# `SkippedRow` construction inside the `StatusPanel` constructor: horizontal, margin `(0, 4, 0, 0)`, transparent background;
    /// the warning (size 13, `Dark.Warning`, centred vertically) then the view-list button.
    fn new() -> Self {
        let mut panel = PanelElement::new(SKIPPED_ROW_ID);
        panel.orientation = OverlayOrientation::Horizontal;
        panel.base_mut().margin = OverlayThickness::new(0.0, 4.0, 0.0, 0.0);
        panel.background = ColorF {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };

        let mut warning = text_element(SKIPPED_WARNING_ID);
        warning.set_style(segoe_ui(13.0));
        warning.color = palette::warning();
        warning.base_mut().vertical_alignment = OverlayVerticalAlignment::Center;

        let mut view_list = button_element(SKIPPED_VIEW_LIST_ID);
        view_list.padding = OverlayThickness::symmetric(8.0, 2.0);
        view_list.base_mut().margin = OverlayThickness::new(8.0, 0.0, 0.0, 0.0);
        view_list.background = dark_color("Dark.Input");
        view_list.border_color = dark_color("Dark.BorderToolbar");
        view_list.corner_radius = 0.0;
        view_list.base_mut().vertical_alignment = OverlayVerticalAlignment::Center;

        Self {
            panel,
            warning,
            view_list,
        }
    }

    /// The warning text (C# `SkippedWarning`).
    pub fn warning(&self) -> &TextElement {
        &self.warning
    }

    /// The "View list" button (C# `ViewList`, read side).
    pub fn view_list(&self) -> &ButtonElement {
        &self.view_list
    }

    /// The "View list" button (write side).
    pub fn view_list_mut(&mut self) -> &mut ButtonElement {
        &mut self.view_list
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
        self.panel.base()
    }

    fn base_mut(&mut self) -> &mut crate::overlay::element::ElementBase {
        self.panel.base_mut()
    }

    fn measure_core(&mut self, available: SizeD, ctx: &OverlayLayoutContext<'_>) -> SizeD {
        let style = self.panel.style();
        let mut children: [&mut dyn ElementNode; 3] = [&mut self.status, &mut self.exif, &mut self.skipped_row];
        measure_stack(style, available, &mut children, ctx)
    }

    fn arrange_children(&mut self, ctx: &OverlayLayoutContext<'_>) {
        let style = self.panel.style();
        let bounds = self.panel.base().bounds();
        let mut children: [&mut dyn ElementNode; 3] = [&mut self.status, &mut self.exif, &mut self.skipped_row];
        arrange_stack(style, bounds, &mut children, ctx);
    }

    fn render_core(&self, dc: &mut dyn DrawContext) {
        let children: [&dyn ElementNode; 3] = [&self.status, &self.exif, &self.skipped_row];
        render_stack(self.panel.style(), self.panel.base().bounds(), &children, dc);
    }

    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        if !self.hit_test(point) {
            return None;
        }
        let children: [&dyn ElementNode; 3] = [&self.status, &self.exif, &self.skipped_row];
        find_at_children(&children, point).or(Some(self as &dyn ElementNode))
    }

    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        if self.panel.base().id() == id {
            return Some(self);
        }
        let children: [&dyn ElementNode; 3] = [&self.status, &self.exif, &self.skipped_row];
        find_in_children(&children, id)
    }

    fn release_resources(&mut self) {
        self.status.release_resources();
        self.exif.release_resources();
        self.skipped_row.release_resources();
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
        self.panel.base()
    }

    fn base_mut(&mut self) -> &mut crate::overlay::element::ElementBase {
        self.panel.base_mut()
    }

    fn measure_core(&mut self, available: SizeD, ctx: &OverlayLayoutContext<'_>) -> SizeD {
        let style = self.panel.style();
        let mut children: [&mut dyn ElementNode; 2] = [&mut self.warning, &mut self.view_list];
        measure_stack(style, available, &mut children, ctx)
    }

    fn arrange_children(&mut self, ctx: &OverlayLayoutContext<'_>) {
        let style = self.panel.style();
        let bounds = self.panel.base().bounds();
        let mut children: [&mut dyn ElementNode; 2] = [&mut self.warning, &mut self.view_list];
        arrange_stack(style, bounds, &mut children, ctx);
    }

    fn render_core(&self, dc: &mut dyn DrawContext) {
        let children: [&dyn ElementNode; 2] = [&self.warning, &self.view_list];
        render_stack(self.panel.style(), self.panel.base().bounds(), &children, dc);
    }

    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        if !self.hit_test(point) {
            return None;
        }
        let children: [&dyn ElementNode; 2] = [&self.warning, &self.view_list];
        find_at_children(&children, point).or(Some(self as &dyn ElementNode))
    }

    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        if self.panel.base().id() == id {
            return Some(self);
        }
        let children: [&dyn ElementNode; 2] = [&self.warning, &self.view_list];
        find_in_children(&children, id)
    }

    fn release_resources(&mut self) {
        self.warning.release_resources();
        self.view_list.release_resources();
    }
}

/// The family of every text in this panel (C# `private const string Family = "Segoe UI"`).
const FAMILY: &str = "Segoe UI";

/// `new TextStyle(Family, size)` with the C# default bold `false`.
fn segoe_ui(size_dip: f64) -> TextStyle {
    TextStyle {
        font_family: FAMILY.to_owned(),
        font_size_dip: size_dip,
        bold: false,
    }
}
