#![cfg(windows)]
//! Q8 (X-http-transport): the environment proxy selection of `WinHttpTransport` observed on the wire. A loopback
//! "proxy" (a plain bounded TCP server started by the test) records what WinHTTP sends to it; no Internet, no
//! DNS, no PAC, no trust or system proxy changes. `set_var` is unsafe in edition 2024, so every scenario re-runs
//! this test binary as a child process with the variables set (an ordinary `Command::env`), like
//! `pr-bench`'s `default_file_log` test. The child entry does nothing unless the parent set `Q8_PROXY_CHILD_URL`.
//! C#: `HttpClient.DefaultProxy` (`HttpEnvironmentProxy`: `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`) used by
//! `UpdateChecker`'s `new HttpClient()`.
#[allow(dead_code)] // the shared fixture also exposes helpers used by other binaries
#[path = "common/http_loopback_fixture.rs"]
mod http_loopback_fixture;
use http_loopback_fixture::{GUARD, Server};
use pr_core::sync::CancelToken;
use pr_core::updates::checker::{ACCEPT, HttpRequest, HttpTransport, USER_AGENT};
use pr_platform_win::http::WinHttpTransport;
use std::io::Write;
use std::process::Command;

const CHILD_URL: &str = "Q8_PROXY_CHILD_URL";
const BAD_GATEWAY: &[u8] = b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";

/// Child entry: one `get` of the URL in the environment, outcome printed as `CHILD_RESULT <text>`.
#[test]
fn proxy_env_child_entry() {
    let Ok(url) = std::env::var(CHILD_URL) else {
        return; // the normal run: only the parent scenarios use this entry
    };
    let request = HttpRequest {
        url,
        user_agent: USER_AGENT.into(),
        accept: ACCEPT.into(),
    };
    let text = match WinHttpTransport.get(&request, GUARD, &CancelToken::new()) {
        Ok(response) => format!("status {}", response.status),
        Err(error) => format!("{error:?}"),
    };
    println!("CHILD_RESULT {text}");
}

fn proxy() -> Server {
    Server::bytes(BAD_GATEWAY.to_vec())
}
fn proxy_uri(server: &Server) -> String {
    server.url().replace("/update", "")
}
/// Runs the child with exactly `env` (the three proxy variables cleared first); returns its `CHILD_RESULT` text.
fn child_get(url: &str, env: &[(&str, &str)]) -> String {
    let exe = std::env::current_exe().expect("current exe");
    let mut command = Command::new(exe);
    command
        .args(["--exact", "proxy_env_child_entry", "--nocapture", "--test-threads=1"])
        .env(CHILD_URL, url)
        .env_remove("HTTPS_PROXY")
        .env_remove("ALL_PROXY")
        .env_remove("NO_PROXY")
        .env_remove("HTTP_PROXY");
    for (name, value) in env {
        command.env(name, value);
    }
    let output = command.output().expect("spawn child");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(output.status.success(), "child failed: {stdout}");
    assert!(stdout.contains("running 1 test"), "child ran no test: {stdout}");
    stdout
        .lines()
        .find_map(|line| line.split_once("CHILD_RESULT ").map(|(_, rest)| rest))
        .unwrap_or_else(|| panic!("no CHILD_RESULT in {stdout}"))
        .to_owned()
}
/// Every request line the proxy received while the child ran (the child has exited, so the queue is complete).
fn received(server: &Server) -> Vec<String> {
    let mut lines = Vec::new();
    while let Ok(request) = server.requests.try_recv() {
        lines.push(request.lines().next().unwrap_or_default().to_owned());
    }
    lines
}

#[test]
fn https_proxy_variable_receives_a_connect_for_the_https_target() {
    // New (no C# test): HttpEnvironmentProxy sends https:// targets through HTTPS_PROXY.
    let server = proxy();
    let target = "https://update.example.test:8443/update";
    child_get(target, &[("HTTPS_PROXY", &proxy_uri(&server))]);
    assert_eq!(received(&server), ["CONNECT update.example.test:8443 HTTP/1.1"]);
}

#[test]
fn all_proxy_variable_is_used_when_https_proxy_is_absent() {
    let server = proxy();
    child_get(
        "https://update.example.test:8443/update",
        &[("ALL_PROXY", &proxy_uri(&server))],
    );
    assert_eq!(received(&server), ["CONNECT update.example.test:8443 HTTP/1.1"]);
}

