//! Unit W8-11e tests: key capture, Settings scrolling, start-up calls and the Shell COM interop
//! (`pr_platform_win::win32::{key_capture, settings_native, startup, shell_native}`).
//! Pure tests: constants and enums against the `win32-constants` golden (written by W8-11a), the Shell GUIDs against the `windows` IIDs, the shape of the
//! drop target vtable (a `#[implement(IDropTarget)]` recorder called through the interface), the documented signatures.
//! Native tests (`#[ignore = "native: needs desktop"]`): each COM test runs on its own STA thread (`run_sta`, bounded by 60 s like the C# `RunSta`); no
//! dialog is ever shown, no test enters a message loop. Port of `ShellComInteropGraphicsTests` (category `Native`); the C# has no test for
//! `KeyCaptureNative`, `SettingsDialogNative` or `StartupAdditions`, so those native tests are new (`// new:`).
//! The process-wide DPI call has its own test executable (`win32_startup_dpi.rs`).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::mem::ManuallyDrop;
use std::sync::mpsc;
use std::time::Duration;

use pr_platform_win::win32::api::{kernel32 as api_kernel32, user32 as api_user32};
use pr_platform_win::win32::key_capture as kc;
use pr_platform_win::win32::settings_native as sn;
use pr_platform_win::win32::shell_native as shell;
use pr_platform_win::win32::startup;
use pr_platform_win::win32::structs::*;
use pr_test_support::golden::{self, i64_of, str_of};
use windows::Win32::Foundation::{HGLOBAL, POINTL};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemAlloc, CoUninitialize, DVASPECT_CONTENT, FORMATETC, IDataObject, IDataObject_Impl,
    STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::Ole::{
    DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE, DROPEFFECT_SCROLL, IDropTarget,
    IDropTarget_Impl,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_ALLNONSTORAGEITEMS, FOS_ALLOWMULTISELECT, FOS_CREATEPROMPT, FOS_DEFAULTNOMINIMODE, FOS_DONTADDTORECENT,
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_FORCESHOWHIDDEN, FOS_HIDEMRUPLACES, FOS_HIDEPINNEDPLACES,
    FOS_NOCHANGEDIR, FOS_NODEREFERENCELINKS, FOS_NOREADONLYRETURN, FOS_NOTESTFILECREATE, FOS_NOVALIDATE,
    FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FOS_SHAREAWARE, FOS_STRICTFILETYPES, FileOpenDialog,
    FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog, IModalWindow, IShellItem, IShellItemArray,
    SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING, SIGDN_URL,
};
use windows::core::{ComObject, GUID, Interface, PCWSTR, implement};

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
fn golden_shell_constants_equal_the_csharp_constants() {
    assert_class(
        "ScrollConstants",
        19,
        &[
            row!(SbVert, sn::SB_VERT),
            row!(SbCtl, sn::SB_CTL),
            row!(SifRange, sn::SIF_RANGE),
            row!(SifPage, sn::SIF_PAGE),
            row!(SifPos, sn::SIF_POS),
            row!(SifDisableNoScroll, sn::SIF_DISABLENOSCROLL),
            row!(SifTrackPos, sn::SIF_TRACKPOS),
            row!(SbLineUp, sn::SB_LINEUP),
            row!(SbLineDown, sn::SB_LINEDOWN),
            row!(SbPageUp, sn::SB_PAGEUP),
            row!(SbPageDown, sn::SB_PAGEDOWN),
            row!(SbThumbPosition, sn::SB_THUMBPOSITION),
            row!(SbThumbTrack, sn::SB_THUMBTRACK),
            row!(SbTop, sn::SB_TOP),
            row!(SbBottom, sn::SB_BOTTOM),
            row!(SbEndScroll, sn::SB_ENDSCROLL),
            row!(SbsVert, sn::SBS_VERT),
            row!(WsClipChildren, sn::WS_CLIPCHILDREN),
            row!(WsClipSiblings, sn::WS_CLIPSIBLINGS),
        ],
    );
    assert_class("Imm32", 1, &[row!(VkProcessKey, kc::VK_PROCESSKEY)]);
    assert_class(
        "ClipboardFormats",
        2,
        &[row!(HDrop, shell::CF_HDROP), row!(UnicodeText, shell::CF_UNICODETEXT)],
    );
    assert_class("DataViewAspect", 1, &[row!(Content, shell::DVASPECT_CONTENT)]);
    assert_class(
        "DataObjectDirection",
        2,
        &[row!(Get, shell::DATADIR_GET), row!(Set, shell::DATADIR_SET)],
    );
    // Trap T1: both HRESULTs have the high bit set, so they are negative as `i32` (the C# `unchecked((int)0x80040064)`).
    assert_class(
        "OleErrors",
        2,
        &[
            row!(DvEFormatEtc, shell::DV_E_FORMATETC),
            row!(DragDropSEffectNone, shell::DROPEFFECT_NONE),
        ],
    );
    assert_eq!(shell::DV_E_FORMATETC as u32, 0x8004_0064);
    assert_eq!(shell::HRESULT_ERROR_CANCELLED as u32, 0x8007_04C7);
    assert_eq!(shell::DV_E_FORMATETC, windows::Win32::Foundation::DV_E_FORMATETC.0);
    assert_eq!(
        shell::HRESULT_ERROR_CANCELLED,
        windows::Win32::Foundation::ERROR_CANCELLED.to_hresult().0
    );
    // `User32.DpiAwarenessContextPerMonitorAwareV2` is the one `User32` constant of this unit (the other eight belong to 11b).
    let user32 = golden_class("User32");
    let case = user32
        .iter()
        .find(|c| str_of(c, "name") == "DpiAwarenessContextPerMonitorAwareV2")
        .expect("golden constant");
    assert_eq!(
        i64_of(case, "value"),
        startup::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 as i64
    );
    assert_eq!(startup::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, -4);
}

