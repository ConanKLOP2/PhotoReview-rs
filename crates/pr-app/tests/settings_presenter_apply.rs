//! W8-10e tests of `presenters::settings::apply`: import / export, actions hand-off, context-menu rows, language picker, Explorer
//! integration, log location and the manual update check. Kind F: fakes and hand-written expectations from the C# tests
//! (baseline d92d6aae). Each test names its C# twin in a `// C#:` comment; tests without a twin say `// New test`.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use pr_app::localization::service::{CultureLookup, LocalizationPaths, LocalizationService};
use pr_app::misc::app_log::{AppLog, AppLogTarget};
use pr_app::presenters::settings::apply::{SettingsExplorerRequest, UpdateCheckOutcome};
use pr_app::presenters::settings::{SettingsPresenter, SettingsPresenterOptions};
use pr_app::presenters::settings_field_map::FieldContext;
use pr_core::abstractions::{FileSystem, Log, NullLog, ShellIntegration, ShellMenuState};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts, SettingsTexts};
use pr_core::settings::context_menu::{ITEMS, item_info};
use pr_core::settings::shortcuts::ReviewAction;
use pr_core::sync::{CancelToken, Canceled};
use pr_core::updates::checker::UpdateCheck;
use pr_core::updates::version::{UpdateCheckResult, UpdateFailure};
use pr_l10n::loader::UiCulture;
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_l10n::template::{LocArg, NumberCulture};
use pr_test_support::{FakeFileSystem, RecordingLog, TempRoot};

const PHYSICAL_MEMORY: i64 = 32 * 1024 * 1024 * 1024;
const RELEASE_URL: &str = "https://github.com/ConanKLOP2/PhotoReview/releases/tag/v2.0.94";
const HANG_GUARD: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------------------------------------------

struct Env {
    texts: Arc<Localizer>,
    numbers: NumberCulture,
}

impl Env {
    fn new() -> Self {
        Self {
            texts: built_in_english_localizer(),
            numbers: NumberCulture::invariant(),
        }
    }

    fn ctx(&self) -> FieldContext<'_> {
        FieldContext {
            texts: &self.texts,
            numbers: &self.numbers,
        }
    }

    fn text(&self, key: &str) -> String {
        self.texts.get(key)
    }

    fn text_with(&self, key: &str, name: &str, value: &str) -> String {
        self.texts
            .format_with(key, &[LocArg::new(name, value.to_owned())], &self.numbers)
    }
}

/// C# `FakeShell`: records calls; `throws` makes `register` fail (the Rust `get_state` is infallible).
struct FakeShell {
    state: Mutex<ShellMenuState>,
    throws: Mutex<Option<String>>,
    calls: Mutex<Vec<String>>,
}

impl FakeShell {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(ShellMenuState::NotRegistered),
            throws: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
        })
    }

    fn set_state(&self, state: ShellMenuState) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = state;
    }

    fn set_throws(&self, message: &str) {
        *self.throws.lock().unwrap_or_else(PoisonError::into_inner) = Some(message.to_owned());
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl ShellIntegration for FakeShell {
    fn get_state(&self, _executable_path: &str) -> ShellMenuState {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn register(&self, executable_path: &str) -> std::io::Result<()> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!("register {executable_path}"));
        if let Some(message) = self.throws.lock().unwrap_or_else(PoisonError::into_inner).clone() {
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, message));
        }
        self.set_state(ShellMenuState::Registered);
        Ok(())
    }

    fn unregister(&self) -> std::io::Result<()> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push("unregister".to_owned());
        self.set_state(ShellMenuState::NotRegistered);
        Ok(())
    }
}

/// C# `FakeChecker`: the closure plays `CheckAsync`.
struct FakeChecker<F>(F);

impl<F> UpdateCheck for FakeChecker<F>
where
    F: Fn(&CancelToken) -> Result<UpdateCheckResult, Canceled> + Send + Sync,
{
    fn check(&self, _current_version: Option<&str>, cancel: &CancelToken) -> Result<UpdateCheckResult, Canceled> {
        (self.0)(cancel)
    }
}

fn checker_returning(result: UpdateCheckResult) -> Arc<dyn UpdateCheck> {
    Arc::new(FakeChecker(move |_: &CancelToken| Ok(result.clone())))
}

fn options() -> SettingsPresenterOptions {
    SettingsPresenterOptions::new(PHYSICAL_MEMORY, Arc::new(EnglishSettingsTexts), FakeShell::new())
}

