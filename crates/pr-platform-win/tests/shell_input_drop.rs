//! Unit W8-14d tests: the OLE drop target (`pr_platform_win::shell_input::drop_target`). Port of
//! `tests/PhotoReview.Shell.Tests/Input/DropTargetTests.cs` (12 tests, baseline `d92d6aae`; `Drop_Cancellation_IsNotAFailure` and
//! `Constructor_RejectsNulls` are retired, D-W8B4I-8 / unrepresentable) plus the new tests of the contract.
//!
//! Pure tests (not ignored) call the COM object through its own `IDropTarget` interface with a fake `IDataObject` that carries a real `HGLOBAL`
//! (`FakeDropData`, the Rust twin of the C# helper in `InputTestSupport.cs`). Native tests (`#[ignore = "native: needs desktop"]`) register the target
//! on a hidden, never shown window (a `STATIC` popup) of their own STA thread and never touch the clipboard or any other window.

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use pr_platform_win::host::pending_panic;
use pr_platform_win::shell_input::drop_target::{
    DropCallbacks, DropRegisterError, FilesReader, HasFilesReader, ShellDropTarget, files, has_files, parse_drop_files,
};
use pr_platform_win::win32::api::kernel32;
use windows::Win32::Foundation::{DV_E_FORMATETC, E_FAIL, E_NOTIMPL, GlobalFree, HGLOBAL, POINTL};
use windows::Win32::System::Com::{FORMATETC, IDataObject, IDataObject_Impl, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL};
use windows::Win32::System::Memory::GlobalFlags;
use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_MOVE, DROPEFFECT_NONE, IDropTarget};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::core::{BOOL, ComObject, HRESULT, IUnknown, implement};

const CF_HDROP: u16 = 15;
const GMEM_MOVEABLE_ZEROINIT: u32 = 0x0042;
const GMEM_LOCKCOUNT: u32 = 0xFF;
/// Hang guard of the native tests.
const BOUND: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------- DROPFILES builders (T3: pFiles 0, pt 4, fNC 12, fWide 16, header 20)

/// C# `FakeDropData.BuildDropFiles` as bytes: the 20-byte header, then NUL-terminated items and the closing empty item. ANSI is Latin-1 like the C#.
fn drop_files_bytes(items: &[&str], wide: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&20u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&i32::from(wide).to_le_bytes());
    for item in items {
        if wide {
            for unit in item.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            bytes.extend_from_slice(&[0, 0]);
        } else {
            bytes.extend(item.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')));
            bytes.push(0);
        }
    }
    bytes.extend_from_slice(if wide { &[0, 0] } else { &[0] });
    bytes
}

/// A block with the given header fields and raw payload; `offset` bytes of header+padding precede the payload.
fn raw_block(offset: u32, wide: i32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&offset.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&wide.to_le_bytes());
    while bytes.len() < usize::try_from(offset).unwrap_or(0) {
        bytes.push(0);
    }
    bytes.extend_from_slice(payload);
    bytes
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// A moveable, zero-initialised `HGLOBAL` holding `bytes`.
fn alloc_global(bytes: &[u8]) -> isize {
    let handle = kernel32::global_alloc(GMEM_MOVEABLE_ZEROINIT, bytes.len()).expect("GlobalAlloc");
    let target = kernel32::global_lock(handle).expect("GlobalLock");
    // SAFETY: `target` points at the locked allocation of `bytes.len()` bytes made just above; the source is a live slice; they do not overlap.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len()) };
    kernel32::global_unlock(handle);
    handle
}

// ---------------------------------------------------------------- the fake IDataObject

#[derive(Default)]
struct Counters {
    get_data_calls: AtomicUsize,
    sentinels_dropped: AtomicUsize,
}

#[implement()]
struct Sentinel(Arc<Counters>);

