//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsPageContext.cs` (166 lines, baseline `d92d6aae`): what a Settings page may do at run time. Unit W8-19a.
//! Contract: `contracts/W8-19a.md`.
//!
//! The C# class is a thin facade over `SettingsDialog` so a page never touches an HWND or a message. The Rust facade is [`SettingsPageContext`] over the seam trait
//! [`SettingsPageHost`], which the dialog (W8-19j..19m) implements; every method of the context that is a one-liner in C# stays a one-liner here, the few with logic
//! (`radio_index`, `set_radio_index`, `run_explorer`, `edit_action_profiles`) are pure and tested with a fake host. The C# `SettingsDialogOptions` record is NOT
//! visible to pages: what pages read from it (`OpenUrl`, `StartExplorer`, `ShowActionProfiles`, `Version`, `Files`) is a method of the host (D-W8B5-707).
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::presenters::settings::{SettingsExplorerRequest, SettingsPresenter};
use pr_app::presenters::settings_field_map::FieldContext;
use pr_core::abstractions::{ShellIntegration, ShellMenuState};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::external_editors::ExternalEditor;
use pr_core::settings::shortcuts::ReviewAction;
use pr_imaging::decoder_traits::ImageDecoderFactory;
use pr_l10n::{
    keys,
    localizer::{Localizer, number_culture},
};

use super::page_view::TextFn;
use crate::dialogs::file_dialog::ShellFileDialog;
use crate::dialogs::native::external_editors_dialog::ExternalEditorsEdit;
use crate::win32::structs::Hwnd;

/// The presenter the dialog shares with its pages (C# `SettingsPresenter Presenter`). The dialog and the pages borrow it briefly on the UI thread; nobody holds a
/// borrow across a modal call (file dialog, warning, action profiles editor), see D-W8B5-706.
pub type SharedSettingsPresenter = Rc<RefCell<SettingsPresenter>>;

/// An action queued for the UI thread of the Settings window. It gets a context (the one of the window it runs in), so a result computed on a worker can be shown
/// without the page itself crossing threads (`Rc` is not `Send`).
pub type ContextUiAction = Box<dyn FnOnce(&SettingsPageContext) + Send + 'static>;

/// A `Send + Sync` handle to post [`ContextUiAction`]s to the Settings window (C# `PostToUi` "is safe to call from a background thread"). Posting to a window that
/// has closed is a silent no-op (the update check answer that arrives after Cancel).
pub trait SettingsUiPost: Send + Sync {
    fn post(&self, action: ContextUiAction);
}

/// The C# `ActionProfiles` seam failed: the port is not wired (`SettingsDialogOptions.ShowActionProfiles == null`) or the editor could not be opened (the C#
/// `catch (Exception)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionProfilesUnavailable;

/// Everything `SettingsPageContext` reads from or does to the Settings dialog (the members of C# `SettingsDialog` it calls, plus the `Options` it reads). Every
/// method takes `&self`: the dialog keeps its state in `Cell`/`RefCell` and a page may call it while the dialog is in the middle of dispatching a command to that
/// page. Implemented by the dialog (W8-19j..19m); the tests of this unit use a recording fake.
pub trait SettingsPageHost {
    /// C# `Presenter`.
    fn presenter(&self) -> SharedSettingsPresenter;
    /// C# `Hwnd` (the window is the owner of the child dialogs).
    fn hwnd(&self) -> Hwnd;
    /// C# `Files` (`Options.Files`): the shell Open/Save dialog; `None` when the dialog was built without one.
    fn files(&self) -> Option<Rc<dyn ShellFileDialog>>;
    /// C# `DecoderFactory`: the Performance page asks whether TurboJpeg is registered; `None` = unknown.
    fn decoder_factory(&self) -> Option<Arc<dyn ImageDecoderFactory>>;
    /// C# `Options.Version`: the version text of the left-column footer and of the update check.
    fn version(&self) -> Option<String>;
    /// C# `Options.OpenUrl ?? DefaultOpenUrl` (ShellExecute): opens `url`; `Err(reason)` when it cannot.
    fn open_url(&self, url: &str) -> Result<(), String>;
    /// C# `Options.StartExplorer is { } start ? start(args) : SettingsPresenter.TryStartExplorer(args)`: `None` = started, `Some(reason)` = failed.
    fn start_explorer(&self, arguments: &str) -> Option<String>;
    /// C# `Options.ShowActionProfiles(owner, actions)`: `Ok(Some(list))` = the edited list after Apply, `Ok(None)` = cancelled, `Err` = not wired or could not open.
    fn show_action_profiles(
        &self,
        owner: Hwnd,
        actions: &[ReviewAction],
    ) -> Result<Option<Vec<ReviewAction>>, ActionProfilesUnavailable>;
    /// W9-E4, C# `Options.ShowExternalEditors(owner, editors, defaultId)`: `Ok(Some(edit))` = the edit after Apply, `Ok(None)` = cancelled, `Err` = not wired or could not
    /// open. Default (so the merged implementors keep compiling): `Err(ActionProfilesUnavailable)` (= `ShowExternalEditors == null`).
    fn show_external_editors(
        &self,
        _owner: Hwnd,
        _editors: &[ExternalEditor],
        _default_id: Option<&str>,
    ) -> Result<Option<ExternalEditorsEdit>, ActionProfilesUnavailable> {
        Err(ActionProfilesUnavailable)
    }