/// The `enum` golden cases of one C# enum: (name, value).
fn golden_enum(name: &str) -> Vec<(String, i64)> {
    golden::load("win32-constants")
        .cases
        .iter()
        .filter(|c| str_of(c, "kind") == "enum" && str_of(c, "enum") == name)
        .map(|c| (str_of(c, "name").to_string(), i64_of(c, "value")))
        .collect()
}

fn assert_enum(name: &str, expected_count: usize, table: &[(&str, i64)]) {
    let golden = golden_enum(name);
    assert_eq!(golden.len(), expected_count, "{name}: golden count");
    let golden_names: BTreeSet<&str> = golden.iter().map(|(n, _)| n.as_str()).collect();
    let table_names: BTreeSet<&str> = table.iter().map(|(n, _)| *n).collect();
    assert_eq!(table.len(), table_names.len(), "{name}: duplicate row");
    assert_eq!(table_names, golden_names, "{name}: a missed or invented member");
    for (member, value) in table {
        let want = golden.iter().find(|(n, _)| n == member).map(|(_, v)| *v);
        assert_eq!(want, Some(*value), "{name}.{member}");
    }
}

#[test]
fn shell_enums_equal_the_windows_rs_values() {
    // Trap T5: compare through the typed windows-rs constants; `SIGDN` is an `i32` and `DROPEFFECT` a `u32`, the golden has the unsigned value.
    assert_enum(
        "Tymed",
        5,
        &[
            row!(Null, windows::Win32::System::Com::TYMED_NULL.0),
            row!(HGlobal, TYMED_HGLOBAL.0),
            row!(File, windows::Win32::System::Com::TYMED_FILE.0),
            row!(IStream, windows::Win32::System::Com::TYMED_ISTREAM.0),
            row!(IStorage, windows::Win32::System::Com::TYMED_ISTORAGE.0),
        ],
    );
    assert_enum(
        "DropEffect",
        5,
        &[
            row!(None, DROPEFFECT_NONE.0),
            row!(Copy, DROPEFFECT_COPY.0),
            row!(Move, DROPEFFECT_MOVE.0),
            row!(Link, DROPEFFECT_LINK.0),
            row!(Scroll, DROPEFFECT_SCROLL.0),
        ],
    );
    assert_enum(
        "FileDialogOptions",
        21,
        &[
            row!(None, 0),
            row!(OverwritePrompt, FOS_OVERWRITEPROMPT.0),
            row!(StrictFileTypes, FOS_STRICTFILETYPES.0),
            row!(NoChangeDir, FOS_NOCHANGEDIR.0),
            row!(PickFolders, FOS_PICKFOLDERS.0),
            row!(ForceFileSystem, FOS_FORCEFILESYSTEM.0),
            row!(AllNonStorageItems, FOS_ALLNONSTORAGEITEMS.0),
            row!(NoValidate, FOS_NOVALIDATE.0),
            row!(AllowMultiSelect, FOS_ALLOWMULTISELECT.0),
            row!(PathMustExist, FOS_PATHMUSTEXIST.0),
            row!(FileMustExist, FOS_FILEMUSTEXIST.0),
            row!(CreatePrompt, FOS_CREATEPROMPT.0),
            row!(ShareAware, FOS_SHAREAWARE.0),
            row!(NoReadOnlyReturn, FOS_NOREADONLYRETURN.0),
            row!(NoTestFileCreate, FOS_NOTESTFILECREATE.0),
            row!(HideMruPlaces, FOS_HIDEMRUPLACES.0),
            row!(HidePinnedPlaces, FOS_HIDEPINNEDPLACES.0),
            row!(NoDereferenceLinks, FOS_NODEREFERENCELINKS.0),
            row!(DontAddToRecent, FOS_DONTADDTORECENT.0),
            row!(ForceShowHidden, FOS_FORCESHOWHIDDEN.0),
            row!(DefaultNoMiniMode, FOS_DEFAULTNOMINIMODE.0),
        ],
    );
    assert_enum(
        "ShellItemDisplayName",
        5,
        &[
            row!(NormalDisplay, SIGDN_NORMALDISPLAY.0 as u32),
            row!(ParentRelativeParsing, SIGDN_PARENTRELATIVEPARSING.0 as u32),
            row!(DesktopAbsoluteParsing, SIGDN_DESKTOPABSOLUTEPARSING.0 as u32),
            row!(FileSystemPath, SIGDN_FILESYSPATH.0 as u32),
            row!(Url, SIGDN_URL.0 as u32),
        ],
    );
    // The shell constants of this crate agree with the typed ones.
    assert_eq!(i64::from(shell::DVASPECT_CONTENT), i64::from(DVASPECT_CONTENT.0));
    assert_eq!(shell::DATADIR_GET as i32, windows::Win32::System::Com::DATADIR_GET.0);
    assert_eq!(shell::DATADIR_SET as i32, windows::Win32::System::Com::DATADIR_SET.0);
    assert_eq!(shell::CF_HDROP, 15);
    assert_eq!(shell::CF_UNICODETEXT, 13);
    assert_eq!(shell::DROPEFFECT_NONE as u32, DROPEFFECT_NONE.0);
}