impl Drop for Sentinel {
    fn drop(&mut self) {
        self.0.sentinels_dropped.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The medium owns the `HGLOBAL` (`pUnkForRelease` null: `ReleaseStgMedium` -> `GlobalFree`), like the C# fake.
    Owned,
    /// `pUnkForRelease` is a sentinel: `ReleaseStgMedium` releases it and the fake owns (and later frees) the `HGLOBAL`.
    Sentinel,
    /// `tymed` is `TYMED_ISTREAM` (4): not an `HGLOBAL`.
    WrongTymed,
    /// `tymed` is `TYMED_HGLOBAL` with a null handle (and a sentinel).
    NullHandle,
}

/// C# `FakeDropData(files, wide, getDataResult)`: `files == None` = the format is missing.
#[implement(IDataObject)]
struct FakeDropData {
    files: Option<Vec<String>>,
    wide: bool,
    get_data_result: i32,
    mode: Mode,
    counters: Arc<Counters>,
    handles: Arc<Mutex<Vec<isize>>>,
}

impl Drop for FakeDropData {
    fn drop(&mut self) {
        if self.mode == Mode::Sentinel {
            for handle in self.handles.lock().expect("handles").drain(..) {
                // SAFETY: the handles were allocated by `alloc_global` and are freed once, here (the medium did not own them in this mode).
                let _ = unsafe { GlobalFree(Some(HGLOBAL(handle as *mut c_void))) };
            }
        }
    }
}

#[allow(non_snake_case)] // the trait methods carry the COM names
impl IDataObject_Impl for FakeDropData_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        self.counters.get_data_calls.fetch_add(1, Ordering::SeqCst);
        // SAFETY: the caller passes a live FORMATETC.
        let format = unsafe { &*pformatetcin };
        if self.get_data_result != 0 {
            return Err(HRESULT(self.get_data_result).into());
        }
        let Some(files) = &self.files else {
            return Err(DV_E_FORMATETC.into());
        };
        if format.cfFormat != CF_HDROP {
            return Err(DV_E_FORMATETC.into());
        }
        let sentinel = || -> ManuallyDrop<Option<IUnknown>> {
            ManuallyDrop::new(Some(
                ComObject::new(Sentinel(self.counters.clone())).to_interface::<IUnknown>(),
            ))
        };
        let items: Vec<&str> = files.iter().map(String::as_str).collect();
        let bytes = drop_files_bytes(&items, self.wide);
        Ok(match self.mode {
            Mode::Owned => STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 {
                    hGlobal: HGLOBAL(alloc_global(&bytes) as *mut c_void),
                },
                pUnkForRelease: ManuallyDrop::new(None),
            },
            Mode::Sentinel => {
                let handle = alloc_global(&bytes);
                self.handles.lock().expect("handles").push(handle);
                STGMEDIUM {
                    tymed: TYMED_HGLOBAL.0 as u32,
                    u: STGMEDIUM_0 {
                        hGlobal: HGLOBAL(handle as *mut c_void),
                    },
                    pUnkForRelease: sentinel(),
                }
            }
            Mode::WrongTymed => STGMEDIUM {
                tymed: 4,
                u: STGMEDIUM_0 {
                    pstm: ManuallyDrop::new(None),
                },
                pUnkForRelease: sentinel(),
            },
            Mode::NullHandle => STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 {
                    hGlobal: HGLOBAL(std::ptr::null_mut()),
                },
                pUnkForRelease: sentinel(),
            },
        })
    }
    fn GetDataHere(&self, _pformatetc: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> windows::core::Result<()> {
        Err(DV_E_FORMATETC.into())
    }
    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        // SAFETY: the caller passes a live FORMATETC.
        let format = unsafe { &*pformatetc };
        if self.files.is_some() && format.cfFormat == CF_HDROP && format.tymed & (TYMED_HGLOBAL.0 as u32) != 0 {
            HRESULT(0)
        } else {
            DV_E_FORMATETC
        }
    }
    fn GetCanonicalFormatEtc(&self, _a: *const FORMATETC, _b: *mut FORMATETC) -> HRESULT {
        DV_E_FORMATETC
    }
    fn SetData(&self, _a: *const FORMATETC, _b: *const STGMEDIUM, _c: BOOL) -> windows::core::Result<()> {
        Err(DV_E_FORMATETC.into())
    }
    fn EnumFormatEtc(&self, _d: u32) -> windows::core::Result<windows::Win32::System::Com::IEnumFORMATETC> {
        Err(DV_E_FORMATETC.into())
    }
    fn DAdvise(
        &self,
        _a: *const FORMATETC,
        _b: u32,
        _c: windows::core::Ref<windows::Win32::System::Com::IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(E_NOTIMPL.into())
    }
    fn DUnadvise(&self, _a: u32) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn EnumDAdvise(&self) -> windows::core::Result<windows::Win32::System::Com::IEnumSTATDATA> {
        Err(E_NOTIMPL.into())
    }
}

struct Fake {
    data: IDataObject,
    counters: Arc<Counters>,
    handles: Arc<Mutex<Vec<isize>>>,
}

fn fake_with(files: Option<&[&str]>, wide: bool, get_data_result: i32, mode: Mode) -> Fake {
    let counters = Arc::new(Counters::default());
    let handles = Arc::new(Mutex::new(Vec::new()));
    let object = FakeDropData {
        files: files.map(|list| list.iter().map(|s| (*s).to_owned()).collect()),
        wide,
        get_data_result,
        mode,
        counters: counters.clone(),
        handles: handles.clone(),
    };
    Fake {
        data: ComObject::new(object).to_interface::<IDataObject>(),
        counters,
        handles,
    }
}

fn fake(files: Option<&[&str]>) -> Fake {
    fake_with(files, true, 0, Mode::Owned)
}

// ---------------------------------------------------------------- the sink (C# `Sink`)

#[derive(Default)]
struct Sink {
    opened: RefCell<Vec<String>>,
    failures: RefCell<Vec<String>>,
}

