//! Pure tests for the settings page framework contract (W8-19a: `page_view`, `catalog`, `context`). Contract: `contracts/W8-19a.md`, "Test plan".
//!
//! No C# test targets `SettingsPageContext` directly (the dialog tests need a window), so most of these are new (`// new:`); the catalog tests port the catalog
//! half of `SettingsDialogTests.ARealPageIsBuilt_ForEveryNavigationEntry_AndPendingOnesAreAnnounced` and `SettingsFilesPageTests` line 546. The golden is
//! `tests/golden/settings-catalog.json` (read through `pr_test_support::golden`).

mod common {
    pub mod settings_host_fixture;
}

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use common::settings_host_fixture::RecordingHost;
use pr_app::presenters::settings::SettingsExplorerRequest;
use pr_app::presenters::settings::load::PAGES;
use pr_app::presenters::settings_form_state::{SettingsFocusField, SettingsFormState};
use pr_core::settings::shortcuts::ReviewAction;
use pr_l10n::catalog::PluralRule;
use pr_l10n::keys;
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_l10n::template::LocTemplate;
use pr_platform_win::dialogs::native::template::DialogControlClass;
use pr_platform_win::dialogs::settings::builder::inputs::SettingsPageBuilder;
use pr_platform_win::dialogs::settings::catalog::{
    FACTORIES, PageFactory, PendingSettingsPage, create_pages, create_pages_with, implemented_keys, is_complete,
    pending_keys, pending_keys_with,
};
use pr_platform_win::dialogs::settings::context::{
    ActionProfilesUnavailable, ContextUiAction, SettingsPageContext, SettingsUiPost, with_field_context,
};
use pr_platform_win::dialogs::settings::page_view::{
    PageContextSlot, SettingsPageView, fixed_text, page_title, text_fn,
};

// ---------------------------------------------------------------- language lock

/// The ambient localizer is process-wide: every test that reads it, and the one that replaces it, hold this lock and run in built-in English.
static LANGUAGE: Mutex<()> = Mutex::new(());

struct English {
    previous: std::sync::Arc<Localizer>,
    _lock: MutexGuard<'static, ()>,
}

fn english() -> English {
    let lock = LANGUAGE.lock().unwrap_or_else(PoisonError::into_inner);
    let previous = Localizer::current();
    Localizer::set_current(built_in_english_localizer());
    English { previous, _lock: lock }
}

impl Drop for English {
    fn drop(&mut self) {
        Localizer::set_current(Arc::clone(&self.previous));
    }
}

// ---------------------------------------------------------------- page_view

struct MinimalPage {
    key: String,
}

impl SettingsPageView for MinimalPage {
    fn key(&self) -> &str {
        &self.key
    }

    fn declare(&mut self, _page: &mut SettingsPageBuilder) {}

    fn attach(&mut self, _context: SettingsPageContext) {}

    fn load(&mut self, _state: &SettingsFormState) {}

    fn collect(&mut self, _state: &mut SettingsFormState) {}
}

const NAV_KEYS: [&str; 7] = [
    "General",
    "Display",
    "Mouse",
    "Performance",
    "Shortcuts",
    "Files",
    "Diagnostics",
];

#[test]
#[rustfmt::skip]
fn default_methods_do_nothing_and_answer_false_or_none() {
    // C#: SettingsPageBase defaults (OnCommand false, FocusTarget null, FocusExternalEditor false, the rest empty).
    let mut page = MinimalPage {
        key: "Nope".to_owned(),
    };
    assert!(!page.on_command(10, 2));
    page.on_scroll(10);
    page.on_language_changed();
    assert_eq!(page.focus_target(SettingsFocusField::default()), None);
    assert!(!page.focus_external_editor());
    page.on_closed();
    page.on_activated();
}

#[test]
fn title_defaults_to_the_localized_page_title() {
    // C#: SettingsPageBase.Title => SettingsPresenter.PageTitle(Key)
    let _english = english();
    for page_def in PAGES.iter() {
        let page = MinimalPage {
            key: page_def.key.to_owned(),
        };
        let expected = Localizer::current().get(page_def.title_key);
        assert!(!expected.is_empty());
        assert_eq!(page.title(), expected, "{}", page_def.key);
        assert_eq!(page_title(page_def.key), expected, "{}", page_def.key);
    }
}

