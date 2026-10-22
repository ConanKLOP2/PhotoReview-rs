//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsPageBuilder.cs` lines 76-122 and 242-307 (baseline `d92d6aae`): the layout elements that group or
//! stack controls: the group box, the check box, the radio (with its hint) and the wrapping row of check boxes. Unit W8-19b. Contract: `contracts/W8-19b.md`.
//!
//! Pure, same conventions as [`super::rows`] (saturating `i32`, a hidden element returns 0).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::fmt;

use super::rows::{LayoutElement, SettingsLayoutElement, SettingsLayoutSurface};

/// C# `SettingsGroupElement(titleId, children)`: a group box around its child elements. Its title id is the id of the `BS_GROUPBOX` control.
pub struct GroupElement {
    visible: bool,
    title_id: i32,
    children: Vec<LayoutElement>,
}

impl fmt::Debug for GroupElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GroupElement")
            .field("visible", &self.visible)
            .field("title_id", &self.title_id)
            .field("children", &self.children.len())
            .finish()
    }
}

impl GroupElement {
    pub fn new(title_id: i32, children: Vec<LayoutElement>) -> Self {
        todo!()
    }

    /// C# `TitleId`.
    pub fn title_id(&self) -> i32 {
        self.title_id
    }

    /// C# `Children`.
    pub fn children(&self) -> &[LayoutElement] {
        &self.children
    }

    /// The mutable children (the frame flips the `visible` flag of a nested group by its title id).
    pub fn children_mut(&mut self) -> &mut [LayoutElement] {
        &mut self.children
    }
}

impl SettingsLayoutElement for GroupElement {
    fn visible(&self) -> bool {
        self.visible
    }
    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
    /// The title id FIRST, then the ids of the children in order (C# `children.SelectMany(...).Prepend(titleId)`).
    fn control_ids(&self) -> Vec<i32> {
        todo!()
    }
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        todo!()
    }
    fn as_group(&self) -> Option<&GroupElement> {
        Some(self)
    }
    fn as_group_mut(&mut self) -> Option<&mut GroupElement> {
        Some(self)
    }
}

/// C# `SettingsCheckElement(id, indentDlu)`: a check box, optionally indented and wrapping.
#[derive(Debug, Clone)]
pub struct CheckElement {
    visible: bool,
    id: i32,
    indent_dlu: i32,
}

impl CheckElement {
    pub fn new(id: i32, indent_dlu: i32) -> Self {
        todo!()
    }
}

impl SettingsLayoutElement for CheckElement {
    fn visible(&self) -> bool {
        self.visible
    }
    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
    fn control_ids(&self) -> Vec<i32> {
        todo!()
    }
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        todo!()
    }
}

/// C# `SettingsRadioElement(id, hintId, indentDlu)`: one radio button (indentable, wrapping) with an optional dimmed hint right below it (indented further).
#[derive(Debug, Clone)]
pub struct RadioElement {
    visible: bool,
    id: i32,
    hint_id: Option<i32>,
    indent_dlu: i32,
}

impl RadioElement {
    pub fn new(id: i32, hint_id: Option<i32>, indent_dlu: i32) -> Self {
        todo!()
    }
}

impl SettingsLayoutElement for RadioElement {
    fn visible(&self) -> bool {
        self.visible
    }
    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
    /// `[id, hint]` when there is a hint, else `[id]`.
    fn control_ids(&self) -> Vec<i32> {
        todo!()
    }
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        todo!()
    }
}

/// C# `SettingsCheckFlowElement(ids, indentDlu)`: check boxes that wrap by width (the WPF `WrapPanel`).
#[derive(Debug, Clone)]
pub struct CheckFlowElement {
    visible: bool,
    ids: Vec<i32>,
    indent_dlu: i32,
}

impl CheckFlowElement {
    pub fn new(ids: Vec<i32>, indent_dlu: i32) -> Self {
        todo!()
    }
}

impl SettingsLayoutElement for CheckFlowElement {
    fn visible(&self) -> bool {
        self.visible
    }
    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
    fn control_ids(&self) -> Vec<i32> {
        todo!()
    }
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        todo!()
    }
}
