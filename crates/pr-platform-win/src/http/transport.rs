//! Port of `src/PhotoReview.Core/Updates/UpdateChecker.cs` default HttpClient transport, 7eaee2a2.
//! Async WinHTTP behind the blocking trait. Event waits sample CancelToken (which has no
//! registration API) every 10 ms. Callback state and read storage survive until HANDLE_CLOSING.
//! Native .NET proxy/PAC/trust equivalence remains unverified.

use super::policy::{
    ProxySource, RedirectDecision, classify_winhttp_error, proxy_source, redirect_decision, remaining_timeout,
};
use pr_core::sync::CancelToken;
use pr_core::updates::checker::{HttpBody, HttpError, HttpRequest, HttpRequestError, HttpResponse, HttpTransport};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::GetLastError;
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::UI::Shell::UrlCombineW;
use windows::core::{PCWSTR, PWSTR, w};

/// Inert adapter: no handle, thread, proxy lookup or socket is created before `get`.
#[derive(Debug, Default)]
pub struct WinHttpTransport;
const READ_SIZE: usize = 16 * 1024;
const HEADER_CAP: u32 = 64 * 1024;
fn other(message: &str) -> HttpError {
    HttpError::Other(message.to_owned())
}
fn wide(value: &str) -> Result<Vec<u16>, HttpError> {
    if value.contains('\0') {
        return Err(other("embedded NUL in HTTP parameter"));
    }
    Ok(value.encode_utf16().chain(Some(0)).collect())
}
fn os_error(code: u32) -> HttpError {
    if code == ERROR_WINHTTP_TIMEOUT {
        HttpError::Timeout
    } else {
        HttpError::Request(classify_winhttp_error(code))
    }
}
fn win_error(error: windows::core::Error) -> HttpError {
    os_error(error.code().0 as u32 & 0xffff)
}

