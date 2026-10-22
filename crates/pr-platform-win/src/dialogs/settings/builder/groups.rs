//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsPageBuilder.cs` lines 76-122 and 242-307 (baseline `d92d6aae`): the layout elements that group or
//! stack controls: the group box, the check box, the radio (with its hint) and the wrapping row of check boxes. Unit W8-19b. Contract: `contracts/W8-19b.md`.
//!
//! Pure, same conventions as [`super::rows`] (saturating `i32`, a hidden element returns 0).
use std::fmt;

use super::rows::{LayoutElement, SettingsLayoutElement, SettingsLayoutSurface};

fn add(a: i32, b: i32) -> i32 {
    a.saturating_add(b)
}
fn sub(a: i32, b: i32) -> i32 {
    a.saturating_sub(b)
}
fn mul(a: i32, b: i32) -> i32 {
    a.saturating_mul(b)
}

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
        Self {
            visible: true,
            title_id,
            children,
        }
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
        let mut ids = vec![self.title_id];
        for child in &self.children {
            ids.extend(child.control_ids());
        }
        ids
    }
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        let pad = surface.dx(8);
        let top = add(surface.line_height(), surface.dy(5));
        let ix = add(x, pad);
        let iw = sub(width, mul(2, pad)).max(surface.dx(40));
        let mut cursor = add(y, top);
        for child in &self.children {
            cursor = add(cursor, child.place(surface, ix, cursor, iw));
        }
        cursor = add(cursor, surface.dy(2));
        surface.place(self.title_id, x, y, width, sub(cursor, y));
        add(sub(cursor, y), surface.dy(8))
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
        Self {
            visible: true,
            id,
            indent_dlu,
        }
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
        vec![self.id]
    }
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        if surface.is_hidden(self.id) {
            surface.hide(self.id);
            return 0;
        }
        let indent = surface.dx(self.indent_dlu);
        let tw = sub(sub(width, indent), surface.dx(14)).max(surface.dx(20));
        let h = add(surface.text_height(self.id, tw), surface.dy(1)).max(add(surface.line_height(), surface.dy(2)));
        surface.place(self.id, add(x, indent), y, sub(width, indent), h);
        add(h, surface.dy(4))
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
        Self {
            visible: true,
            id,
            hint_id,
            indent_dlu,
        }
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
        let mut ids = vec![self.id];
        if let Some(id) = self.hint_id {
            ids.push(id);
        }
        ids
    }
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        if surface.is_hidden(self.id) {
            surface.hide(self.id);
            if let Some(id) = self.hint_id {
                surface.hide(id);
            }
            return 0;
        }
        let indent = surface.dx(self.indent_dlu);
        let tw = sub(sub(width, indent), surface.dx(14)).max(surface.dx(20));
        let h = add(surface.text_height(self.id, tw), surface.dy(1)).max(add(surface.line_height(), surface.dy(2)));
        surface.place(self.id, add(x, indent), y, sub(width, indent), h);
        let mut used = add(h, surface.dy(2));
        if let Some(hint) = self.hint_id {
            let hi = add(indent, surface.dx(12));
            let hw = sub(width, hi).max(surface.dx(30));
            let hh = surface.text_height(hint, hw);
            surface.place(hint, add(x, hi), add(y, used), hw, hh);
            used = add(used, add(hh, surface.dy(2)));
        }
        add(used, surface.dy(4))
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
        Self {
            visible: true,
            ids,
            indent_dlu,
        }
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
        self.ids.clone()
    }
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        let left = add(x, surface.dx(self.indent_dlu));
        let right = add(x, width);
        let gap = surface.dx(10);
        let h = add(surface.line_height(), surface.dy(2));
        let mut cx = left;
        let mut cy = y;
        for id in &self.ids {
            let iw = surface.text_width(&surface.text_of(*id)).saturating_add(surface.dx(16));
            if cx > left && add(cx, iw) > right {
                cx = left;
                cy = add(cy, add(h, surface.dy(4)));
            }
            surface.place(*id, cx, cy, iw.min(sub(right, left)), h);
            cx = add(cx, add(iw, gap));
        }
        add(add(sub(cy, y), h), surface.dy(5))
    }
}
