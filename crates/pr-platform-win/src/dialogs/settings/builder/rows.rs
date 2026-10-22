//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsPageBuilder.cs` lines 1-74 and 124-201 and 203-240 (baseline `d92d6aae`): the row records, the measuring
//! surface, the base class of every layout element and the row/slider/text/button-row elements. Unit W8-19b. Contract: `contracts/W8-19b.md`.
//!
//! Pure: no OS call. Every element places the controls of ONE declared row on a [`SettingsLayoutSurface`] (the real surface is the Settings frame, W8-19j; the tests
//! use a fake) and returns the height it used, in pixels, including the gap below it. All arithmetic is `i32` and saturating (the C# `int` wraps).
use super::groups::GroupElement;

fn add(a: i32, b: i32) -> i32 {
    a.saturating_add(b)
}
fn sub(a: i32, b: i32) -> i32 {
    a.saturating_sub(b)
}
fn mul(a: i32, b: i32) -> i32 {
    a.saturating_mul(b)
}
fn clamp(v: i32, lo: i32, hi: i32) -> i32 {
    v.max(lo).min(hi)
}

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
        if !self.visible() {
            for id in self.control_ids() {
                surface.hide(id);
            }
            0
        } else {
            self.place_visible(surface, x, y, width)
        }
    }
}

/// C# `LabelWidth(surface, width)`: the label column of the rows, about 38 % of the width, clamped to 80..=140 dialog units
/// (`clamp(width * 38 / 100, dx(80), dx(140))`; long Vietnamese labels wrap).
///
/// The clamp must not panic when `dx(80) > dx(140)` (`i32::clamp` does): use `max` then `min` (D-W8B5-703).
pub fn label_width(surface: &dyn SettingsLayoutSurface, width: i32) -> i32 {
    clamp(mul(width, 38) / 100, surface.dx(80), surface.dx(140))
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
        Self {
            visible: true,
            label_id,
            control_id,
            kind,
            width_dlu,
        }
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
        vec![self.label_id, self.control_id]
    }
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        if surface.is_hidden(self.control_id) {
            surface.hide(self.label_id);
            surface.hide(self.control_id);
            return 0;
        }
        let lw = label_width(surface, width);
        let gap = surface.dx(6);
        let cx = add(add(x, lw), gap);
        let available = sub(sub(width, lw), gap).max(surface.dx(30));
        let cw = if self.width_dlu > 0 {
            available.min(surface.dx(self.width_dlu))
        } else {
            available.min(surface.dx(240))
        };
        let ch = surface.field_height();
        let lh = surface.text_height(self.label_id, lw);
        let rh = ch.max(lh);
        surface.place(self.label_id, x, add(y, sub(rh, lh) / 2), lw, lh);
        let wh = if self.kind == RowKind::Combo {
            add(ch, surface.combo_drop_height(self.control_id))
        } else {
            ch
        };
        surface.place(self.control_id, cx, add(y, sub(rh, ch) / 2), cw, wh);
        add(rh, surface.dy(5))
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
        Self {
            visible: true,
            label_id,
            slider_id,
            value_id,
        }
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
        vec![self.label_id, self.slider_id, self.value_id]
    }
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        let lw = label_width(surface, width);
        let gap = surface.dx(6);
        let sx = add(add(x, lw), gap);
        let vw = surface
            .text_width(&surface.text_of(self.value_id))
            .max(surface.text_width("100 %"))
            .saturating_add(surface.dx(6))
            .max(surface.dx(34));
        let sw = clamp(
            sub(sub(sub(width, lw), gap), add(vw, gap)),
            surface.dx(60),
            surface.dx(150),
        );
        let sh = surface.field_height().max(add(surface.line_height(), surface.dy(8)));
        let lh = surface.text_height(self.label_id, lw);
        let rh = sh.max(lh);
        surface.place(self.label_id, x, add(y, sub(rh, lh) / 2), lw, lh);
        surface.place(self.slider_id, sx, add(y, sub(rh, sh) / 2), sw, sh);
        let vh = surface.text_height(self.value_id, vw);
        surface.place(self.value_id, add(add(sx, sw), gap), add(y, sub(rh, vh) / 2), vw, vh);
        add(rh, surface.dy(5))
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
        Self {
            visible: true,
            id,
            aligned_to_controls,
        }
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
        vec![self.id]
    }
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        if surface.is_hidden(self.id) || surface.text_of(self.id).is_empty() {
            surface.hide(self.id);
            return 0;
        }
        let indent = if self.aligned_to_controls {
            add(label_width(surface, width), surface.dx(6))
        } else {
            0
        };
        let tw = sub(width, indent).max(surface.dx(30));
        let h = surface.text_height(self.id, tw);
        surface.place(self.id, add(x, indent), y, tw, h);
        add(h, surface.dy(5))
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
        Self {
            visible: true,
            ids,
            aligned_to_controls,
        }
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
        self.ids.clone()
    }
    fn place_visible(&self, surface: &mut dyn SettingsLayoutSurface, x: i32, y: i32, width: i32) -> i32 {
        let indent = if self.aligned_to_controls {
            add(label_width(surface, width), surface.dx(6))
        } else {
            0
        };
        let left = add(x, indent);
        let right = add(x, width);
        let gap = surface.dx(6);
        let h = surface.field_height();
        let mut cx = left;
        let mut cy = y;
        let mut any = false;
        for id in &self.ids {
            if surface.is_hidden(*id) {
                surface.hide(*id);
                continue;
            }
            let bw = surface
                .text_width(&surface.text_of(*id))
                .saturating_add(surface.dx(16))
                .max(surface.dx(50));
            if cx > left && add(cx, bw) > right {
                cx = left;
                cy = add(cy, add(h, surface.dy(4)));
            }
            surface.place(*id, cx, cy, bw, h);
            cx = add(cx, add(bw, gap));
            any = true;
        }
        if any { add(add(sub(cy, y), h), surface.dy(5)) } else { 0 }
    }
}
