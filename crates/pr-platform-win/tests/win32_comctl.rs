//! Unit W8-11c tests: common controls, TaskDialog and activation context (`pr_platform_win::win32::{comctl, control_constants, activation_context}`).
//! Pure tests: constants against the `win32-constants` golden (written by W8-11a), byte sizes against the `windows` structs, the shell manifest.
//! Native tests (`#[ignore = "native: needs desktop"]`): wrappers on hidden windows and a memory DC created by the test and destroyed by a drop guard.
//!
//! Twins of `ShellAssemblyTests.EmbeddedManifest_*` and `NativeDialogTestKit.ActivateComctl32V6`. No test calls `task_dialog_indirect` with a valid
//! configuration (it would block on a visible modal dialog) and none enters a message loop.
//!
//! Process-wide state: the loaded comctl32 version and the GDI object count are shared by the whole test process, so every native test takes
//! `NATIVE_LOCK`, and the tests that need a process that never loaded comctl32 v6 run the test executable again as a child (`run_child`).

use std::collections::BTreeSet;
use std::process::Command;
use std::sync::Mutex;

use pr_platform_win::win32::activation_context::{self, ActivationScope};
use pr_platform_win::win32::comctl::{self, SHELL_MANIFEST, SubclassProc, common_controls, gdi32, task_dialog, user32};
use pr_platform_win::win32::control_constants as cc;
use pr_platform_win::win32::messages::*;
use pr_platform_win::win32::structs::*;
use pr_test_support::golden::{self, i64_of, str_of};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{GR_GDIOBJECTS, GetGuiResources};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SendMessageW, WINDOW_EX_STYLE, WINDOW_STYLE,
};
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
    let mut golden = golden_class(class);
    golden.retain(|c| c["value"].is_number()); // the four class-name strings are checked separately
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
fn golden_comctl_constants_equal_the_csharp_constants() {
    assert_class(
        "Comctl32",
        22,
        &[
            row!(TdcbfOkButton, comctl::TDCBF_OK_BUTTON),
            row!(TdcbfYesButton, comctl::TDCBF_YES_BUTTON),
            row!(TdcbfNoButton, comctl::TDCBF_NO_BUTTON),
            row!(TdcbfCancelButton, comctl::TDCBF_CANCEL_BUTTON),
            row!(TdcbfRetryButton, comctl::TDCBF_RETRY_BUTTON),
            row!(TdcbfCloseButton, comctl::TDCBF_CLOSE_BUTTON),
            row!(TdfEnableHyperlinks, comctl::TDF_ENABLE_HYPERLINKS),
            row!(TdfUseHIconMain, comctl::TDF_USE_HICON_MAIN),
            row!(TdfUseHIconFooter, comctl::TDF_USE_HICON_FOOTER),
            row!(TdfAllowDialogCancellation, comctl::TDF_ALLOW_DIALOG_CANCELLATION),
            row!(TdfUseCommandLinks, comctl::TDF_USE_COMMAND_LINKS),
            row!(TdfExpandedByDefault, comctl::TDF_EXPANDED_BY_DEFAULT),
            row!(TdfVerificationFlagChecked, comctl::TDF_VERIFICATION_FLAG_CHECKED),
            row!(TdfPositionRelativeToWindow, comctl::TDF_POSITION_RELATIVE_TO_WINDOW),
            row!(TdfRtlLayout, comctl::TDF_RTL_LAYOUT),
            row!(TdfNoDefaultRadioButton, comctl::TDF_NO_DEFAULT_RADIO_BUTTON),
            row!(TdfCanBeMinimized, comctl::TDF_CAN_BE_MINIMIZED),
            row!(TdfSizeToContent, comctl::TDF_SIZE_TO_CONTENT),
            row!(TdWarningIcon, comctl::TD_WARNING_ICON),
            row!(TdErrorIcon, comctl::TD_ERROR_ICON),
            row!(TdInformationIcon, comctl::TD_INFORMATION_ICON),
            row!(TdShieldIcon, comctl::TD_SHIELD_ICON),
        ],
    );
    assert_class(
        "ActivationContext",
        1,
        &[row!(
            FlagResourceNameValid,
            activation_context::ACTCTX_FLAG_RESOURCE_NAME_VALID
        )],
    );
    assert_class(
        "ControlConstants",
        177,
        &[
            row!(IccListViewClasses, cc::ICC_LISTVIEW_CLASSES),
            row!(IccBarClasses, cc::ICC_BAR_CLASSES),
            row!(IccProgressClass, cc::ICC_PROGRESS_CLASS),
            row!(IccWin95Classes, cc::ICC_WIN95_CLASSES),
            row!(IccStandardClasses, cc::ICC_STANDARD_CLASSES),
            row!(WmNotify, cc::WM_NOTIFY),
            row!(WmMeasureItem, cc::WM_MEASUREITEM),
            row!(WmDrawItem, cc::WM_DRAWITEM),
            row!(WmHScroll, cc::WM_HSCROLL),
            row!(WmVScroll, cc::WM_VSCROLL),
            row!(WmMouseLeave, cc::WM_MOUSELEAVE),
            row!(WmThemeChanged, cc::WM_THEMECHANGED),
            row!(NmClick, cc::NM_CLICK),
            row!(NmDblClk, cc::NM_DBLCLK),
            row!(NmCustomDraw, cc::NM_CUSTOMDRAW),
            row!(LvnItemChanged, cc::LVN_ITEMCHANGED),
            row!(LvnColumnClick, cc::LVN_COLUMNCLICK),
            row!(TtnGetDispInfo, cc::TTN_GETDISPINFO),
            row!(CddsPrePaint, cc::CDDS_PREPAINT),
            row!(CddsItem, cc::CDDS_ITEM),
            row!(CddsItemPrePaint, cc::CDDS_ITEMPREPAINT),
            row!(CddsSubItem, cc::CDDS_SUBITEM),
            row!(CdrfDoDefault, cc::CDRF_DODEFAULT),
            row!(CdrfNewFont, cc::CDRF_NEWFONT),
            row!(CdrfSkipDefault, cc::CDRF_SKIPDEFAULT),
            row!(CdrfNotifyItemDraw, cc::CDRF_NOTIFYITEMDRAW),
            row!(CdrfNotifySubItemDraw, cc::CDRF_NOTIFYSUBITEMDRAW),
            row!(CdisSelected, cc::CDIS_SELECTED),
            row!(BsAutoCheckBox, cc::BS_AUTOCHECKBOX),
            row!(BsAutoRadioButton, cc::BS_AUTORADIOBUTTON),
            row!(BsGroupBox, cc::BS_GROUPBOX),
            row!(BmGetCheck, cc::BM_GETCHECK),
            row!(BmSetCheck, cc::BM_SETCHECK),
            row!(BstUnchecked, cc::BST_UNCHECKED),
            row!(BstChecked, cc::BST_CHECKED),
            row!(EsMultiline, cc::ES_MULTILINE),
            row!(EsAutoVScroll, cc::ES_AUTOVSCROLL),
            row!(EsReadOnly, cc::ES_READONLY),
            row!(EsWantReturn, cc::ES_WANTRETURN),
            row!(EsNumber, cc::ES_NUMBER),
            row!(SsEtchedFrame, cc::SS_ETCHEDFRAME),
            row!(SsCenterImage, cc::SS_CENTERIMAGE),
            row!(CbsDropDownList, cc::CBS_DROPDOWNLIST),
            row!(CbsHasStrings, cc::CBS_HASSTRINGS),
            row!(CbAddString, cc::CB_ADDSTRING),
            row!(CbGetCount, cc::CB_GETCOUNT),
            row!(CbGetCurSel, cc::CB_GETCURSEL),
            row!(CbResetContent, cc::CB_RESETCONTENT),
            row!(CbSetCurSel, cc::CB_SETCURSEL),
            row!(CbShowDropDown, cc::CB_SHOWDROPDOWN),
            row!(CbSetItemHeight, cc::CB_SETITEMHEIGHT),
            row!(CbGetDroppedState, cc::CB_GETDROPPEDSTATE),
            row!(CbGetLbText, cc::CB_GETLBTEXT),
            row!(CbGetLbTextLen, cc::CB_GETLBTEXTLEN),
            row!(CbnSelChange, cc::CBN_SELCHANGE),
            row!(CbnDropDown, cc::CBN_DROPDOWN),
            row!(ComboBoxInfoSize, cc::COMBOBOXINFO_SIZE),
            row!(ComboBoxInfoListOffset, cc::COMBOBOXINFO_HWNDLIST_OFFSET),
            row!(LbsMultipleSel, cc::LBS_MULTIPLESEL),
            row!(LbsExtendedSel, cc::LBS_EXTENDEDSEL),
            row!(LbsOwnerDrawFixed, cc::LBS_OWNERDRAWFIXED),
            row!(LbsHasStrings, cc::LBS_HASSTRINGS),
            row!(LbSetSel, cc::LB_SETSEL),
            row!(LbSetCurSel, cc::LB_SETCURSEL),
            row!(LbGetSel, cc::LB_GETSEL),
            row!(LbGetCurSel, cc::LB_GETCURSEL),
            row!(LbGetItemRect, cc::LB_GETITEMRECT),
            row!(LbGetItemData, cc::LB_GETITEMDATA),
            row!(LbSetItemData, cc::LB_SETITEMDATA),
            row!(LbGetSelCount, cc::LB_GETSELCOUNT),
            row!(LbSetItemHeight, cc::LB_SETITEMHEIGHT),
            row!(LbItemFromPoint, cc::LB_ITEMFROMPOINT),
            row!(LbSetTopIndex, cc::LB_SETTOPINDEX),
            row!(LbGetTopIndex, cc::LB_GETTOPINDEX),
            row!(LbSelItemRange, cc::LB_SELITEMRANGE),
            row!(LbGetCount, cc::LB_GETCOUNT),
            row!(LbDeleteString, cc::LB_DELETESTRING),
            row!(LbInsertString, cc::LB_INSERTSTRING),
            row!(LbGetText, cc::LB_GETTEXT),
            row!(LbGetTextLen, cc::LB_GETTEXTLEN),
            row!(LbnSelChange, cc::LBN_SELCHANGE),
            row!(LbnDblClk, cc::LBN_DBLCLK),
            row!(OdsSelected, cc::ODS_SELECTED),
            row!(OdsFocus, cc::ODS_FOCUS),
            row!(OdsDisabled, cc::ODS_DISABLED),
            row!(DrawItemSize, cc::DRAWITEMSTRUCT_SIZE),
            row!(MeasureItemSize, cc::MEASUREITEMSTRUCT_SIZE),
            row!(PbsSmooth, cc::PBS_SMOOTH),
            row!(PbmSetPos, cc::PBM_SETPOS),
            row!(PbmSetRange32, cc::PBM_SETRANGE32),
            row!(PbmGetPos, cc::PBM_GETPOS),
            row!(PbmSetBarColor, cc::PBM_SETBARCOLOR),
            row!(PbmSetBkColor, cc::PBM_SETBKCOLOR),
            row!(TbsAutoTicks, cc::TBS_AUTOTICKS),
            row!(TbsHorz, cc::TBS_HORZ),
            row!(TbsNoTicks, cc::TBS_NOTICKS),
            row!(TbsToolTips, cc::TBS_TOOLTIPS),
            row!(TbmGetPos, cc::TBM_GETPOS),
            row!(TbmSetPos, cc::TBM_SETPOS),
            row!(TbmSetRange, cc::TBM_SETRANGE),
            row!(TbmSetTicFreq, cc::TBM_SETTICFREQ),
            row!(TbmSetPageSize, cc::TBM_SETPAGESIZE),
            row!(TbmSetLineSize, cc::TBM_SETLINESIZE),
            row!(TbmGetRangeMin, cc::TBM_GETRANGEMIN),
            row!(TbmGetRangeMax, cc::TBM_GETRANGEMAX),
            row!(TbThumbTrack, cc::TB_THUMBTRACK),
            row!(TbEndTrack, cc::TB_ENDTRACK),
            row!(TtsAlwaysTip, cc::TTS_ALWAYSTIP),
            row!(TtsNoPrefix, cc::TTS_NOPREFIX),
            row!(TtfIdIsHwnd, cc::TTF_IDISHWND),
            row!(TtfSubclass, cc::TTF_SUBCLASS),
            row!(TtmActivate, cc::TTM_ACTIVATE),
            row!(TtmSetDelayTime, cc::TTM_SETDELAYTIME),
            row!(TtmSetTipBkColor, cc::TTM_SETTIPBKCOLOR),
            row!(TtmSetTipTextColor, cc::TTM_SETTIPTEXTCOLOR),
            row!(TtmSetMaxTipWidth, cc::TTM_SETMAXTIPWIDTH),
            row!(TtmPop, cc::TTM_POP),
            row!(TtmAddTool, cc::TTM_ADDTOOL),
            row!(TtmUpdateTipText, cc::TTM_UPDATETIPTEXT),
            row!(TtmGetToolCount, cc::TTM_GETTOOLCOUNT),
            row!(ToolInfoSize, cc::TTTOOLINFOW_V2_SIZE),
            row!(LpstrTextCallback, cc::LPSTR_TEXTCALLBACKW),
            row!(LvsReport, cc::LVS_REPORT),
            row!(LvsSingleSel, cc::LVS_SINGLESEL),
            row!(LvsShowSelAlways, cc::LVS_SHOWSELALWAYS),
            row!(LvsNoSortHeader, cc::LVS_NOSORTHEADER),
            row!(LvsExFullRowSelect, cc::LVS_EX_FULLROWSELECT),
            row!(LvsExDoubleBuffer, cc::LVS_EX_DOUBLEBUFFER),
            row!(LvsExLabelTip, cc::LVS_EX_LABELTIP),
            row!(LvmSetBkColor, cc::LVM_SETBKCOLOR),
            row!(LvmGetItemCount, cc::LVM_GETITEMCOUNT),
            row!(LvmDeleteAllItems, cc::LVM_DELETEALLITEMS),
            row!(LvmGetNextItem, cc::LVM_GETNEXTITEM),
            row!(LvmEnsureVisible, cc::LVM_ENSUREVISIBLE),
            row!(LvmGetColumnWidth, cc::LVM_GETCOLUMNWIDTH),
            row!(LvmSetColumnWidth, cc::LVM_SETCOLUMNWIDTH),
            row!(LvmGetHeader, cc::LVM_GETHEADER),
            row!(LvmSetItemState, cc::LVM_SETITEMSTATE),
            row!(LvmGetItemState, cc::LVM_GETITEMSTATE),
            row!(LvmGetSelectedCount, cc::LVM_GETSELECTEDCOUNT),
            row!(LvmSetExtendedStyle, cc::LVM_SETEXTENDEDLISTVIEWSTYLE),
            row!(LvmSetTextColor, cc::LVM_SETTEXTCOLOR),
            row!(LvmSetTextBkColor, cc::LVM_SETTEXTBKCOLOR),
            row!(LvmGetItemText, cc::LVM_GETITEMTEXT),
            row!(LvmSetItemText, cc::LVM_SETITEMTEXT),
            row!(LvmInsertColumn, cc::LVM_INSERTCOLUMN),
            row!(LvmInsertItem, cc::LVM_INSERTITEM),
            row!(LvmGetItem, cc::LVM_GETITEM),
            row!(LvmSetItem, cc::LVM_SETITEM),
            row!(LvifText, cc::LVIF_TEXT),
            row!(LvifParam, cc::LVIF_PARAM),
            row!(LvifState, cc::LVIF_STATE),
            row!(LvisFocused, cc::LVIS_FOCUSED),
            row!(LvisSelected, cc::LVIS_SELECTED),
            row!(LvcfFmt, cc::LVCF_FMT),
            row!(LvcfWidth, cc::LVCF_WIDTH),
            row!(LvcfText, cc::LVCF_TEXT),
            row!(LvcfSubItem, cc::LVCF_SUBITEM),
            row!(LvcfmtLeft, cc::LVCFMT_LEFT),
            row!(LvcfmtRight, cc::LVCFMT_RIGHT),
            row!(LvnisSelected, cc::LVNIS_SELECTED),
            row!(LvniSelected, cc::LVNI_SELECTED),
            row!(LvColumnSize, cc::LVCOLUMNW_SIZE),
            row!(LvItemSize, cc::LVITEMW_SIZE),
            row!(NmHdrSize, cc::NMHDR_SIZE),
            row!(NmListViewSize, cc::NMLISTVIEW_SIZE),
            row!(NmCustomDrawSize, cc::NMCUSTOMDRAW_SIZE),
            row!(NmLvCustomDrawSize, cc::NMLVCUSTOMDRAW_SIZE),
            row!(HdmGetItem, cc::HDM_GETITEM),
            row!(HdmSetItem, cc::HDM_SETITEM),
            row!(HdiFormat, cc::HDI_FORMAT),
            row!(HdfSortUp, cc::HDF_SORTUP),
            row!(HdfSortDown, cc::HDF_SORTDOWN),
            row!(HdItemSize, cc::HDITEMW_SIZE),
            row!(WmUser, cc::WM_USER),
            row!(WmNcDestroy, cc::WM_NCDESTROY),
            row!(WmMouseMove, cc::WM_MOUSEMOVE),
        ],
    );

    // The four string constants (window class names).
    let strings = [
        ("ListViewClass", cc::WC_LISTVIEW),
        ("ProgressClass", cc::PROGRESS_CLASS),
        ("TrackbarClass", cc::TRACKBAR_CLASS),
        ("TooltipClass", cc::TOOLTIPS_CLASS),
    ];
    let golden = golden_class("ControlConstants");
    for (name, value) in strings {
        let case = golden
            .iter()
            .find(|c| str_of(c, "name") == name)
            .unwrap_or_else(|| panic!("golden has no {name}"));
        assert_eq!(str_of(case, "value"), value, "ControlConstants.{name}");
    }
    // Trap T2/T3: the negative notification codes and the `isize` custom-draw results.
    assert_eq!(cc::NM_CUSTOMDRAW, -12);
    assert_eq!(cc::CDRF_NOTIFYITEMDRAW, cc::CDRF_NOTIFYSUBITEMDRAW);
    // `WM_USER`, `WM_NCDESTROY`, `WM_MOUSEMOVE` are the single definitions of `messages`.
    assert_eq!(
        (cc::WM_USER, cc::WM_NCDESTROY, cc::WM_MOUSEMOVE),
        (WM_USER, WM_NCDESTROY, WM_MOUSEMOVE)
    );
    let by_name = |name: &str| {
        i64_of(
            golden.iter().find(|c| str_of(c, "name") == name).expect("golden name"),
            "value",
        )
    };
    assert_eq!(by_name("WmUser"), cc::WM_USER as i64);
    assert_eq!(by_name("WmNcDestroy"), cc::WM_NCDESTROY as i64);
    assert_eq!(by_name("WmMouseMove"), cc::WM_MOUSEMOVE as i64);
}