#[test]
fn title_of_an_unknown_key_is_empty() {
    let _english = english();
    assert_eq!(page_title("Nope"), "");
    assert_eq!(page_title(""), "");
    assert_eq!(page_title("general"), "");
}

struct OverriddenPage(MinimalPage);

#[rustfmt::skip]
impl SettingsPageView for OverriddenPage {
    fn key(&self) -> &str { &self.0.key }
    fn title(&self) -> String { "custom title".to_owned() }
    fn declare(&mut self, _page: &mut SettingsPageBuilder) {}
    fn attach(&mut self, _context: SettingsPageContext) {}
    fn load(&mut self, _state: &SettingsFormState) {}
    fn collect(&mut self, _state: &mut SettingsFormState) {}
}

#[test]
#[rustfmt::skip]
fn title_can_be_overridden() {
    let page = OverriddenPage(MinimalPage { key: "General".to_owned() });
    assert_eq!(page.title(), "custom title");
}

#[test]
fn context_slot_is_detached_until_attach() {
    // new: D-W8B5-705 (C# `Context` is `null!` before Attach).
    let slot = PageContextSlot::new();
    assert!(!slot.is_attached());
    let context = slot.context();
    context.set_text(1, "ignored");
    context.set_checked(2, true);
    assert_eq!(context.text(1), "");
    assert!(!context.is_checked(2));
    assert_eq!(context.combo_index(3), -1);
    assert_eq!(context.slider_value(4), 0);
}

#[test]
fn context_slot_returns_the_attached_context_and_attach_twice_replaces_it() {
    // new
    let first = RecordingHost::new();
    let second = RecordingHost::new();
    let mut slot = PageContextSlot::new();
    slot.attach(first.context());
    assert!(slot.is_attached());
    slot.context().set_text(1, "a");
    assert_eq!(first.calls(), ["set_text 1 a"]);
    slot.attach(second.context());
    slot.context().set_text(2, "b");
    assert_eq!(first.calls(), ["set_text 1 a"], "the first host is no longer written");
    assert_eq!(second.calls(), ["set_text 2 b"]);
}

#[test]
fn text_fn_and_fixed_text_return_the_text_each_time() {
    let fixed = fixed_text("fixed");
    assert_eq!(fixed(), "fixed");
    assert_eq!(fixed(), "fixed");
    let dynamic = text_fn(|| "dynamic".to_owned());
    assert_eq!(dynamic(), "dynamic");
    assert_eq!(dynamic(), "dynamic");
}

#[test]
fn golden_page_title_rows() {
    // Oracle `settings-catalog`, kind `page_title` (C# SettingsPresenter.PageTitle with the English default localizer).
    let _english = english();
    let golden = pr_test_support::golden::load("settings-catalog");
    let mut compared = 0;
    for case in golden.cases.iter().filter(|c| c["kind"] == "page_title") {
        let key = pr_test_support::golden::str_of(case, "key");
        let title = pr_test_support::golden::str_of(case, "title");
        assert_eq!(page_title(key), title, "{}", case["name"]);
        compared += 1;
    }
    assert_eq!(compared, 10);
}

// ---------------------------------------------------------------- context

