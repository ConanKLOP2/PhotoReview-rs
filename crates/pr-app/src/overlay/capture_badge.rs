//! Port of `PhotoReview.Shell.Win32/Overlay/CapturePairBadge.cs` (38 lines, baseline d92d6aae, WP-23): the JPEG/RAW capture-pair
//! badge, centred at the top, 44 DIP down (under the toolbar). Its tooltip text tells the user which key switches to the other
//! member. C# `CapturePairBadge : PanelElement` is a composition here: the badge wraps a `PanelElement` and delegates the
//! `OverlayElement` hooks to it (contract.md, decision D-15d-1). Stub: every body is `todo!()`.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use crate::input::primitives::{PointD, SizeD};
use crate::overlay::contracts::OverlayLayoutContext;
use crate::overlay::element::{ElementBase, ElementNode};
use crate::overlay::model::OverlayModel;
use crate::overlay::panel::PanelElement;
use crate::overlay::text::TextElement;
use crate::services::render::surface::DrawContext;

/// C# `CapturePairBadge.PanelId`.
pub const PANEL_ID: &str = "CapturePairBadge";
/// C# `CapturePairBadge.TextId`.
pub const TEXT_ID: &str = "CapturePairText";

/// The badge. `panel` holds the text as child 0.
#[derive(Debug)]
pub struct CapturePairBadge {
    panel: PanelElement,
    tool_tip: String,
}

impl Default for CapturePairBadge {
    /// Same as [`CapturePairBadge::new`] (clippy `new_without_default`).
    fn default() -> Self {
        Self::new()
    }
}

impl CapturePairBadge {
    /// C# ctor: `HorizontalAlignment = Center`, `VerticalAlignment = Top`, `Margin = (8, 44, 8, 8)`, `Padding = (6, 4)`
    /// (`OverlayThickness::hv(6, 4)`), `CornerRadius = 4`, child 0 = `TextElement(TEXT_ID)`, `ToolTip = ""`.
    pub fn new() -> Self {
        todo!()
    }

    /// C# `Text` property: the child text element (child 0 of the panel). `None` only if the child is not a `TextElement`.
    pub fn text(&self) -> Option<&TextElement> {
        todo!()
    }

    /// C# `ToolTip`: the tooltip copied from the model by the last `apply`. The C# XML doc says "empty while hidden", but the code
    /// does not clear it: the model supplies an empty tooltip when the badge is hidden (contract.md, trap T6).
    pub fn tool_tip(&self) -> &str {
        todo!()
    }

    /// C# `internal void Apply(OverlayModel model)`, in this order: `IsVisible = model.CapturePairVisible`;
    /// `Text.Text = model.CapturePairText`; `Text.Style = new TextStyle("Segoe UI", model.FontSize)`
    /// (`TextStyle { font_family: "Segoe UI".to_owned(), font_size_dip: model.font_size, ..TextStyle::default() }`);
    /// `ToolTip = model.CapturePairToolTip`. Visible or not, every field is applied.
    pub fn apply(&mut self, model: &OverlayModel) {
        todo!()
    }
}

impl ElementNode for CapturePairBadge {
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
