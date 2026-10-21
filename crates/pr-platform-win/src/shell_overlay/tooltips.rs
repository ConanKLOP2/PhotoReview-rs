//! Port of `PhotoReview.Shell.Win32/Overlay/NativeTooltipHost.cs` (191 lines) and `DeferredTooltips.cs` (79 lines), baseline `d92d6aae`. Unit W8-15g.
//!
//! - [`OverlayTooltip`] / [`OverlayTooltips`]: where the overlay's tooltips are shown (C# `OverlayTooltip`, `IOverlayTooltips`). Production is
//!   [`NativeTooltipHost`]; tests use a recording fake.
//! - [`NativeTooltipHost`]: the system tooltip control (`tooltips_class32`, `TTS_ALWAYSTIP | TTS_NOPREFIX`), one tool per rectangle (DIP bounds converted
//!   to physical pixels). It does not subclass the owner window: mouse messages are relayed with `TTM_RELAYEVENT` by the message handler (W8-15f).
//! - [`DeferredTooltips`]: creates the real control later, through a [`LocalPoster`] at `Background` priority, so loading comctl32 v6 and creating the
//!   window (about 3 to 4 ms) is not on the path to the first frame. A creation failure means "no tooltips", never a crash.
//!
//! [`LocalPoster`] is the one cross-unit seam of this file: `pr_core`'s `UiAction` is `Send`, but the deferred creation must reach `!Send` state, so the
//! main window (W8-13g) supplies a UI-thread poster for non-`Send` jobs (contract `contracts/W8-15g.md`, question Q1).

use core::ffi::c_void;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

use pr_app::input::primitives::RectD;
use pr_core::abstractions::UiPriority;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::UI::Controls::{TOOLTIP_FLAGS, TTTOOLINFOW};
use windows::core::PWSTR;

use crate::host::error::HostError;
use crate::win32::api::{kernel32, user32};
use crate::win32::comctl::common_controls;
use crate::win32::control_constants::{
    ICC_BAR_CLASSES, TTM_ADDTOOL, TTM_GETTOOLCOUNT, TTM_SETDELAYTIME, TTM_SETMAXTIPWIDTH, TTS_ALWAYSTIP, TTS_NOPREFIX,
};
use crate::win32::dialog_native::user32 as dialog_user32;
use crate::win32::messages::{WS_EX_TOPMOST, WS_POPUP};
use crate::win32::structs::{Hwnd, Lparam, Msg, Win32Error, Wparam, to_utf16z, utf16_to_string};

/// C# `NativeTooltipHost.MaxWidthDip`: the width in DIP at which a tooltip line wraps.
pub const MAX_WIDTH_DIP: f64 = 420.0;
/// C# `OverlayNative.TooltipClass`.
pub const TOOLTIP_CLASS: &str = "tooltips_class32";
/// C# `TtmDelTool` (`TTM_DELTOOLW`); absent from `win32::control_constants`.
pub const TTM_DELTOOL: u32 = 0x0433;
/// C# `TtmRelayEvent`.
pub const TTM_RELAYEVENT: u32 = 0x0407;
/// C# `TtmGetText` (`TTM_GETTEXTW`).
pub const TTM_GETTEXT: u32 = 0x0438;
/// `TTM_GETTOOLINFOW` (used by the C# test `Sync_ConvertsDipBoundsToPhysicalPixelsWithTheDpiScale`).
pub const TTM_GETTOOLINFO: u32 = 0x0435;
/// C# `TtdtInitial`.
pub const TTDT_INITIAL: usize = 3;
/// C# `TtdtAutoPop`.
pub const TTDT_AUTOPOP: usize = 2;

/// C# `OverlayTooltip(string Id, RectD Bounds, string Text)`: one tooltip region (DIP, client coordinates). Record equality.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayTooltip {
    pub id: String,
    pub bounds: RectD,
    pub text: String,
}

/// C# `IOverlayTooltips : IDisposable`. `&self` and interior mutability: one instance is shared by the overlay (which syncs regions) and the message
/// handler (which relays the mouse), both as `Rc<dyn OverlayTooltips>`; the control is re-entered by nested messages.
pub trait OverlayTooltips {
    /// C# `Sync(tools, dpiScale)`: replaces the whole region set (only visible, mouse-receiving regions). Calling it again with the same list and DPI is
    /// cheap (no OS call).
    fn sync(&self, tools: &[OverlayTooltip], dpi_scale: f64);

