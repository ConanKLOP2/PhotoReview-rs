//! Unit W8-11a tests: Win32 constants, struct layouts, handle and error types (`pr_platform_win::win32::{messages, structs}`).
//! Ports `InteropSignatureTests` (HotPath part, 11 layout methods) and compares against `tests/golden/win32-constants.json`
//! (oracle command `win32-constants`; class `WindowMessages` and the 16 `layout` cases; 11b..11e assert their own classes).
//! Pure tests: the only OS call is `SetLastError`/`GetLastError` on the test thread.

use core::mem::{offset_of, size_of};
use std::collections::BTreeSet;

use pr_platform_win::win32::messages::*;
use pr_platform_win::win32::structs::*;
use pr_test_support::golden::{self, i64_of, str_of};

/// `offset_of!` for a field path, or `None` for the marker `NONE` (a C# field with no Rust member).
macro_rules! field_offset {
    ($ty:ty, NONE) => {
        None
    };
    ($ty:ty, $($field:ident).+) => {
        Some(offset_of!($ty, $($field).+))
    };
}

// ---- C# InteropSignatureTests (HotPath): sizes and offsets of the structs ----

#[test]
fn rect_size_is16() {
    // C#: InteropSignatureTests.Rect_Size_Is16
    assert_eq!(size_of::<Rect>(), 16);
}

#[test]
fn point_size_is8() {
    // C#: InteropSignatureTests.Point_Size_Is8
    assert_eq!(size_of::<Point>(), 8);
}

#[test]
fn wnd_class_ex_size_is80() {
    // C#: InteropSignatureTests.WndClassEx_Size_Is80
    assert_eq!(size_of::<WndClassEx>(), 80);
    assert_eq!(offset_of!(WndClassEx, lpfnWndProc), 8);
    assert_eq!(offset_of!(WndClassEx, lpszClassName), 64);
    assert_eq!(offset_of!(WndClassEx, hIconSm), 72);
}

#[test]
fn msg_size_is48() {
    // C#: InteropSignatureTests.Msg_Size_Is48
    assert_eq!(size_of::<Msg>(), 48);
    assert_eq!(offset_of!(Msg, time), 32);
    assert_eq!(offset_of!(Msg, pt), 36);
}

#[test]
fn scroll_info_size_is28() {
    // C#: InteropSignatureTests.ScrollInfo_Size_Is28
    assert_eq!(size_of::<ScrollInfo>(), 28);
    assert_eq!(offset_of!(ScrollInfo, nPos), 20);
    assert_eq!(offset_of!(ScrollInfo, nTrackPos), 24);
}

#[test]
fn monitor_info_size_is40() {
    // C#: InteropSignatureTests.MonitorInfo_Size_Is40
    assert_eq!(size_of::<MonitorInfo>(), 40);
}

#[test]
fn monitor_info_ex_size_is104() {
    // C#: InteropSignatureTests.MonitorInfoEx_Size_Is104
    assert_eq!(size_of::<MonitorInfoEx>(), 104);
    assert_eq!(offset_of!(MonitorInfoEx, szDevice), 40);
}

#[test]
fn window_placement_size_is44() {
    // C#: InteropSignatureTests.WindowPlacement_Size_Is44
    assert_eq!(size_of::<WindowPlacement>(), 44);
}

#[test]
fn input_message_source_size_is8() {
    // C#: InteropSignatureTests.InputMessageSource_Size_Is8
    assert_eq!(size_of::<InputMessageSource>(), 8);
}

#[test]
fn task_dialog_button_size_is12() {
    // C#: InteropSignatureTests.TaskDialogButton_Size_Is12
    assert_eq!(size_of::<TaskDialogButton>(), 12);
}

#[test]
fn task_dialog_config_size_is160_and_field_offsets_match() {
    // C#: InteropSignatureTests.TaskDialogConfig_Size_Is160AndFieldOffsetsMatch
    assert_eq!(size_of::<TaskDialogConfig>(), 160);
    assert_eq!(offset_of!(TaskDialogConfig, hwndParent), 4);
    assert_eq!(offset_of!(TaskDialogConfig, pszWindowTitle), 28);
    assert_eq!(offset_of!(TaskDialogConfig, cButtons), 60);
    assert_eq!(offset_of!(TaskDialogConfig, pButtons), 64);
    assert_eq!(offset_of!(TaskDialogConfig, pfCallback), 140);
    assert_eq!(offset_of!(TaskDialogConfig, cxWidth), 156);
}

// ---- golden: constants ----

