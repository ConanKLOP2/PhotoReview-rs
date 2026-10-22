//! `MainViewModel` (C# `PhotoReview.App.Shared/ViewModels/MainViewModel.cs`, 1 385 lines), split over five files by contract, cut at
//! method boundaries of baseline `d92d6aae`. The struct is defined once, in `state`; the other files add `impl MainViewModel` blocks.
//!
//! | file | unit | C# lines |
//! |---|---|---|
//! | `state` | W8-08a | 1 to 326: fields, constructor, `NotifyCurrentImageChanged`, properties, status, skipped entries, capture-pair badge (+ the Rust-only change set, event inbox, `pump`, bridge) |
//! | `navigation` | W8-08b | 327 to 564: capture member, compare, open folder / path, ownership, next / previous / first / last / skip, sibling folder |
//! | `commands` | W8-08c | 565 to 854: queued file actions, recycle, undo, zoom, fullscreen, info overlay, refresh, external editor, clipboard, duplicates, dialogs |
//! | `session` | W8-08d | 855 to 1085: settings dialog entry and its reloads, close, instance label, title bar, folder line, language switch |
//! | `notify_move` | W8-08e | 1086 to 1385: change notification, the four sink recorders and their replay, move / copy to folder, EXIF line |
//!
//! Lead-owned wiring: `viewmodels/mod.rs` gets `pub mod main;`.

pub mod commands;
pub mod navigation;
pub mod notify_move;
pub mod rename;
pub mod rename_prompt;
pub mod session;
pub mod state;

pub use commands::{ExternalEditorBug, ExternalEditorFailure, ExternalEditorLauncher, ProcessEditorLauncher};
pub use navigation::{OpenFolderError, OpenPurpose, OwnershipRequest};
pub use notify_move::{SinkLog, remember_move_copy_folder};
pub use state::{
    ChangeSet, FormatContextSource, ImageChangingEventArgs, MainChanges, MainEvent, MainFolderInfoLabels, MainProperty,
    MainViewModel, MainViewModelDeps, MainWorkerPort, PresentationBridge, SettingsEdit, SettingsSource, ViewerHandle,
};
