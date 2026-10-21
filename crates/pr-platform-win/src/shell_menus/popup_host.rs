//! Port of `PhotoReview.Shell.Win32/Menus/NativePopupMenuHost.cs` (149 lines, baseline `d92d6aae`). Unit W8-14f. Contract: `contracts/W8-14f.md`.
//!
//! C-12 host (NE-4 = a): a native `CreatePopupMenu` + `TrackPopupMenuEx` with `TPM_RETURNCMD`. Modal: the menu pumps messages itself, so keyboard,
//! accessibility, submenus and multi-monitor are the system's. The dark look is painted by the app (owner-draw, `owner_draw::OwnerDrawMenuSession`),
//! not by uxtheme. Runs on the UI thread of the owner window.

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use pr_app::input::primitives::PointD;
use pr_app::menus::model::{MenuItemKind, MenuItemModel};
use pr_core::abstractions::Log;
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, HMENU, MENU_ITEM_FLAGS, MENUINFO, MENUINFO_MASK, MENUINFO_STYLE, SetMenuInfo,
};
use windows::core::PCWSTR;

use super::controller::PopupMenuHost;
use super::mapping;
use super::owner_draw::{OwnerDrawItem, OwnerDrawMenuSession};
use crate::host::pending_panic;
use crate::win32::api::user32;
use crate::win32::messages::{MF_POPUP, MF_SEPARATOR, TPM_RETURNCMD, TPM_RIGHTBUTTON};
use crate::win32::structs::{Hmenu, Hwnd};

/// C# `BaseDpi`: the DPI at which a DIP is a pixel.
const BASE_DPI: f64 = 96.0;

/// `MF_OWNERDRAW` (`0x0100`): not in `win32::messages` (C# private `MfOwnerDraw`).
pub const MF_OWNERDRAW: u32 = 0x0000_0100;
/// `MIM_BACKGROUND` (`0x2`): `MENUINFO.fMask` bit for `hbrBack` (C# private `MimBackground`).
pub const MIM_BACKGROUND: u32 = 0x0000_0002;
/// `MIM_APPLYTOSUBMENUS` (`0x80000000`): apply `SetMenuInfo` to every submenu (C# private `MimApplyToSubmenus`).
pub const MIM_APPLY_TO_SUBMENUS: u32 = 0x8000_0000;

/// The ids of the commands of a menu tree in the order their command ids are assigned (C# `Populate` appends to `ids` in this order; command id = position + 1):
/// depth first; separators are skipped; a submenu WITH children is not a command (its children are, recursively); a submenu WITHOUT children is a plain command.
pub fn command_ids(items: &[MenuItemModel]) -> Vec<String> {
    let mut ids = Vec::new();
    collect_ids(items, &mut ids);
    ids
}

fn collect_ids(items: &[MenuItemModel], ids: &mut Vec<String>) {
    for item in items {
        if item.kind == MenuItemKind::Separator {
            continue;
        }
        if item.kind == MenuItemKind::Submenu && !item.children.is_empty() {
            collect_ids(&item.children, ids);
            continue;
        }
        ids.push(item.id.clone());
    }
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
        *self.menu_created.borrow_mut() = hook;
    }

    /// C# `ToScreenPixels(dip, dpi)`: `round_ties_even(dip * dpi / 96)` per axis; `dpi == 0` counts as 96.
    pub fn to_screen_pixels(dip: PointD, dpi: u32) -> (i32, i32) {
        let scale = if dpi == 0 { 1.0 } else { f64::from(dpi) / BASE_DPI };
        // `as` saturates (and maps NaN to 0): a point outside the `i32` range cannot wrap.
        (
            (dip.x * scale).round_ties_even() as i32,
            (dip.y * scale).round_ties_even() as i32,
        )
    }

    /// Calls the `menu_created` hook (if any). The hook is taken out of its cell for the call (no borrow is held), so it may call back into this host.
    fn notify_created(&self, menu: Hmenu) {
        let taken = self.menu_created.borrow_mut().take();
        if let Some(mut hook) = taken {
            hook(menu);
            let mut slot = self.menu_created.borrow_mut();
            if slot.is_none() {
                *slot = Some(hook);
            }
        }
    }

    /// C# `Populate`.
    fn populate(
        &self,
        menu: Hmenu,
        items: &[MenuItemModel],
        ids: &mut Vec<String>,
        session: Option<&OwnerDrawMenuSession>,
    ) {
        self.notify_created(menu);
        if let Some(session) = session {
            style_menu(menu, session);
        }
        for item in items {
            if item.kind == MenuItemKind::Separator {
                match session {
                    None => {
                        let _ = user32::append_menu(menu, MF_SEPARATOR, 0, None);
                    }
                    Some(session) => {
                        let data = session.register(OwnerDrawItem {
                            text: String::new(),
                            shortcut: None,
                            is_separator: true,
                            is_submenu: false,
                        });
                        append_menu_owner_draw(menu, MF_SEPARATOR | MF_OWNERDRAW, 0, data);
                    }
                }
                continue;
            }
            let flags = mapping::item_flags(item);
            if item.kind == MenuItemKind::Submenu && !item.children.is_empty() {
                let Ok(sub) = user32::create_popup_menu() else {
                    continue;
                };
                // Until it is appended the submenu is not owned by `menu`: destroy it ourselves if the append fails (T1).
                let mut sub_guard = MenuGuard(Some(sub));
                self.populate(sub, &item.children, ids, session);
                if append_item(menu, flags | MF_POPUP, sub as usize, item, session) {
                    sub_guard.0 = None;
                }
                continue;
            }
            ids.push(item.id.clone());
            // Command id = position + 1 in `ids` (0 = cancelled).
            append_item(menu, flags, ids.len(), item, session);
        }
    }
}

