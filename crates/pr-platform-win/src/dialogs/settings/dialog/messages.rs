//! Port of `SettingsDialog.cs` lines 909 to 1095 (`Forward`, `ForwardProc`, `OnWindowMessage`, `PostToUi`, `OnLocalizerChanged`, `RefreshLocalizedTexts`,
//! `ApplyTextsToControls`), baseline `d92d6aae`. Unit W8-19l. Contract: `contracts/W8-19l.md`.
//!
//! How messages reach the frame: the page containers are subclassed so that what a child sends to its "parent" (a container has no dialog procedure) is forwarded to the
//! dialog; the navigation list and the page comboboxes are subclassed for the mouse wheel only (the list is a direct child of the dialog); the mouse wheel drives the page scroll; work posted from any thread is queued and run on the UI thread; a language change relabels everything.
//!
//! `unsafe` here: `post_message` (an `unsafe fn` of `win32::api`: `WindowPoster::post`, `post_language_changed`, `post_local`; integer-only messages), `set_window_subclass` /
//! `remove_window_subclass` / `def_subclass_proc` (unsafe comctl wrappers), [`forward_proc`] (a `SUBCLASSPROC`; `ref_data` is the registry key of the content, D-W8B5-908) and the `send_message` that forwards a notification whose
//! `lParam` points to a structure owned by the sender for the duration of the call.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, Ordering};

use super::frame::SettingsContent;
use crate::dialogs::native::dialog::NativeDialog;
use crate::dialogs::settings::context::{ContextUiAction, SettingsUiPost};
use crate::win32::structs::{Hwnd, Lparam, Lresult, Wparam};

/// UI-thread-only work posted to the window itself (not `Send`): runs with the content and the live dialog. The one user is the external-editor focus of `initialize`.
pub type LocalAction = Box<dyn FnOnce(&SettingsContent, &NativeDialog)>;

/// C# `_queue` + `PostToUi` + the window handle, as a `Send + Sync` object (D-W8B5-709): workers hold it through `SettingsPageContext::ui_post()`.
pub struct WindowPoster {
    hwnd: AtomicIsize,
    queue: Mutex<VecDeque<ContextUiAction>>,
}

impl std::fmt::Debug for WindowPoster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowPoster").finish_non_exhaustive()
    }
}

impl WindowPoster {
    /// A poster with no window (every `post` is dropped until `set_hwnd`).
    pub fn new() -> Self {
        Self {
            hwnd: AtomicIsize::new(0),
            queue: Mutex::new(VecDeque::new()),
        }
    }

    /// Sets the window the messages go to (`initialize`), `0` when it closes (`close::on_closed`; also clears the queue: queued actions of a closed window are dropped).
    pub fn set_hwnd(&self, hwnd: Hwnd) {
        let mut queue = self.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.hwnd.store(hwnd, Ordering::Release);
        if hwnd == 0 {
            queue.clear();
        }
    }

    /// The window (`0` = none).
    pub fn hwnd(&self) -> Hwnd {
        self.hwnd.load(Ordering::Acquire)
    }

    /// Takes every queued action in FIFO order (the queue is empty afterwards). The mutex is held only for the swap, never while an action runs.
    pub fn drain(&self) -> Vec<ContextUiAction> {
        self.queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect()
    }