/// `row!(WmNull, WM_NULL)`: the C# name and the Rust value as `i64`.
macro_rules! row {
    ($cs:ident, $rust:expr) => {
        (stringify!($cs), $rust as i64)
    };
}

fn window_messages_table() -> Vec<(&'static str, i64)> {
    vec![
        row!(WmNull, WM_NULL),
        row!(WmCreate, WM_CREATE),
        row!(WmDestroy, WM_DESTROY),
        row!(WmMove, WM_MOVE),
        row!(WmSize, WM_SIZE),
        row!(WmActivate, WM_ACTIVATE),
        row!(WmSetFocus, WM_SETFOCUS),
        row!(WmKillFocus, WM_KILLFOCUS),
        row!(WmEnable, WM_ENABLE),
        row!(WmSetText, WM_SETTEXT),
        row!(WmPaint, WM_PAINT),
        row!(WmClose, WM_CLOSE),
        row!(WmQuit, WM_QUIT),
        row!(WmEraseBkgnd, WM_ERASEBKGND),
        row!(WmShowWindow, WM_SHOWWINDOW),
        row!(WmSettingChange, WM_SETTINGCHANGE),
        row!(WmDisplayChange, WM_DISPLAYCHANGE),
        row!(WmActivateApp, WM_ACTIVATEAPP),
        row!(WmSetCursor, WM_SETCURSOR),
        row!(WmMouseActivate, WM_MOUSEACTIVATE),
        row!(WmGetMinMaxInfo, WM_GETMINMAXINFO),
        row!(WmWindowPosChanging, WM_WINDOWPOSCHANGING),
        row!(WmWindowPosChanged, WM_WINDOWPOSCHANGED),
        row!(WmContextMenu, WM_CONTEXTMENU),
        row!(WmNcCreate, WM_NCCREATE),
        row!(WmNcDestroy, WM_NCDESTROY),
        row!(WmNcCalcSize, WM_NCCALCSIZE),
        row!(WmNcHitTest, WM_NCHITTEST),
        row!(WmNcActivate, WM_NCACTIVATE),
        row!(WmGetDlgCode, WM_GETDLGCODE),
        row!(WmInput, WM_INPUT),
        row!(WmKeyDown, WM_KEYDOWN),
        row!(WmKeyUp, WM_KEYUP),
        row!(WmChar, WM_CHAR),
        row!(WmSysKeyDown, WM_SYSKEYDOWN),
        row!(WmSysKeyUp, WM_SYSKEYUP),
        row!(WmSysChar, WM_SYSCHAR),
        row!(WmCommand, WM_COMMAND),
        row!(WmSysCommand, WM_SYSCOMMAND),
        row!(WmTimer, WM_TIMER),
        row!(WmMenuChar, WM_MENUCHAR),
        row!(WmEnterIdle, WM_ENTERIDLE),
        row!(WmMouseMove, WM_MOUSEMOVE),
        row!(WmLButtonDown, WM_LBUTTONDOWN),
        row!(WmLButtonUp, WM_LBUTTONUP),
        row!(WmLButtonDblClk, WM_LBUTTONDBLCLK),
        row!(WmRButtonDown, WM_RBUTTONDOWN),
        row!(WmRButtonUp, WM_RBUTTONUP),
        row!(WmRButtonDblClk, WM_RBUTTONDBLCLK),
        row!(WmMButtonDown, WM_MBUTTONDOWN),
        row!(WmMButtonUp, WM_MBUTTONUP),
        row!(WmMButtonDblClk, WM_MBUTTONDBLCLK),
        row!(WmMouseWheel, WM_MOUSEWHEEL),
        row!(WmXButtonDown, WM_XBUTTONDOWN),
        row!(WmXButtonUp, WM_XBUTTONUP),
        row!(WmXButtonDblClk, WM_XBUTTONDBLCLK),
        row!(WmMouseHWheel, WM_MOUSEHWHEEL),
        row!(WmCaptureChanged, WM_CAPTURECHANGED),
        row!(WmEnterSizeMove, WM_ENTERSIZEMOVE),
        row!(WmExitSizeMove, WM_EXITSIZEMOVE),
        row!(WmDropFiles, WM_DROPFILES),
        row!(WmDpiChanged, WM_DPICHANGED),
        row!(WmDpiChangedBeforeParent, WM_DPICHANGED_BEFOREPARENT),
        row!(WmDpiChangedAfterParent, WM_DPICHANGED_AFTERPARENT),
        row!(WmGetDpiScaledSize, WM_GETDPISCALEDSIZE),
        row!(WmUser, WM_USER),
        row!(WmApp, WM_APP),
        row!(WsOverlapped, WS_OVERLAPPED),
        row!(WsPopup, WS_POPUP),
        row!(WsChild, WS_CHILD),
        row!(WsVisible, WS_VISIBLE),
        row!(WsCaption, WS_CAPTION),
        row!(WsSysMenu, WS_SYSMENU),
        row!(WsThickFrame, WS_THICKFRAME),
        row!(WsMinimizeBox, WS_MINIMIZEBOX),
        row!(WsMaximizeBox, WS_MAXIMIZEBOX),
        row!(WsOverlappedWindow, WS_OVERLAPPEDWINDOW),
        row!(WsExAcceptFiles, WS_EX_ACCEPTFILES),
        row!(WsExTopmost, WS_EX_TOPMOST),
        row!(WsExToolWindow, WS_EX_TOOLWINDOW),
        row!(WsExLayered, WS_EX_LAYERED),
        row!(WsExAppWindow, WS_EX_APPWINDOW),
        row!(WsExNoRedirectionBitmap, WS_EX_NOREDIRECTIONBITMAP),
        row!(WsExNoActivate, WS_EX_NOACTIVATE),
        row!(SwpNoSize, SWP_NOSIZE),
        row!(SwpNoMove, SWP_NOMOVE),
        row!(SwpNoZOrder, SWP_NOZORDER),
        row!(SwpNoRedraw, SWP_NOREDRAW),
        row!(SwpNoActivate, SWP_NOACTIVATE),
        row!(SwpFrameChanged, SWP_FRAMECHANGED),
        row!(SwpShowWindow, SWP_SHOWWINDOW),
        row!(SwpHideWindow, SWP_HIDEWINDOW),
        row!(SwpNoOwnerZOrder, SWP_NOOWNERZORDER),
        row!(SwpNoSendChanging, SWP_NOSENDCHANGING),
        row!(HwndTop, HWND_TOP),
        row!(HwndBottom, HWND_BOTTOM),
        row!(HwndTopMost, HWND_TOPMOST),
        row!(HwndNoTopMost, HWND_NOTOPMOST),
        row!(SwHide, SW_HIDE),
        row!(SwShowNormal, SW_SHOWNORMAL),
        row!(SwShowMinimized, SW_SHOWMINIMIZED),
        row!(SwShowMaximized, SW_SHOWMAXIMIZED),
        row!(SwShowNoActivate, SW_SHOWNOACTIVATE),
        row!(SwShow, SW_SHOW),
        row!(SwMinimize, SW_MINIMIZE),
        row!(SwShowMinNoActive, SW_SHOWMINNOACTIVE),
        row!(SwShowNa, SW_SHOWNA),
        row!(SwRestore, SW_RESTORE),
        row!(SwShowDefault, SW_SHOWDEFAULT),
        row!(GwlExStyle, GWL_EXSTYLE),
        row!(GwlStyle, GWL_STYLE),
        row!(GwlpWndProc, GWLP_WNDPROC),
        row!(GwlpHInstance, GWLP_HINSTANCE),
        row!(GwlpUserData, GWLP_USERDATA),
        row!(SmCxScreen, SM_CXSCREEN),
        row!(SmCyScreen, SM_CYSCREEN),
        row!(SmCyCaption, SM_CYCAPTION),
        row!(SmCxDoubleClk, SM_CXDOUBLECLK),
        row!(SmCyDoubleClk, SM_CYDOUBLECLK),
        row!(SmCxSizeFrame, SM_CXSIZEFRAME),
        row!(SmCySizeFrame, SM_CYSIZEFRAME),
        row!(SmCxSmIcon, SM_CXSMICON),
        row!(SmCySmIcon, SM_CYSMICON),
        row!(SmCxDrag, SM_CXDRAG),
        row!(SmCyDrag, SM_CYDRAG),
        row!(MonitorDefaultToNull, MONITOR_DEFAULTTONULL),
        row!(MonitorDefaultToPrimary, MONITOR_DEFAULTTOPRIMARY),
        row!(MonitorDefaultToNearest, MONITOR_DEFAULTTONEAREST),
        row!(MonitorInfoFPrimary, MONITORINFOF_PRIMARY),
        row!(MfString, MF_STRING),
        row!(MfGrayed, MF_GRAYED),
        row!(MfDisabled, MF_DISABLED),
        row!(MfChecked, MF_CHECKED),
        row!(MfPopup, MF_POPUP),
        row!(MfByCommand, MF_BYCOMMAND),
        row!(MfByPosition, MF_BYPOSITION),
        row!(MfSeparator, MF_SEPARATOR),
        row!(TpmLeftAlign, TPM_LEFTALIGN),
        row!(TpmRightAlign, TPM_RIGHTALIGN),
        row!(TpmTopAlign, TPM_TOPALIGN),
        row!(TpmBottomAlign, TPM_BOTTOMALIGN),
        row!(TpmLeftButton, TPM_LEFTBUTTON),
        row!(TpmRightButton, TPM_RIGHTBUTTON),
        row!(TpmReturnCmd, TPM_RETURNCMD),
        row!(TpmNonotify, TPM_NONOTIFY),
        row!(CfUnicodeText, CF_UNICODETEXT),
        row!(CfHDrop, CF_HDROP),
        row!(GmemMoveable, GMEM_MOVEABLE),
        row!(GmemZeroInit, GMEM_ZEROINIT),
        row!(GHnd, GHND),
        row!(PmNoRemove, PM_NOREMOVE),
        row!(PmRemove, PM_REMOVE),
        row!(PmNoYield, PM_NOYIELD),
        row!(QsAllInput, QS_ALLINPUT),
        row!(MwmoAlertable, MWMO_ALERTABLE),
        row!(MwmoInputAvailable, MWMO_INPUTAVAILABLE),
        row!(WaitObject0, WAIT_OBJECT_0),
        row!(WaitFailed, WAIT_FAILED),
        row!(InfiniteWait, INFINITE),
        row!(IdOk, IDOK),
        row!(IdCancel, IDCANCEL),
        row!(IdYes, IDYES),
        row!(IdNo, IDNO),
    ]
}

