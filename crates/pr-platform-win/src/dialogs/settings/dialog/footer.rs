//! Port of `SettingsDialog.cs` lines 1183 to 1279 (`Margin`, `FieldHeight`, `ButtonHeight`, `ScrollBarWidth`, `ButtonWidthOf`, `LeftButtons`, `RightButtons`, `RowWidth`,
//! `PreferredClientSize`, `MinimumClientSize`, `LayoutControls`, `PlaceRow`), baseline `d92d6aae`. Unit W8-19m. Contract: `contracts/W8-19m.md`.
//!
//! The layout of the FRAME: the two button rows at the bottom (one row when the window is wide enough, else the left group above the right group), the navigation column on
//! the left, the page title / page area / scroll bar on the right. Everything is arithmetic on a [`DialogMeasure`] (pure, D-W8B5-501): [`layout_plan`] returns what the C#
//! `LayoutControls` would `Place`, in the same order; `layout_controls` executes the plan on the real dialog and then lays out the showing page.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use super::frame::SettingsContent;
use crate::dialogs::native::dialog::NativeDialog;
use crate::dialogs::zoom_prompt::{DialogMeasure, Placement};

/// C# `LeftButtons` = `[DefaultsId, ResetPageId, ExportId, ImportId]` (left to right).
pub const LEFT_BUTTONS: [i32; 4] = [121, 122, 123, 124];
/// C# `RightButtons` = `[CancelId, ApplyId, SaveId]` (left to right).
pub const RIGHT_BUTTONS: [i32; 3] = [2, 120, 1];

/// C# `Margin` = `dx(10)`.
pub fn margin(m: &dyn DialogMeasure) -> i32 {
    todo!()
}

/// C# `FieldHeight` = `max(dy(14), line_height + dy(6))`.
pub fn field_height(m: &dyn DialogMeasure) -> i32 {
    todo!()
}

/// C# `ButtonHeight` = `max(dy(14), line_height + dy(6))` (equal to `field_height`).
pub fn button_height(m: &dyn DialogMeasure) -> i32 {
    todo!()
}

/// C# `ScrollBarWidth` = `max(dx(10), 16)`.
pub fn scroll_bar_width(m: &dyn DialogMeasure) -> i32 {
    todo!()
}

/// C# `ButtonWidthOf(id)` = `max(dx(50), measure_text_width(item_text(id)) + dx(16))`.
pub fn button_width_of(m: &dyn DialogMeasure, id: i32) -> i32 {
    todo!()
}

/// C# `RowWidth(ids)` = `sum(button_width_of) + (ids.len() - 1) * dx(6)` (wrapping `i32`; an empty row is `-dx(6)` as in C#, never used).
pub fn row_width(m: &dyn DialogMeasure, ids: &[i32]) -> i32 {
    todo!()
}

/// C# `PreferredClientSize()` = `(dx(540), dy(330))`.
pub fn preferred_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    todo!()
}

/// C# `MinimumClientSize()` = `(max(dx(360), max(row_width(LEFT), row_width(RIGHT)) + 2 * margin), dy(230))`.
pub fn minimum_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    todo!()
}

/// What `LayoutControls` decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsLayoutPlan {
    /// Every `Place(id, x, y, w, h)` in the C# call order: the right button row (Cancel, Apply, Save), the left row (Defaults, Reset, Export, Import), the header, the
    /// navigation list, the version text, the page title, one page container per page (`viewport_id(i)`), the scroll bar. Widths and heights are NOT clamped here
    /// (`NativeDialog::place` clamps negatives to 0).
    pub placements: Vec<Placement>,
    /// `SetListItemHeight(NavId, ..)`: `line_height + dy(6)`; the C# calls it right after the navigation list is placed.
    pub nav_item_height: i32,
    /// C# `_viewportWidth`.
    pub viewport_width: i32,
    /// C# `_viewportHeight`.
    pub viewport_height: i32,
}

/// C# `LayoutControls(clientWidth, clientHeight)` (lines 1206 to 1268) as a plan. Exact arithmetic (all `i32`, saturating):
/// - `m = margin`, `bh = button_height`, `lw = row_width(LEFT)`, `rw = row_width(RIGHT)`, `single_row = lw + dx(16) + rw <= client_width - 2m`;
/// - right row: `top = client_height - m - bh`, starting at `x = client_width - m - rw`; left row: `left_top = single_row ? top : top - dy(4) - bh`, starting at `x = m`;
///   a row places each id with `button_width_of(id)` and advances `x += width + dx(6)`;
/// - `content_bottom = left_top - dy(8)`; `nav_width = max(dx(90), min(dx(130), client_width / 4))`;
/// - header at `(m, m)` with `nav_width` and `h = measure_item_height(HEADER_ID, nav_width)`; `y = m + h + dy(6)`;
/// - `version = if item_text(VERSION_ID).is_empty() { 0 } else { measure_item_height(VERSION_ID, nav_width) }`; `nav_bottom = content_bottom - (version > 0 ? version + dy(4) : 0)`;
///   navigation list at `(m, y)` with `nav_width` x `max(dy(40), nav_bottom - y)`; version at `(m, content_bottom - version)` (`nav_width` x `version`) when `version > 0`, else
///   `(m, content_bottom)` with height `0`;
/// - `right_x = m + nav_width + m`, `right_w = max(dx(100), client_width - right_x - m)`; the page title at `(right_x, m)`, `right_w` x `measure_item_height(PAGE_TITLE_ID,
///   right_w)`; `viewport_y = m + title_height + dy(6)`; `scroll_w = scroll_bar_width`; `viewport_width = max(dx(100), right_w - scroll_w - dx(4))`; `viewport_height = max(dy(40),
///   content_bottom - viewport_y)`;
/// - every page container at `(right_x, viewport_y)` with `viewport_width` x `viewport_height`; the scroll bar at `(right_x + right_w - scroll_w, viewport_y)` with `scroll_w` x
///   `viewport_height`.
pub fn layout_plan(
    m: &dyn DialogMeasure,
    client_width: i32,
    client_height: i32,
    page_count: usize,
) -> SettingsLayoutPlan {
    todo!()
}

impl SettingsContent {
    /// `PreferredClientSize` on the live dialog: `footer::preferred_client_size(dialog)` (`NativeDialog` implements `DialogMeasure`).
    pub(super) fn preferred_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        todo!()
    }

    /// `MinimumClientSize` on the live dialog.
    pub(super) fn minimum_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        todo!()
    }

    /// `LayoutControls`: `plan = layout_plan(dialog, w, h, page_count)`; every placement through `dialog.place(..)`, with `dialog.set_list_item_height(NAV_ID, plan.nav_item_height)` right
    /// after the placement of `NAV_ID`; `viewport_width` / `viewport_height` stored; when a page is showing (`active >= 0`) `layout_page(dialog, active)`.
    pub(super) fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, client_height: i32) {
        todo!()
    }
}
