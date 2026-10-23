#![allow(unused_variables, dead_code)] // Contract stub; porter removes with the last todo.
//! Port of `src/PhotoReview.Core/Updates/UpdateChecker.cs` default HttpClient transport, baseline d92d6aae.

use pr_core::sync::CancelToken;
use pr_core::updates::checker::{HttpError, HttpRequest, HttpResponse, HttpTransport};
use std::time::Duration;

/// Windows transport behind `pr_core::updates::checker::HttpTransport`.
#[derive(Debug, Default)]
pub struct WinHttpTransport;

impl HttpTransport for WinHttpTransport {
    /// Start the bounded request and return once final response headers arrive.
    fn get(&self, request: &HttpRequest, timeout: Duration, cancel: &CancelToken) -> Result<HttpResponse, HttpError> {
        todo!()
    }
}
