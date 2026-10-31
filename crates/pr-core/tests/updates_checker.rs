//! Port of the C# `Updates` checker tests (W2-18): `UpdateCheckerTests`, `UpdateCheckerGapTests`,
//! `UpdateCheckerBoundaryTests`, plus the `update-checker` golden.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pr_core::sync::{CancelToken, Canceled};
use pr_core::updates::checker::{
    ACCEPT, DEFAULT_TIMEOUT, HttpBody, HttpError, HttpRequest, HttpRequestError, HttpResponse, HttpTransport,
    LATEST_RELEASE_URL, MAX_BODY_BYTES, USER_AGENT, UpdateCheck, UpdateChecker,
};
use pr_core::updates::version::{RELEASES_PAGE_URL, UpdateCheckResult, UpdateCheckStatus, UpdateFailure};
use pr_test_support::golden;
use serde_json::Value;

const TAG_URL: &str = "https://github.com/ConanKLOP2/PhotoReview-rs/releases/tag/v2.0.94";

type Responder = Box<dyn Fn(&HttpRequest, Duration, &CancelToken) -> Result<HttpResponse, HttpError> + Send + Sync>;

struct Fake {
    respond: Responder,
    requests: Mutex<Vec<HttpRequest>>,
    timeouts: Mutex<Vec<Duration>>,
}

impl Fake {
    fn new(respond: Responder) -> Arc<Fake> {
        Arc::new(Fake {
            respond,
            requests: Mutex::new(Vec::new()),
            timeouts: Mutex::new(Vec::new()),
        })
    }

    fn request_count(&self) -> usize {
        self.requests.lock().expect("lock").len()
    }
}

impl HttpTransport for Fake {
    fn get(&self, request: &HttpRequest, timeout: Duration, cancel: &CancelToken) -> Result<HttpResponse, HttpError> {
        self.requests.lock().expect("lock").push(request.clone());
        self.timeouts.lock().expect("lock").push(timeout);
        (self.respond)(request, timeout, cancel)
    }
}

struct VecBody {
    data: Vec<u8>,
    pos: usize,
    requested: Arc<AtomicBool>,
}

