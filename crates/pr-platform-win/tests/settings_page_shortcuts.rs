use std::collections::{BTreeMap, BTreeSet};

use pr_app::presenters::settings_field_map::shortcut_properties;
use pr_app::presenters::{settings_field_map::shortcut_get, settings_form_state::SettingsFormState};
use pr_core::settings::shortcuts::{ReviewAction, ShortcutMappings};
use pr_imaging::decoder_traits::ImageDecoderFactory;
use pr_platform_win::dialogs::settings::pages::shortcuts::{
    ACTION_ROWS, EN_CHANGE, MARKER, NAVIGATION_ROWS, ShortcutsSettingsPage, VIEW_ROWS, all_rows, find_problems,
};
use pr_platform_win::dialogs::settings::{
    builder::inputs::SettingsPageBuilder,
    context::{ActionProfilesUnavailable, SettingsPageHost, SettingsUiPost, SharedSettingsPresenter},
    page_view::{SettingsPageView, TextFn},
};
use pr_platform_win::win32::dialog_native::BN_CLICKED;
use pr_platform_win::{dialogs::file_dialog::ShellFileDialog, win32::structs::Hwnd};
use std::{cell::RefCell, rc::Rc, sync::Arc};

#[path = "common/settings_page_fixture.rs"]
mod settings_page_fixture;
use settings_page_fixture::PageHost;

#[derive(Default)]
struct TooltipHost {
    inner: PageHost,
    tooltips: RefCell<Vec<(i32, String)>>,
}