    /// C# `RelayMouse(message, wParam, lParam)`: hands one mouse message of the owner window to the control (`TTM_RELAYEVENT`).
    fn relay_mouse(&self, message: u32, wparam: Wparam, lparam: Lparam);

    /// C# `Dispose()`: destroys the control; idempotent; afterwards `sync` and `relay_mouse` do nothing.
    fn dispose(&self);
}

/// C# `NativeTooltipHost`. UI thread only (`!Send`, `PhantomData<*const ()>`).
pub struct NativeTooltipHost {
    /// C# `_owner`: the shell window; the tooltip is a popup window owned by it.
    owner: Hwnd,
    /// C# `_tip`: `0` after `dispose` (`Cell`: C# used `Interlocked.Exchange`).
    tip: Cell<Hwnd>,
    /// C# `_tools`: the regions of the last real `sync`.
    tools: RefCell<Vec<OverlayTooltip>>,
    /// C# `_dpi` (`NaN` = never synced, so the first sync always rebuilds and sets the maximum width).
    dpi: Cell<f64>,
    _not_send: PhantomData<*const ()>,
}

impl fmt::Debug for NativeTooltipHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeTooltipHost")
            .field("owner", &self.owner)
            .field("tip", &self.tip.get())
            .field("tools", &self.tools.borrow().len())
            .finish_non_exhaustive()
    }
}

impl NativeTooltipHost {
    /// C# constructor: `owner == 0` is `HostError::InvalidArgument` (C# `ArgumentException`); `InitCommonControlsEx({ 8, ICC_BAR_CLASSES })` (the C# constant
    /// named `IccWin95Classes` is the value `0x4`, which is `ICC_BAR_CLASSES`), then `CreateWindowEx(WS_EX_TOPMOST, "tooltips_class32", null,
    /// WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX, CW_USEDEFAULT x4, owner, 0, module, 0)`; a `0` handle is `HostError::Win32` ("Creating the tooltip control failed.").
    pub fn new(owner: Hwnd) -> Result<Self, HostError> {
        if owner == 0 {
            return Err(HostError::InvalidArgument {
                param: "owner",
                message: "A window handle is required.",
            });
        }
        // The result is ignored like in C#: a failure shows up as a failed `CreateWindowEx` below.
        let _registered = common_controls::init_common_controls_ex(ICC_BAR_CLASSES);
        let failed = |error: Win32Error| HostError::Win32 {
            operation: "Creating the tooltip control failed.",
            error,
        };
        let module = kernel32::get_module_handle().map_err(failed)?;
        // SAFETY: the class name is the system tooltip class (it has no creation parameter), `param` is 0, and `owner` is a window handle the caller owns
        // (the OS validates it); the control's window procedure only runs system code.
        let tip = unsafe {
            user32::create_window_ex(
                WS_EX_TOPMOST,
                TOOLTIP_CLASS,
                "",
                WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
                user32::CW_USEDEFAULT,
                user32::CW_USEDEFAULT,
                user32::CW_USEDEFAULT,
                user32::CW_USEDEFAULT,
                owner,
                0,
                module,
                0,
            )
        }
        .map_err(failed)?;
        Ok(Self {
            owner,
            tip: Cell::new(tip),
            tools: RefCell::new(Vec::new()),
            dpi: Cell::new(f64::NAN),
            _not_send: PhantomData,
        })
    }

    /// C# `Handle`.
    pub fn handle(&self) -> Hwnd {
        self.tip.get()
    }

    /// C# `ToolCount`: `TTM_GETTOOLCOUNT`, `0` after `dispose`.
    pub fn tool_count(&self) -> usize {
        let tip = self.tip.get();
        if tip == 0 {
            return 0;
        }
        // SAFETY: TTM_GETTOOLCOUNT takes no pointer (wParam and lParam are 0).
        let count = unsafe { dialog_user32::send_message(tip, TTM_GETTOOLCOUNT, 0, 0) };
        usize::try_from(count).unwrap_or(0)
    }

    /// C# `SetInitialDelay(ms)`: `TTM_SETDELAYTIME(TTDT_INITIAL)`; tests use a small value.
    pub fn set_initial_delay(&self, milliseconds: i32) {
        let tip = self.tip.get();
        if tip == 0 {
            return;
        }
        // SAFETY: TTM_SETDELAYTIME takes the delay kind in wParam and the milliseconds in lParam; both are integers, no pointer.
        let _previous =
            unsafe { dialog_user32::send_message(tip, TTM_SETDELAYTIME, TTDT_INITIAL, milliseconds as Lparam) };
    }