    /// C# `GetControlText(id)`.
    fn control_text(&self, id: i32) -> String;
    /// C# `SetControlText(id, text)`.
    fn set_control_text(&self, id: i32, text: &str);
    /// C# `IsControlChecked(id)`.
    fn is_control_checked(&self, id: i32) -> bool;
    /// C# `SetControlChecked(id, value)`.
    fn set_control_checked(&self, id: i32, value: bool);
    /// C# `ComboSelectedIndex(id)` (`-1` = none).
    fn combo_selected_index(&self, id: i32) -> i32;
    /// C# `SetComboSelectedIndex(id, index)`.
    fn set_combo_selected_index(&self, id: i32, index: i32);
    /// C# `SetComboItems(id, items, selected)`: replaces the list and selects `selected`.
    fn set_combo_items(&self, id: i32, items: &[String], selected: i32);
    /// C# `SliderValue(id)`.
    fn slider_value(&self, id: i32) -> i32;
    /// C# `SetSliderValue(id, value)`.
    fn set_slider_value(&self, id: i32, value: i32);
    /// C# `SetSliderRange(id, min, max)`.
    fn set_slider_range(&self, id: i32, min: i32, max: i32);
    /// C# `SetControlTooltip(id, Func<string>)`: the text is asked again each time the tip is shown.
    fn set_control_tooltip(&self, id: i32, text: TextFn);
    /// C# `CollectAll().ShortcutTexts`: the shortcut texts as typed now (the loaded working copy when the Shortcuts page was never opened).
    fn current_shortcut_texts(&self) -> BTreeMap<String, String>;
    /// C# `SetControlEnabled(id, enabled)`.
    fn set_control_enabled(&self, id: i32, enabled: bool);
    /// C# `IsControlEnabled(id)`.
    fn is_control_enabled(&self, id: i32) -> bool;
    /// C# `SetControlVisible(id, visible)`: a control or a whole group (group box id); hidden parts take no room and the layout is recomputed.
    fn set_control_visible(&self, id: i32, visible: bool);
    /// C# `FocusControl(id)`.
    fn focus_control(&self, id: i32);
    /// C# `RelayoutActivePage()`.
    fn relayout_active_page(&self);
    /// C# `Warn(title, message)`: a modal warning over the Settings window.
    fn warn(&self, title: &str, message: &str);
    /// A `Send + Sync` poster to the window's UI thread (the Rust form of C# `PostToUi`).
    fn ui_post(&self) -> Arc<dyn SettingsUiPost>;
    /// C# `ActionsJson`.
    fn actions_json(&self) -> String;
    /// C# `RefreshActionsJsonFromPages()`: the action JSON as the pages show it now (includes unsaved hand edits); updates `actions_json`.
    fn refresh_actions_json_from_pages(&self) -> String;
    /// C# `SetActionsJson(json, source)`: stores `json` in the working copy and reloads every OTHER created page (their unsaved edits stay). `source_key` is the
    /// `SettingsPageView::key` of the page that made the change (the C# passes the page object; keys are unique).
    fn set_actions_json(&self, json: &str, source_key: &str);
}

/// C# `SettingsPageContext`. Cheap to clone (it shares the host).
#[derive(Clone)]
pub struct SettingsPageContext {
    host: Rc<dyn SettingsPageHost>,
}

impl fmt::Debug for SettingsPageContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsPageContext").finish_non_exhaustive()
    }
}