fn presenter_from(settings: &AppSettings, options: SettingsPresenterOptions) -> SettingsPresenter {
    SettingsPresenter::new(settings, options)
}

/// C# `NewPresenter(settings, checker)`.
fn new_presenter(settings: &AppSettings, checker: Option<Arc<dyn UpdateCheck>>) -> SettingsPresenter {
    let mut options = options();
    options.update_checker = checker;
    presenter_from(settings, options)
}

fn default_settings() -> AppSettings {
    AppSettings::new(&EnglishSettingsTexts)
}

fn with_click_zoom(percent: i32) -> AppSettings {
    let mut settings = default_settings();
    settings.click_zoom_percent = percent;
    settings
}

/// A rooted executable path that exists nowhere (the shell tests never touch the disk).
fn fake_executable(root: &TempRoot) -> String {
    root.join("PhotoReview.exe").display().to_string()
}

fn shell_presenter(shell: &Arc<FakeShell>, executable: Option<String>) -> SettingsPresenter {
    let mut options = options();
    options.shell_integration = shell.clone();
    options.executable_path = executable;
    presenter_from(&default_settings(), options)
}

struct KnownCultures;

impl CultureLookup for KnownCultures {
    fn lookup(&self, name: &str) -> Option<UiCulture> {
        matches!(name, "en" | "vi").then(|| UiCulture::from_name(name))
    }
}

/// A localization service over a fake disk with a real, empty user folder inside `root`.
fn service_in(root: &TempRoot) -> (LocalizationService, PathBuf) {
    let shipped = PathBuf::from(r"C:\App\Languages");
    let user = root.join("Languages");
    let disk = Arc::new(FakeFileSystem::new());
    disk.add_dir(&shipped);
    let file_system: Arc<dyn FileSystem> = disk;
    let log: Arc<dyn Log> = Arc::new(RecordingLog::new());
    let service = LocalizationService::new(
        file_system,
        LocalizationPaths {
            shipped_dir: shipped,
            user_languages_dir: user.clone(),
        },
        UiCulture::from_name("en"),
        Arc::new(KnownCultures),
        log,
    );
    (service, user)
}

fn counting_refresh(count: &mut i32) -> impl FnMut(&SettingsPresenter) -> Result<(), String> + '_ {
    move |_| {
        *count += 1;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Import / export
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn import_half_valid_file_keeps_valid_part() {
    // C#: SettingsPresenterTests.Import_HalfValidFile_KeepsValidPart
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings(), None);
    let mut refreshed = 0;

    let outcome = presenter.try_apply_imported_json(
        r#"{"ConfigVersion":3,"UiLanguage":"en","ClickZoomPercent":"abc","KeyboardZoomStepPercent":40,"Shortcuts":{"Next":"N"}}"#,
        &mut counting_refresh(&mut refreshed),
        &env.ctx(),
    );

    let notice = outcome.expect("half-valid file is applied");
    assert!(
        notice.as_deref().is_some_and(|text| text.contains("ClickZoomPercent")),
        "{notice:?}"
    );
    assert_eq!(
        presenter.settings().click_zoom_percent,
        default_settings().click_zoom_percent
    );
    assert_eq!(presenter.settings().keyboard_zoom_step_percent, 40);
    assert_eq!(presenter.settings().shortcuts.next, "N");
    assert_eq!(refreshed, 1);
}

#[test]
fn import_unusable_restores_previous() {
    // C#: SettingsPresenterTests.Import_Unusable_RestoresPrevious
    let env = Env::new();
    let mut presenter = new_presenter(&with_click_zoom(250), None);
    let before = presenter.settings().clone();
    let mut refreshed = 0;

    let outcome =
        presenter.try_apply_imported_json("this is not json", &mut counting_refresh(&mut refreshed), &env.ctx());

    let message = outcome.expect_err("unparsable text is refused");
    assert!(!message.trim().is_empty());
    assert_eq!(presenter.settings(), &before);
    assert_eq!(presenter.settings().click_zoom_percent, 250);
    assert_eq!(
        refreshed, 1,
        "only the restore reload: the failure happened before the first one"
    );
}

