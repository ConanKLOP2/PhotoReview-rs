//! Tests of W8-14f `shell_menus::popup_host` (C# `NativePopupMenuHostTests`, `NativeContextMenuHostTests`, the two popup-capture tests of
//! `ShellMenuRenderingTests`, baseline `d92d6aae`).
//!
//! Pure: `command_ids`, `to_screen_pixels`, "no items / no owner makes no OS call". Native (`#[ignore = "native: needs desktop"]`): the host on a hidden
//! `STATIC` owner of the test thread, keys posted BEFORE the menu opens (the modal loop of `TrackPopupMenuEx` reads them). Every native test arms a hard
//! watchdog that posts Escape to the owner, so a menu that did not take its keys cannot hang the run. The two `popup_capture_*` tests print the real
//! popup (class `#32768`) into a bitmap from a helper thread while the menu is open; the popup is on screen for a fraction of a second at (10, 10).

#![cfg(windows)]

#[path = "common/owner_draw_fixture.rs"]
mod fixture;

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use fixture::{OwnerWindow, serial};
use pr_app::input::primitives::PointD;
use pr_app::menus::model::{MenuItemKind, MenuItemModel};
use pr_platform_win::host::pending_panic;
use pr_platform_win::shell_menus::controller::PopupMenuHost;
use pr_platform_win::shell_menus::owner_draw_style::{ODS_SELECTED, background, item_background};
use pr_platform_win::shell_menus::popup_host::{
    MF_OWNERDRAW, MIM_APPLY_TO_SUBMENUS, MIM_BACKGROUND, NativePopupMenuHost, command_ids,
};
use pr_platform_win::win32::dialog_native::{kernel32, user32 as dialog_user32};
use pr_platform_win::win32::messages::WM_KEYDOWN;
use pr_platform_win::win32::structs::{Hmenu, Hwnd};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    HDC, ReleaseDC, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowRect, IsMenu, PostMessageW};
use windows::core::{BOOL, w};

#[link(name = "user32")]
unsafe extern "system" {
    // `PrintWindow` lives in the `Win32_Storage_Xps` feature of the `windows` crate, which the crate does not enable.
    fn PrintWindow(hwnd: HWND, hdc: HDC, flags: u32) -> BOOL;
}

const VK_ESCAPE: usize = 0x1B;
const VK_RETURN: usize = 0x0D;
const VK_DOWN: usize = 0x28;
const VK_RIGHT: usize = 0x27;

// ---------------------------------------------------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------------------------------------------------

fn item(id: &str, kind: MenuItemKind, text: &str, enabled: bool, children: Vec<MenuItemModel>) -> MenuItemModel {
    MenuItemModel {
        id: id.to_owned(),
        kind,
        text: text.to_owned(),
        shortcut_text: None,
        is_enabled: enabled,
        is_checked: false,
        children,
        automation_name: None,
    }
}

fn command(id: &str, text: &str) -> MenuItemModel {
    item(id, MenuItemKind::Command, text, true, Vec::new())
}

fn disabled(id: &str, text: &str) -> MenuItemModel {
    item(id, MenuItemKind::Command, text, false, Vec::new())
}

fn separator() -> MenuItemModel {
    item("Separator.1", MenuItemKind::Separator, "", false, Vec::new())
}

fn submenu(id: &str, text: &str, children: Vec<MenuItemModel>) -> MenuItemModel {
    item(id, MenuItemKind::Submenu, text, true, children)
}

fn model_with_submenu() -> Vec<MenuItemModel> {
    vec![
        command("A", "Alpha"),
        submenu(
            "Zoom",
            "Zoom",
            vec![command("Zoom.Preset.50", "50 %"), command("Zoom.Preset.100", "100 %")],
        ),
        command("B", "Beta"),
    ]
}

fn at(x: f64, y: f64) -> PointD {
    PointD { x, y }
}

fn ids(items: &[MenuItemModel]) -> Vec<String> {
    command_ids(items)
}

// ---------------------------------------------------------------------------------------------------------------------
// Pure
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn constants_are_the_win32_values() {
    assert_eq!(MF_OWNERDRAW, 0x100);
    assert_eq!(MIM_BACKGROUND, 2);
    assert_eq!(MIM_APPLY_TO_SUBMENUS, 0x8000_0000);
}