impl SettingsPageContext {
    /// C# `internal SettingsPageContext(SettingsDialog dialog)`.
    pub fn new(host: Rc<dyn SettingsPageHost>) -> Self {
        Self { host }
    }

    /// A context that is not attached to any dialog: reads answer `""`, `false`, `0`, `-1` (combo), empty collections or `None`; writes do nothing; `ui_post` drops
    /// the action. Used by [`PageContextSlot`](super::page_view::PageContextSlot) before `attach` (D-W8B5-705).
    pub fn detached() -> Self {
        thread_local! {
            static PRESENTER: SharedSettingsPresenter = {
                let texts = EnglishSettingsTexts;
                let settings = AppSettings::new(&texts);
                let options = pr_app::presenters::settings::SettingsPresenterOptions::new(
                    0, Arc::new(EnglishSettingsTexts), Arc::new(InertShellIntegration));
                Rc::new(RefCell::new(SettingsPresenter::new(&settings, options)))
            };
        }
        let presenter = PRESENTER.with(Rc::clone);
        Self::new(Rc::new(DetachedHost { presenter }))
    }

    /// C# `Presenter`.
    pub fn presenter(&self) -> SharedSettingsPresenter {
        self.host.presenter()
    }

    /// C# `Owner` (`_dialog.Hwnd`).
    pub fn owner(&self) -> Hwnd {
        self.host.hwnd()
    }

    /// C# `Files`.
    pub fn files(&self) -> Option<Rc<dyn ShellFileDialog>> {
        self.host.files()
    }

    /// C# `DecoderFactory`.
    pub fn decoder_factory(&self) -> Option<Arc<dyn ImageDecoderFactory>> {
        self.host.decoder_factory()
    }

    /// C# `Version`.
    pub fn version(&self) -> Option<String> {
        self.host.version()
    }

    /// C# `Options.OpenUrl ?? DefaultOpenUrl` (the General page passes it to `SettingsPresenter::open_update_page`).
    pub fn open_url(&self, url: &str) -> Result<(), String> {
        self.host.open_url(url)
    }

    /// C# `Text(id)`.
    pub fn text(&self, id: i32) -> String {
        self.host.control_text(id)
    }

    /// C# `SetText(id, text)`.
    pub fn set_text(&self, id: i32, text: &str) {
        self.host.set_control_text(id, text)
    }

    /// C# `IsChecked(id)`.
    pub fn is_checked(&self, id: i32) -> bool {
        self.host.is_control_checked(id)
    }

    /// C# `SetChecked(id, value)`.
    pub fn set_checked(&self, id: i32, value: bool) {
        self.host.set_control_checked(id, value)
    }

    /// C# `RadioIndex(ids)`: the index (in declaration order) of the first checked radio of the group `ids`, `-1` when none is checked.
    pub fn radio_index(&self, ids: &[i32]) -> i32 {
        for (index, id) in ids.iter().enumerate() {
            if self.host.is_control_checked(*id) {
                return i32::try_from(index).unwrap_or(-1);
            }
        }
        -1
    }

    /// C# `SetRadioIndex(ids, index)`: for EVERY id of the group, in order, `set_control_checked(id, i == index)` (`BM_SETCHECK` does not clear the sibling radios);
    /// an index outside `0..ids.len()` (also negative) checks none.
    pub fn set_radio_index(&self, ids: &[i32], index: i32) {
        for (i, id) in ids.iter().enumerate() {
            self.host.set_control_checked(*id, i32::try_from(i).ok() == Some(index));
        }
    }

    /// C# `ComboIndex(id)`.
    pub fn combo_index(&self, id: i32) -> i32 {
        self.host.combo_selected_index(id)
    }

    /// C# `SetComboIndex(id, index)`.
    pub fn set_combo_index(&self, id: i32, index: i32) {
        self.host.set_combo_selected_index(id, index)
    }

    /// C# `SetComboItems(id, items, selected)`: reload a dynamic list (the languages) and select `selected`.
    pub fn set_combo_items(&self, id: i32, items: &[String], selected: i32) {
        self.host.set_combo_items(id, items, selected)
    }

    /// C# `SliderValue(id)`.
    pub fn slider_value(&self, id: i32) -> i32 {
        self.host.slider_value(id)
    }

    /// C# `SetSliderValue(id, value)`.
    pub fn set_slider_value(&self, id: i32, value: i32) {
        self.host.set_slider_value(id, value)
    }

    /// C# `SetSliderRange(id, min, max)`.
    pub fn set_slider_range(&self, id: i32, min: i32, max: i32) {
        self.host.set_slider_range(id, min, max)
    }