/// `GUID` as the lower-case text of the C# `ShellGuids` strings.
fn guid_text(guid: &GUID) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4[0],
        guid.data4[1],
        guid.data4[2],
        guid.data4[3],
        guid.data4[4],
        guid.data4[5],
        guid.data4[6],
        guid.data4[7]
    )
}

#[test]
fn shell_guids_equal_the_windows_rs_iids() {
    // C# ShellGuids (10 strings). The C# names `FileOpenDialog`/`FileSaveDialog` are the interface ids; `Clsid*` are the coclass ids.
    let golden = golden_class("ShellGuids");
    assert_eq!(golden.len(), 10);
    let table: Vec<(&str, GUID)> = vec![
        ("ModalWindow", IModalWindow::IID),
        ("FileDialog", IFileDialog::IID),
        ("FileOpenDialog", IFileOpenDialog::IID),
        ("ShellItem", IShellItem::IID),
        ("ShellItemArray", IShellItemArray::IID),
        ("DropTarget", IDropTarget::IID),
        ("DataObject", IDataObject::IID),
        ("ClsidFileOpenDialog", FileOpenDialog),
        ("FileSaveDialog", IFileSaveDialog::IID),
        ("ClsidFileSaveDialog", FileSaveDialog),
    ];
    let golden_names: BTreeSet<&str> = golden.iter().map(|c| str_of(c, "name")).collect();
    let table_names: BTreeSet<&str> = table.iter().map(|(n, _)| *n).collect();
    assert_eq!(table_names, golden_names, "a missed or invented GUID");
    for (name, guid) in &table {
        let case = golden
            .iter()
            .find(|c| str_of(c, "name") == *name)
            .expect("name checked above");
        assert!(
            str_of(case, "value").eq_ignore_ascii_case(&guid_text(guid)),
            "ShellGuids.{name}: {} vs {}",
            str_of(case, "value"),
            guid_text(guid)
        );
    }
}

// ---------------------------------------------------------------- drop target recorder (pure)

/// C# `RecordingDropTarget`: writes `Copy`/`Link`/`Move` into the effect, records what it received.
#[implement(IDropTarget)]
#[derive(Default)]
struct RecordingDropTarget {
    calls: RefCell<Vec<String>>,
}

#[allow(non_snake_case)] // the trait methods carry the COM names
impl IDropTarget_Impl for RecordingDropTarget_Impl {
    fn DragEnter(
        &self,
        pdataobj: windows::core::Ref<IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        // SAFETY: the caller passes a live effect pointer (the test's local).
        let incoming = unsafe { *pdweffect };
        self.calls.borrow_mut().push(format!(
            "enter:{}:{},{}:{}:{}",
            grfkeystate.0,
            pt.x,
            pt.y,
            incoming.0,
            !pdataobj.is_null()
        ));
        // SAFETY: as above; the effect is written through the pointer (trap: by pointer, not by value).
        unsafe { *pdweffect = DROPEFFECT_COPY };
        Ok(())
    }

    fn DragOver(
        &self,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        // SAFETY: the caller passes a live effect pointer (the test's local).
        let incoming = unsafe { *pdweffect };
        self.calls
            .borrow_mut()
            .push(format!("over:{}:{},{}:{}", grfkeystate.0, pt.x, pt.y, incoming.0));
        // SAFETY: as above.
        unsafe { *pdweffect = DROPEFFECT_LINK };
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.calls.borrow_mut().push("leave".to_string());
        Ok(())
    }

    fn Drop(
        &self,
        pdataobj: windows::core::Ref<IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        // SAFETY: the caller passes a live effect pointer (the test's local).
        let incoming = unsafe { *pdweffect };
        self.calls.borrow_mut().push(format!(
            "drop:{}:{},{}:{}:{}",
            grfkeystate.0,
            pt.x,
            pt.y,
            incoming.0,
            !pdataobj.is_null()
        ));
        // SAFETY: as above.
        unsafe { *pdweffect = DROPEFFECT_MOVE };
        Ok(())
    }
}

/// A data object that carries nothing: only its identity (non-null) matters to the recorder.
#[implement(IDataObject)]
struct EmptyDataObject;

#[allow(non_snake_case)] // the trait methods carry the COM names
impl IDataObject_Impl for EmptyDataObject_Impl {
    fn GetData(&self, _pformatetcin: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }
    fn GetDataHere(&self, _pformatetc: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> windows::core::Result<()> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }
    fn QueryGetData(&self, _pformatetc: *const FORMATETC) -> windows::core::HRESULT {
        windows::Win32::Foundation::DV_E_FORMATETC
    }
    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        _pformatetcout: *mut FORMATETC,
    ) -> windows::core::HRESULT {
        windows::Win32::Foundation::E_NOTIMPL
    }
    fn SetData(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: windows::core::BOOL,
    ) -> windows::core::Result<()> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }
    fn EnumFormatEtc(&self, _dwdirection: u32) -> windows::core::Result<windows::Win32::System::Com::IEnumFORMATETC> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }
    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: windows::core::Ref<windows::Win32::System::Com::IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }
    fn DUnadvise(&self, _dwconnection: u32) -> windows::core::Result<()> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }
    fn EnumDAdvise(&self) -> windows::core::Result<windows::Win32::System::Com::IEnumSTATDATA> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }
}

