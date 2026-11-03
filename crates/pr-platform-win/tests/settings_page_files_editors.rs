//! G9 (settings-ui M1) twins of C# `FilesSettingsPage` Load/Collect/ManageEditors/ShowEditorSummary at baseline e7c2fcd8.
//! The nested editors dialog itself is a native test (`settings_external_editors_native.rs`); here the host answers
//! `show_external_editors` with a canned result so the page logic runs without a window.
#![allow(dead_code)]

#[path = "common/settings_builder_fixture.rs"]
mod settings_builder_fixture;
#[path = "common/settings_page_fixture.rs"]
mod settings_page_fixture;

use pr_app::presenters::settings_form_state::SettingsFormState;
use pr_core::settings::external_editors::ExternalEditor;
use pr_core::settings::shortcuts::ReviewAction;
use pr_l10n::tr;
use pr_platform_win::dialogs::{
    file_dialog::ShellFileDialog,
    native::external_editors_dialog::ExternalEditorsEdit,
    settings::{
        builder::inputs::SettingsPageBuilder,
        context::{
            ActionProfilesUnavailable, SettingsPageContext, SettingsPageHost, SettingsUiPost, SharedSettingsPresenter,
        },
        page_view::{SettingsPageView, TextFn},
        pages::files::FilesSettingsPage,
    },
};
use pr_platform_win::win32::structs::Hwnd;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

const BN_CLICKED: u32 = 0;

type Answer = Result<Option<ExternalEditorsEdit>, ActionProfilesUnavailable>;

/// Delegates everything to the shared recording fixture except `show_external_editors`.
struct EditorHost {
    inner: Rc<settings_page_fixture::PageHost>,
    answer: RefCell<Option<Answer>>,
    seen: RefCell<Vec<(Vec<ExternalEditor>, Option<String>)>>,
}