/// Destroys a menu on drop (`DestroyMenu` also destroys the submenus attached to it). `None` = released.
struct MenuGuard(Option<Hmenu>);

impl Drop for MenuGuard {
    fn drop(&mut self) {
        if let Some(menu) = self.0.take() {
            let _ = user32::destroy_menu(menu);
        }
    }
}

/// Clears the `showing` flag on drop (also when a panic unwinds).
struct ShowingGuard<'a>(&'a Cell<bool>);

impl<'a> ShowingGuard<'a> {
    fn new(flag: &'a Cell<bool>) -> Self {
        flag.set(true);
        Self(flag)
    }
}

impl Drop for ShowingGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// C# `StyleMenu`: the background brush of the menu and of all its submenus.
fn style_menu(menu: Hmenu, session: &OwnerDrawMenuSession) {
    let info = MENUINFO {
        cbSize: std::mem::size_of::<MENUINFO>() as u32,
        fMask: MENUINFO_MASK(MIM_BACKGROUND | MIM_APPLY_TO_SUBMENUS),
        dwStyle: MENUINFO_STYLE(0),
        hbrBack: HBRUSH(session.background_brush() as *mut core::ffi::c_void),
        ..Default::default()
    };
    // SAFETY: `info` is a local `MENUINFO` with `cbSize` set; the OS reads it during the call and copies `hbrBack` (the brush is owned by `session`,
    // which outlives the menu). The result is ignored like the C# `_ =`.
    let _ = unsafe { SetMenuInfo(HMENU(menu as *mut core::ffi::c_void), &info) };
}

/// C# `AppendMenuData`: `AppendMenuW` with `MF_OWNERDRAW`, whose `lpNewItem` is the application value `item_data`. A failure is ignored by the callers
/// (a half-built menu is better than none); returns whether the item was appended.
fn append_menu_owner_draw(menu: Hmenu, flags: u32, id_or_submenu: usize, item_data: isize) -> bool {
    // SAFETY: `item_data` is an application value for `MF_OWNERDRAW`, never dereferenced by the OS; `id_or_submenu` is a command id or a handle the OS
    // validates.
    unsafe {
        AppendMenuW(
            HMENU(menu as *mut core::ffi::c_void),
            MENU_ITEM_FLAGS(flags),
            id_or_submenu,
            PCWSTR(item_data as *const u16),
        )
    }
    .is_ok()
}

/// C# `AppendItem`: returns whether the item was appended.
fn append_item(
    menu: Hmenu,
    flags: u32,
    id_or_submenu: usize,
    item: &MenuItemModel,
    session: Option<&OwnerDrawMenuSession>,
) -> bool {
    match session {
        None => user32::append_menu(menu, flags, id_or_submenu, Some(&mapping::display_text(item))).is_ok(),
        Some(session) => {
            // Owner-draw text is the raw text: it is drawn with `DT_NOPREFIX`, so `&` is not doubled.
            let data = session.register(OwnerDrawItem {
                text: item.text.clone(),
                shortcut: item.shortcut_text.clone(),
                is_separator: false,
                is_submenu: flags & MF_POPUP != 0,
            });
            append_menu_owner_draw(menu, flags | MF_OWNERDRAW, id_or_submenu, data)
        }
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
        let owner = (self.owner_hwnd)();
        if owner == 0 || self.showing.get() || items.is_empty() {
            return Ok(None);
        }
        let dpi = user32::get_dpi_for_window(owner);
        // Declared first, dropped last: the subclass is removed after the menu is destroyed.
        let session = OwnerDrawMenuSession::new(owner, dpi, self.log.clone());
        let owner_draw = session.attach();
        let Ok(root) = user32::create_popup_menu() else {
            return Ok(None);
        };
        let mut ids: Vec<String> = Vec::new();
        let command = {
            let _showing = ShowingGuard::new(&self.showing);
            let _root = MenuGuard(Some(root));
            self.populate(root, items, &mut ids, owner_draw.then_some(&session));
            let (x, y) = Self::to_screen_pixels(screen_point_dip, dpi);
            user32::track_popup_menu_ex(root, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, owner)
            // The menu is destroyed and `showing` cleared here, also when a panic unwinds.
        };
        // A panic captured by the window procedure during the nested loop is resumed by the owner of the native call, with the menu already gone.
        pending_panic::rethrow_pending();
        Ok(mapping::id_for_command(&ids, command).map(str::to_owned))
    }
}
