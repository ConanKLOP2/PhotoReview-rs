//! Port of `PhotoReview.Core/Updates/UpdateChecker.cs` (C# baseline d92d6aae, 126 lines) and `IUpdateChecker.cs`
//! (10 lines): the GitHub "latest release" lookup behind the manual "Check for updates" button.
//!
//! HTTPS only, 10 s timeout, sends nothing about the user's photos or paths. The network sits behind
//! [`HttpTransport`] (R0003: blocking, no async runtime); tests use a fake transport. The result types
//! (`UpdateCheckResult`, `UpdateFailure`), `AppVersion` and `validate_update_url` are W1-10 (`updates::version`).
//!
//! Unit W2-18. Contract: `contracts/W2-18.md`.

use std::sync::Arc;
use std::time::Duration;

use crate::sync::{CancelToken, Canceled};
use crate::updates::version::{AppVersion, RELEASES_PAGE_URL, UpdateCheckResult, UpdateFailure, validate_update_url};

/// The endpoint (C# `LatestReleaseUrl`). Still the C# project's repository: see contracts/W2-18.md open question 2.
pub const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/ConanKLOP2/PhotoReview/releases/latest";

/// C# `DefaultTimeout`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// A hostile or broken endpoint must not make the check buffer an unbounded body (C# `MaxBodyBytes`).
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

/// `User-Agent` sent (C# `ProductInfoHeaderValue("PhotoReview", "update-check")`).
pub const USER_AGENT: &str = "PhotoReview/update-check";

/// `Accept` sent (C# `application/vnd.github+json`).
pub const ACCEPT: &str = "application/vnd.github+json";

/// Manual "Check for updates". The only code in the app that touches the network, and only when the user clicks.
/// C# `IUpdateChecker`.
pub trait UpdateCheck: Send + Sync {
    /// `current_version`: the running version, e.g. `"2.0.93+1a2b3c4d"` (metadata and a leading `v` are tolerated).
    /// `Err(Canceled)` only when `cancel` was canceled by the caller; every other problem is a `Failed` result.
    fn check(&self, current_version: Option<&str>, cancel: &CancelToken) -> Result<UpdateCheckResult, Canceled>;
}

/// The one request the checker makes: a `GET` of [`LATEST_RELEASE_URL`] with two headers and no body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub url: String,
    pub user_agent: String,
    pub accept: String,
}

/// C# `System.Net.Http.HttpRequestError` (.NET 8+): why a request failed before a response was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpRequestError {
    Unknown,
    NameResolutionError,
    ConnectionError,
    SecureConnectionError,
    HttpProtocolError,
    ExtendedConnectNotSupported,
    VersionNegotiationError,
    UserAuthenticationError,
    ProxyTunnelError,
    InvalidResponse,
    ResponseEnded,
    ConfigurationLimitExceeded,
}

/// A failed transport call or body read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// The `timeout` passed to the transport elapsed (C#: a cancellation NOT requested by the caller).
    Timeout,
    /// The caller's [`CancelToken`] was canceled.
    Canceled,
    /// `HttpRequestException` with its error code.
    Request(HttpRequestError),
    /// Any other failure of the HTTP stack (C#: e.g. `InvalidOperationException`): reported as `BadResponse`.
    Other(String),
}

/// The body of a response, read in chunks so the size cap can stop the read early.
pub trait HttpBody: Send {
    /// Reads up to `buf.len()` bytes; `Ok(0)` is the end of the body.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, HttpError>;
}

/// A response whose headers have been read (C# `HttpCompletionOption.ResponseHeadersRead`).
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Header name/value pairs as received; a name may repeat. Lookups ignore ASCII case.
    pub headers: Vec<(String, String)>,
    /// `Content-Length` when the response declares one.
    pub content_length: Option<u64>,
    pub body: Box<dyn HttpBody>,
}

impl std::fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("content_length", &self.content_length)
            .finish_non_exhaustive()
    }
}

impl HttpResponse {
    /// Every value of the header `name` (case-insensitive), in order.
    pub fn header_values(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    }
}

/// The network. A real implementation lives in the platform layer (see the contract: the crate to add) and must:
/// follow redirects like `HttpClient` (up to 50), honour the system proxy and certificate store, enforce `timeout`
/// over the WHOLE exchange (connect, headers and body reads) and stop promptly when `cancel` is canceled.
pub trait HttpTransport: Send + Sync {
    fn get(&self, request: &HttpRequest, timeout: Duration, cancel: &CancelToken) -> Result<HttpResponse, HttpError>;
}