    /// C# `SetTooltip(id, Func<string>)`: shown on hover; the text is asked again each time so it follows the language.
    pub fn set_tooltip(&self, id: i32, text: TextFn) {
        self.host.set_control_tooltip(id, text)
    }

    /// C# `SetTooltip(id, string)` (the overload with a fixed text): set once at `attach`, does not follow the language. A different name because Rust has no overloads.
    pub fn set_tooltip_text(&self, id: i32, text: &str) {
        self.host.set_control_tooltip(id, super::page_view::fixed_text(text))
    }

    /// C# `CurrentShortcutTexts()`.
    pub fn current_shortcut_texts(&self) -> BTreeMap<String, String> {
        self.host.current_shortcut_texts()
    }

    /// C# `SetEnabled(id, enabled)`.
    pub fn set_enabled(&self, id: i32, enabled: bool) {
        self.host.set_control_enabled(id, enabled)
    }

    /// C# `IsEnabled(id)`.
    pub fn is_enabled(&self, id: i32) -> bool {
        self.host.is_control_enabled(id)
    }

    /// C# `SetVisible(id, visible)`.
    pub fn set_visible(&self, id: i32, visible: bool) {
        self.host.set_control_visible(id, visible)
    }

    /// C# `Focus(id)`.
    pub fn focus(&self, id: i32) {
        self.host.focus_control(id)
    }

    /// C# `Relayout()`.
    pub fn relayout(&self) {
        self.host.relayout_active_page()
    }

    /// C# `ShowWarning(title, message)`.
    pub fn show_warning(&self, title: &str, message: &str) {
        self.host.warn(title, message)
    }

    /// C# `PostToUi(action)` from the UI thread or a worker: `action` runs later on the Settings window's UI thread with the window's context. Equal to
    /// `self.ui_post().post(action)`.
    pub fn post_to_ui(&self, action: ContextUiAction) {
        self.ui_post().post(action)
    }

    /// The `Send + Sync` poster a worker thread holds (a page cannot move `self` to a worker: `Rc`).
    pub fn ui_post(&self) -> Arc<dyn SettingsUiPost> {
        self.host.ui_post()
    }

    /// C# `RunExplorer(request)`: `request.error_message` set -> `show_warning(dialog.settingsOpenFailed.title, message)` and stop; `arguments` `None` -> nothing;
    /// else `error = host.start_explorer(arguments)` and, when `Some`, the same warning with the error.
    pub fn run_explorer(&self, request: &SettingsExplorerRequest) {
        if let Some(message) = request.error_message.as_deref() {
            self.show_warning(
                &Localizer::current().get(keys::DIALOG_SETTINGS_OPEN_FAILED_TITLE),
                message,
            );
            return;
        }
        if let Some(arguments) = request.arguments.as_deref() {
            if let Some(error) = self.host.start_explorer(arguments) {
                self.show_warning(
                    &Localizer::current().get(keys::DIALOG_SETTINGS_OPEN_FAILED_TITLE),
                    &error,
                );
            }
        }
    }

    /// C# `ActionsJson`.
    pub fn actions_json(&self) -> String {
        self.host.actions_json()
    }

    /// C# `CollectActionsJson()`.
    pub fn collect_actions_json(&self) -> String {
        self.host.refresh_actions_json_from_pages()
    }

    /// C# `CommitActionsJson(json, source)`; `source_key` = `SettingsPageView::key` of the calling page.
    pub fn commit_actions_json(&self, json: &str, source_key: &str) {
        self.host.set_actions_json(json, source_key)
    }

    /// C# `EditActionProfiles(actions)`: opens the editor modally over the Settings window. `Ok(Some(list))` -> `Some(list)`; `Ok(None)` (cancelled) -> `None` with NO
    /// notice; `Err(ActionProfilesUnavailable)` -> `show_warning(dialog.openEditorFailed.title, dialog.openEditorFailed.message)` and `None`.
    pub fn edit_action_profiles(&self, actions: &[ReviewAction]) -> Option<Vec<ReviewAction>> {
        match self.host.show_action_profiles(self.owner(), actions) {
            Ok(result) => result,
            Err(_) => {
                let localizer = Localizer::current();
                self.show_warning(
                    &localizer.get(keys::DIALOG_OPEN_EDITOR_FAILED_TITLE),
                    &localizer.get(keys::DIALOG_OPEN_EDITOR_FAILED_MESSAGE),
                );
                None
            }
        }
    }
}

