//! Port of `PhotoReview.Shell.Win32/Menus/NativePopupMenuHost.cs` (149 lines, baseline `d92d6aae`). Unit W8-14f. Contract: `contracts/W8-14f.md`.
//!
//! C-12 host (NE-4 = a): a native `CreatePopupMenu` + `TrackPopupMenuEx` with `TPM_RETURNCMD`. Modal: the menu pumps messages itself, so keyboard,
//! accessibility, submenus and multi-monitor are the system's. The dark look is painted by the app (owner-draw, `owner_draw::OwnerDrawMenuSession`),
//! not by uxtheme. Runs on the UI thread of the owner window.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use pr_app::input::primitives::PointD;
use pr_app::menus::model::MenuItemModel;
use pr_core::abstractions::Log;

use super::controller::PopupMenuHost;
use crate::win32::structs::{Hmenu, Hwnd};

/// `MF_OWNERDRAW` (`0x0100`): not in `win32::messages` (C# private `MfOwnerDraw`).
pub const MF_OWNERDRAW: u32 = 0x0000_0100;
/// `MIM_BACKGROUND` (`0x2`): `MENUINFO.fMask` bit for `hbrBack` (C# private `MimBackground`).
pub const MIM_BACKGROUND: u32 = 0x0000_0002;
/// `MIM_APPLYTOSUBMENUS` (`0x80000000`): apply `SetMenuInfo` to every submenu (C# private `MimApplyToSubmenus`).
pub const MIM_APPLY_TO_SUBMENUS: u32 = 0x8000_0000;

/// The ids of the commands of a menu tree in the order their command ids are assigned (C# `Populate` appends to `ids` in this order; command id = position + 1):
/// depth first; separators are skipped; a submenu WITH children is not a command (its children are, recursively); a submenu WITHOUT children is a plain command.
pub fn command_ids(items: &[MenuItemModel]) -> Vec<String> {
    todo!()
}

/// The `MenuCreated` test hook (C# `Action<nint>`).
pub type MenuCreatedHook = Box<dyn FnMut(Hmenu)>;

/// C# `NativePopupMenuHost(Func<nint> ownerHwnd)`.
pub struct NativePopupMenuHost {
    owner_hwnd: Box<dyn Fn() -> Hwnd>,
    showing: Cell<bool>,
    menu_created: RefCell<Option<MenuCreatedHook>>,
    log: Option<Arc<dyn Log>>,
}

impl NativePopupMenuHost {
    /// C# constructor: `owner_hwnd` returns the owner window (the viewer window), `0` when it is gone (then `show` answers `None`).
    pub fn new(owner_hwnd: Box<dyn Fn() -> Hwnd>) -> Self {
        Self {
            owner_hwnd,
            showing: Cell::new(false),
            menu_created: RefCell::new(None),
            log: None,
        }
    }

    /// The log the owner-draw session reports a swallowed paint panic to (D-W8B4I-9); none by default.
    pub fn with_log(mut self, log: Arc<dyn Log>) -> Self {
        self.log = Some(log);
        self
    }

    /// C# `MenuCreated` (tests): called with every `HMENU` (root and submenus) just created, to check that all of them are destroyed after the menu closed.
    pub fn set_menu_created(&self, hook: Option<MenuCreatedHook>) {
        todo!()
    }

    /// C# `ToScreenPixels(dip, dpi)`: `round_ties_even(dip * dpi / 96)` per axis; `dpi == 0` counts as 96.
    pub fn to_screen_pixels(dip: PointD, dpi: u32) -> (i32, i32) {
        todo!()
    }
}

impl fmt::Debug for NativePopupMenuHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativePopupMenuHost")
            .field("showing", &self.showing.get())
            .finish_non_exhaustive()
    }
}

impl PopupMenuHost for NativePopupMenuHost {
    /// C# `Show(items, screenPointDip)`: no owner window, already showing, or no items: `Ok(None)`. Else: create the owner-draw session for the owner's
    /// DPI and `attach` it; `CreatePopupMenu` (a failure is `Ok(None)`, as in C#); populate the tree (ids are the 1-based position in the flattened
    /// command list; a submenu without children is a plain command); `TrackPopupMenuEx(TPM_RETURNCMD | TPM_RIGHTBUTTON)` at the scaled point; map the
    /// command back to a model id; ALWAYS `DestroyMenu(root)` (it destroys the submenus) and clear `showing`, also when a panic unwinds.
    fn show(&self, items: &[MenuItemModel], screen_point_dip: PointD) -> Result<Option<String>, Box<dyn Error>> {
        todo!()
    }
}