#[test]
fn control_constants_byte_sizes_match_the_windows_structs() {
    use core::mem::{offset_of, size_of};
    use windows::Win32::UI::Controls::{
        COMBOBOXINFO, DRAWITEMSTRUCT, HDITEMW, LVCOLUMNW, LVITEMW, MEASUREITEMSTRUCT, NMCUSTOMDRAW, NMHDR, NMLISTVIEW,
        NMLVCUSTOMDRAW, TTTOOLINFOW,
    };

    assert_eq!(size_of::<COMBOBOXINFO>() as i32, cc::COMBOBOXINFO_SIZE);
    assert_eq!(size_of::<COMBOBOXINFO>(), 64);
    assert_eq!(
        offset_of!(COMBOBOXINFO, hwndList) as i32,
        cc::COMBOBOXINFO_HWNDLIST_OFFSET
    );
    assert_eq!(cc::COMBOBOXINFO_HWNDLIST_OFFSET, 56);
    assert_eq!(size_of::<DRAWITEMSTRUCT>() as i32, cc::DRAWITEMSTRUCT_SIZE);
    assert_eq!(size_of::<MEASUREITEMSTRUCT>() as i32, cc::MEASUREITEMSTRUCT_SIZE);
    // The v5-compatible `TTTOOLINFOW` ends after `lParam` (trap T5: the v6 struct is 72 bytes with `lpReserved`).
    assert_eq!(
        (offset_of!(TTTOOLINFOW, lParam) + size_of::<LPARAM>()) as i32,
        cc::TTTOOLINFOW_V2_SIZE
    );
    assert_eq!(size_of::<LVCOLUMNW>() as i32, cc::LVCOLUMNW_SIZE);
    assert_eq!(size_of::<LVITEMW>() as i32, cc::LVITEMW_SIZE);
    assert_eq!(size_of::<NMHDR>() as i32, cc::NMHDR_SIZE);
    assert_eq!(size_of::<NMCUSTOMDRAW>() as i32, cc::NMCUSTOMDRAW_SIZE);
    assert_eq!(size_of::<HDITEMW>() as i32, cc::HDITEMW_SIZE);
    // The two documented C# slips (trap T7): the constants keep the C# value because the golden says so; the structs are 64 and 136 bytes.
    assert_eq!(cc::NMLISTVIEW_SIZE, 88);
    assert_eq!(size_of::<NMLISTVIEW>(), 64);
    assert_eq!(cc::NMLVCUSTOMDRAW_SIZE, 104);
    assert_eq!(size_of::<NMLVCUSTOMDRAW>(), 136);
}