impl SettingsPageContext {
    /// W9-E4, C# `EditExternalEditors(editors, defaultId)`: the nested editors dialog over the Settings window. `Ok(Some(edit))` -> `Some(edit)`; `Ok(None)`
    /// (cancelled) -> `None` with NO notice; `Err(ActionProfilesUnavailable)` -> the same notice as `edit_action_profiles`
    /// (`show_warning(dialog.openEditorFailed.title, dialog.openEditorFailed.message)`) and `None`.
    pub fn edit_external_editors(
        &self,
        editors: &[ExternalEditor],
        default_id: Option<&str>,
    ) -> Option<ExternalEditorsEdit> {
        todo!()
    }
}

struct DetachedHost {
    presenter: SharedSettingsPresenter,
}

impl SettingsPageHost for DetachedHost {
    fn presenter(&self) -> SharedSettingsPresenter {
        Rc::clone(&self.presenter)
    }
    fn hwnd(&self) -> Hwnd {
        0
    }
    fn files(&self) -> Option<Rc<dyn ShellFileDialog>> {
        None
    }
    fn decoder_factory(&self) -> Option<Arc<dyn ImageDecoderFactory>> {
        None
    }
    fn version(&self) -> Option<String> {
        None
    }
    fn open_url(&self, _url: &str) -> Result<(), String> {
        Err("detached".to_owned())
    }
    fn start_explorer(&self, _arguments: &str) -> Option<String> {
        Some("detached".to_owned())
    }
    fn show_action_profiles(
        &self,
        _owner: Hwnd,
        _actions: &[ReviewAction],
    ) -> Result<Option<Vec<ReviewAction>>, ActionProfilesUnavailable> {
        Err(ActionProfilesUnavailable)
    }
    fn control_text(&self, _id: i32) -> String {
        String::new()
    }
    fn set_control_text(&self, _id: i32, _text: &str) {}
    fn is_control_checked(&self, _id: i32) -> bool {
        false
    }
    fn set_control_checked(&self, _id: i32, _value: bool) {}
    fn combo_selected_index(&self, _id: i32) -> i32 {
        -1
    }
    fn set_combo_selected_index(&self, _id: i32, _index: i32) {}
    fn set_combo_items(&self, _id: i32, _items: &[String], _selected: i32) {}
    fn slider_value(&self, _id: i32) -> i32 {
        0
    }
    fn set_slider_value(&self, _id: i32, _value: i32) {}
    fn set_slider_range(&self, _id: i32, _min: i32, _max: i32) {}
    fn set_control_tooltip(&self, _id: i32, _text: TextFn) {}
    fn current_shortcut_texts(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
    fn set_control_enabled(&self, _id: i32, _enabled: bool) {}
    fn is_control_enabled(&self, _id: i32) -> bool {
        false
    }
    fn set_control_visible(&self, _id: i32, _visible: bool) {}
    fn focus_control(&self, _id: i32) {}
    fn relayout_active_page(&self) {}
    fn warn(&self, _title: &str, _message: &str) {}
    fn ui_post(&self) -> Arc<dyn SettingsUiPost> {
        Arc::new(DetachedPost)
    }
    fn actions_json(&self) -> String {
        String::new()
    }
    fn refresh_actions_json_from_pages(&self) -> String {
        String::new()
    }
    fn set_actions_json(&self, _json: &str, _source_key: &str) {}
}

struct DetachedPost;

impl SettingsUiPost for DetachedPost {
    fn post(&self, _action: ContextUiAction) {}
}

#[derive(Debug)]
struct InertShellIntegration;

impl ShellIntegration for InertShellIntegration {
    fn get_state(&self, _executable_path: &str) -> ShellMenuState {
        ShellMenuState::NotRegistered
    }
    fn register(&self, _executable_path: &str) -> std::io::Result<()> {
        Ok(())
    }
    fn unregister(&self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The `FieldContext` of the ambient language for the presenter calls that need one (C# reads the ambient `Tr`): builds `FieldContext { texts, numbers }` from
/// `Localizer::current()` and `pr_l10n::localizer::number_culture()` and runs `f` with it (D-W8-4).
pub fn with_field_context<R>(f: impl FnOnce(&FieldContext<'_>) -> R) -> R {
    let texts = Localizer::current();
    let numbers = number_culture();
    f(&FieldContext {
        texts: &texts,
        numbers: &numbers,
    })
}
