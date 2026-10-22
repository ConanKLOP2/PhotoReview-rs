//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsPageBuilder.cs` lines 367-645 (baseline `d92d6aae`): `SettingsPageBuilder`, the declaration API of a
//! Settings page. Unit W8-19c. Contract: `contracts/W8-19c.md`.
//!
//! A page declares its controls in reading order (= Tab order) and gets the ids back. Ids of a page are `id_base + 1 ..= id_base + 999`; the frame hands out
//! `id_base` by page position, so two pages never share an id. Texts are `TextFn` providers (the frame calls them again when the language changes).
//!
//! Pure (it builds `DialogItem`s and layout elements; nothing here touches a window). The C# `throw`s (`InvalidOperationException` for the 1000th control,
//! `ArgumentException` for a bad radio group) are programmer errors and, because the UI thread never panics (R0003), become RECORDED errors
//! ([`SettingsPageBuilder::errors`]): the failing call returns zero ids and leaves exactly the state the C# call had left when it threw (items added before the throw
//! stay, no layout element is added, an invalid radio call changes nothing), then the next call goes on (D-W8B5-704).
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::groups::{CheckElement, CheckFlowElement, GroupElement, RadioElement};
use super::layout::{KeyGridElement, SettingsKeyCell, SettingsKeyCellSpec};
use super::rows::{
    ButtonsElement, LayoutElement, RowElement, RowKind, SettingsLayoutElement, SettingsRow, SettingsSliderRow,
    SliderElement, TextElement,
};
use crate::dialogs::native::template::{DIALOG_STYLE_CHILD, DialogControlClass, DialogItem};
use crate::dialogs::settings::page_view::TextFn;
use crate::win32::control_constants::{BS_AUTOCHECKBOX, BS_AUTORADIOBUTTON, BS_GROUPBOX, ES_READONLY, TBS_NOTICKS};
use crate::win32::dialog_chrome::BS_MULTILINE;
use crate::win32::dialog_native::{BS_PUSHBUTTON, ES_AUTOHSCROLL, SS_NOPREFIX, WS_EX_CLIENTEDGE, WS_GROUP, WS_TABSTOP};

/// C# `ClearGlyph`: the text of the key-clear button, the multiplication sign U+00D7 (it exists in Segoe UI, so there is no font fallback; the U+2715 of the WPF
/// build got a coloured ClearType fringe at 100 %).
pub const CLEAR_GLYPH: &str = "\u{00D7}";

/// How many controls one page can declare (C# `_counter > 999` throws).
pub const MAX_CONTROLS: i32 = 999;

/// What the C# builder throws, as a value (D-W8B5-704).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsBuilderError {
    /// The 1000th control (C# `InvalidOperationException("A settings page can declare at most 999 controls.")`).
    TooManyControls,
    /// A radio group with fewer than two options (C# `ArgumentException`, "A radio group needs at least two options.").
    RadioNeedsTwoOptions,
    /// A hint list whose length differs from the options (C# `ArgumentException`, "There must be one hint (or null) per radio option.").
    RadioHintCountMismatch,
}

impl fmt::Display for SettingsBuilderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooManyControls => "A settings page can declare at most 999 controls.",
            Self::RadioNeedsTwoOptions => "A radio group needs at least two options.",
            Self::RadioHintCountMismatch => "There must be one hint (or null) per radio option.",
        })
    }
}

impl std::error::Error for SettingsBuilderError {}

/// C# `SettingsPageBuilder`.
pub struct SettingsPageBuilder {
    id_base: i32,
    items: Vec<DialogItem>,
    /// C# `_containers`: the stack of element lists, `containers[0]` is the root.
    containers: Vec<Vec<LayoutElement>>,
    texts: BTreeMap<i32, TextFn>,
    notes: BTreeSet<i32>,
    key_boxes: BTreeSet<i32>,
    warnings: BTreeSet<i32>,
    combo_items: BTreeMap<i32, Vec<TextFn>>,
    sliders: BTreeMap<i32, (i32, i32)>,
    counter: i32,
    break_group: bool,
    errors: Vec<SettingsBuilderError>,
}

impl fmt::Debug for SettingsPageBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsPageBuilder")
            .field("id_base", &self.id_base)
            .field("items", &self.items.len())
            .field("errors", &self.errors)
            .finish_non_exhaustive()
    }
}

