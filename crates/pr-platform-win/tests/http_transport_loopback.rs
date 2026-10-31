#![cfg(windows)]
//! W10-H2 native WinHTTP against bounded loopback only. TLS cases are explicitly unverified.
#[path = "common/http_loopback_fixture.rs"]
mod http_loopback_fixture;
use http_loopback_fixture::{GUARD, Server};
use pr_core::sync::CancelToken;
use pr_core::updates::checker::{
    ACCEPT, HttpError, HttpRequest, HttpRequestError, HttpResponse, HttpTransport, USER_AGENT,
};
use pr_platform_win::http::WinHttpTransport;
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::Duration;

fn request(url: String) -> HttpRequest {
    HttpRequest {
        url,
        user_agent: USER_AGENT.into(),
        accept: ACCEPT.into(),
    }
}
fn get(server: &Server) -> HttpResponse {
    WinHttpTransport
        .get(&request(server.url()), GUARD, &CancelToken::new())
        .unwrap()
}
fn body(mut response: HttpResponse) -> Result<Vec<u8>, HttpError> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let n = response.body.read(&mut buffer)?;
        if n == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
}

#[test]
fn get_headers_before_body_returns_streaming_response() {
    // C#: UpdateCheckerTests.Request_IsMinimalAndHttps (request analogue; HTTP loopback, streaming new).
    let (release, wait) = mpsc::channel();
    let server = Server::new(move |_, stream| {
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 4\r\nX-Repeat: a\r\nX-Repeat: b\r\n\r\n",
            )
            .unwrap();
        wait.recv_timeout(GUARD).unwrap();
        stream.write_all(b"body").unwrap();
    });
    let response = get(&server); // Must complete while body is still held behind the gate.
    assert_eq!(response.status, 200);
    assert_eq!(response.content_length, Some(4));
    assert_eq!(response.header_values("x-repeat"), ["a", "b"]);
    let sent = server.requests.recv_timeout(GUARD).unwrap();
    assert!(sent.starts_with("GET /update HTTP/1.1\r\n"), "{sent}");
    assert!(sent.contains(&format!("User-Agent: {USER_AGENT}\r\n")), "{sent}");
    assert!(sent.contains(&format!("Accept: {ACCEPT}\r\n")), "{sent}");
    assert!(!sent.to_ascii_lowercase().contains("authorization:"));
    assert!(!sent.to_ascii_lowercase().contains("content-length:"));
    release.send(()).unwrap();
    assert_eq!(body(response).unwrap(), b"body");
}

#[test]
fn read_chunked_body_to_eof_returns_decoded_bytes() {
    // C#: UpdateCheckerBoundaryTests.ExactlyMaxBytes_WithoutContentLength_IsAccepted (stream analogue).
    let server = Server::bytes(
        b"HTTP/1.1 200 OK\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"
            .to_vec(),
    );
    let response = get(&server);
    assert_eq!(response.content_length, None);
    assert_eq!(body(response).unwrap(), b"abcde");
}

#[test]
fn get_declared_oversized_body_returns_headers_without_body_read() {
    // C#: UpdateCheckerBoundaryTests.DeclaredLengthOverMax_IsRejected (transport header analogue).
    let (release, wait) = mpsc::channel();
    let server = Server::new(move |_, stream| {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 18446744073709551615\r\n\r\n")
            .unwrap();
        let _ = wait.recv_timeout(GUARD);
    });
    let response = get(&server);
    assert_eq!(response.content_length, Some(u64::MAX));
    drop(response);
    release.send(()).unwrap();
}

#[test]
fn get_fifty_redirects_then_fifty_first_returns_three_xx_response() {
    // New: no C# redirect-limit test; count requests, not merely final status.
    let server = Server::new(|index, stream| {
        write!(
            stream,
            "HTTP/1.1 302 Found\r\nConnection: close\r\nLocation: /hop/{}\r\nContent-Length: 0\r\n\r\n",
            index + 1
        )
        .unwrap();
    });
    let response = get(&server);
    assert_eq!(response.status, 302);
    assert_eq!(response.header_values("location"), ["/hop/51"]);
    for _ in 0..51 {
        server.requests.recv_timeout(GUARD).unwrap();
    }
    assert!(server.requests.try_recv().is_err());
}

#[test]
#[ignore = "native: HTTPS loopback certificate/trust rig unavailable; downgrade policy covered by http_policy"]
fn get_https_to_http_redirect_returns_three_xx_response() {
    // New: no C# downgrade test. Never silently count the pure policy as TLS transport proof.
    panic!("requires locally trusted HTTPS server; do not disable certificate validation");
}

