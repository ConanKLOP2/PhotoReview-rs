//! Mutation killers for `dialogs/settings/{pages/general,pages/shortcuts,dialog/nav}.rs` (lane cl-mut-settings3). Inline fakes, no golden files; 5 native tests (hidden window),
//! no real config, no Recycle Bin. The page runs against a recording `SettingsPageHost`; the background runner is the caller's.
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use pr_app::localization::service::{CultureLookup, LocalizationPaths, LocalizationService};
use pr_app::presenters::settings::{SettingsPresenter, SettingsPresenterOptions};
use pr_app::presenters::settings_field_map::{shortcut_get, shortcut_properties};
use pr_core::abstractions::{FileSystem, Log, ShellIntegration, ShellMenuState};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::shortcuts::ReviewAction;
use pr_core::settings::shortcuts::ShortcutMappings;
use pr_core::sync::{CancelToken, Canceled};
use pr_core::updates::checker::UpdateCheck;
use pr_core::updates::version::UpdateCheckResult;
use pr_imaging::decoder_traits::ImageDecoderFactory;
use pr_l10n::loader::UiCulture;
use pr_l10n::localizer::Localizer;
use pr_platform_win::dialogs::file_dialog::ShellFileDialog;
use pr_platform_win::dialogs::settings::builder::inputs::SettingsPageBuilder;
use pr_platform_win::dialogs::settings::context::{
    ActionProfilesUnavailable, ContextUiAction, SettingsPageContext, SettingsPageHost, SettingsUiPost,
    SharedSettingsPresenter,
};
use pr_platform_win::dialogs::settings::dialog::nav::{
    DialogKeyAction, VK_CONTROL, VK_SHIFT, VK_TAB, classify_dialog_key, page_of,
};
use pr_platform_win::dialogs::settings::page_view::{SettingsPageView, TextFn};
use pr_platform_win::dialogs::settings::pages::general::{BackgroundRunner, GeneralSettingsPage};
use pr_platform_win::dialogs::settings::pages::shortcuts::{EN_CHANGE, MARKER, ShortcutsSettingsPage, all_rows};
use pr_platform_win::win32::dialog_native::BN_CLICKED;
use pr_platform_win::win32::structs::Hwnd;
use pr_test_support::{FakeFileSystem, RecordingLog, TempRoot};

const EXE: &str = r"C:\Apps\PhotoReview\PhotoReview.exe";
const RELEASE_URL: &str = "https://github.com/ConanKLOP2/PhotoReview-rs/releases/tag/v2.0.94";

// ------------------------------------------------------------------------------------------------------------------
// Fakes
// ------------------------------------------------------------------------------------------------------------------

#[derive(Debug)]
struct FakeShell {
    state: Mutex<ShellMenuState>,
    calls: Mutex<Vec<String>>,
}

