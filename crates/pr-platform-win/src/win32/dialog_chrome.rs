//! Port of `PhotoReview.Shell.Interop/Win32/DialogChromeNative.cs` (baseline `d92d6aae`, 143 lines): the user32/gdi32 calls and constants the hand-drawn dark dialog
//! chrome needs (`WM_PAINT`/`WM_NCPAINT` of buttons, check boxes, progress bars, trackbars and edit borders): `BeginPaint`/`EndPaint`, `TrackMouseEvent`, a memory DC
//! and `BitBlt` for flicker-free drawing, `Ellipse`/`Polyline`, and the window-property calls that mark a control as attached.
//!
//! Unit W8-11d (plan: `win32::dialog_native`; D-W8B3-5 keeps this class in its own file because C# keeps it in its own class). The C# comment "window property
//! instead of `GetWindowSubclass`: the test host loads comctl32 v5" is why the attached flag is a window property; the Rust port keeps that.
#![allow(unused_variables, dead_code)] // stub: removed by the W8-11d porter when the last `todo!()` is gone.

use super::structs::{Hdc, Hgdi, Hwnd, Point};

// Repeated in the C# class, same value as `messages`: re-exported, not redefined.
pub use super::messages::{WM_ERASEBKGND, WM_KILLFOCUS, WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT, WM_SETFOCUS};

pub const SRCCOPY: u32 = 0x00CC0020; // SrcCopy
pub const PAINTSTRUCT_SIZE: i32 = 72; // PaintStructSize
pub const TRACKMOUSEEVENT_SIZE: i32 = 24; // TrackMouseEventSize
pub const TME_LEAVE: u32 = 2; // TmeLeave
pub const WM_NCPAINT: u32 = 0x0085; // WmNcPaint
pub const WM_PRINT: u32 = 0x0317; // WmPrint
pub const WM_PRINTCLIENT: u32 = 0x0318; // WmPrintClient
pub const WM_MOUSELEAVE: u32 = 0x02A3; // WmMouseLeave
pub const WM_QUERYUISTATE: u32 = 0x0129; // WmQueryUiState
pub const WM_UPDATEUISTATE: u32 = 0x0128; // WmUpdateUiState
pub const PRF_NONCLIENT: u32 = 2; // PrfNonClient
pub const UISF_HIDEFOCUS: u32 = 1; // UisfHideFocus
pub const UISF_HIDEACCEL: u32 = 2; // UisfHideAccel
pub const BM_GETSTATE: u32 = 0x00F2; // BmGetState
pub const BST_PUSHED: u32 = 4; // BstPushed
pub const BST_FOCUS: u32 = 8; // BstFocus
pub const BS_TYPEMASK: u32 = 0x000F; // BsTypeMask
pub const BS_PUSHLIKE: u32 = 0x1000; // BsPushLike
pub const BS_MULTILINE: u32 = 0x2000; // BsMultiline
pub const BS_LEFTTEXT: u32 = 0x0020; // BsLeftText
pub const BS_ICON: u32 = 0x0040; // BsIcon
pub const BS_BITMAP: u32 = 0x0080; // BsBitmap
pub const CBS_TYPE_MASK: u32 = 3; // CbsTypeMask
pub const CBS_OWNERDRAWFIXED: u32 = 0x0010; // CbsOwnerDrawFixed
pub const CBS_OWNERDRAWVARIABLE: u32 = 0x0020; // CbsOwnerDrawVariable
pub const PBM_GETRANGE: u32 = 0x0407; // PbmGetRange
pub const PBS_MARQUEE: u32 = 8; // PbsMarquee
pub const TBS_VERT: u32 = 2; // TbsVert
pub const TBM_GETNUMTICS: u32 = 0x0410; // TbmGetNumTics
pub const TBM_GETPTICS: u32 = 0x040E; // TbmGetPTics
pub const TBM_GETTHUMBRECT: u32 = 0x0419; // TbmGetThumbRect
pub const TBM_GETCHANNELRECT: u32 = 0x041A; // TbmGetChannelRect
pub const TBM_GETRANGEMIN: u32 = 0x0401; // TbmGetRangeMin
pub const TBM_GETRANGEMAX: u32 = 0x0402; // TbmGetRangeMax
pub const SS_TYPEMASK: u32 = 0x001F; // SsTypeMask
pub const SS_ETCHEDHORZ: u32 = 0x0010; // SsEtchedHorz
pub const SS_ETCHEDVERT: u32 = 0x0011; // SsEtchedVert
pub const SS_ETCHEDFRAME: u32 = 0x0012; // SsEtchedFrame
pub const HDM_GETITEMCOUNT: u32 = 0x1200; // HdmGetItemCount
pub const HDM_GETITEMRECT: u32 = 0x1207; // HdmGetItemRect
pub const LVM_GETBKCOLOR: u32 = 0x1000; // LvmGetBkColor
pub const CDIS_FOCUS: u32 = 0x0010; // CdisFocus
pub const CDIS_HOT: u32 = 0x0040; // CdisHot
pub const WS_BORDER: u32 = 0x00800000; // WsBorder
pub const WS_DLGFRAME: u32 = 0x00400000; // WsDlgFrame
pub const WS_EX_CLIENTEDGE: u32 = 0x0200; // WsExClientEdge
pub const WS_EX_STATICEDGE: u32 = 0x00020000; // WsExStaticEdge
pub const SM_CXBORDER: i32 = 5; // SmCxBorder
pub const SM_CXEDGE: i32 = 45; // SmCxEdge
pub const DT_HIDEPREFIX: u32 = 0x00100000; // DtHidePrefix
pub const DT_RIGHT: u32 = 2; // DtRight

