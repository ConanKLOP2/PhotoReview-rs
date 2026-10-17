#![cfg(windows)]

#[path = "../src/forward_client.rs"]
mod forward_client;

use std::time::Duration;

use forward_client::{ForwardPipeClient, WindowsForwardTransport};
use pr_core::instance::forwarding::ForwardOutcome;
use pr_core::sync::CancelToken;

#[test]
#[ignore = "native named-pipe probe; requires Windows pipe runtime"]
fn send_to_unique_absent_pipe_reports_no_instance() {
    // C# twin: NoServer_IsNoInstance / Client_NoServer_ReportsNoInstance.
    // This deliberately uses a unique synthetic name and can never contact the production pipe.
    let name = format!("PhotoReviewRs.test.w6_03d.{}", std::process::id());
    let client = ForwardPipeClient::new(Some(name), false).unwrap();
    assert_eq!(client.send_with_timeout(&[], Duration::from_millis(500), &CancelToken::none()), Ok(ForwardOutcome::NoInstance));
}