impl FakeShell {
    fn new(state: ShellMenuState) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(state),
            calls: Mutex::new(Vec::new()),
        })
    }
    fn set(&self, state: ShellMenuState) {
        *self.state.lock().unwrap() = state;
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl ShellIntegration for FakeShell {
    fn get_state(&self, _: &str) -> ShellMenuState {
        *self.state.lock().unwrap()
    }
    fn register(&self, exe: &str) -> std::io::Result<()> {
        self.calls.lock().unwrap().push(format!("register:{exe}"));
        *self.state.lock().unwrap() = ShellMenuState::Registered;
        Ok(())
    }
    fn unregister(&self) -> std::io::Result<()> {
        self.calls.lock().unwrap().push("unregister".to_owned());
        *self.state.lock().unwrap() = ShellMenuState::NotRegistered;
        Ok(())
    }
}

struct FakeChecker(UpdateCheckResult);

impl UpdateCheck for FakeChecker {
    fn check(&self, _: Option<&str>, _: &CancelToken) -> Result<UpdateCheckResult, Canceled> {
        Ok(self.0.clone())
    }
}

struct KnownCultures;

impl CultureLookup for KnownCultures {
    fn lookup(&self, name: &str) -> Option<UiCulture> {
        matches!(name, "en" | "vi").then(|| UiCulture::from_name(name))
    }
}

fn service_in(root: &TempRoot) -> LocalizationService {
    let shipped = PathBuf::from(r"C:\App\Languages");
    let disk = Arc::new(FakeFileSystem::new());
    disk.add_dir(&shipped);
    let file_system: Arc<dyn FileSystem> = disk;
    let log: Arc<dyn Log> = Arc::new(RecordingLog::new());
    LocalizationService::new(
        file_system,
        LocalizationPaths {
            shipped_dir: shipped,
            user_languages_dir: root.join("Languages"),
        },
        UiCulture::from_name("en"),
        Arc::new(KnownCultures),
        log,
    )
}

type Posted = Arc<Mutex<Vec<ContextUiAction>>>;

struct Poster(Posted);

impl SettingsUiPost for Poster {
    fn post(&self, action: ContextUiAction) {
        self.0.lock().unwrap().push(action);
    }
}

/// A recording host: every write is kept (raw, no clamping); controls are enabled and visible until a page says otherwise.
struct Host {
    presenter: SharedSettingsPresenter,
    texts: RefCell<BTreeMap<i32, String>>,
    enabled: RefCell<BTreeMap<i32, bool>>,
    visible: RefCell<BTreeMap<i32, bool>>,
    combos: RefCell<BTreeMap<i32, i32>>,
    combo_item_calls: RefCell<Vec<(i32, Vec<String>, i32)>>,
    relayouts: RefCell<usize>,
    warnings: RefCell<Vec<(String, String)>>,
    explorer: RefCell<Vec<String>>,
    urls: RefCell<Vec<String>>,
    posted: Posted,
    actions_text: RefCell<String>,
    profile_calls: RefCell<Vec<Vec<String>>>,
    profile_answer: RefCell<Option<Vec<ReviewAction>>>,
    committed: RefCell<Vec<(String, String)>>,
}

impl Host {
    fn new(presenter: SharedSettingsPresenter) -> Rc<Self> {
        Rc::new(Self {
            presenter,
            texts: RefCell::default(),
            enabled: RefCell::default(),
            visible: RefCell::default(),
            combos: RefCell::default(),
            combo_item_calls: RefCell::default(),
            relayouts: RefCell::new(0),
            warnings: RefCell::default(),
            explorer: RefCell::default(),
            urls: RefCell::default(),
            posted: Arc::default(),
            actions_text: RefCell::default(),
            profile_calls: RefCell::default(),
            profile_answer: RefCell::default(),
            committed: RefCell::default(),
        })
    }
    fn context(self: &Rc<Self>) -> SettingsPageContext {
        SettingsPageContext::new(self.clone())
    }
    fn text(&self, id: i32) -> String {
        self.texts.borrow().get(&id).cloned().unwrap_or_default()
    }
    fn is_enabled(&self, id: i32) -> bool {
        *self.enabled.borrow().get(&id).unwrap_or(&true)
    }
    fn is_visible(&self, id: i32) -> bool {
        *self.visible.borrow().get(&id).unwrap_or(&true)
    }
}

impl SettingsPageHost for Host {
    fn presenter(&self) -> SharedSettingsPresenter {
        Rc::clone(&self.presenter)
    }
    fn hwnd(&self) -> Hwnd {
        0x1234
    }
    fn files(&self) -> Option<Rc<dyn ShellFileDialog>> {
        None
    }
    fn decoder_factory(&self) -> Option<Arc<dyn ImageDecoderFactory>> {
        None
    }
    fn version(&self) -> Option<String> {
        Some("2.0.93".to_owned())
    }
    fn open_url(&self, url: &str) -> Result<(), String> {
        self.urls.borrow_mut().push(url.to_owned());
        Ok(())
    }
    fn start_explorer(&self, arguments: &str) -> Option<String> {
        self.explorer.borrow_mut().push(arguments.to_owned());
        None
    }
    fn show_action_profiles(
        &self,
        _: Hwnd,
        actions: &[ReviewAction],
    ) -> Result<Option<Vec<ReviewAction>>, ActionProfilesUnavailable> {
        self.profile_calls
            .borrow_mut()
            .push(actions.iter().map(|a| a.name.clone()).collect());
        match self.profile_answer.borrow().as_ref() {
            Some(edited) => Ok(Some(edited.clone())),
            None => Err(ActionProfilesUnavailable),
        }
    }
    fn control_text(&self, id: i32) -> String {
        self.text(id)
    }
    fn set_control_text(&self, id: i32, text: &str) {
        self.texts.borrow_mut().insert(id, text.to_owned());
    }
    fn is_control_checked(&self, _: i32) -> bool {
        false
    }
    fn set_control_checked(&self, _: i32, _: bool) {}
    fn combo_selected_index(&self, id: i32) -> i32 {
        *self.combos.borrow().get(&id).unwrap_or(&-1)
    }
    fn set_combo_selected_index(&self, id: i32, index: i32) {
        self.combos.borrow_mut().insert(id, index);
    }
    fn set_combo_items(&self, id: i32, items: &[String], selected: i32) {
        self.combo_item_calls.borrow_mut().push((id, items.to_vec(), selected));
        self.combos.borrow_mut().insert(id, selected);
    }
    fn slider_value(&self, _: i32) -> i32 {
        0
    }
    fn set_slider_value(&self, _: i32, _: i32) {}
    fn set_slider_range(&self, _: i32, _: i32, _: i32) {}
    fn set_control_tooltip(&self, _: i32, _: TextFn) {}
    fn current_shortcut_texts(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
    fn set_control_enabled(&self, id: i32, enabled: bool) {
        self.enabled.borrow_mut().insert(id, enabled);
    }
    fn is_control_enabled(&self, id: i32) -> bool {
        self.is_enabled(id)
    }
    fn set_control_visible(&self, id: i32, visible: bool) {
        self.visible.borrow_mut().insert(id, visible);
    }
    fn focus_control(&self, _: i32) {}
    fn relayout_active_page(&self) {
        *self.relayouts.borrow_mut() += 1;
    }
    fn warn(&self, title: &str, message: &str) {
        self.warnings.borrow_mut().push((title.to_owned(), message.to_owned()));
    }
    fn ui_post(&self) -> Arc<dyn SettingsUiPost> {
        Arc::new(Poster(Arc::clone(&self.posted)))
    }
    fn actions_json(&self) -> String {
        self.actions_text.borrow().clone()
    }
    fn refresh_actions_json_from_pages(&self) -> String {
        self.actions_text.borrow().clone()
    }
    fn set_actions_json(&self, json: &str, source_key: &str) {
        self.committed
            .borrow_mut()
            .push((json.to_owned(), source_key.to_owned()));
        *self.actions_text.borrow_mut() = json.to_owned();
    }
}

struct Setup {
    shell: Arc<FakeShell>,
    host: Rc<Host>,
    page: GeneralSettingsPage,
    /// Jobs the runner was handed and did not run.
    jobs: Rc<RefCell<Vec<Box<dyn FnOnce() + Send>>>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Runner {
    /// Keeps the job without running it and answers "started".
    Park,
    /// Runs the job at once and answers "started".
    Inline,
    /// Drops the job and answers "could not start".
    Refuse,
}

fn setup(
    shell_state: ShellMenuState,
    ui_language: &str,
    root: Option<&TempRoot>,
    checker: Option<UpdateCheckResult>,
    runner: Runner,
) -> Setup {
    let shell = FakeShell::new(shell_state);
    let mut settings = AppSettings::new(&EnglishSettingsTexts);
    settings.ui_language = ui_language.to_owned();
    let mut options = SettingsPresenterOptions::new(16 << 30, Arc::new(EnglishSettingsTexts), shell.clone());
    options.executable_path = Some(EXE.to_owned());
    options.localization = root.map(|r| Arc::new(Mutex::new(service_in(r))));
    options.update_checker = checker.map(|c| Arc::new(FakeChecker(c)) as Arc<dyn UpdateCheck>);
    let presenter: SharedSettingsPresenter = Rc::new(RefCell::new(SettingsPresenter::new(&settings, options)));
    let host = Host::new(presenter);
    let jobs: Rc<RefCell<Vec<Box<dyn FnOnce() + Send>>>> = Rc::default();
    let parked = Rc::clone(&jobs);
    let background: BackgroundRunner = Rc::new(move |job| match runner {
        Runner::Park => {
            parked.borrow_mut().push(job);
            true
        }
        Runner::Inline => {
            job();
            true
        }
        Runner::Refuse => false,
    });
    let mut page = GeneralSettingsPage::with_background_runner(background);
    let mut builder = SettingsPageBuilder::new(1000);
    page.declare(&mut builder);
    page.attach(host.context());
    Setup {
        shell,
        host,
        page,
        jobs,
    }
}

fn english(key: &str) -> String {
    Localizer::current().get(key)
}

// ------------------------------------------------------------------------------------------------------------------
// Killers
// ------------------------------------------------------------------------------------------------------------------

#[test]
fn debug_output_names_the_page_and_its_ids() {
    // general.rs:74 replace Debug::fmt with Ok(Default::default()) (writes nothing).
    let s = setup(ShellMenuState::NotRegistered, "auto", None, None, Runner::Park);
    let text = format!("{:?}", s.page);
    assert!(text.contains("GeneralSettingsPage"), "{text}");
    assert!(text.contains("ids"), "{text}");
}

#[test]
fn attach_fills_the_language_combo_and_exposes_the_languages() {
    // general.rs:138 replace languages() with an empty slice; :334 delete `-` in unwrap_or(-1).
    let root = TempRoot::new("cl-mut-settings3-languages");
    let s = setup(ShellMenuState::NotRegistered, "auto", Some(&root), None, Runner::Park);
    assert!(!s.page.languages().is_empty());
    assert_eq!(s.page.languages()[0].code, "auto");
    let ids = s.page.ids();
    let calls = s.host.combo_item_calls.borrow();
    let (id, items, selected) = calls.last().expect("attach loads the languages");
    assert_eq!(*id, ids.language);
    assert_eq!(items.len(), s.page.languages().len());
    assert_eq!(*selected, 0, "the saved language `auto` is the first entry");
}

#[test]
fn an_unknown_saved_language_selects_nothing() {
    // general.rs:334 delete `-`: unwrap_or(-1) -> unwrap_or(1) would select the second entry (or any index).
    let root = TempRoot::new("cl-mut-settings3-unknown-language");
    let s = setup(ShellMenuState::NotRegistered, "zz", Some(&root), None, Runner::Park);
    let calls = s.host.combo_item_calls.borrow();
    assert_eq!(calls.last().map(|c| c.2), Some(-1));
}

#[test]
fn reload_keeps_the_selected_language_and_warns_with_the_result() {
    // general.rs:365 reload_translations -> (); :338 selected_language -> None (settings say `zz`, the combo says `auto`).
    let root = TempRoot::new("cl-mut-settings3-reload");
    let mut s = setup(ShellMenuState::NotRegistered, "zz", Some(&root), None, Runner::Park);
    let ids = s.page.ids();
    s.host.set_combo_selected_index(ids.language, 0);
    let before = s.host.combo_item_calls.borrow().len();
    assert!(s.page.on_command(ids.reload, 0));
    let calls = s.host.combo_item_calls.borrow();
    assert_eq!(calls.len(), before + 1, "the languages are loaded again");
    assert_eq!(
        calls.last().map(|c| c.2),
        Some(0),
        "the combo keeps `auto`, not the saved `zz`"
    );
    let warnings = s.host.warnings.borrow();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].0, pr_l10n::tr::dialog_reload_translations_title());
}

#[test]
fn export_asks_explorer_to_select_the_exported_file() {
    // general.rs:375 export_translation -> (); :376 delete `!` in `!has_localization()`.
    let root = TempRoot::new("cl-mut-settings3-export");
    let mut s = setup(ShellMenuState::NotRegistered, "auto", Some(&root), None, Runner::Park);
    assert!(s.page.on_command(s.page.ids().export, 0));
    let explorer = s.host.explorer.borrow();
    assert_eq!(explorer.len(), 1, "{explorer:?} {:?}", s.host.warnings.borrow());
    assert!(explorer[0].starts_with("/select,\""), "{}", explorer[0]);
}

#[test]
fn export_without_a_localization_service_does_nothing() {
    // general.rs:376 (the guard is the contract: no service, no Explorer call, no warning).
    let mut s = setup(ShellMenuState::NotRegistered, "auto", None, None, Runner::Park);
    assert!(s.page.on_command(s.page.ids().export, 0));
    assert!(s.host.explorer.borrow().is_empty());
    assert!(s.host.warnings.borrow().is_empty());
}

#[test]
fn shell_add_and_remove_route_to_the_matching_registration() {
    // general.rs:297 `||` -> `&&`; :298 `==` -> `!=`; :350 show_shell_status -> (); :361 refresh_shell_status -> ().
    let mut s = setup(ShellMenuState::NotRegistered, "auto", None, None, Runner::Park);
    let ids = s.page.ids();
    let not_registered = english("settings.shell.status.notRegistered");
    assert_eq!(s.host.text(ids.shell_status), not_registered, "attach shows the state");
    assert!(s.host.is_enabled(ids.shell_add));
    assert!(!s.host.is_enabled(ids.shell_remove));

    assert!(s.page.on_command(ids.shell_add, 0));
    assert_eq!(s.shell.calls(), vec![format!("register:{EXE}")]);
    assert_eq!(
        s.host.text(ids.shell_status),
        english("settings.shell.status.registered")
    );
    assert!(!s.host.is_enabled(ids.shell_add));
    assert!(s.host.is_enabled(ids.shell_remove));

    assert!(s.page.on_command(ids.shell_remove, 0));
    assert_eq!(
        s.shell.calls(),
        vec![format!("register:{EXE}"), "unregister".to_owned()]
    );
    assert_eq!(s.host.text(ids.shell_status), not_registered);
    assert!(s.host.is_enabled(ids.shell_add));
    assert!(!s.host.is_enabled(ids.shell_remove));
}

#[test]
fn elsewhere_state_relabels_the_add_button() {
    // general.rs:350 show_shell_status -> (): the add button label comes from the status.
    let s = setup(ShellMenuState::RegisteredElsewhere, "auto", None, None, Runner::Park);
    assert_eq!(s.host.text(s.page.ids().shell_add), english("settings.shell.update"));
}

#[test]
fn language_change_redraws_the_stored_update_text_and_the_shell_status() {
    // general.rs:308 on_language_changed -> ().
    let mut s = setup(ShellMenuState::NotRegistered, "auto", None, None, Runner::Refuse);
    let ids = s.page.ids();
    assert!(s.page.on_command(ids.check_update, 0));
    let failed = pr_l10n::tr::update_status_failed_bad_response();
    assert_eq!(s.host.text(ids.update_status), failed);
    s.host.set_control_text(ids.update_status, "");
    s.shell.set(ShellMenuState::Registered);
    s.page.on_language_changed();
    assert_eq!(
        s.host.text(ids.update_status),
        failed,
        "the stored status text is written back"
    );
    assert_eq!(
        s.host.text(ids.shell_status),
        english("settings.shell.status.registered")
    );
}

#[test]
fn update_check_disables_the_button_and_hands_a_job_to_the_runner() {
    // general.rs:392 start_update_check -> (); :404 delete `!` in `if !started`; :344 set_update_status -> ().
    let mut s = setup(ShellMenuState::NotRegistered, "auto", None, None, Runner::Park);
    let ids = s.page.ids();
    s.host.set_control_visible(ids.open_update_page, true);
    let relayouts = *s.host.relayouts.borrow();
    assert!(s.page.on_command(ids.check_update, 0));
    assert_eq!(s.jobs.borrow().len(), 1, "exactly one job reached the runner");
    assert!(!s.host.is_enabled(ids.check_update));
    assert!(!s.host.is_visible(ids.open_update_page));
    assert_eq!(s.host.text(ids.update_status), pr_l10n::tr::update_check_checking());
    assert!(*s.host.relayouts.borrow() > relayouts);
}

#[test]
fn update_check_that_cannot_start_reports_failure_and_re_enables_the_button() {
    // general.rs:404 the failure branch runs only when the runner refuses.
    let mut s = setup(ShellMenuState::NotRegistered, "auto", None, None, Runner::Refuse);
    let ids = s.page.ids();
    assert!(s.page.on_command(ids.check_update, 0));
    assert_eq!(
        s.host.text(ids.update_status),
        pr_l10n::tr::update_status_failed_bad_response()
    );
    assert!(s.host.is_enabled(ids.check_update));
}

#[test]
fn a_finished_update_check_posts_its_result_back_and_offers_the_release_page() {
    // general.rs:417 apply_update_result -> ().
    let result = UpdateCheckResult::update_available("2.0.94", RELEASE_URL);
    let mut s = setup(
        ShellMenuState::NotRegistered,
        "auto",
        None,
        Some(result),
        Runner::Inline,
    );
    let ids = s.page.ids();
    assert!(s.page.on_command(ids.check_update, 0));
    let actions: Vec<ContextUiAction> = std::mem::take(&mut *s.host.posted.lock().unwrap());
    assert_eq!(actions.len(), 1, "the worker posts one action");
    assert!(
        !s.host.is_enabled(ids.check_update),
        "still busy until the answer is applied"
    );
    let ctx = s.host.context();
    for action in actions {
        action(&ctx);
    }
    let status = s.host.text(ids.update_status);
    assert!(status.contains("2.0.94") && status.contains("2.0.93"), "{status}");
    assert!(s.host.is_visible(ids.open_update_page));
    assert!(s.host.is_enabled(ids.check_update));

    assert!(s.page.on_command(ids.open_update_page, 0));
    assert_eq!(*s.host.urls.borrow(), vec![RELEASE_URL.to_owned()]);
}

// ------------------------------------------------------------------------------------------------------------------
// shortcuts.rs
// ------------------------------------------------------------------------------------------------------------------

struct Keys {
    host: Rc<Host>,
    page: ShortcutsSettingsPage,
}

fn keys() -> Keys {
    let settings = AppSettings::new(&EnglishSettingsTexts);
    let mut options = SettingsPresenterOptions::new(
        16 << 30,
        Arc::new(EnglishSettingsTexts),
        FakeShell::new(ShellMenuState::NotRegistered),
    );
    options.executable_path = Some(EXE.to_owned());
    let presenter: SharedSettingsPresenter = Rc::new(RefCell::new(SettingsPresenter::new(&settings, options)));
    let host = Host::new(presenter);
    let mut page = ShortcutsSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(5000);
    page.declare(&mut builder);
    page.attach(host.context());
    Keys { host, page }
}

#[test]
fn shortcuts_page_key_and_cells_are_looked_up_by_property() {
    // shortcuts.rs:309 key -> ""; :296 `==` -> `!=` in cell(); :294 cell() -> None / default.
    let k = keys();
    assert_eq!(k.page.key(), "Shortcuts");
    let mut boxes = std::collections::BTreeSet::new();
    for row in all_rows() {
        let cell = k
            .page
            .cell(row.property)
            .unwrap_or_else(|| panic!("no cell for {}", row.property));
        assert!(boxes.insert(cell.box_id), "box id of {} is shared", row.property);
        assert_eq!(
            cell.clear != 0,
            row.clear_tip.is_some(),
            "clear button of {}",
            row.property
        );
    }
    assert_eq!(boxes.len(), 29);
    assert!(k.page.cell("NoSuchProperty").is_none());
    let next = k.page.cell("Next").unwrap();
    let previous = k.page.cell("Previous").unwrap();
    assert_ne!(next.box_id, previous.box_id);
    assert!(next.box_id < previous.box_id, "declaration order is the row order");
}

#[test]
fn shortcuts_load_and_collect_round_trip_every_property() {
    // shortcuts.rs load / collect: the 29 texts travel between the form state and the key boxes.
    let mut k = keys();
    let mut state = pr_app::presenters::settings_form_state::SettingsFormState::default();
    for (i, property) in shortcut_properties().iter().enumerate() {
        state
            .shortcut_texts
            .insert((*property).to_owned(), format!("F{}", i + 1));
    }
    k.page.load(&state);
    for property in shortcut_properties() {
        let cell = k.page.cell(property).unwrap();
        assert_eq!(k.host.text(cell.box_id), state.shortcut_texts[*property], "{property}");
    }
    let mut collected = pr_app::presenters::settings_form_state::SettingsFormState::default();
    k.page.collect(&mut collected);
    assert_eq!(collected.shortcut_texts, state.shortcut_texts);
}

#[test]
fn shortcuts_typing_a_duplicate_key_warns_and_marks_both_labels() {
    // shortcuts.rs refresh / update_markers / on_command(EN_CHANGE): warning line, marker prefix, and removal again.
    let mut k = keys();
    let ids = k.page.ids();
    let next = k.page.cell("Next").unwrap();
    let previous = k.page.cell("Previous").unwrap();
    let defaults = ShortcutMappings::default();
    for property in shortcut_properties() {
        let cell = k.page.cell(property).unwrap();
        k.host.set_control_text(cell.box_id, shortcut_get(&defaults, property));
    }
    assert!(k.page.on_command(next.box_id, EN_CHANGE));
    assert_eq!(k.host.text(ids.warning), "", "the defaults have no duplicate");
    assert!(k.page.marked_properties().is_empty());

    k.host
        .set_control_text(previous.box_id, shortcut_get(&defaults, "Next"));
    assert!(k.page.on_command(previous.box_id, EN_CHANGE));
    assert!(!k.host.text(ids.warning).is_empty());
    assert_eq!(
        k.page.marked_properties(),
        vec!["Next".to_owned(), "Previous".to_owned()]
    );
    assert_eq!(
        k.host.text(next.label),
        format!("{MARKER}{}", pr_l10n::tr::settings_shortcut_next())
    );
    assert_eq!(
        k.host.text(previous.label),
        format!("{MARKER}{}", pr_l10n::tr::settings_shortcut_previous())
    );

    k.host
        .set_control_text(previous.box_id, shortcut_get(&defaults, "Previous"));
    assert!(k.page.on_command(previous.box_id, EN_CHANGE));
    assert_eq!(k.host.text(ids.warning), "");
    assert!(k.page.marked_properties().is_empty());
    assert_eq!(k.host.text(next.label), pr_l10n::tr::settings_shortcut_next());
    assert_eq!(k.host.text(previous.label), pr_l10n::tr::settings_shortcut_previous());
}

#[test]
fn shortcuts_commands_outside_the_page_are_not_handled() {
    // shortcuts.rs on_command: unknown ids / notifications fall through to false.
    let mut k = keys();
    let next = k.page.cell("Next").unwrap();
    assert!(!k.page.on_command(next.box_id, 0x0100), "EN_SETFOCUS is not a change");
    assert!(!k.page.on_command(987_654, EN_CHANGE));
    assert!(!k.page.on_command(987_654, BN_CLICKED));
    assert!(
        !k.page.on_command(k.page.ids().reset_keys, EN_CHANGE),
        "a button does not answer EN_CHANGE"
    );
}

#[test]
fn shortcuts_clear_button_empties_its_box_and_reset_restores_the_defaults() {
    // shortcuts.rs on_command clear / reset_keys.
    let mut k = keys();
    let last = k.page.cell("LastImage").unwrap();
    assert_ne!(last.clear, 0);
    k.host.set_control_text(last.box_id, "End");
    assert!(k.page.on_command(last.clear, BN_CLICKED));
    assert_eq!(k.host.text(last.box_id), "");
    let next = k.page.cell("Next").unwrap();
    k.host.set_control_text(next.box_id, "Q");
    assert!(k.page.on_command(k.page.ids().reset_keys, BN_CLICKED));
    let defaults = ShortcutMappings::default();
    for property in shortcut_properties() {
        let cell = k.page.cell(property).unwrap();
        assert_eq!(
            k.host.text(cell.box_id),
            shortcut_get(&defaults, property),
            "{property}"
        );
    }
}

#[test]
fn shortcuts_edit_actions_that_cannot_open_the_editor_warns_once() {
    // shortcuts.rs:edit_actions: no editor wired -> one warning, nothing committed.
    let mut k = keys();
    *k.host.actions_text.borrow_mut() = "null".to_owned();
    assert!(k.page.on_command(k.page.ids().edit_actions, BN_CLICKED));
    assert_eq!(k.host.profile_calls.borrow().len(), 1);
    let warnings = k.host.warnings.borrow();
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].0,
        Localizer::current().get(pr_l10n::keys::DIALOG_OPEN_EDITOR_FAILED_TITLE)
    );
    assert!(k.host.committed.borrow().is_empty());
}