#[test]
fn task_dialog_icons_differ_from_the_windows_rs_resource_ids_on_purpose() {
    // Note 3 of the contract: the C# values are -1..-4 (`nint`); the SDK resource ids (`MAKEINTRESOURCEW(-1)`) are 0xFFFF..0xFFFC.
    assert_eq!(comctl::TD_WARNING_ICON, -1);
    assert_eq!(windows::Win32::UI::Controls::TD_WARNING_ICON.0 as usize, 0xFFFF);
    assert_eq!(comctl::TD_ERROR_ICON, -2);
    assert_eq!(windows::Win32::UI::Controls::TD_ERROR_ICON.0 as usize, 0xFFFE);
    assert_eq!(comctl::TD_INFORMATION_ICON, -3);
    assert_eq!(comctl::TD_SHIELD_ICON, -4);
    // The value stored in the union is the low 16 bits, which is what the OS reads.
    assert_eq!(comctl::TD_WARNING_ICON as u16, 0xFFFF);
    assert_eq!(comctl::TD_SHIELD_ICON as u16, 0xFFFC);
}

#[test]
fn windows_rs_has_the_same_control_message_values() {
    use windows::Win32::UI::Controls as c;
    use windows::Win32::UI::WindowsAndMessaging as m;
    macro_rules! audit {
        ($ns:ident, $name:ident) => {
            (stringify!($name), cc::$name as i64, $ns::$name as i64)
        };
    }
    let rows: Vec<(&str, i64, i64)> = vec![
        audit!(m, BM_GETCHECK),
        audit!(m, BM_SETCHECK),
        audit!(m, CB_ADDSTRING),
        audit!(m, CB_GETCOUNT),
        audit!(m, CB_GETCURSEL),
        audit!(m, CB_RESETCONTENT),
        audit!(m, CB_SETCURSEL),
        audit!(m, CB_SHOWDROPDOWN),
        audit!(m, CB_SETITEMHEIGHT),
        audit!(m, CB_GETDROPPEDSTATE),
        audit!(m, CB_GETLBTEXT),
        audit!(m, CB_GETLBTEXTLEN),
        audit!(m, LB_SETSEL),
        audit!(m, LB_SETCURSEL),
        audit!(m, LB_GETSEL),
        audit!(m, LB_GETCURSEL),
        audit!(m, LB_GETITEMRECT),
        audit!(m, LB_GETITEMDATA),
        audit!(m, LB_SETITEMDATA),
        audit!(m, LB_GETSELCOUNT),
        audit!(m, LB_SETITEMHEIGHT),
        audit!(m, LB_ITEMFROMPOINT),
        audit!(m, LB_SETTOPINDEX),
        audit!(m, LB_GETTOPINDEX),
        audit!(m, LB_SELITEMRANGE),
        audit!(m, LB_GETCOUNT),
        audit!(m, LB_DELETESTRING),
        audit!(m, LB_INSERTSTRING),
        audit!(m, LB_GETTEXT),
        audit!(m, LB_GETTEXTLEN),
        audit!(c, PBM_SETPOS),
        audit!(c, PBM_SETRANGE32),
        audit!(c, PBM_GETPOS),
        audit!(c, PBM_SETBARCOLOR),
        audit!(c, PBM_SETBKCOLOR),
        audit!(c, TBM_SETPOS),
        audit!(c, TBM_SETRANGE),
        audit!(c, TBM_SETTICFREQ),
        audit!(c, TBM_SETPAGESIZE),
        audit!(c, TBM_SETLINESIZE),
        audit!(c, TBM_GETRANGEMIN),
        audit!(c, TBM_GETRANGEMAX),
        audit!(c, TTM_ACTIVATE),
        audit!(c, TTM_SETDELAYTIME),
        audit!(c, TTM_SETTIPBKCOLOR),
        audit!(c, TTM_SETTIPTEXTCOLOR),
        audit!(c, TTM_SETMAXTIPWIDTH),
        audit!(c, TTM_POP),
        audit!(c, TTM_ADDTOOL),
        audit!(c, TTM_UPDATETIPTEXT),
        audit!(c, TTM_GETTOOLCOUNT),
        audit!(c, LVM_SETBKCOLOR),
        audit!(c, LVM_GETITEMCOUNT),
        audit!(c, LVM_DELETEALLITEMS),
        audit!(c, LVM_GETNEXTITEM),
        audit!(c, LVM_ENSUREVISIBLE),
        audit!(c, LVM_GETCOLUMNWIDTH),
        audit!(c, LVM_SETCOLUMNWIDTH),
        audit!(c, LVM_GETHEADER),
        audit!(c, LVM_SETITEMSTATE),
        audit!(c, LVM_GETITEMSTATE),
        audit!(c, LVM_GETSELECTEDCOUNT),
        audit!(c, LVM_SETEXTENDEDLISTVIEWSTYLE),
        audit!(c, LVM_SETTEXTCOLOR),
        audit!(c, LVM_SETTEXTBKCOLOR),
        audit!(c, LVM_GETITEMTEXT),
        audit!(c, LVM_SETITEMTEXT),
        audit!(c, LVM_INSERTCOLUMN),
        audit!(c, LVM_INSERTITEM),
        audit!(c, LVM_GETITEM),
        audit!(c, LVM_SETITEM),
        audit!(c, HDM_GETITEM),
        audit!(c, HDM_SETITEM),
    ];
    assert!(rows.len() >= 60, "an audit of about 60 constants or more");
    for (name, ours, theirs) in rows {
        assert_eq!(ours, theirs, "{name} differs from the windows crate");
    }
    // The `W` variants (never the `A` ones): LVM_GETITEM is LVM_GETITEMW = 0x104B.
    assert_eq!(cc::LVM_GETITEM, 0x104B);
    assert_eq!(cc::LVM_INSERTCOLUMN, 0x1061);
    assert_eq!(cc::TTM_ADDTOOL, 0x0432);
}

