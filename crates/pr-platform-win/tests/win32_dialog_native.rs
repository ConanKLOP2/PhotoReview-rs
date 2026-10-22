//! Unit W8-11d tests: native dialog and dark-chrome Win32 calls (`pr_platform_win::win32::{dialog_native, dialog_chrome}`).
//! Pure tests: the 65 + 57 constants against the `win32-constants` golden (written by W8-11a), the byte sizes against the `windows` structs, the
//! `DT_MODIFYSTRING` mask, the documented signatures and an audit table against the `windows` constants.
//! Native tests (`#[ignore = "native: needs desktop"]`): the wrappers on hidden windows created by the test, a memory DC and a dialog built from an
//! in-memory template. Every window and GDI object is created and destroyed by the test; none of them enters a message loop (only `SendMessageW` to a
//! window of the test thread), none shows a modal dialog. The C# has no test for these declarations (the callers are `NativeDialog*Tests`, W8-13c/W8-18*),
//! so the native tests are new (`// new:`), written from the C# comments and the contract.
//!
//! Process-wide state: the GDI object count is shared by the whole test process, so every native test takes `NATIVE_LOCK`.

use core::mem::{offset_of, size_of};
use std::cell::Cell;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, Once};

use pr_platform_win::win32::api::{kernel32 as api_kernel32, user32 as api_user32};
use pr_platform_win::win32::dialog_chrome as dc;
use pr_platform_win::win32::dialog_native as dn;
use pr_platform_win::win32::dialog_native::{gdi32, kernel32, user32, uxtheme};
use pr_platform_win::win32::messages as msgs;
use pr_platform_win::win32::structs::*;
use pr_test_support::golden::{self, i64_of, str_of};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreatePen, FillRect, GetPixel, HBRUSH, HDC, PS_SOLID};
use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, RegisterClassExW, SendMessageW, WNDCLASSEXW};
use windows::core::PCWSTR;

// ---------------------------------------------------------------- golden constants

fn golden_class(class: &str) -> Vec<serde_json::Value> {
    golden::load("win32-constants")
        .cases
        .into_iter()
        .filter(|c| str_of(c, "kind") == "const" && str_of(c, "class") == class)
        .collect()
}

/// Checks that `table` has exactly the golden names of `class` and the golden values.
fn assert_class(class: &str, expected_count: usize, table: &[(&str, i64)]) {
    let golden = golden_class(class);
    assert_eq!(golden.len(), expected_count, "{class}: golden count");
    let golden_names: BTreeSet<&str> = golden.iter().map(|c| str_of(c, "name")).collect();
    let table_names: BTreeSet<&str> = table.iter().map(|(n, _)| *n).collect();
    assert_eq!(table.len(), table_names.len(), "{class}: duplicate row");
    assert_eq!(table_names, golden_names, "{class}: a missed or invented constant");
    for (name, value) in table {
        let case = golden
            .iter()
            .find(|c| str_of(c, "name") == *name)
            .expect("name checked above");
        assert_eq!(i64_of(case, "value"), *value, "{class}.{name}");
    }
}

macro_rules! row {
    ($cs:ident, $rust:expr) => {
        (stringify!($cs), $rust as i64)
    };
}

#[test]
fn golden_dialog_constants_equal_the_csharp_constants() {
    // C# DialogConstants (66 constants).
    assert_class(
        "DialogConstants",
        66,
        &[
            row!(WmSetFont, dn::WM_SETFONT),
            row!(WmGetFont, dn::WM_GETFONT),
            row!(WmInitDialog, dn::WM_INITDIALOG),
            row!(WmCtlColorMsgBox, dn::WM_CTLCOLORMSGBOX),
            row!(WmCtlColorEdit, dn::WM_CTLCOLOREDIT),
            row!(WmCtlColorListBox, dn::WM_CTLCOLORLISTBOX),
            row!(WmCtlColorBtn, dn::WM_CTLCOLORBTN),
            row!(WmCtlColorDlg, dn::WM_CTLCOLORDLG),
            row!(WmCtlColorScrollBar, dn::WM_CTLCOLORSCROLLBAR),
            row!(WmCtlColorStatic, dn::WM_CTLCOLORSTATIC),
            row!(DmGetDefId, dn::DM_GETDEFID),
            row!(DmSetDefId, dn::DM_SETDEFID),
            row!(BmSetStyle, dn::BM_SETSTYLE),
            row!(EmSetSel, dn::EM_SETSEL),
            row!(EmGetSel, dn::EM_GETSEL),
            row!(EnChange, dn::EN_CHANGE),
            row!(EmSetLimitText, dn::EM_SETLIMITTEXT),
            row!(EmGetRect, dn::EM_GETRECT),
            row!(EmSetRect, dn::EM_SETRECT),
            row!(EsMultiline, dn::ES_MULTILINE),
            row!(LbAddString, dn::LB_ADDSTRING),
            row!(LbResetContent, dn::LB_RESETCONTENT),
            row!(LbSetCurSel, dn::LB_SETCURSEL),
            row!(LbGetCount, dn::LB_GETCOUNT),
            row!(LbGetTextLen, dn::LB_GETTEXTLEN),
            row!(LbGetText, dn::LB_GETTEXT),
            row!(LbSetHorizontalExtent, dn::LB_SETHORIZONTALEXTENT),
            row!(LbInitStorage, dn::LB_INITSTORAGE),
            row!(BnClicked, dn::BN_CLICKED),
            row!(DwlpMsgResult, dn::DWLP_MSGRESULT),
            row!(DwlpUser, dn::DWLP_USER),
            row!(WsTabStop, dn::WS_TABSTOP),
            row!(WsGroup, dn::WS_GROUP),
            row!(WsBorder, dn::WS_BORDER),
            row!(WsVScroll, dn::WS_VSCROLL),
            row!(WsHScroll, dn::WS_HSCROLL),
            row!(WsDisabled, dn::WS_DISABLED),
            row!(WsDlgFrame, dn::WS_DLGFRAME),
            row!(WsExDlgModalFrame, dn::WS_EX_DLGMODALFRAME),
            row!(WsExClientEdge, dn::WS_EX_CLIENTEDGE),
            row!(WsExControlParent, dn::WS_EX_CONTROLPARENT),
            row!(BsPushButton, dn::BS_PUSHBUTTON),
            row!(BsDefPushButton, dn::BS_DEFPUSHBUTTON),
            row!(EsAutoHScroll, dn::ES_AUTOHSCROLL),
            row!(SsLeft, dn::SS_LEFT),
            row!(SsNoPrefix, dn::SS_NOPREFIX),
            row!(LbsNotify, dn::LBS_NOTIFY),
            row!(LbsNoIntegralHeight, dn::LBS_NOINTEGRALHEIGHT),
            row!(DsModalFrame, dn::DS_MODALFRAME),
            row!(DsSetFont, dn::DS_SETFONT),
            row!(DsNoFailCreate, dn::DS_NOFAILCREATE),
            row!(DtCenter, dn::DT_CENTER),
            row!(DtVCenter, dn::DT_VCENTER),
            row!(DtEndEllipsis, dn::DT_END_ELLIPSIS),
            row!(DtWordBreak, dn::DT_WORDBREAK),
            row!(DtSingleLine, dn::DT_SINGLELINE),
            row!(DtCalcRect, dn::DT_CALCRECT),
            row!(DtNoPrefix, dn::DT_NOPREFIX),
            row!(DtEditControl, dn::DT_EDITCONTROL),
            row!(SpiGetNonClientMetrics, dn::SPI_GETNONCLIENTMETRICS),
            row!(NonClientMetricsSize, dn::NONCLIENTMETRICSW_SIZE),
            row!(NonClientMessageFontOffset, dn::NONCLIENTMETRICSW_LFMESSAGEFONT_OFFSET),
            row!(LogFontSize, dn::LOGFONTW_SIZE),
            row!(GrGdiObjects, dn::GR_GDIOBJECTS),
            row!(GrUserObjects, dn::GR_USEROBJECTS),
            row!(TransparentBackground, dn::TRANSPARENT),
        ],
    );
    // C# DialogChromeNative (57 constants = 51 own + 6 that `dialog_chrome` re-exports from `messages`).
    assert_class(
        "DialogChromeNative",
        57,
        &[
            row!(SrcCopy, dc::SRCCOPY),
            row!(PaintStructSize, dc::PAINTSTRUCT_SIZE),
            row!(TrackMouseEventSize, dc::TRACKMOUSEEVENT_SIZE),
            row!(TmeLeave, dc::TME_LEAVE),
            row!(WmNcPaint, dc::WM_NCPAINT),
            row!(WmPrint, dc::WM_PRINT),
            row!(WmPrintClient, dc::WM_PRINTCLIENT),
            row!(WmMouseLeave, dc::WM_MOUSELEAVE),
            row!(WmQueryUiState, dc::WM_QUERYUISTATE),
            row!(WmUpdateUiState, dc::WM_UPDATEUISTATE),
            row!(PrfNonClient, dc::PRF_NONCLIENT),
            row!(UisfHideFocus, dc::UISF_HIDEFOCUS),
            row!(UisfHideAccel, dc::UISF_HIDEACCEL),
            row!(BmGetState, dc::BM_GETSTATE),
            row!(BstPushed, dc::BST_PUSHED),
            row!(BstFocus, dc::BST_FOCUS),
            row!(BsTypeMask, dc::BS_TYPEMASK),
            row!(BsPushLike, dc::BS_PUSHLIKE),
            row!(BsMultiline, dc::BS_MULTILINE),
            row!(BsLeftText, dc::BS_LEFTTEXT),
            row!(BsIcon, dc::BS_ICON),
            row!(BsBitmap, dc::BS_BITMAP),
            row!(CbsTypeMask, dc::CBS_TYPE_MASK),
            row!(CbsOwnerDrawFixed, dc::CBS_OWNERDRAWFIXED),
            row!(CbsOwnerDrawVariable, dc::CBS_OWNERDRAWVARIABLE),
            row!(PbmGetRange, dc::PBM_GETRANGE),
            row!(PbsMarquee, dc::PBS_MARQUEE),
            row!(TbsVert, dc::TBS_VERT),
            row!(TbmGetNumTics, dc::TBM_GETNUMTICS),
            row!(TbmGetPTics, dc::TBM_GETPTICS),
            row!(TbmGetThumbRect, dc::TBM_GETTHUMBRECT),
            row!(TbmGetChannelRect, dc::TBM_GETCHANNELRECT),
            row!(TbmGetRangeMin, dc::TBM_GETRANGEMIN),
            row!(TbmGetRangeMax, dc::TBM_GETRANGEMAX),
            row!(SsTypeMask, dc::SS_TYPEMASK),
            row!(SsEtchedHorz, dc::SS_ETCHEDHORZ),
            row!(SsEtchedVert, dc::SS_ETCHEDVERT),
            row!(SsEtchedFrame, dc::SS_ETCHEDFRAME),
            row!(HdmGetItemCount, dc::HDM_GETITEMCOUNT),
            row!(HdmGetItemRect, dc::HDM_GETITEMRECT),
            row!(LvmGetBkColor, dc::LVM_GETBKCOLOR),
            row!(CdisFocus, dc::CDIS_FOCUS),
            row!(CdisHot, dc::CDIS_HOT),
            row!(WsBorder, dc::WS_BORDER),
            row!(WsDlgFrame, dc::WS_DLGFRAME),
            row!(WsExClientEdge, dc::WS_EX_CLIENTEDGE),
            row!(WsExStaticEdge, dc::WS_EX_STATICEDGE),
            row!(SmCxBorder, dc::SM_CXBORDER),
            row!(SmCxEdge, dc::SM_CXEDGE),
            row!(DtHidePrefix, dc::DT_HIDEPREFIX),
            row!(DtRight, dc::DT_RIGHT),
            row!(WmEraseBkgnd, dc::WM_ERASEBKGND),
            row!(WmKillFocus, dc::WM_KILLFOCUS),
            row!(WmMouseMove, dc::WM_MOUSEMOVE),
            row!(WmNcDestroy, dc::WM_NCDESTROY),
            row!(WmPaint, dc::WM_PAINT),
            row!(WmSetFocus, dc::WM_SETFOCUS),
        ],
    );
    // The re-exports are the very same values as `messages`.
    assert_eq!(dc::WM_PAINT, msgs::WM_PAINT);
    assert_eq!(dc::WM_NCDESTROY, msgs::WM_NCDESTROY);
    // Trap T5: the x64 values of the window-extra offsets of a dialog.
    assert_eq!((dn::DWLP_MSGRESULT, dn::DWLP_USER), (0, 16));
}

