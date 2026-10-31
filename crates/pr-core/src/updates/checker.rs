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

/// The endpoint (C# `LatestReleaseUrl`). DIVERGENCE (DV-22, P-60): the Rust build's own repository
/// ([`crate::updates::UPDATE_REPO`]), not the C# project's `ConanKLOP2/PhotoReview` (contracts/W2-18.md open question 2).
pub const LATEST_RELEASE_URL: &str = concat!("https://api.github.com/repos/", update_repo!(), "/releases/latest");

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
        // .NET's typed `Headers.RetryAfter` is a single-value header: only the FIRST raw value decides
        // (`["abc", "30"]` is unparsable, `["30", "abc"]` is fine).
        if response
            .header_values("Retry-After")
            .first()
            .is_some_and(|v| retry_after_is_well_formed(v))
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
        // `TryGetProperty`: ordinal, case-sensitive, scanned from the LAST property backwards (the last duplicate
        // wins). `Err(LookupThrows)` is .NET's `InvalidOperationException` for a name holding a lone surrogate.
        let Ok(tag_prop) = find_property(&props, "tag_name") else {
            return bad();
        };
        let tag = match tag_prop {
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
        // C# evaluates both lookups unconditionally, so either may throw.
        let (Ok(draft_prop), Ok(pre_prop)) = (find_property(&props, "draft"), find_property(&props, "prerelease"))
        else {
            return bad();
        };
        let is_true = |p: Option<&Prop>| matches!(p, Some(Prop { value: Value::True, .. }));
        if is_true(draft_prop) || is_true(pre_prop) || latest.is_prerelease {
            return UpdateCheckResult::up_to_date(&current.to_string());
        }
        if latest.cmp(&current) != std::cmp::Ordering::Greater {
            return UpdateCheckResult::up_to_date(&latest.to_string());
        }
        let Ok(url_prop) = find_property(&props, "html_url") else {
            return bad();
        };
        let url = match url_prop {
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

/// `RetryConditionHeaderValue` parse as .NET does it (verified by a differential run against .NET 10): the value is
/// trimmed of SP/HT only; a value starting with an ASCII digit must be ONLY digits (at most 10 characters, at most
/// `i32::MAX`); anything else must be an HTTP-date in one of `HttpDateParser`'s formats (see [`is_http_date`]).
fn retry_after_is_well_formed(value: &str) -> bool {
    let v = value.trim_matches(|c| c == ' ' || c == '\t');
    let Some(first) = v.bytes().next() else {
        return false;
    };
    if first.is_ascii_digit() {
        return v.len() <= 10 && v.bytes().all(|b| b.is_ascii_digit()) && v.parse::<i32>().is_ok();
    }
    is_http_date(v)
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const WEEKDAYS_SHORT: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
const WEEKDAYS_LONG: [&str; 7] = [
    "sunday",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
];

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
    }
}

/// Day of week, 0 = Sunday (days-from-civil algorithm).
fn weekday_of(year: i64, month: i64, day: i64) -> usize {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468; // 1970-01-01 is day 0, a Thursday
    (days + 4).rem_euclid(7) as usize
}

/// One element of a `DateTime.TryParseExact` format string.
#[derive(Clone, Copy)]
enum DateTok {
    /// `ddd`
    WeekdayShort,
    /// `dddd`
    WeekdayLong,
    /// `d` (1 or 2 digits)
    Day,
    /// `MMM`
    Month,
    /// `yy` (exactly 2 digits, pivot 2049)
    Year2,
    /// `yyyy` (exactly 4 digits)
    Year4,
    /// `H`, `m`, `s` (1 or 2 digits each)
    Hour,
    Minute,
    Second,
    /// `zzz`: sign, 1-2 hour digits, optional `:`, 2 minute digits, at most +-14:00
    Zone,
    /// A space: whitespace is skipped before every element (`DateTimeStyles.AllowWhiteSpaces`)
    Space,
    /// A literal character
    Lit(char),
    /// `'GMT'`: `GMT` or `UTC`, case-sensitive
    Gmt,
}

use DateTok::{Day, Gmt, Hour, Lit, Minute, Month, Second, Space, WeekdayLong, WeekdayShort, Year2, Year4, Zone};

/// `HttpDateParser`'s formats as observed on .NET 10 (every `'GMT'` format also accepts `UTC`).
#[rustfmt::skip]
const DATE_FORMATS: &[&[DateTok]] = &[
    // ddd, d MMM yyyy H:m:s 'GMT'
    &[WeekdayShort, Lit(','), Space, Day, Space, Month, Space, Year4, Space, Hour, Lit(':'), Minute, Lit(':'), Second, Space, Gmt],
    // ddd, d MMM yyyy H:m:s
    &[WeekdayShort, Lit(','), Space, Day, Space, Month, Space, Year4, Space, Hour, Lit(':'), Minute, Lit(':'), Second],
    // d MMM yyyy H:m:s 'GMT'
    &[Day, Space, Month, Space, Year4, Space, Hour, Lit(':'), Minute, Lit(':'), Second, Space, Gmt],
    // d MMM yyyy H:m:s
    &[Day, Space, Month, Space, Year4, Space, Hour, Lit(':'), Minute, Lit(':'), Second],
    // ddd, d MMM yy H:m:s 'GMT'
    &[WeekdayShort, Lit(','), Space, Day, Space, Month, Space, Year2, Space, Hour, Lit(':'), Minute, Lit(':'), Second, Space, Gmt],
    // ddd, d MMM yy H:m:s
    &[WeekdayShort, Lit(','), Space, Day, Space, Month, Space, Year2, Space, Hour, Lit(':'), Minute, Lit(':'), Second],
    // d MMM yy H:m:s 'GMT'
    &[Day, Space, Month, Space, Year2, Space, Hour, Lit(':'), Minute, Lit(':'), Second, Space, Gmt],
    // d MMM yy H:m:s
    &[Day, Space, Month, Space, Year2, Space, Hour, Lit(':'), Minute, Lit(':'), Second],
    // dddd, d'-'MMM'-'yy H:m:s 'GMT'
    &[WeekdayLong, Lit(','), Space, Day, Lit('-'), Month, Lit('-'), Year2, Space, Hour, Lit(':'), Minute, Lit(':'), Second, Space, Gmt],
    // dddd, d'-'MMM'-'yy H:m:s
    &[WeekdayLong, Lit(','), Space, Day, Lit('-'), Month, Lit('-'), Year2, Space, Hour, Lit(':'), Minute, Lit(':'), Second],
    // dddd, d'-'MMM'-'yy H:m:s zzz
    &[WeekdayLong, Lit(','), Space, Day, Lit('-'), Month, Lit('-'), Year2, Space, Hour, Lit(':'), Minute, Lit(':'), Second, Space, Zone],
    // ddd MMM d H:m:s yyyy (asctime)
    &[WeekdayShort, Space, Month, Space, Day, Space, Hour, Lit(':'), Minute, Lit(':'), Second, Space, Year4],
    // ddd, d MMM yyyy H:m:s zzz
    &[WeekdayShort, Lit(','), Space, Day, Space, Month, Space, Year4, Space, Hour, Lit(':'), Minute, Lit(':'), Second, Space, Zone],
];

#[derive(Default)]
struct DateFields {
    weekday: Option<usize>,
    day: i64,
    month: i64,
    year: i64,
    year_digits: usize,
    hour: i64,
    minute: i64,
    second: i64,
}

fn skip_ws(s: &[char], mut p: usize) -> usize {
    while p < s.len() && s[p].is_whitespace() {
        p += 1;
    }
    p
}

/// `lo..=hi` ASCII digits at `p`, greedy up to `hi`.
fn read_digits(s: &[char], p: usize, lo: usize, hi: usize) -> Option<(i64, usize)> {
    let mut q = p;
    let mut value: i64 = 0;
    while q < s.len() && q - p < hi && s[q].is_ascii_digit() {
        value = value * 10 + i64::from(s[q].to_digit(10)?);
        q += 1;
    }
    if q - p < lo {
        return None;
    }
    Some((value, q))
}

/// Case-insensitive (ordinal) comparison of the lowercase ASCII `word` with the text at `p`.
fn word_at(s: &[char], p: usize, word: &str) -> bool {
    let n = word.chars().count();
    s.len() >= p + n
        && s[p..p + n]
            .iter()
            .zip(word.chars())
            .all(|(a, b)| a.to_ascii_lowercase() == b)
}

fn match_format(s: &[char], format: &[DateTok]) -> bool {
    let n = s.len();
    let mut p = 0;
    let mut f = DateFields::default();
    for tok in format {
        p = skip_ws(s, p);
        match *tok {
            Space => {}
            Lit(c) => {
                if p >= n || s[p] != c {
                    return false;
                }
                p += 1;
            }
            Gmt => {
                if s.len() >= p + 3 && (s[p..p + 3] == ['G', 'M', 'T'] || s[p..p + 3] == ['U', 'T', 'C']) {
                    p += 3;
                } else {
                    return false;
                }
            }
            WeekdayShort => {
                let Some(i) = WEEKDAYS_SHORT.iter().position(|w| word_at(s, p, w)) else {
                    return false;
                };
                f.weekday = Some(i);
                p += 3;
            }
            WeekdayLong => {
                let Some(i) = WEEKDAYS_LONG.iter().position(|w| word_at(s, p, w)) else {
                    return false;
                };
                f.weekday = Some(i);
                p += WEEKDAYS_LONG[i].len();
            }
            Day => {
                let Some((v, q)) = read_digits(s, p, 1, 2) else {
                    return false;
                };
                f.day = v;
                p = q;
            }
            Month => {
                let Some(i) = MONTHS.iter().position(|m| word_at(s, p, m)) else {
                    return false;
                };
                f.month = i as i64 + 1;
                p += 3;
            }
            Year2 | Year4 => {
                let digits = if matches!(tok, Year2) { 2 } else { 4 };
                let Some((v, q)) = read_digits(s, p, digits, digits) else {
                    return false;
                };
                f.year = v;
                f.year_digits = digits;
                p = q;
            }
            Hour | Minute | Second => {
                let Some((v, q)) = read_digits(s, p, 1, 2) else {
                    return false;
                };
                match tok {
                    Hour => f.hour = v,
                    Minute => f.minute = v,
                    _ => f.second = v,
                }
                p = q;
            }
            Zone => {
                if p >= n || (s[p] != '+' && s[p] != '-') {
                    return false;
                }
                p += 1;
                let Some((h, q)) = read_digits(s, p, 1, 2) else {
                    return false;
                };
                p = q;
                if p < n && s[p] == ':' {
                    p += 1;
                }
                let Some((m, q)) = read_digits(s, p, 2, 2) else {
                    return false;
                };
                p = q;
                if m >= 60 || h * 60 + m > 14 * 60 {
                    return false;
                }
            }
        }
    }
    if skip_ws(s, p) != n {
        return false;
    }
    if f.hour > 23 || f.minute > 59 || f.second > 59 {
        return false;
    }
    if f.year_digits == 2 {
        // .NET `TwoDigitYearMax` 2049: 00..=49 -> 2000s, 50..=99 -> 1900s.
        f.year += if f.year <= 49 { 2000 } else { 1900 };
    }
    if f.year < 1 || !(1..=12).contains(&f.month) || f.day < 1 || f.day > days_in_month(f.year, f.month) {
        return false;
    }
    f.weekday.is_none_or(|w| w == weekday_of(f.year, f.month, f.day))
}

fn is_http_date(v: &str) -> bool {
    let chars: Vec<char> = v.chars().collect();
    let start = skip_ws(&chars, 0);
    DATE_FORMATS.iter().any(|fmt| match_format(&chars[start..], fmt))
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
    /// The property name exactly as written in the text (escapes NOT resolved), like .NET keeps it.
    raw_name: String,
    value: Value,
}

/// .NET throws `InvalidOperationException` when a lookup has to unescape a name holding a lone surrogate; the
/// checker maps that to `BadResponse`.
struct LookupThrows;

/// `JsonElement.TryGetProperty(key)` as .NET 10 implements it: scan from the last property backwards, the first
/// match wins. A name WITHOUT an escape is compared as raw bytes. A name WITH an escape is only unescaped when it
/// is longer than the key, the key is longer than the offset of the first backslash and everything before that
/// backslash equals the key's start: a bad (lone surrogate) name therefore throws only for the lookups that get
/// that far (differential run: `{"\ud800":1,"tag_name":..,"draft":true}` is fine, a missing `draft` throws).
fn find_property<'a>(props: &'a [Prop], key: &str) -> Result<Option<&'a Prop>, LookupThrows> {
    let key_bytes = key.as_bytes();
    for p in props.iter().rev() {
        let raw = p.raw_name.as_bytes();
        let Some(backslash) = raw.iter().position(|b| *b == b'\\') else {
            if raw == key_bytes {
                return Ok(Some(p));
            }
            continue;
        };
        if raw.len() > key_bytes.len() && key_bytes.len() > backslash && raw[..backslash] == key_bytes[..backslash] {
            let unescaped = unescape_name_tail(&p.raw_name[backslash..]).ok_or(LookupThrows)?;
            if unescaped.as_bytes() == &key_bytes[backslash..] {
                return Ok(Some(p));
            }
        }
    }
    Ok(None)
}

