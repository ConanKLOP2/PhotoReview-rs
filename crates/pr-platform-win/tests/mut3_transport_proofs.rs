#![cfg(windows)]
//! MUT3 (2026-11-06), task P2: run proofs for the `http/transport.rs` survivors N4 left open (174:53, 377:48, 443:18, 443:35, 501:39).
//! Bounded loopback server only; no Internet, proxy or trust change. Hand-written, no golden file, no sleep on the passing path.
#[path = "common/http_loopback_fixture.rs"]
mod http_loopback_fixture;
use http_loopback_fixture::Server;
use pr_core::sync::CancelToken;
use pr_core::updates::checker::{
    ACCEPT, HttpError, HttpRequest, HttpRequestError, HttpResponse, HttpTransport, USER_AGENT,
};
use pr_platform_win::http::WinHttpTransport;
use std::io::Write;
use std::time::Duration;

const LONG: Duration = Duration::from_secs(120);

fn get_url(url: String, timeout: Duration) -> Result<HttpResponse, HttpError> {
    WinHttpTransport.get(
        &HttpRequest {
            url,
            user_agent: USER_AGENT.into(),
            accept: ACCEPT.into(),
        },
        timeout,
        &CancelToken::new(),
    )
}

fn authority(server: &Server) -> String {
    let url = server.url();
    let rest = url.strip_prefix("http://").expect("http url");
    rest.split('/').next().expect("authority").to_owned()
}

// ------------------------------------------------------------------------------------------------ 174:53

/// transport.rs:174:53 `&text[..text.len() - 1]` -> `/ 1` (the slice then ends WITH the terminating NUL instead of before it).
///
/// `WinHttpCrackUrl` splits the URL into scheme, host, port, path and extra info; a NUL left inside the counted text could only
/// show up as a character at the END of the LAST component present, so the input classes are "which component is last":
/// * host and port only (no path),
/// * path (plain, trailing slash, long, percent-escaped),
/// * extra info (query: plain, empty `?`, after a path or directly after the authority; fragment).
///
/// Each class is run through `get` against the loopback server and the request line the server saw must be exactly the expected
/// target (`Url::parse` copies the components into `wide(..)` strings that reject an embedded NUL, so a stray NUL would be refused
/// or would reach the request line). The classes below pass on the original and under the mutant.
#[test]
fn every_last_component_class_of_a_url_reaches_the_server_without_a_stray_character() {
    let cases: [(&str, &str); 12] = [
        ("", "/"),
        ("/", "/"),
        ("/update", "/update"),
        ("/update/", "/update/"),
        ("/a/b/c.json", "/a/b/c.json"),
        ("/with%20escape", "/with%20escape"),
        ("/update?x=1", "/update?x=1"),
        ("/update?", "/update?"),
        ("?x=1", "/?x=1"),
        ("/update?x=1&y=2", "/update?x=1&y=2"),
        ("/update#frag", "/update"),
        ("/update?x=1#frag", "/update?x=1"),
    ];
    for (suffix, target) in cases {
        let server = Server::bytes(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".to_vec());
        let host = authority(&server);
        let response = get_url(format!("http://{host}{suffix}"), LONG).unwrap_or_else(|e| panic!("{suffix:?}: {e:?}"));
        assert_eq!(response.status, 200, "{suffix:?}");
        let seen = server
            .requests
            .recv_timeout(LONG)
            .expect("the request reached the server");
        let line = seen.lines().next().expect("request line");
        assert_eq!(line, format!("GET {target} HTTP/1.1"), "{suffix:?}");
        assert!(!seen.contains('\0'), "{suffix:?}: NUL in the request");
        let host_header = seen
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("host:"))
            .expect("Host header");
        assert_eq!(host_header.trim(), format!("Host: {host}"), "{suffix:?}");
    }
}

// ------------------------------------------------------------------------------------------------ 443:18, 443:35

/// The status line plus headers a response of this shape sends, in characters (the block WinHTTP measures, final CRLF included).
fn block_chars(response: &[u8]) -> usize {
    let text = std::str::from_utf8(response).expect("ascii");
    text.find("\r\n\r\n").expect("header end") + 4
}

/// Three shapes of an oversized header block: one long header, many 1 000 character headers, a long reason phrase.
fn shaped(shape: usize, pad: usize) -> Vec<u8> {
    match shape {
        0 => format!(
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\nX-Pad: {}\r\n\r\n",
            "a".repeat(pad)
        )
        .into_bytes(),
        1 => {
            let mut text = String::from("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n");
            let mut left = pad;
            let mut n = 0;
            while left > 0 {
                let chunk = left.min(1000);
                text.push_str(&format!("X-{n}: {}\r\n", "b".repeat(chunk)));
                left -= chunk;
                n += 1;
            }
            text.push_str("\r\n");
            text.into_bytes()
        }
        _ => format!(
            "HTTP/1.1 200 {}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            "c".repeat(pad)
        )
        .into_bytes(),
    }
}

