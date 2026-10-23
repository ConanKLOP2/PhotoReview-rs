//! Shared desktop fixture for W8-18h..18k; port of NativeDialogTestKit.cs.
#![allow(dead_code)] // Different dialog integration tests use different parts of this shared kit.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use pr_platform_win::dialogs::native::chrome::{DialogChromeControl, NativeDialogChrome};
use pr_platform_win::dialogs::native::dialog::core::{DialogContent, DialogError, ListItemText, NativeDialog};
use pr_platform_win::dialogs::native::template::DialogItem;
use pr_platform_win::win32::structs::{Hgdi, Hwnd, Lparam, Lresult, Msg, Point, Rect, Wparam};
use pr_platform_win::win32::{activation_context::ActivationScope, api, comctl, dialog_native, messages};

pub const VK_RETURN: u32 = 0x0D;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_TAB: u32 = 0x09;
pub const SC_CLOSE: usize = 0xF060;
const BOUND: Duration = Duration::from_secs(60);

pub fn on_sta<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("native-dialog-test".into())
        .spawn(move || {
            // SAFETY: initializes COM for this test thread; CoUninitialize is paired on the same thread.
            let hr = unsafe {
                windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED)
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                assert!(hr.is_ok(), "COM STA initialization: {hr:?}");
                body()
            }));
            if hr.is_ok() {
                // SAFETY: this thread successfully initialized COM above.
                unsafe { windows::Win32::System::Com::CoUninitialize() };
            }
            let _ = tx.send(result);
        })
        .expect("spawn native dialog test thread");
    let result = rx
        .recv_timeout(BOUND)
        .expect("native dialog test thread did not finish in 60 seconds");
    thread.join().expect("join native dialog test thread");
    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

pub fn activate_visual_styles() -> ActivationScope {
    ActivationScope::from_manifest_text(comctl::SHELL_MANIFEST).expect("activate comctl32 v6")
}

pub fn create_owner() -> Hwnd {
    let instance = api::kernel32::get_module_handle().unwrap_or(0);
    // SAFETY: only literal class/title strings and integer values are passed; STATIC ignores the null creation parameter.
    unsafe {
        api::user32::create_window_ex(
            0,
            "STATIC",
            "dialog test owner",
            0x9000_0000,
            100,
            100,
            640,
            480,
            0,
            0,
            instance,
            0,
        )
    }
    .expect("create owner window")
}

pub fn destroy_owner(hwnd: Hwnd) {
    if hwnd != 0 && dialog_native::user32::is_window(hwnd) {
        api::user32::destroy_window(hwnd).expect("destroy owner");
    }
}

pub fn show_on_owner(dialog: &NativeDialog, ready: impl Fn(&NativeDialog, Hwnd)) -> Result<i32, DialogError> {
    let owner = create_owner();
    struct Owner(Hwnd);
    impl Drop for Owner {
        fn drop(&mut self) {
            destroy_owner(self.0);
        }
    }
    let _owner = Owner(owner);
    dialog.show_modal(owner, Some(&|d| ready(d, owner)), None)
}