#[test]
fn dialog_byte_sizes_match_the_windows_structs() {
    use windows::Win32::Graphics::Gdi::{LOGFONTW, PAINTSTRUCT, TEXTMETRICW};
    use windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT;
    use windows::Win32::UI::WindowsAndMessaging::NONCLIENTMETRICSW;

    // C# PaintStructSize, TrackMouseEventSize, NonClientMetricsSize, NonClientMessageFontOffset, LogFontSize.
    assert_eq!(size_of::<PAINTSTRUCT>() as i32, dc::PAINTSTRUCT_SIZE);
    assert_eq!(dc::PAINTSTRUCT_SIZE, 72);
    assert_eq!(size_of::<TRACKMOUSEEVENT>() as i32, dc::TRACKMOUSEEVENT_SIZE);
    assert_eq!(dc::TRACKMOUSEEVENT_SIZE, 24);
    assert_eq!(size_of::<NONCLIENTMETRICSW>() as i32, dn::NONCLIENTMETRICSW_SIZE);
    assert_eq!(dn::NONCLIENTMETRICSW_SIZE, 504);
    assert_eq!(
        offset_of!(NONCLIENTMETRICSW, lfMessageFont) as i32,
        dn::NONCLIENTMETRICSW_LFMESSAGEFONT_OFFSET
    );
    assert_eq!(dn::NONCLIENTMETRICSW_LFMESSAGEFONT_OFFSET, 408);
    assert_eq!(size_of::<LOGFONTW>() as i32, dn::LOGFONTW_SIZE);
    assert_eq!(dn::LOGFONTW_SIZE, 92);
    // The C# reads the first five LONGs of the 60-byte TEXTMETRICW.
    assert_eq!(size_of::<TEXTMETRICW>(), 60);
}

#[test]
fn draw_text_format_without_modify_string_clears_only_that_bit() {
    // Trap T3: DT_MODIFYSTRING would make DrawText write into the immutable text.
    assert_eq!(dn::DT_MODIFYSTRING, 0x10000);
    assert_eq!(
        dn::draw_text_format_without_modify_string(0x10000 | 0x8000 | 0x10),
        0x8010
    );
    assert_eq!(dn::draw_text_format_without_modify_string(0x8010), 0x8010);
    assert_eq!(dn::draw_text_format_without_modify_string(0x10000), 0);
    assert_eq!(dn::draw_text_format_without_modify_string(0), 0);
    assert_eq!(dn::draw_text_format_without_modify_string(u32::MAX), 0xFFFE_FFFF);
    // The flags the dialogs use survive.
    let used = dn::DT_EDITCONTROL
        | dn::DT_WORDBREAK
        | dn::DT_NOPREFIX
        | dn::DT_END_ELLIPSIS
        | dn::DT_SINGLELINE
        | dn::DT_CENTER
        | dn::DT_VCENTER
        | dn::DT_CALCRECT
        | dc::DT_HIDEPREFIX
        | dc::DT_RIGHT;
    assert_eq!(
        dn::draw_text_format_without_modify_string(used | dn::DT_MODIFYSTRING),
        used
    );
}