impl SettingsPageHost for EditorHost {
    fn presenter(&self) -> SharedSettingsPresenter {
        self.inner.presenter()
    }
    fn hwnd(&self) -> Hwnd {
        self.inner.hwnd()
    }
    fn files(&self) -> Option<Rc<dyn ShellFileDialog>> {
        None
    }
    fn decoder_factory(&self) -> Option<Arc<dyn pr_imaging::decoder_traits::ImageDecoderFactory>> {
        None
    }
    fn version(&self) -> Option<String> {
        None
    }
    fn open_url(&self, url: &str) -> Result<(), String> {
        self.inner.open_url(url)
    }
    fn start_explorer(&self, arguments: &str) -> Option<String> {
        self.inner.start_explorer(arguments)
    }
    fn show_action_profiles(
        &self,
        owner: Hwnd,
        actions: &[ReviewAction],
    ) -> Result<Option<Vec<ReviewAction>>, ActionProfilesUnavailable> {
        self.inner.show_action_profiles(owner, actions)
    }
    fn show_external_editors(&self, _owner: Hwnd, editors: &[ExternalEditor], default_id: Option<&str>) -> Answer {
        self.seen
            .borrow_mut()
            .push((editors.to_vec(), default_id.map(str::to_owned)));
        self.answer
            .borrow_mut()
            .take()
            .unwrap_or(Err(ActionProfilesUnavailable))
    }
    fn control_text(&self, id: i32) -> String {
        self.inner.control_text(id)
    }
    fn set_control_text(&self, id: i32, text: &str) {
        self.inner.set_control_text(id, text)
    }
    fn is_control_checked(&self, id: i32) -> bool {
        self.inner.is_control_checked(id)
    }
    fn set_control_checked(&self, id: i32, value: bool) {
        self.inner.set_control_checked(id, value)
    }
    fn combo_selected_index(&self, id: i32) -> i32 {
        self.inner.combo_selected_index(id)
    }
    fn set_combo_selected_index(&self, id: i32, index: i32) {
        self.inner.set_combo_selected_index(id, index)
    }
    fn set_combo_items(&self, id: i32, items: &[String], selected: i32) {
        self.inner.set_combo_items(id, items, selected)
    }
    fn slider_value(&self, id: i32) -> i32 {
        SettingsPageHost::slider_value(&*self.inner, id)
    }
    fn set_slider_value(&self, id: i32, value: i32) {
        self.inner.set_slider_value(id, value)
    }
    fn set_slider_range(&self, id: i32, min: i32, max: i32) {
        self.inner.set_slider_range(id, min, max)
    }
    fn set_control_tooltip(&self, id: i32, text: TextFn) {
        self.inner.set_control_tooltip(id, text)
    }
    fn current_shortcut_texts(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
    fn set_control_enabled(&self, id: i32, enabled: bool) {
        self.inner.set_control_enabled(id, enabled)
    }
    fn is_control_enabled(&self, id: i32) -> bool {
        self.inner.is_control_enabled(id)
    }
    fn set_control_visible(&self, id: i32, visible: bool) {
        self.inner.set_control_visible(id, visible)
    }
    fn focus_control(&self, id: i32) {
        self.inner.focus_control(id)
    }
    fn relayout_active_page(&self) {
        self.inner.relayout_active_page()
    }
    fn warn(&self, title: &str, message: &str) {
        self.inner.warn(title, message)
    }
    fn ui_post(&self) -> Arc<dyn SettingsUiPost> {
        self.inner.ui_post()
    }
    fn actions_json(&self) -> String {
        String::new()
    }
    fn refresh_actions_json_from_pages(&self) -> String {
        String::new()
    }
    fn set_actions_json(&self, _json: &str, _source_key: &str) {}
}

fn editor(id: &str, name: &str, path: &str) -> ExternalEditor {
    ExternalEditor {
        id: id.to_owned(),
        name: name.to_owned(),
        path: path.to_owned(),
        arguments: ExternalEditor::DEFAULT_ARGUMENTS.to_owned(),
    }
}

fn attached(answer: Option<Answer>) -> (FilesSettingsPage, Rc<EditorHost>) {
    let mut page = FilesSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(1000);
    page.declare(&mut builder);
    let inner = settings_page_fixture::PageHost::new();
    inner.configure_from(&builder);
    let host = Rc::new(EditorHost {
        inner,
        answer: RefCell::new(answer),
        seen: RefCell::new(Vec::new()),
    });
    page.attach(SettingsPageContext::new(host.clone()));
    (page, host)
}

fn state_with_editors() -> SettingsFormState {
    SettingsFormState {
        external_editors: vec![
            editor("a", "Alpha", r"C:\A\alpha.exe"),
            editor("b", "Beta", r"C:\B\beta.exe"),
        ],
        default_external_editor_id: "b".to_owned(),
        external_editor_path: r"C:\B\beta.exe".to_owned(),
        external_editor_sends_raw_original: true,
        ..SettingsFormState::default()
    }
}

#[test]
// C# FilesSettingsPage.Load/Collect: the editor list, the default id, the path box and the RAW switch round-trip untouched.
fn load_then_collect_round_trips_the_editor_list_default_path_and_raw_switch() {
    let (mut page, host) = attached(None);
    let loaded = state_with_editors();
    page.load(&loaded);
    assert_eq!(host.inner.text(page.ids().editor_path), r"C:\B\beta.exe");
    assert!(host.inner.checked(page.editor_ids().send_raw));
    let mut out = SettingsFormState::default();
    page.collect(&mut out);
    assert_eq!(out.external_editors, loaded.external_editors);
    assert_eq!(out.default_external_editor_id, "b");
    assert_eq!(out.external_editor_path, r"C:\B\beta.exe");
    assert!(out.external_editor_sends_raw_original);
}

#[test]
// C# ShowEditorSummary: no editors -> the "none" text; otherwise count and the DEFAULT editor's display name.
fn summary_shows_none_without_editors_and_count_with_the_default_name() {
    let (mut page, host) = attached(None);
    page.load(&SettingsFormState::default());
    assert_eq!(
        host.inner.text(page.editor_ids().summary),
        tr::settings_external_editors_summary_none()
    );
    page.load(&state_with_editors());
    assert_eq!(
        host.inner.text(page.editor_ids().summary),
        tr::settings_external_editors_summary(2, "Beta".to_owned())
    );
}

#[test]
// C# ManageEditors: Apply replaces the list and default, the path box follows the default editor, the summary is redrawn, Collect returns the new list.
fn manage_editors_apply_updates_list_path_summary_and_collect() {
    let edit = ExternalEditorsEdit {
        editors: vec![editor("c", "", r"C:\Tools\gamma.exe")],
        default_id: "c".to_owned(),
    };
    let (mut page, host) = attached(Some(Ok(Some(edit))));
    page.load(&state_with_editors());
    assert!(page.on_command(page.editor_ids().manage, BN_CLICKED));
    // The dialog was shown the loaded list and default.
    let seen = host.seen.borrow().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0.len(), 2);
    assert_eq!(seen[0].1.as_deref(), Some("b"));
    assert_eq!(host.inner.text(page.ids().editor_path), r"C:\Tools\gamma.exe");
    assert_eq!(
        host.inner.text(page.editor_ids().summary),
        tr::settings_external_editors_summary(1, "gamma".to_owned())
    );
    let mut out = SettingsFormState::default();
    page.collect(&mut out);
    assert_eq!(out.external_editors.len(), 1);
    assert_eq!(out.external_editors[0].id, "c");
    assert_eq!(out.default_external_editor_id, "c");
    assert_eq!(out.external_editor_path, r"C:\Tools\gamma.exe");
}