impl SettingsPageHost for TooltipHost {
    fn presenter(&self) -> SharedSettingsPresenter {
        self.inner.presenter()
    }
    fn hwnd(&self) -> Hwnd {
        self.inner.hwnd()
    }
    fn files(&self) -> Option<Rc<dyn ShellFileDialog>> {
        self.inner.files()
    }
    fn decoder_factory(&self) -> Option<Arc<dyn ImageDecoderFactory>> {
        self.inner.decoder_factory()
    }
    fn version(&self) -> Option<String> {
        self.inner.version()
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
    fn control_text(&self, id: i32) -> String {
        self.inner.control_text(id)
    }
    fn set_control_text(&self, id: i32, text: &str) {
        self.inner.set_control_text(id, text)
    }
    fn is_control_checked(&self, id: i32) -> bool {
        self.inner.is_control_checked(id)
    }
    fn set_control_checked(&self, id: i32, checked: bool) {
        self.inner.set_control_checked(id, checked)
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
        self.inner.slider_value(id)
    }
    fn set_slider_value(&self, id: i32, value: i32) {
        self.inner.set_slider_value(id, value)
    }
    fn set_slider_range(&self, id: i32, min: i32, max: i32) {
        self.inner.set_slider_range(id, min, max)
    }
    fn set_control_tooltip(&self, id: i32, text: TextFn) {
        self.tooltips.borrow_mut().push((id, text()));
    }
    fn current_shortcut_texts(&self) -> BTreeMap<String, String> {
        self.inner.current_shortcut_texts()
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
        self.inner.actions_json()
    }
    fn refresh_actions_json_from_pages(&self) -> String {
        self.inner.refresh_actions_json_from_pages()
    }
    fn set_actions_json(&self, json: &str, source: &str) {
        self.inner.set_actions_json(json, source)
    }
}

fn attached_page() -> (ShortcutsSettingsPage, std::rc::Rc<PageHost>, SettingsPageBuilder) {
    let mut page = ShortcutsSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(5000);
    page.declare(&mut builder);
    let host = PageHost::new();
    host.configure_from(&builder);
    page.attach(host.context());
    (page, host, builder)
}

// C#: SettingsShortcutsPageTests.Page_HasOneKeyBoxPerFieldMapShortcut_OnlyTheOptionalOnesHaveAClearButton
#[test]
fn page_has_one_key_box_per_field_map_shortcut_only_optional_rows_have_clear_buttons() {
    let rows: Vec<_> = all_rows().collect();
    let properties: BTreeSet<_> = rows.iter().map(|row| row.property).collect();
    let expected: BTreeSet<_> = shortcut_properties().iter().copied().collect();

    assert_eq!(rows.len(), 27);
    assert_eq!(properties, expected);
    assert_eq!(rows.iter().filter(|row| row.clear_tip.is_some()).count(), 14);
    for row in rows {
        assert_eq!(row.clear_tip.is_some(), ShortcutMappings::is_optional(row.property));
    }
}

// C#: SettingsShortcutsPageTests.RowOrder_IsTheDocumentedReadingOrder_OfThe27Commands
#[test]
fn row_order_is_the_documented_reading_order_of_the_27_commands() {
    assert_eq!(NAVIGATION_ROWS.len(), 8);
    assert_eq!(VIEW_ROWS.len(), 13);
    assert_eq!(ACTION_ROWS.len(), 6);
    let rows: Vec<_> = all_rows().map(|row| row.property).collect();
    let expected: Vec<_> = NAVIGATION_ROWS
        .iter()
        .chain(VIEW_ROWS.iter())
        .chain(ACTION_ROWS.iter())
        .map(|row| row.property)
        .collect();
    assert_eq!(rows, expected);
}

// C#: SettingsShortcutsPageTests.Labels_AreTheCommandNames_InEnglishAndVietnamese
#[test]
fn row_tables_have_labels_and_optional_clear_tips() {
    let rows: Vec<_> = all_rows().collect();
    assert_eq!(MARKER, "⚠ ");
    assert!(rows.iter().all(|row| !(row.label)().is_empty()));
    assert!(
        rows.iter()
            .filter_map(|row| row.clear_tip.map(|tip| tip()))
            .all(|tip| !tip.is_empty())
    );
}

// C#: SettingsShortcutsPageTests.FindProblems_FlagsEmptyMandatory_AndInvalidNames_ButNotEmptyOptional
#[test]
fn find_problems_flags_missing_mandatory_and_invalid_names_but_allows_empty_optional() {
    let mut texts = BTreeMap::new();
    for property in shortcut_properties() {
        texts.insert((*property).to_owned(), "A".to_owned());
    }
    let mandatory = shortcut_properties()
        .iter()
        .copied()
        .find(|property| !ShortcutMappings::is_optional(property))
        .expect("test data has mandatory shortcut properties");
    let optional = shortcut_properties()
        .iter()
        .copied()
        .find(|property| ShortcutMappings::is_optional(property))
        .expect("test data has optional shortcut properties");
    texts.insert(mandatory.to_owned(), String::new());
    texts.insert(optional.to_owned(), String::new());
    let problems = find_problems(&texts);
    assert!(problems.contains(mandatory));
    assert!(!problems.contains(optional));

    texts.insert(mandatory.to_owned(), "not a key".to_owned());
    assert!(find_problems(&texts).contains(mandatory));
}

// C#: SettingsShortcutsPageTests.TwoOptionalBoxesWithTheSameKey_AreAConflict_ButTwoEmptyOnesAreNot
#[test]
fn find_problems_marks_duplicate_keys_and_ignores_two_empty_optional_values() {
    let optional: Vec<_> = shortcut_properties()
        .iter()
        .copied()
        .filter(|property| ShortcutMappings::is_optional(property))
        .take(2)
        .collect();
    assert_eq!(optional.len(), 2);
    let mut texts = BTreeMap::new();
    texts.insert(optional[0].to_owned(), String::new());
    texts.insert(optional[1].to_owned(), String::new());
    assert!(find_problems(&texts).is_empty());

    texts.insert(optional[0].to_owned(), "F8".to_owned());
    texts.insert(optional[1].to_owned(), "F8".to_owned());
    assert_eq!(
        find_problems(&texts),
        optional.iter().map(|property| (*property).to_owned()).collect()
    );
}

// C#: SettingsShortcutsPageTests.AnAliasSpelling_CountsAsTheSameKey
#[test]
fn find_problems_counts_alias_spellings_as_the_same_key() {
    let rows: Vec<_> = all_rows().collect();
    let properties: Vec<_> = rows
        .iter()
        .filter(|row| !ShortcutMappings::is_optional(row.property))
        .take(2)
        .map(|row| row.property)
        .collect();
    let mut texts = BTreeMap::new();
    texts.insert(properties[0].to_owned(), "Return".to_owned());
    texts.insert(properties[1].to_owned(), "Enter".to_owned());
    assert_eq!(
        find_problems(&texts),
        properties.iter().map(|property| (*property).to_owned()).collect()
    );
}

// C#: SettingsShortcutsPageTests.Page_HasOneKeyBoxPerFieldMapShortcut_OnlyTheOptionalOnesHaveAClearButton
#[test]
fn page_accessors_return_exact_cells_key_and_sorted_marks() {
    let (page, _, _) = attached_page();
    assert_eq!(page.key(), "Shortcuts");
    let next = page.cell("Next").expect("declared Next cell");
    assert!(next.label > 0);
    assert!(next.box_id > next.label);
    assert_eq!(next.clear, 0);
    let last = page.cell("LastImage").expect("declared LastImage cell");
    assert!(last.clear > last.box_id);
    assert!(page.cell("NoSuchShortcut").is_none());
    assert_eq!(page.marked_properties(), Vec::<String>::new());
}

// C#: SettingsShortcutsPageTests.ClearButtons_HaveATooltip_OneToolPerOptionalShortcut
#[test]
fn attach_registers_each_nonzero_optional_clear_tooltip() {
    let mut page = ShortcutsSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(5000);
    page.declare(&mut builder);
    let host = Rc::new(TooltipHost::default());
    host.inner.configure_from(&builder);
    page.attach(pr_platform_win::dialogs::settings::context::SettingsPageContext::new(
        host.clone(),
    ));

    let expected: BTreeMap<_, _> = all_rows()
        .filter_map(|row| {
            let cell = page.cell(row.property)?;
            let tip = row.clear_tip?;
            (cell.clear != 0).then(|| (cell.clear, tip()))
        })
        .collect();
    let actual: BTreeMap<_, _> = host.tooltips.borrow().iter().cloned().collect();
    assert_eq!(actual.len(), 14);
    assert_eq!(actual, expected);
}

// C#: SettingsShortcutsPageTests.EveryShortcut_LoadsIntoItsBox_AndCollectsAndSavesBack
#[test]
fn load_and_collect_copy_shortcut_texts_without_canonicalizing() {
    let (mut page, host, _) = attached_page();
    let cell = page.cell("Next").expect("Next cell");
    let mut state = SettingsFormState::default();
    state.shortcut_texts.insert("Next".into(), " Space ".into());
    page.load(&state);
    assert_eq!(host.text(cell.box_id), " Space ");

    host.put_text(cell.box_id, "return");
    let mut collected = SettingsFormState::default();
    page.collect(&mut collected);
    assert_eq!(collected.shortcut_texts.get("Next").map(String::as_str), Some("return"));
}

// C#: SettingsShortcutsPageTests.LoadWithAMissingProperty_WritesAnEmptyBox
#[test]
fn load_with_missing_shortcut_writes_an_empty_box() {
    let (mut page, host, _) = attached_page();
    let cell = page.cell("Next").expect("Next cell");
    host.put_text(cell.box_id, "old");
    page.load(&SettingsFormState::default());
    assert_eq!(host.text(cell.box_id), "");
}

// C#: SettingsShortcutsPageTests.ClearButton_EmptiesAnOptionalBox_AndSaveAcceptsIt
#[test]
fn clear_command_empties_only_the_target_and_handles_the_click() {
    let (mut page, host, _) = attached_page();
    let cell = page.cell("LastImage").expect("optional cell");
    let other = page.cell("OpenFolder").expect("second optional cell");
    host.put_text(cell.box_id, "F8");
    host.put_text(other.box_id, "F9");
    assert!(page.on_command(cell.clear, BN_CLICKED));
    assert_eq!(host.text(cell.box_id), "");
    assert_eq!(host.text(other.box_id), "F9");
    assert!(!page.on_command(0, BN_CLICKED));
}

// C#: SettingsShortcutsPageTests.ResetKeys_PutsEveryBoxBackToItsDefault_AndLeavesTheActionsAlone
#[test]
fn reset_command_restores_all_default_mappings() {
    let (mut page, host, _) = attached_page();
    for row in all_rows() {
        let cell = page.cell(row.property).expect("every row has a cell");
        host.put_text(cell.box_id, "F12");
    }
    let reset = page.ids().reset_keys;
    assert!(page.on_command(reset, BN_CLICKED));
    let defaults = ShortcutMappings::default();
    for property in shortcut_properties() {
        let cell = page.cell(property).expect("field-map shortcut has a cell");
        assert_eq!(host.text(cell.box_id), shortcut_get(&defaults, property));
    }
}

// C#: SettingsShortcutsPageTests.TwoBoxesWithTheSameKey_ShowAWarning_AndAMarkOnBothLabels_UntilFixed
#[test]
fn changing_a_key_refreshes_warning_and_exact_sorted_marks() {
    let (mut page, host, _) = attached_page();
    let defaults = ShortcutMappings::default();
    for property in shortcut_properties() {
        if let Some(cell) = page.cell(property) {
            host.put_text(cell.box_id, shortcut_get(&defaults, property));
        }
    }
    let first = page.cell("Next").expect("Next cell");
    let second = page.cell("Previous").expect("Previous cell");
    host.put_text(first.box_id, "F8");
    host.put_text(second.box_id, "F8");
    assert!(page.on_command(second.box_id, EN_CHANGE));
    assert_eq!(page.marked_properties(), vec!["Next".to_owned(), "Previous".to_owned()]);
    assert!(host.text(first.label).starts_with(MARKER));
    assert!(host.text(second.label).starts_with(MARKER));
    assert!(!host.text(page.ids().warning).is_empty());

    host.put_text(second.box_id, "F9");
    assert!(page.on_command(second.box_id, EN_CHANGE));
    assert!(page.marked_properties().is_empty());
    assert_eq!(
        host.text(first.label),
        (all_rows().find(|r| r.property == "Next").unwrap().label)()
    );
    assert_eq!(
        host.text(second.label),
        (all_rows().find(|r| r.property == "Previous").unwrap().label)()
    );
}

// C#: SettingsShortcutsPageTests.OnCommand_IgnoresUnrelatedIdsAndNotifications
#[test]
fn command_dispatch_distinguishes_box_edits_clicks_and_other_notifications() {
    let (mut page, _, _) = attached_page();
    let box_id = page.cell("Next").expect("Next cell").box_id;
    assert!(!page.on_command(box_id, BN_CLICKED));
    assert!(!page.on_command(-1, EN_CHANGE));
    assert!(page.on_command(box_id, EN_CHANGE));
    assert!(!page.on_command(-1, 0));
    assert!(!page.on_command(0, BN_CLICKED));
}

// C#: SettingsShortcutsPageTests.ChangingTheLanguage_RelabelsEveryRow_KeepsTheKeys_AndTheMarkers
#[test]
fn language_change_reissues_warning_text_and_keeps_key_and_mark_state() {
    let (mut page, host, _) = attached_page();
    let cell = page.cell("Next").expect("Next cell");
    host.put_text(cell.box_id, "");
    page.on_command(cell.box_id, EN_CHANGE);
    let marks = page.marked_properties();
    host.clear_calls();
    page.on_language_changed();
    assert_eq!(page.marked_properties(), marks);
    assert!(
        host.calls()
            .iter()
            .any(|call| call.starts_with(&format!("set_text {} ", page.ids().warning)))
    );
}

// C#: SettingsShortcutsPageTests.WhenTheEditorCannotBeOpened_TheUserIsToldAndNothingChanges
#[test]
fn edit_action_command_reports_unusable_action_json_and_handles_the_click() {
    let (mut page, host, _) = attached_page();
    assert!(page.on_command(page.ids().edit_actions, BN_CLICKED));
    assert_eq!(host.warnings().len(), 1);
    assert!(!page.on_command(page.ids().edit_actions, EN_CHANGE));
}
