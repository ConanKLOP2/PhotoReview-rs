#![cfg(windows)]
//! W10-H2/H3 wire level: the real `UpdateChecker` over the real `WinHttpTransport`, routed to a bounded
//! loopback server. Pins the HTTP status -> `UpdateFailure` mapping end to end (DV-22: a repository without a
//! release answers 404 and must be a failure, never an update). No Internet, no proxy/TLS/trust changes.
#[allow(dead_code)] // the shared fixture also exposes request capture used by other binaries
#[path = "common/http_loopback_fixture.rs"]
mod http_loopback_fixture;
use http_loopback_fixture::{GUARD, Server};
use pr_core::sync::CancelToken;
use pr_core::updates::checker::{
    ACCEPT, HttpError, HttpRequest, HttpResponse, HttpTransport, LATEST_RELEASE_URL, USER_AGENT, UpdateCheck,
    UpdateChecker,
};
use pr_core::updates::version::{UpdateCheckResult, UpdateCheckStatus, UpdateFailure};
use pr_platform_win::http::WinHttpTransport;
use std::sync::Arc;
use std::time::Duration;

struct Local(String);
impl HttpTransport for Local {
    fn get(&self, input: &HttpRequest, timeout: Duration, cancel: &CancelToken) -> Result<HttpResponse, HttpError> {
        assert_eq!(input.url, LATEST_RELEASE_URL);
        assert_eq!(input.user_agent, USER_AGENT);
        assert_eq!(input.accept, ACCEPT);
        let mut local = input.clone();
        local.url = self.0.clone();
        WinHttpTransport.get(&local, timeout, cancel)
    }
}
fn check_url(url: String) -> UpdateCheckResult {
    UpdateChecker::new(Arc::new(Local(url)), Some(GUARD))
        .check(Some("2.0.93"), &CancelToken::new())
        .unwrap()
}
fn reply(head: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {head}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}
fn failure_of(head: &str, body: &str) -> UpdateFailure {
    let server = Server::bytes(reply(head, body));
    let result = check_url(server.url());
    assert_eq!(result.status(), UpdateCheckStatus::Failed, "{head}");
    result.failure()
}

#[test]
fn not_found_404_over_the_wire_is_bad_response_not_an_update() {
    // DV-22 / P-61: a repository with no release answers 404 on releases/latest.
    assert_eq!(
        failure_of("404 Not Found", r#"{"message":"Not Found"}"#),
        UpdateFailure::BadResponse
    );
}
#[test]
fn server_errors_and_plain_forbidden_over_the_wire_are_bad_response() {
    for head in [
        "500 Internal Server Error",
        "503 Service Unavailable",
        "403 Forbidden",
        "401 Unauthorized",
    ] {
        assert_eq!(failure_of(head, "{}"), UpdateFailure::BadResponse, "{head}");
    }
}
#[test]
fn rate_limit_statuses_and_headers_over_the_wire_are_rate_limited() {
    assert_eq!(failure_of("429 Too Many Requests", "{}"), UpdateFailure::RateLimited);
    let with_header = |name: &str, value: &str| {
        let body = "{}";
        let bytes = format!(
            "HTTP/1.1 403 Forbidden\r\nConnection: close\r\n{name}: {value}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes();
        let server = Server::bytes(bytes);
        check_url(server.url()).failure()
    };
    assert_eq!(with_header("X-RateLimit-Remaining", "0"), UpdateFailure::RateLimited);
    assert_eq!(with_header("Retry-After", "30"), UpdateFailure::RateLimited);
    assert_eq!(with_header("X-RateLimit-Remaining", "7"), UpdateFailure::BadResponse);
}
#[test]
fn newer_and_equal_release_over_the_wire_map_to_available_and_up_to_date() {
    let release = |tag: &str| {
        format!(
            r#"{{"tag_name":"{tag}","html_url":"https://github.com/ConanKLOP2/PhotoReview-rs/releases/tag/{tag}"}}"#
        )
    };
    let newer = Server::bytes(reply("200 OK", &release("v2.0.94")));
    let result = check_url(newer.url());
    assert_eq!(result.status(), UpdateCheckStatus::UpdateAvailable);
    assert_eq!(result.latest_version(), Some("2.0.94"));
    let same = Server::bytes(reply("200 OK", &release("v2.0.93")));
    assert_eq!(check_url(same.url()).status(), UpdateCheckStatus::UpToDate);
}
#[test]
fn non_json_success_body_over_the_wire_is_bad_response() {
    assert_eq!(
        failure_of("200 OK", "<html>captive portal</html>"),
        UpdateFailure::BadResponse
    );
}
#[test]
fn redirect_to_the_release_is_followed_over_the_wire() {
    let body =
        r#"{"tag_name":"v2.0.94","html_url":"https://github.com/ConanKLOP2/PhotoReview-rs/releases/tag/v2.0.94"}"#;
    let target = Server::bytes(reply("200 OK", body));
    let location = target.url();
    let source = Server::bytes(
        format!("HTTP/1.1 302 Found\r\nConnection: close\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n")
            .into_bytes(),
    );
    assert_eq!(check_url(source.url()).status(), UpdateCheckStatus::UpdateAvailable);
}
#[test]
fn connection_refused_over_the_wire_is_offline() {
    let free = {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.local_addr().unwrap()
    }; // dropped: nothing listens on this port any more.
    let result = check_url(format!("http://{free}/update"));
    assert_eq!(result.status(), UpdateCheckStatus::Failed);
    assert_eq!(result.failure(), UpdateFailure::Offline);
}