type OpenBehaviour = Box<dyn Fn(&str) -> Result<(), Box<dyn std::error::Error>>>;

fn callbacks(
    sink: &Rc<Sink>,
    behaviour: Option<OpenBehaviour>,
    files: Option<FilesReader>,
    has_files: Option<HasFilesReader>,
) -> DropCallbacks {
    let open_sink = Rc::clone(sink);
    let fail_sink = Rc::clone(sink);
    DropCallbacks {
        open_path: Box::new(move |path| {
            open_sink.opened.borrow_mut().push(path.to_owned());
            match &behaviour {
                Some(behaviour) => behaviour(path),
                None => Ok(()),
            }
        }),
        on_failure: Box::new(move |error| fail_sink.failures.borrow_mut().push(error.to_string())),
        files,
        has_files,
    }
}

fn create(sink: &Rc<Sink>) -> ShellDropTarget {
    ShellDropTarget::new(callbacks(sink, None, None, None))
}

const POINT: POINTL = POINTL { x: 5, y: 6 };

fn enter(target: &IDropTarget, data: &IDataObject, effect: &mut DROPEFFECT) -> windows::core::Result<()> {
    // SAFETY: `target` and `data` are live COM objects of this thread; `effect` is a live local passed by pointer, the point by value.
    unsafe { target.DragEnter(data, MODIFIERKEYS_FLAGS(0), POINT, effect) }
}

fn over(target: &IDropTarget, effect: &mut DROPEFFECT) -> windows::core::Result<()> {
    // SAFETY: as in `enter`.
    unsafe { target.DragOver(MODIFIERKEYS_FLAGS(0), POINT, effect) }
}

fn leave(target: &IDropTarget) -> windows::core::Result<()> {
    // SAFETY: `DragLeave` takes no arguments.
    unsafe { target.DragLeave() }
}

