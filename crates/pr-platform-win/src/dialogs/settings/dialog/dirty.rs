//! Port of `SettingsDialog.cs` lines 1097 to 1181 and 1418 (the control accessors that `SettingsPageContext` calls, `SetControlVisible`, `IsControlVisible`,
//! `FocusControl`, `RelayoutActivePage`), baseline `d92d6aae`. Unit W8-19k. Contract: `contracts/W8-19k.md`.
//!
//! Module name from the plan (`state, dirty`): in the baseline every one of these calls that changes what a page looks like ends in `RelayoutActivePage` (the C# `_dirty`
//! array they also set is write-only and is NOT ported, D-W8B5-907). Every accessor takes the live `&NativeDialog`; an absent control (`item(id) == 0`) is a no-op / the neutral
//! answer, as in C#.

use super::frame::SettingsContent;
use crate::dialogs::native::dialog::NativeDialog;
use crate::dialogs::settings::page_view::TextFn;
use std::rc::Rc;
use crate::dialogs::native::controls::{basic::ComboBoxControl,custom::{ButtonStateControl,TrackbarControl}};
use crate::win32::dialog_native::user32;

/// C# `SetControlText`: a button's text is shown with `&` doubled (no accelerator): `"&"` -> `"&&"` (ordinal). Any other control gets the text as is. Pure.
pub fn control_text_for(is_button: bool, text: &str) -> String {
    if is_button {text.replace('&',"&&")}else{text.to_owned()}
}

/// C# `SetComboItems` clamp: the index to select for `count` items: `-1` when `count == 0`, else `clamp(selected, 0, count - 1)`. Pure.
pub fn combo_selection(count: usize, selected: i32) -> i32 {
    if count==0 {-1}else{selected.max(0).min(i32::try_from(count-1).unwrap_or(i32::MAX))}
}

/// C# `SetSliderRange` step: `max(1, (max - min) / 10)`. Pure.
pub fn slider_page_size(min: i32, max: i32) -> i32 {
    (max.wrapping_sub(min)/10).max(1)
}

impl SettingsContent {
    /// C# `GetControlText(id)` = `dialog.get_item_text(id)`.
    pub fn get_control_text(&self, dialog: &NativeDialog, id: i32) -> String {
        dialog.get_item_text(id)
    }

    /// C# `SetControlText(id, text)` = `dialog.set_item_text(id, control_text_for(is_button(item(id)), text))`.
    pub fn set_control_text(&self, dialog: &NativeDialog, id: i32, text: &str) {
        dialog.set_item_text(id,&control_text_for(NativeDialog::is_button(dialog.item(id)),text));
    }

    /// C# `IsControlChecked(id)` = `ButtonStateControl::is_checked(item(id))`.
    pub fn is_control_checked(&self, dialog: &NativeDialog, id: i32) -> bool {
        ButtonStateControl::is_checked(dialog.item(id))
    }

    /// C# `SetControlChecked(id, value)`.
    pub fn set_control_checked(&self, dialog: &NativeDialog, id: i32, value: bool) {
        ButtonStateControl::set_checked(dialog.item(id),value);
    }

    /// C# `ComboSelectedIndex(id)`.
    pub fn combo_selected_index(&self, dialog: &NativeDialog, id: i32) -> i32 {
        let control=dialog.item(id);if control==0 {-1}else{ComboBoxControl::selected_index(control)}
    }

    /// C# `SetComboSelectedIndex(id, index)`.
    pub fn set_combo_selected_index(&self, dialog: &NativeDialog, id: i32, index: i32) {
        ComboBoxControl::select(dialog.item(id),index);
    }

    /// C# `SetComboItems(id, items, selected)`: clear, add each, select `combo_selection(items.len(), selected)`.
    pub fn set_combo_items(&self, dialog: &NativeDialog, id: i32, items: &[String], selected: i32) {
        let control=dialog.item(id);ComboBoxControl::clear(control);
        for item in items {ComboBoxControl::add(control,item);}
        ComboBoxControl::select(control,combo_selection(items.len(),selected));
    }

    /// C# `SliderValue(id)`.
    pub fn slider_value(&self, dialog: &NativeDialog, id: i32) -> i32 {
        TrackbarControl::position(dialog.item(id))
    }