/// The response of `shape` whose header block has exactly `total` characters, if there is one (the many-headers shape skips a few
/// sizes right where it starts another header).
fn with_chars(shape: usize, total: usize) -> Option<Vec<u8>> {
    let mut pad = total.saturating_sub(block_chars(&shaped(shape, 0)));
    for _ in 0..8 {
        let bytes = shaped(shape, pad);
        let chars = block_chars(&bytes);
        if chars == total {
            return Some(bytes);
        }
        pad = (pad + total).checked_sub(chars)?;
    }
    None
}

fn outcome(bytes: Vec<u8>) -> Result<(), HttpError> {
    let server = Server::bytes(bytes);
    get_url(server.url(), LONG).map(|response| assert_eq!(response.status, 200))
}

/// transport.rs:443:18 (`>` -> `==` / `>=`) and 443:35 (`+ 2` -> `- 2` / `* 2`) on `bytes > 2 * HEADER_CAP + 2` (131 074 UTF-16 bytes).
/// WinHTTP lets a header block through up to exactly the configured cap, 65 536 characters (131 072 bytes of raw text, 131 074 with the
/// terminating NUL the size query counts), and refuses 65 537. So the original check never fires on a real response, while `>=`, `==`
/// and `- 2` (bound 131 070) DO fire on the block of exactly 65 536 characters: the test that accepts that block kills them. For each of
/// three block shapes (one long header, many 1 000 character headers, a long reason phrase) the window around the limit is run (shape 0:
/// every size 64 900 ..= 65 700, 801 requests; the others 40 characters either side), then a ladder above it (66 000 .. 1 000 000 and
/// the multiples of 65 536): one threshold, the same for every shape, everything above refused (WinHTTP answers either with its header
/// size overflow, `ConfigurationLimitExceeded`, or, depending on how the bytes arrive, with an invalid-response error; both count as refused).
///
/// `+ 2` -> `* 2` (bound 262 144 bytes) stays RUN-SUPPORTED, not a closure under rule B: it differs from the original only for a block
/// above 65 536 characters that WinHTTP would still deliver, which the ladder shows it does not; the sizes between the ladder rungs
/// are not enumerated and the cap cannot be lowered without a production seam. See the evidence file.
#[test]
fn winhttp_refuses_every_header_block_the_checked_size_could_still_distinguish() {
    let mut thresholds = Vec::new();
    for shape in 0..3usize {
        let mut first_refused = None;
        let mut last_accepted = 0usize;
        // Shape 0 sweeps the whole window; the other shapes sweep 40 characters either side of shape 0's threshold.
        let window = match thresholds.first() {
            None => 64_900..=65_700usize,
            Some(&(_, first, _)) => first - 40..=first + 40,
        };
        for total in window {
            let Some(bytes) = with_chars(shape, total) else {
                continue;
            };
            let chars = total;
            match outcome(bytes) {
                Ok(()) => {
                    assert!(
                        first_refused.is_none(),
                        "shape {shape}: {chars} accepted again after a refusal"
                    );
                    last_accepted = chars;
                }
                Err(HttpError::Request(
                    HttpRequestError::ConfigurationLimitExceeded | HttpRequestError::InvalidResponse,
                )) => {
                    first_refused.get_or_insert(chars);
                }
                Err(other) => panic!("shape {shape}, {chars} characters: {other:?}"),
            }
        }
        let threshold = first_refused.unwrap_or_else(|| panic!("shape {shape}: nothing refused up to 65 700"));
        thresholds.push((shape, threshold, last_accepted));
        // The largest block WinHTTP lets through is exactly the configured cap of 65 536 characters (its raw text is 131 072 bytes, 131 074
        // with the terminating NUL), the first refused one has 65 537. A response of exactly the cap must be accepted: this is what kills
        // the mutants that lower the checked size (`>=`, `==`, `- 2`).
        assert_eq!(last_accepted, 65_536, "shape {shape}: largest accepted block");
        assert_eq!(threshold, 65_537, "shape {shape}: first refused block");
        // The ladder above the window.
        let mut ladder: Vec<usize> = vec![
            66_000, 70_000, 98_304, 100_000, 131_072, 131_074, 140_000, 196_608, 262_144, 524_288, 1_000_000,
        ];
        ladder.extend((2..=8).map(|k| k * 65_536));
        for total in ladder {
            let bytes =
                with_chars(shape, total).unwrap_or_else(|| panic!("shape {shape}: no block of {total} characters"));
            match outcome(bytes) {
                Err(HttpError::Request(
                    HttpRequestError::ConfigurationLimitExceeded | HttpRequestError::InvalidResponse,
                )) => {}
                other => panic!("shape {shape}, {total} characters: expected the limit error, got {other:?}"),
            }
        }
    }
    let first = thresholds[0].1;
    for (shape, threshold, _) in &thresholds {
        assert_eq!(*threshold, first, "shape {shape} is refused from a different size");
    }
}

