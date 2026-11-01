//! `pr_platform_win::host::main_window`: the main window of the Win32 shell, `PhotoReview.Shell.Win32/ShellMainWindow*.cs` and `Windowing/*`
//! (Wave 8 batch B4, units W8-13g to W8-13k; baseline `d92d6aae`). Lead-owned index.
//!
//! The C# `ShellMainWindow` is one `partial class` spread over many files. The Rust twin is one type pair, [`MainWindow`] (the owner handle,
//! `Drop` = C# `Dispose`) over `MainWindowInner` (all state, `Rc`-shared with the closures that the window events and the dispatcher call), and one
//! `impl` block per C# file:
//!
//! | file | unit | C# |
//! |---|---|---|
//! | `core` | W8-13g | `ShellMainWindow.cs` 1-226: options, fields, constructor, `RegisterFeatures`, `AttachRenderer`, `OpenInitialAsync` |
//! | `lifecycle` | W8-13h | `ShellMainWindow.cs` 226-452: forwarded open, layout and decode box, change fan-out, closing, closed, `Dispose` |
//! | `frame` | W8-13i | `ShellMainWindow.Frame.cs`: `Present`, `RenderNow`, first-present events |
//! | `scrollbars` | W8-13i | `ShellMainWindow.ScrollBars.cs`: registration and drawing of the scroll bars (the bars are W8-16f/16g) |
//! | `viewing` | W8-13j | `ShellMainWindow.Viewing.cs`: composer, prefetcher, image changes, GPU cache retain and clear |
//! | `windowing` | W8-13k | `Windowing/*`: `ShellFullscreen`, `TitleBar`, the real `WindowOps`, `Win32MonitorLayout`, display change |
//!
//! `Program.cs` (16 lines) becomes W8-20a (`crates/pr-shell`). The files of other units (`ShellMainWindow.Input/Menus/Overlay/KeepWarm`) are
//! W8-14c, 14f, 15j, 16d; they plug in through [`core::MainWindowFeature`] and the `pub(crate)` accessors of `core` (D-W8B4M-3).
//!
//! Thread affinity: UI thread only, `!Send`. Pre-conditions every unit repeats: never call a method of this module that can render, close or
//! re-enter (`apply_changes`, `render_now`, `apply_fullscreen`, `close`) while a `RefCell` borrow of the view model or the viewer is held (D-W8B4M-2).

pub mod core;
pub mod frame;
pub mod lifecycle;
pub(crate) mod modal_intents;
pub mod scrollbars;
pub mod viewing;
pub mod windowing;

// `self::` because a module named `core` would make a bare `core::` path ambiguous with the `core` crate (E0659).
pub use self::core::{MainWindow, MainWindowDeps, MainWindowOptions};
