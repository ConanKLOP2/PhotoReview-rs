//! Test helpers shared across crates (port of `tests/PhotoReview.TestSupport` plus the fakes C# tests built inline).
//! Never a production dependency. Owned by the lead: porters use these, they do not edit them; a missing helper is
//! reported as BLOCKED (docs/MULTI-AGENT-PLAN.md §5.2).

mod fake_clock;
mod fake_file_system;
mod fake_memory_probe;
mod fake_recycle_bin;
mod fake_ui;
pub mod golden;
mod recording_log;
mod temp_root;

pub use fake_clock::FakeClock;
pub use fake_file_system::{FakeFileSystem, FsOp};
pub use fake_memory_probe::FakeMemoryProbe;
pub use fake_recycle_bin::{BinHook, FakeRecycleBin};
pub use fake_ui::QueuedUiScheduler;
pub use recording_log::{LogLevel, RecordingLog};
pub use temp_root::TempRoot;