/// GitHub "latest release" lookup. C# `UpdateChecker`.
pub struct UpdateChecker {
    transport: Arc<dyn HttpTransport>,
    timeout: Duration,
}

impl std::fmt::Debug for UpdateChecker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdateChecker")
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl UpdateChecker {
    /// `timeout == None` -> [`DEFAULT_TIMEOUT`].
    pub fn new(transport: Arc<dyn HttpTransport>, timeout: Option<Duration>) -> Self {
        Self {
            transport,
            timeout: timeout.unwrap_or(DEFAULT_TIMEOUT),
        }
    }

    /// C# `Classify(HttpRequestError)`: `NameResolutionError`, `ConnectionError`, `ProxyTunnelError` and `Unknown`
    /// -> `Offline`; `SecureConnectionError`, `HttpProtocolError`, `InvalidResponse`, `ResponseEnded`,
    /// `ConfigurationLimitExceeded`, `UserAuthenticationError`, `VersionNegotiationError`,
    /// `ExtendedConnectNotSupported` -> `BadResponse`.
    pub fn classify(error: HttpRequestError) -> UpdateFailure {
        match error {
            HttpRequestError::NameResolutionError
            | HttpRequestError::ConnectionError
            | HttpRequestError::ProxyTunnelError => UpdateFailure::Offline,
            HttpRequestError::SecureConnectionError
            | HttpRequestError::HttpProtocolError
            | HttpRequestError::InvalidResponse
            | HttpRequestError::ResponseEnded
            | HttpRequestError::ConfigurationLimitExceeded
            | HttpRequestError::UserAuthenticationError
            | HttpRequestError::VersionNegotiationError
            | HttpRequestError::ExtendedConnectNotSupported => UpdateFailure::BadResponse,
            // The historical behavior of C# (`_ => Offline`).
            HttpRequestError::Unknown => UpdateFailure::Offline,
        }
    }

    /// C# `IsRateLimited`: 429 always; 403 only with a well-formed `Retry-After` header or an `X-RateLimit-Remaining`
    /// value that equals `"0"` after trimming ASCII/Unicode whitespace.
    pub fn is_rate_limited(response: &HttpResponse) -> bool {
        if response.status == 429 {
            return true;
        }
        if response.status != 403 {
            return false;
        }
        if response
            .header_values("Retry-After")
            .into_iter()
            .any(retry_after_is_well_formed)
        {
            return true;
        }
        response
            .header_values("X-RateLimit-Remaining")
            .into_iter()
            .any(|v| v.trim() == "0")
    }

    /// C# `Interpret(body, current)`: parses the release JSON text and decides. Public for tests.
    pub fn interpret(body: &str, current: AppVersion) -> UpdateCheckResult {
        let bad = || UpdateCheckResult::failed(UpdateFailure::BadResponse);
        let Some(props) = scan_root_object(body) else {
            return bad();
        };
        // `TryGetProperty`: ordinal, case-sensitive, the LAST duplicate wins.
        let find = |name: &str| props.iter().rev().find(|p| p.name == name);
        let tag = match find("tag_name") {
            Some(Prop {
                value: Value::Str(Some(s)),
                ..
            }) => Some(s.as_str()),
            // Missing, wrong type, or a string holding a lone surrogate (reading it throws): BadResponse.
            _ => None,
        };
        let Some(latest) = AppVersion::try_parse(tag) else {
            return bad();
        };
        let is_true = |name: &str| matches!(find(name), Some(Prop { value: Value::True, .. }));
        let draft = is_true("draft");
        let pre = is_true("prerelease");
        if draft || pre || latest.is_prerelease {
            return UpdateCheckResult::up_to_date(&current.to_string());
        }
        if latest.cmp(&current) != std::cmp::Ordering::Greater {
            return UpdateCheckResult::up_to_date(&latest.to_string());
        }
        let url = match find("html_url") {
            Some(Prop {
                value: Value::Str(Some(s)),
                ..
            }) => Some(s.as_str()),
            Some(Prop {
                value: Value::Str(None),
                ..
            }) => return bad(),
            _ => None,
        };
        let url = validate_update_url(url).unwrap_or_else(|| RELEASES_PAGE_URL.to_owned());
        UpdateCheckResult::update_available(&latest.to_string(), &url)
    }
}