    /// C# `IsShowing`: the control window is visible.
    pub fn is_showing(&self) -> bool {
        let tip = self.tip.get();
        tip != 0 && user32::is_window_visible(tip)
    }

    /// C# `TextOf(index)`: `TTM_GETTEXT` of the tool at `index`; empty for an index outside the registered tools or after `dispose`.
    pub fn text_of(&self, index: usize) -> String {
        let tip = self.tip.get();
        if tip == 0 || index >= self.tools.borrow().len() {
            return String::new();
        }
        let mut buffer = [0u16; TEXT_BUFFER_UNITS];
        let mut info = self.tool_info(index, RectD::default(), 1.0, buffer.as_mut_ptr());
        // SAFETY: TTM_GETTEXT takes the buffer size in wParam and a `TTTOOLINFOW` in lParam; `info` is a live local with `cbSize` set whose `lpszText`
        // points at `buffer`, a live local array of exactly `TEXT_BUFFER_UNITS` (512) units, which is the size passed.
        let _result = unsafe {
            dialog_user32::send_message(
                tip,
                TTM_GETTEXT,
                TEXT_BUFFER_UNITS,
                &mut info as *mut TTTOOLINFOW as Lparam,
            )
        };
        utf16_to_string(&buffer)
    }

    /// C# `Info(index, bounds, dpi, text)`: the `TOOLINFOW` of tool `index` (id `index + 1`, flags 0, `hwnd` = owner, rectangle from [`tool_rect_pixels`]).
    /// `cbSize` is `size_of::<TTTOOLINFOW>()` (72 on x64, the full structure: the v6 control needs it).
    fn tool_info(&self, index: usize, bounds: RectD, dpi: f64, text: *mut u16) -> TTTOOLINFOW {
        let (left, top, right, bottom) = tool_rect_pixels(bounds, dpi);
        TTTOOLINFOW {
            cbSize: size_of::<TTTOOLINFOW>() as u32,
            uFlags: TOOLTIP_FLAGS(0),
            hwnd: HWND(self.owner as *mut c_void),
            uId: index + 1,
            rect: RECT {
                left,
                top,
                right,
                bottom,
            },
            lpszText: PWSTR(text),
            ..TTTOOLINFOW::default()
        }
    }
}

impl OverlayTooltips for NativeTooltipHost {
    /// C# `Sync`. After `dispose`: nothing. `dpi_scale <= 0` is `1`. The same list at the same DPI: nothing (compared with record equality, element by
    /// element). Otherwise: `TTM_DELTOOL` for every old tool from the last to the first; when the DPI changed, store it and send
    /// `TTM_SETMAXTIPWIDTH(round_ties_even(MAX_WIDTH_DIP * dpi))`; store the new list; `TTM_ADDTOOL` for each (the control copies the text, so the
    /// temporary UTF-16 buffer is freed right after the call).
    fn sync(&self, tools: &[OverlayTooltip], dpi_scale: f64) {
        let tip = self.tip.get();
        if tip == 0 {
            return;
        }
        // `dpi_scale <= 0` (NaN is not `<= 0`, like in C#).
        let dpi = if dpi_scale <= 0.0 { 1.0 } else { dpi_scale };
        // No `RefCell` borrow is held across a `send_message`: the control can notify the owner while a tool is added (T6).
        let old_count = {
            let current = self.tools.borrow();
            if tools_unchanged(&current, self.dpi.get(), tools, dpi) {
                return;
            }
            current.len()
        };
        for index in (0..old_count).rev() {
            let mut old = self.tool_info(index, RectD::default(), 1.0, core::ptr::null_mut());
            // SAFETY: TTM_DELTOOL takes a `TTTOOLINFOW` in lParam; `old` is a live local with `cbSize`, `hwnd` and `uId` set (no text is read).
            let _removed =
                unsafe { dialog_user32::send_message(tip, TTM_DELTOOL, 0, &mut old as *mut TTTOOLINFOW as Lparam) };
        }
        if dpi != self.dpi.get() {
            self.dpi.set(dpi);
            let width = (MAX_WIDTH_DIP * dpi).round_ties_even() as isize;
            // SAFETY: TTM_SETMAXTIPWIDTH takes the width in lParam (an integer, no pointer).
            let _previous = unsafe { dialog_user32::send_message(tip, TTM_SETMAXTIPWIDTH, 0, width) };
        }
        *self.tools.borrow_mut() = tools.to_vec();
        for (index, tool) in tools.iter().enumerate() {
            // The control copies the text during the call; the buffer (cut at the first NUL by the control, like the C# marshalling) is dropped right after it.
            let mut text = to_utf16z(&tool.text);
            let mut info = self.tool_info(index, tool.bounds, dpi, text.as_mut_ptr());
            // SAFETY: TTM_ADDTOOL takes a `TTTOOLINFOW` in lParam; `info` is a live local with `cbSize` set whose `lpszText` points at `text`, a live
            // NUL-terminated buffer that outlives the call (the control copies it).
            let _added =
                unsafe { dialog_user32::send_message(tip, TTM_ADDTOOL, 0, &mut info as *mut TTTOOLINFOW as Lparam) };
        }
    }