    /// C# `SetSliderValue(id, value)`.
    pub fn set_slider_value(&self, dialog: &NativeDialog, id: i32, value: i32) {
        TrackbarControl::set_position(dialog.item(id),value);
    }

    /// C# `SetSliderRange(id, min, max)`: reads the position, `TrackbarControl::configure(slider, min, max, 1, 1, slider_page_size(min, max))`, writes back
    /// `clamp(position, min, max)` (written with `max` then `min` so `min > max` cannot panic).
    pub fn set_slider_range(&self, dialog: &NativeDialog, id: i32, min: i32, max: i32) {
        let control=dialog.item(id);let position=TrackbarControl::position(control);
        TrackbarControl::configure(control,min,max,1,1,slider_page_size(min,max));
        TrackbarControl::set_position(control,position.max(min).min(max));
    }

    /// C# `SetControlTooltip(id, Func<string>)` = `dialog.set_item_tooltips(id, |_| Some(text()))`: the text is asked again each time the tip is shown.
    pub fn set_control_tooltip_fn(&self, dialog: &NativeDialog, id: i32, text: TextFn) {
        dialog.set_item_tooltips(id,Rc::new(move |_|Some(text())));
    }

    /// C# `SetControlTooltip(id, string)` = `dialog.set_tooltip(id, text)`: fixed text, set once. (The page host has no fixed-text method: `SettingsPageContext::set_tooltip_text`
    /// reaches the window as a constant `TextFn` through `set_control_tooltip_fn`; this one is for the handle and the tests.)
    pub fn set_control_tooltip_text(&self, dialog: &NativeDialog, id: i32, text: &str) {
        dialog.set_tooltip(id,text);
    }

    /// C# `SetControlEnabled(id, enabled)` = `enable_window(item(id), enabled)`.
    pub fn set_control_enabled(&self, dialog: &NativeDialog, id: i32, enabled: bool) {
        user32::enable_window(dialog.item(id),enabled);
    }

    /// C# `IsControlEnabled(id)` = `is_window_enabled(item(id))`.
    pub fn is_control_enabled(&self, dialog: &NativeDialog, id: i32) -> bool {
        user32::is_window_enabled(dialog.item(id))
    }

    /// C# `SetControlVisible(id, visible)`: when `id` is the title id of a group (any page's builder answers `set_group_visible(id, visible)` with `true`) only the group's
    /// flag changes; else `hidden` gains (`visible == false`) or loses `id`. Then `relayout_active_page`. The builders are reached with `try_borrow_mut`: `layout_page` holds a
    /// SHARED borrow of one builder while it places controls, and a page that reacts to a notification raised by those OS calls must not make this call panic; a builder that is
    /// borrowed is skipped (the request is dropped; unreachable on a healthy run, covered by an in-file test).
    pub fn set_control_visible(&self, dialog: &NativeDialog, id: i32, visible: bool) {
        let mut group=false;
        for builder in &self.builders {if let Ok(mut builder)=builder.try_borrow_mut(){if builder.set_group_visible(id,visible){group=true;break;}}}
        if !group {if visible {self.view.hidden.borrow_mut().remove(&id);}else{self.view.hidden.borrow_mut().insert(id);}}
        self.relayout_active_page(dialog);
    }

    /// C# `IsControlVisible(id)`: a group's own flag (`group_visible(id)` of the first builder that knows it, reached with `try_borrow`: a borrowed builder is skipped), else
    /// `!hidden.contains(id)`. No window needed.
    pub fn is_control_visible(&self, id: i32) -> bool {
        for builder in &self.builders {if let Ok(builder)=builder.try_borrow(){if let Some(visible)=builder.group_visible(id){return visible;}}}
        !self.view.hidden.borrow().contains(&id)
    }

    /// C# `FocusControl(id)`: `item(id) == 0` -> nothing; else `ensure_visible(id)` then `set_focus(control)`.
    pub fn focus_control(&self, dialog: &NativeDialog, id: i32) {
        let control=dialog.item(id);if control!=0 {self.ensure_visible(dialog,id);user32::set_focus(control);}
    }

    /// C# `RelayoutActivePage()`: `active >= 0` -> `dialog.run_layout()`; else nothing.
    pub fn relayout_active_page(&self, dialog: &NativeDialog) {
        if self.view.active.get()>=0 {dialog.run_layout();}
    }
}
