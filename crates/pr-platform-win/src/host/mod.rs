//! `pr_platform_win::host`: the window host of `PhotoReview.Shell.Win32/Hosting` (Wave 8 unit 13, batch B3: W8-13a to W8-13f; the main window
//! W8-13g to 13k is batch B4). Lead-owned index (D-W8B1-4). Baseline `d92d6aae`.
//!
//! Frame clocks (`frame_clock`, `swap_chain_clock`), the shell window contracts and icons (`contracts`, `icons`, `native`), the message loop
//! (`message_loop`, `pending_panic`), the UI dispatcher (`ui_dispatcher`) and the window procedure plus the shell window (`wndproc`, `shell_window`).
//! Everything here is UI-thread code: the types are `!Send` except the cross-thread half of `ui_dispatcher`. All `unsafe` has a `// SAFETY:` comment.
//!
//! `pr-platform-win` depends on `pr-app` (not the other way round): `SizeD`/`PointD` (`contracts`) and `FrameTick`/`FrameHandler`/`FrameHandlerId`
//! (`frame_clock::base`) are `pub use` of the `pr_app` types (reconciliation amendment A10), so no conversion exists at the boundary.
//! `UiDispatcher`/`UiScheduler` are in `pr_core::abstractions` and need no twin.

pub mod contracts;
pub mod error;
pub mod frame_clock;
pub mod icons;
pub mod main_window;
pub mod message_loop;
pub mod native;
pub mod pending_panic;
pub mod shell_window;
pub mod swap_chain_clock;
pub mod ui_dispatcher;
pub mod wndproc;
