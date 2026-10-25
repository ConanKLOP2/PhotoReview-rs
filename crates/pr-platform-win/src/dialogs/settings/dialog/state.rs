//! Port of `SettingsDialog.cs` lines 464 to 564 (`WorkingFields`, `ActionsJson`, `RefreshActionsJsonFromPages`, `SetActionsJson`, `LoadAll`, `CollectAll`), baseline
//! `d92d6aae`. Unit W8-19k. Contract: `contracts/W8-19k.md`.
//!
//! The working copy of the form: the presenter's `load_fields()` plus the action JSON that the Shortcuts page edits through the nested editor. A page that was never shown
//! has no controls, so it keeps the loaded values (Save does not lose the settings of a page the user never opened).

use pr_app::presenters::settings_form_state::SettingsFormState;

use super::frame::SettingsContent;
use crate::dialogs::native::dialog::NativeDialog;

/// RAII form of the C# `_loading = true; try { .. } finally { _loading = false; }`: sets the flag on `new`, restores the PREVIOUS value on drop (nesting-safe).
pub struct LoadingGuard<'a> {
    content: &'a SettingsContent,
    previous: bool,
}

impl<'a> std::fmt::Debug for LoadingGuard<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadingGuard").finish_non_exhaustive()
    }
}

impl<'a> LoadingGuard<'a> {
    /// Sets `loading` and remembers the previous value.
    pub fn new(content: &'a SettingsContent) -> Self {
        let previous = content.view.loading.replace(true);
        Self { content, previous }
    }
}

impl Drop for LoadingGuard<'_> {
    fn drop(&mut self) {
        self.content.view.loading.set(self.previous);
    }
}

impl SettingsContent {
    /// C# `WorkingFields()`: `state = presenter.load_fields()`; when `actions_json` is `Some(json)` the state's `actions_json` becomes `json`, else `actions_json` becomes
    /// `Some(state.actions_json.clone())`. The presenter is borrowed for the call only.
    pub(super) fn working_fields(&self) -> SettingsFormState {
        let mut state = self.presenter.borrow().load_fields();
        let stored = self.view.actions_json.borrow().clone();
        if let Some(json) = stored {
            state.actions_json = json;
        } else {
            *self.view.actions_json.borrow_mut() = Some(state.actions_json.clone());
        }
        state
    }

    /// C# `ActionsJson` (`_actionsJson ??= LoadFields().ActionsJson`): the working action JSON shared by the Files/Shortcuts pages.
    pub fn actions_json(&self) -> String {
        let stored = self.view.actions_json.borrow().clone();
        stored.unwrap_or_else(|| self.working_fields().actions_json)
    }

    /// C# `RefreshActionsJsonFromPages()`: `json = collect_all().actions_json`; stored as the working value; returned.
    pub fn refresh_actions_json_from_pages(&self) -> String {
        let json = self.collect_all().actions_json;
        *self.view.actions_json.borrow_mut() = Some(json.clone());
        json
    }

    /// C# `SetActionsJson(json, source)` (`source_key` = the key of the page that made the change, `""` = none): stores `json`; `state = collect_all()` with `state.actions_json =
    /// json` (a page with its own JSON box collected stale text); then, with `loading` set, `page.load(&state)` for every CREATED page whose key is not `source_key` (a page that
    /// is borrowed is skipped); then `relayout_active_page`. (The C# also marks every page dirty: write-only, not ported, D-W8B5-907.)
    pub fn set_actions_json(&self, dialog: &NativeDialog, json: &str, source_key: &str) {
        *self.view.actions_json.borrow_mut() = Some(json.to_owned());
        let mut state = self.collect_all();
        state.actions_json = json.to_owned();
        {
            let _loading = LoadingGuard::new(self);
            for (index, page) in self.pages.iter().enumerate() {
                if self.is_page_created(index) {
                    if let Ok(mut page) = page.try_borrow_mut() {
                        if page.key() != source_key {
                            page.load(&state);
                        }
                    }
                }
            }
        }
        self.relayout_active_page(dialog);
    }