impl UpdateCheck for UpdateChecker {
    fn check(&self, current_version: Option<&str>, cancel: &CancelToken) -> Result<UpdateCheckResult, Canceled> {
        let Some(current) = AppVersion::try_parse(current_version) else {
            return Ok(UpdateCheckResult::failed(UpdateFailure::InvalidVersion));
        };
        let request = HttpRequest {
            url: LATEST_RELEASE_URL.to_owned(),
            user_agent: USER_AGENT.to_owned(),
            accept: ACCEPT.to_owned(),
        };
        let map_error = |e: HttpError| -> Result<UpdateCheckResult, Canceled> {
            match e {
                // Both are decided by the CALLER token (C#: `catch (OperationCanceledException) when (!cancel.Requested)`).
                HttpError::Canceled | HttpError::Timeout => {
                    if cancel.is_canceled() {
                        Err(Canceled)
                    } else {
                        Ok(UpdateCheckResult::failed(UpdateFailure::Timeout))
                    }
                }
                HttpError::Request(e) => Ok(UpdateCheckResult::failed(Self::classify(e))),
                HttpError::Other(_) => Ok(UpdateCheckResult::failed(UpdateFailure::BadResponse)),
            }
        };
        let mut response = match self.transport.get(&request, self.timeout, cancel) {
            Ok(r) => r,
            Err(e) => return map_error(e),
        };
        if Self::is_rate_limited(&response) {
            return Ok(UpdateCheckResult::failed(UpdateFailure::RateLimited));
        }
        if !(200..=299).contains(&response.status) {
            return Ok(UpdateCheckResult::failed(UpdateFailure::BadResponse));
        }
        if response.content_length.is_some_and(|n| n > MAX_BODY_BYTES as u64) {
            return Ok(UpdateCheckResult::failed(UpdateFailure::BadResponse));
        }
        let mut bytes: Vec<u8> = Vec::new();
        let mut chunk = vec![0u8; 16 * 1024];
        loop {
            match response.body.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if bytes.len() + n > MAX_BODY_BYTES {
                        return Ok(UpdateCheckResult::failed(UpdateFailure::BadResponse));
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                }
                Err(e) => return map_error(e),
            }
        }
        // Lossy UTF-8 like `Encoding.UTF8.GetString`; a BOM is kept (and then rejected by the JSON reader).
        let text = String::from_utf8_lossy(&bytes);
        Ok(Self::interpret(&text, current))
    }
}

// ---- Retry-After ------------------------------------------------------------------------------------------------

