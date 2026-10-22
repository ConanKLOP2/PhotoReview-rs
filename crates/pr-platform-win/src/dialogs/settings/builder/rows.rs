//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsPageBuilder.cs` lines 1-74 and 124-201 and 203-240 (baseline `d92d6aae`): the row records, the measuring
//! surface, the base class of every layout element and the row/slider/text/button-row elements. Unit W8-19b. Contract: `contracts/W8-19b.md`.
//!
//! Pure: no OS call. Every element places the controls of ONE declared row on a [`SettingsLayoutSurface`] (the real surface is the Settings frame, W8-19j; the tests
//! use a fake) and returns the height it used, in pixels, including the gap below it. All arithmetic is `i32` and saturating (the C# `int` wraps).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use super::groups::GroupElement;

/// C# `SettingsRow(Label, Control)`: the ids of a "label + control" row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SettingsRow {
    pub label: i32,
    pub control: i32,
}

/// C# `SettingsSliderRow(Label, Slider, Value)`: the ids of a "label + trackbar + value text" row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SettingsSliderRow {
    pub label: i32,
    pub slider: i32,
    pub value: i32,
}

/// C# `ISettingsLayoutSurface`: what a layout element measures and places on. All sizes are pixels, `dx`/`dy` convert dialog units to pixels.
///
/// The queries take `&self`, the two writes `&mut self` (the real surface records the placement and moves the window; the fake records into a map).
pub trait SettingsLayoutSurface {
    /// Dialog units to horizontal pixels.
    fn dx(&self, dialog_units: i32) -> i32;
    /// Dialog units to vertical pixels.
    fn dy(&self, dialog_units: i32) -> i32;
    /// One text line in pixels.
    fn line_height(&self) -> i32;
    /// Height of an edit box, combo box or push button in pixels.
    fn field_height(&self) -> i32;
    /// The current text of the control (the declared text provider's output, or what the page set).
    fn text_of(&self, id: i32) -> String;
    /// The pixel height of the text of `id` when it wraps in `width`.
    fn text_height(&self, id: i32, width: i32) -> i32;
    /// The pixel width of `text` on one line.
    fn text_width(&self, text: &str) -> i32;
    /// The control was hidden by its page (`SettingsPageContext::set_visible`): it takes no room.
    fn is_hidden(&self, id: i32) -> bool;
    /// The extra height of the drop-down list of a combo box (counted in the combo's window height).
    fn combo_drop_height(&self, id: i32) -> i32;
    /// Moves/sizes the control `id`.
    fn place(&mut self, id: i32, x: i32, y: i32, width: i32, height: i32);
    /// Hides the control `id`.
    fn hide(&mut self, id: i32);
}

/// The owned element type of a page's layout tree (C# `List<SettingsLayoutElement>`).
pub type LayoutElement = Box<dyn SettingsLayoutElement>;

/// C# `SettingsLayoutElement` (abstract class): a group, a row, a check box, a note, a button row... `place` puts its controls at the given width and returns the
/// HEIGHT used (including the gap below); a hidden element returns 0. The layout is recomputed on every resize, DPI change, language change and scroll.
pub trait SettingsLayoutElement {
    /// C# `Visible` (default `true`). `false` = the element and its children are hidden and take no room.
    fn visible(&self) -> bool;

    /// Sets C# `Visible` (the frame does it for a group, `SettingsPageContext::set_visible` on a group box id).
    fn set_visible(&mut self, visible: bool);

    /// C# `ControlIds`: every control id of the element, children included (a group lists its title id first).
    fn control_ids(&self) -> Vec<i32>;

    /// C# `PlaceVisible`: places the controls of a VISIBLE element at (`x`, `y`) in `width` and returns the height used.
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32;

    /// `is SettingsGroupElement group` of the C# frame (`_groupsById`, `Flatten`).
    fn as_group(&self) -> Option<&GroupElement> {
        None
    }

    /// The mutable twin, so the frame can toggle a group's visibility by its title id.
    fn as_group_mut(&mut self) -> Option<&mut GroupElement> {
        None
    }

    /// C# `Place(surface, x, y, width)`: when not [`visible`](Self::visible), `surface.hide(id)` for EVERY id of `control_ids()` in order and return 0; else
    /// `place_visible(...)`.
    fn place(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        todo!()
    }
}

/// C# `LabelWidth(surface, width)`: the label column of the rows, about 38 % of the width, clamped to 80..=140 dialog units
/// (`clamp(width * 38 / 100, dx(80), dx(140))`; long Vietnamese labels wrap).
///
/// The clamp must not panic when `dx(80) > dx(140)` (`i32::clamp` does): use `max` then `min` (D-W8B5-703).
pub fn label_width(surface: &dyn SettingsLayoutSurface, width: i32) -> i32 {
    todo!()
}

/// C# `SettingsRowKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Combo,
    Edit,
}

/// C# `SettingsRowElement(labelId, controlId, kind, widthDlu)`: a label (left column) and one control (right column): combo box, edit, button.
#[derive(Debug, Clone)]
pub struct RowElement {
    visible: bool,
    label_id: i32,
    control_id: i32,
    kind: RowKind,
    width_dlu: i32,
}

impl RowElement {
    pub fn new(label_id: i32, control_id: i32, kind: RowKind, width_dlu: i32) -> Self {
        todo!()
    }
}

impl SettingsLayoutElement for RowElement {
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

/// C# `SettingsSliderElement(labelId, sliderId, valueId)`: label, trackbar and the value text on its right.
#[derive(Debug, Clone)]
pub struct SliderElement {
    visible: bool,
    label_id: i32,
    slider_id: i32,
    value_id: i32,
}

impl SliderElement {
    pub fn new(label_id: i32, slider_id: i32, value_id: i32) -> Self {
        todo!()
    }
}

impl SettingsLayoutElement for SliderElement {
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

/// C# `SettingsTextElement(id, alignedToControls)`: multi-line text, either a dimmed note aligned with the control column or a full-width status line. An
/// empty text (or a hidden control) takes no room.
#[derive(Debug, Clone)]
pub struct TextElement {
    visible: bool,
    id: i32,
    aligned_to_controls: bool,
}

impl TextElement {
    pub fn new(id: i32, aligned_to_controls: bool) -> Self {
        todo!()
    }
}

impl SettingsLayoutElement for TextElement {
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

/// C# `SettingsButtonsElement(ids, alignedToControls)`: a row of push buttons that wraps when the width runs out (hidden buttons are skipped).
#[derive(Debug, Clone)]
pub struct ButtonsElement {
    visible: bool,
    ids: Vec<i32>,
    aligned_to_controls: bool,
}

impl ButtonsElement {
    pub fn new(ids: Vec<i32>, aligned_to_controls: bool) -> Self {
        todo!()
    }
}

impl SettingsLayoutElement for ButtonsElement {
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
