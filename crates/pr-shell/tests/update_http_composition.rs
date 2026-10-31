#![cfg(windows)]
//! W10-H3 F: production graph wiring and Settings presenter/job boundary, with fake checker/HTTP.
//! These assertions do not establish native control rendering, ShellExecute or dialog lifetime.
// Fixture also exposes registry/decoder helpers used by other integration test binaries.
#[allow(dead_code)]
#[path = "common/composition_fixture.rs"]
mod composition_fixture;
use pr_app::presenters::settings::{SettingsPresenter, SettingsPresenterOptions};
use pr_app::presenters::settings_field_map::FieldContext;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::sync::{CancelToken, Canceled};
use pr_core::updates::checker::{HttpError, HttpRequest, HttpResponse, HttpTransport, UpdateCheck, UpdateChecker};
use pr_core::updates::version::{UpdateCheckResult, UpdateFailure};
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_l10n::template::NumberCulture;
use pr_shell::composition::{graph::dialog_services_of, update_http::update_checker};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

const URL: &str = "https://github.com/ConanKLOP2/PhotoReview-rs/releases/tag/v2.0.94";
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
}
struct Checker<F>(F);
impl<F: Fn(&CancelToken) -> Result<UpdateCheckResult, Canceled> + Send + Sync> UpdateCheck for Checker<F> {
    fn check(&self, _: Option<&str>, cancel: &CancelToken) -> Result<UpdateCheckResult, Canceled> {
        (self.0)(cancel)
    }
}
fn fixed(result: UpdateCheckResult) -> Arc<dyn UpdateCheck> {
    Arc::new(Checker(move |_: &CancelToken| Ok(result.clone())))
}
fn presenter(checker: Arc<dyn UpdateCheck>) -> SettingsPresenter {
    let (services, _root, _) = composition_fixture::shared_services();
    let dialogs = dialog_services_of(&services);
    assert!(
        dialogs.update_checker.is_some(),
        "production graph must supply the fallback checker"
    );
    let mut options = SettingsPresenterOptions::new(
        32_i64 << 30,
        Arc::new(EnglishSettingsTexts),
        dialogs.shell_integration.clone(),
    );
    options.update_checker = Some(checker);
    SettingsPresenter::new(&AppSettings::default(), options)
}