/// `BeginPaint`: the device context and the `PAINTSTRUCT` (72 bytes, `PAINTSTRUCT_SIZE`) to give back to `end_paint`.
#[derive(Debug, Clone, Copy)]
pub struct PaintSession {
    /// The painting DC.
    pub hdc: Hdc,
    /// The structure `EndPaint` needs.
    pub paint: windows::Win32::Graphics::Gdi::PAINTSTRUCT,
}

/// C# `BeginPaint(hwnd, byte* paintStruct)`: `None` when the call fails (no display context). Only valid while handling `WM_PAINT` of `hwnd`.
pub fn begin_paint(hwnd: Hwnd) -> Option<PaintSession> {
    todo!()
}

/// C# `EndPaint(hwnd, byte* paintStruct)`: always `true` in the OS; pair every successful `begin_paint` with one `end_paint` of the same window.
pub fn end_paint(hwnd: Hwnd, session: &PaintSession) -> bool {
    todo!()
}

/// C# `GetWindowDC`: the DC of the whole window, non-client area included (for `WM_NCPAINT`); release it with `dialog_native::user32::release_dc`.
pub fn get_window_dc(hwnd: Hwnd) -> Hdc {
    todo!()
}

/// C# `TrackMouseEvent(byte* track)` with a `TRACKMOUSEEVENT` (24 bytes, `TRACKMOUSEEVENT_SIZE`): the wrapper sets `cbSize`.
pub fn track_mouse_event(flags: u32, hwnd: Hwnd, hover_time: u32) -> bool {
    todo!()
}

/// C# `CreateCompatibleDC`: `0` on failure; delete it with `delete_dc`.
pub fn create_compatible_dc(hdc: Hdc) -> Hdc {
    todo!()
}

/// C# `CreateCompatibleBitmap`: `0` on failure; delete it with `dialog_native::gdi32::delete_object`.
pub fn create_compatible_bitmap(hdc: Hdc, width: i32, height: i32) -> Hgdi {
    todo!()
}

/// C# `DeleteDC` (a DC from `create_compatible_dc`, not a window DC).
pub fn delete_dc(hdc: Hdc) -> bool {
    todo!()
}

/// C# `BitBlt(dest, x, y, w, h, source, sx, sy, rop)` (`SRCCOPY`).
#[allow(clippy::too_many_arguments)] // mirrors BitBlt 1:1
pub fn bit_blt(
    destination: Hdc,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    source: Hdc,
    source_x: i32,
    source_y: i32,
    rop: u32,
) -> bool {
    todo!()
}

/// C# `Ellipse`.
pub fn ellipse(hdc: Hdc, left: i32, top: i32, right: i32, bottom: i32) -> bool {
    todo!()
}

/// C# `Polyline(hdc, Point*, count)`; the count is `points.len()` (the OS needs at least 2 points, else it fails).
pub fn polyline(hdc: Hdc, points: &[Point]) -> bool {
    todo!()
}

/// C# `SetProp(hwnd, name, value)`: attaches `value` under the property name (the "chrome attached" flag).
pub fn set_prop(hwnd: Hwnd, name: &str, value: isize) -> bool {
    todo!()
}

/// C# `GetProp`: `0` when the property is absent.
pub fn get_prop(hwnd: Hwnd, name: &str) -> isize {
    todo!()
}

/// C# `RemoveProp`: the removed value (`0` when absent). A window must remove its own properties before it is destroyed (`WM_NCDESTROY`).
pub fn remove_prop(hwnd: Hwnd, name: &str) -> isize {
    todo!()
}
