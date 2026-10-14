//! Port of `IExplorerOrderProvider.cs`. C#'s `Task`-returning query becomes a blocking call that the caller runs on
//! a worker thread (docs/adr/R0003-concurrency.md).

use std::path::Path;
use std::time::Duration;

use crate::catalog::explorer_types::{ExplorerQueryProgress, ExplorerViewSnapshot};
use crate::sync::CancelToken;

/// The real file order of a folder as shown by an open Windows Explorer window.
pub trait ExplorerOrderProvider: Send + Sync {
    /// Blocks until the snapshot is read, `timeout` passes or `cancel` fires; never fails: problems are reported in
    /// the snapshot's status/reason. `progress` receives partial counts as batches of `batch_size` items arrive
    /// (C# default 16).
    fn try_get_snapshot_progressive(
        &self,
        folder: &Path,
        timeout: Duration,
        progress: Option<&(dyn Fn(ExplorerQueryProgress) + Sync)>,
        batch_size: usize,
        cancel: &CancelToken,
    ) -> ExplorerViewSnapshot;

    /// perf(startup): starts the query for `folder` before anyone asks; the next query for the same folder joins it,
    /// any other request cancels it. Default: no-op (test fakes).
    fn prefetch(&self, folder: &Path, timeout: Duration) {
        let _ = (folder, timeout);
    }
}