#[test]
fn shortcuts_edit_actions_commits_the_edited_list_under_the_page_key() {
    // shortcuts.rs:edit_actions: null JSON opens the editor with the defaults; the edit is committed as text for the "Shortcuts" source.
    let mut k = keys();
    *k.host.actions_text.borrow_mut() = "null".to_owned();
    let mut edited = ReviewAction::defaults(&|n| format!("Edited {n}"), "Backup");
    edited[0].name = "Renamed".to_owned();
    *k.host.profile_answer.borrow_mut() = Some(edited.clone());
    assert!(k.page.on_command(k.page.ids().edit_actions, BN_CLICKED));
    let shown = k.host.profile_calls.borrow();
    assert_eq!(shown.len(), 1);
    let defaults = ReviewAction::defaults(
        &|n| {
            Localizer::current().format_with(
                pr_l10n::keys::ACTION_DEFAULT_NAME,
                &[pr_l10n::template::LocArg::new("n", n)],
                &pr_l10n::localizer::number_culture(),
            )
        },
        &Localizer::current().get(pr_l10n::keys::ACTION_DEFAULT_BACKUP),
    );
    assert_eq!(shown[0], defaults.iter().map(|a| a.name.clone()).collect::<Vec<_>>());
    assert!(shown[0].iter().all(|name| !name.is_empty() && name != "xyzzy"));
    assert!(k.host.warnings.borrow().is_empty());
    let committed = k.host.committed.borrow();
    assert_eq!(
        *committed,
        vec![(SettingsPresenter::actions_to_text(&edited), "Shortcuts".to_owned())]
    );
}

