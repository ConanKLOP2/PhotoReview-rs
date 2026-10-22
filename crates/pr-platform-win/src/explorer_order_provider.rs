//! The production constructor of the Explorer order provider (W8-09b slice S2, `contracts/W8-09b.md`, decision D-W8B6-151).
//!
//! C# `services.AddSingleton<IExplorerOrderProvider, ExplorerOrderService>()` (`SharedServiceRegistration.cs` line 110, baseline `d92d6aae`): the DI container used the public constructor `ExplorerOrderService(ILog? log = null)` (the other, with a query seam, is `internal`),
//! which wires `ExplorerNativeView.Query` when no query is given. The Rust port (W6-01c/d) kept `ExplorerOrderService` and `explorer_native_view::query_shell`
//! `pub(crate)` and gave the service only `with_query(query, log)`, so no code OUTSIDE this crate can build the production provider. This module is the missing public door:
//! one function, no new behaviour. The two `pub(crate)` modules stay as they are (their tests `#[path]`-include the files, so they must not name `crate::` siblings that the test crates lack;
//! this file is NOT included by any test).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use pr_core::abstractions::{ExplorerOrderProvider, Log};

/// The dedicated STA pump thread could not be started (`PumpStartError` of `explorer_order_service`, which is crate-private).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerOrderStartError {
    /// The text of the underlying error.
    pub message: String,
}

impl fmt::Display for ExplorerOrderStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Explorer order service could not start: {}", self.message)
    }
}

impl Error for ExplorerOrderStartError {}

/// `ExplorerOrderService::with_query(Box::new(move |folder, progress, batch_size, token| explorer_native_view::query_shell(folder, progress, batch_size, token, log.clone())), Some(log))`
/// as the trait object the graph shares (`StartupGraph::explorer_order`, `FolderLoadDeps::explorer_order`). The pump thread starts here; the first query runs on it. A start failure is
/// the `Err` (the C# constructor would have thrown from `GetRequiredService`).
pub fn production_explorer_order(log: Arc<dyn Log>) -> Result<Arc<dyn ExplorerOrderProvider>, ExplorerOrderStartError> {
    todo!()
}