#[test]
#[allow(clippy::type_complexity)] // the point of the test is to spell the signatures out
fn dialog_native_api_has_the_documented_shapes() {
    let _: unsafe fn(Hinstance, &[u8], Hwnd, WndProc, isize) -> Result<Hwnd, Win32Error> =
        user32::create_dialog_indirect_param;
    let _: unsafe fn(Hwnd, &mut Msg) -> bool = user32::is_dialog_message;
    let _: fn(Hwnd, bool) -> bool = user32::enable_window;
    let _: fn(Hwnd) -> bool = user32::is_window_enabled;
    let _: fn(Hwnd) -> bool = user32::is_window;
    let _: fn(Hwnd, i32) -> Hwnd = user32::get_dlg_item;
    let _: fn(Hwnd, &str) -> Result<(), Win32Error> = user32::set_window_text;
    let _: fn(Hwnd) -> i32 = user32::get_window_text_length;
    let _: fn(Hwnd) -> String = user32::get_window_text;
    let _: unsafe fn(Hwnd, u32, Wparam, Lparam) -> Lresult = user32::send_message;
    let _: fn(Hwnd, i32, i32, i32, i32, bool) -> Result<(), Win32Error> = user32::move_window;
    let _: fn(Hwnd) -> Hwnd = user32::set_focus;
    let _: fn() -> Hwnd = user32::get_focus;
    let _: fn(Hwnd) -> Hdc = user32::get_dc;
    let _: fn(Hwnd, Hdc) -> i32 = user32::release_dc;
    let _: fn(Hdc, &str, &mut Rect, u32) -> i32 = user32::draw_text;
    let _: unsafe fn(u32, u32, *mut core::ffi::c_void, u32, u32) -> Result<(), Win32Error> =
        user32::system_parameters_info_for_dpi;
    let _: fn(u32) -> Result<windows::Win32::UI::WindowsAndMessaging::NONCLIENTMETRICSW, Win32Error> =
        user32::non_client_metrics_for_dpi;
    let _: fn(Hwnd, &mut Point) -> bool = user32::screen_to_client;
    let _: fn(isize, u32) -> u32 = user32::get_gui_resources;
    let _: fn(Hwnd, u32) -> Hwnd = user32::get_window;
    let _: fn(Hwnd) -> i32 = user32::get_dlg_ctrl_id;
    let _: fn(Hwnd, Option<&Rect>, bool) -> bool = user32::invalidate_rect;

    let _: fn(u32) -> Hgdi = gdi32::create_solid_brush;
    let _: fn(Hgdi) -> bool = gdi32::delete_object;
    let _: fn(Hdc, u32) -> u32 = gdi32::set_text_color;
    let _: fn(Hdc, u32) -> u32 = gdi32::set_bk_color;
    let _: fn(Hdc) -> u32 = gdi32::get_text_color;
    let _: fn(Hdc) -> u32 = gdi32::get_bk_color;
    let _: fn(Hdc, Hgdi) -> Hgdi = gdi32::select_object;
    let _: fn(Hdc, &str) -> Option<Point> = gdi32::get_text_extent_point32;
    let _: fn(Hdc) -> Option<windows::Win32::Graphics::Gdi::TEXTMETRICW> = gdi32::get_text_metrics;
    let _: fn(&windows::Win32::Graphics::Gdi::LOGFONTW) -> Hgdi = gdi32::create_font_indirect;
    let _: fn(Hgdi, &mut [u8]) -> i32 = gdi32::get_object;
    let _: fn(Hgdi) -> Option<windows::Win32::Graphics::Gdi::LOGFONTW> = gdi32::get_log_font;
    let _: fn() -> isize = kernel32::get_current_process;
    let _: fn(Hwnd, Option<&str>, Option<&str>) -> Result<(), Hresult> = uxtheme::set_window_theme;

    let _: fn(Hwnd) -> Option<dc::PaintSession> = dc::begin_paint;
    let _: fn(Hwnd, &dc::PaintSession) -> bool = dc::end_paint;
    let _: fn(Hwnd) -> Hdc = dc::get_window_dc;
    let _: fn(u32, Hwnd, u32) -> bool = dc::track_mouse_event;
    let _: fn(Hdc) -> Hdc = dc::create_compatible_dc;
    let _: fn(Hdc, i32, i32) -> Hgdi = dc::create_compatible_bitmap;
    let _: fn(Hdc) -> bool = dc::delete_dc;
    let _: fn(Hdc, i32, i32, i32, i32, Hdc, i32, i32, u32) -> bool = dc::bit_blt;
    let _: fn(Hdc, i32, i32, i32, i32) -> bool = dc::ellipse;
    let _: fn(Hdc, &[Point]) -> bool = dc::polyline;
    let _: fn(Hwnd, &str, isize) -> bool = dc::set_prop;
    let _: fn(Hwnd, &str) -> isize = dc::get_prop;
    let _: fn(Hwnd, &str) -> isize = dc::remove_prop;
}