impl HttpBody for VecBody {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, HttpError> {
        self.requested.store(true, Ordering::SeqCst);
        // Small reads so the checker's loop is exercised across chunk boundaries.
        let n = buf.len().min(7000).min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

fn response(
    status: u16,
    headers: Vec<(String, String)>,
    content_length: Option<u64>,
    data: Vec<u8>,
    requested: Arc<AtomicBool>,
) -> HttpResponse {
    HttpResponse {
        status,
        headers,
        content_length,
        body: Box::new(VecBody {
            data,
            pos: 0,
            requested,
        }),
    }
}

fn json(status: u16, body: &str) -> Arc<Fake> {
    let data = body.as_bytes().to_vec();
    Fake::new(Box::new(move |_, _, _| {
        Ok(response(
            status,
            Vec::new(),
            Some(data.len() as u64),
            data.clone(),
            Arc::new(AtomicBool::new(false)),
        ))
    }))
}

fn with_headers(status: u16, headers: &[(&str, &str)]) -> Arc<Fake> {
    let headers: Vec<(String, String)> = headers.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect();
    Fake::new(Box::new(move |_, _, _| {
        Ok(response(
            status,
            headers.clone(),
            Some(2),
            b"{}".to_vec(),
            Arc::new(AtomicBool::new(false)),
        ))
    }))
}

fn failing(error: HttpError) -> Arc<Fake> {
    Fake::new(Box::new(move |_, _, _| Err(error.clone())))
}

fn release(tag: &str, url: &str, draft: bool, prerelease: bool) -> String {
    format!(r#"{{"tag_name":"{tag}","html_url":"{url}","draft":{draft},"prerelease":{prerelease}}}"#)
}

fn check_with(
    fake: &Arc<Fake>,
    current: Option<&str>,
    timeout: Option<Duration>,
    cancel: &CancelToken,
) -> Result<UpdateCheckResult, Canceled> {
    UpdateChecker::new(fake.clone(), timeout).check(current, cancel)
}

fn check(fake: &Arc<Fake>) -> UpdateCheckResult {
    check_with(fake, Some("2.0.93+abc123"), None, &CancelToken::new()).expect("not canceled")
}

fn failure_of(fake: &Arc<Fake>) -> UpdateFailure {
    let r = check(fake);
    assert_eq!(r.status(), UpdateCheckStatus::Failed);
    r.failure()
}

// C#: UpdateCheckerTests.Newer_ReturnsUpdateAvailable
#[test]
fn newer_returns_update_available() {
    let r = check(&json(200, &release("v2.0.94", TAG_URL, false, false)));
    assert_eq!(r.status(), UpdateCheckStatus::UpdateAvailable);
    assert_eq!(r.latest_version(), Some("2.0.94"));
    assert_eq!(r.download_url(), Some(TAG_URL));
}

// C#: UpdateCheckerTests.SameOrOlder_ReturnsUpToDate
#[test]
fn same_or_older_returns_up_to_date() {
    for tag in ["v2.0.93", "v2.0.10"] {
        let r = check(&json(200, &release(tag, TAG_URL, false, false)));
        assert_eq!(r.status(), UpdateCheckStatus::UpToDate, "{tag}");
        assert_eq!(r.download_url(), None);
    }
}

// C#: UpdateCheckerTests.ForeignUrl_FallsBackToReleasesPage
#[test]
fn foreign_url_falls_back_to_releases_page() {
    let r = check(&json(
        200,
        &release("v2.0.94", "https://evil.example/download.exe", false, false),
    ));
    assert_eq!(r.status(), UpdateCheckStatus::UpdateAvailable);
    assert_eq!(r.download_url(), Some(RELEASES_PAGE_URL));
}

// C#: UpdateCheckerTests.DraftOrPrerelease_IsIgnored
#[test]
fn draft_or_prerelease_is_ignored() {
    for (draft, pre, tag) in [
        (true, false, "v2.0.94"),
        (false, true, "v2.0.94"),
        (false, false, "v2.0.94-beta.1"),
    ] {
        let r = check(&json(200, &release(tag, TAG_URL, draft, pre)));
        assert_eq!(r.status(), UpdateCheckStatus::UpToDate, "{draft} {pre} {tag}");
    }
}

// C#: UpdateCheckerTests.HttpError_MapsToFailure
#[test]
fn http_error_maps_to_failure() {
    for (status, expected) in [
        (403, UpdateFailure::BadResponse),
        (429, UpdateFailure::RateLimited),
        (404, UpdateFailure::BadResponse),
        (500, UpdateFailure::BadResponse),
    ] {
        assert_eq!(failure_of(&json(status, "{}")), expected, "{status}");
    }
}

// C#: UpdateCheckerTests.BadJson_ReturnsBadResponse
#[test]
fn bad_json_returns_bad_response() {
    for body in [
        "not json",
        "",
        "[]",
        "{}",
        r#"{"tag_name":"garbage","html_url":"x"}"#,
        r#"{"tag_name":42}"#,
    ] {
        assert_eq!(failure_of(&json(200, body)), UpdateFailure::BadResponse, "{body:?}");
    }
}

// C#: UpdateCheckerTests.NetworkException_ReturnsOffline
#[test]
fn network_exception_returns_offline() {
    assert_eq!(
        failure_of(&failing(HttpError::Request(HttpRequestError::Unknown))),
        UpdateFailure::Offline
    );
}

// C#: UpdateCheckerTests.TimeoutStyleCancellation_ReturnsTimeout
#[test]
fn timeout_style_cancellation_returns_timeout() {
    // The transport reports Canceled but the caller's token is not canceled: a timeout.
    assert_eq!(failure_of(&failing(HttpError::Canceled)), UpdateFailure::Timeout);
    assert_eq!(failure_of(&failing(HttpError::Timeout)), UpdateFailure::Timeout);
}

fn hang() -> Arc<Fake> {
    Fake::new(Box::new(|_, timeout, cancel| {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if cancel.is_canceled() {
                return Err(HttpError::Canceled);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Err(HttpError::Timeout)
    }))
}

// C#: UpdateCheckerTests.SlowServer_TimesOut
#[test]
fn slow_server_times_out() {
    let fake = hang();
    let r = check_with(
        &fake,
        Some("2.0.93"),
        Some(Duration::from_millis(50)),
        &CancelToken::new(),
    )
    .expect("ok");
    assert_eq!(r.failure(), UpdateFailure::Timeout);
}

// C#: UpdateCheckerTests.CallerCancel_Throws
#[test]
fn caller_cancel_throws() {
    let cancel = CancelToken::new();
    let fake = Fake::new(Box::new(|_, _, cancel| {
        // Wait for the caller to cancel (poll every 1 ms like the real transport would).
        loop {
            if cancel.is_canceled() {
                return Err(HttpError::Canceled);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }));
    let canceler = cancel.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        canceler.cancel();
    });
    let result = check_with(&fake, Some("2.0.93"), None, &cancel);
    handle.join().expect("join");
    assert_eq!(result, Err(Canceled));
}

// C#: UpdateCheckerTests.InvalidCurrentVersion_FailsWithoutNetwork
#[test]
fn invalid_current_version_fails_without_network() {
    for current in [None, Some("garbage")] {
        let fake = json(200, &release("v9.9.9", TAG_URL, false, false));
        let r = check_with(&fake, current, None, &CancelToken::new()).expect("ok");
        assert_eq!(r.failure(), UpdateFailure::InvalidVersion, "{current:?}");
        assert_eq!(fake.request_count(), 0);
    }
}

// DV-22: the endpoint and the releases page are the Rust repository's, derived from the one configured repo.
#[test]
fn endpoints_follow_the_rust_repository_not_the_csharp_one() {
    assert_eq!(
        LATEST_RELEASE_URL,
        "https://api.github.com/repos/ConanKLOP2/PhotoReview-rs/releases/latest"
    );
    assert_eq!(
        RELEASES_PAGE_URL,
        "https://github.com/ConanKLOP2/PhotoReview-rs/releases"
    );
    assert!(!LATEST_RELEASE_URL.contains("ConanKLOP2/PhotoReview/"));
    assert!(!RELEASES_PAGE_URL.contains("ConanKLOP2/PhotoReview/"));
}

// DV-22 risk: a repository without any release answers 404 on `releases/latest`. That is a failure
// (BadResponse, as in C#), never an update, and never a crash.
#[test]
fn no_release_published_404_is_a_failure_not_an_update() {
    let fake = json(404, r#"{"message":"Not Found"}"#);
    let r = check(&fake);
    assert_eq!(r.status(), UpdateCheckStatus::Failed);
    assert_eq!(r.failure(), UpdateFailure::BadResponse);
    assert_eq!(r.download_url(), None);
}

// A release whose html_url points at the old C# repository is not opened: it falls back to the Rust releases page.
#[test]
fn release_url_of_the_csharp_repository_falls_back_to_the_rust_releases_page() {
    let fake = json(
        200,
        &release(
            "v9.0.0",
            "https://github.com/ConanKLOP2/PhotoReview/releases/tag/v9.0.0",
            false,
            false,
        ),
    );
    let r = check(&fake);
    assert_eq!(r.status(), UpdateCheckStatus::UpdateAvailable);
    assert_eq!(r.download_url(), Some(RELEASES_PAGE_URL));
}

// C#: UpdateCheckerTests.Request_IsMinimalAndHttps
#[test]
fn request_is_minimal_and_https() {
    let fake = json(200, &release("v2.0.93", TAG_URL, false, false));
    check(&fake);
    let requests = fake.requests.lock().expect("lock");
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        request.url,
        "https://api.github.com/repos/ConanKLOP2/PhotoReview-rs/releases/latest"
    );
    assert_eq!(request.url, LATEST_RELEASE_URL);
    assert_eq!(request.user_agent, USER_AGENT);
    assert_eq!(request.accept, ACCEPT);
    assert!(!request.url.contains('?'));
}

// C#: UpdateCheckerTests.OversizedBody_ReturnsBadResponse
#[test]
fn oversized_body_returns_bad_response() {
    let body = release("v2.0.94", TAG_URL, false, false) + &" ".repeat(MAX_BODY_BYTES + 10);
    assert_eq!(failure_of(&json(200, &body)), UpdateFailure::BadResponse);
}

// C#: UpdateCheckerGapTests.CheckAsync_Http403WithANonRateLimitBody_ReturnsFailedWithoutThrowing
#[test]
fn check_http_403_with_a_non_rate_limit_body_returns_failed() {
    let fake = json(403, r#"{"message":"Resource not accessible by integration"}"#);
    assert_eq!(check(&fake).status(), UpdateCheckStatus::Failed);
}

// C#: UpdateCheckerGapTests.CheckAsync_HandlerThrowsInvalidOperationException_ReturnsFailedWithoutThrowing
#[test]
fn check_handler_throws_other_returns_failed() {
    let r = check(&failing(HttpError::Other("handler broke".into())));
    assert_eq!(r.status(), UpdateCheckStatus::Failed);
    assert_eq!(r.failure(), UpdateFailure::BadResponse);
}

// C#: UpdateCheckerGapTests.Http403_RemainingZero_IsRateLimited
#[test]
fn http_403_remaining_zero_is_rate_limited() {
    assert_eq!(
        failure_of(&with_headers(403, &[("X-RateLimit-Remaining", "0")])),
        UpdateFailure::RateLimited
    );
}

// C#: UpdateCheckerGapTests.Http403_RetryAfter_IsRateLimited
#[test]
fn http_403_retry_after_is_rate_limited() {
    assert_eq!(
        failure_of(&with_headers(403, &[("Retry-After", "30")])),
        UpdateFailure::RateLimited
    );
}

// C#: UpdateCheckerGapTests.Http403_RemainingPositive_IsBadResponse
#[test]
fn http_403_remaining_positive_is_bad_response() {
    assert_eq!(
        failure_of(&with_headers(403, &[("X-RateLimit-Remaining", "57")])),
        UpdateFailure::BadResponse
    );
}

// C#: UpdateCheckerGapTests.HttpRequestError_IsClassified
#[test]
fn http_request_error_is_classified() {
    use HttpRequestError::*;
    for (error, expected) in [
        (NameResolutionError, UpdateFailure::Offline),
        (ConnectionError, UpdateFailure::Offline),
        (ProxyTunnelError, UpdateFailure::Offline),
        (Unknown, UpdateFailure::Offline),
        (SecureConnectionError, UpdateFailure::BadResponse),
        (InvalidResponse, UpdateFailure::BadResponse),
        (ResponseEnded, UpdateFailure::BadResponse),
    ] {
        assert_eq!(failure_of(&failing(HttpError::Request(error))), expected, "{error:?}");
    }
}

fn padded(text: &str, total: usize) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.resize(total, b' ');
    bytes
}

fn sized(data: Vec<u8>, content_length: Option<u64>) -> (Arc<Fake>, Arc<AtomicBool>) {
    let requested = Arc::new(AtomicBool::new(false));
    let flag = requested.clone();
    let fake = Fake::new(Box::new(move |_, _, _| {
        Ok(response(200, Vec::new(), content_length, data.clone(), flag.clone()))
    }));
    (fake, requested)
}

const MIN_RELEASE: &str = r#"{"tag_name":"v2.0.94"}"#;

// C#: UpdateCheckerBoundaryTests.ExactlyMaxBytes_WithContentLength_IsAccepted
#[test]
fn exactly_max_bytes_with_content_length_is_accepted() {
    let (fake, _) = sized(padded(MIN_RELEASE, MAX_BODY_BYTES), Some(MAX_BODY_BYTES as u64));
    assert_eq!(check(&fake).status(), UpdateCheckStatus::UpdateAvailable);
}

// C#: UpdateCheckerBoundaryTests.ExactlyMaxBytes_WithoutContentLength_IsAccepted
#[test]
fn exactly_max_bytes_without_content_length_is_accepted() {
    let (fake, _) = sized(padded(MIN_RELEASE, MAX_BODY_BYTES), None);
    assert_eq!(check(&fake).status(), UpdateCheckStatus::UpdateAvailable);
}

// C#: UpdateCheckerBoundaryTests.OneByteOverMax_WithoutContentLength_IsRejected
#[test]
fn one_byte_over_max_without_content_length_is_rejected() {
    let (fake, _) = sized(padded(MIN_RELEASE, MAX_BODY_BYTES + 1), None);
    assert_eq!(failure_of(&fake), UpdateFailure::BadResponse);
}

// C#: UpdateCheckerBoundaryTests.DeclaredLengthOverMax_IsRejected
#[test]
fn declared_length_over_max_is_rejected_without_reading() {
    let (fake, requested) = sized(padded(MIN_RELEASE, MAX_BODY_BYTES + 1), Some(MAX_BODY_BYTES as u64 + 1));
    assert_eq!(failure_of(&fake), UpdateFailure::BadResponse);
    assert!(!requested.load(Ordering::SeqCst), "the body must never be requested");
}

// C#: UpdateCheckerBoundaryTests.NonStringHtmlUrl_FallsBackToReleasesPage
#[test]
fn non_string_html_url_falls_back_to_releases_page() {
    for raw in ["123", "true", "{}"] {
        let body = format!(r#"{{"tag_name":"v2.0.94","html_url":{raw}}}"#);
        let r = check(&json(200, &body));
        assert_eq!(r.status(), UpdateCheckStatus::UpdateAvailable, "{raw}");
        assert_eq!(r.download_url(), Some(RELEASES_PAGE_URL), "{raw}");
    }
}

// C#: UpdateCheckerBoundaryTests.CustomTimeout_IsHonoured
#[test]
fn custom_timeout_is_honoured() {
    let fake = hang();
    let started = Instant::now();
    let r = check_with(
        &fake,
        Some("2.0.93"),
        Some(Duration::from_millis(100)),
        &CancelToken::new(),
    )
    .expect("ok");
    assert_eq!(r.failure(), UpdateFailure::Timeout);
    assert_eq!(fake.timeouts.lock().expect("lock")[0], Duration::from_millis(100));
    assert!(started.elapsed() >= Duration::from_millis(100));
}

#[test]
fn default_timeout_is_used_when_none() {
    let fake = json(200, MIN_RELEASE);
    check(&fake);
    assert_eq!(fake.timeouts.lock().expect("lock")[0], DEFAULT_TIMEOUT);
    assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(10));
}

// ---- differential-derived cases (repair round 1) ------------------------------------------------------------
//
// Expected values below were produced by running the C# `UpdateChecker` pieces (.NET 10, baseline d92d6aae) on
// the same inputs: `HttpResponseMessage.Headers.RetryAfter` and `JsonDocument.Parse` + `TryGetProperty`.

// C#: UpdateChecker.CheckAsync status gate (`IsSuccessStatusCode`): 300 with a parsable release is a failure.
#[test]
fn status_300_with_valid_release_is_bad_response() {
    for status in [199u16, 300, 301, 302] {
        let fake = json(status, &release("v2.0.94", TAG_URL, false, false));
        assert_eq!(failure_of(&fake), UpdateFailure::BadResponse, "status {status}");
    }
    for status in [200u16, 201, 299] {
        let fake = json(status, &release("v2.0.94", TAG_URL, false, false));
        assert_eq!(
            check(&fake).status(),
            UpdateCheckStatus::UpdateAvailable,
            "status {status}"
        );
    }
}

// C#: JsonDocument.Parse rejects every raw control character U+0000..U+001F inside a string.
#[test]
fn raw_control_character_in_json_string_is_bad_response() {
    for c in ['\u{0}', '\u{1}', '\u{9}', '\u{a}', '\u{1e}', '\u{1f}'] {
        let body = format!(r#"{{"x":"a{c}b","tag_name":"v2.0.94"}}"#);
        assert_eq!(
            failure_of(&json(200, &body)),
            UpdateFailure::BadResponse,
            "U+{:04X}",
            c as u32
        );
    }
    // U+0020 and U+007F are fine.
    for c in [' ', '\u{7f}'] {
        let body = format!(r#"{{"x":"a{c}b","tag_name":"v2.0.94"}}"#);
        assert_eq!(check(&json(200, &body)).status(), UpdateCheckStatus::UpdateAvailable);
    }
}

// C#: Interpret / JsonElement.TryGetProperty with a lone-surrogate property NAME: only a lookup that actually has
// to unescape that name throws (and then BadResponse); everything else is unaffected.
#[test]
fn lone_surrogate_property_name_only_fails_when_compared() {
    const RELEASES: &str = "https://github.com/ConanKLOP2/PhotoReview-rs/releases";
    let up_to_date_running = UpdateCheckResult::up_to_date("2.0.93");
    let available = UpdateCheckResult::update_available("2.1.0", RELEASES);
    let bad = UpdateCheckResult::failed(UpdateFailure::BadResponse);
    let cases: &[(&str, &UpdateCheckResult)] = &[
        // draft/prerelease are found before the bad name is reached; html_url (8 chars) is longer than the name.
        (
            r#"{"\ud800":1,"tag_name":"v2.1.0","draft":false,"prerelease":false}"#,
            &available,
        ),
        (
            r#"{"tag_name":"v2.1.0","\ud800":1,"draft":false,"prerelease":false}"#,
            &available,
        ),
        // early exit: `draft` is found first, `prerelease` (10 chars) is longer than the name.
        (r#"{"\ud800":1,"tag_name":"v3.0.0","draft":true}"#, &up_to_date_running),
        (r#"{"tag_name":"v3.0.0","\ud800":1,"draft":true}"#, &up_to_date_running),
        (
            r#"{"tag_name":"v3.0.0","prerelease":true,"\ud800":1,"draft":false}"#,
            &up_to_date_running,
        ),
        (
            r#"{"\ud800":1,"draft":true,"prerelease":true,"tag_name":"v3.0.0"}"#,
            &up_to_date_running,
        ),
        // shares a prefix with the key but is shorter / the escape starts after the key
        (r#"{"a\ud800":1,"tag_name":"v2.1.0"}"#, &available),
        (r#"{"tag_name\ud800":1,"tag_name":"v2.1.0"}"#, &available),
        // `draft` (5 chars) is shorter than the 6-char name: unescaped while looking for it => throws
        (r#"{"\ud800":1,"tag_name":"v2.1.0"}"#, &bad),
        (r#"{"tag_name":"v2.1.0","\ud800":1}"#, &bad),
        (r#"{"\u0074ag_name":"v2.1.0","\ud800x":1}"#, &bad),
        // an escaped but valid name still matches
        (r#"{"\u0074ag_name":"v2.1.0","d\u0072aft":true}"#, &up_to_date_running),
        (
            r#"{"tag\u005fname":"v2.1.0","html\u005furl":"https://github.com/ConanKLOP2/PhotoReview-rs/releases/tag/v2.1.0"}"#,
            &UpdateCheckResult::update_available(
                "2.1.0",
                "https://github.com/ConanKLOP2/PhotoReview-rs/releases/tag/v2.1.0",
            ),
        ),
    ];
    let current = pr_core::updates::version::AppVersion::try_parse(Some("2.0.93")).expect("version");
    for (body, expected) in cases {
        assert_eq!(&&UpdateChecker::interpret(body, current), expected, "{body}");
    }
}

// C#: `Headers.RetryAfter` (typed, single value): trims SP/HT only, digits only (at most 10 characters, at most
// Int32.MaxValue), HTTP-dates per `HttpDateParser` (case-insensitive names, flexible whitespace, `GMT`/`UTC`/numeric
// zone up to +-14:00, two-digit years pivot at 2049, weekday must match), and only the FIRST raw value counts.
#[test]
fn retry_after_grammar_matches_dotnet() {
    let cases: &[(&[&str], bool)] = &[
        (&["00000000000000000030"], false),
        (&["0000000030"], true),
        (&["2147483647"], true),
        (&["2147483648"], false),
        (&["9999999999"], false),
        (&["00000000030"], false),
        (&[" 30"], true),
        (&["30 "], true),
        (&["\n30"], false),
        (&["30\r\n"], false),
        (&["\t30\t"], true),
        (&["\u{a0}30"], false),
        (&["abc", "30"], false),
        (&["30", "abc"], true),
        (&["", "30"], false),
        (&["Wed, 21 Oct 2015 07:8:00 GMT"], true),
        (&["Wed, 21 Oct 2015 07:28:5 GMT"], true),
        (&["Wed, 21 Oct 2015 7:28:0 GMT"], true),
        (&["Wed, 21 Oct 2015 07:28:00 UTC"], true),
        (&["Wednesday, 21-Oct-15 07:28:00 UTC"], true),
        (&["Wed, 21 Oct 2015 07:28:00 +00:00"], true),
        (&["Wed, 21 Oct 15 07:28:00 GMT"], true),
        (&["Wed,21 Oct 2015 07:28:00 GMT"], true),
        (&["Wednesday,21-Oct-15 07:28:00 GMT"], true),
        (&["Wed , 21 Oct 2015 07:28:00 GMT"], true),
        (&["wed, 21 Oct 2015 07:28:00 GMT"], true),
        (&["WED, 21 OCT 2015 07:28:00 GMT"], true),
        (&["Wed, 21 oct 2015 07:28:00 GMT"], true),
        (&["Wednesday, 21-oct-15 07:28:00 GMT"], true),
        (&["wednesday, 21-Oct-15 07:28:00 GMT"], true),
        (&["wed oct 21 07:28:00 2015"], true),
        (&["Wed, 21 Oct 2015 07:28:00 gmt"], false),
        (&["Wed, 21 Oct 2015\u{a0}07:28:00 GMT"], true),
        (&["Wed, 21 Oct 2015 07:28:00\u{a0}GMT"], true),
        (&["Wed, 21 Oct 2015 07:28:00 +9999"], false),
        (&["Wed, 21 Oct 2015 07:28:00 +2400"], false),
        (&["Wed, 21 Oct 2015 07:28:00 +0160"], false),
        (&["Wed, 21 Oct 2015 07:28:00 +1500"], false),
        (&["Wed, 21 Oct 2015 07:28:00 -9999"], false),
        (&["Wed, 21 Oct 2015 07:28:00 +1400"], true),
        (&["Wed, 21 Oct 2015 07:28:00 +1401"], false),
        (&["Wed, 21 Oct 2015 07:28:00 -14:00"], true),
        (&["Sunday, 21-Oct-35 07:28:00 GMT"], true),
        (&["Monday, 21-Oct-35 07:28:00 GMT"], false),
        (&["Thursday, 21-Oct-49 07:28:00 GMT"], true),
        (&["Saturday, 21-Oct-50 07:28:00 GMT"], true),
        (&["Friday, 21-Oct-50 07:28:00 GMT"], false),
        (&["Wednesday, 21-Oct-29 07:28:00 GMT"], false),
        (&["Sat, 29 Feb 2020 23:59:59 GMT"], true),
        (&["Sun, 29 Feb 2015 23:59:59 GMT"], false),
        (&["Wed, 21 Oct 2015 24:00:00 GMT"], false),
        (&["Wed, 21 Oct 2015 07:60:00 GMT"], false),
        (&["Wed, 21 Oct 2015 07:28:00 GMT\u{1f}"], false),
    ];
    for (values, well_formed) in cases {
        let headers: Vec<(&str, &str)> = values.iter().map(|v| ("Retry-After", *v)).collect();
        let expected = if *well_formed {
            UpdateFailure::RateLimited
        } else {
            UpdateFailure::BadResponse
        };
        assert_eq!(
            failure_of(&with_headers(403, &headers)),
            expected,
            "Retry-After {values:?}"
        );
    }
}

// ---- golden ---------------------------------------------------------------------------------------------------

/// DIVERGENCE (DV-22, P-60): the goldens come from the C# oracle, which follows `ConanKLOP2/PhotoReview`; this build
/// follows `ConanKLOP2/PhotoReview-rs`. Rewrites the C# project path (ASCII case-insensitive) in a golden input or
/// expected value. Everything else in a case stays byte-for-byte the oracle's.
fn divergence_adjusted_repo(s: &str) -> String {
    // The path also appears JSON-escaped (`ConanKLOP2\/PhotoReview`) in some goldens.
    const FORMS: [&str; 2] = ["ConanKLOP2/PhotoReview", r"ConanKLOP2\/PhotoReview"];
    let mut out = String::with_capacity(s.len() + 16);
    let mut i = 0;
    while i < s.len() {
        let hit = FORMS.iter().find(|f| {
            s.as_bytes()
                .get(i..i + f.len())
                .is_some_and(|w| w.eq_ignore_ascii_case(f.as_bytes()))
        });
        if let Some(f) = hit {
            out.push_str(&s[i..i + f.len()]);
            out.push_str("-rs");
            i += f.len();
        } else {
            let c = s[i..].chars().next().expect("char");
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

fn hex_decode(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn body_bytes(body: &Value) -> Vec<u8> {
    if let Some(hex) = body["raw_hex"].as_str() {
        return hex_decode(hex);
    }
    let mut text = divergence_adjusted_repo(body["text"].as_str().unwrap_or(""));
    if let Some(n) = body["nested_arrays"].as_u64() {
        let n = n as usize;
        text = format!(r#"{{"a":{}{},"tag_name":"v2.0.94"}}"#, "[".repeat(n), "]".repeat(n));
    }
    let mut bytes = text.into_bytes();
    if let Some(total) = body["padded_to"].as_u64() {
        bytes.resize(total as usize, b' ');
        return bytes;
    }
    if body["bom"].as_bool() == Some(true) {
        let mut with = vec![0xEF, 0xBB, 0xBF];
        with.extend(bytes);
        return with;
    }
    bytes
}

fn push_headers(out: &mut Vec<(String, String)>, obj: &Value) {
    let Some(map) = obj.as_object() else { return };
    for (name, value) in map {
        match value {
            Value::String(s) => out.push((name.clone(), s.clone())),
            Value::Array(items) => {
                for item in items {
                    out.push((name.clone(), item.as_str().expect("header string").to_owned()));
                }
            }
            other => panic!("unexpected header value {other}"),
        }
    }
}

fn http_error_of(name: &str) -> HttpRequestError {
    use HttpRequestError::*;
    match name {
        "Unknown" => Unknown,
        "NameResolutionError" => NameResolutionError,
        "ConnectionError" => ConnectionError,
        "SecureConnectionError" => SecureConnectionError,
        "HttpProtocolError" => HttpProtocolError,
        "ExtendedConnectNotSupported" => ExtendedConnectNotSupported,
        "VersionNegotiationError" => VersionNegotiationError,
        "UserAuthenticationError" => UserAuthenticationError,
        "ProxyTunnelError" => ProxyTunnelError,
        "InvalidResponse" => InvalidResponse,
        "ResponseEnded" => ResponseEnded,
        "ConfigurationLimitExceeded" => ConfigurationLimitExceeded,
        other => panic!("unknown HttpRequestError {other}"),
    }
}

fn failure_name(f: UpdateFailure) -> &'static str {
    match f {
        UpdateFailure::None => "None",
        UpdateFailure::Offline => "Offline",
        UpdateFailure::Timeout => "Timeout",
        UpdateFailure::RateLimited => "RateLimited",
        UpdateFailure::BadResponse => "BadResponse",
        UpdateFailure::InvalidVersion => "InvalidVersion",
    }
}

fn status_name(s: UpdateCheckStatus) -> &'static str {
    match s {
        UpdateCheckStatus::UpToDate => "UpToDate",
        UpdateCheckStatus::UpdateAvailable => "UpdateAvailable",
        UpdateCheckStatus::Failed => "Failed",
    }
}

// C#: UpdateCheckerTests / UpdateCheckerGapTests / UpdateCheckerBoundaryTests (golden `update-checker`)
#[test]
fn golden_update_checker_cases() {
    let g = golden::load("update-checker");
    assert_eq!(g.generator, "oracle-cs update-checker");
    assert_eq!(g.cases.len(), 126);
    for case in &g.cases {
        let id = golden::str_of(case, "id");
        let current = case["current"].as_str();
        let timeout = case["timeout_ms"].as_u64().map(Duration::from_millis);
        let transport = &case["transport"];
        let kind = golden::str_of(transport, "kind");
        let requested = Arc::new(AtomicBool::new(false));
        let flag = requested.clone();

        let fake: Arc<Fake> = match kind {
            "response" => {
                let status = transport["status"].as_u64().unwrap_or(200) as u16;
                let data = body_bytes(&transport["body"]);
                let mut headers = Vec::new();
                push_headers(&mut headers, &transport["headers"]);
                push_headers(&mut headers, &transport["raw_headers"]);
                if let Some(secs) = transport["retry_after_seconds"].as_i64() {
                    headers.push(("Retry-After".to_owned(), secs.to_string()));
                }
                let content_length = match transport["length_mode"].as_str().unwrap_or("buffered") {
                    "stream_unknown_length" => None,
                    "declared" => Some(transport["declared_length"].as_u64().expect("declared_length")),
                    _ => Some(data.len() as u64),
                };
                Fake::new(Box::new(move |_, _, _| {
                    Ok(response(
                        status,
                        headers.clone(),
                        content_length,
                        data.clone(),
                        flag.clone(),
                    ))
                }))
            }
            "throw_http_request_error" => {
                failing(HttpError::Request(http_error_of(golden::str_of(transport, "error"))))
            }
            "throw_http_request_exception_no_error" => failing(HttpError::Request(HttpRequestError::Unknown)),
            "throw_task_canceled" => failing(HttpError::Canceled),
            "throw_invalid_operation" => failing(HttpError::Other("invalid operation".into())),
            "hang" => hang(),
            "cancel_by_caller" => Fake::new(Box::new(|_, _, cancel| {
                loop {
                    if cancel.is_canceled() {
                        return Err(HttpError::Canceled);
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            })),
            other => panic!("{id}: unknown transport kind {other}"),
        };

        let cancel = CancelToken::new();
        let outcome = if kind == "cancel_by_caller" {
            let canceler = cancel.clone();
            let handle = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(10));
                canceler.cancel();
            });
            let r = check_with(&fake, current, timeout, &cancel);
            handle.join().expect("join");
            r
        } else {
            check_with(&fake, current, timeout, &cancel)
        };

        let expected = &case["result"];
        if expected["threw"].is_string() {
            assert_eq!(outcome, Err(Canceled), "{id}: expected cancellation");
        } else {
            let r = outcome.unwrap_or_else(|_| panic!("{id}: unexpected cancellation"));
            assert_eq!(
                status_name(r.status()),
                expected["status"].as_str().expect("status"),
                "{id}: status"
            );
            assert_eq!(
                r.latest_version(),
                expected["latest_version"].as_str(),
                "{id}: latest_version"
            );
            assert_eq!(
                r.download_url(),
                expected["download_url"]
                    .as_str()
                    .map(divergence_adjusted_repo)
                    .as_deref(),
                "{id}: download_url"
            );
            assert_eq!(
                failure_name(r.failure()),
                expected["failure"].as_str().expect("failure"),
                "{id}: failure"
            );
        }

        assert_eq!(
            fake.request_count() as u64,
            golden::i64_of(case, "request_count") as u64,
            "{id}: request_count"
        );
        if let Some(expected_request) = case["request"].as_object() {
            if golden::i64_of(case, "request_count") > 0 {
                let requests = fake.requests.lock().expect("lock");
                let req = &requests[0];
                assert_eq!(
                    req.url,
                    divergence_adjusted_repo(expected_request["uri"].as_str().expect("uri")),
                    "{id}: uri"
                );
                assert_eq!(
                    req.user_agent,
                    expected_request["user_agent"].as_str().expect("ua"),
                    "{id}: ua"
                );
                assert_eq!(
                    req.accept,
                    expected_request["accept"].as_str().expect("accept"),
                    "{id}: accept"
                );
                assert_eq!(expected_request["method"].as_str(), Some("GET"), "{id}");
                assert_eq!(expected_request["has_content"].as_bool(), Some(false), "{id}");
                assert_eq!(expected_request["query"].as_str(), Some(""), "{id}");
            }
        }
        if let Some(was_requested) = case["body_was_requested"].as_bool() {
            assert_eq!(
                requested.load(Ordering::SeqCst),
                was_requested,
                "{id}: body_was_requested"
            );
        }
        if let Some(max) = case["max_body_bytes"].as_u64() {
            assert_eq!(max as usize, MAX_BODY_BYTES, "{id}");
        }
    }
}

// ---- W2-18 repair: mutation-killing tests for is_rate_limited / is_leap -------------------------------------------

fn limited(status: u16, headers: &[(&str, &str)]) -> bool {
    let headers = headers.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect();
    let r = response(status, headers, Some(0), Vec::new(), Arc::new(AtomicBool::new(false)));
    UpdateChecker::is_rate_limited(&r)
}

fn retry_after_ok(value: &str) -> bool {
    limited(403, &[("Retry-After", value)])
}

#[test]
fn remaining_zero_is_trimmed_of_unicode_whitespace() {
    for v in [
        "0",
        " 0",
        "0 ",
        "\t0\t",
        "\u{00A0}0",
        "0\u{00A0}",
        "\u{2003}0\u{2003}",
        "\u{3000}0",
        "\u{0085}0",
    ] {
        assert!(limited(403, &[("X-RateLimit-Remaining", v)]), "{v:?}");
    }
    for v in ["00", "-0", "", " ", "\u{00A0}", "1", "0.0", "+0", "0\u{200B}"] {
        assert!(!limited(403, &[("X-RateLimit-Remaining", v)]), "{v:?}");
    }
    // Repeated header: any value that is zero suffices.
    assert!(limited(
        403,
        &[("X-RateLimit-Remaining", "5"), ("X-RateLimit-Remaining", "\u{00A0}0")]
    ));
}

#[test]
fn rate_limit_headers_only_count_on_403() {
    // 429 is always limited, even without headers.
    assert!(limited(429, &[]));
    for status in [200u16, 201, 204, 302, 400, 401, 402, 404, 418, 500, 503] {
        assert!(!limited(status, &[]), "{status}");
        assert!(
            !limited(status, &[("X-RateLimit-Remaining", "0")]),
            "{status} remaining 0"
        );
        assert!(!limited(status, &[("Retry-After", "30")]), "{status} retry-after");
    }
    assert!(!limited(403, &[]));
    assert!(limited(403, &[("X-RateLimit-Remaining", "0")]));
    assert!(limited(403, &[("Retry-After", "30")]));
}

#[test]
fn retry_after_date_century_leap_year_rule() {
    // A leading digit would be parsed as delta-seconds, so every date carries a (correct) weekday.
    assert!(retry_after_ok("Tue, 29 Feb 2000 00:00:00 GMT")); // divisible by 400: leap
    assert!(retry_after_ok("Thu, 29 Feb 2024 00:00:00 GMT")); // divisible by 4: leap
    assert!(retry_after_ok("Tue, 29 Feb 2400 00:00:00 GMT"));
    assert!(retry_after_ok("Tue Feb 29 00:00:00 2000"));
    for wd in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
        // Century year that is not divisible by 400, and ordinary non-leap years: 29 Feb never exists.
        for y in [1900, 2100, 2200, 2300, 2023, 2001, 1999] {
            let d = format!("{wd}, 29 Feb {y} 00:00:00 GMT");
            assert!(!retry_after_ok(&d), "{d}");
        }
    }
    assert!(!retry_after_ok("Tue, 30 Feb 2000 00:00:00 GMT"));
    assert!(retry_after_ok("Wed, 28 Feb 1900 00:00:00 GMT"));
    assert!(!retry_after_ok("Wed, 29 Feb 2000 00:00:00 GMT")); // wrong weekday
    assert!(retry_after_ok("Thu, 01 Mar 1900 00:00:00 GMT"));
    // Two-digit year pivot: yy 00 -> 2000 (leap), yy 99 -> 1999.
    assert!(retry_after_ok("Tuesday, 29-Feb-00 00:00:00 GMT"));
    assert!(!retry_after_ok("Monday, 29-Feb-99 00:00:00 GMT"));
}
