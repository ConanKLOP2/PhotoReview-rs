//! Port of `src/PhotoReview.Core/Updates/UpdateChecker.cs` transport defaults, baseline d92d6aae.

use pr_core::updates::checker::HttpRequestError;
use std::time::Duration;

/// Decision for one response after `followed` previous redirect requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectDecision {
    /// Send the next request, counting one more followed redirect.
    Follow,
    /// Give this response to the checker, including a refused downgrade or the 51st redirect.
    ReturnResponse,
}

/// Where an HTTPS request obtains its proxy after environment precedence is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxySource {
    /// A nonempty `HTTPS_PROXY` setting.
    HttpsEnvironment,
    /// A nonempty `ALL_PROXY` setting when `HTTPS_PROXY` is absent.
    AllEnvironment,
    /// Current-user WinINet/IE settings when neither environment proxy is set.
    UserSettings,
    /// No environment proxy or current-user proxy is configured.
    Direct,
}

/// Choose whether to follow a redirect response; `has_location` means a usable target URI.
/// `followed` is the number of redirects already followed before this response.
pub fn redirect_decision(
    status: u16,
    from_https: bool,
    to_https: bool,
    has_location: bool,
    followed: u32,
) -> RedirectDecision {
    if matches!(status, 301 | 302 | 303 | 307 | 308) && has_location && followed < 50 && (!from_https || to_https) {
        RedirectDecision::Follow
    } else {
        RedirectDecision::ReturnResponse
    }
}
/// Select the proxy source for HTTPS. Bypass matching is a later per-target step.
pub fn proxy_source(has_https_proxy: bool, has_all_proxy: bool, has_user_proxy: bool) -> ProxySource {
    if has_https_proxy {
        ProxySource::HttpsEnvironment
    } else if has_all_proxy {
        ProxySource::AllEnvironment
    } else if has_user_proxy {
        ProxySource::UserSettings
    } else {
        ProxySource::Direct
    }
}
/// Remaining whole-exchange time, or `None` when the deadline has expired.
pub fn remaining_timeout(total: Duration, elapsed: Duration) -> Option<Duration> {
    total.checked_sub(elapsed).filter(|remaining| !remaining.is_zero())
}
/// Map one WinHTTP error code to the checker's .NET-style error category.
pub fn classify_winhttp_error(code: u32) -> HttpRequestError {
    // Numeric values from WinHttp.h (windows 0.62.2); semantics checked against
    // https://learn.microsoft.com/windows/win32/winhttp/error-messages
    // Keep ambiguous reset/truncation/proxy errors Unknown rather than guessing.
    match code {
        12007 => HttpRequestError::NameResolutionError, // NAME_NOT_RESOLVED: DNS failed.
        12029 => HttpRequestError::ConnectionError,     // CANNOT_CONNECT: connection failed.
        12037 | 12038 | 12044 | 12045 | 12057 | 12157 | 12169 | 12170 | 12175 | 12179 => {
            // Certificate validity/name/client certificate/CA/revocation/channel/security/usage errors.
            HttpRequestError::SecureConnectionError
        }
        12152 => HttpRequestError::InvalidResponse, // INVALID_SERVER_RESPONSE: cannot parse response.
        12181..=12184 => HttpRequestError::ConfigurationLimitExceeded,
        // HEADER_COUNT_EXCEEDED, HEADER_SIZE_OVERFLOW, CHUNKED_ENCODING_HEADER_SIZE_OVERFLOW,
        // RESPONSE_DRAIN_OVERFLOW: documented configured limit exceeded.
        _ => HttpRequestError::Unknown,
    }
}