#[test]
fn blank_https_proxy_falls_through_to_all_proxy() {
    let server = proxy();
    child_get(
        "https://update.example.test:8443/update",
        &[("HTTPS_PROXY", "   "), ("ALL_PROXY", &proxy_uri(&server))],
    );
    assert_eq!(received(&server), ["CONNECT update.example.test:8443 HTTP/1.1"]);
}

#[test]
fn https_proxy_wins_over_all_proxy() {
    let https = proxy();
    let all = proxy();
    child_get(
        "https://update.example.test:8443/update",
        &[("HTTPS_PROXY", &proxy_uri(&https)), ("ALL_PROXY", &proxy_uri(&all))],
    );
    assert_eq!(received(&https), ["CONNECT update.example.test:8443 HTTP/1.1"]);
    assert!(
        received(&all).is_empty(),
        "ALL_PROXY must not be used when HTTPS_PROXY is set"
    );
}

#[test]
fn no_proxy_exact_name_bypasses_the_proxy_and_connects_directly() {
    let server = proxy();
    let result = child_get(
        "https://update.example.test:8443/update",
        &[
            ("HTTPS_PROXY", &proxy_uri(&server)),
            ("NO_PROXY", "other.example, update.example.test"),
        ],
    );
    assert!(received(&server).is_empty(), "NO_PROXY host must not reach the proxy");
    // Direct: the name does not resolve (reserved .test), so the child fails by itself; never a proxy answer.
    assert!(
        result.starts_with("Request("),
        "direct attempt must fail on its own, got {result}"
    );
}

#[test]
fn no_proxy_that_names_another_host_still_uses_the_proxy() {
    let server = proxy();
    child_get(
        "https://update.example.test:8443/update",
        &[
            ("HTTPS_PROXY", &proxy_uri(&server)),
            ("NO_PROXY", "other.example,.other.test,example.test"),
        ],
    );
    assert_eq!(received(&server), ["CONNECT update.example.test:8443 HTTP/1.1"]);
}

#[test]
fn proxy_with_credentials_fails_explicitly_without_any_connection() {
    // Known parity gap (C# accepts user:pass@): the transport must fail closed, never bypass the proxy.
    let server = proxy();
    let uri = format!(
        "http://user:secret@{}",
        proxy_uri(&server).trim_start_matches("http://")
    );
    let result = child_get("https://update.example.test:8443/update", &[("HTTPS_PROXY", &uri)]);
    assert!(received(&server).is_empty());
    assert!(result.contains("unsupported environment proxy URI"), "{result}");
}

#[test]
fn proxy_server_helper_answers_on_the_wire() {
    // Guards the fixture used above: the fake proxy really reads one request and replies 502.
    let server = proxy();
    let mut stream = std::net::TcpStream::connect(proxy_uri(&server).trim_start_matches("http://")).expect("connect");
    stream.write_all(b"CONNECT a:1 HTTP/1.1\r\n\r\n").expect("write");
    let mut text = String::new();
    std::io::Read::read_to_string(&mut stream, &mut text).expect("read");
    assert!(text.starts_with("HTTP/1.1 502"), "{text}");
    assert_eq!(received(&server), ["CONNECT a:1 HTTP/1.1"]);
}

#[test]
fn no_proxy_leading_dot_entry_bypasses_subdomains_like_dotnet() {
    // .NET 10 oracle (Q8 probe, NO_PROXY=".example.test", target update.example.test): the proxy saw nothing.
    let server = proxy();
    let result = child_get(
        "https://update.example.test:8443/update",
        &[("HTTPS_PROXY", &proxy_uri(&server)), ("NO_PROXY", ".example.test")],
    );
    assert!(
        received(&server).is_empty(),
        "leading-dot NO_PROXY entry must bypass the proxy"
    );
    assert!(
        result.starts_with("Request("),
        "direct attempt must fail on its own, got {result}"
    );
}

#[test]
fn proxy_connect_refusal_surfaces_as_the_proxy_status_unlike_dotnet_proxy_tunnel_error() {
    // KNOWN DIFFERENCE (Q8 finding, no fix: see the evidence file). .NET 10 oracle: a proxy that answers the CONNECT
    // with 502 makes `HttpClient` throw `HttpRequestException(ProxyTunnelError)`, which `UpdateChecker.Classify` turns
    // into `Failed(Offline)`. WinHTTP hands the proxy's 502 to the transport as an ordinary response, so the checker
    // sees a non-2xx status and answers `Failed(BadResponse)`. This test pins today's behaviour so a change is noticed.
    let server = proxy();
    let result = child_get(
        "https://update.example.test:8443/update",
        &[("HTTPS_PROXY", &proxy_uri(&server))],
    );
    assert_eq!(result, "status 502");
}