#[test]
fn import_view_reload_fails_still_restores_previous() {
    // C#: SettingsPresenterTests.Import_ViewReloadThrows_RestoresPrevious
    let env = Env::new();
    let mut presenter = new_presenter(&with_click_zoom(250), None);
    let mut calls = 0;
    let mut refresh = |_: &SettingsPresenter| {
        calls += 1;
        if calls == 1 {
            Err("view broke".to_owned())
        } else {
            Ok(())
        }
    };

    let outcome = presenter.try_apply_imported_json(
        r#"{"ConfigVersion":3,"ClickZoomPercent":400}"#,
        &mut refresh,
        &env.ctx(),
    );

    let message = outcome.expect_err("a failing view reload refuses the import");
    assert!(message.contains("view broke"), "{message}");
    assert_eq!(presenter.settings().click_zoom_percent, 250);
    assert_eq!(calls, 2);
}

#[test]
fn export_then_import_round_trips() {
    // C#: SettingsPresenterTests.ExportThenImport_RoundTrips
    let env = Env::new();
    let root = TempRoot::new("w8-10e-roundtrip");
    let path = root.join("settings.json");
    let mut source_settings = with_click_zoom(333);
    source_settings.keyboard_zoom_step_percent = 45;
    source_settings.logging_enabled = true;
    let source = new_presenter(&source_settings, None);
    assert_eq!(source.export_settings_to(&path, &env.ctx()), None);

    let mut target = new_presenter(&default_settings(), None);
    let result = target.import_settings_from(&path, &mut |_| Ok(()), &env.ctx());

    assert_eq!(result.error, None);
    assert_eq!(target.settings().click_zoom_percent, 333);
    assert_eq!(target.settings().keyboard_zoom_step_percent, 45);
    assert!(target.settings().logging_enabled);
    // UTF-8 without a BOM, like File.WriteAllText.
    let bytes = std::fs::read(&path).expect("exported file exists");
    assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
}

#[test]
fn export_and_import_failures_are_reported() {
    // C#: SettingsPresenterTests.ExportAndImport_FailuresAreReported
    let env = Env::new();
    let root = TempRoot::new("w8-10e-missing");
    let mut presenter = new_presenter(&default_settings(), None);

    let export = presenter.export_settings_to(&root.join("nope").join("settings.json"), &env.ctx());
    let export = export.expect("a missing directory is a failure");
    assert!(export.starts_with("Could not export settings: "), "{export}");

    let result = presenter.import_settings_from(&root.join("missing.json"), &mut |_| Ok(()), &env.ctx());
    let error = result.error.expect("a missing file is a failure");
    assert!(error.starts_with("Could not import settings: "), "{error}");
    assert_eq!(result.repairs_notice, None);
}

#[test]
fn import_null_json_is_refused() {
    // New test
    let env = Env::new();
    let mut presenter = new_presenter(&with_click_zoom(250), None);
    let mut refreshed = 0;

    let outcome = presenter.try_apply_imported_json("null", &mut counting_refresh(&mut refreshed), &env.ctx());

    let message = outcome.expect_err("the literal null is not a settings object");
    assert!(message.contains("does not contain a settings object"), "{message}");
    assert_eq!(presenter.settings().click_zoom_percent, 250);
    assert_eq!(refreshed, 1);
}

#[test]
fn import_io_error_does_not_call_refresh() {
    // New test
    let env = Env::new();
    let root = TempRoot::new("w8-10e-io");
    let mut presenter = new_presenter(&with_click_zoom(250), None);
    let mut refreshed = 0;

    let result = presenter.import_settings_from(
        &root.join("missing.json"),
        &mut counting_refresh(&mut refreshed),
        &env.ctx(),
    );

    assert!(result.error.is_some());
    assert_eq!(refreshed, 0);
    assert_eq!(presenter.settings().click_zoom_percent, 250);
}

#[test]
fn settings_file_dialog_directory_is_the_executable_folder() {
    // New test
    let directory = SettingsPresenter::settings_file_dialog_directory();
    assert!(directory.is_dir(), "{}", directory.display());
}

// ---------------------------------------------------------------------------------------------------------------
// Actions hand-off
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn actions_for_editor_null_gives_defaults() {
    // C#: SettingsPresenterTests.ActionsForEditor_NullGivesDefaults
    let texts = EnglishSettingsTexts;
    let expected = ReviewAction::defaults(&|n| texts.action_default_name(n), &texts.action_default_backup());
    let actions = SettingsPresenter::actions_for_editor("null", &texts).expect("null gives the defaults");
    assert_eq!(actions.len(), expected.len());
    assert!(!actions.is_empty());
    assert!(SettingsPresenter::actions_for_editor("{ nope", &texts).is_err());
}