#[test]
fn golden_window_messages_equal_the_csharp_constants() {
    let golden = golden::load("win32-constants");
    let expected: Vec<&serde_json::Value> = golden
        .cases
        .iter()
        .filter(|c| str_of(c, "kind") == "const" && str_of(c, "class") == "WindowMessages")
        .collect();
    assert_eq!(expected.len(), 163, "the golden holds 163 WindowMessages constants");

    let table = window_messages_table();
    let table_names: BTreeSet<&str> = table.iter().map(|(name, _)| *name).collect();
    let golden_names: BTreeSet<&str> = expected.iter().map(|c| str_of(c, "name")).collect();
    assert_eq!(table.len(), table_names.len(), "duplicate row in the table");
    assert_eq!(table_names, golden_names, "a missed or invented constant");

    for case in expected {
        let name = str_of(case, "name");
        let want = i64_of(case, "value");
        let got = table.iter().find(|(n, _)| *n == name).map(|(_, v)| *v);
        assert_eq!(got, Some(want), "WindowMessages.{name}");
    }
    // Trap T1: WS_POPUP is only representable as u32.
    assert_eq!(WS_POPUP as i64, 2_147_483_648);
    assert_eq!(WAIT_FAILED, 0xFFFF_FFFF);
    assert_eq!(INFINITE, 0xFFFF_FFFF);
    assert_eq!((HWND_TOPMOST, HWND_NOTOPMOST), (-1, -2));
}