// ---------------------------------------------------------------- manifest

#[test]
fn manifest_declares_per_monitor_v2_dpi_awareness() {
    // C#: ShellAssemblyTests.EmbeddedManifest_DeclaresPerMonitorV2DpiAwareness
    assert!(SHELL_MANIFEST.contains("http://schemas.microsoft.com/SMI/2016/WindowsSettings"));
    let start = SHELL_MANIFEST.find("<dpiAwareness").expect("dpiAwareness element");
    let text = &SHELL_MANIFEST[start..];
    let content = &text[text.find('>').expect("end of tag") + 1..];
    assert!(content.trim_start().starts_with("PerMonitorV2"), "{content:.60}");
}

#[test]
fn manifest_depends_on_common_controls_v6() {
    // C#: ShellAssemblyTests.EmbeddedManifest_DependsOnCommonControlsV6
    let start = SHELL_MANIFEST
        .find("name=\"Microsoft.Windows.Common-Controls\"")
        .expect("Common-Controls identity");
    let element_start = SHELL_MANIFEST[..start]
        .rfind("<assemblyIdentity")
        .expect("assemblyIdentity element");
    let element = &SHELL_MANIFEST[element_start..start + SHELL_MANIFEST[start..].find("/>").expect("end of element")];
    for attribute in [
        "version=\"6.0.0.0\"",
        "type=\"win32\"",
        "processorArchitecture=\"*\"",
        "publicKeyToken=\"6595b64144ccf1df\"",
        "language=\"*\"",
    ] {
        assert!(element.contains(attribute), "{attribute} missing in {element}");
    }
    assert!(SHELL_MANIFEST.contains("<dependentAssembly>"));
    assert!(SHELL_MANIFEST.contains("requestedExecutionLevel level=\"asInvoker\""));
    assert!(SHELL_MANIFEST.contains("{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"));
}