#[test]
fn windows_rs_has_the_same_dialog_message_values() {
    use windows::Win32::Graphics::Gdi as g;
    use windows::Win32::UI::Controls as c;
    use windows::Win32::UI::WindowsAndMessaging as m;
    // (name, our value, windows-rs value): raw integers use `as i64`, typed constants `.0 as i64`.
    let rows: Vec<(&str, i64, i64)> = vec![
        ("WM_SETFONT", dn::WM_SETFONT as i64, m::WM_SETFONT as i64),
        ("WM_INITDIALOG", dn::WM_INITDIALOG as i64, m::WM_INITDIALOG as i64),
        (
            "WM_CTLCOLORMSGBOX",
            dn::WM_CTLCOLORMSGBOX as i64,
            m::WM_CTLCOLORMSGBOX as i64,
        ),
        ("WM_CTLCOLOREDIT", dn::WM_CTLCOLOREDIT as i64, m::WM_CTLCOLOREDIT as i64),
        (
            "WM_CTLCOLORLISTBOX",
            dn::WM_CTLCOLORLISTBOX as i64,
            m::WM_CTLCOLORLISTBOX as i64,
        ),
        ("WM_CTLCOLORBTN", dn::WM_CTLCOLORBTN as i64, m::WM_CTLCOLORBTN as i64),
        ("WM_CTLCOLORDLG", dn::WM_CTLCOLORDLG as i64, m::WM_CTLCOLORDLG as i64),
        (
            "WM_CTLCOLORSCROLLBAR",
            dn::WM_CTLCOLORSCROLLBAR as i64,
            m::WM_CTLCOLORSCROLLBAR as i64,
        ),
        (
            "WM_CTLCOLORSTATIC",
            dn::WM_CTLCOLORSTATIC as i64,
            m::WM_CTLCOLORSTATIC as i64,
        ),
        ("DM_GETDEFID", dn::DM_GETDEFID as i64, m::DM_GETDEFID as i64),
        ("DM_SETDEFID", dn::DM_SETDEFID as i64, m::DM_SETDEFID as i64),
        ("BM_SETSTYLE", dn::BM_SETSTYLE as i64, m::BM_SETSTYLE as i64),
        ("BN_CLICKED", dn::BN_CLICKED as i64, m::BN_CLICKED as i64),
        ("EM_SETSEL", dn::EM_SETSEL as i64, c::EM_SETSEL as i64),
        ("EM_GETSEL", dn::EM_GETSEL as i64, c::EM_GETSEL as i64),
        ("EM_SETLIMITTEXT", dn::EM_SETLIMITTEXT as i64, c::EM_SETLIMITTEXT as i64),
        ("EM_GETRECT", dn::EM_GETRECT as i64, c::EM_GETRECT as i64),
        ("EM_SETRECT", dn::EM_SETRECT as i64, c::EM_SETRECT as i64),
        ("LB_ADDSTRING", dn::LB_ADDSTRING as i64, m::LB_ADDSTRING as i64),
        ("LB_RESETCONTENT", dn::LB_RESETCONTENT as i64, m::LB_RESETCONTENT as i64),
        ("LB_SETCURSEL", dn::LB_SETCURSEL as i64, m::LB_SETCURSEL as i64),
        ("LB_GETCOUNT", dn::LB_GETCOUNT as i64, m::LB_GETCOUNT as i64),
        ("LB_GETTEXTLEN", dn::LB_GETTEXTLEN as i64, m::LB_GETTEXTLEN as i64),
        ("LB_GETTEXT", dn::LB_GETTEXT as i64, m::LB_GETTEXT as i64),
        (
            "LB_SETHORIZONTALEXTENT",
            dn::LB_SETHORIZONTALEXTENT as i64,
            m::LB_SETHORIZONTALEXTENT as i64,
        ),
        ("LB_INITSTORAGE", dn::LB_INITSTORAGE as i64, m::LB_INITSTORAGE as i64),
        ("BS_PUSHBUTTON", dn::BS_PUSHBUTTON as i64, m::BS_PUSHBUTTON as i64),
        (
            "BS_DEFPUSHBUTTON",
            dn::BS_DEFPUSHBUTTON as i64,
            m::BS_DEFPUSHBUTTON as i64,
        ),
        ("ES_MULTILINE", dn::ES_MULTILINE as i64, m::ES_MULTILINE as i64),
        ("ES_AUTOHSCROLL", dn::ES_AUTOHSCROLL as i64, m::ES_AUTOHSCROLL as i64),
        ("LBS_NOTIFY", dn::LBS_NOTIFY as i64, m::LBS_NOTIFY as i64),
        (
            "LBS_NOINTEGRALHEIGHT",
            dn::LBS_NOINTEGRALHEIGHT as i64,
            m::LBS_NOINTEGRALHEIGHT as i64,
        ),
        ("DS_MODALFRAME", dn::DS_MODALFRAME as i64, m::DS_MODALFRAME as i64),
        ("DS_SETFONT", dn::DS_SETFONT as i64, m::DS_SETFONT as i64),
        ("DS_NOFAILCREATE", dn::DS_NOFAILCREATE as i64, m::DS_NOFAILCREATE as i64),
        ("DWLP_MSGRESULT", dn::DWLP_MSGRESULT as i64, m::DWLP_MSGRESULT as i64),
        // windows-rs has no DWLP_USER; the SDK defines it as DWLP_MSGRESULT + sizeof(LRESULT) + sizeof(DLGPROC).
        (
            "DWLP_USER",
            dn::DWLP_USER as i64,
            (m::DWLP_MSGRESULT as usize + 2 * size_of::<usize>()) as i64,
        ),
        ("WS_TABSTOP", dn::WS_TABSTOP as i64, m::WS_TABSTOP.0 as i64),
        ("WS_GROUP", dn::WS_GROUP as i64, m::WS_GROUP.0 as i64),
        ("WS_BORDER", dn::WS_BORDER as i64, m::WS_BORDER.0 as i64),
        ("WS_VSCROLL", dn::WS_VSCROLL as i64, m::WS_VSCROLL.0 as i64),
        ("WS_HSCROLL", dn::WS_HSCROLL as i64, m::WS_HSCROLL.0 as i64),
        ("WS_DISABLED", dn::WS_DISABLED as i64, m::WS_DISABLED.0 as i64),
        ("WS_DLGFRAME", dn::WS_DLGFRAME as i64, m::WS_DLGFRAME.0 as i64),
        (
            "WS_EX_DLGMODALFRAME",
            dn::WS_EX_DLGMODALFRAME as i64,
            m::WS_EX_DLGMODALFRAME.0 as i64,
        ),
        (
            "WS_EX_CLIENTEDGE",
            dn::WS_EX_CLIENTEDGE as i64,
            m::WS_EX_CLIENTEDGE.0 as i64,
        ),
        (
            "WS_EX_CONTROLPARENT",
            dn::WS_EX_CONTROLPARENT as i64,
            m::WS_EX_CONTROLPARENT.0 as i64,
        ),
        ("DT_CENTER", dn::DT_CENTER as i64, g::DT_CENTER.0 as i64),
        ("DT_VCENTER", dn::DT_VCENTER as i64, g::DT_VCENTER.0 as i64),
        (
            "DT_END_ELLIPSIS",
            dn::DT_END_ELLIPSIS as i64,
            g::DT_END_ELLIPSIS.0 as i64,
        ),
        ("DT_WORDBREAK", dn::DT_WORDBREAK as i64, g::DT_WORDBREAK.0 as i64),
        ("DT_SINGLELINE", dn::DT_SINGLELINE as i64, g::DT_SINGLELINE.0 as i64),
        ("DT_CALCRECT", dn::DT_CALCRECT as i64, g::DT_CALCRECT.0 as i64),
        ("DT_NOPREFIX", dn::DT_NOPREFIX as i64, g::DT_NOPREFIX.0 as i64),
        ("DT_EDITCONTROL", dn::DT_EDITCONTROL as i64, g::DT_EDITCONTROL.0 as i64),
        (
            "DT_MODIFYSTRING",
            dn::DT_MODIFYSTRING as i64,
            g::DT_MODIFYSTRING.0 as i64,
        ),
        ("DT_HIDEPREFIX", dc::DT_HIDEPREFIX as i64, g::DT_HIDEPREFIX.0 as i64),
        ("DT_RIGHT", dc::DT_RIGHT as i64, g::DT_RIGHT.0 as i64),
        (
            "SPI_GETNONCLIENTMETRICS",
            dn::SPI_GETNONCLIENTMETRICS as i64,
            m::SPI_GETNONCLIENTMETRICS.0 as i64,
        ),
        (
            "GR_GDIOBJECTS",
            dn::GR_GDIOBJECTS as i64,
            windows::Win32::System::Threading::GR_GDIOBJECTS.0 as i64,
        ),
        (
            "GR_USEROBJECTS",
            dn::GR_USEROBJECTS as i64,
            windows::Win32::System::Threading::GR_USEROBJECTS.0 as i64,
        ),
        ("TRANSPARENT", dn::TRANSPARENT as i64, g::TRANSPARENT.0 as i64),
        ("SRCCOPY", dc::SRCCOPY as i64, g::SRCCOPY.0 as i64),
        (
            "TME_LEAVE",
            dc::TME_LEAVE as i64,
            windows::Win32::UI::Input::KeyboardAndMouse::TME_LEAVE.0 as i64,
        ),
        ("WM_MOUSELEAVE", dc::WM_MOUSELEAVE as i64, c::WM_MOUSELEAVE as i64),
        ("WM_NCPAINT", dc::WM_NCPAINT as i64, m::WM_NCPAINT as i64),
        ("WM_PRINTCLIENT", dc::WM_PRINTCLIENT as i64, m::WM_PRINTCLIENT as i64),
        ("SM_CXEDGE", dc::SM_CXEDGE as i64, m::SM_CXEDGE.0 as i64),
        ("SM_CXBORDER", dc::SM_CXBORDER as i64, m::SM_CXBORDER.0 as i64),
    ];
    for (name, ours, theirs) in rows {
        assert_eq!(ours, theirs, "{name}");
    }
}

// ---------------------------------------------------------------- native helpers

/// All native tests share the process GDI count and the desktop: one at a time.
static NATIVE_LOCK: Mutex<()> = Mutex::new(());

fn native_lock() -> MutexGuard<'static, ()> {
    NATIVE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A window destroyed on drop (on the creating thread).
struct Win(Hwnd);

impl Drop for Win {
    fn drop(&mut self) {
        if user32::is_window(self.0) {
            let _ = api_user32::destroy_window(self.0);
        }
    }
}

const WS_POPUP: u32 = 0x8000_0000;
const WS_CHILD: u32 = 0x4000_0000;
const WS_CAPTION: u32 = 0x00C0_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_EX_TOOLWINDOW: u32 = 0x80;
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;

/// A built-in-class window (`STATIC`, `EDIT`, `SCROLLBAR`...) that the test owns.
fn make_window(class: &str, title: &str, style: u32, ex_style: u32, rect: (i32, i32, i32, i32), parent: Hwnd) -> Win {
    let instance = api_kernel32::get_module_handle().expect("module handle");
    // SAFETY: built-in window classes have no custom `WM_NCCREATE` parameter handling; `param` is 0.
    let hwnd = unsafe {
        api_user32::create_window_ex(
            ex_style, class, title, style, rect.0, rect.1, rect.2, rect.3, parent, 0, instance, 0,
        )
    }
    .expect("create_window_ex");
    assert_ne!(hwnd, 0);
    Win(hwnd)
}

/// A hidden top-level window of the `STATIC` class.
fn hidden_window(title: &str) -> Win {
    make_window("STATIC", title, WS_POPUP, 0, (10, 20, 100, 50), 0)
}

/// How a dialog template item names its class.
enum Class<'a> {
    Atom(u16),
    Name(&'a str),
}

const ATOM_STATIC: u16 = 0x0082;

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_wide_z(bytes: &mut Vec<u8>, text: &str) {
    for unit in text.encode_utf16() {
        push_u16(bytes, unit);
    }
    push_u16(bytes, 0);
}

fn pad_to_dword(bytes: &mut Vec<u8>) {
    while bytes.len() % 4 != 0 {
        bytes.push(0);
    }
}