// ---- golden: layouts ----

/// One row of the Rust side: golden type name, Rust size, fields.
type LayoutRow = (&'static str, usize, Vec<(&'static str, Option<usize>)>);

/// (golden type, Rust size, (golden field, Rust offset or `None` for a field with no Rust member)).
fn rust_layouts() -> Vec<LayoutRow> {
    macro_rules! l {
        ($ty:ty, $name:expr, [$(($cs:expr, $($field:tt)+)),* $(,)?]) => {
            ($name, size_of::<$ty>(), vec![$(($cs, field_offset!($ty, $($field)+))),*])
        };
    }
    vec![
        l!(
            Rect,
            "Rect",
            [("Left", left), ("Top", top), ("Right", right), ("Bottom", bottom)]
        ),
        l!(Point, "Point", [("X", x), ("Y", y)]),
        l!(
            WndClassEx,
            "WndClassEx",
            [
                ("Size", cbSize),
                ("Style", style),
                ("WndProc", lpfnWndProc),
                ("ClassExtra", cbClsExtra),
                ("WindowExtra", cbWndExtra),
                ("Instance", hInstance),
                ("Icon", hIcon),
                ("Cursor", hCursor),
                ("Background", hbrBackground),
                ("MenuName", lpszMenuName),
                ("ClassName", lpszClassName),
                ("IconSmall", hIconSm)
            ]
        ),
        // `Private` (offset 44) has no Rust member: the 4 bytes are tail padding, pinned through the size 48.
        l!(
            Msg,
            "Msg",
            [
                ("Hwnd", hwnd),
                ("Message", message),
                ("WParam", wParam),
                ("LParam", lParam),
                ("Time", time),
                ("PointX", pt.x),
                ("PointY", pt.y),
                ("Private", NONE)
            ]
        ),
        l!(
            MonitorInfo,
            "MonitorInfo",
            [
                ("Size", cbSize),
                ("Monitor", rcMonitor),
                ("Work", rcWork),
                ("Flags", dwFlags)
            ]
        ),
        l!(
            MonitorInfoEx,
            "MonitorInfoEx",
            [
                ("Size", monitorInfo.cbSize),
                ("Monitor", monitorInfo.rcMonitor),
                ("Work", monitorInfo.rcWork),
                ("Flags", monitorInfo.dwFlags),
                ("Device", szDevice)
            ]
        ),
        l!(
            WindowPlacement,
            "WindowPlacement",
            [
                ("Length", length),
                ("Flags", flags),
                ("ShowCmd", showCmd),
                ("MinPosition", ptMinPosition),
                ("MaxPosition", ptMaxPosition),
                ("NormalPosition", rcNormalPosition)
            ]
        ),
        l!(
            InputMessageSource,
            "InputMessageSource",
            [("DeviceType", deviceType), ("OriginId", originId)]
        ),
        l!(
            TaskDialogButton,
            "TaskDialogButton",
            [("Id", nButtonID), ("Text", pszButtonText)]
        ),
        l!(
            TaskDialogConfig,
            "TaskDialogConfig",
            [
                ("Size", cbSize),
                ("ParentWindow", hwndParent),
                ("Instance", hInstance),
                ("Flags", dwFlags),
                ("CommonButtons", dwCommonButtons),
                ("WindowTitle", pszWindowTitle),
                ("MainIcon", Anonymous1),
                ("MainInstruction", pszMainInstruction),
                ("Content", pszContent),
                ("ButtonCount", cButtons),
                ("Buttons", pButtons),
                ("DefaultButton", nDefaultButton),
                ("RadioButtonCount", cRadioButtons),
                ("RadioButtons", pRadioButtons),
                ("DefaultRadioButton", nDefaultRadioButton),
                ("VerificationText", pszVerificationText),
                ("ExpandedInformation", pszExpandedInformation),
                ("ExpandedControlText", pszExpandedControlText),
                ("CollapsedControlText", pszCollapsedControlText),
                ("FooterIcon", Anonymous2),
                ("Footer", pszFooter),
                ("Callback", pfCallback),
                ("CallbackData", lpCallbackData),
                ("Width", cxWidth)
            ]
        ),
        l!(
            ScrollInfo,
            "ScrollInfo",
            [
                ("Size", cbSize),
                ("Mask", fMask),
                ("Min", nMin),
                ("Max", nMax),
                ("Page", nPage),
                ("Pos", nPos),
                ("TrackPos", nTrackPos)
            ]
        ),
        l!(
            ActCtx,
            "ActivationContext+ActCtx",
            [
                ("Size", cbSize),
                ("Flags", dwFlags),
                ("Source", lpSource),
                ("ProcessorArchitecture", wProcessorArchitecture),
                ("LangId", wLangId),
                ("AssemblyDirectory", lpAssemblyDirectory),
                ("ResourceName", lpResourceName),
                ("ApplicationName", lpApplicationName),
                ("Module", hModule)
            ]
        ),
        l!(
            FormatEtc,
            "FormatEtc",
            [
                ("ClipboardFormat", cfFormat),
                ("TargetDevice", ptd),
                ("Aspect", dwAspect),
                ("Index", lindex),
                ("Medium", tymed)
            ]
        ),
        l!(
            StgMedium,
            "StgMedium",
            [("Medium", tymed), ("Handle", u), ("ReleaseUnknown", pUnkForRelease)]
        ),
        l!(PointL, "PointL", [("X", x), ("Y", y)]),
        l!(
            ComdlgFilterSpec,
            "ComdlgFilterSpec",
            [("Name", pszName), ("Spec", pszSpec)]
        ),
    ]
}