    /// C# `LoadAll(state)`: `actions_json = state.actions_json`; with `loading` set, `page.load(state)` for every CREATED page (borrowed pages skipped); when a page is showing
    /// (`active >= 0`) `relayout_active_page`. Used by Defaults, Import and the C# `ImportSettingsFrom` refresh callback.
    pub fn load_all(&self, dialog: &NativeDialog, state: &SettingsFormState) {
        *self.view.actions_json.borrow_mut() = Some(state.actions_json.clone());
        {
            let _loading = LoadingGuard::new(self);
            for (index, page) in self.pages.iter().enumerate() {
                if self.is_page_created(index) {
                    if let Ok(mut page) = page.try_borrow_mut() {
                        page.load(state);
                    }
                }
            }
        }
        self.relayout_active_page(dialog);
    }

    /// C# `CollectAll()`: `state = working_fields()`; every CREATED page, in order, `page.collect(&mut state)` (a page that is borrowed is skipped: only the CALLER can be
    /// borrowed and no page's `collect` writes `actions_json`, D-W8B5-904). Pages never created keep the loaded values.
    pub fn collect_all(&self) -> SettingsFormState {
        let mut state = self.working_fields();
        for (index, page) in self.pages.iter().enumerate() {
            if self.is_page_created(index) {
                if let Ok(mut page) = page.try_borrow_mut() {
                    page.collect(&mut state);
                }
            }
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::settings::{
        builder::inputs::SettingsPageBuilder, context::SettingsPageContext, page_view::SettingsPageView,
    };
    use std::{cell::RefCell, rc::Rc};

    struct Page {
        name: &'static str,
        loads: Rc<RefCell<Vec<String>>>,
    }
    impl SettingsPageView for Page {
        fn key(&self) -> &str {
            self.name
        }
        fn declare(&mut self, _: &mut SettingsPageBuilder) {}
        fn attach(&mut self, _: SettingsPageContext) {}
        fn load(&mut self, state: &SettingsFormState) {
            self.loads.borrow_mut().push(state.actions_json.clone());
        }
        fn collect(&mut self, state: &mut SettingsFormState) {
            state.logging_enabled = true;
        }
    }
    #[test]
    fn collect_and_load_skip_the_running_page_and_keep_other_created_pages() {
        let content = super::super::frame::test_content();
        let loads = Rc::new(RefCell::new(Vec::new()));
        for (i, name) in ["General", "Display"].into_iter().enumerate() {
            *content.pages[i].borrow_mut() = Box::new(Page {
                name,
                loads: loads.clone(),
            });
            content.view.pages.borrow_mut()[i].created = true;
        }
        let dialog = NativeDialog::new(content.clone());
        let _running = content.pages[0].borrow_mut();
        let state = content.collect_all();
        assert!(state.logging_enabled, "other created page was not collected");
        content.load_all(&dialog, &state);
        assert_eq!(
            loads.borrow().len(),
            1,
            "borrowed page was loaded or remaining page was skipped"
        );
        content.set_actions_json(&dialog, "[]", "General");
        assert_eq!(&*loads.borrow(), &[state.actions_json, "[]".to_owned()]);
        assert_eq!(content.actions_json(), "[]");
        assert!(!content.view.loading.get());
    }

    #[test]
    fn set_actions_json_preserves_unsaved_fields_and_does_not_reload_the_source_page() {
        let content = super::super::frame::test_content();
        let first = Rc::new(RefCell::new(Vec::new()));
        let second = Rc::new(RefCell::new(Vec::new()));
        for (i, (name, loads)) in [("General", first.clone()), ("Display", second.clone())]
            .into_iter()
            .enumerate()
        {
            *content.pages[i].borrow_mut() = Box::new(Page { name, loads });
            content.view.pages.borrow_mut()[i].created = true;
        }
        let dialog = NativeDialog::new(content.clone());
        content.set_actions_json(&dialog, "", "General");
        assert!(first.borrow().is_empty());
        assert_eq!(&*second.borrow(), &[String::new()]);
        let state = content.collect_all();
        assert!(state.logging_enabled);
        assert_eq!(state.actions_json, "");
    }
}