#[test]
fn each_forwarder_calls_the_matching_host_method_with_the_same_arguments() {
    // new: the 29 one-line forwarders (contract semantic 6), exact call lines in order.
    let host = RecordingHost::new();
    let context = host.context();
    host.texts.borrow_mut().insert(3, "stored".to_owned());
    host.checked.borrow_mut().insert(5, true);
    host.combo.borrow_mut().insert(6, 4);
    host.slider.borrow_mut().insert(7, 42);
    host.enabled.borrow_mut().insert(9, true);
    host.shortcut_texts
        .borrow_mut()
        .insert("Zoom".to_owned(), "Z".to_owned());
    *host.actions_json.borrow_mut() = "[1]".to_owned();
    *host.refreshed_actions_json.borrow_mut() = "[2]".to_owned();
    *host.version.borrow_mut() = Some("1.2.3".to_owned());
    *host.open_url_result.borrow_mut() = Err("no browser".to_owned());

    let _ = context.presenter();
    assert_eq!(context.owner(), 0x1234);
    assert!(context.files().is_none());
    assert!(context.decoder_factory().is_none());
    assert_eq!(context.version().as_deref(), Some("1.2.3"));
    assert_eq!(context.open_url("https://example.test"), Err("no browser".to_owned()));
    assert_eq!(context.text(3), "stored");
    context.set_text(3, "hello");
    assert!(context.is_checked(5));
    context.set_checked(5, false);
    assert_eq!(context.combo_index(6), 4);
    context.set_combo_index(6, 2);
    context.set_combo_items(6, &["a".to_owned(), "b".to_owned()], 1);
    assert_eq!(context.slider_value(7), 42);
    context.set_slider_value(7, 9);
    context.set_slider_range(7, 1, 10);
    context.set_tooltip(8, text_fn(|| "tip".to_owned()));
    assert_eq!(
        context.current_shortcut_texts().get("Zoom").map(String::as_str),
        Some("Z")
    );
    context.set_enabled(9, false);
    assert!(!context.is_enabled(9));
    context.set_visible(10, true);
    context.focus(11);
    context.relayout();
    context.show_warning("t", "m");
    let _ = context.ui_post();
    context.post_to_ui(Box::new(|_| {}));
    assert_eq!(context.actions_json(), "[1]");
    assert_eq!(context.collect_actions_json(), "[2]");
    context.commit_actions_json("{}", "Files");

    assert_eq!(
        host.calls(),
        [
            "presenter",
            "files",
            "decoder_factory",
            "version",
            "open_url https://example.test",
            "text 3",
            "set_text 3 hello",
            "is_checked 5",
            "set_checked 5 false",
            "combo_index 6",
            "set_combo_index 6 2",
            "set_combo_items 6 [a,b] 1",
            "slider_value 7",
            "set_slider_value 7 9",
            "set_slider_range 7 1 10",
            "set_tooltip 8 tip",
            "shortcut_texts",
            "set_enabled 9 false",
            "is_enabled 9",
            "set_visible 10 true",
            "focus 11",
            "relayout",
            "warn t|m",
            "ui_post",
            "ui_post",
            "actions_json",
            "refresh_actions_json",
            "set_actions_json {} Files",
        ]
    );
}

#[test]
fn owner_is_the_host_hwnd() {
    let host = RecordingHost::new();
    host.hwnd.set(0x77);
    assert_eq!(host.context().owner(), 0x77);
    assert!(host.calls().is_empty(), "hwnd is not recorded");
}

#[test]
fn set_tooltip_text_passes_a_fixed_provider() {
    let host = RecordingHost::new();
    let context = host.context();
    context.set_tooltip_text(8, "fixed tip");
    assert_eq!(host.calls(), ["set_tooltip 8 fixed tip"]);
    let provider = host.tooltips.borrow().get(&8).cloned().expect("provider stored");
    assert_eq!(provider(), "fixed tip");
    assert_eq!(provider(), "fixed tip");
}

#[test]
fn radio_index_is_the_first_checked_or_minus_one() {
    // new: C# RadioIndex(ids)
    let host = RecordingHost::new();
    let context = host.context();
    let ids = [10, 11, 12];
    assert_eq!(context.radio_index(&ids), -1, "none checked");
    host.checked.borrow_mut().insert(11, true);
    assert_eq!(context.radio_index(&ids), 1);
    host.checked.borrow_mut().insert(10, true);
    host.checked.borrow_mut().insert(12, true);
    assert_eq!(context.radio_index(&ids), 0, "first and third checked: the first wins");
    assert_eq!(context.radio_index(&[]), -1, "empty group");
}

#[test]
fn radio_index_stops_reading_at_the_first_checked() {
    let host = RecordingHost::new();
    host.checked.borrow_mut().insert(11, true);
    let context = host.context();
    assert_eq!(context.radio_index(&[10, 11, 12]), 1);
    assert_eq!(host.calls(), ["is_checked 10", "is_checked 11"]);
}

#[test]
fn set_radio_index_writes_every_radio_in_order() {
    // new: C# SetRadioIndex (BM_SETCHECK does not clear the siblings, so every id is written)
    let host = RecordingHost::new();
    host.context().set_radio_index(&[10, 11, 12], 1);
    assert_eq!(
        host.calls(),
        ["set_checked 10 false", "set_checked 11 true", "set_checked 12 false"]
    );
}