pub fn item(dialog: &NativeDialog, id: i32) -> Hwnd {
    dialog_native::user32::get_dlg_item(dialog.hwnd(), id)
}
pub fn text_of(hwnd: Hwnd) -> String {
    dialog_native::user32::get_window_text(hwnd)
}
pub fn set_text(dialog: &NativeDialog, id: i32, text: &str) {
    dialog_native::user32::set_window_text(item(dialog, id), text).expect("set control text");
}
pub fn is_visible(dialog: &NativeDialog, id: i32) -> bool {
    api::user32::is_window_visible(item(dialog, id))
}
pub fn command(dialog: &NativeDialog, id: i32, notification: i32) -> Lresult {
    let wparam = (((notification as usize) << 16) | ((id as usize) & 0xFFFF)) as Wparam;
    // SAFETY: WM_COMMAND carries integers here, with a null control handle.
    unsafe { dialog_native::user32::send_message(dialog.hwnd(), messages::WM_COMMAND, wparam, 0) }
}
pub fn key(dialog: &NativeDialog, target: Hwnd, vk: u32) -> bool {
    let mut msg = Msg {
        hwnd: windows::Win32::Foundation::HWND(target as *mut _),
        message: messages::WM_KEYDOWN,
        wParam: windows::Win32::Foundation::WPARAM(vk as usize),
        lParam: windows::Win32::Foundation::LPARAM(1),
        ..Default::default()
    };
    // SAFETY: the MSG is a valid key event for a live test dialog control.
    unsafe { dialog_native::user32::is_dialog_message(dialog.hwnd(), &mut msg) }
}
pub fn drain(dialog: &NativeDialog) {
    let mut msg = Msg::default();
    while api::user32::peek_message(&mut msg, 0, 0, 0, messages::PM_REMOVE) {
        if msg.message == messages::WM_QUIT {
            api::user32::post_quit_message(msg.wParam.0 as i32);
            return;
        }
        // SAFETY: PeekMessage supplied this MSG, and the dialog is live while we claim it.
        if dialog.hwnd() != 0 && unsafe { dialog_native::user32::is_dialog_message(dialog.hwnd(), &mut msg) } {
            continue;
        }
        api::user32::translate_message(&msg);
        // SAFETY: PeekMessage supplied this MSG; its parameters are valid for dispatch.
        unsafe { api::user32::dispatch_message(&msg) };
    }
}
pub fn window_rect(hwnd: Hwnd) -> Rect {
    api::user32::get_window_rect(hwnd).expect("window rect")
}
pub fn client_rect(hwnd: Hwnd) -> Rect {
    api::user32::get_client_rect(hwnd).expect("client rect")
}
pub fn child_rect(dialog: &NativeDialog, id: i32) -> Rect {
    let rect = window_rect(item(dialog, id));
    let mut tl = Point {
        x: rect.left,
        y: rect.top,
    };
    let mut br = Point {
        x: rect.right,
        y: rect.bottom,
    };
    assert!(dialog_native::user32::screen_to_client(dialog.hwnd(), &mut tl));
    assert!(dialog_native::user32::screen_to_client(dialog.hwnd(), &mut br));
    Rect {
        left: tl.x,
        top: tl.y,
        right: br.x,
        bottom: br.y,
    }
}
pub fn width(rect: Rect) -> i32 {
    rect.right - rect.left
}
pub fn height(rect: Rect) -> i32 {
    rect.bottom - rect.top
}
pub fn font_height(font: Hgdi) -> i32 {
    dialog_native::gdi32::get_log_font(font).expect("font LOGFONT").lfHeight
}
pub fn needed_text_height(control: Hwnd) -> i32 {
    let text = text_of(control);
    let font = {
        // SAFETY: WM_GETFONT carries no pointers.
        unsafe { dialog_native::user32::send_message(control, dialog_native::WM_GETFONT, 0, 0) }
    };
    let dc = dialog_native::user32::get_dc(control);
    let old = dialog_native::gdi32::select_object(dc, font);
    let mut rect = Rect {
        right: width(window_rect(control)).max(1),
        ..Default::default()
    };
    dialog_native::user32::draw_text(
        dc,
        &text,
        &mut rect,
        dialog_native::DT_CALCRECT | dialog_native::DT_WORDBREAK | dialog_native::DT_NOPREFIX,
    );
    dialog_native::gdi32::select_object(dc, old);
    dialog_native::user32::release_dc(control, dc);
    height(rect)
}
pub fn gdi_objects() -> u32 {
    dialog_native::user32::get_gui_resources(
        dialog_native::kernel32::get_current_process(),
        dialog_native::GR_GDIOBJECTS as u32,
    )
}
pub fn user_objects() -> u32 {
    dialog_native::user32::get_gui_resources(
        dialog_native::kernel32::get_current_process(),
        dialog_native::GR_USEROBJECTS as u32,
    )
}
pub fn settles_at_or_below(read: impl Fn() -> u32, baseline: u32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if read() <= baseline {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::yield_now();
    }
}
pub fn has_quit_pending() -> Option<i32> {
    let mut msg = Msg::default();
    api::user32::peek_message(&mut msg, 0, messages::WM_QUIT, messages::WM_QUIT, messages::PM_REMOVE)
        .then_some(msg.wParam.0 as i32)
}