/// `RetryAfterHeaderValue` parse as .NET does it: delta-seconds that fit `i32`, or an HTTP-date from the formats of
/// `HttpDateParser`. Only the formats the golden covers are accepted (contract note 4).
fn retry_after_is_well_formed(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() {
        return false;
    }
    if v.bytes().all(|b| b.is_ascii_digit()) {
        return v.parse::<i32>().is_ok();
    }
    is_http_date(v)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WEEKDAYS_LONG: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

fn days_in_month(year: i64, month: usize) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

/// Day of week, 0 = Sunday (days-from-civil algorithm).
fn weekday_of(year: i64, month: usize, day: i64) -> usize {
    let m = month as i64;
    let y = if m <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468; // 1970-01-01 is day 0, a Thursday
    (days + 4).rem_euclid(7) as usize
}

fn parse_digits(s: &str, min: usize, max: usize) -> Option<i64> {
    if s.len() < min || s.len() > max || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn valid_time(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return false;
    }
    matches!(
        (
            parse_digits(parts[0], 1, 2),
            parse_digits(parts[1], 2, 2),
            parse_digits(parts[2], 2, 2)
        ),
        (Some(h), Some(m), Some(sec)) if h < 24 && m < 60 && sec < 60
    )
}

fn valid_zone(s: &str) -> bool {
    s == "GMT"
        || (s.len() == 5 && (s.starts_with('+') || s.starts_with('-')) && s[1..].bytes().all(|b| b.is_ascii_digit()))
}

fn month_index(s: &str) -> Option<usize> {
    MONTHS.iter().position(|m| *m == s).map(|i| i + 1)
}

fn date_consistent(weekday: usize, year: i64, month: usize, day: i64) -> bool {
    day >= 1 && day <= days_in_month(year, month) && weekday_of(year, month, day) == weekday
}

fn is_http_date(v: &str) -> bool {
    let tokens: Vec<&str> = v.split_ascii_whitespace().collect();
    let Some(first) = tokens.first() else {
        return false;
    };
    if let Some(name) = first.strip_suffix(',') {
        if let Some(wd) = WEEKDAYS_SHORT.iter().position(|w| *w == name) {
            // RFC 1123: `Wed, 21 Oct 2015 07:28:00 GMT` (zone optional, `+0000` accepted).
            if tokens.len() != 5 && tokens.len() != 6 {
                return false;
            }
            let (Some(day), Some(month), Some(year)) = (
                parse_digits(tokens[1], 1, 2),
                month_index(tokens[2]),
                parse_digits(tokens[3], 4, 4),
            ) else {
                return false;
            };
            return date_consistent(wd, year, month, day)
                && valid_time(tokens[4])
                && tokens.get(5).is_none_or(|z| valid_zone(z));
        }
        if let Some(wd) = WEEKDAYS_LONG.iter().position(|w| *w == name) {
            // RFC 850: `Wednesday, 21-Oct-15 07:28:00 GMT`.
            if tokens.len() != 3 && tokens.len() != 4 {
                return false;
            }
            let parts: Vec<&str> = tokens[1].split('-').collect();
            if parts.len() != 3 {
                return false;
            }
            let (Some(day), Some(month), Some(yy)) = (
                parse_digits(parts[0], 1, 2),
                month_index(parts[1]),
                parse_digits(parts[2], 2, 2),
            ) else {
                return false;
            };
            // .NET two-digit-year pivot (2029).
            let year = if yy <= 29 { 2000 + yy } else { 1900 + yy };
            return date_consistent(wd, year, month, day)
                && valid_time(tokens[2])
                && tokens.get(3).is_none_or(|z| valid_zone(z));
        }
        return false;
    }
    // asctime: `Wed Oct 21 07:28:00 2015`.
    if tokens.len() != 5 {
        return false;
    }
    let Some(wd) = WEEKDAYS_SHORT.iter().position(|w| w == first) else {
        return false;
    };
    let (Some(month), Some(day), Some(year)) = (
        month_index(tokens[1]),
        parse_digits(tokens[2], 1, 2),
        parse_digits(tokens[4], 4, 4),
    ) else {
        return false;
    };
    date_consistent(wd, year, month, day) && valid_time(tokens[3])
}

// ---- JSON reader (System.Text.Json `JsonDocument.Parse` rules) ---------------------------------------------------

/// `JsonReaderOptions.MaxDepth` default.
const MAX_JSON_DEPTH: usize = 64;

#[derive(Debug)]
enum Value {
    /// `None`: the string holds a lone surrogate escape (parses, but reading it throws).
    Str(Option<String>),
    True,
    Other,
}

#[derive(Debug)]
struct Prop {
    name: String,
    value: Value,
}

struct Scanner<'a> {
    b: &'a [u8],
    pos: usize,
    /// Set when a property name of the root object holds a lone surrogate.
    bad_name: bool,
}