#[test]
fn set_radio_index_out_of_range_checks_none() {
    for index in [3, -1, i32::MAX, i32::MIN] {
        let host = RecordingHost::new();
        host.context().set_radio_index(&[10, 11, 12], index);
        assert_eq!(
            host.calls(),
            ["set_checked 10 false", "set_checked 11 false", "set_checked 12 false"],
            "index {index}"
        );
    }
    let host = RecordingHost::new();
    host.context().set_radio_index(&[], 0);
    assert!(host.calls().is_empty());
}

fn open_failed_title() -> String {
    let title = Localizer::current().get(keys::DIALOG_SETTINGS_OPEN_FAILED_TITLE);
    assert!(!title.is_empty());
    assert_ne!(
        title,
        keys::DIALOG_SETTINGS_OPEN_FAILED_TITLE,
        "the key exists in the catalog"
    );
    title
}

#[test]
fn run_explorer_with_an_error_message_warns_and_does_not_start() {
    // new: C# RunExplorer, ErrorMessage branch
    let _english = english();
    let host = RecordingHost::new();
    let request = SettingsExplorerRequest {
        arguments: Some("/select,x".to_owned()),
        error_message: Some("cannot".to_owned()),
    };
    host.context().run_explorer(&request);
    assert_eq!(host.warnings(), [(open_failed_title(), "cannot".to_owned())]);
    assert_eq!(host.calls().len(), 1, "no start_explorer call: {:?}", host.calls());
}

#[test]
fn run_explorer_without_arguments_does_nothing() {
    let _english = english();
    let host = RecordingHost::new();
    let request = SettingsExplorerRequest {
        arguments: None,
        error_message: None,
    };
    host.context().run_explorer(&request);
    assert!(host.calls().is_empty());
}

#[test]
fn run_explorer_start_error_warns_with_the_reason() {
    let _english = english();
    let host = RecordingHost::new();
    *host.explorer_error.borrow_mut() = Some("denied".to_owned());
    let request = SettingsExplorerRequest {
        arguments: Some("/select,x".to_owned()),
        error_message: None,
    };
    host.context().run_explorer(&request);
    assert_eq!(
        host.calls(),
        [
            "start_explorer /select,x".to_owned(),
            format!("warn {}|denied", open_failed_title())
        ]
    );
}

#[test]
fn run_explorer_success_shows_nothing() {
    let _english = english();
    let host = RecordingHost::new();
    let request = SettingsExplorerRequest {
        arguments: Some("/select,x".to_owned()),
        error_message: None,
    };
    host.context().run_explorer(&request);
    assert_eq!(host.calls(), ["start_explorer /select,x"]);
    assert!(host.warnings().is_empty());
}

#[test]
fn edit_action_profiles_returns_the_edited_list_and_passes_owner_and_list() {
    // new: C# EditActionProfiles, Options.ShowActionProfiles result
    let host = RecordingHost::new();
    host.hwnd.set(0x99);
    let edited = vec![ReviewAction::new("edited")];
    *host.action_profiles.borrow_mut() = Ok(Some(edited.clone()));
    let given = vec![ReviewAction::new("one"), ReviewAction::new("two")];
    assert_eq!(host.context().edit_action_profiles(&given), Some(edited));
    assert_eq!(host.last_profiles_call.borrow().clone(), Some((0x99, given)));
    assert!(host.warnings().is_empty());
}

#[test]
fn edit_action_profiles_cancelled_is_none_without_a_notice() {
    let host = RecordingHost::new();
    *host.action_profiles.borrow_mut() = Ok(None);
    assert_eq!(host.context().edit_action_profiles(&[]), None);
    assert!(host.warnings().is_empty());
}

#[test]
fn edit_action_profiles_unavailable_warns_with_the_editor_texts_and_returns_none() {
    let _english = english();
    let host = RecordingHost::new();
    *host.action_profiles.borrow_mut() = Err(ActionProfilesUnavailable);
    assert_eq!(host.context().edit_action_profiles(&[]), None);
    let localizer = Localizer::current();
    let title = localizer.get(keys::DIALOG_OPEN_EDITOR_FAILED_TITLE);
    let message = localizer.get(keys::DIALOG_OPEN_EDITOR_FAILED_MESSAGE);
    assert!(!title.is_empty() && !message.is_empty());
    assert_ne!(title, message);
    assert_eq!(host.warnings(), [(title, message)]);
}

