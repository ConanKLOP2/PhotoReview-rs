//! The shell window of `PhotoReview.Shell.Win32/Hosting/ShellWindow.cs` (516 lines, baseline `d92d6aae`): W8-13e (creation, public API, lifecycle;
//! `create`) and W8-13f (message processing and DPI; `messages`, `dpi`). Lead-owned index.
//!
//! One type, [`ShellWindow`] (the owner handle) over `ShellWindowInner` (all state, `Rc`-shared with the window procedure through the
//! `GWLP_USERDATA` cookie). `create.rs` declares the struct with ALL fields; `messages.rs` and `dpi.rs` add `impl ShellWindowInner` blocks.

pub mod create;
pub mod dpi;
pub mod messages;

pub use create::{ShellWindow, ShellWindowOptions};