impl SettingsPageBuilder {
    /// C# `new SettingsPageBuilder(idBase)` (internal): the frame passes `1000 * (page index + 1)`.
    pub fn new(id_base: i32) -> Self {
        Self {
            id_base,
            items: Vec::new(),
            containers: vec![Vec::new()],
            texts: BTreeMap::new(),
            notes: BTreeSet::new(),
            key_boxes: BTreeSet::new(),
            warnings: BTreeSet::new(),
            combo_items: BTreeMap::new(),
            sliders: BTreeMap::new(),
            counter: 0,
            break_group: false,
            errors: Vec::new(),
        }
    }

    /// C# `IdBase`.
    pub fn id_base(&self) -> i32 {
        self.id_base
    }

    /// C# `Items`: the template items in declaration order (the Tab order; a group box is sent to the bottom of the z-order by the frame).
    pub fn items(&self) -> &[DialogItem] {
        &self.items
    }

    /// C# `Elements`: the root layout elements (groups hold their children).
    pub fn elements(&self) -> &[LayoutElement] {
        self.containers.first().map(Vec::as_slice).unwrap_or_default()
    }

    /// The mutable root elements (the frame flips `visible` of a group found by its title id).
    pub fn elements_mut(&mut self) -> &mut [LayoutElement] {
        self.containers.first_mut().map(Vec::as_mut_slice).unwrap_or_default()
    }

    /// C# `Texts`: id to text provider, only for controls declared with a text (the button providers are the `&` -> `&&` wrapped ones).
    pub fn texts(&self) -> &BTreeMap<i32, TextFn> {
        &self.texts
    }

    /// C# `Notes`: ids of the dimmed notes and radio hints.
    pub fn notes(&self) -> &BTreeSet<i32> {
        &self.notes
    }

    /// C# `KeyBoxes`: ids of the key-capture boxes (the frame subclasses them).
    pub fn key_boxes(&self) -> &BTreeSet<i32> {
        &self.key_boxes
    }

    /// C# `WarningTexts`: ids of the warning lines (painted in the warning colour).
    pub fn warning_texts(&self) -> &BTreeSet<i32> {
        &self.warnings
    }

    /// C# `ComboItems`: combo id to its static item providers (only combos declared with items).
    pub fn combo_items(&self) -> &BTreeMap<i32, Vec<TextFn>> {
        &self.combo_items
    }

    /// C# `Sliders`: slider id to `(min, max)`.
    pub fn sliders(&self) -> &BTreeMap<i32, (i32, i32)> {
        &self.sliders
    }

    /// The first recorded programmer error, or `None` (D-W8B5-704). The frame refuses to build a page whose builder has an error.
    pub fn error(&self) -> Option<SettingsBuilderError> {
        self.errors.first().copied()
    }

    /// EVERY violation in the order it happened (one entry per C# `throw`; the tests compare the count with the golden's `threw` entries).
    pub fn errors(&self) -> &[SettingsBuilderError] {
        &self.errors
    }

    /// Looks a group up by its title id in the whole tree (the frame's `_groupsById` + `Flatten`) and sets its `visible`; `false` when no group has that id.
    pub fn set_group_visible(&mut self, title_id: i32, visible: bool) -> bool {
        for element in self.elements_mut() {
            if set_group_visible_in(element.as_mut(), title_id, visible) {
                return true;
            }
        }
        false
    }

    /// The `visible` flag of the group with that title id, `None` when there is no such group.
    pub fn group_visible(&self, title_id: i32) -> Option<bool> {
        self.elements()
            .iter()
            .find_map(|element| group_visibility_in(element.as_ref(), title_id))
    }

    /// Every element of the tree, parents before children, in declaration order (C# `Flatten`).
    pub fn flatten(&self) -> Vec<&dyn super::rows::SettingsLayoutElement> {
        let mut flattened = Vec::new();
        for element in self.elements() {
            flatten_element(element.as_ref(), &mut flattened);
        }
        flattened
    }

    /// C# `Group(title, content)`: a titled `BS_GROUPBOX` (the `Button` class, style `Child | BS_GROUPBOX`, no tab stop) whose `content` declares the inner
    /// elements. Returns the group box id (to hide or show the whole group).
    pub fn group(&mut self, title: TextFn, content: impl FnOnce(&mut SettingsPageBuilder)) -> i32 {
        let Some(id) = self.add(
            DialogControlClass::Button,
            Some(title),
            DIALOG_STYLE_CHILD | BS_GROUPBOX,
            0,
        ) else {
            return 0;
        };
        self.containers.push(Vec::new());
        content(self);
        let children = self.containers.pop().unwrap_or_default();
        self.push_element(Box::new(GroupElement::new(id, children)));
        id
    }