// Exclusive opaque handle ownership; WinHTTP permits moving handles between threads.
// Operations are serialized by the Exchange owner; the numeric value is never dereferenced.
struct Handle(usize);
impl Handle {
    fn new(raw: *mut c_void) -> Result<Self, HttpError> {
        if raw.is_null() {
            // SAFETY: no preconditions, immediately after failed WinHTTP creation.
            Err(os_error(unsafe { GetLastError() }.0))
        } else {
            Ok(Self(raw as usize))
        }
    }
    fn raw(&self) -> *mut c_void {
        self.0 as *mut c_void
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: unique owner, closes the live handle exactly once.
        let _ = unsafe { WinHttpCloseHandle(self.raw()) };
    }
}
enum Event {
    Sent,
    Headers,
    Read(Vec<u8>),
    Error(u32),
}
struct CallbackState {
    tx: mpsc::Sender<Event>,
    // Stable buffer owned by the callback's Arc through final close, even after a timed-out read.
    buffer: Mutex<Box<[u8; READ_SIZE]>>,
}
// SAFETY: WinHTTP supplies the registered Arc context and status-specific buffers. Its last
// HANDLE_CLOSING notification is serialized after all other callbacks for the same request.
unsafe extern "system" fn callback(_handle: *mut c_void, context: usize, status: u32, info: *mut c_void, length: u32) {
    if context == 0 {
        return;
    }
    // SAFETY: context is an Arc::into_raw reference; only HANDLE_CLOSING releases it.
    let state = unsafe { &*(context as *const CallbackState) };
    let event = match status {
        WINHTTP_CALLBACK_STATUS_SENDREQUEST_COMPLETE => Some(Event::Sent),
        WINHTTP_CALLBACK_STATUS_HEADERS_AVAILABLE => Some(Event::Headers),
        WINHTTP_CALLBACK_STATUS_READ_COMPLETE => {
            let bytes = if length == 0 {
                Vec::new()
            } else {
                // SAFETY: READ_COMPLETE supplies initialized bytes of the completed buffer; no
                // next read starts until this callback has copied and sent the event.
                unsafe { std::slice::from_raw_parts(info.cast::<u8>(), length as usize) }.to_vec()
            };
            Some(Event::Read(bytes))
        }
        WINHTTP_CALLBACK_STATUS_REQUEST_ERROR => {
            // SAFETY: REQUEST_ERROR supplies a valid WINHTTP_ASYNC_RESULT for this notification.
            Some(Event::Error(unsafe { (*info.cast::<WINHTTP_ASYNC_RESULT>()).dwError }))
        }
        WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING => {
            // SAFETY: final notification consumes exactly the one raw callback Arc reference.
            // The SDK guarantees HANDLE_CLOSING does not overlap other callbacks for this request.
            drop(unsafe { Arc::from_raw(context as *const CallbackState) });
            return;
        }
        _ => None,
    };
    if let Some(event) = event {
        let _ = state.tx.send(event);
    }
}
struct Deadline {
    started: Instant,
    total: Duration,
    cancel: CancelToken,
}
impl Deadline {
    fn remaining(&self) -> Result<Duration, HttpError> {
        if self.cancel.is_canceled() {
            return Err(HttpError::Canceled);
        }
        remaining_timeout(self.total, self.started.elapsed()).ok_or(HttpError::Timeout)
    }
    fn wait(&self, rx: &mpsc::Receiver<Event>) -> Result<Event, HttpError> {
        loop {
            let remaining = self.remaining()?;
            match rx.recv_timeout(remaining.min(Duration::from_millis(10))) {
                Ok(event) => {
                    self.remaining()?;
                    return match event {
                        Event::Error(code) => Err(os_error(code)),
                        _ => Ok(event),
                    };
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(other("WinHTTP callback disconnected")),
            }
        }
    }
}
struct Url {
    host: Vec<u16>,
    path: Vec<u16>,
    port: u16,
    https: bool,
    loopback: bool,
}
impl Url {
    fn parse(url: &str) -> Result<Self, HttpError> {
        let text = wide(url)?;
        let mut parts = URL_COMPONENTS {
            dwStructSize: std::mem::size_of::<URL_COMPONENTS>() as u32,
            dwHostNameLength: u32::MAX,
            dwUrlPathLength: u32::MAX,
            dwExtraInfoLength: u32::MAX,
            dwUserNameLength: u32::MAX,
            dwPasswordLength: u32::MAX,
            ..Default::default()
        };
        // SAFETY: initialized structure; returned ranges borrow text and are copied before it drops.
        unsafe { WinHttpCrackUrl(&text[..text.len() - 1], 0, &mut parts) }.map_err(win_error)?;
        if parts.dwUserNameLength != 0 || parts.dwPasswordLength != 0 {
            return Err(other("credentials in update URL"));
        }
        let component = |pointer: PWSTR, count: u32| -> String {
            if count == 0 {
                String::new()
            } else {
                // SAFETY: WinHttpCrackUrl validated each range inside the still-live URL buffer.
                String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer.0, count as usize) })
            }
        };
        let host = component(parts.lpszHostName, parts.dwHostNameLength);
        let loopback = host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback());
        let https = parts.nScheme == WINHTTP_INTERNET_SCHEME_HTTPS;
        if !https && parts.nScheme != WINHTTP_INTERNET_SCHEME_HTTP {
            return Err(other("unsupported update URL scheme"));
        }
        let mut path = component(parts.lpszUrlPath, parts.dwUrlPathLength);
        if path.is_empty() {
            path.push('/');
        }
        let extra = component(parts.lpszExtraInfo, parts.dwExtraInfoLength);
        path.push_str(extra.split('#').next().unwrap_or_default());
        Ok(Self {
            host: wide(&host)?,
            path: wide(&path)?,
            port: parts.nPort,
            https,
            loopback,
        })
    }
}
fn redirect_target(base: &str, location: &str) -> Option<String> {
    if location.trim().is_empty() {
        return None;
    }
    let base = wide(base).ok()?;
    let relative = wide(location.trim()).ok()?;
    let mut buffer = vec![0u16; 32 * 1024];
    let mut size = buffer.len() as u32;
    // SAFETY: terminated inputs, writable output of size UTF-16 elements.
    unsafe {
        UrlCombineW(
            PCWSTR(base.as_ptr()),
            PCWSTR(relative.as_ptr()),
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut size,
            0,
        )
    }
    .ok()?;
    let target = String::from_utf16(&buffer[..size as usize]).ok()?;
    // Validate downgrades too, without opening them: H1 returns the original response.
    Url::parse(&target).ok().map(|_| target)
}
// .NET NO_PROXY syntax is not WinINet syntax: exact names or leading-dot subdomains,
// comma-separated, without wildcard expansion. See HttpClient.DefaultProxy documentation.
fn bypass_environment_proxy(host: &str, list: &str) -> bool {
    let host = host.to_ascii_lowercase();
    list.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .any(|entry| {
            let entry = entry.to_ascii_lowercase();
            if entry.starts_with('.') {
                host.len() > entry.len() && host.ends_with(&entry)
            } else {
                host == entry
            }
        })
}
fn environment_proxy(value: &str) -> Result<String, HttpError> {
    // WinHTTP named proxy syntax is not a credential-bearing URI. Fail unsupported settings
    // explicitly rather than bypassing the proxy. Authenticated/TLS proxies remain a parity gap.
    let authority = value
        .trim()
        .strip_prefix("http://")
        .unwrap_or(value.trim())
        .trim_end_matches('/');
    if authority.is_empty()
        || authority.contains(['/', '?', '#', '@', ';', '=', '\\'])
        || authority.chars().any(char::is_whitespace)
    {
        return Err(other("unsupported environment proxy URI"));
    }
    Ok(authority.to_owned())
}