#[test]
fn command_ids_assigns_one_based_ids_depth_first_skips_separators_and_submenus_with_children() {
    let items = vec![
        command("A", "Alpha"),
        separator(),
        submenu(
            "Zoom",
            "Zoom",
            vec![command("Z1", "one"), separator(), command("Z2", "two")],
        ),
        command("B", "Beta"),
    ];
    assert_eq!(ids(&items), ["A", "Z1", "Z2", "B"]);
}

#[test]
fn command_ids_nested_submenus_are_flattened_in_order() {
    let items = vec![
        submenu(
            "Outer",
            "Outer",
            vec![
                command("O1", "o1"),
                submenu("Inner", "Inner", vec![command("I1", "i1"), command("I2", "i2")]),
                command("O2", "o2"),
            ],
        ),
        command("Tail", "tail"),
    ];
    assert_eq!(ids(&items), ["O1", "I1", "I2", "O2", "Tail"]);
}

#[test]
fn command_ids_a_submenu_without_children_is_a_command() {
    let items = vec![
        command("A", "Alpha"),
        submenu("Empty", "Empty", Vec::new()),
        command("B", "Beta"),
    ];
    assert_eq!(ids(&items), ["A", "Empty", "B"]);
}

#[test]
fn command_ids_of_nothing_or_only_separators_is_empty() {
    assert!(ids(&[]).is_empty());
    assert!(ids(&[separator(), separator()]).is_empty());
}

#[test]
fn command_ids_keeps_disabled_items() {
    // A disabled item still owns its id (the OS never returns it); the numbering must not shift.
    let items = vec![command("A", "Alpha"), disabled("Off", "Off"), command("B", "Beta")];
    assert_eq!(ids(&items), ["A", "Off", "B"]);
}

#[test]
fn to_screen_pixels_scales_the_dip_point_by_the_window_dpi() {
    let cases: [(f64, f64, u32, i32, i32); 6] = [
        (100.0, 50.0, 96, 100, 50),
        (100.0, 50.4, 120, 125, 63),
        (100.0, 50.0, 144, 150, 75),
        (100.4, 50.4, 192, 201, 101),
        (-300.0, 8.0, 120, -375, 10),
        (100.0, 50.0, 0, 100, 50),
    ];
    for (x, y, dpi, expected_x, expected_y) in cases {
        assert_eq!(
            NativePopupMenuHost::to_screen_pixels(at(x, y), dpi),
            (expected_x, expected_y),
            "({x}, {y}) at {dpi} dpi"
        );
    }
}

#[test]
fn to_screen_pixels_rounds_halves_to_even() {
    // .NET `Math.Round` default: 0.5 -> 0, 1.5 -> 2, 2.5 -> 2.
    assert_eq!(NativePopupMenuHost::to_screen_pixels(at(0.5, 1.5), 96), (0, 2));
    assert_eq!(NativePopupMenuHost::to_screen_pixels(at(2.5, -0.5), 96), (2, 0));
}

#[test]
fn to_screen_pixels_never_wraps_outside_the_i32_range() {
    let (x, y) = NativePopupMenuHost::to_screen_pixels(at(1.0e12, -1.0e12), 96);
    assert_eq!((x, y), (i32::MAX, i32::MIN));
    assert_eq!(NativePopupMenuHost::to_screen_pixels(at(f64::NAN, 3.0), 96), (0, 3));
}

#[test]
fn show_with_no_items_makes_no_os_call() {
    let asked = Rc::new(Cell::new(0_u32));
    let counter = Rc::clone(&asked);
    // A handle that is not a window: if `show` reached the OS it would fail or misbehave. The owner closure is asked once.
    let host = NativePopupMenuHost::new(Box::new(move || {
        counter.set(counter.get() + 1);
        0x7FFF_FFF0
    }));
    let created = Rc::new(Cell::new(0_u32));
    let seen = Rc::clone(&created);
    host.set_menu_created(Some(Box::new(move |_| seen.set(seen.get() + 1))));
    assert_eq!(host.show(&[], at(0.0, 0.0)).expect("no error"), None);
    assert_eq!(asked.get(), 1);
    assert_eq!(created.get(), 0, "no menu was created");
}

#[test]
fn show_with_no_owner_window_makes_no_os_call() {
    let created = Rc::new(Cell::new(0_u32));
    let seen = Rc::clone(&created);
    let host = NativePopupMenuHost::new(Box::new(|| 0));
    host.set_menu_created(Some(Box::new(move |_| seen.set(seen.get() + 1))));
    assert_eq!(
        host.show(&[command("A", "Alpha")], at(0.0, 0.0)).expect("no error"),
        None
    );
    assert_eq!(created.get(), 0);
}