    /// C# `Check(text, indentDlu = 0)`: `BS_AUTOCHECKBOX | BS_MULTILINE` with a tab stop; `indent_dlu` indents a dependent box.
    pub fn check(&mut self, text: TextFn, indent_dlu: i32) -> i32 {
        let Some(id) = self.add(
            DialogControlClass::Button,
            Some(text),
            DIALOG_STYLE_CHILD | WS_TABSTOP | BS_AUTOCHECKBOX | BS_MULTILINE,
            0,
        ) else {
            return 0;
        };
        self.push_element(Box::new(CheckElement::new(id, indent_dlu)));
        id
    }

    /// C# `Combo(label, items = null, widthDlu = 0)`: a label (Static) then a combo box (`Child | ComboBoxControl::DEFAULT_STYLE`). The ORDER of `items` is the
    /// contract with `SettingsFormState` (the index).
    pub fn combo(&mut self, label: TextFn, items: Option<Vec<TextFn>>, width_dlu: i32) -> SettingsRow {
        let Some(label_id) = self.add(
            DialogControlClass::Static,
            Some(label),
            DIALOG_STYLE_CHILD | SS_NOPREFIX,
            0,
        ) else {
            return SettingsRow { label: 0, control: 0 };
        };
        let Some(combo_id) = self.add(
            DialogControlClass::ComboBox,
            None,
            DIALOG_STYLE_CHILD | crate::dialogs::native::controls::basic::ComboBoxControl::DEFAULT_STYLE,
            0,
        ) else {
            return SettingsRow { label: 0, control: 0 };
        };
        if let Some(items) = items {
            self.combo_items.insert(combo_id, items);
        }
        self.push_element(Box::new(RowElement::new(label_id, combo_id, RowKind::Combo, width_dlu)));
        SettingsRow {
            label: label_id,
            control: combo_id,
        }
    }

    /// C# `Edit(label, widthDlu = 60)`: a label then a one-line edit (`ES_AUTOHSCROLL`, `WS_EX_CLIENTEDGE`, tab stop).
    pub fn edit(&mut self, label: TextFn, width_dlu: i32) -> SettingsRow {
        let Some(label_id) = self.add(
            DialogControlClass::Static,
            Some(label),
            DIALOG_STYLE_CHILD | SS_NOPREFIX,
            0,
        ) else {
            return SettingsRow { label: 0, control: 0 };
        };
        let Some(edit_id) = self.add(
            DialogControlClass::Edit,
            None,
            DIALOG_STYLE_CHILD | WS_TABSTOP | ES_AUTOHSCROLL,
            WS_EX_CLIENTEDGE,
        ) else {
            return SettingsRow { label: 0, control: 0 };
        };
        self.push_element(Box::new(RowElement::new(label_id, edit_id, RowKind::Edit, width_dlu)));
        SettingsRow {
            label: label_id,
            control: edit_id,
        }
    }

    /// C# `Slider(label, min, max)`: a label, a trackbar (`TBS_NOTICKS`, tab stop) and an empty value text (the page sets it). Records `(min, max)`.
    pub fn slider(&mut self, label: TextFn, min: i32, max: i32) -> SettingsSliderRow {
        let Some(label_id) = self.add(
            DialogControlClass::Static,
            Some(label),
            DIALOG_STYLE_CHILD | SS_NOPREFIX,
            0,
        ) else {
            return SettingsSliderRow {
                label: 0,
                slider: 0,
                value: 0,
            };
        };
        let Some(slider_id) = self.add(
            DialogControlClass::Trackbar,
            None,
            DIALOG_STYLE_CHILD | WS_TABSTOP | TBS_NOTICKS,
            0,
        ) else {
            return SettingsSliderRow {
                label: 0,
                slider: 0,
                value: 0,
            };
        };
        let Some(value_id) = self.add(DialogControlClass::Static, None, DIALOG_STYLE_CHILD | SS_NOPREFIX, 0) else {
            return SettingsSliderRow {
                label: 0,
                slider: 0,
                value: 0,
            };
        };
        self.sliders.insert(slider_id, (min, max));
        self.push_element(Box::new(SliderElement::new(label_id, slider_id, value_id)));
        SettingsSliderRow {
            label: label_id,
            slider: slider_id,
            value: value_id,
        }
    }

    /// C# `Note(text, aligned = true)`: a dimmed hint aligned with the control column (`aligned`) or full width; `text == None` = the page sets it later.
    pub fn note(&mut self, text: Option<TextFn>, aligned: bool) -> i32 {
        let Some(id) = self.add(DialogControlClass::Static, text, DIALOG_STYLE_CHILD | SS_NOPREFIX, 0) else {
            return 0;
        };
        self.notes.insert(id);
        self.push_element(Box::new(TextElement::new(id, aligned)));
        id
    }

