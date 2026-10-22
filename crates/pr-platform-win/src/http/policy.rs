#![allow(unused_variables, dead_code)] // Contract stub; porter removes with the last todo.
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
    todo!()
}
/// Select the proxy source for HTTPS. Bypass matching is a later per-target step.
pub fn proxy_source(has_https_proxy: bool, has_all_proxy: bool, has_user_proxy: bool) -> ProxySource {
    todo!()
}
/// Remaining whole-exchange time, or `None` when the deadline has expired.
pub fn remaining_timeout(total: Duration, elapsed: Duration) -> Option<Duration> {
    todo!()
}
/// Map one WinHTTP error code to the checker's .NET-style error category.
pub fn classify_winhttp_error(code: u32) -> HttpRequestError {
    todo!()
}