#[test]
fn drop_target_slots_are_enter_over_leave_drop_with_point_by_value_and_effect_by_pointer() {
    // C#: ShellComInteropGraphicsTests.DropTarget_VtableSlots3To6_AreEnterOverLeaveDrop. The C# reads the raw vtable slots 3..6 of oleidl.h; windows-rs
    // generates the vtable, so the twin calls the four methods through the `IDropTarget` interface (the same order of slots) with the same values.
    let recorder = ComObject::new(RecordingDropTarget::default());
    let target: IDropTarget = recorder.to_interface();
    let data: IDataObject = ComObject::new(EmptyDataObject).to_interface();
    let point = POINTL { x: 12, y: 34 };
    let mut effect = DROPEFFECT_MOVE;

    // SAFETY: `target` and `data` are live COM objects of this thread; `effect` is a live local passed by pointer, `point` by value.
    unsafe {
        target
            .DragEnter(&data, MODIFIERKEYS_FLAGS(8), point, &mut effect)
            .expect("DragEnter");
    }
    assert_eq!(effect, DROPEFFECT_COPY);
    // SAFETY: as above.
    unsafe {
        target
            .DragOver(MODIFIERKEYS_FLAGS(8), point, &mut effect)
            .expect("DragOver");
    }
    assert_eq!(effect, DROPEFFECT_LINK);
    // SAFETY: `DragLeave` takes no arguments.
    unsafe {
        target.DragLeave().expect("DragLeave");
    }
    effect = DROPEFFECT_COPY;
    // SAFETY: as above.
    unsafe {
        target
            .Drop(&data, MODIFIERKEYS_FLAGS(0), point, &mut effect)
            .expect("Drop");
    }
    assert_eq!(effect, DROPEFFECT_MOVE);

    assert_eq!(
        *recorder.calls.borrow(),
        ["enter:8:12,34:2:true", "over:8:12,34:1", "leave", "drop:0:12,34:1:true"]
    );
}

#[test]
fn shell_native_api_has_the_documented_shapes() {
    let _: fn(&str) -> Result<IShellItem, Hresult> = shell::create_item_from_path;
    let _: fn() -> Result<IFileOpenDialog, Hresult> = shell::create_file_open_dialog;
    let _: fn() -> Result<IFileSaveDialog, Hresult> = shell::create_file_save_dialog;
    let _: fn() -> Result<IDataObject, Hresult> = shell::create_empty_data_object;
    let _: unsafe fn(*mut u16) -> Option<String> = shell::take_co_task_string;
    let _: unsafe fn(&mut STGMEDIUM) = shell::release_stg_medium;

    let _: fn(Hwnd, isize) -> isize = kc::imm_associate_context;
    let _: fn(Hwnd) -> u32 = kc::imm_get_virtual_key;
    let _: fn(Hwnd, Hwnd) -> Hwnd = sn::set_parent;
    let _: fn(Hwnd, i32, &ScrollInfo, bool) -> i32 = sn::set_scroll_info;
    let _: fn(Hwnd, i32, &mut ScrollInfo) -> bool = sn::get_scroll_info;
    let _: fn(i32) -> i16 = sn::get_key_state;
    let _: fn(isize) -> Result<(), Win32Error> = startup::set_process_dpi_awareness_context;
    let _: fn(isize, isize) -> bool = startup::are_dpi_awareness_contexts_equal;
    let _: fn(Hwnd) -> bool = startup::is_iconic;
    let _: fn(Hwnd) -> bool = startup::set_foreground_window;
}

#[test]
fn windows_rs_scroll_constants_match() {
    use windows::Win32::UI::WindowsAndMessaging as m;
    let rows: Vec<(&str, i64, i64)> = vec![
        ("SB_VERT", sn::SB_VERT as i64, m::SB_VERT.0 as i64),
        ("SB_CTL", sn::SB_CTL as i64, m::SB_CTL.0 as i64),
        ("SIF_RANGE", sn::SIF_RANGE as i64, m::SIF_RANGE.0 as i64),
        ("SIF_PAGE", sn::SIF_PAGE as i64, m::SIF_PAGE.0 as i64),
        ("SIF_POS", sn::SIF_POS as i64, m::SIF_POS.0 as i64),
        (
            "SIF_DISABLENOSCROLL",
            sn::SIF_DISABLENOSCROLL as i64,
            m::SIF_DISABLENOSCROLL.0 as i64,
        ),
        ("SIF_TRACKPOS", sn::SIF_TRACKPOS as i64, m::SIF_TRACKPOS.0 as i64),
        ("SB_LINEUP", sn::SB_LINEUP as i64, m::SB_LINEUP.0 as i64),
        ("SB_LINEDOWN", sn::SB_LINEDOWN as i64, m::SB_LINEDOWN.0 as i64),
        ("SB_PAGEUP", sn::SB_PAGEUP as i64, m::SB_PAGEUP.0 as i64),
        ("SB_PAGEDOWN", sn::SB_PAGEDOWN as i64, m::SB_PAGEDOWN.0 as i64),
        (
            "SB_THUMBPOSITION",
            sn::SB_THUMBPOSITION as i64,
            m::SB_THUMBPOSITION.0 as i64,
        ),
        ("SB_THUMBTRACK", sn::SB_THUMBTRACK as i64, m::SB_THUMBTRACK.0 as i64),
        ("SB_TOP", sn::SB_TOP as i64, m::SB_TOP.0 as i64),
        ("SB_BOTTOM", sn::SB_BOTTOM as i64, m::SB_BOTTOM.0 as i64),
        ("SB_ENDSCROLL", sn::SB_ENDSCROLL as i64, m::SB_ENDSCROLL.0 as i64),
        ("SBS_VERT", sn::SBS_VERT as i64, m::SBS_VERT as i64),
        (
            "WS_CLIPCHILDREN",
            sn::WS_CLIPCHILDREN as i64,
            m::WS_CLIPCHILDREN.0 as i64,
        ),
        (
            "WS_CLIPSIBLINGS",
            sn::WS_CLIPSIBLINGS as i64,
            m::WS_CLIPSIBLINGS.0 as i64,
        ),
        (
            "VK_PROCESSKEY",
            kc::VK_PROCESSKEY as i64,
            windows::Win32::UI::Input::KeyboardAndMouse::VK_PROCESSKEY.0 as i64,
        ),
    ];
    for (name, ours, theirs) in rows {
        assert_eq!(ours, theirs, "{name}");
    }
}

