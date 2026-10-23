//! W8-09b slice S2: `explorer_order_provider::production_explorer_order` (the pump thread starts, answers and is joined on drop).
//!
//! C# twin: none (the DI container called `new ExplorerOrderService()`); every test is `// new:`. Native: it starts the real STA pump and asks the shell about the open Explorer windows
//! (read-only); it never touches the user's files (the folder asked about is a `TempRoot`).
#![cfg(windows)]

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use pr_core::abstractions::NullLog;
use pr_core::sync::CancelToken;
use pr_platform_win::explorer_order_provider::{ExplorerOrderStartError, production_explorer_order};
use pr_test_support::TempRoot;

// new: the error text.
#[test]
fn start_error_display_names_the_cause() {
    let error = ExplorerOrderStartError {
        message: "boom".to_owned(),
    };
    assert_eq!(error.to_string(), "Explorer order service could not start: boom");
}

// new: the pump starts, `prefetch` returns without waiting for the query, a query answers within its bound, and dropping the provider joins the pump thread.
#[test]
#[ignore = "native: starts the Explorer STA pump thread and queries the shell"]
fn production_provider_starts_prefetches_answers_and_stops_on_drop() {
    let (sender, receiver) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let temp = TempRoot::new("w8-09b-explorer-order");
        let provider = production_explorer_order(Arc::new(NullLog)).expect("the pump starts");
        provider.prefetch(temp.path(), Duration::from_secs(10));
        let snapshot =
            provider.try_get_snapshot_progressive(temp.path(), Duration::from_secs(10), None, 16, &CancelToken::none());
        drop(provider); // joins the pump
        let _ = sender.send(snapshot.folder);
    });
    let folder = receiver
        .recv_timeout(Duration::from_secs(30))
        .expect("start, query and drop finish within 30 s");
    handle.join().expect("thread");
    assert!(!folder.is_empty());
}