#[test]
fn actions_to_text_keeps_non_ascii_readable() {
    // New test
    let texts = EnglishSettingsTexts;
    let mut actions = SettingsPresenter::actions_for_editor("null", &texts).expect("defaults");
    actions[0].name = "Giữ lại".to_owned();
    let text = SettingsPresenter::actions_to_text(&actions);
    assert!(text.contains("Giữ lại"), "{text}");
}

// ---------------------------------------------------------------------------------------------------------------
// Context menu rows
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn context_menu_rows_match_the_item_list() {
    // C#: SettingsPresenterTests.ContextMenuRows_MatchTheItemList
    let env = Env::new();
    let rows = SettingsPresenter::context_menu_rows(&env.ctx());
    assert_eq!(rows.len(), ITEMS.len());
    assert_eq!(rows.len(), 18);
    for (row, item) in rows.iter().zip(ITEMS.iter()) {
        let info = item_info(row.id);
        assert_eq!(row.id, item.id, "table order");
        assert_eq!(row.locked, info.locked);
        assert_eq!(row.is_child, info.parent.is_some());
        assert_eq!(row.locked_tooltip.is_some(), info.locked);
        assert!(!row.label.trim().is_empty());
        assert!(row.automation_name.contains(&row.label), "{}", row.automation_name);
    }
    assert_eq!(rows.iter().filter(|row| row.locked).count(), 1);
}

// ---------------------------------------------------------------------------------------------------------------
// Update check
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn update_check_up_to_date() {
    // C#: SettingsPresenterTests.UpdateCheck_UpToDate
    let env = Env::new();
    let mut presenter = new_presenter(
        &default_settings(),
        Some(checker_returning(UpdateCheckResult::up_to_date("2.0.93"))),
    );
    let display = presenter
        .check_for_update_blocking(Some("2.0.93"), &env.ctx())
        .expect("a finished check shows something");
    assert_eq!(
        display.status_text,
        env.text_with("update.status.upToDate", "version", "2.0.93")
    );
    assert!(display.status_text.contains("2.0.93"));
    assert!(!display.show_open_page());
}

#[test]
fn update_check_available_offers_validated_url() {
    // C#: SettingsPresenterTests.UpdateCheck_Available_OffersValidatedUrl
    let env = Env::new();
    let mut presenter = new_presenter(
        &default_settings(),
        Some(checker_returning(UpdateCheckResult::update_available(
            "2.0.94",
            RELEASE_URL,
        ))),
    );
    let display = presenter
        .check_for_update_blocking(Some("2.0.93"), &env.ctx())
        .expect("a finished check shows something");
    assert!(display.show_open_page());
    assert_eq!(display.download_url.as_deref(), Some(RELEASE_URL));
    assert!(
        display.status_text.contains("2.0.94") && display.status_text.contains("2.0.93"),
        "{}",
        display.status_text
    );

    let mut opened: Vec<String> = Vec::new();
    let status = presenter.open_update_page(
        &mut |url| {
            opened.push(url.to_owned());
            Ok(())
        },
        &env.ctx(),
    );
    assert_eq!(status, None);
    assert_eq!(opened, [RELEASE_URL]);
}

#[test]
fn update_check_foreign_url_is_never_opened() {
    // C#: SettingsPresenterTests.UpdateCheck_ForeignUrl_IsNeverOpened
    let env = Env::new();
    let foreign = "https://evil.example/x.exe";
    let mut presenter = new_presenter(
        &default_settings(),
        Some(checker_returning(UpdateCheckResult::update_available(
            "2.0.94", foreign,
        ))),
    );
    let display = presenter
        .check_for_update_blocking(Some("2.0.93"), &env.ctx())
        .expect("a finished check shows something");
    assert!(!display.show_open_page());
    assert_eq!(presenter.update_url(), None);

    presenter.set_update_url(Some(foreign.to_owned()));
    let mut opened: Vec<String> = Vec::new();
    let status = presenter.open_update_page(
        &mut |url| {
            opened.push(url.to_owned());
            Ok(())
        },
        &env.ctx(),
    );
    assert_eq!(status, None);
    assert!(opened.is_empty());
}