    /// How many actions wait (tests).
    pub fn pending(&self) -> usize {
        self.queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// C# `OnLocalizerChanged`: `hwnd != 0` -> `post_message(hwnd, WM_APP_LANGUAGE, 0, 0)` (any thread); the error is ignored.
    pub fn post_language_changed(&self) {
        let hwnd = self.hwnd();
        if hwnd != 0 {
            // SAFETY: integer-only wake message, no borrowed data; the OS validates the window handle.
            let _ = unsafe { crate::win32::api::user32::post_message(hwnd, super::frame::WM_APP_LANGUAGE, 0, 0) };
        }
    }
}

impl Default for WindowPoster {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsUiPost for WindowPoster {
    /// C# `PostToUi`: `hwnd == 0` -> the action is dropped; else it is queued and `post_message(hwnd, WM_APP_QUEUE, 0, 0)` wakes the window (a failed post leaves the action
    /// queued: the next wake runs it). Safe from any thread.
    fn post(&self, action: ContextUiAction) {
        let hwnd = {
            let mut queue = self.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let hwnd = self.hwnd();
            if hwnd == 0 {
                return;
            }
            queue.push_back(action);
            hwnd
        };
        // SAFETY: integer-only wake message, no borrowed data; queued actions are owned by the poster.
        let _ = unsafe { crate::win32::api::user32::post_message(hwnd, super::frame::WM_APP_QUEUE, 0, 0) };
    }
}

/// C# `(short)((long)wParam >> 16 & 0xFFFF)`: the signed high word of a `WM_MOUSEWHEEL` `wParam` (the wheel delta; 120 = one notch). Pure.
pub fn wheel_delta(wparam: Wparam) -> i32 {
    ((wparam >> 16) as u16 as i16) as i32
}

/// Does `ForwardProc` send this message on to the dialog? Pure. `wheel_only` is `subclass_id == WHEEL_SUBCLASS_ID`.
/// - `WM_MOUSEWHEEL`: `!combo_is_dropped` (a dropped list scrolls itself) for both subclass kinds;
/// - else only for the "forward everything" kind: `WM_COMMAND`, `WM_NOTIFY`, `WM_DRAWITEM`, `WM_MEASUREITEM`, `WM_HSCROLL`, `WM_VSCROLL`, and every `WM_CTLCOLOR*`
///   (`0x0132 ..= 0x0138`);
/// - everything else: `false`.
pub fn is_forwarded(message: u32, wheel_only: bool, combo_is_dropped: bool) -> bool {
    if message == 0x020a {
        return !combo_is_dropped;
    }
    !wheel_only
        && matches!(
            message,
            0x0111 | 0x004e | 0x002b | 0x002c | 0x0114 | 0x0115 | 0x0132..=0x0138
        )
}

impl SettingsContent {
    /// C# `Forward(control, everything)`: `control == 0` -> nothing; else `set_window_subclass(control, forward_proc, everything ? FORWARD_ALL_SUBCLASS_ID : WHEEL_SUBCLASS_ID,
    /// self.subclass_key.get())`; a `false` result is ignored.
    pub(super) fn forward(&self, _dialog: &NativeDialog, control: Hwnd, everything: bool) {
        if control == 0 {
            return;
        }
        let id = if everything {
            super::frame::FORWARD_ALL_SUBCLASS_ID
        } else {
            super::frame::WHEEL_SUBCLASS_ID
        };
        // SAFETY: static SUBCLASSPROC with matching signature, UI-thread handle and registry key (never a pointer), removed on WM_NCDESTROY.
        unsafe {
            crate::win32::comctl::common_controls::set_window_subclass(
                control,
                forward_proc,
                id,
                self.subclass_key.get(),
            );
        }
    }

    /// C# `OnWindowMessage` (lines 969 to 991): `WM_MOUSEWHEEL` -> `scroll_by_wheel(dialog, wheel_delta(wparam))`, `Some(0)`; `WM_APP_QUEUE` -> run the local queue first (one action
    /// at a time, popped under a short borrow), then every action of `poster.drain()` with `&self.context`, `Some(0)`; `WM_APP_LANGUAGE` -> `refresh_localized_texts`, `Some(0)`;
    /// anything else `None`.
    pub(super) fn handle_window_message(
        &self,
        dialog: &NativeDialog,
        message: u32,
        wparam: Wparam,
        _lparam: Lparam,
    ) -> Option<Lresult> {
        match message {
            0x020a => {
                self.scroll_by_wheel(dialog, wheel_delta(wparam));
                Some(0)
            }
            super::frame::WM_APP_QUEUE => {
                loop {
                    let action = self.local_queue.borrow_mut().pop_front();
                    let Some(action) = action else {
                        break;
                    };
                    action(self, dialog);
                }
                for action in self.poster.drain() {
                    action(&self.context);
                }
                Some(0)
            }
            super::frame::WM_APP_LANGUAGE => {
                self.refresh_localized_texts(dialog);
                Some(0)
            }
            _ => None,
        }
    }

    /// Queues `action` for the UI thread of the window and wakes it (`post_message(hwnd, WM_APP_QUEUE)`). `hwnd == 0` -> dropped. Not `Send` (see [`LocalAction`]).
    pub fn post_local(&self, dialog: &NativeDialog, action: LocalAction) {
        let hwnd = dialog.hwnd();
        if hwnd == 0 {
            return;
        }
        self.local_queue.borrow_mut().push_back(action);
        // SAFETY: integer-only wake message; the action is owned by this content, no pointer crosses the queue.
        let _ = unsafe { crate::win32::api::user32::post_message(hwnd, super::frame::WM_APP_QUEUE, 0, 0) };
    }