/// A `DLGTEMPLATE` (no `DS_SETFONT`, no menu, default class, empty title) with the given items `(id, class, text, style)`.
fn build_template(style: u32, items: &[(u16, Class, &str, u32)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, style);
    push_u32(&mut bytes, 0); // extended style
    push_u16(&mut bytes, items.len() as u16);
    for value in [10i16, 10, 120, 60] {
        push_u16(&mut bytes, value as u16); // x, y, cx, cy
    }
    push_u16(&mut bytes, 0); // no menu
    push_u16(&mut bytes, 0); // default dialog class
    push_u16(&mut bytes, 0); // empty title
    for (id, class, text, item_style) in items {
        pad_to_dword(&mut bytes);
        push_u32(&mut bytes, *item_style);
        push_u32(&mut bytes, 0); // extended style
        for value in [5i16, 5, 40, 12] {
            push_u16(&mut bytes, value as u16);
        }
        push_u16(&mut bytes, *id);
        match class {
            Class::Atom(atom) => {
                push_u16(&mut bytes, 0xFFFF);
                push_u16(&mut bytes, *atom);
            }
            Class::Name(name) => push_wide_z(&mut bytes, name),
        }
        push_wide_z(&mut bytes, text);
        push_u16(&mut bytes, 0); // no creation data
    }
    bytes
}

/// What the dialog procedure saw: the `lparam` of `WM_INITDIALOG` and how often it arrived.
#[derive(Default)]
struct InitRecord {
    init_param: AtomicIsize,
    hits: AtomicU32,
}

/// The test dialog procedure: records `WM_INITDIALOG` and returns `FALSE` ("not handled", the dialog manager does its default work).
unsafe extern "system" fn recording_dialog_proc(_hwnd: Hwnd, message: u32, _wparam: Wparam, lparam: Lparam) -> Lresult {
    if message == dn::WM_INITDIALOG && lparam != 0 {
        // SAFETY: the test passes the address of an `InitRecord` that outlives the (synchronous) creation call as `init_param`.
        let record = unsafe { &*(lparam as *const InitRecord) };
        record.init_param.store(lparam, Ordering::SeqCst);
        record.hits.fetch_add(1, Ordering::SeqCst);
    }
    0
}

fn create_dialog(template: &[u8], record: &InitRecord) -> Result<Win, Win32Error> {
    let instance = api_kernel32::get_module_handle().expect("module handle");
    // SAFETY: the templates of this file are complete and well-formed; the procedure does not unwind; `record` outlives the call.
    unsafe {
        user32::create_dialog_indirect_param(
            instance,
            template,
            0,
            recording_dialog_proc,
            record as *const InitRecord as isize,
        )
    }
    .map(Win)
}

fn dialog_with_static_item_7() -> (Win, InitRecord) {
    let record = InitRecord::default();
    let template = build_template(
        WS_POPUP | WS_CAPTION,
        &[(7, Class::Atom(ATOM_STATIC), "label", WS_CHILD | WS_VISIBLE)],
    );
    let dialog = create_dialog(&template, &record).expect("dialog with one STATIC item");
    (dialog, record)
}

/// The first GDI use of a process leaves one handle counted by `GetGuiResources` for good (observed on Windows 11: a brush created and deleted
/// keeps the count at 1, then it stays there). A test that compares counts runs one create/delete cycle first.
fn warm_up_gdi() {
    let dc = dc::create_compatible_dc(0);
    let brush = gdi32::create_solid_brush(0x0000_00FF);
    let bitmap = dc::create_compatible_bitmap(dc, 2, 2);
    let old_brush = gdi32::select_object(dc, brush);
    let old_bitmap = gdi32::select_object(dc, bitmap);
    gdi32::select_object(dc, old_bitmap);
    gdi32::select_object(dc, old_brush);
    gdi32::delete_object(bitmap);
    gdi32::delete_object(brush);
    dc::delete_dc(dc);
}

// ---------------------------------------------------------------- native: user32