#[test]
fn update_check_failures_have_their_own_text() {
    // C#: SettingsPresenterTests.UpdateCheck_Failures_HaveTheirOwnText (5 rows)
    let env = Env::new();
    let rows = [
        (UpdateFailure::Offline, "update.status.failed.offline"),
        (UpdateFailure::Timeout, "update.status.failed.timeout"),
        (UpdateFailure::RateLimited, "update.status.failed.rateLimited"),
        (UpdateFailure::InvalidVersion, "update.status.failed.invalidVersion"),
        (UpdateFailure::BadResponse, "update.status.failed.badResponse"),
    ];
    let mut seen: Vec<String> = Vec::new();
    for (failure, key) in rows {
        let mut presenter = new_presenter(
            &default_settings(),
            Some(checker_returning(UpdateCheckResult::failed(failure))),
        );
        let display = presenter
            .check_for_update_blocking(Some("2.0.93"), &env.ctx())
            .expect("a failure is displayed");
        assert_eq!(display.status_text, env.text(key), "{failure:?}");
        assert!(!display.show_open_page());
        seen.push(display.status_text);
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 5, "each failure has its own text");
}

#[test]
fn update_check_checker_panics_is_a_bad_response() {
    // C#: SettingsPresenterTests.UpdateCheck_CheckerThrows_IsABadResponse
    let env = Env::new();
    let checker: Arc<dyn UpdateCheck> =
        Arc::new(FakeChecker(|_: &CancelToken| -> Result<UpdateCheckResult, Canceled> {
            panic!("boom")
        }));
    let mut presenter = new_presenter(&default_settings(), Some(checker));
    let display = presenter
        .check_for_update_blocking(Some("2.0.93"), &env.ctx())
        .expect("a panic is shown as a bad response");
    assert_eq!(display.status_text, env.text("update.status.failed.badResponse"));
}

#[test]
fn update_check_cancelled_yields_nothing() {
    // C#: SettingsPresenterTests.UpdateCheck_Cancelled_YieldsNothing
    let env = Env::new();
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let started_tx = Mutex::new(started_tx);
    let checker: Arc<dyn UpdateCheck> = Arc::new(FakeChecker(move |cancel: &CancelToken| {
        let _ = started_tx.lock().unwrap_or_else(PoisonError::into_inner).send(());
        // Never completes on its own: only the token ends it (hang guard: 10 s of wall time).
        let deadline = Instant::now() + HANG_GUARD;
        while !cancel.is_canceled() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        if cancel.is_canceled() {
            Err(Canceled)
        } else {
            Ok(UpdateCheckResult::up_to_date("x"))
        }
    }));
    let mut presenter = new_presenter(&default_settings(), Some(checker));

    let job = presenter.begin_update_check(Some("2.0.93"));
    let worker = std::thread::spawn(move || job.run());
    started_rx
        .recv_timeout(HANG_GUARD)
        .expect("the checker started within the hang guard");
    presenter.cancel_update_check();
    let completed = worker.join().expect("worker finished");

    assert_eq!(completed.outcome, UpdateCheckOutcome::Canceled);
    assert_eq!(presenter.finish_update_check(completed, &env.ctx()), None);
}

#[test]
fn open_update_page_failure_is_reported() {
    // C#: SettingsPresenterTests.OpenUpdatePage_Failure_IsReported
    let env = Env::new();
    let mut presenter = new_presenter(
        &default_settings(),
        Some(checker_returning(UpdateCheckResult::update_available(
            "2.0.94",
            RELEASE_URL,
        ))),
    );
    presenter.check_for_update_blocking(Some("2.0.93"), &env.ctx());

    let status = presenter.open_update_page(&mut |_| Err("no browser".to_owned()), &env.ctx());

    assert_eq!(status, Some(env.text("update.openPage.failed")));
}

#[test]
fn update_check_stale_completion_after_a_second_check_started_is_ignored() {
    // New test (contract trap T2)
    let env = Env::new();
    let mut presenter = new_presenter(
        &default_settings(),
        Some(checker_returning(UpdateCheckResult::up_to_date("2.0.93"))),
    );
    let first = presenter.begin_update_check(Some("2.0.93")).run();
    presenter.cancel_update_check(); // the dialog of check 1 closed
    let second = presenter.begin_update_check(Some("2.0.93")).run(); // a newer check replaced the stored token

    assert_eq!(
        presenter.finish_update_check(first, &env.ctx()),
        None,
        "the answer of the cancelled check is stale even though a live token is stored"
    );
    assert!(presenter.finish_update_check(second, &env.ctx()).is_some());
}