    /// C# `Text(text = null)`: a plain full-width text (status, description).
    pub fn text(&mut self, text: Option<TextFn>) -> i32 {
        let Some(id) = self.add(DialogControlClass::Static, text, DIALOG_STYLE_CHILD | SS_NOPREFIX, 0) else {
            return 0;
        };
        self.push_element(Box::new(TextElement::new(id, false)));
        id
    }

    /// C# `Buttons(aligned, params texts)`: push buttons (tab stops) that wrap when the width runs out. Returns the ids in order.
    pub fn buttons(&mut self, aligned: bool, texts: &[TextFn]) -> Vec<i32> {
        let mut ids = Vec::with_capacity(texts.len());
        let mut failed = false;
        for text in texts {
            match self.add(
                DialogControlClass::Button,
                Some(text.clone()),
                DIALOG_STYLE_CHILD | WS_TABSTOP | BS_PUSHBUTTON,
                0,
            ) {
                Some(id) => ids.push(id),
                None => {
                    ids.resize(texts.len(), 0);
                    failed = true;
                    break;
                }
            }
        }
        if !failed {
            self.push_element(Box::new(ButtonsElement::new(ids.clone(), aligned)));
        }
        ids
    }

    /// C# `Radio(texts, hints = null, indentDlu = 0)`: `BS_AUTORADIOBUTTON | BS_MULTILINE`; the FIRST radio has `WS_GROUP | WS_TABSTOP`; the control declared
    /// next opens a new group (`WS_GROUP`). `hints` (one entry per radio, `None` = no hint) are dimmed notes created AFTER every radio. Returns the radio ids in
    /// option order. Fewer than two options or a hint list of another length records an error and returns `texts.len()` zeros (D-W8B5-704).
    pub fn radio(&mut self, texts: &[TextFn], hints: Option<&[Option<TextFn>]>, indent_dlu: i32) -> Vec<i32> {
        if texts.len() < 2 {
            self.errors.push(SettingsBuilderError::RadioNeedsTwoOptions);
            return vec![0; texts.len()];
        }
        if hints.is_some_and(|values| values.len() != texts.len()) {
            self.errors.push(SettingsBuilderError::RadioHintCountMismatch);
            return vec![0; texts.len()];
        }
        let mut ids = Vec::with_capacity(texts.len());
        for (index, text) in texts.iter().enumerate() {
            let mut style = DIALOG_STYLE_CHILD | BS_AUTORADIOBUTTON | BS_MULTILINE;
            if index == 0 {
                style |= WS_GROUP | WS_TABSTOP;
            }
            match self.add(DialogControlClass::Button, Some(text.clone()), style, 0) {
                Some(id) => ids.push(id),
                None => {
                    return vec![0; texts.len()];
                }
            }
        }
        self.break_group = true;
        let mut hint_ids = vec![None; texts.len()];
        if let Some(hints) = hints {
            for (index, hint) in hints.iter().enumerate() {
                if let Some(text) = hint {
                    let Some(id) = self.add(
                        DialogControlClass::Static,
                        Some(text.clone()),
                        DIALOG_STYLE_CHILD | SS_NOPREFIX,
                        0,
                    ) else {
                        return vec![0; texts.len()];
                    };
                    self.notes.insert(id);
                    hint_ids[index] = Some(id);
                }
            }
        }
        for (index, id) in ids.iter().enumerate() {
            self.push_element(Box::new(RadioElement::new(
                *id,
                hint_ids.get(index).copied().flatten(),
                indent_dlu,
            )));
        }
        ids
    }

    /// C# `CheckFlow(texts, indentDlu = 0)`: auto check boxes (tab stops) that wrap like a `WrapPanel`. Returns the ids in order.
    pub fn check_flow(&mut self, texts: &[TextFn], indent_dlu: i32) -> Vec<i32> {
        let mut ids = Vec::with_capacity(texts.len());
        let mut failed = false;
        for text in texts {
            match self.add(
                DialogControlClass::Button,
                Some(text.clone()),
                DIALOG_STYLE_CHILD | WS_TABSTOP | BS_AUTOCHECKBOX,
                0,
            ) {
                Some(id) => ids.push(id),
                None => {
                    ids.resize(texts.len(), 0);
                    failed = true;
                    break;
                }
            }
        }
        if !failed {
            self.push_element(Box::new(CheckFlowElement::new(ids.clone(), indent_dlu)));
        }
        ids
    }

