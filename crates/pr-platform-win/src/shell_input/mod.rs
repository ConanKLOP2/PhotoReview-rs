//! `pr_platform_win::shell_input`: the Win32 input layer of `PhotoReview.Shell.Win32/Input` (Wave 8 unit 14, batch B4: W8-14a to W8-14d). Lead-owned
//! index (D-W8B1-4). Baseline `d92d6aae`.
//!
//! | file | unit | C# |
//! |---|---|---|
//! | `click_counter`, `native`, `environment`, `handlers` | W8-14a | `ClickCounter`, `InputNative`, `ShellInputEnvironment`, `KeyboardMessageHandler`, `PointerMessageHandler`, `WheelMessageHandler` |
//! | `dispatch` | W8-14b | `ShellInput` (+ `IShellInputHost`) |
//! | `surface`, `main_window` | W8-14c | `ViewportImageSurface`, `ShellMainWindow.Input` |
//! | `drop_target` | W8-14d | `DropTarget` (`DropData`, `ShellDropTarget`) |
//!
//! The WPF adapters (`WpfInputAdapters`, `WpfImageSurface`) are retired. Everything here is UI-thread code: the types are `!Send` (`Rc`, `RefCell`);
//! the window procedure re-enters the handler chain through nested `SendMessage` (D-W8B3H-6), so no `RefCell` borrow is held across a call that can
//! reach the OS or the host (D-W8B4I-1). The pure decisions are in `pr_app::input` (shortcut router, pointer controller, gestures); this module
//! only decodes messages and wires those controllers to the window.

pub mod click_counter;
pub mod dispatch;
pub mod drop_target;
pub mod environment;
pub mod handlers;
pub mod main_window;
pub mod native;
pub mod surface;