// ------------------------------------------------------------------------------------------------------------------
// dialog/nav.rs (pure parts)
// ------------------------------------------------------------------------------------------------------------------

#[test]
fn nav_page_of_maps_id_ranges_to_page_indices() {
    // nav.rs:16 `>=` boundary, `/`, `-`: ids below 1000 belong to no page.
    for (id, page) in [
        (i32::MIN, -1),
        (-1, -1),
        (0, -1),
        (999, -1),
        (1000, 0),
        (1001, 0),
        (1999, 0),
        (2000, 1),
        (5432, 4),
        (7999, 6),
        (8000, 7),
        (9500, 8),
    ] {
        assert_eq!(page_of(id), page, "id {id}");
    }
}

#[test]
fn nav_virtual_key_constants_match_win32() {
    // nav.rs:7..12 (C# VkTab / VkControl / VkShift).
    assert_eq!((VK_TAB, VK_CONTROL, VK_SHIFT), (0x09, 0x11, 0x10));
}

#[test]
fn nav_classify_dialog_key_covers_every_input_combination() {
    // nav.rs classify_dialog_key: `==` / `&&` / `||` / `!` operators and the three results.
    const KEYDOWN: u32 = 0x0100;
    const SYSKEYDOWN: u32 = 0x0104;
    const CHAR: u32 = 0x0102;
    let tab = VK_TAB as usize;
    for shift in [false, true] {
        for key_box in [false, true] {
            assert_eq!(
                classify_dialog_key(KEYDOWN, tab, true, shift, key_box),
                DialogKeyAction::CyclePage { forward: !shift },
                "ctrl+tab shift={shift} key_box={key_box}"
            );
            assert_eq!(
                classify_dialog_key(KEYDOWN, tab, false, shift, key_box),
                DialogKeyAction::NotHandled,
                "plain tab is never captured (shift={shift} key_box={key_box})"
            );
            for control in [false, true] {
                assert_eq!(
                    classify_dialog_key(SYSKEYDOWN, tab, control, shift, key_box),
                    if key_box {
                        DialogKeyAction::CaptureKey
                    } else {
                        DialogKeyAction::NotHandled
                    },
                    "alt+tab is no page cycle (control={control} shift={shift} key_box={key_box})"
                );
            }
        }
    }
    for control in [false, true] {
        for shift in [false, true] {
            assert_eq!(
                classify_dialog_key(KEYDOWN, 0x41, control, shift, true),
                DialogKeyAction::CaptureKey
            );
            assert_eq!(
                classify_dialog_key(SYSKEYDOWN, 0x41, control, shift, true),
                DialogKeyAction::CaptureKey
            );
            assert_eq!(
                classify_dialog_key(KEYDOWN, 0x41, control, shift, false),
                DialogKeyAction::NotHandled
            );
            assert_eq!(
                classify_dialog_key(CHAR, 0x41, control, shift, true),
                DialogKeyAction::NotHandled
            );
            assert_eq!(
                classify_dialog_key(CHAR, tab, control, shift, true),
                DialogKeyAction::NotHandled
            );
            assert_eq!(
                classify_dialog_key(0, 0x41, control, shift, false),
                DialogKeyAction::NotHandled
            );
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------
// dialog/nav.rs (native: a real Settings window on a desktop; run with --include-ignored, registered in native-nightly.yml)
// ------------------------------------------------------------------------------------------------------------------

use pr_platform_win::dialogs::native::controls::basic::ListBoxControl;
use pr_platform_win::dialogs::settings::dialog::{
    SettingsDialog, SettingsDialogArgs, frame::NAV_ID, keybox::key_box_text,
};
use pr_platform_win::win32::structs::Msg;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};

/// The Shortcuts page (index 4) and the control id of its first key box.
const SHORTCUTS_PAGE: i32 = 4;
const FIRST_KEY_BOX: i32 = 5006;
const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_CHAR: u32 = 0x0102;
const WM_SYSKEYDOWN: u32 = 0x0104;
const VK_TAB_USIZE: usize = 0x09;

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn native_modal(
    prepare: impl FnOnce(&mut SettingsDialogArgs) + Send + 'static,
    body: impl Fn(&SettingsDialog) + Send + 'static,
) {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    desktop::on_sta(move || {
        let _styles = desktop::activate_visual_styles();
        let mut args = fixture::args();
        prepare(&mut args);
        let dialog = SettingsDialog::new(args).unwrap();
        let shown = dialog.show_modal(
            0,
            Some(&|d| {
                body(d);
                d.close();
            }),
            None,
        );
        assert_eq!(shown.unwrap(), 2);
    });
}

fn key_msg(message: u32, hwnd: isize, vk: usize) -> Msg {
    Msg {
        hwnd: HWND(hwnd as *mut _),
        message,
        wParam: WPARAM(vk),
        lParam: LPARAM(0),
        ..Default::default()
    }
}

fn set_keyboard(control: bool, shift: bool) {
    let mut state = [0u8; 256];
    if control {
        state[0x11] = 0x80;
    }
    if shift {
        state[0x10] = 0x80;
    }
    // SAFETY: SetKeyboardState reads 256 bytes of this thread's own key table; it changes no global state.
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::SetKeyboardState(&state) }.expect("SetKeyboardState");
}

#[test]
#[ignore = "native: needs desktop"]
fn nav_activating_a_page_selects_its_row_in_the_navigation_list() {
    // nav.rs:89 replace `!=` with `==` (the row is only selected when it already is).
    native_modal(
        |_| {},
        |d| {
            let nav = d.native().item(NAV_ID);
            for index in [3, 1, 6, 0, 5] {
                d.activate_page(index, false, true);
                assert_eq!(
                    ListBoxControl::selected_index(nav),
                    index,
                    "row after activating {index}"
                );
                assert_eq!(d.active_page_index(), index);
            }
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn nav_list_selection_notification_opens_the_selected_page() {
    // nav.rs:166 replace on_nav_selection_changed with (): LBN_SELCHANGE on the list activates the selected page.
    native_modal(
        |_| {},
        |d| {
            let nav = d.native().item(NAV_ID);
            ListBoxControl::select_single(nav, 2);
            assert_eq!(d.active_page_index(), 0, "select_single sends no notification");
            desktop::command(d.native(), NAV_ID, 1);
            assert_eq!(d.active_page_index(), 2);
            assert_eq!(d.active_page_key(), d.page_key(2).unwrap());
            pr_app::presenters::settings::SettingsPresenter::remember_page("General");
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn nav_key_messages_reach_the_key_box_without_asking_for_the_control_key() {
    // nav.rs:130 `!=` -> `==` (SYSKEYDOWN dropped); :136 `&&` -> `||` (is_tab); :153 delete `!` (key box asked only for Tab).
    let asked = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let counter = Arc::clone(&asked);
    let seen = Arc::clone(&asked);
    native_modal(
        move |args| {
            args.options.control_key_down = Some(Rc::new(move || {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                false
            }))
        },
        move |d| {
            d.activate_page(SHORTCUTS_PAGE, false, true);
            let boxed = d.native().item(FIRST_KEY_BOX);
            assert_ne!(boxed, 0);
            let before = d.control_text(FIRST_KEY_BOX);
            for (message, vk) in [(WM_KEYDOWN, 0x42usize), (WM_SYSKEYDOWN, 0x43)] {
                assert!(
                    d.handle_key_for_test(&key_msg(message, boxed, vk)),
                    "message {message:#x}"
                );
                assert_eq!(d.control_text(FIRST_KEY_BOX), key_box_text(vk as i32, false).unwrap());
            }
            let captured = d.control_text(FIRST_KEY_BOX);
            assert_ne!(captured, before);
            for message in [WM_KEYUP, WM_CHAR, 0x0200] {
                assert!(
                    !d.handle_key_for_test(&key_msg(message, boxed, 0x44)),
                    "message {message:#x}"
                );
                assert_eq!(d.control_text(FIRST_KEY_BOX), captured);
            }
            assert!(
                !d.handle_key_for_test(&key_msg(WM_KEYDOWN, 0, 0x41)),
                "no target window"
            );
            assert_eq!(
                seen.load(std::sync::atomic::Ordering::SeqCst),
                0,
                "the Control check is only made for Tab"
            );
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn nav_a_foreign_window_with_a_key_box_id_is_not_a_key_box() {
    // nav.rs:153 `&&` -> `||`: IsKeyBox also requires dialog.item(id) == hwnd; CaptureKey alone would accept the id.
    native_modal(
        |_| {},
        |d| {
            d.activate_page(SHORTCUTS_PAGE, false, true);
            let before = d.control_text(FIRST_KEY_BOX);
            let owner = desktop::create_owner();
            let instance = pr_platform_win::win32::api::kernel32::get_module_handle().unwrap_or(0);
            // SAFETY: literal class/title, integer arguments only; the child is destroyed with its owner below.
            let foreign = unsafe {
                pr_platform_win::win32::api::user32::create_window_ex(
                    0,
                    "STATIC",
                    "foreign",
                    0x4000_0000,
                    0,
                    0,
                    10,
                    10,
                    owner,
                    FIRST_KEY_BOX as isize,
                    instance,
                    0,
                )
            }
            .expect("foreign child");
            assert_eq!(
                pr_platform_win::win32::dialog_native::user32::get_dlg_ctrl_id(foreign),
                FIRST_KEY_BOX
            );
            assert!(!d.is_key_box(foreign));
            assert!(!d.handle_key_for_test(&key_msg(WM_KEYDOWN, foreign, 0x41)));
            assert_eq!(d.control_text(FIRST_KEY_BOX), before);
            desktop::destroy_owner(owner);
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn nav_ctrl_tab_reads_the_real_control_and_shift_key_state() {
    // nav.rs:175 key_down -> false; `<` -> `==`; `<` -> `<=` (an unpressed key reports 0, a pressed one is negative).
    native_modal(
        |_| {},
        |d| {
            d.activate_page(0, false, true);
            let tab = key_msg(WM_KEYDOWN, 0, VK_TAB_USIZE);
            set_keyboard(false, false);
            assert!(!d.handle_key_for_test(&tab), "plain Tab stays with the dialog manager");
            assert_eq!(d.active_page_index(), 0);
            set_keyboard(true, false);
            assert!(d.handle_key_for_test(&tab));
            assert_eq!(d.active_page_index(), 1, "Ctrl+Tab goes forward");
            set_keyboard(true, true);
            assert!(d.handle_key_for_test(&tab));
            assert_eq!(d.active_page_index(), 0, "Ctrl+Shift+Tab goes back");
            assert!(d.handle_key_for_test(&tab));
            assert_eq!(d.active_page_index(), d.page_count() as i32 - 1, "and wraps");
            set_keyboard(false, false);
            pr_app::presenters::settings::SettingsPresenter::remember_page("General");
        },
    );
}