#[test]
fn manifest_is_well_formed_enough() {
    assert!(SHELL_MANIFEST.trim_start().starts_with("<?xml"));
    assert!(SHELL_MANIFEST.contains("urn:schemas-microsoft-com:asm.v1"));
    assert_eq!(SHELL_MANIFEST.matches("<assembly ").count(), 1);
    assert_eq!(SHELL_MANIFEST.matches("</assembly>").count(), 1);
    assert!(
        SHELL_MANIFEST.contains("PhotoReviewRs"),
        "the identity is renamed (R0002)"
    );
}

// ---------------------------------------------------------------- child processes (fresh comctl32 state)

const CHILD_ENV: &str = "PR_W8_11C_CHILD";

/// Runs this test executable again with only `child_probe`, in a fresh process where nothing loaded comctl32 v6, and returns its output.
fn run_child(mode: &str) -> String {
    let exe = std::env::current_exe().expect("current_exe");
    let output = Command::new(exe)
        .args(["--exact", "child_probe", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, mode)
        .output()
        .expect("spawn the child test process");
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "child `{mode}` failed:\n{text}");
    text
}

/// Entry point of the child processes; a no-op in a normal run (the env variable is absent).
#[test]
fn child_probe() {
    let Ok(mode) = std::env::var(CHILD_ENV) else {
        return;
    };
    match mode.as_str() {
        "v5" => {
            if task_dialog::is_available() {
                eprintln!("CHILD-SKIP: this test executable already runs with a comctl32 v6 manifest");
                return;
            }
            // `is_available` loaded the comctl32 of a manifest-less process (v5.82): it has no `TaskDialogIndirect` (note 1 of the contract).
            let before = task_dialog::is_available();
            assert!(!before);
            eprintln!("CHILD-OK-v5");
        }
        "lifecycle" => {
            assert!(
                !task_dialog::is_available(),
                "a manifest-less process has no TaskDialogIndirect"
            );
            let prefix = format!("PhotoReviewRs-comctl-{}-", std::process::id());
            let leftovers = || {
                std::fs::read_dir(std::env::temp_dir())
                    .expect("read temp dir")
                    .filter_map(Result::ok)
                    .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
                    .count()
            };
            {
                let scope = ActivationScope::from_manifest_text(SHELL_MANIFEST).expect("activation scope");
                assert_eq!(
                    leftovers(),
                    0,
                    "the temp manifest file is deleted before from_manifest_text returns"
                );
                // The failed lookup above must not have poisoned this one (only successful lookups are cached).
                assert!(task_dialog::is_available(), "comctl32 v6 is active inside the scope");
                drop(scope);
            }
            // The v6 module stays loaded and the successful lookup stays cached: still resolvable (contract correction, see the report).
            assert!(task_dialog::is_available());
            eprintln!("CHILD-OK-lifecycle");
        }
        other => panic!("unknown child mode {other}"),
    }
}

