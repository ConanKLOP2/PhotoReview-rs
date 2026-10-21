//! Native dialogs and the settings UI (Wave 8 B5; port of `PhotoReview.Shell.Win32/Dialogs` and `Native`).
//!
//! Lead-owned list of submodules so the unit groups of B5 never edit this file in parallel; each group replaces
//! only the placeholder files it owns.

pub mod action_profiles;
pub mod benchmark;
pub mod clipboard;
pub mod diagnostics;
pub mod dialog_service;
pub mod file_dialog;
pub mod folder_dialog;
pub mod native;
pub mod recovery;
pub mod secondary_window_port;
pub mod settings;
pub mod task_dialog;
pub mod zoom_prompt;
pub mod zoom_prompt_service;
