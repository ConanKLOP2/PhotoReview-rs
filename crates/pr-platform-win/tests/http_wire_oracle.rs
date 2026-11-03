#![cfg(windows)]
//! Q8 (X-http-transport): the request that `WinHttpTransport` puts on the wire, compared with what the .NET 10
//! `HttpClient` that `UpdateChecker.cs` uses sends for the same request (runtime oracle run on this machine,
//! 2026-11-04, probe in `docs/reviews/Q8-PARITY-CLOSE-2026-11-03.md`): `GET /x HTTP/1.1`, then exactly `Host`,
//! `User-Agent`, `Accept` (no `Accept-Encoding`, no `Cookie` on the first request) and a cookie
//! set by a redirect is returned on the next request. ONE benign difference is pinned on purpose: WinHTTP also sends
//! `Connection: Keep-Alive`, which HTTP/1.1 implies and .NET leaves out. Loopback only, no Internet.
#[allow(dead_code)] // the shared fixture also exposes helpers used by other binaries
#[path = "common/http_loopback_fixture.rs"]
mod http_loopback_fixture;
use http_loopback_fixture::{GUARD, Server};
use pr_core::sync::CancelToken;
use pr_core::updates::checker::{ACCEPT, HttpRequest, HttpTransport, USER_AGENT};
use pr_platform_win::http::WinHttpTransport;
use std::io::Write;

fn get(url: String) -> u16 {
    let request = HttpRequest {
        url,
        user_agent: USER_AGENT.into(),
        accept: ACCEPT.into(),
    };
    WinHttpTransport
        .get(&request, GUARD, &CancelToken::new())
        .expect("loopback get")
        .status
}
fn header_names(request: &str) -> Vec<&str> {
    request
        .split("\r\n")
        .skip(1)
        .filter_map(|line| line.split_once(':').map(|(name, _)| name))
        .collect()
}

#[test]
fn first_request_has_the_dotnet_headers_plus_only_connection_keep_alive() {
    // .NET oracle: "GET /x HTTP/1.1 | Host | User-Agent | Accept".
    let server = Server::bytes(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".to_vec());
    assert_eq!(get(server.url()), 200);
    let sent = server.requests.recv_timeout(GUARD).unwrap();
    assert!(sent.starts_with("GET /update HTTP/1.1\r\n"), "{sent}");
    let mut names = header_names(&sent);
    names.sort_unstable();
    assert_eq!(names, ["Accept", "Connection", "Host", "User-Agent"], "{sent}");
    assert!(sent.contains("Connection: Keep-Alive\r\n"), "{sent}");
    assert!(sent.contains(&format!("User-Agent: {USER_AGENT}\r\n")), "{sent}");
    assert!(sent.contains(&format!("Accept: {ACCEPT}\r\n")), "{sent}");
}

#[test]
fn cookie_from_a_redirect_is_sent_on_the_followed_request_only() {
    // .NET oracle: GET /start -> 302 + Set-Cookie: a=1; Location: /next; the second request carries "Cookie: a=1".
    let server = Server::new(|index, stream| {
        let reply: &[u8] = if index == 0 {
            b"HTTP/1.1 302 Found\r\nSet-Cookie: a=1; Path=/\r\nLocation: /next\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
        } else {
            b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
        };
        let _ = stream.write_all(reply);
    });
    assert_eq!(get(server.url()), 200);
    let first = server.requests.recv_timeout(GUARD).unwrap();
    let second = server.requests.recv_timeout(GUARD).unwrap();
    assert!(!first.contains("Cookie:"), "{first}");
    assert!(second.starts_with("GET /next HTTP/1.1\r\n"), "{second}");
    assert!(second.contains("Cookie: a=1\r\n"), "{second}");
}

#[test]
fn content_encoding_is_not_decoded_like_dotnet_without_automatic_decompression() {
    // .NET oracle: `Content-Encoding: gzip` with body "abc" and a default HttpClient reads "abc" (no automatic
    // decompression); WinHTTP must hand over the same raw bytes.
    let server = Server::bytes(
        b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nConnection: close\r\nContent-Length: 3\r\n\r\nabc".to_vec(),
    );
    let request = HttpRequest {
        url: server.url(),
        user_agent: USER_AGENT.into(),
        accept: ACCEPT.into(),
    };
    let mut response = WinHttpTransport.get(&request, GUARD, &CancelToken::new()).expect("get");
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 64];
    loop {
        let n = response.body.read(&mut buffer).expect("read");
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    assert_eq!(bytes, b"abc");
}
