#![cfg(windows)]
//! N4 (2026-11-05): killers and run-based proofs for `http/transport.rs` (24:29 `READ_SIZE`, 25:28 `HEADER_CAP`, 443:18, 443:22,
//! 443:35 the header-size check, 481:9 `Body::read`). Bounded loopback server only; no Internet, proxy or trust change.
#[path = "common/http_loopback_fixture.rs"]
mod http_loopback_fixture;
use http_loopback_fixture::{GUARD, Server};
use pr_core::sync::CancelToken;
use pr_core::updates::checker::{
    ACCEPT, HttpError, HttpRequest, HttpRequestError, HttpResponse, HttpTransport, USER_AGENT,
};
use pr_platform_win::http::WinHttpTransport;
use std::io::Write;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn get(server: &Server) -> Result<HttpResponse, HttpError> {
    WinHttpTransport.get(
        &HttpRequest {
            url: server.url(),
            user_agent: USER_AGENT.into(),
            accept: ACCEPT.into(),
        },
        GUARD,
        &CancelToken::new(),
    )
}

fn with_header_padding(pad: usize) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\nX-Pad: {}\r\n\r\n",
        "a".repeat(pad)
    )
    .into_bytes()
}

/// Runs `work` on a thread and fails after `seconds` (a mutant that loops forever fails instead of hanging the harness).
fn bounded<T: Send + 'static>(seconds: u64, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(work());
    });
    receiver
        .recv_timeout(Duration::from_secs(seconds))
        .unwrap_or_else(|_| panic!("did not finish within {seconds} s (an endless loop)"))
}

/// transport.rs:25:28 (`HEADER_CAP = 64 * 1024` -> `64 + 1024`) and 443:22 (`2 * HEADER_CAP` -> `2 + HEADER_CAP`): response headers of
/// 40 000 and 60 000 characters are accepted (the mutants cap them near 1 000 and 32 000 characters).
#[test]
fn response_headers_of_tens_of_kilobytes_are_accepted() {
    for pad in [5_000usize, 40_000, 60_000] {
        let server = Server::bytes(with_header_padding(pad));
        let response = get(&server).unwrap_or_else(|e| panic!("pad {pad}: {e:?}"));
        assert_eq!(response.status, 200, "pad {pad}");
        assert_eq!(response.header_values("x-pad")[0].len(), pad, "pad {pad}");
    }
}

/// transport.rs:443:18 (`>` -> `==` / `>=`) and 443:35 (`+ 2` -> `- 2` / `* 2`) on `bytes > 2 * HEADER_CAP + 2`: WinHTTP itself refuses
/// a header block of about 65 300 characters or more, long before the check could fire (it needs 65 537). Run over every padding
/// length 65 000 ..= 65 600 (601 requests): the outcome is one threshold, `Ok` below it and the configuration-limit error from it,
/// the threshold lies below the smallest mutated cap (65 535 characters), so no mutated cap changes any outcome.
#[test]
fn the_header_limit_is_a_single_threshold_set_by_winhttp_below_the_checked_size() {
    let mut first_refused = None;
    for pad in 65_000..=65_600usize {
        let server = Server::bytes(with_header_padding(pad));
        match get(&server) {
            Ok(response) => {
                assert_eq!(response.status, 200, "pad {pad}");
                assert!(first_refused.is_none(), "pad {pad} is accepted again after a refusal");
            }
            Err(HttpError::Request(HttpRequestError::ConfigurationLimitExceeded)) => {
                first_refused.get_or_insert(pad);
            }
            Err(other) => panic!("pad {pad}: {other:?}"),
        }
    }
    let threshold = first_refused.expect("a 65 600 character header must be refused");
    // The accepted block (status line, other headers, the pad) is under 65 535 characters, i.e. under 131 070 UTF-16 bytes.
    assert!(
        threshold + 64 <= 65_535,
        "the refusal starts at pad {threshold}, which would be accepted by a cap of 131 070 bytes"
    );
}

/// transport.rs:24:29 (`READ_SIZE = 16 * 1024` -> `16 + 1024`): a 200 000 byte body arrives in about a dozen reads with a 64 KiB
/// buffer; with 1 040 byte chunks it needs about 190. The bound (60) is far from both.
#[test]
fn a_large_body_is_read_in_large_chunks() {
    let body = vec![0x5Au8; 200_000];
    let server = Server::new({
        let body = body.clone();
        move |_, stream| {
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            );
            let _ = stream.write_all(&body);
        }
    });
    let mut response = get(&server).expect("headers");
    let (total, reads) = bounded(30, move || {
        let mut buffer = vec![0u8; 65_536];
        let (mut total, mut reads) = (0usize, 0usize);
        loop {
            let n = response.body.read(&mut buffer).expect("read");
            if n == 0 {
                return (total, reads);
            }
            total += n;
            reads += 1;
            assert!(n <= 16 * 1024, "a read never exceeds the 16 KiB chunk");
        }
    });
    assert_eq!(total, body.len());
    assert!(reads <= 60, "{reads} reads for {total} bytes");
}

/// transport.rs:481:9 (`Body::read` -> `Ok(1)`): a three byte body is read to the end and the stream then reports 0, within a bound
/// of reads. The mutant never reports the end (and never fills the buffer), so the bound fails it.
#[test]
fn a_small_body_ends_with_a_zero_read() {
    let server = Server::bytes(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 3\r\n\r\nabc".to_vec());
    let mut response = get(&server).expect("headers");
    let sent = server
        .requests
        .recv_timeout(GUARD)
        .expect("the request reached the server");
    assert!(sent.starts_with("GET /update HTTP/1.1"), "{sent}");
    let bytes = bounded(20, move || {
        let mut out = Vec::new();
        let mut buffer = [0u8; 64];
        for _ in 0..1_000 {
            let n = response.body.read(&mut buffer).expect("read");
            if n == 0 {
                return out;
            }
            out.extend_from_slice(&buffer[..n]);
        }
        panic!("the body did not end within 1000 reads");
    });
    assert_eq!(bytes, b"abc");
}