    /// C# `RelayMouse`: nothing without a control or without tools; else `TTM_RELAYEVENT` with a `MSG { hwnd: owner, message, wParam, lParam,
    /// pt.x = low word, pt.y = high word (both signed 16 bit) }`.
    fn relay_mouse(&self, message: u32, wparam: Wparam, lparam: Lparam) {
        let tip = self.tip.get();
        if tip == 0 || self.tools.borrow().is_empty() {
            return;
        }
        let mut msg = Msg {
            hwnd: HWND(self.owner as *mut c_void),
            message,
            wParam: WPARAM(wparam),
            lParam: LPARAM(lparam),
            time: 0,
            pt: POINT {
                x: i32::from((lparam & 0xFFFF) as u16 as i16),
                y: i32::from(((lparam >> 16) & 0xFFFF) as u16 as i16),
            },
        };
        // SAFETY: TTM_RELAYEVENT takes a `MSG` in lParam; `msg` is a live local that outlives the synchronous call.
        let _relayed = unsafe { dialog_user32::send_message(tip, TTM_RELAYEVENT, 0, &mut msg as *mut Msg as Lparam) };
    }

    /// C# `Dispose`: `DestroyWindow(tip)` once, clears the tools.
    fn dispose(&self) {
        let tip = self.tip.replace(0);
        if tip != 0 {
            // An error is ignored like in C#.
            let _destroyed = user32::destroy_window(tip);
        }
        self.tools.borrow_mut().clear();
    }
}

impl Drop for NativeTooltipHost {
    /// Same as `dispose` (C# relied on the caller; Rust frees the OS window on drop too).
    fn drop(&mut self) {
        self.dispose();
    }
}

/// `TTM_GETTEXT` buffer size in UTF-16 units (C# `stackalloc char[512]`).
const TEXT_BUFFER_UNITS: usize = 512;

/// C# `Info`: DIP bounds to physical pixels for `TTM_ADDTOOL`: `left = floor(x * dpi)`, `top = floor(y * dpi)`, `right = ceil((x + width) * dpi)`,
/// `bottom = ceil((y + height) * dpi)` (`(int)` casts: saturating in Rust, NaN gives 0). Pure; the C# test expects `(10, 8, 40, 30)` at `1.5` to give
/// `(15, 12, 75, 57)`.
pub fn tool_rect_pixels(bounds: RectD, dpi: f64) -> (i32, i32, i32, i32) {
    (
        (bounds.x * dpi).floor() as i32,
        (bounds.y * dpi).floor() as i32,
        ((bounds.x + bounds.width) * dpi).ceil() as i32,
        ((bounds.y + bounds.height) * dpi).ceil() as i32,
    )
}

/// C# `Same(tools)` plus the DPI test of `Sync`: true when nothing needs rebuilding (`dpi_scale` already normalized, same count, every element equal).
pub fn tools_unchanged(old: &[OverlayTooltip], old_dpi: f64, new: &[OverlayTooltip], new_dpi: f64) -> bool {
    new_dpi == old_dpi && new.len() == old.len() && new.iter().zip(old).all(|(a, b)| a == b)
}

/// The one poster seam for non-`Send` UI-thread jobs is owned by W8-13g (`host::main_window::core`: the trait `LocalPoster`, the alias `LocalJob` and the
/// implementation `UiLocalPoster` over `Win32UiDispatcher` plus a UI-thread mailbox); this file re-exports it (D-W8B4X-3) instead of declaring a twin. Tests
/// implement it with a recording fake that runs jobs on demand.
pub use crate::host::main_window::core::{LocalJob, LocalPoster};

