//! Test helpers shared across crates (port of `tests/PhotoReview.TestSupport` plus the fakes C# tests built inline).
//! Never a production dependency. Owned by the lead: porters use these, they do not edit them; a missing helper is
//! reported as BLOCKED (docs/MULTI-AGENT-PLAN.md §5.2).

mod fake_clock;
mod fake_file_system;
mod fake_memory_probe;
mod fake_recycle_bin;
mod fake_ui;
mod fault_injection_file_system;
pub mod golden;
mod hook_file_system;
mod recording_log;
mod temp_root;

pub use fake_clock::FakeClock;
pub use fake_file_system::{FakeFileSystem, FsOp};
pub use fake_memory_probe::FakeMemoryProbe;
pub use fake_recycle_bin::{BinHook, FakeRecycleBin};
pub use fake_ui::QueuedUiScheduler;
pub use fault_injection_file_system::{
    CancelHook, DISK_FULL_MESSAGE, FaultInjectionFileSystem, FaultKind, FaultPlan, ProcessCrashed, Step,
    is_process_crash,
};
pub use hook_file_system::{CopyPartialHook, FsEvent, HookFileSystem, PairHook, PathHook, StampOnMove};
pub use recording_log::{LogLevel, RecordingLog};
pub use temp_root::TempRoot;
