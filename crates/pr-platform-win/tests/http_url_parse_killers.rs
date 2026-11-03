#![cfg(windows)]
//! Q8 (X-http-transport) mutation killers for `Url::parse` in `pr_platform_win::http::transport`: survivors of the first
//! run were `dwExtraInfoLength`/`dwUserNameLength`/`dwPasswordLength` deleted from the `URL_COMPONENTS` request,
//! `|| -> &&` in the credentials guard and `len - 1 -> len / 1` for the cracked slice. All are observable through
//! `WinHttpTransport::get` on loopback: credentials are refused before any socket, and a query string reaches the wire.
//! C#: `HttpClient` rejects nothing here (the checker only ever asks for the fixed https constant); the guard is a
//! Rust-only hardening (an update URL must never carry credentials).
#[allow(dead_code)] // the shared fixture also exposes helpers used by other binaries
#[path = "common/http_loopback_fixture.rs"]
mod http_loopback_fixture;
use http_loopback_fixture::{GUARD, Server};
use pr_core::sync::CancelToken;
use pr_core::updates::checker::{ACCEPT, HttpError, HttpRequest, HttpTransport, USER_AGENT};
use pr_platform_win::http::WinHttpTransport;

fn request(url: String) -> HttpRequest {
    HttpRequest {
        url,
        user_agent: USER_AGENT.into(),
        accept: ACCEPT.into(),
    }
}
fn get(url: String) -> Result<u16, HttpError> {
    WinHttpTransport
        .get(&request(url), GUARD, &CancelToken::new())
        .map(|r| r.status)
}
const OK: &[u8] = b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";

#[test]
fn user_name_or_password_alone_or_both_are_refused_before_any_socket() {
    // Port 1 is never opened: the refusal must come from the URL check, not from a failed connection.
    for url in [
        "http://user@127.0.0.1:1/update",
        "http://:secret@127.0.0.1:1/update",
        "http://user:secret@127.0.0.1:1/update",
        "https://user:secret@api.example.test/update",
    ] {
        assert_eq!(
            get(url.to_owned()),
            Err(HttpError::Other("credentials in update URL".into())),
            "{url}"
        );
    }
}

#[test]
fn query_string_reaches_the_request_line_and_the_fragment_does_not() {
    let server = Server::bytes(OK.to_vec());
    assert_eq!(get(format!("{}?a=b&c=d#frag", server.url())), Ok(200));
    let sent = server.requests.recv_timeout(GUARD).unwrap();
    assert!(sent.starts_with("GET /update?a=b&c=d HTTP/1.1\r\n"), "{sent}");
}

#[test]
fn url_without_a_path_requests_the_root_and_keeps_its_query() {
    let server = Server::bytes(OK.to_vec());
    let origin = server.url().replace("/update", "");
    assert_eq!(get(origin.clone()), Ok(200));
    assert!(
        server
            .requests
            .recv_timeout(GUARD)
            .unwrap()
            .starts_with("GET / HTTP/1.1\r\n")
    );
    assert_eq!(get(format!("{origin}?q=1")), Ok(200));
    assert!(
        server
            .requests
            .recv_timeout(GUARD)
            .unwrap()
            .starts_with("GET /?q=1 HTTP/1.1\r\n")
    );
}

#[test]
fn schemes_other_than_http_and_https_are_refused() {
    for url in [
        "ftp://127.0.0.1:1/update",
        "file:///C:/update",
        "ws://127.0.0.1:1/update",
    ] {
        assert!(get(url.to_owned()).is_err(), "{url}");
    }
}

#[test]
fn line_breaks_in_either_header_value_are_refused_before_any_socket() {
    // Kills `|| -> &&` in the header-injection guard of `Exchange::open`: each value alone must be enough.
    for (user_agent, accept) in [
        ("ok/1\r\nX-Injected: 1", ACCEPT),
        ("ok/1\nX-Injected: 1", ACCEPT),
        (USER_AGENT, "text/plain\r\nX-Injected: 1"),
        (USER_AGENT, "text/plain\nX-Injected: 1"),
    ] {
        let request = HttpRequest {
            url: "http://127.0.0.1:1/update".into(),
            user_agent: user_agent.into(),
            accept: accept.into(),
        };
        assert_eq!(
            WinHttpTransport
                .get(&request, GUARD, &CancelToken::new())
                .map(|r| r.status),
            Err(HttpError::Other("invalid HTTP header".into())),
            "{user_agent:?} / {accept:?}"
        );
    }
}