/// Creates the real tooltip control (C# `Func<IOverlayTooltips>`); runs on the UI thread, once.
pub type TooltipFactory = Box<dyn FnOnce() -> Result<Rc<dyn OverlayTooltips>, HostError>>;

/// State shared with the posted job (which holds only a `Weak`, so a dropped `DeferredTooltips` never keeps it alive).
struct DeferredState {
    /// C# `_create`: taken by the first `create_now`.
    create: RefCell<Option<TooltipFactory>>,
    /// C# `_inner`.
    inner: RefCell<Option<Rc<dyn OverlayTooltips>>>,
    /// C# `_failed`.
    failed: Cell<bool>,
    /// C# `_disposed`.
    disposed: Cell<bool>,
    /// C# `_pending`: the latest list given before the control existed.
    pending: RefCell<Vec<OverlayTooltip>>,
    /// C# `_pendingDpi` (initial `1`).
    pending_dpi: Cell<f64>,
}

/// C# `DeferredTooltips`.
pub struct DeferredTooltips {
    state: Rc<DeferredState>,
}

impl fmt::Debug for DeferredTooltips {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeferredTooltips")
            .field("created", &self.is_created())
            .field("failed", &self.state.failed.get())
            .field("disposed", &self.state.disposed.get())
            .finish_non_exhaustive()
    }
}

impl DeferredTooltips {
    /// C# constructor: stores `create` and posts `create_now` at `UiPriority::Background` through `poster` (the job holds a `Weak` of the state).
    pub fn new(create: TooltipFactory, poster: &dyn LocalPoster) -> Self {
        let state = Rc::new(DeferredState {
            create: RefCell::new(Some(create)),
            inner: RefCell::new(None),
            failed: Cell::new(false),
            disposed: Cell::new(false),
            pending: RefCell::new(Vec::new()),
            pending_dpi: Cell::new(1.0),
        });
        let weak = Rc::downgrade(&state);
        poster.post(UiPriority::Background, Box::new(move || Self::create_now(&weak)));
        Self { state }
    }

    /// C# `IsCreated`: the real control exists.
    pub fn is_created(&self) -> bool {
        self.state.inner.borrow().is_some()
    }

    /// C# `CreateNow` (private; the posted job calls it through the `Weak`): nothing when disposed, already created or failed. Else the factory runs; on
    /// `Ok` the control gets the pending list and DPI (`sync`) and the pending list is cleared; on `Err` `failed` is set (no tooltips, no panic).
    fn create_now(state: &Weak<DeferredState>) {
        let Some(state) = state.upgrade() else {
            return;
        };
        if state.disposed.get() || state.inner.borrow().is_some() || state.failed.get() {
            return;
        }
        // Taken out of its slot first: a second job cannot run the factory again, and no borrow is held while it runs.
        let Some(create) = state.create.borrow_mut().take() else {
            return;
        };
        match create() {
            Ok(created) => {
                *state.inner.borrow_mut() = Some(Rc::clone(&created));
                let pending = std::mem::take(&mut *state.pending.borrow_mut());
                created.sync(&pending, state.pending_dpi.get());
            }
            Err(_) => state.failed.set(true), // no tooltips rather than a crashed frame
        }
    }
}

impl OverlayTooltips for DeferredTooltips {
    /// C# `Sync`: nothing when disposed; forwards to the control when it exists, else remembers the list and DPI.
    fn sync(&self, tools: &[OverlayTooltip], dpi_scale: f64) {
        if self.state.disposed.get() {
            return;
        }
        let inner = self.state.inner.borrow().clone();
        if let Some(inner) = inner {
            inner.sync(tools, dpi_scale);
            return;
        }
        *self.state.pending.borrow_mut() = tools.to_vec();
        self.state.pending_dpi.set(dpi_scale);
    }

    /// C# `RelayMouse`: forwards when the control exists, else ignored.
    fn relay_mouse(&self, message: u32, wparam: Wparam, lparam: Lparam) {
        let inner = self.state.inner.borrow().clone();
        if let Some(inner) = inner {
            inner.relay_mouse(message, wparam, lparam);
        }
    }

    /// C# `Dispose`: idempotent; disposes the control if it exists and drops it.
    fn dispose(&self) {
        if self.state.disposed.replace(true) {
            return;
        }
        let inner = self.state.inner.borrow_mut().take();
        if let Some(inner) = inner {
            inner.dispose();
        }
    }
}