// ------------------------------------------------------------------------------------------------ 501:39

/// transport.rs:501:39 `Event::Read(bytes) if bytes.len() <= count` -> `true`.
///
/// `count = min(buf.len(), READ_SIZE = 16 384)`, so the whole domain of `count` is 1..=16 384. The guard differs from `true` only
/// when WinHTTP reports MORE bytes than it was asked for. Run for EVERY `count` in 1..=16 384 (buffers of exactly that size) and for
/// the larger buffers 16 385, 20 000, 65 536 and 1 MiB, over one continuous unbounded body: every read returns at most
/// `min(buf.len(), 16 384)` bytes and at least one. 134 MB flow through the transport. RUN-SUPPORTED, not a closure: WinHTTP's
/// behaviour cannot be enumerated, only the `count` domain can.
#[test]
fn every_read_size_gets_at_most_that_many_bytes_back() {
    let server = Server::new(|_, stream| {
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
        let block = vec![0x41u8; 64 * 1024];
        while stream.write_all(&block).is_ok() {}
    });
    let mut response = get_url(server.url(), Duration::from_secs(300)).expect("headers");
    let mut buffer = vec![0u8; 1 << 20];
    let sizes = (1..=16_384usize).chain([16_385, 20_000, 65_536, 1 << 20]);
    let mut total = 0usize;
    for size in sizes {
        let n = response
            .body
            .read(&mut buffer[..size])
            .unwrap_or_else(|e| panic!("size {size}: {e:?}"));
        assert!(n >= 1, "size {size}: end of body from an unbounded stream");
        assert!(n <= size.min(16 * 1024), "size {size}: {n} bytes returned");
        total += n;
    }
    assert!(total > 0);
    drop(response);
}

// ------------------------------------------------------------------------------------------------ 377:48

/// transport.rs:377:48 `previous.is_some_and(|f| f as usize == usize::MAX)` -> `!= usize::MAX` on the result of
/// `WinHttpSetStatusCallback`.
///
/// The original fails only for the sentinel `WINHTTP_INVALID_STATUS_CALLBACK` (usize::MAX); the mutant fails for every REAL previous
/// callback. They agree when the previous callback is absent. This probe repeats the transport's own handle chain (async session,
/// connection, GET request) and asks `WinHttpSetStatusCallback` for the previous callback of the fresh request handle: it is none.
/// RUN-SUPPORTED, not a closure: the sentinel (a failed registration) cannot be provoked on a live handle, and the probe copies the
/// handle chain rather than running the production function.
#[test]
fn a_fresh_request_handle_has_no_previous_status_callback() {
    use std::ffi::c_void;
    use windows::Win32::Networking::WinHttp::*;
    use windows::core::{PCWSTR, w};
    // SAFETY: a status callback that ignores every argument and never dereferences them; it has no preconditions.
    unsafe extern "system" fn noop(_: *mut c_void, _: usize, _: u32, _: *mut c_void, _: u32) {}
    let server = Server::bytes(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".to_vec());
    let url = server.url();
    let port: u16 = url
        .strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.split('/').next())
        .and_then(|p| p.parse().ok())
        .expect("port");
    let host: Vec<u16> = "127.0.0.1".encode_utf16().chain(Some(0)).collect();
    let path: Vec<u16> = "/update".encode_utf16().chain(Some(0)).collect();
    let agent: Vec<u16> = "mut3".encode_utf16().chain(Some(0)).collect();
    // SAFETY: terminated strings outlive the calls; every handle is closed below in reverse order.
    unsafe {
        let session = WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_NO_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            WINHTTP_FLAG_ASYNC,
        );
        assert!(!session.is_null());
        let connection = WinHttpConnect(session, PCWSTR(host.as_ptr()), port, 0);
        assert!(!connection.is_null());
        let request = WinHttpOpenRequest(
            connection,
            w!("GET"),
            PCWSTR(path.as_ptr()),
            w!("HTTP/1.1"),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_OPEN_REQUEST_FLAGS(0),
        );
        assert!(!request.is_null());
        let previous = WinHttpSetStatusCallback(request, Some(noop), WINHTTP_CALLBACK_FLAG_ALL_NOTIFICATIONS, 0);
        assert!(
            previous.is_none(),
            "a fresh request handle must not carry a callback: {:?}",
            previous.map(|f| f as usize)
        );
        // Registering again returns the one just installed (a real callback, not the sentinel).
        let again = WinHttpSetStatusCallback(request, Some(noop), WINHTTP_CALLBACK_FLAG_ALL_NOTIFICATIONS, 0);
        assert_eq!(again.map(|f| f as usize), Some(noop as *const () as usize));
        assert_ne!(again.map(|f| f as usize), Some(usize::MAX));
        let _ = WinHttpCloseHandle(request);
        let _ = WinHttpCloseHandle(connection);
        let _ = WinHttpCloseHandle(session);
    }
}