#[test]
fn update_check_without_a_checker_is_a_bad_response() {
    // New test
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings(), None);
    let display = presenter
        .check_for_update_blocking(Some("2.0.93"), &env.ctx())
        .expect("no checker is a displayed failure");
    assert_eq!(display.status_text, env.text("update.status.failed.badResponse"));
}

// ---------------------------------------------------------------------------------------------------------------
// Explorer integration
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn shell_status_maps_states() {
    // C#: SettingsPresenterTests.ShellStatus_MapsStates
    let env = Env::new();
    let root = TempRoot::new("w8-10e-shell");
    let shell = FakeShell::new();
    let presenter = shell_presenter(&shell, Some(fake_executable(&root)));

    let not_registered = presenter.refresh_shell_status(&env.ctx());
    assert_eq!(not_registered.text, env.text("settings.shell.status.notRegistered"));
    assert!(not_registered.add_enabled);
    assert!(!not_registered.remove_enabled);
    assert_eq!(not_registered.add_label, Some(env.text("settings.shell.add")));

    shell.set_state(ShellMenuState::Registered);
    let registered = presenter.refresh_shell_status(&env.ctx());
    assert_eq!(registered.text, env.text("settings.shell.status.registered"));
    assert!(!registered.add_enabled);
    assert!(registered.remove_enabled);

    shell.set_state(ShellMenuState::RegisteredElsewhere);
    let elsewhere = presenter.refresh_shell_status(&env.ctx());
    assert_eq!(elsewhere.text, env.text("settings.shell.status.elsewhere"));
    assert!(elsewhere.add_enabled);
    assert!(elsewhere.remove_enabled);
    assert_eq!(elsewhere.add_label, Some(env.text("settings.shell.update")));
}

#[test]
fn shell_status_no_executable_is_unavailable() {
    // C#: SettingsPresenterTests.ShellStatus_NoExecutable_IsUnavailable
    let env = Env::new();
    let presenter = shell_presenter(&FakeShell::new(), None);

    let status = presenter.refresh_shell_status(&env.ctx());

    assert_eq!(status.text, env.text("settings.shell.status.unavailable"));
    assert!(!status.add_enabled);
    assert!(!status.remove_enabled);
    assert_eq!(status.add_label, None);
}

#[test]
fn shell_status_dotnet_host_is_unavailable() {
    // C#: SettingsPresenterTests.ShellStatus_DotnetHost_IsUnavailable
    let env = Env::new();
    let root = TempRoot::new("w8-10e-dotnet");
    for name in ["dotnet.exe", "DOTNET.EXE"] {
        let presenter = shell_presenter(&FakeShell::new(), Some(root.join(name).display().to_string()));
        assert_eq!(
            presenter.refresh_shell_status(&env.ctx()).text,
            env.text("settings.shell.status.unavailable"),
            "{name}"
        );
    }
}

#[test]
fn shell_status_relative_path_is_unavailable() {
    // New test
    let env = Env::new();
    let presenter = shell_presenter(&FakeShell::new(), Some("PhotoReview.exe".to_owned()));
    assert_eq!(
        presenter.refresh_shell_status(&env.ctx()).text,
        env.text("settings.shell.status.unavailable")
    );
}

#[test]
fn shell_change_registers_removes_and_reports_failure() {
    // C#: SettingsPresenterTests.ShellChange_RegistersRemovesAndReportsFailure
    let env = Env::new();
    let root = TempRoot::new("w8-10e-shell-change");
    let executable = fake_executable(&root);
    let shell = FakeShell::new();
    let presenter = shell_presenter(&shell, Some(executable.clone()));

    let added = presenter.change_shell_registration(true, &env.ctx());
    assert_eq!(shell.calls(), [format!("register {executable}")]);
    assert_eq!(added.text, env.text("settings.shell.status.registered"));

    let removed = presenter.change_shell_registration(false, &env.ctx());
    assert_eq!(removed.text, env.text("settings.shell.status.notRegistered"));

    shell.set_throws("denied");
    let failed = presenter.change_shell_registration(true, &env.ctx());
    assert_eq!(
        failed.text,
        env.text_with("settings.shell.status.error", "message", "denied")
    );
    // Only the text is replaced: the buttons follow the current (not registered) state.
    assert!(failed.add_enabled);
    assert!(!failed.remove_enabled);
}

