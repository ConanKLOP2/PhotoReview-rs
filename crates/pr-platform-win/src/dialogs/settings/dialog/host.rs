//! Rust only (no C# twin): the adapter that makes the Settings window a [`SettingsPageHost`]. In C# `SettingsPageContext` holds the `SettingsDialog` itself; here the context
//! holds a [`PageHost`] that holds a `Weak<SettingsContent>` (D-W8B5-901), so the pages (owned by the content) and the content never form an `Rc` cycle. Unit W8-19k.
//! Contract: `contracts/W8-19k.md`.
//!
//! After the window closed (`upgrade()` fails, or `with_native` answers `None`) every read answers the neutral value of the DETACHED context (`""`, `false`, `0`, `-1`, empty,
//! `None`; D-W8B5-705) and every write does nothing, so a late callback of a page never panics. Every method is ONE delegation to the method of `SettingsContent` named in its doc.
//!
//! `unsafe` here: [`default_open_url`] (`ShellExecuteExW`).

use std::collections::BTreeMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use pr_core::settings::shortcuts::ReviewAction;
use pr_imaging::decoder_traits::ImageDecoderFactory;

use super::frame::SettingsContent;
use crate::dialogs::file_dialog::ShellFileDialog;
use crate::dialogs::settings::context::{
    ActionProfilesUnavailable, SettingsPageHost, SettingsUiPost, SharedSettingsPresenter,
};
use crate::dialogs::settings::page_view::TextFn;
use crate::win32::structs::Hwnd;

/// C# `DefaultOpenUrl` (`Process.Start(new ProcessStartInfo(url) { UseShellExecute = true })`): .NET calls `ShellExecuteEx` with `SEE_MASK_FLAG_NO_UI` (no `ErrorDialog`), so a bad
/// URL gives an exception and never a system message box. Rust: `ShellExecuteExW` with `SHELLEXECUTEINFOW { fMask = SEE_MASK_FLAG_NO_UI | SEE_MASK_FLAG_DDEWAIT, lpVerb = "open",
/// lpFile = url, nShow = SW_SHOWNORMAL }` and NOT `SEE_MASK_NOCLOSEPROCESS` (no `hProcess` to close: a handle leak otherwise); `FALSE` is `Err(text with GetLastError)`. Plain
/// `ShellExecuteW` is wrong: it pops up the shell's own error dialog on the UI thread. The page that wants to open a URL calls `SettingsPageContext::open_url`; this is only the fallback of `SettingsDialogOptions::open_url`.
/// An empty `url` is `Err`. `unsafe` with a SAFETY comment (NUL-terminated UTF-16 buffers that outlive the call).
pub fn default_open_url(url: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::{SEE_MASK_FLAG_DDEWAIT, SEE_MASK_FLAG_NO_UI, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;
    if url.trim().is_empty() {
        return Err("empty url".to_owned());
    }
    let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let file: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_FLAG_DDEWAIT,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: info is a live initialized struct; NUL-terminated UTF-16 buffers outlive the call, null parameters/directory are allowed, and no process handle is requested.
    unsafe { ShellExecuteExW(&mut info) }.map_err(|error| format!("ShellExecute failed ({})", error.code().0))
}

/// The weak adapter. Built by `SettingsContent::new` inside `Rc::new_cyclic`.
pub struct PageHost {
    content: Weak<SettingsContent>,
}

impl std::fmt::Debug for PageHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageHost").finish_non_exhaustive()
    }
}

impl PageHost {
    /// Wraps the weak reference.
    pub fn new(content: Weak<SettingsContent>) -> Self {
        Self { content }
    }

    fn live(&self) -> Option<Rc<SettingsContent>> {
        self.content.upgrade().filter(|content| content.hwnd() != 0)
    }

    fn with_dialog<R>(
        &self,
        f: impl FnOnce(&SettingsContent, &crate::dialogs::native::dialog::NativeDialog) -> R,
    ) -> Option<R> {
        let content = self.live()?;
        content.with_native(|dialog| f(&content, dialog))
    }
}

impl SettingsPageHost for PageHost {
    /// `content.presenter.clone()`; after close the presenter of a detached context (`SettingsPageContext::detached().presenter()`).
    fn presenter(&self) -> SharedSettingsPresenter {
        self.live().map_or_else(
            || crate::dialogs::settings::context::SettingsPageContext::detached().presenter(),
            |c| c.presenter.clone(),
        )
    }

    /// `content.hwnd()` (`0` after close).
    fn hwnd(&self) -> Hwnd {
        self.content.upgrade().map_or(0, |c| c.hwnd())
    }

    /// `content.options.files.clone()`.
    fn files(&self) -> Option<Rc<dyn ShellFileDialog>> {
        self.live().and_then(|c| c.options.files.clone())
    }

    /// `content.decoder_factory.clone()`.
    fn decoder_factory(&self) -> Option<Arc<dyn ImageDecoderFactory>> {
        self.live().and_then(|c| c.decoder_factory.clone())
    }

    /// `content.options.version.clone()`.
    fn version(&self) -> Option<String> {
        self.live().and_then(|c| c.options.version.clone())
    }

    /// `options.open_url` when set, else [`default_open_url`].
    fn open_url(&self, url: &str) -> Result<(), String> {
        let Some(c) = self.live() else {
            return Err("detached".to_owned());
        };
        c.options
            .open_url
            .as_ref()
            .map_or_else(|| default_open_url(url), |open| open(url))
    }