#[test]
fn golden_layouts_equal_the_csharp_layouts() {
    let golden = golden::load("win32-constants");
    let cases: Vec<&serde_json::Value> = golden.cases.iter().filter(|c| str_of(c, "kind") == "layout").collect();
    assert_eq!(cases.len(), 16, "the golden holds 16 layout cases");
    let rust = rust_layouts();
    assert_eq!(rust.len(), 16);

    for case in cases {
        let name = str_of(case, "type");
        let (_, size, fields) = rust
            .iter()
            .find(|(n, _, _)| *n == name)
            .unwrap_or_else(|| panic!("no Rust layout for the golden type {name}"));
        assert_eq!(*size as i64, i64_of(case, "size"), "{name}: size");
        let golden_fields = case["fields"].as_array().unwrap_or_else(|| panic!("{name}: no fields"));
        assert_eq!(golden_fields.len(), fields.len(), "{name}: field count");
        for (golden_field, (cs_name, rust_offset)) in golden_fields.iter().zip(fields) {
            assert_eq!(str_of(golden_field, "name"), *cs_name, "{name}: field order");
            let want = i64_of(golden_field, "offset");
            match rust_offset {
                Some(offset) => assert_eq!(*offset as i64, want, "{name}.{cs_name}"),
                // Msg.Private: tail padding in the Rust struct; the last 4 bytes end exactly at the struct size.
                None => assert_eq!(want + 4, *size as i64, "{name}.{cs_name} must end at the struct size"),
            }
        }
    }
}