#[test]
fn post_to_ui_runs_the_action_with_the_context_through_the_host_poster() {
    let host = RecordingHost::new();
    host.context()
        .post_to_ui(Box::new(|context| context.set_text(1, "posted")));
    assert_eq!(host.calls(), ["ui_post", "set_text 1 posted"]);
}

#[test]
fn ui_post_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    fn assert_send<T: Send>() {}
    assert_send_sync::<dyn SettingsUiPost>();
    assert_send::<ContextUiAction>();
    let host = RecordingHost::new();
    let poster = host.context().ui_post();
    poster.post(Box::new(|context| context.focus(5)));
    assert_eq!(host.calls(), ["ui_post", "focus 5"]);
}

#[test]
fn commit_actions_json_passes_the_source_key() {
    let host = RecordingHost::new();
    host.context().commit_actions_json("[]", "Shortcuts");
    assert_eq!(host.calls(), ["set_actions_json [] Shortcuts"]);
    assert_eq!(host.context().actions_json(), "[]");
}

#[test]
fn detached_context_reads_neutral_values_and_writes_nothing() {
    // new: contract semantic 5
    let _english = english();
    let context = SettingsPageContext::detached();
    assert_eq!(context.owner(), 0);
    assert!(context.files().is_none());
    assert!(context.decoder_factory().is_none());
    assert!(context.version().is_none());
    assert_eq!(context.open_url("https://example.test"), Err("detached".to_owned()));
    assert_eq!(context.text(1), "");
    assert!(!context.is_checked(1));
    assert_eq!(context.radio_index(&[1, 2]), -1);
    assert_eq!(context.combo_index(1), -1);
    assert_eq!(context.slider_value(1), 0);
    assert!(!context.is_enabled(1));
    assert!(context.current_shortcut_texts().is_empty());
    assert_eq!(context.actions_json(), "");
    assert_eq!(context.collect_actions_json(), "");
    context.set_text(1, "x");
    context.set_checked(1, true);
    context.set_radio_index(&[1, 2], 0);
    context.set_combo_index(1, 3);
    context.set_combo_items(1, &["a".to_owned()], 0);
    context.set_slider_value(1, 5);
    context.set_slider_range(1, 0, 9);
    context.set_tooltip_text(1, "tip");
    context.set_enabled(1, true);
    context.set_visible(1, false);
    context.focus(1);
    context.relayout();
    context.show_warning("t", "m");
    context.commit_actions_json("{}", "Files");
    assert_eq!(context.text(1), "");
    assert!(!context.is_checked(1));
    // The editor and the explorer are "unavailable": the detached host warns nowhere and never panics.
    assert_eq!(context.edit_action_profiles(&[]), None);
    context.run_explorer(&SettingsExplorerRequest {
        arguments: Some("/select,x".to_owned()),
        error_message: None,
    });
    // One shared presenter per thread.
    assert!(std::rc::Rc::ptr_eq(
        &context.presenter(),
        &SettingsPageContext::detached().presenter()
    ));
}

#[test]
fn detached_post_to_ui_drops_the_action() {
    let ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&ran);
    SettingsPageContext::detached().post_to_ui(Box::new(move |_| flag.store(true, Ordering::SeqCst)));
    assert!(!ran.load(Ordering::SeqCst));
}

#[test]
fn with_field_context_uses_the_current_language() {
    // new: D-W8B5-710
    let english = english();
    let key = "settings.nav.general";
    assert_eq!(with_field_context(|ctx| ctx.texts.get(key)), "General");
    let marked = built_in_english_localizer().transform("zz", "ZZ", PluralRule::OneOther, |key, _| {
        LocTemplate::literal(&format!("[{key}]"))
    });
    Localizer::set_current(Arc::new(marked));
    assert_eq!(with_field_context(|ctx| ctx.texts.get(key)), "[settings.nav.general]");
    assert_eq!(page_title("General"), "[settings.nav.general]");
    drop(english);
}

#[test]
fn contexts_are_cheap_clones_sharing_the_host() {
    let host = RecordingHost::new();
    let first = host.context();
    let second = first.clone();
    first.set_text(1, "a");
    second.set_text(2, "b");
    assert_eq!(host.calls(), ["set_text 1 a", "set_text 2 b"]);
}

// ---------------------------------------------------------------- catalog

struct FakePage {
    key: &'static str,
}