fn drop_on(target: &IDropTarget, data: &IDataObject, effect: &mut DROPEFFECT) -> windows::core::Result<()> {
    // SAFETY: as in `enter`.
    unsafe { target.Drop(data, MODIFIERKEYS_FLAGS(0), POINT, effect) }
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

// ---------------------------------------------------------------- DropData (C# `Files_*`, `Parse_*`)

#[test]
fn files_reads_a_wide_hdrop_list_in_order() {
    // C#: DropTargetTests.Files_ReadsAWideHDropList_InOrder
    let list = [r"C:\a\one.jpg", r"D:\b b\two.png", r"E:\3.jpg"];
    let data = fake(Some(&list));
    assert!(has_files(&data.data));
    assert_eq!(files(&data.data), strings(&list));
}

#[test]
fn files_reads_an_ansi_hdrop_list() {
    // C#: DropTargetTests.Files_ReadsAnAnsiHDropList
    let list = [r"C:\a\one.jpg", r"C:\two.jpg"];
    let data = fake_with(Some(&list), false, 0, Mode::Owned);
    assert_eq!(files(&data.data), strings(&list));
}

#[test]
fn files_empty_list_format_missing_or_get_data_failing_are_empty() {
    // C#: DropTargetTests.Files_EmptyListFormatMissingOrGetDataFailing_AreEmpty
    assert!(files(&fake(Some(&[])).data).is_empty());
    assert!(!has_files(&fake(None).data));
    assert!(files(&fake(None).data).is_empty());
    let failing = fake_with(Some(&[r"C:\x.jpg"]), true, E_FAIL.0, Mode::Owned);
    assert!(files(&failing.data).is_empty());
    assert_eq!(failing.counters.get_data_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn parse_stops_at_the_double_nul_and_honours_the_list_offset() {
    // C#: DropTargetTests.Parse_StopsAtTheDoubleNul_AndHonoursTheListOffset (the C# builds the block on an HGLOBAL; here the same bytes go through one)
    let bytes = drop_files_bytes(&[r"C:\x", r"C:\y"], true);
    let handle = alloc_global(&bytes);
    let ptr = kernel32::global_lock(handle).expect("lock");
    // SAFETY: the block is locked and `bytes.len()` long (made by `alloc_global`).
    let locked = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>().cast_const(), bytes.len()) };
    assert_eq!(parse_drop_files(locked), strings(&[r"C:\x", r"C:\y"]));
    kernel32::global_unlock(handle);
    // SAFETY: allocated above, freed once.
    let _ = unsafe { GlobalFree(Some(HGLOBAL(handle as *mut c_void))) };
}

/// Latin-1 `0xE9` is `e acute` only on code page 1252; the test is meaningful (and runs) only there.
#[test]
fn parse_ansi_with_a_code_page_character_decodes_with_the_system_code_page() {
    // new: ACP decode (open question 2)
    // SAFETY: no arguments.
    if unsafe { windows::Win32::Globalization::GetACP() } != 1252 {
        return;
    }
    let block = raw_block(20, 0, b"C:\\caf\xE9.jpg\0\0");
    assert_eq!(parse_drop_files(&block), strings(&["C:\\caf\u{e9}.jpg"]));
}

#[test]
fn parse_drop_files_table() {
    // new: the contract table
    let two = [r"C:\a", r"D:\b"];
    let wide_two = drop_files_bytes(&two, true);
    let ansi_two = drop_files_bytes(&two, false);
    assert_eq!(parse_drop_files(&wide_two), strings(&two), "wide");
    assert_eq!(parse_drop_files(&ansi_two), strings(&two), "ansi ascii");
    assert!(
        parse_drop_files(&drop_files_bytes(&[], true)).is_empty(),
        "empty list, wide"
    );
    assert!(
        parse_drop_files(&drop_files_bytes(&[], false)).is_empty(),
        "empty list, ansi"
    );
    assert!(parse_drop_files(&raw_block(20, 1, &[])).is_empty(), "header only");
    assert!(
        parse_drop_files(&raw_block(21, 1, &[])).is_empty(),
        "offset one past the end of a header-only block"
    );
    assert!(
        parse_drop_files(&raw_block(20, 1, &[])[..19]).is_empty(),
        "header shorter than 20 bytes"
    );
    assert!(parse_drop_files(&[]).is_empty(), "empty block");
    assert!(
        parse_drop_files(&raw_block(5000, 1, &utf16("a\0\0"))[..30]).is_empty(),
        "offset past the end"
    );
    let mut huge = wide_two.clone();
    huge[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(parse_drop_files(&huge).is_empty(), "offset u32::MAX");
    // A non-default pFiles offset (24): the list starts after 4 bytes of padding.
    assert_eq!(
        parse_drop_files(&raw_block(24, 1, &utf16("x\0y\0\0"))),
        strings(&["x", "y"]),
        "offset 24"
    );
    // fWide is any non-zero value.
    assert_eq!(
        parse_drop_files(&raw_block(20, 2, &utf16("x\0\0"))),
        strings(&["x"]),
        "fWide 2"
    );
    assert_eq!(
        parse_drop_files(&raw_block(20, -1, &utf16("x\0\0"))),
        strings(&["x"]),
        "fWide -1"
    );
    // Truncated item: the item without a NUL is dropped, the ones before it are kept.
    assert_eq!(
        parse_drop_files(&raw_block(20, 1, &utf16("a\0bc"))),
        strings(&["a"]),
        "truncated wide item"
    );
    assert!(
        parse_drop_files(&raw_block(20, 1, &utf16("abc"))).is_empty(),
        "wide item without NUL"
    );
    assert_eq!(
        parse_drop_files(&raw_block(20, 0, b"a\0bc")),
        strings(&["a"]),
        "truncated ansi item"
    );
    assert!(
        parse_drop_files(&raw_block(20, 0, b"abc")).is_empty(),
        "ansi item without NUL"
    );
    // No closing empty item: the last item still has its NUL and is kept.
    assert_eq!(
        parse_drop_files(&raw_block(20, 1, &utf16("a\0b\0"))),
        strings(&["a", "b"]),
        "no terminator, wide"
    );
    assert_eq!(
        parse_drop_files(&raw_block(20, 0, b"a\0b\0")),
        strings(&["a", "b"]),
        "no terminator, ansi"
    );
    // An odd byte count for UTF-16: the stray byte is ignored.
    let mut odd = utf16("a\0");
    odd.push(0x62);
    assert_eq!(
        parse_drop_files(&raw_block(20, 1, &odd)),
        strings(&["a"]),
        "odd byte count"
    );
    // An empty item ends the list even when more data follows.
    assert_eq!(
        parse_drop_files(&raw_block(20, 1, &utf16("a\0\0b\0\0"))),
        strings(&["a"]),
        "double NUL ends the list"
    );
    assert_eq!(
        parse_drop_files(&raw_block(20, 0, b"a\0\0b\0\0")),
        strings(&["a"]),
        "double NUL ends the list, ansi"
    );
    // Duplicates are kept, in order.
    assert_eq!(
        parse_drop_files(&drop_files_bytes(&["x", "y", "x"], true)),
        strings(&["x", "y", "x"]),
        "duplicates"
    );
    // An unpaired surrogate becomes U+FFFD (documented difference T8).
    let lone = [0x3Du8, 0xD8, 0x00, 0x00, 0x00, 0x00];
    assert_eq!(
        parse_drop_files(&raw_block(20, 1, &lone)),
        strings(&["\u{FFFD}"]),
        "unpaired surrogate"
    );
}

#[test]
fn parse_drop_files_never_reads_out_of_bounds() {
    // new: every truncation and every hostile pFiles of valid blocks must come back without a panic (a read past the slice would panic or fault)
    for wide in [true, false] {
        let block = drop_files_bytes(&[r"C:\one.jpg", r"D:\two.jpg"], wide);
        for len in 0..=block.len() {
            let _ = parse_drop_files(&block[..len]);
        }
        for offset in [0u32, 1, 3, 16, 19, 20, 21, 25, 40, 41, 1000, u32::MAX - 1, u32::MAX] {
            let mut hostile = block.clone();
            hostile[..4].copy_from_slice(&offset.to_le_bytes());
            let parsed = parse_drop_files(&hostile);
            let limit = block
                .len()
                .saturating_sub(usize::try_from(offset).unwrap_or(usize::MAX));
            assert!(
                parsed.iter().map(|s| s.chars().count()).sum::<usize>() <= limit,
                "offset {offset}"
            );
        }
    }
    // an item that runs past the end must not be returned
    assert!(parse_drop_files(&raw_block(20, 1, &utf16("abcdef"))).is_empty());
}

#[test]
fn files_releases_the_medium_exactly_once_and_unlocks_on_every_path() {
    // new: T2. The sentinel stands for `pUnkForRelease`: it is dropped (released) exactly once per `GetData` result, and the lock count is back to 0.
    let ok = fake_with(Some(&[r"C:\a.jpg", r"C:\b.jpg"]), true, 0, Mode::Sentinel);
    assert_eq!(files(&ok.data), strings(&[r"C:\a.jpg", r"C:\b.jpg"]));
    assert_eq!(ok.counters.sentinels_dropped.load(Ordering::SeqCst), 1);
    assert_eq!(files(&ok.data).len(), 2);
    assert_eq!(ok.counters.sentinels_dropped.load(Ordering::SeqCst), 2);
    assert_eq!(ok.counters.get_data_calls.load(Ordering::SeqCst), 2);
    for handle in ok.handles.lock().unwrap().iter() {
        // SAFETY: the fake still owns the handles (Sentinel mode), so they are valid.
        let flags = unsafe { GlobalFlags(HGLOBAL(*handle as *mut c_void)) };
        assert_eq!(flags & GMEM_LOCKCOUNT, 0, "balanced lock");
    }

    // An empty list stops early: still released and unlocked.
    let empty = fake_with(Some(&[]), true, 0, Mode::Sentinel);
    assert!(files(&empty.data).is_empty());
    assert_eq!(empty.counters.sentinels_dropped.load(Ordering::SeqCst), 1);
    for handle in empty.handles.lock().unwrap().iter() {
        // SAFETY: as above.
        let flags = unsafe { GlobalFlags(HGLOBAL(*handle as *mut c_void)) };
        assert_eq!(flags & GMEM_LOCKCOUNT, 0);
    }

    // Not an HGLOBAL, and a null handle: empty, medium released once.
    let wrong = fake_with(Some(&[r"C:\a.jpg"]), true, 0, Mode::WrongTymed);
    assert!(files(&wrong.data).is_empty());
    assert_eq!(wrong.counters.sentinels_dropped.load(Ordering::SeqCst), 1);
    let null = fake_with(Some(&[r"C:\a.jpg"]), true, 0, Mode::NullHandle);
    assert!(files(&null.data).is_empty());
    assert_eq!(null.counters.sentinels_dropped.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------- IDropTarget (C# tests through the interface)

#[test]
fn drag_enter_and_over_show_copy_when_files_are_carried_none_otherwise() {
    // C#: DropTargetTests.DragEnterAndOver_ShowCopyWhenFilesAreCarried_NoneOtherwise
    let target = create(&Rc::new(Sink::default()));
    let com = target.interface();
    let mut effect = DROPEFFECT_MOVE;

    enter(com, &fake(Some(&[r"C:\a.jpg"])).data, &mut effect).unwrap();
    assert_eq!(effect, DROPEFFECT_COPY);
    effect = DROPEFFECT_NONE;
    over(com, &mut effect).unwrap();
    assert_eq!(effect, DROPEFFECT_COPY);

    leave(com).unwrap();
    effect = DROPEFFECT_COPY;
    over(com, &mut effect).unwrap(); // after DragLeave nothing is being dragged
    assert_eq!(effect, DROPEFFECT_NONE);

    enter(com, &fake(None).data, &mut effect).unwrap();
    assert_eq!(effect, DROPEFFECT_NONE);
    effect = DROPEFFECT_COPY;
    over(com, &mut effect).unwrap();
    assert_eq!(effect, DROPEFFECT_NONE);
}

#[test]
fn drop_clears_the_drag_state_so_a_later_over_shows_none() {
    // new: C# Drop sets _dragHasFiles = false
    let target = create(&Rc::new(Sink::default()));
    let com = target.interface();
    let data = fake(Some(&[r"C:\a.jpg"]));
    let mut effect = DROPEFFECT_NONE;
    enter(com, &data.data, &mut effect).unwrap();
    drop_on(com, &data.data, &mut effect).unwrap();
    effect = DROPEFFECT_COPY;
    over(com, &mut effect).unwrap();
    assert_eq!(effect, DROPEFFECT_NONE);
}

#[test]
fn drop_opens_only_the_first_file_with_copy_effect() {
    // C#: DropTargetTests.Drop_OpensOnlyTheFirstFile_WithCopyEffect
    let sink = Rc::new(Sink::default());
    let target = create(&sink);
    let mut effect = DROPEFFECT_NONE;

    drop_on(
        target.interface(),
        &fake(Some(&[r"C:\first.jpg", r"C:\second.jpg"])).data,
        &mut effect,
    )
    .expect("S_OK");

    assert_eq!(effect, DROPEFFECT_COPY);
    assert_eq!(*sink.opened.borrow(), strings(&[r"C:\first.jpg"]));
    assert!(sink.failures.borrow().is_empty());
}

#[test]
fn drop_without_files_opens_nothing_and_shows_none() {
    // C#: DropTargetTests.Drop_WithoutFiles_OpensNothing_AndShowsNone
    let sink = Rc::new(Sink::default());
    let target = create(&sink);
    let mut effect = DROPEFFECT_COPY;

    drop_on(target.interface(), &fake(None).data, &mut effect).expect("S_OK");

    assert_eq!(effect, DROPEFFECT_NONE);
    assert!(sink.opened.borrow().is_empty());
    assert!(sink.failures.borrow().is_empty());
}

#[test]
fn drop_when_opening_fails_reports_the_failure_not_a_crash() {
    // C#: DropTargetTests.Drop_WhenOpeningFails_ReportsTheFailure_NotACrash (T5: the answer is still S_OK)
    let sink = Rc::new(Sink::default());
    let behaviour: OpenBehaviour = Box::new(|_| Err("gone".into()));
    let target = ShellDropTarget::new(callbacks(&sink, Some(behaviour), None, None));
    let mut effect = DROPEFFECT_NONE;

    drop_on(target.interface(), &fake(Some(&[r"C:\x.jpg"])).data, &mut effect).expect("S_OK even when the open fails");

    assert_eq!(*sink.failures.borrow(), strings(&["gone"]));
    assert_eq!(*sink.opened.borrow(), strings(&[r"C:\x.jpg"]));
}

#[test]
fn drop_when_reading_the_data_object_fails_is_reported() {
    // C#: DropTargetTests.Drop_WhenReadingTheDataObjectThrows_IsReported
    let sink = Rc::new(Sink::default());
    let reader: FilesReader = Box::new(|_| Err("rpc".into()));
    let target = ShellDropTarget::new(callbacks(&sink, None, Some(reader), None));
    let mut effect = DROPEFFECT_COPY;

    drop_on(target.interface(), &fake(Some(&[r"C:\x.jpg"])).data, &mut effect).expect("S_OK");

    assert_eq!(*sink.failures.borrow(), strings(&["rpc"]));
    assert_eq!(effect, DROPEFFECT_NONE);
    assert!(sink.opened.borrow().is_empty());
}

#[test]
fn a_failing_has_files_reader_counts_as_no_files() {
    // new: C# `SafeHasFiles` catch
    let sink = Rc::new(Sink::default());
    let reader: HasFilesReader = Box::new(|_| Err("rpc".into()));
    let target = ShellDropTarget::new(callbacks(&sink, None, None, Some(reader)));
    let mut effect = DROPEFFECT_MOVE;
    enter(target.interface(), &fake(Some(&[r"C:\x.jpg"])).data, &mut effect).unwrap();
    assert_eq!(effect, DROPEFFECT_NONE);
    assert!(sink.failures.borrow().is_empty());
}

#[test]
fn the_vtable_marshalling_carries_point_by_value_and_the_effect_by_ref() {
    // C#: DropTargetTests.TheVtableMarshalling_CarriesPointByValue_AndTheEffectByRef: enter then drop through the generated vtable of `interface()`
    let sink = Rc::new(Sink::default());
    let target = create(&sink);
    let com = target.interface();
    let data = fake(Some(&[r"C:\drag\a.jpg"]));
    let mut effect = DROPEFFECT_NONE;
    enter(com, &data.data, &mut effect).unwrap();
    assert_eq!(effect, DROPEFFECT_COPY);
    effect = DROPEFFECT_NONE;
    drop_on(com, &data.data, &mut effect).unwrap();
    assert_eq!(effect, DROPEFFECT_COPY);
    assert_eq!(*sink.opened.borrow(), strings(&[r"C:\drag\a.jpg"]));
}

#[test]
fn the_source_allowed_effects_and_the_key_state_are_ignored() {
    // new: open question 1 default (C# parity): a source that allows only Move still gets Copy
    let sink = Rc::new(Sink::default());
    let target = create(&sink);
    let data = fake(Some(&[r"C:\a.jpg"]));
    let mut effect = DROPEFFECT_MOVE;
    // SAFETY: live COM objects and a live local effect; the key state has Ctrl+Shift bits set.
    unsafe {
        target
            .interface()
            .DragEnter(&data.data, MODIFIERKEYS_FLAGS(0x0C), POINT, &mut effect)
    }
    .unwrap();
    assert_eq!(effect, DROPEFFECT_COPY);
}

#[test]
fn effects_are_written_through_the_pointer_for_enter_over_drop() {
    // new: T4. The same local is rewritten by each call.
    let target = create(&Rc::new(Sink::default()));
    let com = target.interface();
    let data = fake(Some(&[r"C:\a.jpg"]));
    let mut effect = DROPEFFECT(0xFFFF_FFFF);
    enter(com, &data.data, &mut effect).unwrap();
    assert_eq!(effect, DROPEFFECT_COPY);
    effect = DROPEFFECT(0xFFFF_FFFF);
    over(com, &mut effect).unwrap();
    assert_eq!(effect, DROPEFFECT_COPY);
    effect = DROPEFFECT(0xFFFF_FFFF);
    drop_on(com, &data.data, &mut effect).unwrap();
    assert_eq!(effect, DROPEFFECT_COPY);
    effect = DROPEFFECT(0xFFFF_FFFF);
    drop_on(com, &fake(None).data, &mut effect).unwrap();
    assert_eq!(effect, DROPEFFECT_NONE);
}

#[test]
fn null_effect_pointer_is_ignored() {
    // new: T4
    let sink = Rc::new(Sink::default());
    let target = create(&sink);
    let com = target.interface();
    let data = fake(Some(&[r"C:\a.jpg"]));
    // SAFETY: live COM objects; the null effect pointer is exactly what the call under test must tolerate.
    unsafe {
        com.DragEnter(&data.data, MODIFIERKEYS_FLAGS(0), POINT, std::ptr::null_mut())
            .expect("enter");
        com.DragOver(MODIFIERKEYS_FLAGS(0), POINT, std::ptr::null_mut())
            .expect("over");
        com.Drop(&data.data, MODIFIERKEYS_FLAGS(0), POINT, std::ptr::null_mut())
            .expect("drop");
    }
    assert_eq!(*sink.opened.borrow(), strings(&[r"C:\a.jpg"]), "the drop still ran");
}

#[test]
fn a_panic_in_a_callback_answers_e_fail_and_is_resumed_by_the_owner() {
    // new: D-W8B4I-9. The panic must not unwind through the generated vtable function.
    let sink = Rc::new(Sink::default());
    let behaviour: OpenBehaviour = Box::new(|_| panic!("callback boom"));
    let target = ShellDropTarget::new(callbacks(&sink, Some(behaviour), None, None));
    let mut effect = DROPEFFECT_NONE;
    let answer = drop_on(target.interface(), &fake(Some(&[r"C:\a.jpg"])).data, &mut effect);
    assert_eq!(answer.expect_err("E_FAIL").code(), E_FAIL);
    assert!(pending_panic::has_pending(), "captured for the message loop");
    let resumed = catch_unwind(AssertUnwindSafe(pending_panic::rethrow_pending));
    let payload = resumed.expect_err("the owner resumes the panic");
    assert_eq!(payload.downcast_ref::<&str>().copied(), Some("callback boom"));
    assert!(!pending_panic::has_pending());
    // the target still works afterwards
    leave(target.interface()).unwrap();
}

#[test]
fn dropping_an_unregistered_target_releases_the_callbacks() {
    // new: T1 (pure part). The callbacks live exactly as long as the COM object.
    let alive = Rc::new(());
    let sentinel = Rc::clone(&alive);
    let sink = Rc::new(Sink::default());
    let mut cb = callbacks(&sink, None, None, None);
    cb.on_failure = Box::new(move |_| {
        let _ = &sentinel;
    });
    let target = ShellDropTarget::new(cb);
    assert_eq!(Rc::strong_count(&alive), 2);
    assert!(!target.is_registered());
    drop(target);
    assert_eq!(Rc::strong_count(&alive), 1);
}

#[test]
fn register_refuses_a_null_window_and_dispose_without_register_is_a_no_op() {
    // new: no argument check as in C#: the OS refuses RegisterDragDrop(0); dispose is idempotent
    let target = create(&Rc::new(Sink::default()));
    match target.register(0) {
        Err(DropRegisterError::Refused(hresult)) => assert!(!hresult.succeeded()),
        other => panic!("expected Refused, got {other:?}"),
    }
    assert!(!target.is_registered());
    target.dispose();
    target.dispose();
}

// ---------------------------------------------------------------- native: registration (hidden windows, own STA thread)

mod native {
    use super::*;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP};
    use windows::core::w;

    /// `DRAGDROP_E_ALREADYREGISTERED`.
    const ALREADY_REGISTERED: i32 = 0x8004_0101_u32 as i32;

    /// Runs `body` on its own thread (an STA after `register`'s `OleInitialize`), bounded by [`BOUND`].
    fn run_sta(body: impl FnOnce() + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("drop target native test".to_owned())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(body));
                let _ = tx.send(result.is_ok());
                if let Err(payload) = result {
                    std::panic::resume_unwind(payload);
                }
            })
            .expect("spawn");
        let ok = rx.recv_timeout(BOUND).expect("the native test hung");
        let joined = thread.join();
        assert!(ok && joined.is_ok(), "the native test failed");
    }

    /// A hidden window: a `STATIC` popup that is never shown or activated.
    fn hidden_window() -> HWND {
        // SAFETY: a predefined window class, no parent, no menu; the window is destroyed by the test that made it.
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
        }
        .expect("hidden window")
    }

    fn destroy(hwnd: HWND) {
        // SAFETY: the window was created by this thread.
        let _ = unsafe { DestroyWindow(hwnd) };
    }

    fn handle(hwnd: HWND) -> isize {
        hwnd.0 as isize
    }

    #[test]
    #[ignore = "native: needs desktop"]
    fn register_succeeds_and_a_second_register_on_the_same_hwnd_is_refused_by_ole() {
        run_sta(|| {
            let hwnd = hidden_window();
            let first = create(&Rc::new(Sink::default()));
            first.register(handle(hwnd)).expect("register");
            assert!(first.is_registered());
            assert_eq!(first.register(handle(hwnd)), Err(DropRegisterError::AlreadyRegistered));
            let second = create(&Rc::new(Sink::default()));
            match second.register(handle(hwnd)) {
                Err(DropRegisterError::Refused(hresult)) => assert_eq!(hresult.0, ALREADY_REGISTERED),
                other => panic!("expected OLE to refuse, got {other:?}"),
            }
            assert!(!second.is_registered());
            first.dispose();
            destroy(hwnd);
        });
    }

    #[test]
    #[ignore = "native: needs desktop"]
    fn dispose_allows_registering_again() {
        run_sta(|| {
            let hwnd = hidden_window();
            let target = create(&Rc::new(Sink::default()));
            target.register(handle(hwnd)).expect("register");
            target.dispose();
            assert!(!target.is_registered());
            target.dispose();
            target.register(handle(hwnd)).expect("register again after dispose");
            target.dispose();
            destroy(hwnd);
        });
    }

    #[test]
    #[ignore = "native: needs desktop"]
    fn dropping_the_handle_revokes_the_registration() {
        run_sta(|| {
            let hwnd = hidden_window();
            let target = create(&Rc::new(Sink::default()));
            target.register(handle(hwnd)).expect("register");
            drop(target);
            let fresh = create(&Rc::new(Sink::default()));
            fresh.register(handle(hwnd)).expect("the window is free again");
            fresh.dispose();
            destroy(hwnd);
        });
    }

    #[test]
    #[ignore = "native: needs desktop"]
    fn dispose_revokes_and_releases_the_callbacks() {
        // new: T1. OLE holds a reference until RevokeDragDrop; after `dispose` and dropping the handle the callbacks' state is gone.
        run_sta(|| {
            let hwnd = hidden_window();
            let alive = Rc::new(());
            let sentinel = Rc::clone(&alive);
            let sink = Rc::new(Sink::default());
            let mut cb = callbacks(&sink, None, None, None);
            cb.on_failure = Box::new(move |_| {
                let _ = &sentinel;
            });
            let target = ShellDropTarget::new(cb);
            target.register(handle(hwnd)).expect("register");
            assert_eq!(Rc::strong_count(&alive), 2);
            drop(target);
            assert_eq!(
                Rc::strong_count(&alive),
                1,
                "revoked, so OLE no longer keeps the object (and the callbacks) alive"
            );
            destroy(hwnd);
        });
    }

    #[test]
    #[ignore = "native: needs desktop"]
    fn dispose_after_the_window_is_destroyed_does_not_fail() {
        run_sta(|| {
            let hwnd = hidden_window();
            let target = create(&Rc::new(Sink::default()));
            target.register(handle(hwnd)).expect("register");
            destroy(hwnd);
            target.dispose(); // RevokeDragDrop fails (invalid window): ignored like the C# Dispose
            assert!(!target.is_registered());
        });
    }

    #[test]
    #[ignore = "native: needs desktop"]
    fn register_does_not_uninitialize_ole() {
        // new: T7. A second window on the same thread still registers after the first target was disposed (no OleUninitialize anywhere).
        run_sta(|| {
            let first = hidden_window();
            let target = create(&Rc::new(Sink::default()));
            target.register(handle(first)).expect("register");
            target.dispose();
            let second = hidden_window();
            let other = create(&Rc::new(Sink::default()));
            other
                .register(handle(second))
                .expect("OLE is still initialised on this thread");
            other.dispose();
            destroy(first);
            destroy(second);
        });
    }
}