// ---- audit against the `windows` crate ----

#[test]
fn windows_rs_has_the_same_values() {
    use windows::Win32::UI::WindowsAndMessaging as w;
    // Typed newtype constants (`WS_*`, `SWP_*`, ...): the value is field `.0`.
    macro_rules! audit {
        ($name:ident) => {
            (stringify!($name), $name as i64, w::$name.0 as i64)
        };
    }
    // `WM_*` are plain `u32` constants in the windows crate.
    macro_rules! audit_plain {
        ($name:ident) => {
            (stringify!($name), $name as i64, w::$name as i64)
        };
    }
    let rows: Vec<(&str, i64, i64)> = vec![
        audit_plain!(WM_NULL),
        audit_plain!(WM_CREATE),
        audit_plain!(WM_DESTROY),
        audit_plain!(WM_MOVE),
        audit_plain!(WM_SIZE),
        audit_plain!(WM_ACTIVATE),
        audit_plain!(WM_SETFOCUS),
        audit_plain!(WM_KILLFOCUS),
        audit_plain!(WM_ENABLE),
        audit_plain!(WM_SETTEXT),
        audit_plain!(WM_PAINT),
        audit_plain!(WM_CLOSE),
        audit_plain!(WM_QUIT),
        audit_plain!(WM_ERASEBKGND),
        audit_plain!(WM_SHOWWINDOW),
        audit_plain!(WM_SETTINGCHANGE),
        audit_plain!(WM_DISPLAYCHANGE),
        audit_plain!(WM_ACTIVATEAPP),
        audit_plain!(WM_SETCURSOR),
        audit_plain!(WM_MOUSEACTIVATE),
        audit_plain!(WM_GETMINMAXINFO),
        audit_plain!(WM_WINDOWPOSCHANGING),
        audit_plain!(WM_WINDOWPOSCHANGED),
        audit_plain!(WM_CONTEXTMENU),
        audit_plain!(WM_NCCREATE),
        audit_plain!(WM_NCDESTROY),
        audit_plain!(WM_NCCALCSIZE),
        audit_plain!(WM_NCHITTEST),
        audit_plain!(WM_NCACTIVATE),
        audit_plain!(WM_GETDLGCODE),
        audit_plain!(WM_INPUT),
        audit_plain!(WM_KEYDOWN),
        audit_plain!(WM_KEYUP),
        audit_plain!(WM_CHAR),
        audit_plain!(WM_SYSKEYDOWN),
        audit_plain!(WM_SYSKEYUP),
        audit_plain!(WM_SYSCHAR),
        audit_plain!(WM_COMMAND),
        audit_plain!(WM_SYSCOMMAND),
        audit_plain!(WM_TIMER),
        audit_plain!(WM_MENUCHAR),
        audit_plain!(WM_ENTERIDLE),
        audit_plain!(WM_MOUSEMOVE),
        audit_plain!(WM_LBUTTONDOWN),
        audit_plain!(WM_LBUTTONUP),
        audit_plain!(WM_LBUTTONDBLCLK),
        audit_plain!(WM_RBUTTONDOWN),
        audit_plain!(WM_RBUTTONUP),
        audit_plain!(WM_RBUTTONDBLCLK),
        audit_plain!(WM_MBUTTONDOWN),
        audit_plain!(WM_MBUTTONUP),
        audit_plain!(WM_MBUTTONDBLCLK),
        audit_plain!(WM_MOUSEWHEEL),
        audit_plain!(WM_XBUTTONDOWN),
        audit_plain!(WM_XBUTTONUP),
        audit_plain!(WM_XBUTTONDBLCLK),
        audit_plain!(WM_MOUSEHWHEEL),
        audit_plain!(WM_CAPTURECHANGED),
        audit_plain!(WM_ENTERSIZEMOVE),
        audit_plain!(WM_EXITSIZEMOVE),
        audit_plain!(WM_DROPFILES),
        audit_plain!(WM_DPICHANGED),
        audit_plain!(WM_DPICHANGED_BEFOREPARENT),
        audit_plain!(WM_DPICHANGED_AFTERPARENT),
        audit_plain!(WM_GETDPISCALEDSIZE),
        audit_plain!(WM_USER),
        audit_plain!(WM_APP),
        audit!(WS_OVERLAPPED),
        audit!(WS_POPUP),
        audit!(WS_CHILD),
        audit!(WS_VISIBLE),
        audit!(WS_CAPTION),
        audit!(WS_SYSMENU),
        audit!(WS_THICKFRAME),
        audit!(WS_MINIMIZEBOX),
        audit!(WS_MAXIMIZEBOX),
        audit!(WS_OVERLAPPEDWINDOW),
        audit!(WS_EX_ACCEPTFILES),
        audit!(WS_EX_TOPMOST),
        audit!(WS_EX_TOOLWINDOW),
        audit!(WS_EX_LAYERED),
        audit!(WS_EX_APPWINDOW),
        audit!(WS_EX_NOREDIRECTIONBITMAP),
        audit!(WS_EX_NOACTIVATE),
        audit!(SWP_NOSIZE),
        audit!(SWP_NOMOVE),
        audit!(SWP_NOZORDER),
        audit!(SWP_NOREDRAW),
        audit!(SWP_NOACTIVATE),
        audit!(SWP_FRAMECHANGED),
        audit!(SWP_SHOWWINDOW),
        audit!(SWP_HIDEWINDOW),
        audit!(SWP_NOOWNERZORDER),
        audit!(SWP_NOSENDCHANGING),
        audit!(SW_HIDE),
        audit!(SW_SHOWNORMAL),
        audit!(SW_SHOWMINIMIZED),
        audit!(SW_SHOWMAXIMIZED),
        audit!(SW_SHOWNOACTIVATE),
        audit!(SW_SHOW),
        audit!(SW_MINIMIZE),
        audit!(SW_SHOWMINNOACTIVE),
        audit!(SW_SHOWNA),
        audit!(SW_RESTORE),
        audit!(SW_SHOWDEFAULT),
        audit!(SM_CXSCREEN),
        audit!(SM_CYSCREEN),
        audit!(SM_CYCAPTION),
        audit!(SM_CXDOUBLECLK),
        audit!(SM_CYDOUBLECLK),
        audit!(SM_CXSIZEFRAME),
        audit!(SM_CYSIZEFRAME),
        audit!(SM_CXSMICON),
        audit!(SM_CYSMICON),
        audit!(SM_CXDRAG),
        audit!(SM_CYDRAG),
        audit!(MF_STRING),
        audit!(MF_GRAYED),
        audit!(MF_DISABLED),
        audit!(MF_CHECKED),
        audit!(MF_POPUP),
        audit!(MF_BYCOMMAND),
        audit!(MF_BYPOSITION),
        audit!(MF_SEPARATOR),
        audit!(TPM_LEFTALIGN),
        audit!(TPM_RIGHTALIGN),
        audit!(TPM_TOPALIGN),
        audit!(TPM_BOTTOMALIGN),
        audit!(TPM_LEFTBUTTON),
        audit!(TPM_RIGHTBUTTON),
        audit!(TPM_RETURNCMD),
        audit!(TPM_NONOTIFY),
        audit!(PM_NOREMOVE),
        audit!(PM_REMOVE),
        audit!(PM_NOYIELD),
    ];
    assert!(rows.len() >= 100, "an audit of about 100 constants or more");
    for (name, ours, theirs) in rows {
        assert_eq!(ours, theirs, "{name} differs from the windows crate");
    }
}