impl SettingsPageView for FakePage {
    fn key(&self) -> &str {
        self.key
    }
    fn title(&self) -> String {
        format!("fake:{}", self.key)
    }
    fn declare(&mut self, _page: &mut SettingsPageBuilder) {}
    fn attach(&mut self, _context: SettingsPageContext) {}
    fn load(&mut self, _state: &SettingsFormState) {}
    fn collect(&mut self, _state: &mut SettingsFormState) {}
}

fn fake_files() -> Box<dyn SettingsPageView> {
    Box::new(FakePage { key: "Files" })
}
fn fake_mouse() -> Box<dyn SettingsPageView> {
    Box::new(FakePage { key: "Mouse" })
}

const FILES_AND_MOUSE: &[(&str, PageFactory)] = &[("Files", fake_files), ("Mouse", fake_mouse)];

fn nav_keys() -> Vec<&'static str> {
    PAGES.iter().map(|page| page.key).collect()
}

#[test]
fn pending_keys_are_the_nav_keys_without_a_factory_in_nav_order() {
    // C#: SettingsDialogTests.ARealPageIsBuilt_ForEveryNavigationEntry_AndPendingOnesAreAnnounced (catalog half)
    assert_eq!(
        pending_keys_with(&NAV_KEYS, FILES_AND_MOUSE),
        ["General", "Display", "Performance", "Shortcuts", "Diagnostics"]
    );
}

#[test]
fn pending_keys_with_an_empty_table_is_the_whole_nav() {
    assert_eq!(pending_keys_with(&NAV_KEYS, &[]), NAV_KEYS);
    assert!(pending_keys_with(&[], FILES_AND_MOUSE).is_empty());
}

#[test]
fn is_complete_is_true_only_when_every_nav_key_has_a_factory() {
    // C#: SettingsPortTests.TheRemovedEnvironmentFlag_HasNoEffect_SettingsFollowsTheCatalogOnly (IsComplete is the only thing the router reads)
    let nav = nav_keys();
    assert!(pending_keys_with(&nav, FACTORIES).is_empty());
    assert!(!pending_keys_with(&nav, FILES_AND_MOUSE).is_empty());
    assert!(is_complete());
    assert!(pending_keys().is_empty());
}

#[test]
fn create_pages_with_returns_one_page_per_nav_key_in_nav_order() {
    let pages = create_pages_with(&["Mouse", "Files"], FILES_AND_MOUSE);
    let keys: Vec<&str> = pages.iter().map(|page| page.key()).collect();
    assert_eq!(keys, ["Mouse", "Files"]);
}

#[test]
fn create_pages_with_uses_a_pending_page_for_a_missing_factory() {
    let _english = english();
    let pages = create_pages_with(&["Files", "Display", "Mouse"], FILES_AND_MOUSE);
    let titles: Vec<String> = pages.iter().map(|page| page.title()).collect();
    // The fakes answer `fake:<key>`; the pending placeholder answers the localized page title of its key.
    assert_eq!(
        titles,
        ["fake:Files".to_owned(), page_title("Display"), "fake:Mouse".to_owned()]
    );
    assert_eq!(pages.get(1).map(|page| page.key()), Some("Display"));
}

static NEVER_CALLED: AtomicUsize = AtomicUsize::new(0);
static CALLED: AtomicUsize = AtomicUsize::new(0);

fn counted_unlisted() -> Box<dyn SettingsPageView> {
    NEVER_CALLED.fetch_add(1, Ordering::SeqCst);
    Box::new(FakePage { key: "Unlisted" })
}
fn counted_files() -> Box<dyn SettingsPageView> {
    CALLED.fetch_add(1, Ordering::SeqCst);
    Box::new(FakePage { key: "Files" })
}

#[test]
fn create_pages_with_never_calls_a_factory_whose_key_is_not_in_the_nav() {
    let table: &[(&str, PageFactory)] = &[("Files", counted_files), ("Unlisted", counted_unlisted)];
    let pages = create_pages_with(&["Files", "Display"], table);
    assert_eq!(pages.len(), 2);
    assert_eq!(CALLED.load(Ordering::SeqCst), 1);
    assert_eq!(NEVER_CALLED.load(Ordering::SeqCst), 0);
}

