//! `ShellOverlay` of `PhotoReview.Shell.Win32/Overlay/ShellOverlay.cs` (675 lines, baseline `d92d6aae`): the HUD of the shell (toolbar, status line,
//! folder info, zoom indicator, capture-pair badge), its auto-hide, its fade animation, the toolbar buttons, the Tools menu and the tooltips.
//! W8-15h (`state`, `layout`, `hit_test`, `autohide`) and W8-15i (`input`, `paint`). Lead-owned index.
//!
//! One type, [`state::ShellOverlay`], in one `Rc`; each file adds an `impl ShellOverlay` block (the W8-13e/13f precedent for a big class split across
//! units, D-W8B4O-3). The class knows neither `MainViewModel` nor an `HWND`: data comes in through `update(OverlayModel)`, pointer and keys through the
//! `pointer_*`/`key_down` methods, drawing through `render`. It contains no `unsafe` and no `windows` type.
//!
//! Function map (C# member -> Rust file; "pure" = no OS, decided by the data it is given):
//! - `state`: options, state, `new`, accessors, `update`, `settings_changed`, `apply_localization`, `note_activity`, `window_activated`/`deactivated`,
//!   `apply_model`, `dispose`.
//! - `layout` (pure): `arrange`, `last_client`, `last_dpi`, `element_bounds`.
//! - `hit_test` (pure): `hit_test`, `pointer_moved`, `pointer_left`, `hit_button`.
//! - `autohide` (pure over the injected clock and settings): the toolbar and info-overlay visibility rules, `fade`, the frame subscription and `on_frame`.
//! - `input` (pure over the injected seams): `pointer_down`/`up`/`cancel`, `wheel`, `key_down`, button clicks, the Tools menu, focus clearing.
//! - `paint` (pure over a `DrawContext`): `render`, `collect_tooltips`, `sync_tooltips`.

pub mod autohide;
pub mod hit_test;
pub mod input;
pub mod layout;
pub mod paint;
pub mod state;
