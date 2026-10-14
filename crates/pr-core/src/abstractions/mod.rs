//! Port of `PhotoReview.Core/Abstractions` (C# baseline e441d67a): the seams production code depends on and tests
//! replace with fakes (`pr-test-support`). Mapping rules from C# are in docs/adr/R0003-concurrency.md:
//! blocking calls instead of `Task`, `io::Result` instead of exceptions, [`CancelToken`](crate::sync::CancelToken)
//! instead of `CancellationToken`, drop instead of `IDisposable`.
//!
//! `IAppPaths` has no trait: [`AppPaths`](crate::app_paths::AppPaths) is plain data that tests construct directly.

mod clock;
mod dialog;
mod display;
mod explorer;
mod file_system;
mod journal_files;
mod live_operations;
mod log;
mod memory;
mod misc;
mod recycle_bin;
mod source_reader;
mod ui;

pub use clock::{Clock, SystemClock};
pub use dialog::{BatchReviewItem, DialogService};
pub use display::{DisplayClock, DisplayTiming};
pub use explorer::ExplorerOrderProvider;
pub use file_system::{CopyCreationProof, FileStat, FileSystem, ReadSeek, SkippedEntry, SkippedKind};
pub use journal_files::{JournalCompactionFiles, StagedReplacement};
pub use live_operations::{InProcessLiveOperationRegistry, LiveMarker, LiveOperationRegistry};
pub use log::{Log, NullLog};
pub use memory::{MemoryProbe, MemorySnapshot};
pub use misc::{KeyNameValidator, NaturalComparer};
pub use recycle_bin::RecycleBin;
pub use source_reader::{PhysicalSourceReader, SourceReadPriority, SourceReader};
pub use ui::{ImmediateUiScheduler, UiAction, UiActionPanicked, UiCompletion, UiScheduler, panic_message};