// ---------------------------------------------------------------- native helpers

/// C# `RunSta`: runs `action` on a fresh STA thread (`CoInitializeEx(COINIT_APARTMENTTHREADED)` ... `CoUninitialize`), joins with a 60 s bound and
/// re-raises a failure of the action as a failure of the calling test.
fn run_sta(action: impl FnOnce() + Send + 'static) {
    let (sender, receiver) = mpsc::channel::<Result<(), String>>();
    std::thread::spawn(move || {
        // SAFETY: plain arguments; paired with `CoUninitialize` below on the same thread.
        let hr = unsafe { CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED) };
        let outcome = if hr.is_err() {
            Err(format!("CoInitializeEx failed: {hr:?}"))
        } else {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action));
            // SAFETY: balances the successful `CoInitializeEx` above; every COM object of `action` is dropped by now.
            unsafe { CoUninitialize() };
            result.map_err(|payload| {
                payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                    .unwrap_or_else(|| "the STA action panicked".to_string())
            })
        };
        let _ = sender.send(outcome);
    });
    match receiver.recv_timeout(Duration::from_secs(60)) {
        Ok(Ok(())) => {}
        Ok(Err(message)) => panic!("STA test failed: {message}"),
        Err(_) => panic!("the STA thread did not finish within 60 seconds"),
    }
}

fn windows_dir() -> String {
    std::env::var("SystemRoot").expect("SystemRoot")
}

/// The file-system path of a shell item (`SIGDN_FILESYSPATH`), freed through `take_co_task_string`.
fn file_system_path(item: &IShellItem) -> String {
    // SAFETY: `item` is a live COM object used on its creating STA thread.
    let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.expect("GetDisplayName");
    // SAFETY: `name` was allocated by the shell with `CoTaskMemAlloc` and is freed exactly once here.
    unsafe { shell::take_co_task_string(name.0) }.expect("a file system path")
}

// ---------------------------------------------------------------- native: Shell COM