#[test]
fn comctl32_v5_has_no_task_dialog_indirect_in_a_manifestless_process() {
    let output = run_child("v5");
    assert!(
        output.contains("CHILD-OK-v5") || output.contains("CHILD-SKIP"),
        "{output}"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn task_dialog_is_not_available_without_activation_and_failed_lookups_are_not_cached() {
    let output = run_child("lifecycle");
    assert!(output.contains("CHILD-OK-lifecycle"), "{output}");
}

// ---------------------------------------------------------------- native helpers

/// Serialises the native tests: the GDI object count and the loaded comctl32 are process-wide.
static NATIVE_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    NATIVE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A window destroyed on drop (children die with their parent).
struct Window(HWND);

impl Window {
    fn create(class: &str, style: u32, parent: Option<&Window>, size: (i32, i32)) -> Window {
        let class_w = to_utf16z(class);
        let title = to_utf16z("");
        // SAFETY: both strings are NUL-terminated UTF-16 buffers that live past the call; the parent is a live window of this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class_w.as_ptr()),
                PCWSTR(title.as_ptr()),
                WINDOW_STYLE(style),
                0,
                0,
                size.0,
                size.1,
                parent.map(|p| p.0),
                None,
                None,
                None,
            )
        }
        .expect("CreateWindowEx");
        Window(hwnd)
    }

    fn handle(&self) -> Hwnd {
        self.0.0 as isize
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: the window was created by this thread and is destroyed once.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

const WS_POPUP: u32 = 0x8000_0000;
const WS_CHILD: u32 = 0x4000_0000;
const CBS_DROPDOWNLIST: u32 = 0x0003;

/// A hidden top-level `STATIC` window: the parent of the test controls (never shown).
fn hidden_parent() -> Window {
    Window::create("STATIC", WS_POPUP, None, (50, 50))
}

fn gdi_objects() -> u32 {
    // SAFETY: the pseudo handle of the current process is always valid.
    unsafe { GetGuiResources(windows::Win32::System::Threading::GetCurrentProcess(), GR_GDIOBJECTS) }
}

// ---------------------------------------------------------------- native tests

#[test]
#[ignore = "native: needs desktop"]
fn activation_scope_from_manifest_text_activates_comctl32_v6_and_deletes_its_temp_file() {
    // C#: NativeDialogTestKit.ActivateComctl32V6
    let _guard = lock();
    let prefix = format!("PhotoReviewRs-comctl-{}-", std::process::id());
    let leftovers = || {
        std::fs::read_dir(std::env::temp_dir())
            .expect("read temp dir")
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .count()
    };
    let scope = ActivationScope::from_manifest_text(SHELL_MANIFEST).expect("activation scope");
    assert_eq!(leftovers(), 0);
    assert!(task_dialog::is_available());
    drop(scope);
    assert_eq!(leftovers(), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn activation_scope_from_missing_resource_fails_with_an_error_text() {
    let _guard = lock();
    let error =
        ActivationScope::from_module_resource("C:\\PhotoReviewRs\\no\\such\\module.dll", 1).expect_err("must fail");
    assert!(error.contains("CreateActCtx"), "{error}");
    assert!(error.contains("Win32 error"), "{error}");
    // A broken manifest is a creation error too, and leaves no temp file behind.
    let error = ActivationScope::from_manifest_text("this is not a manifest").expect_err("must fail");
    assert!(error.contains("CreateActCtx"), "{error}");
}

#[test]
#[ignore = "native: needs desktop"]
fn task_dialog_indirect_with_a_zero_cbsize_returns_e_invalidarg_without_showing_a_dialog() {
    let _guard = lock();
    let _scope = ActivationScope::from_manifest_text(SHELL_MANIFEST).expect("activation scope");
    assert!(task_dialog::is_available());
    let mut config = new_task_dialog_config();
    config.cbSize = 0;
    // SAFETY: the config has no pointers set (all null) and `cbSize` 0, which the OS rejects before showing anything.
    let result = unsafe { task_dialog::task_dialog_indirect(&config) };
    assert_eq!(result, Err(Hresult(0x8007_0057_u32 as i32)));
}

#[test]
#[ignore = "native: needs desktop"]
fn init_common_controls_ex_accepts_the_shell_classes() {
    let _guard = lock();
    // The three classes every comctl32 version knows. `ICC_STANDARD_CLASSES` (0x4000) is comctl32 v6 only, and a test executable without a
    // manifest binds the v5 module at load time (an `ActivationScope` cannot change an already bound import), so it is not asserted here; the
    // application exe embeds the manifest (W8-20a) and the C# callers pass all four.
    assert!(common_controls::init_common_controls_ex(
        cc::ICC_LISTVIEW_CLASSES | cc::ICC_PROGRESS_CLASS | cc::ICC_BAR_CLASSES
    ));
    // Repeating the call is harmless.
    assert!(common_controls::init_common_controls_ex(cc::ICC_BAR_CLASSES));
}

unsafe extern "system" fn test_subclass(
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
    _id: usize,
    _ref_data: usize,
) -> Lresult {
    if message == WM_APP {
        return 42;
    }
    // SAFETY: called with the parameters this subclass procedure received.
    unsafe { common_controls::def_subclass_proc(hwnd, message, wparam, lparam) }
}

#[test]
#[ignore = "native: needs desktop"]
fn set_window_subclass_intercepts_and_remove_restores() {
    let _guard = lock();
    let parent = hidden_parent();
    let child = Window::create("STATIC", WS_CHILD, Some(&parent), (10, 10));
    let procedure: SubclassProc = test_subclass;
    let send = |message: u32| {
        // SAFETY: a plain integer message to a live window of this thread.
        unsafe { SendMessageW(child.0, message, Some(WPARAM(0)), Some(LPARAM(0))) }.0
    };
    assert_eq!(send(WM_APP), 0, "the STATIC window ignores WM_APP");
    // SAFETY: `test_subclass` is a valid subclass procedure for the whole process.
    assert!(unsafe { common_controls::set_window_subclass(child.handle(), procedure, 1, 0) });
    assert_eq!(send(WM_APP), 42);
    assert_eq!(send(WM_NULL), 0, "other messages are forwarded to the next procedure");
    assert!(common_controls::remove_window_subclass(child.handle(), procedure, 1));
    assert_eq!(send(WM_APP), 0);
    assert!(
        !common_controls::remove_window_subclass(child.handle(), procedure, 1),
        "already removed"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn get_class_name_returns_the_window_class() {
    let _guard = lock();
    let window = hidden_parent();
    let name = user32::get_class_name(window.handle()).expect("class name");
    assert!(name.eq_ignore_ascii_case("Static"), "{name}");
    assert_eq!(user32::get_class_name(0), None);
}

#[test]
#[ignore = "native: needs desktop"]
fn get_parent_returns_the_parent_or_zero() {
    let _guard = lock();
    let parent = hidden_parent();
    let child = Window::create("STATIC", WS_CHILD, Some(&parent), (10, 10));
    assert_eq!(user32::get_parent(child.handle()), parent.handle());
    assert_eq!(user32::get_parent(parent.handle()), 0);
    assert_eq!(user32::get_parent(0), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn get_combo_box_info_reports_cbsize_64_and_a_list_window() {
    let _guard = lock();
    let parent = hidden_parent();
    let combo = Window::create("COMBOBOX", WS_CHILD | CBS_DROPDOWNLIST, Some(&parent), (100, 120));
    let info = user32::get_combo_box_info(combo.handle()).expect("GetComboBoxInfo");
    assert_eq!(info.cbSize, 64);
    assert_ne!(info.hwndList.0 as isize, 0, "the drop-down list window");
    assert!(user32::get_combo_box_info(0).is_none());
}

#[test]
#[ignore = "native: needs desktop"]
fn gdi_helpers_on_a_memory_dc_do_not_leak() {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateSolidBrush, DeleteDC, DeleteObject, HBRUSH, HGDIOBJ, SelectObject,
    };
    let _guard = lock();
    let cycle = || {
        // SAFETY: a memory DC compatible with the screen; deleted below.
        let dc = unsafe { CreateCompatibleDC(None) };
        assert!(!dc.is_invalid());
        let hdc = dc.0 as isize;

        let pen = gdi32::create_pen(0, 1, 0x0000_FF00);
        assert_ne!(pen, 0);
        // SAFETY: the DC and the pen are live handles created above.
        let old_pen = unsafe { SelectObject(dc, HGDIOBJ(pen as *mut core::ffi::c_void)) };
        assert!(gdi32::round_rect(hdc, 0, 0, 1, 1, 0, 0));

        // SAFETY: a solid brush, deleted below.
        let brush: HBRUSH = unsafe { CreateSolidBrush(COLORREF(0x00FF_0000)) };
        let rect = Rect {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        };
        assert_ne!(user32::fill_rect(hdc, &rect, brush.0 as isize), 0);
        assert!(user32::draw_focus_rect(hdc, &rect));

        // OPAQUE (2) is the default; the previous mode comes back, then it is restored.
        assert_eq!(gdi32::set_bk_mode(hdc, 1), 2);
        assert_eq!(gdi32::set_bk_mode(hdc, 2), 1);

        // SAFETY: the old pen goes back into the DC before the new objects are deleted; every handle is deleted once.
        unsafe {
            SelectObject(dc, old_pen);
            assert!(DeleteObject(HGDIOBJ(pen as *mut core::ffi::c_void)).as_bool());
            assert!(DeleteObject(HGDIOBJ(brush.0)).as_bool());
            assert!(DeleteDC(dc).as_bool());
        }
    };
    // The first GDI call of a process creates internal objects that the count includes: warm up, then measure a second identical cycle.
    cycle();
    let before = gdi_objects();
    cycle();
    assert_eq!(gdi_objects(), before, "GDI object count");
}

#[test]
#[ignore = "native: needs desktop"]
fn get_cursor_pos_returns_a_point() {
    let _guard = lock();
    assert!(user32::get_cursor_pos().is_some());
}