// ---- constructors ----

/// Every byte of `value` except the alignment `padding` (which has no defined value after a move) is zero, except the first four, which hold `size`.
fn only_size_field_set<T>(value: &T, size: u32, padding: core::ops::Range<usize>) {
    let base = (value as *const T).cast::<u8>();
    for index in (0..size_of::<T>()).filter(|index| !padding.contains(index)) {
        // SAFETY: `index < size_of::<T>()`, so the address is inside `value`; the loop skips the padding bytes, so only initialised
        // bytes of a plain-old-data struct are read.
        let byte = unsafe { base.add(index).read() };
        if index < 4 {
            assert_eq!(byte, size.to_le_bytes()[index], "size byte {index}");
        } else {
            assert_eq!(byte, 0, "{} byte {index} must be zero", core::any::type_name::<T>());
        }
    }
}

#[test]
fn new_constructors_set_the_size_field() {
    assert_eq!(new_wnd_class_ex().cbSize, 80);
    assert_eq!(new_monitor_info().cbSize, 40);
    assert_eq!(new_monitor_info_ex().monitorInfo.cbSize, 104);
    assert_eq!(new_window_placement().length, 44);
    assert_eq!(new_scroll_info().cbSize, 28);
    let config = new_task_dialog_config();
    assert_eq!({ config.cbSize }, 160);
    assert_eq!(new_act_ctx().cbSize, 56);

    only_size_field_set(&new_wnd_class_ex(), 80, 0..0);
    only_size_field_set(&new_monitor_info(), 40, 0..0);
    only_size_field_set(&new_monitor_info_ex(), 104, 0..0);
    only_size_field_set(&new_window_placement(), 44, 0..0);
    only_size_field_set(&new_scroll_info(), 28, 0..0);
    only_size_field_set(&new_task_dialog_config(), 160, 0..0);
    // ACTCTXW has 4 bytes of alignment padding after `wLangId` (offsets 20..24).
    only_size_field_set(&new_act_ctx(), 56, 20..24);
}