#[test]
fn get_cross_origin_redirect_removes_authorization_header() {
    // New: request shape has no Authorization; ensure neither request invents one across origins.
    let target = Server::bytes(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".to_vec());
    let source = Server::bytes(
        format!(
            "HTTP/1.1 302 Found\r\nConnection: close\r\nLocation: {}\r\nContent-Length: 0\r\n\r\n",
            target.url()
        )
        .into_bytes(),
    );
    assert_eq!(get(&source).status, 200);
    for server in [&source, &target] {
        assert!(
            !server
                .requests
                .recv_timeout(GUARD)
                .unwrap()
                .to_ascii_lowercase()
                .contains("authorization:")
        );
    }
}

#[test]
fn read_deadline_across_redirects_and_body_returns_timeout() {
    // C#: UpdateCheckerBoundaryTests.CustomTimeout_IsHonoured (whole-exchange extension).
    let (release, wait) = mpsc::channel();
    let server = Server::new(move |index, stream| {
        if index == 0 {
            stream
                .write_all(b"HTTP/1.1 302 Found\r\nConnection: close\r\nLocation: /body\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        } else {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 1\r\n\r\n")
                .unwrap();
            let _ = wait.recv_timeout(GUARD);
        }
    });
    let response = WinHttpTransport
        .get(&request(server.url()), Duration::from_millis(500), &CancelToken::new())
        .unwrap();
    assert_eq!(body(response), Err(HttpError::Timeout));
    release.send(()).unwrap();
    server.requests.recv_timeout(GUARD).unwrap();
    server.requests.recv_timeout(GUARD).unwrap();
}

#[test]
fn get_caller_cancel_while_headers_pending_returns_canceled() {
    // C#: UpdateCheckerTests.CallerCancel_Throws (transport analogue).
    let (release, wait) = mpsc::channel();
    let server = Server::new(move |_, _| {
        let _ = wait.recv_timeout(GUARD);
    });
    let input = request(server.url());
    let cancel = CancelToken::new();
    let worker_cancel = cancel.clone();
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        tx.send(WinHttpTransport.get(&input, GUARD, &worker_cancel).map(|_| ()))
            .unwrap();
    });
    server.requests.recv_timeout(GUARD).unwrap();
    cancel.cancel();
    assert_eq!(rx.recv_timeout(GUARD).unwrap(), Err(HttpError::Canceled));
    release.send(()).unwrap();
    worker.join().unwrap();
}

#[test]
fn read_caller_cancel_while_body_pending_returns_canceled() {
    // C#: UpdateCheckerTests.CallerCancel_Throws (body analogue).
    let (release, wait) = mpsc::channel();
    let server = Server::new(move |_, stream| {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 1\r\n\r\n")
            .unwrap();
        let _ = wait.recv_timeout(GUARD);
    });
    let cancel = CancelToken::new();
    let response = WinHttpTransport.get(&request(server.url()), GUARD, &cancel).unwrap();
    let (started, ready) = mpsc::channel();
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        started.send(()).unwrap();
        tx.send(body(response)).unwrap();
    });
    ready.recv_timeout(GUARD).unwrap();
    cancel.cancel();
    assert_eq!(rx.recv_timeout(GUARD).unwrap(), Err(HttpError::Canceled));
    release.send(()).unwrap();
    worker.join().unwrap();
}

#[test]
fn drop_repeated_unread_responses_releases_request_handles() {
    // New: observable socket lifetime guard (does not prove exact OS handle counts).
    let (closed, rx) = mpsc::channel();
    let server = Server::new(move |_, stream| {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 1\r\n\r\n")
            .unwrap();
        let mut byte = [0];
        assert_eq!(stream.read(&mut byte).unwrap(), 0, "unread body drop closes socket");
        closed.send(()).unwrap();
    });
    for _ in 0..32 {
        drop(get(&server));
        rx.recv_timeout(GUARD).unwrap();
    }
}

#[test]
fn get_oversized_response_headers_returns_configuration_error() {
    // New: documented 64 KiB response-header limit.
    let server = Server::bytes(
        format!(
            "HTTP/1.1 200 OK\r\nConnection: close\r\nX-Huge: {}\r\nContent-Length: 0\r\n\r\n",
            "a".repeat(70 * 1024)
        )
        .into_bytes(),
    );
    assert!(matches!(
        WinHttpTransport.get(&request(server.url()), GUARD, &CancelToken::new()),
        Err(HttpError::Request(HttpRequestError::ConfigurationLimitExceeded))
    ));
}

#[test]
#[ignore = "native: attended proxy/PAC, local Windows trust and cancellation rig required"]
fn native_proxy_pac_trust_and_cancel_matches_windows_defaults() {
    panic!("native outcomes not recorded; requires an attended local network rig");
}

#[test]
fn truncated_chunked_response_is_rejected() {
    // New: malformed chunk stream is not silently accepted as EOF.
    let server =
        Server::bytes(b"HTTP/1.1 200 OK\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n8\r\nabc".to_vec());
    assert!(body(get(&server)).is_err());
}

#[test]
fn caller_cancel_precedes_zero_timeout_and_invalid_url() {
    // C#: UpdateCheckerTests.CallerCancel_Throws (transport precedence extension).
    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(matches!(
        WinHttpTransport.get(&request("bad url".into()), Duration::ZERO, &cancel),
        Err(HttpError::Canceled)
    ));
    assert!(matches!(
        WinHttpTransport.get(&request("bad url".into()), Duration::ZERO, &CancelToken::new()),
        Err(HttpError::Timeout)
    ));
}

