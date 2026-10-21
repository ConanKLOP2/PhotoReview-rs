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

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use std::cell::{Cell, RefCell};
use std::fmt;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

use pr_app::input::primitives::RectD;
use windows::Win32::UI::Controls::TTTOOLINFOW;

use crate::host::error::HostError;
use crate::win32::structs::{Hwnd, Lparam, Wparam};

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
        todo!()
    }
}

impl NativeTooltipHost {
    /// C# constructor: `owner == 0` is `HostError::InvalidArgument` (C# `ArgumentException`); `InitCommonControlsEx({ 8, ICC_BAR_CLASSES })` (the C# constant
    /// named `IccWin95Classes` is the value `0x4`, which is `ICC_BAR_CLASSES`), then `CreateWindowEx(WS_EX_TOPMOST, "tooltips_class32", null,
    /// WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX, CW_USEDEFAULT x4, owner, 0, module, 0)`; a `0` handle is `HostError::Win32` ("Creating the tooltip control failed.").
    pub fn new(owner: Hwnd) -> Result<Self, HostError> {
        todo!()
    }

    /// C# `Handle`.
    pub fn handle(&self) -> Hwnd {
        todo!()
    }

    /// C# `ToolCount`: `TTM_GETTOOLCOUNT`, `0` after `dispose`.
    pub fn tool_count(&self) -> usize {
        todo!()
    }

    /// C# `SetInitialDelay(ms)`: `TTM_SETDELAYTIME(TTDT_INITIAL)`; tests use a small value.
    pub fn set_initial_delay(&self, milliseconds: i32) {
        todo!()
    }

    /// C# `IsShowing`: the control window is visible.
    pub fn is_showing(&self) -> bool {
        todo!()
    }

    /// C# `TextOf(index)`: `TTM_GETTEXT` of the tool at `index`; empty for an index outside the registered tools or after `dispose`.
    pub fn text_of(&self, index: usize) -> String {
        todo!()
    }

    /// C# `Info(index, bounds, dpi, text)`: the `TOOLINFOW` of tool `index` (id `index + 1`, flags 0, `hwnd` = owner, rectangle from [`tool_rect_pixels`]).
    /// `cbSize` is `size_of::<TTTOOLINFOW>()` (72 on x64, the full structure: the v6 control needs it).
    fn tool_info(&self, index: usize, bounds: RectD, dpi: f64, text: *mut u16) -> TTTOOLINFOW {
        todo!()
    }
}

impl OverlayTooltips for NativeTooltipHost {
    /// C# `Sync`. After `dispose`: nothing. `dpi_scale <= 0` is `1`. The same list at the same DPI: nothing (compared with record equality, element by
    /// element). Otherwise: `TTM_DELTOOL` for every old tool from the last to the first; when the DPI changed, store it and send
    /// `TTM_SETMAXTIPWIDTH(round_ties_even(MAX_WIDTH_DIP * dpi))`; store the new list; `TTM_ADDTOOL` for each (the control copies the text, so the
    /// temporary UTF-16 buffer is freed right after the call).
    fn sync(&self, tools: &[OverlayTooltip], dpi_scale: f64) {
        todo!()
    }

    /// C# `RelayMouse`: nothing without a control or without tools; else `TTM_RELAYEVENT` with a `MSG { hwnd: owner, message, wParam, lParam,
    /// pt.x = low word, pt.y = high word (both signed 16 bit) }`.
    fn relay_mouse(&self, message: u32, wparam: Wparam, lparam: Lparam) {
        todo!()
    }

    /// C# `Dispose`: `DestroyWindow(tip)` once, clears the tools.
    fn dispose(&self) {
        todo!()
    }
}

impl Drop for NativeTooltipHost {
    /// Same as `dispose` (C# relied on the caller; Rust frees the OS window on drop too).
    fn drop(&mut self) {
        todo!()
    }
}

/// C# `Info`: DIP bounds to physical pixels for `TTM_ADDTOOL`: `left = floor(x * dpi)`, `top = floor(y * dpi)`, `right = ceil((x + width) * dpi)`,
/// `bottom = ceil((y + height) * dpi)` (`(int)` casts: saturating in Rust, NaN gives 0). Pure; the C# test expects `(10, 8, 40, 30)` at `1.5` to give
/// `(15, 12, 75, 57)`.
pub fn tool_rect_pixels(bounds: RectD, dpi: f64) -> (i32, i32, i32, i32) {
    todo!()
}

/// C# `Same(tools)` plus the DPI test of `Sync`: true when nothing needs rebuilding (`dpi_scale` already normalized, same count, every element equal).
pub fn tools_unchanged(old: &[OverlayTooltip], old_dpi: f64, new: &[OverlayTooltip], new_dpi: f64) -> bool {
    todo!()
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
        todo!()
    }
}

impl DeferredTooltips {
    /// C# constructor: stores `create` and posts `create_now` at `UiPriority::Background` through `poster` (the job holds a `Weak` of the state).
    pub fn new(create: TooltipFactory, poster: &dyn LocalPoster) -> Self {
        todo!()
    }

    /// C# `IsCreated`: the real control exists.
    pub fn is_created(&self) -> bool {
        todo!()
    }

    /// C# `CreateNow` (private; the posted job calls it through the `Weak`): nothing when disposed, already created or failed. Else the factory runs; on
    /// `Ok` the control gets the pending list and DPI (`sync`) and the pending list is cleared; on `Err` `failed` is set (no tooltips, no panic).
    fn create_now(state: &Weak<DeferredState>) {
        todo!()
    }
}

impl OverlayTooltips for DeferredTooltips {
    /// C# `Sync`: nothing when disposed; forwards to the control when it exists, else remembers the list and DPI.
    fn sync(&self, tools: &[OverlayTooltip], dpi_scale: f64) {
        todo!()
    }

    /// C# `RelayMouse`: forwards when the control exists, else ignored.
    fn relay_mouse(&self, message: u32, wparam: Wparam, lparam: Lparam) {
        todo!()
    }

    /// C# `Dispose`: idempotent; disposes the control if it exists and drops it.
    fn dispose(&self) {
        todo!()
    }
}