    /// C# `Warning(text = null)`: a full-width line painted in the warning colour; takes no room while empty.
    pub fn warning(&mut self, text: Option<TextFn>) -> i32 {
        let Some(id) = self.add(DialogControlClass::Static, text, DIALOG_STYLE_CHILD | SS_NOPREFIX, 0) else {
            return 0;
        };
        self.warnings.insert(id);
        self.push_element(Box::new(TextElement::new(id, false)));
        id
    }

    /// C# `KeyGrid(specs)`: per spec a label (Static), a read-only edit (`ES_READONLY | ES_AUTOHSCROLL`, `WS_EX_CLIENTEDGE`, tab stop, registered as a key box)
    /// and, when `clearable`, a push button with [`CLEAR_GLYPH`] (tab stop). Creation order = Tab order (label, box, button).
    pub fn key_grid(&mut self, specs: &[SettingsKeyCellSpec]) -> Vec<SettingsKeyCell> {
        let mut cells = Vec::with_capacity(specs.len());
        let mut failed = false;
        for spec in specs {
            let Some(label_id) = self.add(
                DialogControlClass::Static,
                Some(spec.label.clone()),
                DIALOG_STYLE_CHILD | SS_NOPREFIX,
                0,
            ) else {
                failed = true;
                break;
            };
            let Some(box_id) = self.add(
                DialogControlClass::Edit,
                None,
                DIALOG_STYLE_CHILD | WS_TABSTOP | ES_AUTOHSCROLL | ES_READONLY,
                WS_EX_CLIENTEDGE,
            ) else {
                failed = true;
                break;
            };
            self.key_boxes.insert(box_id);
            let clear_id = if spec.clearable {
                let glyph: TextFn = std::rc::Rc::new(|| CLEAR_GLYPH.to_owned());
                let Some(id) = self.add(
                    DialogControlClass::Button,
                    Some(glyph),
                    DIALOG_STYLE_CHILD | WS_TABSTOP | BS_PUSHBUTTON,
                    0,
                ) else {
                    failed = true;
                    break;
                };
                id
            } else {
                0
            };
            cells.push(SettingsKeyCell {
                label: label_id,
                box_id,
                clear: clear_id,
            });
        }
        if !failed {
            self.push_element(Box::new(KeyGridElement::new(cells.clone())));
        }
        cells
    }

    fn add(&mut self, class: DialogControlClass, text: Option<TextFn>, mut style: u32, ex_style: u32) -> Option<i32> {
        self.counter = self.counter.saturating_add(1);
        if self.counter > MAX_CONTROLS {
            self.errors.push(SettingsBuilderError::TooManyControls);
            return None;
        }
        let id = self.id_base.saturating_add(self.counter);
        let is_group_box = class == DialogControlClass::Button && style & 0x0f == BS_GROUPBOX;
        if self.break_group && !is_group_box {
            style |= WS_GROUP;
            self.break_group = false;
        }
        let provider = if class == DialogControlClass::Button {
            text.map(|provider| -> TextFn { std::rc::Rc::new(move || provider().replace('&', "&&")) })
        } else {
            text
        };
        let value = provider.as_ref().map_or_else(String::new, |provider| provider());
        self.items
            .push(DialogItem::new(class, id, value, style).with_ex_style(ex_style));
        if let Some(provider) = provider {
            self.texts.insert(id, provider);
        }
        Some(id)
    }

    fn push_element(&mut self, element: LayoutElement) {
        if let Some(container) = self.containers.last_mut() {
            container.push(element);
        }
    }
}

fn flatten_element<'a>(element: &'a dyn SettingsLayoutElement, output: &mut Vec<&'a dyn SettingsLayoutElement>) {
    output.push(element);
    if let Some(group) = element.as_group() {
        for child in group.children() {
            flatten_element(child.as_ref(), output);
        }
    }
}

fn group_visibility_in(element: &dyn SettingsLayoutElement, title_id: i32) -> Option<bool> {
    let group = element.as_group()?;
    if group.title_id() == title_id {
        return Some(element.visible());
    }
    group
        .children()
        .iter()
        .find_map(|child| group_visibility_in(child.as_ref(), title_id))
}

fn set_group_visible_in(element: &mut dyn SettingsLayoutElement, title_id: i32, visible: bool) -> bool {
    let Some(group) = element.as_group_mut() else {
        return false;
    };
    if group.title_id() == title_id {
        element.set_visible(visible);
        return true;
    }
    for child in group.children_mut() {
        if set_group_visible_in(child.as_mut(), title_id, visible) {
            return true;
        }
    }
    false
}
