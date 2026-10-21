//! `pr_platform_win::shell_menus`: the Win32 menus of `PhotoReview.Shell.Win32/Menus` (Wave 8 unit 14, batch B4: W8-14e to W8-14g). Lead-owned index
//! (D-W8B1-4). Baseline `d92d6aae`.
//!
//! | file | unit | C# |
//! |---|---|---|
//! | `controller`, `message_handler`, `mapping`, `owner_draw_style` | W8-14e | `ContextMenuController` (+ `IContextMenuActions`), `ContextMenuMessageHandler`, `IPopupMenuHost`, `NativeMenuMapping`, `MenuOwnerDraw` |
//! | `popup_host`, `main_window` | W8-14f | `NativePopupMenuHost`, `ShellMainWindow.Menus` |
//! | `owner_draw` | W8-14g | `OwnerDrawMenuSession` (+ `OwnerDrawItem`) |
//!
//! The menu MODEL and its builder are `pr_app::menus` (W8-02a); this module shows it with a native `TrackPopupMenuEx` menu that the app paints itself
//! (owner-draw, dark palette `pr_app::overlay::palette`), so it does not depend on the undocumented uxtheme ordinals. UI-thread code, `!Send`;
//! `TrackPopupMenuEx` pumps messages of its own, so the window procedure is re-entered while a menu is open: no `RefCell` borrow is held across
//! `PopupMenuHost::show` (D-W8B4I-1).

pub mod controller;
pub mod main_window;
pub mod mapping;
pub mod message_handler;
pub mod owner_draw;
pub mod owner_draw_style;
pub mod popup_host;