#[test]
fn update_check_up_to_date_shows_status_no_open_button() {
    // C#: SettingsWindowUpdateCheckTests.UpToDate_ShowsStatus_NoOpenButton; SettingsPresenterTests.UpdateCheck_UpToDate.
    let env = Env::new();
    let mut p = presenter(fixed(UpdateCheckResult::up_to_date("2.0.93")));
    let display = p.check_for_update_blocking(Some("2.0.93"), &env.ctx()).unwrap();
    assert!(!display.show_open_page());
    assert_eq!(p.update_url(), None);
    assert_eq!(
        display.status_text,
        env.texts.format_with(
            "update.status.upToDate",
            &[pr_l10n::template::LocArg::new("version", "2.0.93")],
            &env.numbers
        )
    );
}
#[test]
fn update_check_available_shows_open_button_and_opens_url() {
    // C#: SettingsWindowUpdateCheckTests.UpdateAvailable_ShowsOpenButton_AndOpensUrl; SettingsPresenterTests.UpdateCheck_Available_OffersValidatedUrl.
    let env = Env::new();
    let mut p = presenter(fixed(UpdateCheckResult::update_available("2.0.94", URL)));
    let display = p.check_for_update_blocking(Some("v2.0.93+build"), &env.ctx()).unwrap();
    assert!(display.show_open_page());
    assert_eq!(display.download_url.as_deref(), Some(URL));
    assert!(display.status_text.contains("2.0.94") && display.status_text.contains("2.0.93"));
    assert!(!display.status_text.contains("+build"));
    let mut opened = Vec::new();
    assert_eq!(
        p.open_update_page(
            &mut |url| {
                opened.push(url.to_owned());
                Ok(())
            },
            &env.ctx()
        ),
        None
    );
    assert_eq!(opened, [URL]);
    // A new check clears the previous offered URL before the worker runs.
    let _job = p.begin_update_check(Some("2.0.93"));
    assert_eq!(p.update_url(), None);
}
#[test]
fn update_check_foreign_url_never_opens() {
    // C#: SettingsPresenterTests.UpdateCheck_ForeignUrl_IsNeverOpened;
    // SettingsWindowUpdateCheckTests.UpdateAvailable_ForeignUrl_NoOpenButton and OpenPage_RevalidatesUrl.
    let env = Env::new();
    let foreign = "https://evil.example/x.exe";
    let mut p = presenter(fixed(UpdateCheckResult::update_available("2.0.94", foreign)));
    assert!(
        !p.check_for_update_blocking(Some("2.0.93"), &env.ctx())
            .unwrap()
            .show_open_page()
    );
    assert_eq!(p.update_url(), None);
    p.set_update_url(Some(foreign.into()));
    let mut opened = Vec::new();
    assert_eq!(
        p.open_update_page(
            &mut |url| {
                opened.push(url.to_owned());
                Ok(())
            },
            &env.ctx()
        ),
        None
    );
    assert!(opened.is_empty());
}
#[test]
fn update_check_failures_have_their_own_text() {
    // C#: SettingsPresenterTests.UpdateCheck_Failures_HaveTheirOwnText; SettingsWindowUpdateCheckTests.Failed_ShowsMessage.
    let env = Env::new();
    let mut seen = Vec::new();
    for (failure, key) in [
        (UpdateFailure::Offline, "update.status.failed.offline"),
        (UpdateFailure::Timeout, "update.status.failed.timeout"),
        (UpdateFailure::RateLimited, "update.status.failed.rateLimited"),
        (UpdateFailure::InvalidVersion, "update.status.failed.invalidVersion"),
        (UpdateFailure::BadResponse, "update.status.failed.badResponse"),
    ] {
        let mut p = presenter(fixed(UpdateCheckResult::failed(failure)));
        let display = p.check_for_update_blocking(Some("2.0.93"), &env.ctx()).unwrap();
        assert_eq!(display.status_text, env.texts.get(key));
        assert!(!display.show_open_page());
        seen.push(display.status_text);
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 5);
}
#[test]
fn update_check_checker_throws_shows_bad_response() {
    // C#: SettingsPresenterTests.UpdateCheck_CheckerThrows_IsABadResponse; SettingsWindowUpdateCheckTests.CheckerThrows_ShowsFailure.
    let env = Env::new();
    let mut p = presenter(Arc::new(Checker(|_: &CancelToken| panic!("fake checker failure"))));
    let display = p.check_for_update_blocking(Some("2.0.93"), &env.ctx()).unwrap();
    assert_eq!(display.status_text, env.texts.get("update.status.failed.badResponse"));
    assert!(!display.show_open_page());
}
#[test]
fn update_check_cancelled_yields_no_ui_write() {
    // C#: SettingsPresenterTests.UpdateCheck_Cancelled_YieldsNothing; includes a late successful completion after cancel.
    let env = Env::new();
    for honors_cancel in [false, true] {
        let mut p = presenter(Arc::new(Checker(move |cancel: &CancelToken| {
            if honors_cancel {
                cancel.check()?;
            }
            Ok(UpdateCheckResult::update_available("2.0.94", URL))
        })));
        let job = p.begin_update_check(Some("2.0.93"));
        p.cancel_update_check();
        assert_eq!(p.finish_update_check(job.run(), &env.ctx()), None);
        assert_eq!(p.update_url(), None);
    }
}
#[test]
fn open_update_page_failure_is_reported() {
    // C#: SettingsPresenterTests.OpenUpdatePage_Failure_IsReported.
    let env = Env::new();
    let mut p = presenter(fixed(UpdateCheckResult::update_available("2.0.94", URL)));
    p.check_for_update_blocking(Some("2.0.93"), &env.ctx()).unwrap();
    assert_eq!(
        p.open_update_page(&mut |_| Err("no browser".into()), &env.ctx()),
        Some(env.texts.get("update.openPage.failed"))
    );
}
struct CountingTransport(Arc<AtomicUsize>);
impl HttpTransport for CountingTransport {
    fn get(&self, _: &HttpRequest, _: Duration, _: &CancelToken) -> Result<HttpResponse, HttpError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(HttpError::Timeout)
    }
}
#[test]
fn update_checker_factory_before_click_sends_no_request() {
    // New: IUpdateChecker.cs:3-9 manual-only network use. Factory plus real checker over a counting fake.
    let factory = update_checker();
    // Invalid version returns before transport entry, so this never makes a real network call.
    assert_eq!(
        factory.check(Some("invalid"), &CancelToken::new()).unwrap().failure(),
        UpdateFailure::InvalidVersion
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let checker = Arc::new(UpdateChecker::new(Arc::new(CountingTransport(calls.clone())), None));
    let mut p = presenter(checker);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let job = p.begin_update_check(Some("2.0.93"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let env = Env::new();
    let display = p.finish_update_check(job.run(), &env.ctx()).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(display.status_text, env.texts.get("update.status.failed.timeout"));
}
#[test]
#[ignore = "native: attended Settings close/worker/ShellExecute verification required"]
fn native_settings_close_cancels_and_open_page_uses_shell() {
    panic!("requires attended desktop; fake opener tests do not establish ShellExecute behavior");
}

// Real General page with a queued worker/poster and a recording UI host. The queue makes
// checking/reentrance and late completion deterministic, without sleeps or desktop handles.
use pr_platform_win::dialogs::settings::{
    builder::inputs::SettingsPageBuilder,
    context::{
        ActionProfilesUnavailable, ContextUiAction, SettingsPageContext, SettingsPageHost, SettingsUiPost,
        SharedSettingsPresenter,
    },
    page_view::{SettingsPageView, TextFn},
    pages::general::GeneralSettingsPage,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Mutex, PoisonError},
};
#[derive(Default)]
struct Post(Mutex<Vec<ContextUiAction>>);
impl SettingsUiPost for Post {
    fn post(&self, action: ContextUiAction) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push(action);
    }
}
struct UiHost {
    presenter: SharedSettingsPresenter,
    post: Arc<Post>,
    text: RefCell<BTreeMap<i32, String>>,
    enabled: RefCell<BTreeMap<i32, bool>>,
    visible: RefCell<BTreeMap<i32, bool>>,
    opened: RefCell<Vec<String>>,
}
impl SettingsPageHost for UiHost {
    fn presenter(&self) -> SharedSettingsPresenter {
        self.presenter.clone()
    }
    fn hwnd(&self) -> pr_platform_win::win32::structs::Hwnd {
        0
    }
    fn files(&self) -> Option<Rc<dyn pr_platform_win::dialogs::file_dialog::ShellFileDialog>> {
        None
    }
    fn decoder_factory(&self) -> Option<Arc<dyn pr_imaging::decoder_traits::ImageDecoderFactory>> {
        None
    }
    fn version(&self) -> Option<String> {
        Some("2.0.93".into())
    }
    fn open_url(&self, url: &str) -> Result<(), String> {
        self.opened.borrow_mut().push(url.into());
        Ok(())
    }
    fn start_explorer(&self, _: &str) -> Option<String> {
        panic!("unexpected Explorer call")
    }
    fn show_action_profiles(
        &self,
        _: pr_platform_win::win32::structs::Hwnd,
        _: &[pr_core::settings::shortcuts::ReviewAction],
    ) -> Result<Option<Vec<pr_core::settings::shortcuts::ReviewAction>>, ActionProfilesUnavailable> {
        Err(ActionProfilesUnavailable)
    }
    fn control_text(&self, id: i32) -> String {
        self.text.borrow().get(&id).cloned().unwrap_or_default()
    }
    fn set_control_text(&self, id: i32, value: &str) {
        self.text.borrow_mut().insert(id, value.into());
    }
    fn is_control_checked(&self, _: i32) -> bool {
        false
    }
    fn set_control_checked(&self, _: i32, _: bool) {}
    fn combo_selected_index(&self, _: i32) -> i32 {
        -1
    }
    fn set_combo_selected_index(&self, _: i32, _: i32) {}
    fn set_combo_items(&self, _: i32, _: &[String], _: i32) {}
    fn slider_value(&self, _: i32) -> i32 {
        0
    }
    fn set_slider_value(&self, _: i32, _: i32) {}
    fn set_slider_range(&self, _: i32, _: i32, _: i32) {}
    fn set_control_tooltip(&self, _: i32, _: TextFn) {}
    fn current_shortcut_texts(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
    fn set_control_enabled(&self, id: i32, value: bool) {
        self.enabled.borrow_mut().insert(id, value);
    }
    fn is_control_enabled(&self, id: i32) -> bool {
        self.enabled.borrow().get(&id).copied().unwrap_or(true)
    }
    fn set_control_visible(&self, id: i32, value: bool) {
        self.visible.borrow_mut().insert(id, value);
    }
    fn focus_control(&self, _: i32) {}
    fn relayout_active_page(&self) {}
    fn warn(&self, _: &str, _: &str) {
        panic!("unexpected warning")
    }
    fn ui_post(&self) -> Arc<dyn SettingsUiPost> {
        self.post.clone()
    }
    fn actions_json(&self) -> String {
        String::new()
    }
    fn refresh_actions_json_from_pages(&self) -> String {
        String::new()
    }
    fn set_actions_json(&self, _: &str, _: &str) {}
}
#[test]
fn general_page_click_guard_completion_visibility_and_open_revalidation_match_csharp() {
    // C#: SettingsWindowUpdateCheckTests.UpToDate_ShowsStatus_NoOpenButton,
    // UpdateAvailable_ShowsOpenButton_AndOpensUrl, UpdateAvailable_ForeignUrl_NoOpenButton,
    // OpenPage_RevalidatesUrl, Failed_ShowsMessage, CheckerThrows_ShowsFailure.
    let env = Env::new();
    let cases = [
        (Some(UpdateCheckResult::up_to_date("2.0.93")), false, None),
        (Some(UpdateCheckResult::update_available("2.0.94", URL)), true, None),
        (
            Some(UpdateCheckResult::update_available("2.0.94", "https://evil.example/x")),
            false,
            None,
        ),
        (
            Some(UpdateCheckResult::failed(UpdateFailure::Offline)),
            false,
            Some("update.status.failed.offline"),
        ),
        (
            Some(UpdateCheckResult::failed(UpdateFailure::Timeout)),
            false,
            Some("update.status.failed.timeout"),
        ),
        (
            Some(UpdateCheckResult::failed(UpdateFailure::RateLimited)),
            false,
            Some("update.status.failed.rateLimited"),
        ),
        (
            Some(UpdateCheckResult::failed(UpdateFailure::BadResponse)),
            false,
            Some("update.status.failed.badResponse"),
        ),
        (None, false, Some("update.status.failed.badResponse")),
    ];
    for (result, offered, error_key) in cases {
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let checker = Arc::new(Checker(move |_: &CancelToken| {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(result.clone().unwrap_or_else(|| panic!("fake checker throws")))
        }));
        let host = Rc::new(UiHost {
            presenter: Rc::new(RefCell::new(presenter(checker))),
            post: Arc::new(Post::default()),
            text: RefCell::default(),
            enabled: RefCell::default(),
            visible: RefCell::default(),
            opened: RefCell::default(),
        });
        type Job = Box<dyn FnOnce() + Send>;
        let jobs: Rc<RefCell<Vec<Job>>> = Rc::new(RefCell::new(Vec::new()));
        let queued = jobs.clone();
        let mut page = GeneralSettingsPage::with_background_runner(Rc::new(move |job| {
            queued.borrow_mut().push(job);
            true
        }));
        page.declare(&mut SettingsPageBuilder::new(1000));
        let context = SettingsPageContext::new(host.clone());
        page.attach(context.clone());
        let ids = page.ids();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(page.on_command(ids.check_update, 0));
        assert!(!host.is_control_enabled(ids.check_update));
        assert_eq!(
            host.control_text(ids.update_status),
            pr_l10n::tr::update_check_checking()
        );
        assert_eq!(host.visible.borrow().get(&ids.open_update_page), Some(&false));
        page.on_command(ids.check_update, 0);
        assert_eq!(jobs.borrow().len(), 1);
        jobs.borrow_mut().pop().unwrap()();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // Deliver while borrowed: the page must repost, then finish after the borrow ends.
        let action = host.post.0.lock().unwrap().pop().unwrap();
        let borrowed = host.presenter.borrow_mut();
        action(&context);
        drop(borrowed);
        host.post.0.lock().unwrap().pop().unwrap()(&context);
        assert!(host.is_control_enabled(ids.check_update));
        assert_eq!(host.visible.borrow().get(&ids.open_update_page), Some(&offered));
        if let Some(key) = error_key {
            assert_eq!(host.control_text(ids.update_status), env.texts.get(key));
        }
        assert!(host.opened.borrow().is_empty());
        page.on_command(ids.open_update_page, 0);
        assert_eq!(
            *host.opened.borrow(),
            if offered { vec![URL.to_owned()] } else { vec![] }
        );
        host.opened.borrow_mut().clear();
        host.presenter
            .borrow_mut()
            .set_update_url(Some("https://evil.example/x".into()));
        page.on_command(ids.open_update_page, 0);
        assert!(host.opened.borrow().is_empty());
    }
}
