//! Wave 8 batch B6 start-up logic that needs no window and no `windows` crate (lead-owned index, contract writer of 17a..17c).
//!
//! `launch`, `forwarded_open`, `error_log` and `context` are W8-17a (`contracts/W8-17a.md`). The native parts of the start-up are in
//! `pr_platform_win::shell_startup`, the composition in `pr_shell::startup` (D-W8B6-1). W8-17d/17e append their pure modules here (re-read this
//! file before editing it).

pub mod context;
pub mod error_log;
pub mod forwarded_open;
pub mod launch;

// W8-17d (`contracts/W8-17d.md`): options and constants, the UI hop, settings load and repair notice (C-28), single-instance negotiation, early decode.
pub mod early_decode;
pub mod instance;
pub mod options;
pub mod settings_load;
pub mod ui_hop;

// W8-17e (`contracts/W8-17e.md`): crash handling (P-15), recovery prompt (C-27), shutdown and failure order (W-11), smoke run.
pub mod crash;
pub mod placement_read;
pub mod recovery;
pub mod shutdown;
pub mod smoke;