#[test]
fn debug_names_the_host_without_the_closures() {
    let host = NativePopupMenuHost::new(Box::new(|| 0));
    let text = format!("{host:?}");
    assert!(text.contains("NativePopupMenuHost"), "{text}");
    assert!(text.contains("showing: false"), "{text}");
}

// ---------------------------------------------------------------------------------------------------------------------
// Native helpers
// ---------------------------------------------------------------------------------------------------------------------

fn to_hwnd(hwnd: Hwnd) -> HWND {
    HWND(hwnd as *mut core::ffi::c_void)
}

fn post_key(owner: Hwnd, key: usize) {
    // SAFETY: a plain message post to a window of this process; the OS validates the handle.
    unsafe { PostMessageW(Some(to_hwnd(owner)), WM_KEYDOWN, WPARAM(key), LPARAM(1)) }.expect("post a key to the owner");
}

fn post_keys(owner: Hwnd, keys: &[usize]) {
    for key in keys {
        post_key(owner, *key);
    }
}

/// Posts Escape to `owner` from another thread after `timeout` unless disarmed: a menu that did not take its keys is closed instead of hanging the run.
struct Watchdog {
    armed: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Watchdog {
    fn arm(owner: Hwnd, timeout: Duration) -> Self {
        let armed = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&armed);
        let thread = thread::spawn(move || {
            let deadline = Instant::now() + timeout;
            while Instant::now() < deadline {
                if !flag.load(Ordering::SeqCst) {
                    return;
                }
                thread::sleep(Duration::from_millis(50));
            }
            // Escape closes a menu and a submenu level at a time: a few are harmless.
            while flag.load(Ordering::SeqCst) {
                // SAFETY: a plain message post; the owner may be gone by now (then the post fails and is ignored).
                let _ = unsafe { PostMessageW(Some(to_hwnd(owner)), WM_KEYDOWN, WPARAM(VK_ESCAPE), LPARAM(1)) };
                thread::sleep(Duration::from_millis(100));
            }
        });
        Self {
            armed,
            thread: Some(thread),
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.armed.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

const GUARD: Duration = Duration::from_secs(20);

fn is_menu(menu: Hmenu) -> bool {
    // SAFETY: a handle value; `IsMenu` validates it and answers false for anything that is not a live menu.
    unsafe {
        IsMenu(windows::Win32::UI::WindowsAndMessaging::HMENU(
            menu as *mut core::ffi::c_void,
        ))
    }
    .as_bool()
}

fn gui_objects() -> u32 {
    dialog_user32::get_gui_resources(kernel32::get_current_process(), 0)
}

fn user_objects() -> u32 {
    dialog_user32::get_gui_resources(kernel32::get_current_process(), 1)
}

/// Shows `items` on a fresh hidden owner after posting `keys`; every created `HMENU` is recorded.
fn show_with_keys(items: &[MenuItemModel], keys: &[usize]) -> (Option<String>, Vec<Hmenu>) {
    let owner = OwnerWindow::new();
    let hwnd = owner.hwnd;
    let host = NativePopupMenuHost::new(Box::new(move || hwnd));
    let created = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&created);
    host.set_menu_created(Some(Box::new(move |menu| sink.borrow_mut().push(menu))));
    post_keys(hwnd, keys);
    let guard = Watchdog::arm(hwnd, GUARD);
    let result = host.show(items, at(10.0, 10.0)).expect("show");
    drop(guard);
    let handles = created.borrow().clone();
    (result, handles)
}

// ---------------------------------------------------------------------------------------------------------------------
// Native: keys (NativePopupMenuHostTests)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs desktop"]
fn show_escape_pressed_closes_and_returns_none() {
    let _serial = serial();
    let (result, _) = show_with_keys(&[command("A", "Alpha"), command("B", "Beta")], &[VK_ESCAPE]);
    assert_eq!(result, None);
}

#[test]
#[ignore = "native: needs desktop"]
fn show_down_then_return_returns_the_first_item_id() {
    let _serial = serial();
    let (result, _) = show_with_keys(&[command("A", "Alpha"), command("B", "Beta")], &[VK_DOWN, VK_RETURN]);
    assert_eq!(result.as_deref(), Some("A"));
}

#[test]
#[ignore = "native: needs desktop"]
fn show_second_item_returns_the_second_id() {
    let _serial = serial();
    let (result, _) = show_with_keys(
        &[command("A", "Alpha"), command("B", "Beta")],
        &[VK_DOWN, VK_DOWN, VK_RETURN],
    );
    assert_eq!(result.as_deref(), Some("B"));
}

#[test]
#[ignore = "native: needs desktop"]
fn show_disabled_item_chosen_with_keyboard_never_returns_its_id() {
    let _serial = serial();
    let items = [
        command("A", "Alpha"),
        separator(),
        disabled("Off", "Disabled"),
        command("B", "Beta"),
    ];
    let (result, _) = show_with_keys(&items, &[VK_DOWN, VK_DOWN, VK_RETURN]);
    assert_eq!(result, None);
}

#[test]
#[ignore = "native: needs desktop"]
fn show_submenu_child_chosen_with_keyboard_returns_the_child_id() {
    let _serial = serial();
    let items = [submenu("Zoom", "Zoom", vec![command("Zoom.Custom", "Custom\u{2026}")])];
    let (result, _) = show_with_keys(&items, &[VK_DOWN, VK_RIGHT, VK_DOWN, VK_RETURN]);
    assert_eq!(result.as_deref(), Some("Zoom.Custom"));
}

// ---------------------------------------------------------------------------------------------------------------------
// Native: handles (NativeContextMenuHostTests) and the contract's extra cases
// ---------------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs desktop"]
fn show_cancelled_destroys_the_root_and_the_submenu_handles() {
    let _serial = serial();
    let (result, created) = show_with_keys(&model_with_submenu(), &[VK_ESCAPE]);
    assert_eq!(result, None);
    assert_eq!(created.len(), 2, "root + the Zoom submenu");
    for handle in created {
        assert!(
            !is_menu(handle),
            "HMENU {handle:#x} is still alive after the menu closed"
        );
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn show_item_chosen_destroys_the_handles_too() {
    let _serial = serial();
    let (result, created) = show_with_keys(&model_with_submenu(), &[VK_DOWN, VK_RETURN]);
    assert_eq!(result.as_deref(), Some("A"));
    assert_eq!(created.len(), 2);
    for handle in created {
        assert!(!is_menu(handle));
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn show_the_created_hook_sees_the_menus_alive_while_the_menu_is_open() {
    let _serial = serial();
    let owner = OwnerWindow::new();
    let hwnd = owner.hwnd;
    let host = NativePopupMenuHost::new(Box::new(move || hwnd));
    let alive = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&alive);
    host.set_menu_created(Some(Box::new(move |menu| sink.borrow_mut().push(is_menu(menu)))));
    post_keys(hwnd, &[VK_ESCAPE]);
    let guard = Watchdog::arm(hwnd, GUARD);
    host.show(&model_with_submenu(), at(10.0, 10.0)).expect("show");
    drop(guard);
    assert_eq!(*alive.borrow(), [true, true]);
}

#[test]
#[ignore = "native: needs desktop"]
fn show_repeated_open_and_cancel_never_accumulates_live_menus() {
    let _serial = serial();
    let owner = OwnerWindow::new();
    let hwnd = owner.hwnd;
    let host = NativePopupMenuHost::new(Box::new(move || hwnd));
    let created = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&created);
    host.set_menu_created(Some(Box::new(move |menu| sink.borrow_mut().push(menu))));
    let model = model_with_submenu();
    // One warm-up cycle: the first menu loads shared system resources that are never released.
    post_keys(hwnd, &[VK_ESCAPE]);
    let guard = Watchdog::arm(hwnd, GUARD);
    host.show(&model, at(10.0, 10.0)).expect("show");
    drop(guard);
    created.borrow_mut().clear();
    let (gdi_before, user_before) = (gui_objects(), user_objects());
    for _ in 0..20 {
        post_keys(hwnd, &[VK_ESCAPE]);
        let guard = Watchdog::arm(hwnd, GUARD);
        host.show(&model, at(10.0, 10.0)).expect("show");
        drop(guard);
    }
    let handles = created.borrow().clone();
    assert_eq!(handles.len(), 40);
    assert_eq!(handles.iter().filter(|handle| is_menu(**handle)).count(), 0);
    assert_eq!(gui_objects(), gdi_before, "GDI objects grew over 20 open/cancel cycles");
    assert_eq!(
        user_objects(),
        user_before,
        "USER objects grew over 20 open/cancel cycles"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn show_while_showing_returns_none() {
    let _serial = serial();
    let owner = OwnerWindow::new();
    let hwnd = owner.hwnd;
    let host = Rc::new(NativePopupMenuHost::new(Box::new(move || hwnd)));
    let inner_result = Rc::new(RefCell::new(None));
    let slot = Rc::clone(&inner_result);
    let weak = Rc::downgrade(&host);
    host.set_menu_created(Some(Box::new(move |_| {
        if let Some(host) = weak.upgrade() {
            // Called while the first `show` is populating: `showing` is already set.
            *slot.borrow_mut() = Some(host.show(&[command("X", "Nested")], at(0.0, 0.0)).expect("nested"));
        }
    })));
    post_keys(hwnd, &[VK_DOWN, VK_RETURN]);
    let guard = Watchdog::arm(hwnd, GUARD);
    let outer = host.show(&[command("A", "Alpha")], at(10.0, 10.0)).expect("outer");
    drop(guard);
    assert_eq!(*inner_result.borrow(), Some(None), "the nested show answers None");
    assert_eq!(outer.as_deref(), Some("A"), "the outer menu is unaffected");
}

#[test]
#[ignore = "native: needs desktop"]
fn show_after_a_show_works_again() {
    // `showing` is cleared by the guard: a second menu opens.
    let _serial = serial();
    let owner = OwnerWindow::new();
    let hwnd = owner.hwnd;
    let host = NativePopupMenuHost::new(Box::new(move || hwnd));
    for _ in 0..2 {
        post_keys(hwnd, &[VK_DOWN, VK_RETURN]);
        let guard = Watchdog::arm(hwnd, GUARD);
        let result = host.show(&[command("A", "Alpha")], at(10.0, 10.0)).expect("show");
        drop(guard);
        assert_eq!(result.as_deref(), Some("A"));
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn a_panic_captured_during_the_modal_loop_is_resumed_after_the_menu_is_destroyed() {
    let _serial = serial();
    let owner = OwnerWindow::new();
    let hwnd = owner.hwnd;
    let host = NativePopupMenuHost::new(Box::new(move || hwnd));
    let created = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&created);
    // Stand-in for a panic caught by a window procedure while the menu is modal (the window procedure parks it for the owner of the native call).
    host.set_menu_created(Some(Box::new(move |menu| {
        sink.borrow_mut().push(menu);
        pending_panic::capture(Box::new("panic inside the modal loop"));
    })));
    post_keys(hwnd, &[VK_ESCAPE]);
    let guard = Watchdog::arm(hwnd, GUARD);
    let outcome = catch_unwind(AssertUnwindSafe(|| host.show(&model_with_submenu(), at(10.0, 10.0))));
    drop(guard);
    let payload = outcome.expect_err("the parked panic is resumed by show");
    assert_eq!(
        payload.downcast_ref::<&str>().copied(),
        Some("panic inside the modal loop")
    );
    assert!(!pending_panic::has_pending(), "the slot is empty afterwards");
    for handle in created.borrow().iter() {
        assert!(!is_menu(*handle), "the menu was destroyed before the panic was resumed");
    }
    // `showing` is not stuck.
    host.set_menu_created(None);
    post_keys(hwnd, &[VK_DOWN, VK_RETURN]);
    let guard = Watchdog::arm(hwnd, GUARD);
    let again = host.show(&[command("A", "Alpha")], at(10.0, 10.0)).expect("show");
    drop(guard);
    assert_eq!(again.as_deref(), Some("A"));
}

#[test]
#[ignore = "native: needs desktop"]
fn a_panic_in_the_created_hook_destroys_the_menu_and_clears_showing() {
    let _serial = serial();
    let owner = OwnerWindow::new();
    let hwnd = owner.hwnd;
    let host = NativePopupMenuHost::new(Box::new(move || hwnd));
    let created = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&created);
    host.set_menu_created(Some(Box::new(move |menu| {
        sink.borrow_mut().push(menu);
        panic!("hook exploded");
    })));
    let outcome = catch_unwind(AssertUnwindSafe(|| host.show(&model_with_submenu(), at(10.0, 10.0))));
    assert!(outcome.is_err());
    let handles = created.borrow().clone();
    assert_eq!(handles.len(), 1, "the panic came out of the root's hook");
    assert!(!is_menu(handles[0]), "the root was destroyed while unwinding");
    host.set_menu_created(None);
    post_keys(hwnd, &[VK_DOWN, VK_RETURN]);
    let guard = Watchdog::arm(hwnd, GUARD);
    let again = host.show(&[command("A", "Alpha")], at(10.0, 10.0)).expect("show");
    drop(guard);
    assert_eq!(again.as_deref(), Some("A"));
}

#[test]
#[ignore = "native: needs desktop"]
fn show_keeps_the_owner_window_procedure_installed() {
    // The owner-draw subclass is removed after the menu: the owner answers `WM_GETTEXTLENGTH` as a plain STATIC again (smoke check that the subclass
    // chain is intact, not a dangling pointer).
    let _serial = serial();
    let owner = OwnerWindow::new();
    let hwnd = owner.hwnd;
    let host = NativePopupMenuHost::new(Box::new(move || hwnd));
    for _ in 0..3 {
        post_keys(hwnd, &[VK_ESCAPE]);
        let guard = Watchdog::arm(hwnd, GUARD);
        host.show(&model_with_submenu(), at(10.0, 10.0)).expect("show");
        drop(guard);
    }
    // SAFETY: the owner is a live window of this thread.
    let length = unsafe {
        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            to_hwnd(hwnd),
            windows::Win32::UI::WindowsAndMessaging::WM_GETTEXTLENGTH,
            None,
            None,
        )
    };
    assert_eq!(length.0, "owner-draw-owner".len() as isize);
}

// ---------------------------------------------------------------------------------------------------------------------
// Native: pixels of the real popup (ShellMenuRenderingTests, popup-capture half)
// ---------------------------------------------------------------------------------------------------------------------

/// The pixels of a popup window as `0x00RRGGBB`.
struct Shot {
    pixels: Vec<u32>,
}

impl Shot {
    fn count(&self, rgb: u32) -> usize {
        self.pixels.iter().filter(|p| **p == rgb).count()
    }

    fn light(&self) -> usize {
        self.pixels.iter().filter(|p| luma(**p) > 0.6).count()
    }

    fn most_common(&self) -> u32 {
        let mut counts = std::collections::HashMap::new();
        for pixel in &self.pixels {
            *counts.entry(*pixel).or_insert(0_usize) += 1;
        }
        counts
            .into_iter()
            .max_by_key(|(pixel, count)| (*count, *pixel))
            .map_or(0, |(pixel, _)| pixel)
    }

    fn max_luma(&self) -> f64 {
        self.pixels.iter().map(|p| luma(*p)).fold(0.0, f64::max)
    }
}

fn luma(rgb: u32) -> f64 {
    (0.2126 * f64::from((rgb >> 16) & 0xFF) + 0.7152 * f64::from((rgb >> 8) & 0xFF) + 0.0722 * f64::from(rgb & 0xFF))
        / 255.0
}

/// A `COLORREF` (`0x00BBGGRR`) as `0x00RRGGBB`.
fn rgb_of(color_ref: u32) -> u32 {
    ((color_ref & 0xFF) << 16) | (color_ref & 0xFF00) | ((color_ref >> 16) & 0xFF)
}

fn find_menu_window() -> Option<HWND> {
    // SAFETY: a class-name lookup on the desktop of this thread.
    unsafe { FindWindowW(w!("#32768"), None) }
        .ok()
        .filter(|h| !h.is_invalid())
}

/// `PrintWindow` of the popup into a 32-bit DIB.
fn capture(hwnd: HWND) -> Option<Shot> {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a live local the OS fills.
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    if width <= 0 || height <= 0 {
        return None;
    }
    // SAFETY: the screen DC is only used to create compatible objects; every object created here is released before returning.
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let old = SelectObject(mem, bitmap.into());
        let mut result = None;
        // `PW_RENDERFULLCONTENT` = 2.
        if PrintWindow(hwnd, mem, 2).as_bool() {
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = vec![0_u32; (width * height) as usize];
            let lines = GetDIBits(
                mem,
                bitmap,
                0,
                height as u32,
                Some(bits.as_mut_ptr().cast()),
                &mut info,
                windows::Win32::Graphics::Gdi::DIB_RGB_COLORS,
            );
            if lines != 0 {
                for pixel in &mut bits {
                    *pixel &= 0x00FF_FFFF;
                }
                result = Some(Shot { pixels: bits });
            }
        }
        SelectObject(mem, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        result
    }
}

/// While the menu is open: waits for the popup, posts `keys` to the owner, captures until `ready` (or the deadline), then closes the menu with Escape
/// and returns the last shot. Runs on a helper thread; `owner` is only used as a message target.
fn capture_while_open(
    owner: Hwnd,
    keys: Vec<usize>,
    ready: impl Fn(&Shot) -> bool + Send + 'static,
) -> thread::JoinHandle<Option<Shot>> {
    thread::spawn(move || {
        let deadline = Instant::now() + GUARD;
        let escape = || {
            // SAFETY: a plain message post; the owner outlives the menu.
            let _ = unsafe { PostMessageW(Some(to_hwnd(owner)), WM_KEYDOWN, WPARAM(VK_ESCAPE), LPARAM(1)) };
        };
        let mut popup = None;
        while Instant::now() < deadline {
            popup = find_menu_window();
            if popup.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let mut last = None;
        if let Some(popup) = popup {
            for key in keys {
                post_key(owner, key);
            }
            while Instant::now() < deadline {
                if let Some(shot) = capture(popup) {
                    let done = ready(&shot);
                    last = Some(shot);
                    if done {
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
        // Close the menu (the popup may need more than one Escape: a submenu level).
        let closing = Instant::now() + Duration::from_secs(10);
        while find_menu_window().is_some() && Instant::now() < closing {
            escape();
            thread::sleep(Duration::from_millis(100));
        }
        last
    })
}

fn capture_items() -> Vec<MenuItemModel> {
    let mut with_shortcut = command("Undo", "Undo last action");
    with_shortcut.shortcut_text = Some("Ctrl+Z".to_owned());
    let mut checked = command("Toggle", "Also set as click zoom level");
    checked.is_checked = true;
    vec![
        with_shortcut,
        command("Copy", "Copy file name"),
        separator(),
        submenu("Zoom", "Zoom", vec![command("Zoom.Preset.100", "100 %"), checked]),
        disabled("Off", "Disabled item"),
        command("Last", "Open in external editor"),
    ]
}

fn show_and_capture(keys: Vec<usize>, ready: impl Fn(&Shot) -> bool + Send + 'static) -> Shot {
    let owner = OwnerWindow::new();
    let hwnd = owner.hwnd;
    let host = NativePopupMenuHost::new(Box::new(move || hwnd));
    let capturer = capture_while_open(hwnd, keys, ready);
    // Modal: returns when the helper thread has closed the popup with Escape (the watchdog is the same mechanism at a later deadline).
    let guard = Watchdog::arm(hwnd, GUARD + Duration::from_secs(15));
    let result = host.show(&capture_items(), at(10.0, 10.0)).expect("show");
    drop(guard);
    assert_eq!(result, None, "the menu was closed with Escape");
    capturer
        .join()
        .expect("capture thread")
        .expect("the popup was never captured (no #32768 window, or PrintWindow failed)")
}

fn assert_dark_readable(shot: &Shot) {
    let dominant = shot.most_common();
    assert!(luma(dominant) < 0.25, "the menu background is not dark: {dominant:06X}");
    assert_eq!(
        dominant,
        rgb_of(background()),
        "the background is the app's own palette colour, not the system theme"
    );
    let light = shot.light();
    assert!(light >= 30, "no light text visible (only {light} light pixels)");
    assert!(
        light < shot.pixels.len() / 5,
        "too many light pixels: a white background?"
    );
    assert!(
        shot.max_luma() - luma(dominant) > 0.5,
        "the text does not contrast enough with the background"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn popup_capture_context_menu_is_dark_with_readable_text_without_hover() {
    let _serial = serial();
    let shot = show_and_capture(Vec::new(), |shot| shot.light() >= 30);
    assert_dark_readable(&shot);
}

#[test]
#[ignore = "native: needs desktop"]
fn popup_capture_highlights_the_keyboard_selected_item_with_the_hover_colour_keeping_text_readable() {
    let _serial = serial();
    let hover = rgb_of(item_background(ODS_SELECTED));
    let shot = show_and_capture(vec![VK_DOWN], move |shot| shot.count(hover) > 1500);
    let hover_pixels = shot.count(hover);
    assert!(
        hover_pixels > 1500,
        "no hover-coloured selected item ({hover_pixels} pixels)"
    );
    assert!(shot.light() >= 30, "the text is not readable");
}