// Route the checker's immutable public endpoint to the local test server, retaining the real
// request shape and real WinHTTP stream. Production endpoint ownership stays in pr-core.
struct LocalTransport(String);
impl HttpTransport for LocalTransport {
    fn get(&self, input: &HttpRequest, timeout: Duration, cancel: &CancelToken) -> Result<HttpResponse, HttpError> {
        assert_eq!(input.url, pr_core::updates::checker::LATEST_RELEASE_URL);
        assert_eq!(input.user_agent, USER_AGENT);
        assert_eq!(input.accept, ACCEPT);
        let mut local = input.clone();
        local.url = self.0.clone();
        WinHttpTransport.get(&local, timeout, cancel)
    }
}
fn capped_check(size: usize, chunked: bool) -> pr_core::updates::version::UpdateCheckResult {
    use pr_core::updates::checker::{UpdateCheck, UpdateChecker};
    let mut payload =
        br#"{"tag_name":"v2.0.94","html_url":"https://github.com/ConanKLOP2/PhotoReview-rs/releases/tag/v2.0.94"}"#
            .to_vec();
    payload.resize(size, b' ');
    let mut response = if chunked {
        format!("HTTP/1.1 200 OK\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n{size:x}\r\n").into_bytes()
    } else {
        format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {size}\r\n\r\n").into_bytes()
    };
    response.extend_from_slice(&payload);
    if chunked {
        response.extend_from_slice(b"\r\n0\r\n\r\n");
    }
    let server = Server::bytes(response);
    UpdateChecker::new(std::sync::Arc::new(LocalTransport(server.url())), Some(GUARD))
        .check(Some("2.0.93"), &CancelToken::new())
        .unwrap()
}
#[test]
fn exactly_max_bytes_with_content_length_is_accepted() {
    // C#: UpdateCheckerBoundaryTests.ExactlyMaxBytes_WithContentLength_IsAccepted.
    assert_eq!(
        capped_check(1024 * 1024, false).status(),
        pr_core::updates::version::UpdateCheckStatus::UpdateAvailable
    );
}
#[test]
fn exactly_max_bytes_without_content_length_is_accepted() {
    // C#: UpdateCheckerBoundaryTests.ExactlyMaxBytes_WithoutContentLength_IsAccepted.
    assert_eq!(
        capped_check(1024 * 1024, true).status(),
        pr_core::updates::version::UpdateCheckStatus::UpdateAvailable
    );
}
#[test]
fn one_byte_over_max_without_content_length_is_rejected() {
    // C#: UpdateCheckerBoundaryTests.OneByteOverMax_WithoutContentLength_IsRejected; UpdateCheckerTests.OversizedBody_ReturnsBadResponse.
    assert_eq!(
        capped_check(1024 * 1024 + 1, true).failure(),
        pr_core::updates::version::UpdateFailure::BadResponse
    );
}
#[test]
fn declared_length_over_max_is_rejected() {
    // C#: UpdateCheckerBoundaryTests.DeclaredLengthOverMax_IsRejected.
    use pr_core::updates::checker::{UpdateCheck, UpdateChecker};
    let (release, wait) = mpsc::channel();
    let server = Server::new(move |_, stream| {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 1048577\r\n\r\n")
            .unwrap();
        let _ = wait.recv_timeout(GUARD); // Body unavailable: checker must reject from headers.
    });
    let result = UpdateChecker::new(std::sync::Arc::new(LocalTransport(server.url())), Some(GUARD))
        .check(Some("2.0.93"), &CancelToken::new())
        .unwrap();
    assert_eq!(result.failure(), pr_core::updates::version::UpdateFailure::BadResponse);
    release.send(()).unwrap();
}
#[test]
fn slow_server_times_out() {
    // C#: UpdateCheckerTests.SlowServer_TimesOut; UpdateCheckerBoundaryTests.CustomTimeout_IsHonoured.
    let (release, wait) = mpsc::channel();
    let server = Server::new(move |_, _| {
        let _ = wait.recv_timeout(GUARD);
    });
    assert!(matches!(
        WinHttpTransport.get(&request(server.url()), Duration::from_millis(100), &CancelToken::new()),
        Err(HttpError::Timeout)
    ));
    release.send(()).unwrap();
}

#[test]
fn redirect_cookies_are_scoped_to_the_manual_exchange() {
    // New: C# creates one default HttpClient per check; redirects share its cookie container.
    let server = Server::new(|index, stream| {
        if index % 2 == 0 {
            stream.write_all(b"HTTP/1.1 302 Found\r\nConnection: close\r\nSet-Cookie: update=one; Path=/\r\nLocation: /cookie\r\nContent-Length: 0\r\n\r\n").unwrap();
        } else {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        }
    });
    for _ in 0..2 {
        assert_eq!(get(&server).status, 200);
        let first = server.requests.recv_timeout(GUARD).unwrap();
        let redirected = server.requests.recv_timeout(GUARD).unwrap();
        assert!(!first.to_ascii_lowercase().contains("cookie:"), "{first}");
        assert!(redirected.contains("update=one"), "{redirected}");
    }
}