// ---- strings ----

#[test]
fn to_utf16z_empty_is_a_single_nul() {
    assert_eq!(to_utf16z(""), vec![0]);
}

#[test]
fn to_utf16z_ascii_ends_with_nul() {
    assert_eq!(to_utf16z("abc"), vec![0x61, 0x62, 0x63, 0]);
}

#[test]
fn to_utf16z_non_bmp_gives_a_surrogate_pair() {
    assert_eq!(to_utf16z("\u{1F600}"), vec![0xD83D, 0xDE00, 0]);
}

#[test]
fn to_utf16z_keeps_an_embedded_nul() {
    assert_eq!(to_utf16z("a\0b"), vec![0x61, 0, 0x62, 0]);
}

#[test]
fn utf16_to_string_stops_at_the_first_nul() {
    assert_eq!(utf16_to_string(&[0x61, 0, 0x62]), "a");
}

#[test]
fn utf16_to_string_without_nul_keeps_the_whole_slice() {
    assert_eq!(utf16_to_string(&[0x61, 0x62, 0x63]), "abc");
    assert_eq!(utf16_to_string(&[]), "");
}

#[test]
fn utf16_to_string_turns_a_lone_surrogate_into_u_fffd() {
    assert_eq!(utf16_to_string(&[0x61, 0xD800, 0x62]), "a\u{FFFD}b");
}

// ---- errors ----

#[test]
fn win32_error_display() {
    assert_eq!(Win32Error(5).to_string(), "Win32 error 5 (0x00000005)");
    assert_eq!(Win32Error(1400).to_string(), "Win32 error 1400 (0x00000578)");
    let boxed: Box<dyn std::error::Error> = Box::new(Win32Error(2));
    assert_eq!(boxed.to_string(), "Win32 error 2 (0x00000002)");
}

#[test]
fn hresult_display_and_succeeded() {
    assert_eq!(Hresult(-2005270523).to_string(), "HRESULT 0x887A0005");
    assert!(Hresult(0).succeeded());
    assert!(Hresult(1).succeeded());
    assert!(!Hresult(-1).succeeded());
    let boxed: Box<dyn std::error::Error> = Box::new(Hresult(-1));
    assert_eq!(boxed.to_string(), "HRESULT 0xFFFFFFFF");
}

#[test]
fn win32_error_last_reads_the_thread_error_code() {
    // SAFETY: `SetLastError` only writes the calling thread's last-error value.
    unsafe { windows::Win32::Foundation::SetLastError(windows::Win32::Foundation::WIN32_ERROR(1234)) };
    assert_eq!(Win32Error::last(), Win32Error(1234));
}