impl Scanner<'_> {
    fn skip_ws(&mut self) {
        while matches!(self.b.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Option<()> {
        if self.b.get(self.pos) == Some(&c) {
            self.pos += 1;
            Some(())
        } else {
            None
        }
    }

    fn literal(&mut self, word: &[u8]) -> Option<()> {
        if self.b[self.pos..].starts_with(word) {
            self.pos += word.len();
            Some(())
        } else {
            None
        }
    }

    fn hex4(&mut self) -> Option<u16> {
        let digits = self.b.get(self.pos..self.pos + 4)?;
        let mut v = 0u16;
        for d in digits {
            v = v * 16 + u16::try_from((*d as char).to_digit(16)?).ok()?;
        }
        self.pos += 4;
        Some(v)
    }

    /// Reads a string (opening quote already consumed). `Some(None)` = valid JSON but holds a lone surrogate.
    fn string(&mut self) -> Option<Option<String>> {
        let mut units: Vec<u16> = Vec::new();
        loop {
            let c = *self.b.get(self.pos)?;
            match c {
                b'"' => {
                    self.pos += 1;
                    break;
                }
                0..=0x1F => return None,
                b'\\' => {
                    self.pos += 1;
                    let e = *self.b.get(self.pos)?;
                    self.pos += 1;
                    let unit = match e {
                        b'"' => u16::from(b'"'),
                        b'\\' => u16::from(b'\\'),
                        b'/' => u16::from(b'/'),
                        b'b' => 8,
                        b'f' => 12,
                        b'n' => 10,
                        b'r' => 13,
                        b't' => 9,
                        b'u' => self.hex4()?,
                        _ => return None,
                    };
                    units.push(unit);
                }
                _ => {
                    // The text is valid UTF-8 (it came from a `&str`): decode one scalar.
                    let end = (self.pos + 4).min(self.b.len());
                    let chunk = &self.b[self.pos..end];
                    let valid = match std::str::from_utf8(chunk) {
                        Ok(s) => s,
                        Err(e) => std::str::from_utf8(&chunk[..e.valid_up_to()]).ok()?,
                    };
                    let ch = valid.chars().next()?;
                    let mut buf = [0u16; 2];
                    units.extend_from_slice(ch.encode_utf16(&mut buf));
                    self.pos += ch.len_utf8();
                }
            }
        }
        Some(String::from_utf16(&units).ok())
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.b.get(self.pos), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&mut self) -> Option<()> {
        let _ = self.eat(b'-');
        match self.b.get(self.pos)? {
            b'0' => self.pos += 1,
            b'1'..=b'9' => {
                self.digits();
            }
            _ => return None,
        }
        if self.eat(b'.').is_some() && self.digits() == 0 {
            return None;
        }
        if matches!(self.b.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.b.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return None;
            }
        }
        Some(())
    }

    /// Reads one value found inside a container of depth `depth`.
    fn value(&mut self, depth: usize) -> Option<Value> {
        self.skip_ws();
        match *self.b.get(self.pos)? {
            b'"' => {
                self.pos += 1;
                Some(Value::Str(self.string()?))
            }
            b'{' => {
                if depth + 1 > MAX_JSON_DEPTH {
                    return None;
                }
                self.pos += 1;
                self.object(depth + 1, None)?;
                Some(Value::Other)
            }
            b'[' => {
                if depth + 1 > MAX_JSON_DEPTH {
                    return None;
                }
                self.pos += 1;
                self.array(depth + 1)?;
                Some(Value::Other)
            }
            b't' => self.literal(b"true").map(|()| Value::True),
            b'f' => self.literal(b"false").map(|()| Value::Other),
            b'n' => self.literal(b"null").map(|()| Value::Other),
            b'-' | b'0'..=b'9' => self.number().map(|()| Value::Other),
            _ => None,
        }
    }

    fn array(&mut self, depth: usize) -> Option<()> {
        self.skip_ws();
        if self.eat(b']').is_some() {
            return Some(());
        }
        loop {
            self.value(depth)?;
            self.skip_ws();
            if self.eat(b',').is_some() {
                continue;
            }
            return self.eat(b']');
        }
    }

    /// `props` is `Some` only for the root object, whose properties are recorded.
    fn object(&mut self, depth: usize, mut props: Option<&mut Vec<Prop>>) -> Option<()> {
        self.skip_ws();
        if self.eat(b'}').is_some() {
            return Some(());
        }
        loop {
            self.skip_ws();
            self.eat(b'"')?;
            let name = self.string()?;
            self.skip_ws();
            self.eat(b':')?;
            let value = self.value(depth)?;
            if let Some(list) = props.as_deref_mut() {
                match name {
                    Some(name) => list.push(Prop { name, value }),
                    None => self.bad_name = true,
                }
            }
            self.skip_ws();
            if self.eat(b',').is_some() {
                continue;
            }
            return self.eat(b'}');
        }
    }
}

/// Parses `text` as one JSON document whose root is an object. `None` = invalid JSON, root not an object, or a root
/// property name holding a lone surrogate (all `BadResponse`).
fn scan_root_object(text: &str) -> Option<Vec<Prop>> {
    let mut s = Scanner {
        b: text.as_bytes(),
        pos: 0,
        bad_name: false,
    };
    s.skip_ws();
    s.eat(b'{')?;
    let mut props = Vec::new();
    s.object(1, Some(&mut props))?;
    s.skip_ws();
    if s.pos != s.b.len() || s.bad_name {
        return None;
    }
    Some(props)
}