// ---------------------------------------------------------------------------------------------------------------
// Language picker and log location
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn language_without_service_has_no_group() {
    // C#: SettingsPresenterTests.Language_WithoutService_HasNoGroup
    let env = Env::new();
    let presenter = new_presenter(&default_settings(), None);
    assert!(!presenter.has_localization());
    assert_eq!(presenter.load_languages(None, &env.ctx()), None);
    assert_eq!(presenter.reload_translations(), None);
    assert_eq!(
        presenter.prepare_languages_folder(),
        SettingsExplorerRequest {
            arguments: None,
            error_message: None
        }
    );
}

#[test]
fn export_translation_without_service_is_none_none() {
    // New test (C# asserts this inside Language_WithoutService_HasNoGroup)
    let presenter = new_presenter(&default_settings(), None);
    assert_eq!(presenter.export_translation(None), (None, None));
}

#[test]
#[ignore = "needs W8-04a export_todo"]
fn export_translation_with_service_selects_the_written_file() {
    // New test
    let root = TempRoot::new("w8-10e-export-translation");
    let (service, _user) = service_in(&root);
    let mut options = options();
    options.localization = Some(Arc::new(Mutex::new(service)));
    let presenter = presenter_from(&default_settings(), options);
    let (request, error) = presenter.export_translation(None);
    assert_eq!(error, None);
    let arguments = request.and_then(|request| request.arguments).expect("arguments");
    assert!(arguments.starts_with("/select,\""), "{arguments}");
}

#[test]
fn prepare_languages_folder_quotes_the_directory() {
    // New test
    let root = TempRoot::new("w8-10e-languages");
    let (service, user) = service_in(&root);
    let mut options = options();
    options.localization = Some(Arc::new(Mutex::new(service)));
    let presenter = presenter_from(&default_settings(), options);
    assert!(!user.exists());

    let request = presenter.prepare_languages_folder();

    assert_eq!(request.error_message, None);
    assert_eq!(request.arguments, Some(format!("\"{}\"", user.display())));
    assert!(user.is_dir(), "the folder is created");
}

#[test]
fn load_languages_selects_the_saved_language() {
    // New test
    let env = Env::new();
    let root = TempRoot::new("w8-10e-load-languages");
    let (service, _user) = service_in(&root);
    let mut options = options();
    options.localization = Some(Arc::new(Mutex::new(service)));
    let presenter = presenter_from(&default_settings(), options);

    let (languages, selected) = presenter.load_languages(None, &env.ctx()).expect("a service exists");

    assert_eq!(languages[0].code, "auto");
    assert_eq!(selected, Some(0), "the default UiLanguage is auto");
    let (_, unknown) = presenter
        .load_languages(Some("zz"), &env.ctx())
        .expect("a service exists");
    assert_eq!(unknown, None);
}

/// An `AppLogTarget` whose file lives in a temp folder, so the log-location test never creates the real data folder.
struct TempFileLog {
    path: PathBuf,
}

impl Log for TempFileLog {
    fn enabled(&self) -> bool {
        false
    }
    fn info(&self, message: &str) {
        NullLog.info(message);
    }
    fn warn(&self, message: &str) {
        NullLog.warn(message);
    }
    fn error(&self, message: &str, error: Option<&(dyn std::error::Error + 'static)>) {
        NullLog.error(message, error);
    }
}

impl AppLogTarget for TempFileLog {
    fn set_enabled(&self, _enabled: bool) {}
    fn flush(&self) {}
    fn shutdown(&self) {}
    fn file_path(&self) -> PathBuf {
        self.path.clone()
    }
}

#[test]
fn log_location_names_the_log_file() {
    // C#: SettingsPresenterTests.LogLocation_NamesTheLogFile
    let root = TempRoot::new("w8-10e-log");
    let log_file = root.join("logs").join("PhotoReviewRs.log");
    AppLog::global().set_instance(Some(Arc::new(TempFileLog { path: log_file.clone() })));

    let request = SettingsPresenter::prepare_log_location();

    AppLog::global().set_instance(None);
    assert_eq!(request.error_message, None);
    let arguments = request.arguments.expect("arguments");
    assert!(arguments.starts_with("/select,"), "{arguments}");
    assert!(arguments.contains(&*log_file.display().to_string()), "{arguments}");
    assert!(Path::new(&root.join("logs")).is_dir(), "the log folder is created");
}
