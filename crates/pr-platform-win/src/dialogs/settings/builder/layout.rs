//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsPageBuilder.cs` lines 309-365 (baseline `d92d6aae`): the key-capture grid of the Shortcuts page
//! (`SettingsKeyCell`, `SettingsKeyCellSpec`, `SettingsKeyGridElement`). Unit W8-19c. Contract: `contracts/W8-19c.md`.
//!
//! Pure, same conventions as [`super::rows`]. The grid lays "label + key box (+ clear button)" cells out row by row in one or two columns.
use std::fmt;

use super::rows::{SettingsLayoutElement, SettingsLayoutSurface};
use crate::dialogs::settings::page_view::TextFn;

/// C# `SettingsKeyCell(Label, Box, Clear)`: the ids of one declared cell. `clear == 0` = no clear button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SettingsKeyCell {
    pub label: i32,
    pub box_id: i32,
    pub clear: i32,
}

/// C# `SettingsKeyCellSpec(Func<string> Label, bool Clearable)`: what a page asks of the grid for one cell.
#[derive(Clone)]
pub struct SettingsKeyCellSpec {
    pub label: TextFn,
    pub clearable: bool,
}

impl fmt::Debug for SettingsKeyCellSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsKeyCellSpec")
            .field("clearable", &self.clearable)
            .finish_non_exhaustive()
    }
}

/// C# `SettingsKeyGridElement(cells)`: 1 or 2 columns by width, the key boxes aligned in columns (also in rows without a clear button), labels wrap.
#[derive(Debug, Clone)]
pub struct KeyGridElement {
    visible: bool,
    cells: Vec<SettingsKeyCell>,
}

impl KeyGridElement {
    pub fn new(cells: Vec<SettingsKeyCell>) -> Self {
        Self { visible: true, cells }
    }

    /// C# `Cells`.
    pub fn cells(&self) -> &[SettingsKeyCell] {
        &self.cells
    }
}

impl SettingsLayoutElement for KeyGridElement {
    fn visible(&self) -> bool {
        self.visible
    }
    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
    /// Per cell in order: `label, box` and, when `clear != 0`, `clear` (C# `ControlIds`).
    fn control_ids(&self) -> Vec<i32> {
        let mut ids = Vec::with_capacity(self.cells.len().saturating_mul(3));
        for cell in &self.cells {
            ids.push(cell.label);
            ids.push(cell.box_id);
            if cell.clear != 0 {
                ids.push(cell.clear);
            }
        }
        ids
    }
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        if !self.visible || self.cells.is_empty() {
            return 0;
        }

        let cell_min = surface.dx(160);
        let column_gap = surface.dx(12);
        let divisor = cell_min.saturating_add(column_gap);
        let columns = if divisor == 0 {
            1
        } else {
            width
                .saturating_add(column_gap)
                .checked_div(divisor)
                .unwrap_or(1)
                .clamp(1, 2)
        };
        let cell_width = surface
            .dx(100)
            .max(width.saturating_sub(columns.saturating_sub(1).saturating_mul(column_gap)) / columns);
        let gap = surface.dx(4);
        let box_width = surface.dx(40).max(cell_width / 2).min(surface.dx(64));
        let field_height = surface.field_height();
        let clear_width = field_height;
        let label_width = surface.dx(30).max(
            cell_width
                .saturating_sub(box_width)
                .saturating_sub(clear_width)
                .saturating_sub(gap.saturating_mul(2)),
        );

        let mut cursor = y;
        let step = usize::try_from(columns).unwrap_or(1);
        let mut start = 0usize;
        while start < self.cells.len() {
            let end = start.saturating_add(step).min(self.cells.len());
            let mut row_height = field_height;
            for cell in self.cells.get(start..end).unwrap_or_default() {
                row_height = row_height.max(surface.text_height(cell.label, label_width));
            }
            for (column, cell) in self.cells.get(start..end).unwrap_or_default().iter().enumerate() {
                let column = i32::try_from(column).unwrap_or(i32::MAX);
                let left = x.saturating_add(column.saturating_mul(cell_width.saturating_add(column_gap)));
                let label_height = surface.text_height(cell.label, label_width);
                surface.place(
                    cell.label,
                    left,
                    cursor.saturating_add(row_height.saturating_sub(label_height) / 2),
                    label_width,
                    label_height,
                );
                let box_left = left.saturating_add(label_width).saturating_add(gap);
                let field_y = cursor.saturating_add(row_height.saturating_sub(field_height) / 2);
                surface.place(cell.box_id, box_left, field_y, box_width, field_height);
                if cell.clear != 0 {
                    surface.place(
                        cell.clear,
                        box_left.saturating_add(box_width).saturating_add(gap),
                        field_y,
                        clear_width,
                        field_height,
                    );
                }
            }
            cursor = cursor.saturating_add(row_height).saturating_add(surface.dy(5));
            start = end;
        }
        cursor.saturating_sub(y)
    }
}