#[test]
#[ignore = "native: needs desktop"]
fn shell_item_from_path_returns_file_system_path_and_parent() {
    // C#: ShellComInteropGraphicsTests.ShellItem_FromPath_ReturnsFileSystemPathAndParent
    run_sta(|| {
        let windows = windows_dir();
        let item = shell::create_item_from_path(&windows).expect("create_item_from_path");
        assert!(file_system_path(&item).eq_ignore_ascii_case(&windows));
        // SAFETY: a live COM object on its STA thread.
        let parent = unsafe { item.GetParent() }.expect("GetParent");
        let expected = std::path::Path::new(&windows)
            .parent()
            .expect("parent")
            .to_string_lossy()
            .into_owned();
        assert!(file_system_path(&parent).eq_ignore_ascii_case(&expected));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn create_item_from_path_for_a_missing_path_is_err_with_file_not_found() {
    // new: ShellNative.CreateItemFromPath on a path that does not exist (HRESULT_FROM_WIN32 of ERROR_FILE_NOT_FOUND or ERROR_PATH_NOT_FOUND).
    run_sta(|| {
        let path = format!("{}\\PhotoReviewRs-no-such-folder-11e\\missing.jpg", windows_dir());
        let error = shell::create_item_from_path(&path).expect_err("a missing file is an error");
        assert!(
            error.0 as u32 == 0x8007_0002 || error.0 as u32 == 0x8007_0003,
            "unexpected {error}"
        );
        assert!(!error.succeeded());
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn file_open_dialog_configure_and_read_back_without_showing() {
    // C#: ShellComInteropGraphicsTests.FileOpenDialog_ConfigureAndReadBack_WithoutShowing (the dialog is never shown).
    run_sta(|| {
        let windows = windows_dir();
        let dialog = shell::create_file_open_dialog().expect("create_file_open_dialog");
        let folder = shell::create_item_from_path(&windows).expect("folder item");

        // The file-type filter comes BEFORE the switch to folder-picking mode (afterwards `E_UNEXPECTED`); the strings stay alive until the dialog is dropped (trap T3).
        let name = to_utf16z("Ảnh JPEG");
        let spec = to_utf16z("*.jpg;*.jpeg");
        let filter = COMDLG_FILTERSPEC {
            pszName: PCWSTR(name.as_ptr()),
            pszSpec: PCWSTR(spec.as_ptr()),
        };
        // SAFETY: all COM calls below; `dialog`/`folder` are live COM objects owned by their smart pointers and used on their creating STA thread; the arguments
        // are locals that outlive the calls, and the filter strings outlive the dialog.
        unsafe {
            dialog.SetFileTypes(&[filter]).expect("SetFileTypes");
            dialog.SetFileTypeIndex(1).expect("SetFileTypeIndex");
            assert_eq!(dialog.GetFileTypeIndex().expect("GetFileTypeIndex"), 1);

            let wanted = FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_ALLOWMULTISELECT | FOS_PATHMUSTEXIST;
            dialog.SetOptions(wanted).expect("SetOptions");
            // The dialog may add default flags (FileMustExist...): only the flags that were set must stay.
            assert_eq!(dialog.GetOptions().expect("GetOptions") & wanted, wanted);

            let title = to_utf16z("Chọn thư mục ảnh");
            dialog.SetTitle(PCWSTR(title.as_ptr())).expect("SetTitle");
            let ok = to_utf16z("Chọn");
            dialog.SetOkButtonLabel(PCWSTR(ok.as_ptr())).expect("SetOkButtonLabel");
            let file_name = to_utf16z("Ảnh");
            dialog.SetFileName(PCWSTR(file_name.as_ptr())).expect("SetFileName");
            let read_back = dialog.GetFileName().expect("GetFileName");
            assert_eq!(shell::take_co_task_string(read_back.0).as_deref(), Some("Ảnh"));

            dialog.SetDefaultFolder(&folder).expect("SetDefaultFolder");
            let extension = to_utf16z("jpg");
            dialog
                .SetDefaultExtension(PCWSTR(extension.as_ptr()))
                .expect("SetDefaultExtension");
            let client = GUID::from_u128(0x5b2f0a1c_3d4e_4f60_8a71_92b3c4d5e6f7);
            dialog.SetClientGuid(&client).expect("SetClientGuid");
            let _ = dialog.ClearClientData(); // may fail when there is no client data yet; it only must not crash (slot [25])
            let label = to_utf16z("Tên");
            dialog
                .SetFileNameLabel(PCWSTR(label.as_ptr()))
                .expect("SetFileNameLabel");
            dialog.SetFolder(&folder).expect("SetFolder");
            let current = dialog.GetFolder().expect("GetFolder");
            assert!(file_system_path(&current).eq_ignore_ascii_case(&windows));
        }
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn create_file_save_dialog_set_options_overwrite_prompt_round_trips() {
    // new: ShellNative.CreateFileSaveDialog (the save dialog is never shown).
    run_sta(|| {
        let dialog = shell::create_file_save_dialog().expect("create_file_save_dialog");
        let wanted = FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
        // SAFETY: a live COM object owned by its smart pointer, used on its creating STA thread; the arguments are locals.
        unsafe {
            dialog.SetOptions(wanted).expect("SetOptions");
            assert_eq!(dialog.GetOptions().expect("GetOptions") & wanted, wanted);
            let extension = to_utf16z("png");
            dialog
                .SetDefaultExtension(PCWSTR(extension.as_ptr()))
                .expect("SetDefaultExtension");
        }
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn create_file_open_dialog_without_com_is_err_co_e_notinitialized() {
    // new: a thread that never initialised COM gets CO_E_NOTINITIALIZED (0x800401F0); skipped when COM happens to be initialised implicitly.
    // The dialog is dropped inside the thread (a COM pointer is not `Send`), only the outcome crosses.
    let outcome = std::thread::spawn(|| shell::create_file_open_dialog().map(|_| ()))
        .join()
        .expect("thread");
    match outcome {
        Err(error) => assert_eq!(error.0 as u32, 0x8004_01F0, "unexpected {error}"),
        Ok(_) => eprintln!("skipped: COM is initialised implicitly in this process"),
    }
}

/// A `CF_UNICODETEXT` / `TYMED_HGLOBAL` request as the C# builds it.
fn unicode_text_format() -> FORMATETC {
    FORMATETC {
        cfFormat: shell::CF_UNICODETEXT,
        ptd: std::ptr::null_mut(),
        dwAspect: shell::DVASPECT_CONTENT,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

/// A moveable global with `text` (NUL-terminated UTF-16), as the clipboard formats want it.
fn global_with_text(text: &str) -> isize {
    let units = to_utf16z(text);
    let handle = api_kernel32::global_alloc(0x0002, units.len() * 2).expect("GlobalAlloc"); // GMEM_MOVEABLE
    let locked = api_kernel32::global_lock(handle).expect("GlobalLock").cast::<u16>();
    // SAFETY: the block has `units.len() * 2` bytes and is locked; the regions do not overlap.
    unsafe { std::ptr::copy_nonoverlapping(units.as_ptr(), locked, units.len()) };
    api_kernel32::global_unlock(handle);
    handle
}

fn hglobal_medium(handle: isize) -> STGMEDIUM {
    STGMEDIUM {
        tymed: TYMED_HGLOBAL.0 as u32,
        u: STGMEDIUM_0 {
            hGlobal: HGLOBAL(handle as *mut _),
        },
        pUnkForRelease: ManuallyDrop::new(None),
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn data_object_set_then_get_unicode_text_round_trips() {
    // C#: ShellComInteropGraphicsTests.DataObject_SetThenGetUnicodeText_RoundTrips
    run_sta(|| {
        const TEXT: &str = "Ảnh đẹp";
        let data = shell::create_empty_data_object().expect("create_empty_data_object");
        let format = unicode_text_format();
        // SAFETY: all COM calls below; `data` is a live COM object on its creating STA thread; `format`/`medium` are locals that outlive the calls.
        unsafe {
            assert_ne!(
                data.QueryGetData(&format).0,
                0,
                "nothing stored yet (S_FALSE or DV_E_FORMATETC)"
            );

            let medium = hglobal_medium(global_with_text(TEXT));
            // fRelease = TRUE: the data object takes over the HGLOBAL.
            data.SetData(&format, &medium, true).expect("SetData");
            assert_eq!(data.QueryGetData(&format).0, 0);

            let mut result = data.GetData(&format).expect("GetData");
            assert_eq!(result.tymed, TYMED_HGLOBAL.0 as u32);
            let handle = result.u.hGlobal.0 as isize;
            let locked = api_kernel32::global_lock(handle).expect("GlobalLock").cast::<u16>();
            let length = TEXT.encode_utf16().count() + 1;
            let read = utf16_to_string(std::slice::from_raw_parts(locked, length));
            api_kernel32::global_unlock(handle);
            assert_eq!(read, TEXT);
            shell::release_stg_medium(&mut result);

            let mut hdrop = unicode_text_format();
            hdrop.cfFormat = shell::CF_HDROP;
            assert_ne!(data.QueryGetData(&hdrop).0, 0, "CF_HDROP is still absent");
        }
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn release_stg_medium_frees_the_hglobal_of_a_get_data_result() {
    // new: OleNative.ReleaseStgMedium on a medium built by the test (HGLOBAL, no release interface): the block is gone afterwards.
    let handle = global_with_text("to be released");
    let mut medium = hglobal_medium(handle);
    // SAFETY: the medium owns a fresh moveable HGLOBAL with no `pUnkForRelease`; it is released exactly once.
    unsafe { shell::release_stg_medium(&mut medium) };
    assert!(
        api_kernel32::global_lock(handle).is_err(),
        "the global handle was freed"
    );
    // `TYMED_NULL` is a no-op.
    let mut empty = STGMEDIUM::default();
    // SAFETY: a zeroed medium with `TYMED_NULL` owns nothing.
    unsafe { shell::release_stg_medium(&mut empty) };
}

#[test]
#[ignore = "native: needs desktop"]
fn take_co_task_string_null_is_none_and_frees_the_allocation() {
    // new: ShellNative.TakeCoTaskString; trap T2: null is `None`, an empty string is `Some("")`. The test cannot observe the free; it checks the text and
    // takes each allocation exactly once.
    // SAFETY: null is allowed.
    assert_eq!(unsafe { shell::take_co_task_string(std::ptr::null_mut()) }, None);
    for (text, expected) in [("Ảnh đẹp \u{1F4F7}", "Ảnh đẹp \u{1F4F7}"), ("", "")] {
        let units = to_utf16z(text);
        // SAFETY: allocates `units.len() * 2` bytes with the COM allocator, fills them from `units` (no overlap) and hands the block over once.
        let taken = unsafe {
            let block = CoTaskMemAlloc(units.len() * 2).cast::<u16>();
            assert!(!block.is_null());
            std::ptr::copy_nonoverlapping(units.as_ptr(), block, units.len());
            shell::take_co_task_string(block)
        };
        assert_eq!(taken.as_deref(), Some(expected));
    }
    // Invalid UTF-16 (a lone surrogate) is read lossily and the block is still freed.
    let invalid = [0xD800u16, 0x0041, 0];
    // SAFETY: as above.
    let taken = unsafe {
        let block = CoTaskMemAlloc(invalid.len() * 2).cast::<u16>();
        assert!(!block.is_null());
        std::ptr::copy_nonoverlapping(invalid.as_ptr(), block, invalid.len());
        shell::take_co_task_string(block)
    };
    assert_eq!(taken.as_deref(), Some("\u{FFFD}A"));
}

// ---------------------------------------------------------------- native: window calls

const WS_POPUP: u32 = 0x8000_0000;
const WS_CHILD: u32 = 0x4000_0000;

/// A window destroyed on drop (on the creating thread).
struct Win(Hwnd);

impl Drop for Win {
    fn drop(&mut self) {
        let _ = api_user32::destroy_window(self.0);
    }
}

fn make_window(class: &str, style: u32, rect: (i32, i32, i32, i32), parent: Hwnd) -> Win {
    let instance = api_kernel32::get_module_handle().expect("module handle");
    // SAFETY: built-in window classes; `param` is 0.
    let hwnd = unsafe {
        api_user32::create_window_ex(
            0, class, "", style, rect.0, rect.1, rect.2, rect.3, parent, 0, instance, 0,
        )
    }
    .expect("create_window_ex");
    assert_ne!(hwnd, 0);
    Win(hwnd)
}

fn hidden_window(class: &str) -> Win {
    make_window(class, WS_POPUP, (0, 0, 50, 50), 0)
}

#[test]
#[ignore = "native: needs desktop"]
fn imm_associate_context_zero_returns_the_previous_context_and_detaches() {
    // new: KeyCaptureNative.ImmAssociateContext(box, 0) - the shortcut box detaches the IME once and never restores.
    let window = hidden_window("EDIT");
    let _previous = kc::imm_associate_context(window.0, 0); // the default input context, or 0 on a system without an IME
    assert_eq!(
        kc::imm_associate_context(window.0, 0),
        0,
        "the window has no context any more"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn imm_get_virtual_key_without_ime_processing_returns_zero_or_vk_processkey() {
    // new: KeyCaptureNative.ImmGetVirtualKey - outside a VK_PROCESSKEY WM_KEYDOWN there is no processed key. Observed on Windows 11: the OS answers
    // VK_PROCESSKEY (229) itself instead of 0 (the contract says 0); either way the C# caller (`if (real != 0) key = real`) ends with VK_PROCESSKEY.
    let window = hidden_window("EDIT");
    let key = kc::imm_get_virtual_key(window.0);
    assert!(
        key == 0 || key == kc::VK_PROCESSKEY as u32,
        "unexpected virtual key {key}"
    );
}

/// A vertical scroll-bar control inside a hidden parent.
fn scroll_bar() -> (Win, Win) {
    let parent = hidden_window("STATIC");
    let bar = make_window("SCROLLBAR", WS_CHILD | sn::SBS_VERT, (0, 0, 20, 100), parent.0);
    (parent, bar)
}

#[test]
#[ignore = "native: needs desktop"]
fn set_scroll_info_get_scroll_info_round_trip_on_a_scroll_bar_control() {
    // new: SettingsDialogNative.SetScrollInfo/GetScrollInfo (SB_CTL; the pages read nTrackPos on SB_THUMBTRACK).
    let (_parent, bar) = scroll_bar();
    let mut info = new_scroll_info();
    info.fMask = windows::Win32::UI::WindowsAndMessaging::SCROLLINFO_MASK(sn::SIF_RANGE | sn::SIF_PAGE | sn::SIF_POS);
    info.nMin = 0;
    info.nMax = 100;
    info.nPage = 10;
    info.nPos = 30;
    info.cbSize = 0; // the wrapper sets the size
    assert_eq!(sn::set_scroll_info(bar.0, sn::SB_CTL, &info, false), 30);

    let mut read = ScrollInfo {
        fMask: windows::Win32::UI::WindowsAndMessaging::SCROLLINFO_MASK(
            sn::SIF_RANGE | sn::SIF_PAGE | sn::SIF_POS | sn::SIF_TRACKPOS,
        ),
        ..Default::default()
    };
    assert!(sn::get_scroll_info(bar.0, sn::SB_CTL, &mut read));
    assert_eq!(read.cbSize, 28);
    assert_eq!((read.nMin, read.nMax, read.nPage, read.nPos), (0, 100, 10, 30));
    assert_eq!(
        read.nTrackPos, 30,
        "no drag in progress: the track position is the position"
    );
    assert!(!sn::get_scroll_info(0, sn::SB_CTL, &mut read), "a null window fails");
}

#[test]
#[ignore = "native: needs desktop"]
fn set_scroll_info_clamps_position_to_range_minus_page_plus_one() {
    // new: the OS clamps nPos to nMax - nPage + 1 (the Settings page scroll relies on it).
    let (_parent, bar) = scroll_bar();
    let mut info = new_scroll_info();
    info.fMask = windows::Win32::UI::WindowsAndMessaging::SCROLLINFO_MASK(sn::SIF_RANGE | sn::SIF_PAGE | sn::SIF_POS);
    info.nMin = 0;
    info.nMax = 100;
    info.nPage = 10;
    info.nPos = 100;
    assert_eq!(sn::set_scroll_info(bar.0, sn::SB_CTL, &info, false), 91);
    let mut read = ScrollInfo {
        fMask: windows::Win32::UI::WindowsAndMessaging::SCROLLINFO_MASK(sn::SIF_POS),
        ..Default::default()
    };
    assert!(sn::get_scroll_info(bar.0, sn::SB_CTL, &mut read));
    assert_eq!(read.nPos, 91);
}

#[test]
#[ignore = "native: needs desktop"]
fn set_parent_moves_a_child_and_returns_the_previous_parent() {
    // new: SettingsDialogNative.SetParent (the Settings pages move template controls into the scrolling viewport).
    let first = hidden_window("STATIC");
    let second = hidden_window("STATIC");
    let child = make_window("STATIC", WS_CHILD, (0, 0, 10, 10), first.0);
    assert_eq!(sn::set_parent(child.0, second.0), first.0);
    // SAFETY: a live window handle of this thread.
    let now = unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetParent(windows::Win32::Foundation::HWND(child.0 as *mut _))
    }
    .expect("GetParent");
    assert_eq!(now.0 as isize, second.0);
    assert_eq!(sn::set_parent(0, second.0), 0, "a null child fails with 0");
}

#[test]
#[ignore = "native: needs desktop"]
fn get_key_state_of_an_unpressed_key_has_no_high_bit() {
    // new: SettingsDialogNative.GetKeyState (high bit negative = pressed). Trap T7: VK_OEM_CLEAR (0xFE) is a key nobody presses.
    assert!(sn::get_key_state(0xFE) >= 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn is_iconic_is_false_for_a_hidden_window() {
    // new: StartupAdditions.IsIconic.
    let window = hidden_window("STATIC");
    assert!(!startup::is_iconic(window.0));
    assert!(!startup::is_iconic(0));
}

#[test]
#[ignore = "native: needs desktop"]
fn set_foreground_window_on_a_hidden_window_does_not_panic() {
    // new: StartupAdditions.SetForegroundWindow - Windows may refuse (foreground lock); that is not an error, so the result is not asserted.
    let window = hidden_window("STATIC");
    let _ = startup::set_foreground_window(window.0);
    assert!(!startup::set_foreground_window(0), "a null window is refused");
}

#[test]
#[ignore = "native: needs desktop"]
fn are_dpi_awareness_contexts_equal_is_true_for_the_same_constant_and_false_for_another() {
    // new: StartupAdditions.AreDpiAwarenessContextsEqual (-4 = per-monitor v2, -2 = system aware).
    let v2 = startup::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;
    assert!(startup::are_dpi_awareness_contexts_equal(v2, v2));
    assert!(!startup::are_dpi_awareness_contexts_equal(v2, -2));
}
