//! Port of `SettingsDialog.cs` lines 1412 to 1416 (`StaticTextColor`) and 1424 to 1512 (the nested class `PageSurface`), baseline `d92d6aae`. Unit W8-19m.
//! Contract: `contracts/W8-19m.md`.
//!
//! `PageSurface` is the real [`SettingsLayoutSurface`] that the layout elements of a page measure and place on: it reads texts and sizes from the dialog, remembers the last
//! rectangle of every page control (one `SetWindowPos` per control and none when nothing changed: a page of about 80 controls is laid out on every resize, scroll and
//! language change) and hides controls that were never placed.

use super::frame::SettingsContent;
use crate::dialogs::native::dialog::NativeDialog;
use crate::dialogs::settings::builder::rows::SettingsLayoutSurface;

/// How a STATIC is coloured (C# `StaticTextColor` branches).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticTextKind {
    /// A dimmed note or radio hint: `DialogColors::from_key("Dark.TextMuted")`.
    Muted,
    /// A warning line: `DialogColors::warning()`.
    Warning,
    /// Everything else: the base answer, `DialogColors::text()`.
    Normal,
}

/// C# `StaticTextColor` decision: a note wins over a warning (the C# conditional order), pure.
pub fn static_text_kind(is_note: bool, is_warning: bool) -> StaticTextKind {
    if is_note {
        StaticTextKind::Muted
    } else if is_warning {
        StaticTextKind::Warning
    } else {
        StaticTextKind::Normal
    }
}

/// C# `ComboDropHeight`: `count = clamp(item_count, 1, 12)`; `item_height <= 0` is replaced by `line_height + dy(2)`; the result `count * item_height + dy(4)`. Saturating. Pure.
pub fn combo_drop_height(item_count: i32, item_height: i32, line_height: i32, dy2: i32, dy4: i32) -> i32 {
    item_count
        .clamp(1, 12)
        .saturating_mul(if item_height <= 0 {
            line_height.saturating_add(dy2)
        } else {
            item_height
        })
        .saturating_add(dy4)
}

/// C# `Place` cache decision: `Some(rect)` is the new rectangle `(x, y, max(0, width), max(0, height))` to apply, `None` when it equals the last one (nothing to do). Pure.
pub fn placement_change(
    last: Option<(i32, i32, i32, i32)>,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Option<(i32, i32, i32, i32)> {
    let rect = (x, y, width.max(0), height.max(0));
    (last != Some(rect)).then_some(rect)
}

impl SettingsContent {
    /// C# `StaticTextColor(id)`: `static_text_kind(declared.notes.contains(id), declared.warning_texts.contains(id))` -> the colour.
    pub(super) fn static_text_color(&self, _dialog: &NativeDialog, id: i32) -> u32 {
        use crate::dialogs::native::chrome::DialogColors;
        match static_text_kind(
            self.declared.notes.contains(&id),
            self.declared.warning_texts.contains(&id),
        ) {
            StaticTextKind::Muted => {
                DialogColors::from_key("Dark.TextMuted").unwrap_or_else(DialogColors::text_secondary)
            }
            StaticTextKind::Warning => DialogColors::warning(),
            StaticTextKind::Normal => DialogColors::text(),
        }
    }
}

/// C# `PageSurface(SettingsDialog)`. Lives for one layout pass; borrows the content and the dialog.
pub struct PageSurface<'a> {
    content: &'a SettingsContent,
    dialog: &'a NativeDialog,
    /// C# `Origin`: `-scroll` of the page being laid out (the layout elements receive `origin + cursor` as `y` already; the field is kept for parity and tests).
    pub origin: i32,
}

impl<'a> std::fmt::Debug for PageSurface<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageSurface").finish_non_exhaustive()
    }
}

impl<'a> PageSurface<'a> {
    /// A surface with `origin == 0`.
    pub fn new(content: &'a SettingsContent, dialog: &'a NativeDialog) -> Self {
        Self {
            content,
            dialog,
            origin: 0,
        }
    }
}