/// Unescapes a (validated) JSON string body; `None` when it holds a lone surrogate.
fn unescape_name_tail(raw: &str) -> Option<String> {
    let mut closed = String::with_capacity(raw.len() + 1);
    closed.push_str(raw);
    closed.push('"');
    Scanner {
        b: closed.as_bytes(),
        pos: 0,
    }
    .string()?
}

struct Scanner<'a> {
    b: &'a [u8],
    pos: usize,
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
            let name_start = self.pos;
            // A name holding a lone surrogate still parses; it only throws when compared (`find_property`).
            self.string()?;
            let name_end = self.pos - 1;
            self.skip_ws();
            self.eat(b':')?;
            let value = self.value(depth)?;
            if let Some(list) = props.as_deref_mut() {
                let raw_name = std::str::from_utf8(self.b.get(name_start..name_end)?).ok()?.to_owned();
                list.push(Prop { raw_name, value });
            }
            self.skip_ws();
            if self.eat(b',').is_some() {
                continue;
            }
            return self.eat(b'}');
        }
    }
}

/// Parses `text` as one JSON document whose root is an object. `None` = invalid JSON or root not an object
/// (`BadResponse`).
fn scan_root_object(text: &str) -> Option<Vec<Prop>> {
    let mut s = Scanner {
        b: text.as_bytes(),
        pos: 0,
    };
    s.skip_ws();
    s.eat(b'{')?;
    let mut props = Vec::new();
    s.object(1, Some(&mut props))?;
    s.skip_ws();
    if s.pos != s.b.len() {
        return None;
    }
    Some(props)
}