    /// `options.start_explorer` when set, else `SettingsPresenter::try_start_explorer(arguments)`.
    fn start_explorer(&self, arguments: &str) -> Option<String> {
        let Some(c) = self.live() else {
            return Some("detached".to_owned());
        };
        c.options.start_explorer.as_ref().map_or_else(
            || pr_app::presenters::settings::SettingsPresenter::try_start_explorer(arguments),
            |start| start(arguments),
        )
    }

    /// `options.show_action_profiles`: `Some(f)` -> `f(owner, actions)`; `None` -> `Err(ActionProfilesUnavailable)`.
    fn show_action_profiles(
        &self,
        owner: Hwnd,
        actions: &[ReviewAction],
    ) -> Result<Option<Vec<ReviewAction>>, ActionProfilesUnavailable> {
        let c = self.live().ok_or(ActionProfilesUnavailable)?;
        c.options
            .show_action_profiles
            .as_ref()
            .ok_or(ActionProfilesUnavailable)?(owner, actions)
    }

    /// `dirty::get_control_text`.
    fn control_text(&self, id: i32) -> String {
        self.with_dialog(|c, d| c.get_control_text(d, id)).unwrap_or_default()
    }

    /// `dirty::set_control_text`.
    fn set_control_text(&self, id: i32, text: &str) {
        self.with_dialog(|c, d| c.set_control_text(d, id, text));
    }

    /// `dirty::is_control_checked`.
    fn is_control_checked(&self, id: i32) -> bool {
        self.with_dialog(|c, d| c.is_control_checked(d, id)).unwrap_or(false)
    }

    /// `dirty::set_control_checked`.
    fn set_control_checked(&self, id: i32, value: bool) {
        self.with_dialog(|c, d| c.set_control_checked(d, id, value));
    }

    /// `dirty::combo_selected_index` (`-1` after close).
    fn combo_selected_index(&self, id: i32) -> i32 {
        self.with_dialog(|c, d| c.combo_selected_index(d, id)).unwrap_or(-1)
    }

    /// `dirty::set_combo_selected_index`.
    fn set_combo_selected_index(&self, id: i32, index: i32) {
        self.with_dialog(|c, d| c.set_combo_selected_index(d, id, index));
    }

    /// `dirty::set_combo_items`.
    fn set_combo_items(&self, id: i32, items: &[String], selected: i32) {
        self.with_dialog(|c, d| c.set_combo_items(d, id, items, selected));
    }

    /// `dirty::slider_value`.
    fn slider_value(&self, id: i32) -> i32 {
        self.with_dialog(|c, d| c.slider_value(d, id)).unwrap_or(0)
    }

    /// `dirty::set_slider_value`.
    fn set_slider_value(&self, id: i32, value: i32) {
        self.with_dialog(|c, d| c.set_slider_value(d, id, value));
    }

    /// `dirty::set_slider_range`.
    fn set_slider_range(&self, id: i32, min: i32, max: i32) {
        self.with_dialog(|c, d| c.set_slider_range(d, id, min, max));
    }

    /// `dirty::set_control_tooltip_fn`.
    fn set_control_tooltip(&self, id: i32, text: TextFn) {
        self.with_dialog(|c, d| c.set_control_tooltip_fn(d, id, text));
    }

    /// C# `CollectAll().ShortcutTexts`: `content.collect_all().shortcut_texts` (the loaded working copy for a Shortcuts page that was never shown).
    fn current_shortcut_texts(&self) -> BTreeMap<String, String> {
        self.live().map(|c| c.collect_all().shortcut_texts).unwrap_or_default()
    }

    /// `dirty::set_control_enabled`.
    fn set_control_enabled(&self, id: i32, enabled: bool) {
        self.with_dialog(|c, d| c.set_control_enabled(d, id, enabled));
    }

    /// `dirty::is_control_enabled`.
    fn is_control_enabled(&self, id: i32) -> bool {
        self.with_dialog(|c, d| c.is_control_enabled(d, id)).unwrap_or(false)
    }

    /// `dirty::set_control_visible`.
    fn set_control_visible(&self, id: i32, visible: bool) {
        self.with_dialog(|c, d| c.set_control_visible(d, id, visible));
    }

    /// `dirty::focus_control`.
    fn focus_control(&self, id: i32) {
        self.with_dialog(|c, d| c.focus_control(d, id));
    }

    /// `dirty::relayout_active_page`.
    fn relayout_active_page(&self) {
        self.with_dialog(|c, d| c.relayout_active_page(d));
    }

    /// `apply::warn` (a modal warning over the Settings window).
    fn warn(&self, title: &str, message: &str) {
        self.with_dialog(|c, d| c.warn(d, title, message));
    }

    /// `content.poster.clone()` as an `Arc<dyn SettingsUiPost>`; after close a poster that drops every action.
    fn ui_post(&self) -> Arc<dyn SettingsUiPost> {
        self.live().map_or_else(
            || crate::dialogs::settings::context::SettingsPageContext::detached().ui_post(),
            |c| c.poster.clone() as Arc<dyn SettingsUiPost>,
        )
    }

    /// `state::actions_json`.
    fn actions_json(&self) -> String {
        self.live().map(|c| c.actions_json()).unwrap_or_default()
    }

    /// `state::refresh_actions_json_from_pages`.
    fn refresh_actions_json_from_pages(&self) -> String {
        self.live()
            .map(|c| c.refresh_actions_json_from_pages())
            .unwrap_or_default()
    }

    /// `state::set_actions_json(json, source_key)`.
    fn set_actions_json(&self, json: &str, source_key: &str) {
        self.with_dialog(|c, d| c.set_actions_json(d, json, source_key));
    }
}