impl SettingsLayoutSurface for PageSurface<'_> {
    /// `dialog.dx(dialog_units)`.
    fn dx(&self, dialog_units: i32) -> i32 {
        self.dialog.dx(dialog_units)
    }

    /// `dialog.dy(dialog_units)`.
    fn dy(&self, dialog_units: i32) -> i32 {
        self.dialog.dy(dialog_units)
    }

    /// `dialog.line_height()`.
    fn line_height(&self) -> i32 {
        self.dialog.line_height()
    }

    /// `max(dy(14), line_height + dy(6))`.
    fn field_height(&self) -> i32 {
        super::footer::field_height(self.dialog)
    }

    /// `dialog.get_item_text(id)`.
    fn text_of(&self, id: i32) -> String {
        self.dialog.get_item_text(id)
    }

    /// `dialog.measure_item_height(id, width)`.
    fn text_height(&self, id: i32, width: i32) -> i32 {
        self.dialog.measure_item_height(id, width)
    }

    /// `dialog.measure_text_width(text, 0)` (the body font).
    fn text_width(&self, text: &str) -> i32 {
        self.dialog.measure_text_width(text, 0)
    }

    /// `content.view.hidden.contains(id)` (groups are not in the set: their elements are invisible themselves).
    fn is_hidden(&self, id: i32) -> bool {
        self.content.view.hidden.borrow().contains(&id)
    }

    /// C# `ComboDropHeight`: `combo = dialog.item(id)`, `0` when absent; `item_height = send_message(combo, CB_GETITEMHEIGHT = 0x0154, 0, 0)`; `combo_drop_height(ComboBoxControl::count(combo),
    /// item_height, line_height, dy(2), dy(4))`.
    fn combo_drop_height(&self, id: i32) -> i32 {
        let combo = self.dialog.item(id);
        if combo == 0 {
            return 0;
        }
        // SAFETY: CB_GETITEMHEIGHT carries integer arguments only, to an OS-validated control handle.
        let height = unsafe { crate::win32::dialog_native::user32::send_message(combo, 0x0154, 0, 0) } as i32;
        combo_drop_height(
            crate::dialogs::native::controls::basic::ComboBoxControl::count(combo),
            height,
            self.line_height(),
            self.dy(2),
            self.dy(4),
        )
    }

    /// C# `Place(id, x, y, width, height)`: control absent -> nothing; id hidden -> `hide(id)` and stop; `placement_change(placed[control], ..)`: `None` -> nothing; `Some(r)` ->
    /// ONE `set_window_pos(control, 0, r.0, r.1, r.2, r.3, SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW)`, `placed[control] = r`, and for a page combobox (`page_combos`)
    /// `invalidate_rect(control, None, false)` (a dropdown-list combobox shrinks itself to its closed height INSIDE the move and the paint that move queued is lost).
    fn place(&mut self, id: i32, x: i32, y: i32, width: i32, height: i32) {
        let control = self.dialog.item(id);
        if control == 0 {
            return;
        }
        if self.is_hidden(id) {
            self.hide(id);
            return;
        }
        let last = self.content.view.placed.borrow().get(&control).copied();
        let Some(rect) = placement_change(last, x, y, width, height) else {
            return;
        };
        let _ = crate::win32::api::user32::set_window_pos(
            control,
            0,
            rect.0,
            rect.1,
            rect.2,
            rect.3,
            0x0004 | 0x0010 | 0x0040,
        );
        self.content.view.placed.borrow_mut().insert(control, rect);
        let is_combo = self.content.view.page_combos.borrow().contains(&control);
        if is_combo {
            crate::win32::dialog_native::user32::invalidate_rect(control, None, false);
        }
    }

    /// C# `Hide(id)`: control absent -> nothing; `was_placed = placed.remove(control)`; `style_visible = get_window_long_ptr(control, GWL_STYLE) & WS_VISIBLE != 0`;
    /// `was_placed || style_visible` -> `show_window(control, SW_HIDE)` (a control is created `WS_VISIBLE` at 0,0 and a never-placed hidden group must still be hidden).
    fn hide(&mut self, id: i32) {
        let control = self.dialog.item(id);
        if control == 0 {
            return;
        }
        let placed = self.content.view.placed.borrow_mut().remove(&control).is_some();
        let visible = crate::win32::api::user32::get_window_long_ptr(control, -16) as u32 & 0x1000_0000 != 0;
        if placed || visible {
            crate::win32::api::user32::show_window(control, 0);
        }
    }
}