    /// C# `RefreshLocalizedTexts()` (lines 1016 to 1082): `hwnd == 0` or already running (`in_language_refresh`, restored on every path) -> nothing. Then, in order:
    /// `set_window_text(hwnd, title())`; `apply_texts_to_controls`; the header and the seven buttons (`set_item_text` with `tr::settings_header`, `settings_restore_defaults`,
    /// `settings_reset_page`, `settings_export_settings`, `settings_import_settings`, `common_cancel`, `common_apply`, `common_save`); the navigation list (`clear`, one `add` per
    /// page title, `select_single(active)` and the page title text when `active >= 0`); every declared combo whose control exists (`item(id) != 0`): remember the selection, `clear`,
    /// `add` each provider, `select(max(0, selected))`; `on_language_changed()` of every CREATED page (skipped when borrowed); then `dialog.run_layout()`.
    pub fn refresh_localized_texts(&self, dialog: &NativeDialog) {
        use super::frame::*;
        use crate::dialogs::native::controls::basic::{ComboBoxControl, ListBoxControl};
        if dialog.hwnd() == 0 || self.view.in_language_refresh.replace(true) {
            return;
        }
        struct RefreshGuard<'a>(&'a std::cell::Cell<bool>);
        impl Drop for RefreshGuard<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _refresh = RefreshGuard(&self.view.in_language_refresh);
        let _ =
            crate::win32::dialog_native::user32::set_window_text(dialog.hwnd(), &pr_l10n::tr::settings_window_title());
        self.apply_texts_to_controls(dialog);
        for (id, text) in [
            (HEADER_ID, pr_l10n::tr::settings_header()),
            (DEFAULTS_ID, pr_l10n::tr::settings_restore_defaults()),
            (RESET_PAGE_ID, pr_l10n::tr::settings_reset_page()),
            (EXPORT_ID, pr_l10n::tr::settings_export_settings()),
            (IMPORT_ID, pr_l10n::tr::settings_import_settings()),
            (CANCEL_ID, pr_l10n::tr::common_cancel()),
            (APPLY_ID, pr_l10n::tr::common_apply()),
            (SAVE_ID, pr_l10n::tr::common_save()),
        ] {
            dialog.set_item_text(id, &text);
        }
        let nav = dialog.item(NAV_ID);
        ListBoxControl::clear(nav);
        for page in &self.pages {
            if let Ok(page) = page.try_borrow() {
                ListBoxControl::add(nav, &page.title());
            }
        }
        let active = self.view.active.get();
        if active >= 0 {
            ListBoxControl::select_single(nav, active);
            if let Some(page) = self.page(active) {
                if let Ok(page) = page.try_borrow() {
                    dialog.set_item_text(PAGE_TITLE_ID, &page.title());
                }
            }
        }
        for (id, items) in &self.declared.combo_items {
            let combo = dialog.item(*id);
            if combo == 0 {
                continue;
            }
            let selected = ComboBoxControl::selected_index(combo);
            ComboBoxControl::clear(combo);
            for text in items {
                ComboBoxControl::add(combo, &text());
            }
            ComboBoxControl::select(combo, selected.max(0));
        }
        for (index, page) in self.pages.iter().enumerate() {
            if self.is_page_created(index) {
                if let Ok(mut page) = page.try_borrow_mut() {
                    page.on_language_changed();
                }
            }
        }
        dialog.run_layout();
    }

    /// C# `ApplyTextsToControls()`: for every builder, for every `(id, provider)` of `texts()`: `set_item_text(id, provider())` (the providers are cloned out of the builder first).
    pub(super) fn apply_texts_to_controls(&self, dialog: &NativeDialog) {
        for builder in &self.builders {
            let texts = builder.try_borrow().ok().map(|builder| builder.texts().clone());
            if let Some(texts) = texts {
                for (id, text) in texts {
                    dialog.set_item_text(id, &text());
                }
            }
        }
    }
}

/// C# `ForwardProc` (`[UnmanagedCallersOnly]`). The content is `from_subclass_key(ref_data)`; with a live window: a `WM_CTLCOLORSTATIC` for a key box (`is_key_box(lparam)`) in the
/// "forward everything" kind answers `key_box_colors(wparam)`; `is_forwarded(..)` and `hwnd != dialog hwnd` -> `send_message(dialog hwnd, message, wparam, lparam)` and that result;
/// `WM_NCDESTROY` removes the subclass; everything else (and a missing content) goes to `def_subclass_proc`. The body runs under `pending_panic::guarded`.
///
/// # Safety
/// Only the OS calls it, as a `SUBCLASSPROC` installed by `forward`, with the arguments of the window message.
pub(super) unsafe extern "system" fn forward_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
    id: usize,
    ref_data: usize,
) -> Lresult {
    use crate::win32::comctl::common_controls;
    let handled = crate::host::pending_panic::guarded(
        None,
        std::panic::AssertUnwindSafe(|| {
            if message == crate::win32::messages::WM_NCDESTROY {
                common_controls::remove_window_subclass(hwnd, forward_proc, id);
                return None;
            }
            let content = SettingsContent::from_subclass_key(ref_data)?;
            content
                .with_native(|dialog| {
                    if message == 0x0138
                        && id == super::frame::FORWARD_ALL_SUBCLASS_ID
                        && content.is_key_box(dialog, lparam)
                    {
                        return Some(content.key_box_colors(wparam as isize));
                    }
                    let dropped =
                        message == 0x020a && crate::dialogs::native::controls::basic::ComboBoxControl::is_dropped(hwnd);
                    if hwnd != dialog.hwnd() && is_forwarded(message, id == super::frame::WHEEL_SUBCLASS_ID, dropped) {
                        // SAFETY: forwards unchanged synchronous child-to-parent arguments; any notification pointer remains valid while its sender is blocked.
                        Some(unsafe {
                            crate::win32::dialog_native::user32::send_message(dialog.hwnd(), message, wparam, lparam)
                        })
                    } else {
                        None
                    }
                })
                .flatten()
        }),
    );
    if let Some(result) = handled {
        return result;
    }
    // SAFETY: this installed SUBCLASSPROC passes its original OS message arguments unchanged for unhandled messages and panic fallback.
    unsafe { common_controls::def_subclass_proc(hwnd, message, wparam, lparam) }
}
