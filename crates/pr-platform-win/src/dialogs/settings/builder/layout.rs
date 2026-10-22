//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsPageBuilder.cs` lines 309-365 (baseline `d92d6aae`): the key-capture grid of the Shortcuts page
//! (`SettingsKeyCell`, `SettingsKeyCellSpec`, `SettingsKeyGridElement`). Unit W8-19c. Contract: `contracts/W8-19c.md`.
//!
//! Pure, same conventions as [`super::rows`]. The grid lays "label + key box (+ clear button)" cells out row by row in one or two columns.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

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
        todo!()
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
        todo!()
    }
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        todo!()
    }
}