#[test]
#[ignore = "native: needs desktop"]
fn create_dialog_indirect_param_from_a_minimal_template_delivers_init_param() {
    // new: DialogNative.CreateDialogIndirectParam (callers: NativeDialog, W8-13c).
    let _lock = native_lock();
    let record = InitRecord::default();
    let template = build_template(WS_POPUP | WS_CAPTION, &[]);
    let dialog = create_dialog(&template, &record).expect("dialog");
    assert_ne!(dialog.0, 0);
    assert!(user32::is_window(dialog.0));
    assert_eq!(
        record.hits.load(Ordering::SeqCst),
        1,
        "WM_INITDIALOG arrives once, before the call returns"
    );
    assert_eq!(
        record.init_param.load(Ordering::SeqCst),
        &record as *const InitRecord as isize,
        "WM_INITDIALOG lparam is init_param"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn create_dialog_indirect_param_with_an_unaligned_vec_still_works() {
    // new: trap T4, the template slice does not start on a DWORD boundary.
    let _lock = native_lock();
    let template = build_template(
        WS_POPUP | WS_CAPTION,
        &[(7, Class::Atom(ATOM_STATIC), "x", WS_CHILD | WS_VISIBLE)],
    );
    let mut buffer = vec![0u8; template.len() + 8];
    for offset in [1usize, 2, 3] {
        buffer[offset..offset + template.len()].copy_from_slice(&template);
        let record = InitRecord::default();
        let dialog = create_dialog(&buffer[offset..offset + template.len()], &record).expect("dialog");
        assert!(user32::is_window(dialog.0), "offset {offset}");
        assert_eq!(record.hits.load(Ordering::SeqCst), 1, "offset {offset}");
        assert_ne!(
            user32::get_dlg_item(dialog.0, 7),
            0,
            "offset {offset}: the item was parsed"
        );
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn create_dialog_indirect_param_with_a_slice_shorter_than_the_header_is_err_invalid_parameter() {
    // new: the wrapper refuses what cannot be a DLGTEMPLATE without calling the OS. The truncated-but-long-enough variant is never run (it would read past the slice).
    let _lock = native_lock();
    let record = InitRecord::default();
    for length in [0usize, 1, 17] {
        let result = create_dialog(&vec![0u8; length], &record);
        assert_eq!(result.err(), Some(Win32Error(87)), "length {length}");
    }
    assert_eq!(record.hits.load(Ordering::SeqCst), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn create_dialog_indirect_param_with_an_unknown_item_class_is_err() {
    // new: the handle is 0 (a well-formed template whose control class does not exist), so the result is `Err`.
    let _lock = native_lock();
    let record = InitRecord::default();
    let template = build_template(
        WS_POPUP | WS_CAPTION,
        &[(1, Class::Name("PrNoSuchClass11d"), "x", WS_CHILD)],
    );
    let result = create_dialog(&template, &record);
    assert!(result.is_err(), "unknown control class must fail the creation");
}

#[test]
#[ignore = "native: needs desktop"]
fn set_window_text_then_get_window_text_round_trips_unicode() {
    // new: DialogNative.SetWindowText/GetWindowText (the "length, then length + 1" idiom).
    let _lock = native_lock();
    let window = hidden_window("");
    let text = "Ảnh đẹp Việt Nam - chào buổi sáng \u{1F600}\u{1F4F7}";
    user32::set_window_text(window.0, text).expect("set_window_text");
    assert_eq!(user32::get_window_text(window.0), text);
    assert_eq!(
        user32::get_window_text_length(window.0) as usize,
        text.encode_utf16().count()
    );
    user32::set_window_text(window.0, "").expect("set empty");
    assert_eq!(user32::get_window_text(window.0), "");
    assert_eq!(user32::get_window_text_length(window.0), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn get_window_text_of_a_textless_window_is_empty() {
    // new: trap T2, no text is "", never an error; a stale handle is "" too.
    let _lock = native_lock();
    let window = hidden_window("");
    assert_eq!(user32::get_window_text(window.0), "");
    let stale = {
        let gone = hidden_window("x");
        gone.0
    };
    assert_eq!(user32::get_window_text(stale), "");
    assert_eq!(user32::get_window_text_length(stale), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn enable_window_returns_the_previous_state_and_is_window_enabled_follows() {
    // new: DialogNative.EnableWindow/IsWindowEnabled.
    let _lock = native_lock();
    let window = hidden_window("w");
    assert!(user32::is_window_enabled(window.0));
    assert!(!user32::enable_window(window.0, false), "it was not disabled before");
    assert!(!user32::is_window_enabled(window.0));
    assert!(user32::enable_window(window.0, true), "it was disabled before");
    assert!(user32::is_window_enabled(window.0));
}

#[test]
#[ignore = "native: needs desktop"]
fn move_window_changes_the_rectangle() {
    // new: DialogNative.MoveWindow.
    let _lock = native_lock();
    let window = hidden_window("w");
    let before = api_user32::get_window_rect(window.0).expect("rect");
    assert_eq!(
        (before.left, before.top, before.right, before.bottom),
        (10, 20, 110, 70)
    );
    user32::move_window(window.0, 30, 40, 120, 60, true).expect("move_window");
    let after = api_user32::get_window_rect(window.0).expect("rect");
    assert_eq!((after.left, after.top, after.right, after.bottom), (30, 40, 150, 100));
    assert!(
        user32::move_window(0, 0, 0, 1, 1, false).is_err(),
        "a null window is an error"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn get_dlg_item_and_get_dlg_ctrl_id_are_inverse() {
    // new: DialogNative.GetDlgItem/GetDlgCtrlID; trap T2: an unknown id is 0, not an error.
    let _lock = native_lock();
    let (dialog, _record) = dialog_with_static_item_7();
    let item = user32::get_dlg_item(dialog.0, 7);
    assert_ne!(item, 0);
    assert_eq!(user32::get_dlg_ctrl_id(item), 7);
    assert_eq!(user32::get_window_text(item), "label");
    assert_eq!(user32::get_dlg_item(dialog.0, 99), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn set_focus_get_focus_round_trip_on_an_enabled_visible_window() {
    // new: DialogNative.SetFocus/GetFocus on a one-pixel tool window that never takes the activation.
    let _lock = native_lock();
    let window = make_window(
        "EDIT",
        "",
        WS_POPUP | WS_VISIBLE,
        WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        (0, 0, 2, 2),
        0,
    );
    user32::set_focus(window.0);
    assert_eq!(user32::get_focus(), window.0);
}

#[test]
#[ignore = "native: needs desktop"]
fn screen_to_client_subtracts_the_client_origin() {
    // new: DialogNative.ScreenToClient (a borderless popup: the client origin is the window origin).
    let _lock = native_lock();
    let window = make_window("STATIC", "", WS_POPUP, 0, (100, 200, 50, 50), 0);
    let mut point = Point { x: 150, y: 260 };
    assert!(user32::screen_to_client(window.0, &mut point));
    assert_eq!((point.x, point.y), (50, 60));
    assert!(!user32::screen_to_client(0, &mut point), "a null window fails");
}

#[test]
#[ignore = "native: needs desktop"]
fn invalidate_rect_returns_true() {
    // new: DialogNative.InvalidateRect.
    let _lock = native_lock();
    let window = hidden_window("w");
    assert!(user32::invalidate_rect(window.0, None, true));
    let rect = Rect {
        left: 0,
        top: 0,
        right: 10,
        bottom: 10,
    };
    assert!(user32::invalidate_rect(window.0, Some(&rect), false));
}

#[test]
#[ignore = "native: needs desktop"]
fn get_window_returns_the_owner_or_child() {
    // new: DialogNative.GetWindow (GW_OWNER 4, GW_CHILD 5).
    let _lock = native_lock();
    let owner = hidden_window("owner");
    let owned = make_window("STATIC", "", WS_POPUP, 0, (0, 0, 10, 10), owner.0);
    assert_eq!(user32::get_window(owned.0, 4), owner.0, "GW_OWNER");
    assert_eq!(user32::get_window(owner.0, 4), 0, "no owner is 0");
    let (dialog, _record) = dialog_with_static_item_7();
    let item = user32::get_dlg_item(dialog.0, 7);
    assert_eq!(user32::get_window(dialog.0, 5), item, "GW_CHILD");
}

#[test]
#[ignore = "native: needs desktop"]
fn send_message_reaches_the_window_procedure_of_a_window_of_this_thread() {
    // new: DialogNative.SendMessage (WM_SETTEXT/WM_GETTEXTLENGTH on a STATIC window of the test thread).
    let _lock = native_lock();
    let window = hidden_window("abc");
    // SAFETY: WM_GETTEXTLENGTH takes no pointers; the window belongs to this thread.
    let length = unsafe { user32::send_message(window.0, 0x000E, 0, 0) };
    assert_eq!(length, 3);
    let text = to_utf16z("hello");
    // SAFETY: WM_SETTEXT reads the NUL-terminated UTF-16 `text`, alive during the synchronous call.
    let result = unsafe { user32::send_message(window.0, 0x000C, 0, text.as_ptr() as isize) };
    assert_eq!(result, 1);
    assert_eq!(user32::get_window_text(window.0), "hello");
}

#[test]
#[ignore = "native: needs desktop"]
fn draw_text_calc_rect_measures_a_wider_rect_for_longer_text_and_never_modifies_the_input() {
    // new: DialogNative.DrawText with DT_CALCRECT | DT_SINGLELINE; D-W8B3-9: DT_MODIFYSTRING in `format` changes nothing.
    let _lock = native_lock();
    let dc = dc::create_compatible_dc(0);
    assert_ne!(dc, 0);
    let format = dn::DT_CALCRECT | dn::DT_SINGLELINE;
    let measure = |text: &str, format: u32| {
        let mut rect = Rect::default();
        let height = user32::draw_text(dc, text, &mut rect, format);
        (height, rect)
    };
    let (short_height, short_rect) = measure("Hi", format);
    let (long_height, long_rect) = measure("A much longer line of dialog text", format);
    assert!(short_height > 0 && long_height > 0);
    assert!(short_rect.right > 0);
    assert!(long_rect.right > short_rect.right);
    assert_eq!(short_rect.bottom, long_rect.bottom, "one line: the same height");
    let text = String::from("A much longer line of dialog text");
    let (modify_height, modify_rect) = measure(&text, format | dn::DT_MODIFYSTRING);
    assert_eq!(modify_height, long_height);
    assert_eq!(
        (modify_rect.right, modify_rect.bottom),
        (long_rect.right, long_rect.bottom)
    );
    assert_eq!(text, "A much longer line of dialog text");
    assert!(dc::delete_dc(dc));
}

#[test]
#[ignore = "native: needs desktop"]
fn get_dc_release_dc_round_trip() {
    // new: DialogNative.GetDC/ReleaseDC and the screen DC `0`.
    let _lock = native_lock();
    let screen = user32::get_dc(0);
    assert_ne!(screen, 0);
    assert_eq!(user32::release_dc(0, screen), 1);
    let window = hidden_window("w");
    let window_dc = user32::get_dc(window.0);
    assert_ne!(window_dc, 0);
    assert_eq!(user32::release_dc(window.0, window_dc), 1);
}

// ---------------------------------------------------------------- native: gdi32, kernel32, uxtheme

#[test]
#[ignore = "native: needs desktop"]
fn gdi_color_and_object_round_trips_do_not_leak() {
    // new: trap T6 (reselect before delete) and the GR_GDIOBJECTS leak check of NativeDialogTestKit.
    let _lock = native_lock();
    warm_up_gdi();
    let process = kernel32::get_current_process();
    let before = user32::get_gui_resources(process, dn::GR_GDIOBJECTS as u32);
    let dc = dc::create_compatible_dc(0);
    assert_ne!(dc, 0);
    let brush = gdi32::create_solid_brush(0x0040_80FF);
    assert_ne!(brush, 0);
    let old = gdi32::select_object(dc, brush);
    assert_ne!(old, 0);
    assert_eq!(
        gdi32::set_text_color(dc, 0x0011_2233),
        0x0000_0000,
        "the default text colour is black"
    );
    assert_eq!(gdi32::get_text_color(dc), 0x0011_2233);
    assert_eq!(
        gdi32::set_bk_color(dc, 0x00AA_BBCC),
        0x00FF_FFFF,
        "the default background is white"
    );
    assert_eq!(gdi32::get_bk_color(dc), 0x00AA_BBCC);
    assert_eq!(gdi32::select_object(dc, old), brush, "the reselect returns the brush");
    assert!(gdi32::delete_object(brush));
    // Same dance for a bitmap. Whether `DeleteObject` refuses an object that is still selected is not asserted: current Windows builds accept it for
    // brushes and bitmaps alike (the contract says "false"); the rule that matters is the reselect, which keeps the GDI count balanced.
    let bitmap = dc::create_compatible_bitmap(dc, 4, 4);
    let old_bitmap = gdi32::select_object(dc, bitmap);
    assert_ne!(old_bitmap, 0);
    assert_eq!(gdi32::select_object(dc, old_bitmap), bitmap);
    assert!(gdi32::delete_object(bitmap));
    assert!(dc::delete_dc(dc));
    assert_eq!(user32::get_gui_resources(process, dn::GR_GDIOBJECTS as u32), before);
    // Trap T1: CLR_INVALID is 0xFFFFFFFF, an unsigned value.
    assert_eq!(gdi32::set_text_color(0, 1), 0xFFFF_FFFF);
    assert_eq!(gdi32::set_bk_color(0, 1), 0xFFFF_FFFF);
    assert_eq!(gdi32::select_object(0, brush), 0);
    assert!(!gdi32::delete_object(0));
}

#[test]
#[ignore = "native: needs desktop"]
fn get_text_extent_point32_and_get_text_metrics_agree_on_the_line_height() {
    // new: DialogNative.GetTextExtentPoint32/GetTextMetrics (the C# reads the first five LONGs of the TEXTMETRICW).
    let _lock = native_lock();
    let dc = dc::create_compatible_dc(0);
    let size = gdi32::get_text_extent_point32(dc, "Hg").expect("extent");
    let metrics = gdi32::get_text_metrics(dc).expect("metrics");
    assert!(size.x > 0);
    assert_eq!(size.y, metrics.tmHeight);
    assert_eq!(metrics.tmHeight, metrics.tmAscent + metrics.tmDescent);
    let longer = gdi32::get_text_extent_point32(dc, "Hg Hg").expect("extent");
    assert!(longer.x > size.x);
    assert!(gdi32::get_text_extent_point32(0, "x").is_none());
    assert!(gdi32::get_text_metrics(0).is_none());
    assert!(dc::delete_dc(dc));
}

#[test]
#[ignore = "native: needs desktop"]
fn create_font_indirect_then_get_log_font_round_trips_the_face_and_height() {
    // new: DialogNative.CreateFontIndirect/GetObject with the 92-byte LOGFONTW.
    use windows::Win32::Graphics::Gdi::LOGFONTW;
    let _lock = native_lock();
    warm_up_gdi();
    let process = kernel32::get_current_process();
    let before = user32::get_gui_resources(process, dn::GR_GDIOBJECTS as u32);
    let mut log_font = LOGFONTW {
        lfHeight: -16,
        lfWeight: 700,
        lfCharSet: windows::Win32::Graphics::Gdi::FONT_CHARSET(1),
        ..Default::default()
    };
    for (slot, unit) in log_font.lfFaceName.iter_mut().zip("Segoe UI".encode_utf16()) {
        *slot = unit;
    }
    let font = gdi32::create_font_indirect(&log_font);
    assert_ne!(font, 0);
    let read = gdi32::get_log_font(font).expect("a font has a 92-byte LOGFONTW");
    assert_eq!(read.lfHeight, -16);
    assert_eq!(read.lfWeight, 700);
    assert_eq!(utf16_to_string(&read.lfFaceName), "Segoe UI");
    assert!(gdi32::delete_object(font));
    assert_eq!(user32::get_gui_resources(process, dn::GR_GDIOBJECTS as u32), before);
}

#[test]
#[ignore = "native: needs desktop"]
fn get_object_on_a_pen_writes_fewer_bytes_than_a_logfont() {
    // new: get_log_font returns None unless the OS wrote the whole 92 bytes.
    let _lock = native_lock();
    // SAFETY: plain values; the pen is deleted below.
    let pen = unsafe { CreatePen(PS_SOLID, 1, COLORREF(0)) };
    let handle = pen.0 as isize;
    assert_ne!(handle, 0);
    let mut buffer = [0u8; 92];
    let written = gdi32::get_object(handle, &mut buffer);
    assert!(written > 0 && written < 92, "LOGPEN is {written} bytes");
    assert!(gdi32::get_log_font(handle).is_none());
    assert_eq!(gdi32::get_object(handle, &mut []), 0, "an empty buffer gets nothing");
    assert!(gdi32::delete_object(handle));
    assert_eq!(gdi32::get_object(0, &mut buffer), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn non_client_metrics_for_dpi_96_has_a_message_font_height() {
    // new: DialogNative.SystemParametersInfoForDpi (the only C# use, NativeDialog.CreateDialogFont).
    let _lock = native_lock();
    let metrics = user32::non_client_metrics_for_dpi(96).expect("SPI_GETNONCLIENTMETRICS");
    assert_eq!(metrics.cbSize, 504);
    assert_ne!(metrics.lfMessageFont.lfHeight, 0);
    assert_ne!(
        metrics.lfMessageFont.lfFaceName[0], 0,
        "the message font has a face name"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn non_client_metrics_for_dpi_144_is_taller_than_at_96() {
    // new: the metrics are scaled to the requested DPI.
    let _lock = native_lock();
    let at_96 = user32::non_client_metrics_for_dpi(96)
        .expect("96")
        .lfMessageFont
        .lfHeight
        .abs();
    let at_144 = user32::non_client_metrics_for_dpi(144)
        .expect("144")
        .lfMessageFont
        .lfHeight
        .abs();
    assert!(at_144 > at_96, "{at_144} should exceed {at_96}");
}

#[test]
#[ignore = "native: needs desktop"]
fn system_parameters_info_for_dpi_with_a_bad_cb_size_is_err() {
    // new: the generic wrapper reports the OS failure (cbSize 4 is not a valid NONCLIENTMETRICSW size).
    use windows::Win32::UI::WindowsAndMessaging::NONCLIENTMETRICSW;
    let _lock = native_lock();
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: 4,
        ..Default::default()
    };
    // SAFETY: the pointer is to a live local `NONCLIENTMETRICSW`, bigger than the (invalid) size it declares.
    let result = unsafe {
        user32::system_parameters_info_for_dpi(
            dn::SPI_GETNONCLIENTMETRICS,
            4,
            (&mut metrics as *mut NONCLIENTMETRICSW).cast(),
            0,
            96,
        )
    };
    assert!(result.is_err());
}

#[test]
#[ignore = "native: needs desktop"]
fn get_gui_resources_current_process_counts_are_non_zero() {
    // new: DialogNative.GetGuiResources/GetCurrentProcess (the test process owns a window and a brush while it counts).
    let _lock = native_lock();
    assert_eq!(kernel32::get_current_process(), -1);
    let window = hidden_window("w");
    let brush = gdi32::create_solid_brush(0x00FF_0000);
    let process = kernel32::get_current_process();
    assert!(user32::get_gui_resources(process, dn::GR_GDIOBJECTS as u32) > 0);
    assert!(user32::get_gui_resources(process, dn::GR_USEROBJECTS as u32) > 0);
    assert!(gdi32::delete_object(brush));
    drop(window);
}

#[test]
#[ignore = "native: needs desktop"]
fn set_window_theme_dark_explorer_returns_ok_or_a_failure_hresult() {
    // new: DialogNative.SetWindowTheme; Windows 10 1809+ only, so no assertion on success, only on the shape of a failure.
    let _lock = native_lock();
    let window = make_window("EDIT", "", WS_POPUP, 0, (0, 0, 20, 20), 0);
    for (app, list) in [
        (Some("DarkMode_Explorer"), None),
        (Some("DarkMode_CFD"), None),
        (Some(""), Some("")),
        (None, None),
    ] {
        if let Err(hresult) = uxtheme::set_window_theme(window.0, app, list) {
            assert!(!hresult.succeeded(), "{app:?}: an Err carries a failed HRESULT");
        }
    }
}

// ---------------------------------------------------------------- native: painting and chrome

thread_local! {
    /// What `paint_proc` saw on this thread: (calls, begin_paint gave a DC, end_paint was true, get_window_dc gave a DC).
    static PAINT_RECORD: Cell<(u32, bool, bool, bool)> = const { Cell::new((0, false, false, false)) };
}

unsafe extern "system" fn paint_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == dc::WM_PAINT {
        let handle = hwnd.0 as isize;
        let session = dc::begin_paint(handle);
        let ended = session.as_ref().is_some_and(|s| dc::end_paint(handle, s));
        let window_dc = dc::get_window_dc(handle);
        if window_dc != 0 {
            user32::release_dc(handle, window_dc);
        }
        PAINT_RECORD.with(|record| {
            let (calls, _, _, _) = record.get();
            record.set((calls + 1, session.is_some_and(|s| s.hdc != 0), ended, window_dc != 0));
        });
        return LRESULT(0);
    }
    // SAFETY: the parameters are the ones this window procedure received.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn paint_class() -> &'static str {
    static REGISTER: Once = Once::new();
    const NAME: &str = "PrTest11dPaintClass";
    REGISTER.call_once(|| {
        let name = to_utf16z(NAME);
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(paint_proc),
            lpszClassName: PCWSTR(name.as_ptr()),
            ..Default::default()
        };
        // SAFETY: `class` is a complete WNDCLASSEXW and `name` outlives the call (the OS copies the name).
        let atom = unsafe { RegisterClassExW(&class) };
        assert_ne!(atom, 0, "RegisterClassExW");
    });
    NAME
}

#[test]
#[ignore = "native: needs desktop"]
fn begin_paint_end_paint_inside_wm_paint_balance() {
    // new: DialogChromeNative.BeginPaint/EndPaint/GetWindowDC. WM_PAINT is sent directly (no message loop, no visible window).
    let _lock = native_lock();
    PAINT_RECORD.with(|record| record.set((0, false, false, false)));
    warm_up_gdi();
    let process = kernel32::get_current_process();
    let window = make_window(paint_class(), "", WS_POPUP, 0, (0, 0, 40, 40), 0);
    let before = user32::get_gui_resources(process, dn::GR_GDIOBJECTS as u32);
    // SAFETY: WM_PAINT carries no pointers; the window belongs to this thread.
    unsafe {
        SendMessageW(HWND(window.0 as *mut _), dc::WM_PAINT, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    let (calls, painted, ended, window_dc) = PAINT_RECORD.with(Cell::get);
    assert_eq!(calls, 1);
    assert!(painted, "begin_paint returned a session with a DC");
    assert!(ended, "end_paint of that session is true");
    assert!(window_dc, "get_window_dc returned a DC");
    assert_eq!(
        user32::get_gui_resources(process, dn::GR_GDIOBJECTS as u32),
        before,
        "no DC leaked"
    );
}

/// A memory DC with a 4x4 bitmap selected; everything is released on drop.
struct MemoryCanvas {
    dc: Hdc,
    bitmap: Hgdi,
    old_bitmap: Hgdi,
}

impl MemoryCanvas {
    fn new() -> Self {
        let screen = user32::get_dc(0);
        let dc = dc::create_compatible_dc(screen);
        let bitmap = dc::create_compatible_bitmap(screen, 4, 4);
        user32::release_dc(0, screen);
        assert!(dc != 0 && bitmap != 0);
        let old_bitmap = gdi32::select_object(dc, bitmap);
        MemoryCanvas { dc, bitmap, old_bitmap }
    }
}

impl Drop for MemoryCanvas {
    fn drop(&mut self) {
        gdi32::select_object(self.dc, self.old_bitmap);
        gdi32::delete_object(self.bitmap);
        dc::delete_dc(self.dc);
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn get_window_dc_and_memory_dc_bitblt_copies_pixels() {
    // new: DialogChromeNative.CreateCompatibleDC/CreateCompatibleBitmap/BitBlt/DeleteDC/GetWindowDC.
    let _lock = native_lock();
    warm_up_gdi();
    let process = kernel32::get_current_process();
    let before = user32::get_gui_resources(process, dn::GR_GDIOBJECTS as u32);
    {
        let window = hidden_window("w");
        let window_dc = dc::get_window_dc(window.0);
        assert_ne!(window_dc, 0);
        assert_eq!(user32::release_dc(window.0, window_dc), 1);

        let source = MemoryCanvas::new();
        let target = MemoryCanvas::new();
        let colour = 0x0000_80FF_u32; // COLORREF 0x00BBGGRR: R = 0xFF, G = 0x80, B = 0
        let brush = gdi32::create_solid_brush(colour);
        let fill = RECT4;
        // SAFETY: `fill` is a live RECT; the DC and the brush are live handles owned by this test.
        unsafe {
            FillRect(HDC(source.dc as *mut _), &fill, HBRUSH(brush as *mut _));
        }
        assert!(gdi32::delete_object(brush));
        assert!(dc::bit_blt(target.dc, 0, 0, 4, 4, source.dc, 0, 0, dc::SRCCOPY));
        // SAFETY: a live DC handle and in-range coordinates.
        let pixel = unsafe { GetPixel(HDC(target.dc as *mut _), 2, 2) };
        assert_eq!(pixel.0, colour);
        assert!(
            !dc::bit_blt(0, 0, 0, 4, 4, source.dc, 0, 0, dc::SRCCOPY),
            "a null destination fails"
        );
    }
    assert_eq!(user32::get_gui_resources(process, dn::GR_GDIOBJECTS as u32), before);
}

const RECT4: Rect = Rect {
    left: 0,
    top: 0,
    right: 4,
    bottom: 4,
};

#[test]
#[ignore = "native: needs desktop"]
fn ellipse_and_polyline_on_a_memory_dc_return_true() {
    // new: DialogChromeNative.Ellipse/Polyline (the OS needs at least two points).
    let _lock = native_lock();
    let canvas = MemoryCanvas::new();
    assert!(dc::ellipse(canvas.dc, 0, 0, 4, 4));
    let points = [Point { x: 0, y: 0 }, Point { x: 3, y: 3 }, Point { x: 0, y: 3 }];
    assert!(dc::polyline(canvas.dc, &points));
    assert!(!dc::polyline(canvas.dc, &points[..1]), "one point is not a line");
    assert!(!dc::polyline(canvas.dc, &[]));
    assert!(!dc::ellipse(0, 0, 0, 4, 4));
}

#[test]
#[ignore = "native: needs desktop"]
fn track_mouse_event_leave_succeeds_on_a_visible_window() {
    // new: DialogChromeNative.TrackMouseEvent (TME_LEAVE asks for one WM_MOUSELEAVE; the window never receives the mouse here).
    let _lock = native_lock();
    let window = make_window(
        "STATIC",
        "",
        WS_POPUP | WS_VISIBLE,
        WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        (0, 0, 2, 2),
        0,
    );
    assert!(dc::track_mouse_event(dc::TME_LEAVE, window.0, 0));
    assert!(!dc::track_mouse_event(dc::TME_LEAVE, 0, 0), "a null window fails");
}

#[test]
#[ignore = "native: needs desktop"]
fn set_prop_get_prop_remove_prop_round_trip() {
    // new: DialogChromeNative.SetProp/GetProp/RemoveProp (the "chrome attached" flag), trap T8: one fixed spelling.
    let _lock = native_lock();
    let window = hidden_window("w");
    let name = "PhotoReviewRs.TestChromeAttached";
    assert_eq!(dc::get_prop(window.0, name), 0, "absent is 0");
    assert!(dc::set_prop(window.0, name, 1));
    assert_eq!(dc::get_prop(window.0, name), 1);
    assert_eq!(dc::get_prop(window.0, "PhotoReviewRs.OtherName"), 0);
    assert!(dc::set_prop(window.0, name, 42));
    assert_eq!(dc::remove_prop(window.0, name), 42, "remove returns the value");
    assert_eq!(dc::get_prop(window.0, name), 0);
    assert_eq!(dc::remove_prop(window.0, name), 0, "removing an absent property is 0");
    assert!(!dc::set_prop(0, name, 1), "a null window fails");
}

#[test]
#[ignore = "native: needs desktop"]
fn is_dialog_message_consumes_nothing_for_a_message_of_another_window() {
    // new: DialogNative.IsDialogMessage with a synthetic WM_NULL addressed to an unrelated window: not consumed, nothing dispatched.
    let _lock = native_lock();
    let (dialog, _record) = dialog_with_static_item_7();
    let other = hidden_window("other");
    let mut message = Msg {
        hwnd: HWND(other.0 as *mut _),
        message: 0, // WM_NULL
        ..Default::default()
    };
    // SAFETY: `message` is a live MSG for a window of this thread whose procedure ignores WM_NULL.
    let consumed = unsafe { user32::is_dialog_message(dialog.0, &mut message) };
    assert!(!consumed);
}