#[derive(Default)]
pub struct RecordingChrome {
    pub apply_calls: RefCell<Vec<(Hwnd, Vec<DialogChromeControl>)>>,
    pub apply_controls_calls: RefCell<Vec<(Hwnd, Vec<DialogChromeControl>)>>,
}
impl NativeDialogChrome for RecordingChrome {
    fn apply(&self, dialog: Hwnd, controls: &[DialogChromeControl]) {
        self.apply_calls.borrow_mut().push((dialog, controls.to_vec()));
    }
    fn apply_controls(&self, dialog: Hwnd, controls: &[DialogChromeControl]) {
        self.apply_controls_calls.borrow_mut().push((dialog, controls.to_vec()));
    }
}

pub type SizeHook = Box<dyn Fn(&NativeDialog) -> (i32, i32)>;
pub type LayoutHook = Box<dyn Fn(&NativeDialog, i32, i32)>;
pub type MessageHook = Box<dyn Fn(&NativeDialog, u32)>;
pub type ListTextHook = Box<dyn Fn(i32, i32) -> ListItemText>;
pub type SubtitleHook = Box<dyn Fn(i32, i32) -> Option<String>>;
pub type BadgeColorHook = Box<dyn Fn(i32, i32) -> Option<u32>>;
pub struct RecordingContent {
    pub title: String,
    pub items: RefCell<Vec<DialogItem>>,
    pub resizable: Cell<bool>,
    pub header_ids: RefCell<Vec<i32>>,
    pub initial_focus: Cell<i32>,
    pub default_button: Cell<i32>,
    pub preferred: RefCell<SizeHook>,
    pub minimum: RefCell<Option<SizeHook>>,
    pub layout: RefCell<LayoutHook>,
    pub ok_result: Cell<bool>,
    pub cancel_handled: Cell<bool>,
    pub can_close: Cell<bool>,
    pub wait_cursor: Cell<bool>,
    pub command_handled: Cell<bool>,
    pub notify_result: Cell<Option<isize>>,
    pub window_message_result: Cell<Option<isize>>,
    pub window_message_hook: RefCell<Option<MessageHook>>,
    pub dialog_key_handled: Cell<bool>,
    pub uses_modal_frame: Cell<bool>,
    pub extra_style: Cell<u32>,
    pub static_color: Cell<Option<u32>>,
    pub panic_in: Cell<Option<&'static str>>,
    pub list_text: RefCell<Option<ListTextHook>>,
    pub subtitle: RefCell<Option<SubtitleHook>>,
    pub badge_color: RefCell<Option<BadgeColorHook>>,
    pub events: RefCell<Vec<String>>,
    pub token: Rc<()>,
}
impl Default for RecordingContent {
    fn default() -> Self {
        Self {
            title: "test dialog".into(),
            items: RefCell::new(Vec::new()),
            resizable: Cell::new(false),
            header_ids: RefCell::new(Vec::new()),
            initial_focus: Cell::new(0),
            default_button: Cell::new(1),
            preferred: RefCell::new(Box::new(|d| (d.dx(200), d.dy(100)))),
            minimum: RefCell::new(None),
            layout: RefCell::new(Box::new(|_, _, _| {})),
            ok_result: Cell::new(true),
            cancel_handled: Cell::new(false),
            can_close: Cell::new(true),
            wait_cursor: Cell::new(false),
            command_handled: Cell::new(false),
            notify_result: Cell::new(None),
            window_message_result: Cell::new(None),
            window_message_hook: RefCell::new(None),
            dialog_key_handled: Cell::new(false),
            uses_modal_frame: Cell::new(true),
            extra_style: Cell::new(0),
            static_color: Cell::new(None),
            panic_in: Cell::new(None),
            list_text: RefCell::new(None),
            subtitle: RefCell::new(None),
            badge_color: RefCell::new(None),
            events: RefCell::new(Vec::new()),
            token: Rc::new(()),
        }
    }
}
impl RecordingContent {
    fn log(&self, hook: &'static str, event: String) {
        self.events.borrow_mut().push(event);
        if self.panic_in.get() == Some(hook) {
            panic!("hook panic: {hook}");
        }
    }
}
impl DialogContent for RecordingContent {
    fn title(&self) -> String {
        self.title.clone()
    }
    fn items(&self) -> Vec<DialogItem> {
        self.items.borrow().clone()
    }
    fn resizable(&self) -> bool {
        self.resizable.get()
    }
    fn header_control_ids(&self) -> Vec<i32> {
        self.header_ids.borrow().clone()
    }
    fn initial_focus_id(&self) -> i32 {
        self.initial_focus.get()
    }
    fn default_button_id(&self) -> i32 {
        self.default_button.get()
    }
    fn preferred_client_size(&self, d: &NativeDialog) -> (i32, i32) {
        (self.preferred.borrow())(d)
    }
    fn minimum_client_size(&self, d: &NativeDialog) -> (i32, i32) {
        self.minimum
            .borrow()
            .as_ref()
            .map_or_else(|| self.preferred_client_size(d), |f| f(d))
    }
    fn layout_controls(&self, d: &NativeDialog, w: i32, h: i32) {
        self.log("layout", format!("layout:{w}x{h}"));
        (self.layout.borrow())(d, w, h);
    }
    fn on_initialized(&self, _: &NativeDialog) {
        self.log("initialized", "init".into());
    }
    fn on_ok(&self, _: &NativeDialog) -> bool {
        self.log("ok", "ok".into());
        self.ok_result.get()
    }
    fn on_cancel(&self, _: &NativeDialog) -> bool {
        self.log("cancel", "cancel".into());
        self.cancel_handled.get()
    }
    fn on_command(&self, _: &NativeDialog, id: i32, notification: i32) -> bool {
        self.log("command", format!("command:{id}:{notification}"));
        self.command_handled.get()
    }
    fn on_notify(&self, _: &NativeDialog, id: i32, code: i32, _: Lparam) -> Option<Lresult> {
        self.log("notify", format!("notify:{id}:{code}"));
        self.notify_result.get()
    }
    fn on_scroll(&self, _: &NativeDialog, id: i32, code: i32) {
        self.log("scroll", format!("scroll:{id}:{code}"));
    }
    fn list_item_text(&self, d: &NativeDialog, id: i32, index: i32) -> ListItemText {
        self.log("item_text", format!("item_text:{id}:{index}"));
        self.list_text.borrow().as_ref().map_or_else(
            || ListItemText {
                text: pr_platform_win::dialogs::native::controls::basic::ListBoxControl::text_at(d.item(id), index),
                badge: None,
            },
            |f| f(id, index),
        )
    }
    fn list_item_subtitle(&self, _: &NativeDialog, id: i32, index: i32) -> Option<String> {
        self.log("item_subtitle", format!("item_subtitle:{id}:{index}"));
        self.subtitle.borrow().as_ref().and_then(|f| f(id, index))
    }
    fn list_item_badge_color(&self, _: &NativeDialog, id: i32, index: i32) -> Option<u32> {
        self.log("item_badge", format!("item_badge:{id}:{index}"));
        self.badge_color.borrow().as_ref().and_then(|f| f(id, index))
    }
    fn can_close_by_user(&self) -> bool {
        self.can_close.get()
    }
    fn wait_cursor(&self) -> bool {
        self.wait_cursor.get()
    }
    fn on_closed(&self, _: &NativeDialog) {
        self.log("closed", "closed".into());
    }
    fn on_window_message(&self, dialog: &NativeDialog, m: u32, _: Wparam, _: Lparam) -> Option<Lresult> {
        self.log("message", format!("message:{m:x}"));
        if let Some(hook) = self.window_message_hook.borrow().as_ref() {
            hook(dialog, m);
        }
        self.window_message_result.get()
    }
    fn on_dialog_key(&self, _: &NativeDialog, message: &Msg) -> bool {
        self.log("key", "key".into());
        matches!(
            message.message,
            messages::WM_KEYDOWN | messages::WM_KEYUP | messages::WM_SYSKEYDOWN | messages::WM_SYSKEYUP
        ) && self.dialog_key_handled.get()
    }
    fn static_text_color(&self, _: &NativeDialog, _: i32) -> u32 {
        self.static_color
            .get()
            .unwrap_or_else(pr_platform_win::dialogs::native::chrome::DialogColors::text)
    }
    fn extra_window_style(&self) -> u32 {
        self.extra_style.get()
    }
    fn uses_modal_frame(&self) -> bool {
        self.uses_modal_frame.get()
    }
}
