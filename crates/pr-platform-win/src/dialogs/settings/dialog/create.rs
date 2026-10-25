//! Port of `SettingsDialog.cs` lines 333 to 438 (`EnsurePageCreated`, `EnsureAllPagesCreated`, `IsPageCreated`), baseline `d92d6aae`. Unit W8-19j.
//! Contract: `contracts/W8-19j.md`.
//!
//! The controls of a page are created the first time the page is shown (the window opens without paying for the six other pages: milestone "open <= 150 ms").
//! `unsafe` here: `create_window_ex` (the window procedure of the control class runs inside the call; `param` is `0`).

use super::frame::SettingsContent;
use crate::dialogs::native::controls::{basic::ComboBoxControl, custom::TrackbarControl};
use crate::dialogs::native::dialog::NativeDialog;
use crate::dialogs::native::{
    chrome::DialogChromeControl,
    template::{DialogControlClass, window_class_name},
};
use crate::win32::api::{kernel32, user32};

impl SettingsContent {
    /// C# `EnsurePageCreated(index)`. `index` out of range, the page already created, or `dialog.hwnd() == 0` -> nothing (the "created" flag is NOT set when there is no
    /// window). Else, in this order:
    /// 1. `created = true`; `viewport = dialog.item(viewport_id(index))`; the items, the text providers and the declared sets are COPIED out of the builder (no borrow is held later);
    /// 2. per item, in declaration order: `text` = the provider of `builder.texts()[id]` called NOW (not the text captured at `declare`: the language may have changed), else
    ///    `item.text`; `control = create_window_ex(item.ex_style, window_class_name(item.class), text, item.style | WS_CLIPSIBLINGS, 0, 0, 0, 0, viewport, item.id, module, 0)`;
    ///    `Err` -> skip the item (the C# `continue`); `dialog.register_child(control)`; a `Button` with `(style & 0xF) == BS_GROUPBOX` is remembered as a group box; the
    ///    `(control, class)` pair goes to the chrome list; a `ComboBox` joins `page_combos` and gets the wheel-only forwarding subclass (`messages::forward(.., false)`); a single-line `Edit`
    ///    (`style & ES_MULTILINE == 0`) gets `edit_centering::attach`;
    /// 3. every group box is moved to the BOTTOM of the z-order (`set_window_pos(gb, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)`), one at a time, after all
    ///    controls exist (a group box declared before its controls would otherwise clip them away);
    /// 4. `dialog.apply_chrome(&controls)`;
    /// 5. every combo of `builder.combo_items()`: `clear`, one `add` per provider (called now), `select(0)`; every slider of `builder.sliders()`: `TrackbarControl::configure(item(id),
    ///    min, max, 1, 1, max(1, (max - min) / 10))`; every id of `builder.key_boxes()`: `hook_key_box(dialog, item(id))`;
    /// 6. `page.attach(context.clone())`; then, with `loading` set, `page.load(&working_fields())`; `loading` is restored on every path (a guard);
    /// 7. a page that is already borrowed (re-entrancy) is skipped at 6 (D-W8B5-904).
    pub fn ensure_page_created(&self, dialog: &NativeDialog, index: usize) {
        if dialog.hwnd() == 0 || self.is_page_created(index) {
            return;
        }
        let Some(builder) = self.builders.get(index) else {
            return;
        };
        let Ok(declaration) = builder.try_borrow() else {
            return;
        };
        let items = declaration.items().to_vec();
        let texts = declaration.texts().clone();
        let combos = declaration.combo_items().clone();
        let sliders = declaration.sliders().clone();
        let key_boxes = declaration.key_boxes().clone();
        drop(declaration);
        let Ok(mut pages) = self.view.pages.try_borrow_mut() else {
            return;
        };
        let Some(page) = pages.get_mut(index) else {
            return;
        };
        page.created = true;
        drop(pages);
        let viewport = dialog.item(super::frame::viewport_id(index as i32));
        let instance = kernel32::get_module_handle().unwrap_or(0);
        let mut controls = Vec::new();
        let mut group_boxes = Vec::new();
        for item in items {
            let text = texts
                .get(&item.id)
                .map_or_else(|| item.text.clone(), |provider| provider());
            // SAFETY: class/text are borrowed strings kept alive through the call; integer styles/ids and OS-validated handles, with no creation parameter.
            let control = unsafe {
                user32::create_window_ex(
                    item.ex_style,
                    window_class_name(item.class),
                    &text,
                    page_control_style(item.style),
                    0,
                    0,
                    0,
                    0,
                    viewport,
                    item.id as isize,
                    instance,
                    0,
                )
            };
            let Ok(control) = control else {
                continue;
            };
            dialog.register_child(control);
            if item.class == DialogControlClass::Button && is_group_box_style(item.style) {
                group_boxes.push(control);
            }
            controls.push(DialogChromeControl {
                hwnd: control,
                class: item.class,
            });
            if item.class == DialogControlClass::ComboBox {
                self.view.page_combos.borrow_mut().insert(control);
                self.forward(dialog, control, false);
            } else if item.class == DialogControlClass::Edit && item.style & 4 == 0 {
                crate::dialogs::native::edit_centering::attach(control);
            }
        }
        for group_box in group_boxes {
            let _ = user32::set_window_pos(group_box, 1, 0, 0, 0, 0, 0x0002 | 0x0001 | 0x0010);
        }
        dialog.apply_chrome(&controls);
        for (id, texts) in combos {
            let combo = dialog.item(id);
            ComboBoxControl::clear(combo);
            for text in texts {
                ComboBoxControl::add(combo, &text());
            }
            ComboBoxControl::select(combo, 0);
        }
        for (id, (min, max)) in sliders {
            TrackbarControl::configure(
                dialog.item(id),
                min,
                max,
                1,
                1,
                super::dirty::slider_page_size(min, max),
            );
        }
        for id in key_boxes {
            self.hook_key_box(dialog, dialog.item(id));
        }
        if let Some(page) = self.pages.get(index) {
            if let Ok(mut page) = page.try_borrow_mut() {
                page.attach(self.context.clone());
                let _loading = super::state::LoadingGuard::new(self);
                page.load(&self.working_fields());
            }
        }
    }

    /// C# `EnsureAllPagesCreated()`: `ensure_page_created` for every index in order.
    pub fn ensure_all_pages_created(&self, dialog: &NativeDialog) {
        for index in 0..self.pages.len() {
            self.ensure_page_created(dialog, index);
        }
    }

    /// C# `IsPageCreated(index)`; `false` out of range.
    pub fn is_page_created(&self, index: usize) -> bool {
        self.view.pages.borrow().get(index).is_some_and(|page| page.created)
    }
}

/// Is `style` a group box (`BS_GROUPBOX` = `7` in the low nibble of a `Button`)? C# `(item.Style & 0xFu) == ControlConstants.BsGroupBox`. Pure.
pub fn is_group_box_style(style: u32) -> bool {
    style & 0x0f == 7
}

/// The style a page control is created with: `item_style | WS_CLIPSIBLINGS` (`0x0400_0000`). Pure.
pub fn page_control_style(item_style: u32) -> u32 {
    item_style | 0x0400_0000
}

/// Is `hwnd` a control that was created for a page? Test helper used by the e2e tests: `dialog.item(id) != 0` (kept here so the tests need no OS knowledge).
pub fn control_exists(dialog: &NativeDialog, id: i32) -> bool {
    dialog.item(id) != 0
}
