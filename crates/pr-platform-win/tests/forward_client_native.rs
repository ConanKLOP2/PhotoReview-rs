#![cfg(windows)]

#[path = "../src/forward_client.rs"]
mod forward_client;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use forward_client::ForwardPipeClient;
use pr_core::instance::forwarding::ForwardOutcome;
use pr_core::sync::CancelToken;

static NEXT_PIPE: AtomicU64 = AtomicU64::new(1);

fn unique_pipe_name() -> String {
    format!(
        "PhotoReviewRs.test.w6_03d.{}.{}",
        std::process::id(),
        NEXT_PIPE.fetch_add(1, Ordering::Relaxed)
    )
}

#[test]
#[ignore = "native named-pipe probe; requires Windows pipe runtime"]
fn send_to_unique_absent_pipe_reports_no_instance() {
    // C# twin: NoServer_IsNoInstance / Client_NoServer_ReportsNoInstance.
    // This deliberately uses a unique synthetic name and can never contact the production pipe.
    let name = unique_pipe_name();
    let client = ForwardPipeClient::new(Some(name), false).unwrap();
    assert_eq!(
        client.send_with_timeout(&[], Duration::from_millis(500), &CancelToken::none()),
        Ok(ForwardOutcome::NoInstance)
    );
}

#[test]
#[ignore = "native named-pipe integration; requires Windows pipe runtime"]
fn send_accepts_same_user_server_and_delivers_empty_front_request() {
    // C# twin: Forward_DeliversPathsToServer; empty path lists are valid foreground requests.
    let name = unique_pipe_name();
    let (accepted_tx, accepted_rx) = std::sync::mpsc::channel();
    let server = pr_platform_win::forward_server::InstanceForwardServer::new(
        name.clone(),
        move |paths| {
            let _ = accepted_tx.send(paths.to_vec());
        },
        None,
        None,
        Some(Duration::from_secs(2)),
    )
    .unwrap();
    server.start().unwrap();

    let client = ForwardPipeClient::new(Some(name), false).unwrap();
    assert_eq!(
        client.send_with_timeout(&[], Duration::from_secs(2), &CancelToken::none()),
        Ok(ForwardOutcome::Delivered)
    );
    assert_eq!(
        accepted_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        Vec::<String>::new()
    );
    server.dispose();
}
