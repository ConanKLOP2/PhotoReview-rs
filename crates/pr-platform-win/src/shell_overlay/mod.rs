//! `pr_platform_win::shell_overlay`: the Win32 parts of the overlay HUD of `PhotoReview.Shell.Win32/Overlay` (Wave 8 unit 15, batch B4: W8-15f to W8-15j;
//! the pure element tree 15a to 15e is `pr_app::overlay`). Lead-owned index (D-W8B1-4). Baseline `d92d6aae`.
//!
//! - `native` and `host` and `messages` (W8-15f): `OverlayNative`, `OverlayHost` (z-ordered roots), `OverlayMessageHandler` (window messages into the overlay).
//! - `tooltips` (W8-15g): `NativeTooltipHost` (`tooltips_class32`), `DeferredTooltips`, the `OverlayTooltips` seam.
//! - `overlay` (W8-15h, W8-15i): `ShellOverlay` split into `core`, `layout`, `hit_test`, `autohide` (15h) and `input`, `paint` (15i).
//! - `main_window` (W8-15j): `ShellMainWindow.Overlay` (attach, draw, dispose, view-model and settings wiring, the Tools menu commands).
//!
//! Everything is UI-thread code: the types are `!Send` (`Rc`, `RefCell`). `ShellOverlay` and its collaborators contain no `unsafe` (the OS is behind the
//! `OverlayOs`, `OverlayTooltips` and `OverlayMenuHost` seams); `native`, `tooltips` (the native host) and `main_window` (one `ClientToScreen` helper) do.
//! All `unsafe` has a `// SAFETY:` comment. ShellOverlayServices.cs (19 lines, one DI registration) is retired: no DI container (W8-09).

pub mod host;
pub mod main_window;
pub mod messages;
pub mod native;
pub mod overlay;
pub mod tooltips;