#[test]
#[ignore = "waits for W8-19b/19c: SettingsPageBuilder is a stub (todo!) until then"]
fn pending_page_declares_one_unaligned_note_with_the_pending_text() {
    // new: PendingSettingsPage.Declare
    let _english = english();
    let mut builder = SettingsPageBuilder::new(1000);
    PendingSettingsPage::new("Mouse").declare(&mut builder);
    assert_eq!(builder.items().len(), 1);
    let item = &builder.items()[0];
    assert_eq!(item.class, DialogControlClass::Static);
    let text = builder.texts().get(&item.id).expect("the note has a text provider");
    let expected = Localizer::current().get(keys::SETTINGS_PAGE_PENDING);
    assert!(!expected.is_empty());
    assert_eq!(text(), expected);
    assert!(builder.notes().contains(&item.id));
    assert_eq!(builder.elements().len(), 1);
}

#[test]
fn pending_page_load_and_collect_leave_the_state_untouched() {
    // C#: PendingSettingsPage.Load / Collect are empty (Save keeps the page's values as loaded)
    let mut page = PendingSettingsPage::new("Mouse");
    let mut state = SettingsFormState::default();
    let before = format!("{state:?}");
    page.load(&state);
    page.collect(&mut state);
    assert_eq!(format!("{state:?}"), before);
}

#[test]
fn pending_page_key_is_any_text() {
    for key in ["Mouse", "Future", "", "with space"] {
        assert_eq!(PendingSettingsPage::new(key).key(), key);
    }
}

#[test]
fn pending_page_focus_external_editor_is_false() {
    // C#: SettingsFilesPageTests line 546: PendingSettingsPage("Mouse").FocusExternalEditor() is false
    assert!(!PendingSettingsPage::new("Mouse").focus_external_editor());
}

#[test]
fn pending_page_title_follows_the_key() {
    let _english = english();
    assert_eq!(PendingSettingsPage::new("Mouse").title(), page_title("Mouse"));
    assert_eq!(PendingSettingsPage::new("Future").title(), "");
}

#[test]
fn golden_catalog_static_facts() {
    let golden = pr_test_support::golden::load("settings-catalog");
    let case = golden
        .cases
        .iter()
        .find(|c| c["kind"] == "catalog")
        .expect("the catalog row");
    let strings = |name: &str| -> Vec<String> {
        case[name]
            .as_array()
            .unwrap_or_else(|| panic!("no array {name}"))
            .iter()
            .map(|v| v.as_str().expect("string").to_owned())
            .collect()
    };
    assert_eq!(strings("implemented_keys"), implemented_keys());
    assert_eq!(
        FACTORIES.iter().map(|(key, _)| (*key).to_owned()).collect::<Vec<_>>(),
        strings("implemented_keys"),
        "FACTORIES is alphabetical, not navigation order"
    );
    assert!(strings("pending_keys").is_empty());
    assert!(pending_keys().is_empty());
    assert!(pr_test_support::golden::bool_of(case, "is_complete"));
    assert!(is_complete());
    assert_eq!(strings("nav_keys"), nav_keys());
}

#[test]
fn golden_pending_page_titles() {
    let _english = english();
    let golden = pr_test_support::golden::load("settings-catalog");
    let mut compared = 0;
    for case in golden.cases.iter().filter(|c| c["kind"] == "pending_page") {
        let key = pr_test_support::golden::str_of(case, "key");
        let page = PendingSettingsPage::new(key);
        assert_eq!(page.key(), pr_test_support::golden::str_of(case, "page_key"));
        assert_eq!(page.title(), pr_test_support::golden::str_of(case, "title"));
        assert_eq!(page_title(key), pr_test_support::golden::str_of(case, "title"));
        compared += 1;
    }
    assert_eq!(compared, 2);
}

#[test]
#[ignore = "waits for W8-19d..19i: page constructors are stubs until then"]
fn real_create_pages_returns_the_seven_pages_in_nav_order() {
    let golden = pr_test_support::golden::load("settings-catalog");
    let case = golden
        .cases
        .iter()
        .find(|c| c["kind"] == "catalog")
        .expect("the catalog row");
    let expected: Vec<String> = case["created_keys"]
        .as_array()
        .expect("created_keys")
        .iter()
        .map(|v| v.as_str().expect("string").to_owned())
        .collect();
    let pages = create_pages();
    let keys: Vec<String> = pages.iter().map(|page| page.key().to_owned()).collect();
    assert_eq!(keys, expected);
}