#[test]
// C# ManageEditors: Cancel (null edit) changes nothing and shows no notice.
fn manage_editors_cancel_changes_nothing_and_shows_no_warning() {
    let (mut page, host) = attached(Some(Ok(None)));
    let loaded = state_with_editors();
    page.load(&loaded);
    assert!(page.on_command(page.editor_ids().manage, BN_CLICKED));
    assert!(host.inner.warnings().is_empty());
    let mut out = SettingsFormState::default();
    page.collect(&mut out);
    assert_eq!(out.external_editors, loaded.external_editors);
    assert_eq!(out.default_external_editor_id, "b");
    assert_eq!(out.external_editor_path, r"C:\B\beta.exe");
}

#[test]
// C# EditExternalEditors with ShowExternalEditors == null: one warning, the page keeps its state (no todo!() panic any more).
fn manage_editors_unwired_host_warns_once_and_keeps_the_state() {
    let (mut page, host) = attached(None);
    let loaded = state_with_editors();
    page.load(&loaded);
    assert!(page.on_command(page.editor_ids().manage, BN_CLICKED));
    assert_eq!(host.inner.warnings().len(), 1);
    let mut out = SettingsFormState::default();
    page.collect(&mut out);
    assert_eq!(out.external_editors, loaded.external_editors);
}

#[test]
// C# ManageEditors: a typed path in the box is folded into the list BEFORE the dialog opens (what is on screen is what the dialog shows).
fn manage_editors_folds_a_typed_path_into_the_list_the_dialog_receives() {
    let (mut page, host) = attached(Some(Ok(None)));
    page.load(&state_with_editors());
    host.inner.put_text(page.ids().editor_path, r"C:\Typed\typed.exe");
    assert!(page.on_command(page.editor_ids().manage, BN_CLICKED));
    let seen = host.seen.borrow().clone();
    assert_eq!(seen.len(), 1);
    assert!(
        seen[0].0.iter().any(|e| e.path == r"C:\Typed\typed.exe"),
        "the typed path must reach the dialog: {:?}",
        seen[0].0
    );
}

#[test]
// C# OnCommand: only BN_CLICKED on the manager button is handled; any other notification is not.
fn manage_button_ignores_other_notifications() {
    let (mut page, host) = attached(Some(Ok(None)));
    page.load(&state_with_editors());
    assert!(!page.on_command(page.editor_ids().manage, BN_CLICKED + 1));
    assert!(host.seen.borrow().is_empty());
}