struct Exchange {
    // Request closes before parents; the final close callback independently owns read storage.
    request: Handle,
    _connection: Handle,
    _session: Arc<Handle>,
    state: Arc<CallbackState>,
    rx: mpsc::Receiver<Event>,
    deadline: Arc<Deadline>,
}
// One cookie container per exchange and proxy configuration, retained over redirects. A new
// manual check starts with an empty pool, like the C# using(new HttpClient()) scope.
type SessionPool = BTreeMap<(u32, Option<Vec<u16>>), Arc<Handle>>;
impl Exchange {
    fn open(
        url: &Url,
        input: &HttpRequest,
        deadline: Arc<Deadline>,
        sessions: &mut SessionPool,
    ) -> Result<Self, HttpError> {
        let remaining = deadline.remaining()?;
        let agent = wide(&input.user_agent)?;
        if input.accept.contains(['\r', '\n']) || input.user_agent.contains(['\r', '\n']) {
            return Err(other("invalid HTTP header"));
        }
        // Numeric loopback is direct: tests never consult a proxy/PAC. AUTOMATIC_PROXY uses
        // current-user settings (PAC/WPAD included), rather than machine DEFAULT_PROXY.
        let setting = |name| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let https = setting("HTTPS_PROXY");
        let all = setting("ALL_PROXY");
        let proxy = match proxy_source(https.is_some(), all.is_some(), true) {
            ProxySource::HttpsEnvironment => https,
            ProxySource::AllEnvironment => all,
            _ => None,
        };
        let host = String::from_utf16_lossy(&url.host[..url.host.len() - 1]);
        let bypass = url.loopback
            || (proxy.is_some() && setting("NO_PROXY").is_some_and(|list| bypass_environment_proxy(&host, &list)));
        let proxy_wide = if bypass {
            None
        } else {
            proxy
                .as_deref()
                .map(environment_proxy)
                .transpose()?
                .as_deref()
                .map(wide)
                .transpose()?
        };
        let access = if bypass {
            WINHTTP_ACCESS_TYPE_NO_PROXY
        } else if proxy_wide.is_some() {
            WINHTTP_ACCESS_TYPE_NAMED_PROXY
        } else {
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY
        };
        let key = (access.0, proxy_wide.clone());
        let session = if let Some(session) = sessions.get(&key) {
            session.clone()
        } else {
            // SAFETY: terminated strings live throughout the call, which copies configuration.
            let session = Arc::new(Handle::new(unsafe {
                WinHttpOpen(
                    PCWSTR(agent.as_ptr()),
                    access,
                    proxy_wide.as_ref().map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr())),
                    PCWSTR::null(),
                    WINHTTP_FLAG_ASYNC,
                )
            })?);
            sessions.insert(key, session.clone());
            session
        };
        let milliseconds = remaining.as_millis().clamp(1, i32::MAX as u128) as i32;
        // SAFETY: live session, positive per-phase bounds supplement whole-exchange deadline.
        unsafe { WinHttpSetTimeouts(session.raw(), milliseconds, milliseconds, milliseconds, milliseconds) }
            .map_err(win_error)?;
        // SAFETY: live session and terminated copied host component.
        let connection = Handle::new(unsafe { WinHttpConnect(session.raw(), PCWSTR(url.host.as_ptr()), url.port, 0) })?;
        // SAFETY: live connection, terminated request strings, no referrer or extra accept types.
        let request = Handle::new(unsafe {
            WinHttpOpenRequest(
                connection.raw(),
                w!("GET"),
                PCWSTR(url.path.as_ptr()),
                w!("HTTP/1.1"),
                PCWSTR::null(),
                null(),
                if url.https {
                    WINHTTP_FLAG_SECURE
                } else {
                    WINHTTP_OPEN_REQUEST_FLAGS(0)
                },
            )
        })?;
        for (option, value) in [
            (WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_NEVER),
            (WINHTTP_OPTION_MAX_RESPONSE_HEADER_SIZE, HEADER_CAP),
        ] {
            // SAFETY: live request, option expects and copies one DWORD.
            unsafe { WinHttpSetOption(Some(request.raw()), option, Some(&value.to_ne_bytes())) }.map_err(win_error)?;
        }
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(CallbackState {
            tx,
            buffer: Mutex::new(Box::new([0; READ_SIZE])),
        });
        // SAFETY: SDK callback signature; callback never unwinds, context is initially absent.
        let previous = unsafe {
            WinHttpSetStatusCallback(
                request.raw(),
                Some(callback),
                WINHTTP_CALLBACK_FLAG_ALL_NOTIFICATIONS,
                0,
            )
        };
        if previous.is_some_and(|f| f as usize == usize::MAX) {
            return Err(other("WinHTTP callback registration failed"));
        }
        let context = Arc::into_raw(state.clone()) as usize;
        // SAFETY: copies pointer-sized context; callback owns this raw Arc through HANDLE_CLOSING.
        if let Err(error) = unsafe {
            WinHttpSetOption(
                Some(request.raw()),
                WINHTTP_OPTION_CONTEXT_VALUE,
                Some(&context.to_ne_bytes()),
            )
        } {
            // SAFETY: context installation failed, so ownership never passed to the callback.
            drop(unsafe { Arc::from_raw(context as *const CallbackState) });
            return Err(win_error(error));
        }
        let exchange = Self {
            request,
            _connection: connection,
            _session: session,
            state,
            rx,
            deadline,
        };
        exchange.deadline.remaining()?;
        let headers = wide(&format!("Accept: {}\r\n", input.accept))?;
        // SAFETY: live request; header slice is copied, no optional upload buffer; context stays live.
        unsafe {
            WinHttpSendRequest(
                exchange.request.raw(),
                Some(&headers[..headers.len() - 1]),
                None,
                0,
                0,
                context,
            )
        }
        .map_err(win_error)?;
        if !matches!(exchange.deadline.wait(&exchange.rx)?, Event::Sent) {
            return Err(other("unexpected send completion"));
        }
        // SAFETY: send completed, request live, reserved parameter null.
        unsafe { WinHttpReceiveResponse(exchange.request.raw(), null_mut()) }.map_err(win_error)?;
        if !matches!(exchange.deadline.wait(&exchange.rx)?, Event::Headers) {
            return Err(other("unexpected headers completion"));
        }
        Ok(exchange)
    }
    fn headers(&self) -> Result<(u16, Vec<(String, String)>), HttpError> {
        let mut bytes = 0u32;
        // SAFETY: headers complete, null buffer asks for required size.
        let result = unsafe {
            WinHttpQueryHeaders(
                self.request.raw(),
                WINHTTP_QUERY_RAW_HEADERS_CRLF,
                PCWSTR::null(),
                None,
                &mut bytes,
                null_mut(),
            )
        };
        if let Err(error) = result {
            if error.code().0 as u32 & 0xffff != 122 {
                return Err(win_error(error));
            }
        }
        if bytes > 2 * HEADER_CAP + 2 {
            return Err(HttpError::Request(HttpRequestError::ConfigurationLimitExceeded));
        }
        let mut raw = vec![0u16; bytes as usize / 2 + 1];
        // SAFETY: buffer contains at least bytes writable bytes, handle has completed headers.
        unsafe {
            WinHttpQueryHeaders(
                self.request.raw(),
                WINHTTP_QUERY_RAW_HEADERS_CRLF,
                PCWSTR::null(),
                Some(raw.as_mut_ptr().cast()),
                &mut bytes,
                null_mut(),
            )
        }
        .map_err(win_error)?;
        let text = String::from_utf16_lossy(&raw[..bytes as usize / 2]);
        let mut lines = text.trim_end_matches('\0').split("\r\n");
        let status = lines
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|status| status.parse().ok())
            .ok_or(HttpError::Request(HttpRequestError::InvalidResponse))?;
        let mut headers = Vec::new();
        for line in lines.filter(|line| !line.is_empty()) {
            let Some((name, value)) = line.split_once(':') else {
                return Err(HttpError::Request(HttpRequestError::InvalidResponse));
            };
            headers.push((name.to_owned(), value.trim().to_owned()));
        }
        Ok((status, headers))
    }
}
struct Body {
    exchange: Option<Exchange>,
}
impl HttpBody for Body {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, HttpError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let Some(exchange) = &self.exchange else {
            return Ok(0);
        };
        let result = (|| {
            exchange.deadline.remaining()?;
            let count = buf.len().min(READ_SIZE);
            let pointer = exchange
                .state
                .buffer
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_mut_ptr();
            // SAFETY: stable callback-owned storage survives until HANDLE_CLOSING even on timeout.
            // Reads never overlap; the callback copies the data before reporting completion.
            unsafe { WinHttpReadData(exchange.request.raw(), pointer.cast(), count as u32, null_mut()) }
                .map_err(win_error)?;
            match exchange.deadline.wait(&exchange.rx)? {
                Event::Read(bytes) if bytes.len() <= count => {
                    buf[..bytes.len()].copy_from_slice(&bytes);
                    Ok(bytes.len())
                }
                _ => Err(other("unexpected read completion")),
            }
        })();
        if !matches!(result, Ok(n) if n > 0) {
            self.exchange.take();
        }
        result
    }
}
impl HttpTransport for WinHttpTransport {
    fn get(&self, request: &HttpRequest, timeout: Duration, cancel: &CancelToken) -> Result<HttpResponse, HttpError> {
        let deadline = Arc::new(Deadline {
            started: Instant::now(),
            total: timeout,
            cancel: cancel.clone(),
        });
        deadline.remaining()?;
        let mut target = request.url.clone();
        let mut sessions = SessionPool::new();
        for followed in 0..=50 {
            let url = Url::parse(&target)?;
            if !url.https && !url.loopback {
                return Err(other("HTTPS required; HTTP allowed only for numeric loopback tests"));
            }
            let exchange = Exchange::open(&url, request, deadline.clone(), &mut sessions)?;
            let (status, headers) = exchange.headers()?;
            let next = headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("Location"))
                .and_then(|(_, location)| redirect_target(&target, location));
            let to_https = next
                .as_ref()
                .is_some_and(|next| next.to_ascii_lowercase().starts_with("https://"));
            if redirect_decision(status, url.https, to_https, next.is_some(), followed) == RedirectDecision::Follow {
                if let Some(next) = next {
                    target = next;
                    continue;
                }
            }
            let content_length = headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("Content-Length"))
                .and_then(|(_, value)| value.parse::<u64>().ok());
            return Ok(HttpResponse {
                status,
                headers,
                content_length,
                body: Box::new(Body {
                    exchange: Some(exchange),
                }),
            });
        }
        Err(other("redirect loop exceeded policy"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_proxy_leading_dot_entry_requires_a_longer_host() {
        // C#: default HttpClient proxy grammar seam; no direct C# unit-test twin.
        // Mutant http/transport.rs:231:28: replace > with >= in bypass_environment_proxy.
        assert!(!bypass_environment_proxy(".example.com", ".example.com"));
    }

    #[test]
    fn no_proxy_names_subdomains_and_wildcards_follow_documented_dotnet_grammar() {
        // New: HttpClient.DefaultProxy documented environment grammar; runtime probe pending.
        for (host, list, expected) in [
            ("example.com", "example.com", true),
            ("www.example.com", "example.com", false),
            ("www.example.com", ".example.com", true),
            ("example.com", ".example.com", false),
            ("badexample.com", ".example.com", false),
            ("www.example.com", "*.example.com", false),
            ("api.github.com", "*, invalid, API.GITHUB.COM ", true),
            ("api.github.com", "", false),
        ] {
            assert_eq!(bypass_environment_proxy(host, list), expected, "{host} {list}");
        }
    }
    #[test]
    fn proxy_authority_is_normalized_and_unsupported_credentials_fail_explicitly() {
        // New: adapter syntax seam, no C# twin and no environment mutations.
        assert_eq!(
            environment_proxy(" http://proxy.example:8080/ ").unwrap(),
            "proxy.example:8080"
        );
        assert_eq!(environment_proxy("proxy.example:8080").unwrap(), "proxy.example:8080");
        for value in [
            "",
            "http://user:secret@proxy.example",
            "https://proxy.example",
            "http://proxy/a",
            "a;b",
        ] {
            assert!(environment_proxy(value).is_err(), "{value}");
        }
    }
    #[test]
    fn redirect_target_validates_uri_and_combines_relative_paths() {
        // New: URI seam; .NET malformed-Location behavior still needs a loopback probe.
        let base = "https://example.com/a/b";
        assert_eq!(
            redirect_target(base, "../c?x=1"),
            Some("https://example.com/c?x=1".into())
        );
        assert_eq!(
            redirect_target(base, "http://example.com/down"),
            Some("http://example.com/down".into())
        );
        for target in ["", "file:///a", "https://user:secret@example.com", "\0"] {
            assert_eq!(redirect_target(base, target), None, "{target}");
        }
    }

    #[test]
    fn wide_appends_one_nul_terminator_and_encodes_utf16() {
        // New: adapter seam, no C# twin. Mutants: wide -> Ok(vec![]) / Ok(vec![0]), dropping the chain(Some(0)).
        assert_eq!(wide("").unwrap(), vec![0u16]);
        assert_eq!(wide("ab").unwrap(), vec![b'a' as u16, b'b' as u16, 0]);
        // A supplementary-plane char is a surrogate pair in UTF-16, not one unit.
        assert_eq!(wide("\u{1F600}").unwrap(), vec![0xD83D, 0xDE00, 0]);
    }

    #[test]
    fn wide_rejects_an_embedded_nul_anywhere_as_other() {
        // New: a NUL would truncate the parameter at the WinHTTP boundary (header/URL smuggling), so it fails closed.
        for value in ["\0", "a\0", "\0a", "a\0b"] {
            assert_eq!(
                wide(value),
                Err(HttpError::Other("embedded NUL in HTTP parameter".to_owned())),
                "{value:?}"
            );
        }
    }

    #[test]
    fn os_error_maps_only_the_winhttp_timeout_code_to_timeout() {
        // New: ERROR_WINHTTP_TIMEOUT = 12002. Mutants: `==` -> `!=`, the Timeout arm swapped with the Request arm.
        assert_eq!(os_error(12002), HttpError::Timeout);
        assert_eq!(
            os_error(12007),
            HttpError::Request(HttpRequestError::NameResolutionError)
        );
        assert_eq!(os_error(12029), HttpError::Request(HttpRequestError::ConnectionError));
        assert_eq!(os_error(12001), HttpError::Request(HttpRequestError::Unknown));
        assert_eq!(os_error(0), HttpError::Request(HttpRequestError::Unknown));
    }

    #[test]
    fn win_error_keeps_only_the_low_16_bits_of_the_hresult() {
        // New: HRESULT_FROM_WIN32(12002) = 0x80072EE2; the facility/severity bits must not reach the classifier.
        let timeout = windows::core::Error::from_hresult(windows::core::HRESULT(0x8007_2EE2_u32 as i32));
        assert_eq!(win_error(timeout), HttpError::Timeout);
        let dns = windows::core::Error::from_hresult(windows::core::HRESULT(0x8007_2EE7_u32 as i32));
        assert_eq!(
            win_error(dns),
            HttpError::Request(HttpRequestError::NameResolutionError)
        );
    }
}
