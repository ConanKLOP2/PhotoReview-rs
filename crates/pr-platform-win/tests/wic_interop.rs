//! Red tests of W4-11a: `pr_platform_win::wic::interop` (C# `WicInterop.cs` at d92d6aae, plus the factory and
//! HRESULT-to-exception rules it hosts). They compile against the stub and fail through `todo!()` until the port.
//!
//! Ported C# tests (`tests/PhotoReview.Imaging.Tests/Decoding/`), each with `// C#: <Class>.<Method>`:
//! - `ManagedIStreamTests` (7 methods; most run both through the safe methods and through the COM vtable, see the `_via_vtable`
//!   twins; `Clone`/`CopyTo` have no safe Rust method, so they are vtable-only);
//! - `DecodingHelpersMutationTests` write side (3 methods);
//! - `WicInteropVtableTests` (1 method);
//! - `WicGeneratedComTests` (7 methods);
//! - `WicDirectFactoryFailureTests` (5 methods, `ThrowIfFactoryFailed` is `check_factory_result` here).
//!
//! Golden-driven tests: `wic-interop-constants`, `wic-hresult-exception`, `wic-managed-istream`.
//!
//! Skips (documented, not ported): `WicGeneratedComTests.EnumerateDecoders_*` and `CollectDecoders_*` (W4-11b),
//! `ReadColorContexts_*` (W4-11c), `WicLifetimeBalanceTests` and `WicDirectDecoderPixelParityTests` (their units).
//! The `preserve_sig` flag of the `wic-interop-constants` golden is not checked: a Rust method returns
//! `windows_core::Result`, nothing to port (interop notes, item 10). `Commit`/`Stat` flag values are passed as 0/1 only.
//!
//! Tests that create real WIC or COM objects are `#[ignore = "native: needs WIC"]` or `"native: needs COM"`.

#![cfg(windows)]

use std::ffi::c_void;
use std::io::{self, Cursor, ErrorKind, SeekFrom};
use std::mem::{offset_of, size_of};
use std::ptr;
use std::sync::Arc;
use std::thread;

use pr_platform_win::wic::interop::{
    COR_E_ARGUMENTOUTOFRANGE, COR_E_IO, COR_E_NOTSUPPORTED, COR_E_OBJECTDISPOSED, Com, ComStream, DotnetExceptionKind,
    E_FAIL, E_INVALIDARG, E_NOINTERFACE, IWICBitmapFlipRotator, IWICBitmapScaler, IWICBitmapSourceTransform,
    IWICImagingFactory, IWICStream, StreamBackend, StreamFault, WICDecodeOptions, WINCODEC_ERR_NOTINITIALIZED,
    WicError, add_ref_raw, check_factory_result, create_bitmap_scaler, create_decoder_from_stream, create_factory,
    ensure_com_initialized, exception_kind_for_hresult, get_frame, guids, guids::ALL as GUID_ALL, hresult_from_win32,
    outstanding_on_this_thread, release_raw, seek_from,
};
use pr_test_support::golden;
use serde_json::Value;
use windows::Win32::Graphics::Imaging::{
    IWICBitmap, IWICBitmapDecoder_Vtbl, IWICBitmapEncoder, IWICBitmapFlipRotator_Vtbl, IWICBitmapFrameDecode_Vtbl,
    IWICBitmapFrameEncode, IWICBitmapScaler_Vtbl, IWICBitmapSource, IWICBitmapSource_Vtbl,
    IWICBitmapSourceTransform_Vtbl, IWICColorContext_Vtbl, IWICColorTransform_Vtbl, IWICFormatConverter_Vtbl,
    IWICImagingFactory_Vtbl, IWICMetadataQueryReader_Vtbl, IWICStream_Vtbl, WICBitmapDitherTypeNone,
    WICBitmapEncoderNoCache, WICBitmapInterpolationModeCubic, WICBitmapInterpolationModeFant,
    WICBitmapInterpolationModeHighQualityCubic, WICBitmapInterpolationModeLinear,
    WICBitmapInterpolationModeNearestNeighbor, WICBitmapPaletteTypeCustom, WICBitmapTransformFlipHorizontal,
    WICBitmapTransformFlipVertical, WICBitmapTransformRotate0, WICBitmapTransformRotate90, WICBitmapTransformRotate180,
    WICBitmapTransformRotate270, WICColorContextExifColorSpace, WICColorContextProfile, WICColorContextUninitialized,
    WICDecodeMetadataCacheOnDemand, WICDecodeMetadataCacheOnLoad,
};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize, IErrorInfo, IStream, IStream_Vtbl, STATSTG, STREAM_SEEK,
    SetErrorInfo,
};
use windows::Win32::System::Ole::CreateErrorInfo;
use windows_core::{BSTR, GUID, HRESULT, Interface};

// ---------------------------------------------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------------------------------------------

const DATA: [u8; 8] = [10, 11, 12, 13, 14, 15, 16, 17];
const SENTINEL_U32: u32 = 0xCDCD_CDCD;
const SENTINEL_U64: u64 = 0xCDCD_CDCD_CDCD_CDCD;
const UNREAD: u8 = 0xEE;

/// The COM vtable of an `IStream` pointer (the first word of a COM object is its vtable pointer).
///
/// # Safety
/// `raw` is a live `IStream` interface pointer.
unsafe fn stream_vtbl<'a>(raw: *mut c_void) -> &'a IStream_Vtbl {
    // SAFETY: COM ABI: the object's first word points to its vtable, which lives as long as the object.
    unsafe { &**(raw as *const *const IStream_Vtbl) }
}

/// `ISequentialStream::Read` through the vtable. `pv` must be valid for `cb` bytes, or `cb` out of range
/// (the port rejects it before touching `pv`); `pcb` is null or a live `u32`.
fn vt_read(raw: *mut c_void, pv: *mut c_void, cb: u32, pcb: *mut u32) -> HRESULT {
    // SAFETY: the caller keeps the IStream alive (it owns the `IStream` the raw pointer comes from); `pv`/`pcb`
    // are documented above.
    unsafe { (stream_vtbl(raw).base__.Read)(raw, pv, cb, pcb) }
}

/// `ISequentialStream::Write` through the vtable (same pointer rules as [`vt_read`]).
fn vt_write(raw: *mut c_void, pv: *const c_void, cb: u32, pcb: *mut u32) -> HRESULT {
    // SAFETY: as in `vt_read`.
    unsafe { (stream_vtbl(raw).base__.Write)(raw, pv, cb, pcb) }
}

/// `IStream::Seek` through the vtable; `pnew` is null or a live `u64`.
fn vt_seek(raw: *mut c_void, offset: i64, origin: u32, pnew: *mut u64) -> HRESULT {
    // SAFETY: as in `vt_read`; `STREAM_SEEK` is a transparent `u32`, the raw origin is passed unchanged.
    unsafe { (stream_vtbl(raw).Seek)(raw, offset, STREAM_SEEK(origin), pnew) }
}

fn vt_set_size(raw: *mut c_void, size: u64) -> HRESULT {
    // SAFETY: as in `vt_read`.
    unsafe { (stream_vtbl(raw).SetSize)(raw, size) }
}

fn vt_copy_to(raw: *mut c_void, pstm: *mut c_void, cb: u64, pread: *mut u64, pwritten: *mut u64) -> HRESULT {
    // SAFETY: as in `vt_read`; the port returns NotSupported before it reads `pstm`, the tests pass null.
    unsafe { (stream_vtbl(raw).CopyTo)(raw, pstm, cb, pread, pwritten) }
}

fn vt_commit(raw: *mut c_void, flags: u32) -> HRESULT {
    // SAFETY: as in `vt_read`.
    unsafe { (stream_vtbl(raw).Commit)(raw, flags) }
}

fn vt_revert(raw: *mut c_void) -> HRESULT {
    // SAFETY: as in `vt_read`.
    unsafe { (stream_vtbl(raw).Revert)(raw) }
}

fn vt_lock(raw: *mut c_void, offset: u64, cb: u64, kind: u32) -> HRESULT {
    // SAFETY: as in `vt_read`.
    unsafe { (stream_vtbl(raw).LockRegion)(raw, offset, cb, kind) }
}

fn vt_unlock(raw: *mut c_void, offset: u64, cb: u64, kind: u32) -> HRESULT {
    // SAFETY: as in `vt_read`.
    unsafe { (stream_vtbl(raw).UnlockRegion)(raw, offset, cb, kind) }
}

/// `IStream::Stat` through the vtable; `p` is null or points to a writable 80-byte `STATSTG` (8-byte aligned).
fn vt_stat(raw: *mut c_void, p: *mut STATSTG, flag: u32) -> HRESULT {
    // SAFETY: as in `vt_read`; `STATFLAG` is a transparent `i32`, the raw flag bits are passed unchanged.
    unsafe { (stream_vtbl(raw).Stat)(raw, p, flag) }
}

/// `IStream::Clone` through the vtable; `out` points to a writable pointer slot.
fn vt_clone(raw: *mut c_void, out: *mut *mut c_void) -> HRESULT {
    // SAFETY: as in `vt_read`.
    unsafe { (stream_vtbl(raw).Clone)(raw, out) }
}

/// Takes the bytes of a growable memory stream back out (disposes it).
fn take_bytes(com: &ComStream) -> Vec<u8> {
    let backend = com.dispose().expect("the stream was not disposed yet");
    let any = backend.into_any();
    let cursor = any
        .downcast::<Cursor<Vec<u8>>>()
        .unwrap_or_else(|_| panic!("backend is not a growable memory stream"));
    cursor.into_inner()
}

/// Formats a GUID the way the goldens write it: lower-case, 8-4-4-4-12.
fn guid_text(g: &GUID) -> String {
    let d = g.data4;
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        g.data1, g.data2, g.data3, d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
    )
}

/// Parses a golden HRESULT text (`"0x80131620"`) into the signed value the port returns.
fn hex_i32(text: &str) -> i32 {
    let digits = text.trim_start_matches("0x");
    u32::from_str_radix(digits, 16).unwrap_or_else(|e| panic!("bad HRESULT {text}: {e}")) as i32
}

/// Parses a golden `u64` that JSON may store as a number or as a string (values above `i64::MAX`).
fn u64_of(value: &Value) -> u64 {
    match value {
        Value::Number(n) => n.as_u64().unwrap_or_else(|| panic!("not a u64: {value}")),
        Value::String(s) => s.parse().unwrap_or_else(|e| panic!("bad u64 {s}: {e}")),
        other => panic!("not a u64: {other}"),
    }
}

fn bytes_of(value: &Value) -> Vec<u8> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("not an array: {value}"))
        .iter()
        .map(|b| b.as_u64().expect("byte") as u8)
        .collect()
}

/// Sets the stale `IErrorInfo` of the calling thread (`SetErrorInfo`), clears it again on drop, as the C# test does.
struct StaleErrorInfo;

impl StaleErrorInfo {
    fn set(description: &str) -> windows_core::Result<Self> {
        // SAFETY: CreateErrorInfo/SetErrorInfo take no pointers we own; the BSTR and the interface are kept alive
        // until SetErrorInfo has taken its own reference.
        unsafe {
            let create = CreateErrorInfo()?;
            create.SetDescription(&BSTR::from(description))?;
            let info: IErrorInfo = create.cast()?;
            SetErrorInfo(0, &info)?;
        }
        Ok(StaleErrorInfo)
    }
}

impl Drop for StaleErrorInfo {
    fn drop(&mut self) {
        // SAFETY: SetErrorInfo(0, NULL) clears the calling thread's error object; no pointer is dereferenced.
        unsafe {
            let _ = SetErrorInfo(0, None::<&IErrorInfo>);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// ManagedIStreamTests (C#: ManagedIStreamTests.*): safe methods, then the same through the COM vtable
// ---------------------------------------------------------------------------------------------------------------

// C#: ManagedIStreamTests.Read_ReportsBytesReadThroughPointer_IncludingShortReadAtEof
#[test]
fn read_reports_bytes_read_through_pointer_including_short_read_at_eof() {
    let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
    com.seek(6, 0).expect("seek to 6");
    let mut buffer = [0u8; 4];
    assert_eq!(com.read(&mut buffer).expect("read"), 2);
    assert_eq!(buffer, [16, 17, 0, 0]);
    assert_eq!(com.read(&mut buffer).expect("read at end"), 0);
}

// C#: ManagedIStreamTests.Read_ReportsBytesReadThroughPointer_IncludingShortReadAtEof (COM vtable twin)
#[test]
fn read_reports_bytes_read_through_pointer_including_short_read_at_eof_via_vtable() {
    let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
    com.seek(6, 0).expect("seek to 6");
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    let mut buffer = [0u8; 4];
    let mut pcb: u32 = SENTINEL_U32;
    assert_eq!(vt_read(raw, buffer.as_mut_ptr().cast(), 4, &mut pcb).0, 0);
    assert_eq!(pcb, 2);
    assert_eq!(buffer, [16, 17, 0, 0]);
    assert_eq!(vt_read(raw, buffer.as_mut_ptr().cast(), 4, &mut pcb).0, 0);
    assert_eq!(pcb, 0);
}

// C#: ManagedIStreamTests.ReadAndSeek_NullOutPointer_StillAdvanceThePosition
#[test]
fn read_and_seek_null_out_pointer_still_advance_the_position() {
    let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
    let mut buffer = [0u8; 3];
    assert_eq!(com.read(&mut buffer).expect("read"), 3);
    assert_eq!(buffer, [10, 11, 12]);
    assert_eq!(com.seek(0, 1).expect("position"), 3);
    com.seek(1, 0).expect("seek");
    assert_eq!(com.seek(0, 1).expect("position"), 1);
}

// C#: ManagedIStreamTests.ReadAndSeek_NullOutPointer_StillAdvanceThePosition (COM vtable twin)
#[test]
fn read_and_seek_null_out_pointer_still_advance_the_position_via_vtable() {
    let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    let mut buffer = [0u8; 3];
    assert_eq!(vt_read(raw, buffer.as_mut_ptr().cast(), 3, ptr::null_mut()).0, 0);
    assert_eq!(com.seek(0, 1).expect("position"), 3);
    assert_eq!(buffer, [10, 11, 12]);
    assert_eq!(vt_seek(raw, 1, 0, ptr::null_mut()).0, 0);
    assert_eq!(com.seek(0, 1).expect("position"), 1);
}

// C#: ManagedIStreamTests.Seek_ReportsNewPosition (5 InlineData rows; one table test for both paths)
#[test]
fn seek_reports_new_position() {
    // (origin, move, start position, expected absolute position)
    let rows: [(u32, i64, u64, u64); 5] = [(0, 5, 4, 5), (1, 2, 4, 6), (1, -3, 4, 1), (2, -2, 4, 6), (2, 0, 4, 8)];
    for (origin, move_by, start, expected) in rows {
        let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
        com.seek(start as i64, 0).expect("start position");
        let got = com.seek(move_by, origin);
        assert_eq!(
            got.ok(),
            Some(expected),
            "safe seek origin={origin} move={move_by} start={start}"
        );
        assert_eq!(com.seek(0, 1).expect("position"), expected);

        let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
        com.seek(start as i64, 0).expect("start position");
        let stream: IStream = com.istream();
        let raw = stream.as_raw();
        let mut pnew: u64 = SENTINEL_U64;
        assert_eq!(
            vt_seek(raw, move_by, origin, &mut pnew).0,
            0,
            "vtable seek origin={origin}"
        );
        assert_eq!(
            pnew, expected,
            "vtable seek origin={origin} move={move_by} start={start}"
        );
        assert_eq!(com.seek(0, 1).expect("position"), expected);
    }
}

// C#: ManagedIStreamTests.Seek_BeforeStart_Throws
#[test]
fn seek_before_start_throws() {
    let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
    com.seek(2, 0).expect("start position");
    let fault = com.seek(-3, 1).expect_err("seek before the start must fail");
    assert_eq!(fault.hresult(), COR_E_IO);
    assert_eq!(com.seek(0, 1).expect("position"), 2);

    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    let mut pnew: u64 = SENTINEL_U64;
    assert_eq!(vt_seek(raw, -3, 1, &mut pnew).0, COR_E_IO);
    assert_eq!(pnew, SENTINEL_U64, "*plibNewPosition is written only on success");
    assert_eq!(com.seek(0, 1).expect("position"), 2);
}

// C#: ManagedIStreamTests.Stat_ReportsLength
#[test]
fn stat_reports_length() {
    let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
    let stat = com.stat().expect("stat");
    assert_eq!(stat.cbSize, DATA.len() as u64);
    assert_eq!(stat.r#type, 2, "STGTY_STREAM");
}

// C#: ManagedIStreamTests.Stat_ReportsLength (COM vtable twin)
#[test]
fn stat_reports_length_via_vtable() {
    let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    let mut stat = [0u64; 10];
    assert_eq!(vt_stat(raw, stat.as_mut_ptr().cast(), 0).0, 0);
    // SAFETY: `stat` is 80 bytes of initialised memory with 8-byte alignment (a `[u64; 10]`), so it holds a STATSTG.
    let stat: STATSTG = unsafe { std::ptr::read(stat.as_ptr().cast::<STATSTG>()) };
    assert_eq!(stat.cbSize, DATA.len() as u64);
    assert_eq!(stat.r#type, 2);
}

// C#: ManagedIStreamTests.AfterDispose_OperationsThrowObjectDisposed
#[test]
fn after_dispose_operations_throw_object_disposed() {
    let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
    assert!(com.dispose().is_some());
    assert!(matches!(com.read(&mut [0u8; 1]), Err(StreamFault::Disposed)));
    assert!(matches!(com.seek(0, 0), Err(StreamFault::Disposed)));
    assert!(matches!(com.stat(), Err(StreamFault::Disposed)));
    assert!(com.commit().is_ok(), "commit is tolerated after dispose");
}

// C#: ManagedIStreamTests.AfterDispose_OperationsThrowObjectDisposed (COM vtable twin)
#[test]
fn after_dispose_operations_throw_object_disposed_via_vtable() {
    let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    assert!(com.dispose().is_some());
    let mut byte = 0u8;
    assert_eq!(
        vt_read(raw, (&mut byte as *mut u8).cast(), 1, ptr::null_mut()).0,
        COR_E_OBJECTDISPOSED
    );
    assert_eq!(vt_seek(raw, 0, 0, ptr::null_mut()).0, COR_E_OBJECTDISPOSED);
    let mut stat = [0u64; 10];
    assert_eq!(vt_stat(raw, stat.as_mut_ptr().cast(), 0).0, COR_E_OBJECTDISPOSED);
    assert_eq!(vt_commit(raw, 0).0, 0, "Commit is S_OK after dispose");
}

// C#: ManagedIStreamTests.CloneAndCopyTo_NotSupported (vtable only: ComStream has no safe Clone or CopyTo)
#[test]
fn clone_and_copy_to_not_supported() {
    let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    let mut clone: *mut c_void = ptr::null_mut();
    assert_eq!(vt_clone(raw, &mut clone).0, COR_E_NOTSUPPORTED);
    assert_eq!(
        vt_copy_to(raw, ptr::null_mut(), 1, ptr::null_mut(), ptr::null_mut()).0,
        COR_E_NOTSUPPORTED
    );
}

// ---------------------------------------------------------------------------------------------------------------
// DecodingHelpersMutationTests: ManagedIStream write side (C#: DecodingHelpersMutationTests.*)
// ---------------------------------------------------------------------------------------------------------------

// C#: DecodingHelpersMutationTests.ManagedIStream_Write_WritesAndReportsTheCountThroughThePointer
#[test]
fn managed_istream_write_writes_and_reports_the_count_through_the_pointer() {
    let com = ComStream::writable_memory();
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    let data = [1u8, 2, 3, 4, 5];
    let mut pcb: u32 = u32::MAX; // C# pre-sets -1
    assert_eq!(vt_write(raw, data.as_ptr().cast(), 3, &mut pcb).0, 0);
    assert_eq!(pcb, 3);
    assert_eq!(vt_write(raw, [9u8].as_ptr().cast(), 1, ptr::null_mut()).0, 0);
    drop(stream);
    assert_eq!(take_bytes(&com), vec![1, 2, 3, 9]);
}

// C#: DecodingHelpersMutationTests.ManagedIStream_WriteAndSetSizeAfterDispose_ThrowObjectDisposed
#[test]
fn managed_istream_write_and_set_size_after_dispose_throw_object_disposed() {
    let com = ComStream::writable_memory();
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    assert!(com.dispose().is_some());
    assert!(matches!(com.write(&[1]), Err(StreamFault::Disposed)));
    assert!(matches!(com.set_size(4), Err(StreamFault::Disposed)));
    assert_eq!(
        vt_write(raw, [1u8].as_ptr().cast(), 1, ptr::null_mut()).0,
        COR_E_OBJECTDISPOSED
    );
    assert_eq!(vt_set_size(raw, 4).0, COR_E_OBJECTDISPOSED);
}

// C#: DecodingHelpersMutationTests.ManagedIStream_SetSizeCommitAndNoOps_BehaveLikeAStream
#[test]
fn managed_istream_set_size_commit_and_no_ops_behave_like_a_stream() {
    let com = ComStream::writable_memory();
    com.set_size(7).expect("set size");
    com.commit().expect("commit");
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    assert_eq!(vt_revert(raw).0, 0);
    assert_eq!(vt_lock(raw, 0, 1, 0).0, 0);
    assert_eq!(vt_unlock(raw, 0, 1, 0).0, 0);
    drop(stream);
    assert_eq!(take_bytes(&com), vec![0u8; 7]);
}

// ---------------------------------------------------------------------------------------------------------------
// WicInteropVtableTests (C#: WicInteropVtableTests.IWICBitmapSourceTransform_MethodOrder_MatchesWincodecHeader)
// ---------------------------------------------------------------------------------------------------------------

// C#: WicInteropVtableTests.IWICBitmapSourceTransform_MethodOrder_MatchesWincodecHeader
#[test]
fn iwic_bitmap_source_transform_method_order_matches_wincodec_header() {
    let word = size_of::<usize>();
    assert_eq!(offset_of!(IWICBitmapSourceTransform_Vtbl, CopyPixels), 3 * word);
    assert_eq!(offset_of!(IWICBitmapSourceTransform_Vtbl, GetClosestSize), 4 * word);
    assert_eq!(
        offset_of!(IWICBitmapSourceTransform_Vtbl, GetClosestPixelFormat),
        5 * word
    );
    assert_eq!(
        offset_of!(IWICBitmapSourceTransform_Vtbl, DoesSupportTransform),
        6 * word
    );
}

// ---------------------------------------------------------------------------------------------------------------
// WicGeneratedComTests (C#: WicGeneratedComTests.*)
// ---------------------------------------------------------------------------------------------------------------

// C#: WicGeneratedComTests.Release_DropsTheWrappersReferenceImmediately
#[test]
#[ignore = "native: needs WIC"]
fn release_drops_the_wrappers_reference_immediately() {
    let factory = create_factory().expect("factory");
    // SAFETY: the factory is live; CreateBitmapScaler returns one owned reference.
    let scaler = unsafe { factory.CreateBitmapScaler() }.expect("scaler");
    let raw = scaler.into_raw(); // one reference, owned by this test
    // SAFETY: `raw` is a live interface pointer we own one reference of.
    let own = unsafe { add_ref_raw(raw) }; // the test's own reference
    assert_eq!(own, 2);
    // SAFETY: `raw` is a live IWICBitmapScaler; wrap takes over the one reference we hand it.
    let wrapper = unsafe { Com::<IWICBitmapScaler>::wrap(raw, "IWICBitmapScaler") }.expect("wrap");
    // SAFETY: as above; the test still holds its own reference.
    let while_wrapped = unsafe { add_ref_raw(raw) };
    assert!(
        while_wrapped >= 3,
        "the wrapper must hold its own reference (count was {while_wrapped})"
    );
    // SAFETY: gives back the reference added just above.
    unsafe { release_raw(raw) };
    wrapper.release();
    // SAFETY: the test's own reference is the last one: release destroys the object.
    assert_eq!(unsafe { release_raw(raw) }, 0);
}

// C#: WicGeneratedComTests.Wrap_WrongInterface_ThrowsAndReleasesImmediately
#[test]
#[ignore = "native: needs WIC"]
fn wrap_wrong_interface_throws_and_releases_immediately() {
    let factory = create_factory().expect("factory");
    // SAFETY: as in release_drops_the_wrappers_reference_immediately.
    let scaler = unsafe { factory.CreateBitmapScaler() }.expect("scaler");
    let raw = scaler.into_raw();
    // SAFETY: a second reference of the live scaler, ours.
    unsafe { add_ref_raw(raw) };
    let before = outstanding_on_this_thread();

    // SAFETY: `raw` is a live scaler; the scaler is not an IWICStream, so the QueryInterface fails.
    let result = unsafe { Com::<IWICStream>::wrap(raw, "IWICStream") };
    assert!(
        matches!(
            result,
            Err(WicError::NoInterface {
                interface: "IWICStream"
            })
        ),
        "got {result:?}"
    );
    assert_eq!(outstanding_on_this_thread(), before);
    // SAFETY: the test's own reference is the one left.
    assert_eq!(unsafe { release_raw(raw) }, 0);
}

// C#: WicGeneratedComTests.Wrap_WrongInterface_IgnoresStaleErrorInfoOfTheThread
#[test]
#[ignore = "native: needs WIC"]
fn wrap_wrong_interface_ignores_stale_error_info_of_the_thread() {
    let factory = create_factory().expect("factory");
    // SAFETY: as in release_drops_the_wrappers_reference_immediately.
    let scaler = unsafe { factory.CreateBitmapScaler() }.expect("scaler");
    let raw = scaler.into_raw();
    // SAFETY: a second reference of the live scaler, ours.
    unsafe { add_ref_raw(raw) };
    let before = outstanding_on_this_thread();
    let _stale = StaleErrorInfo::set("stale error info left by an earlier CCW call").expect("stale IErrorInfo");

    // SAFETY: as in wrap_wrong_interface_throws_and_releases_immediately.
    let result = unsafe { Com::<IWICStream>::wrap(raw, "IWICStream") };
    assert!(
        matches!(
            result,
            Err(WicError::NoInterface {
                interface: "IWICStream"
            })
        ),
        "got {result:?}"
    );
    assert_eq!(outstanding_on_this_thread(), before);
    // SAFETY: the test's own reference is the one left.
    assert_eq!(unsafe { release_raw(raw) }, 0);
}

// C#: WicGeneratedComTests.Release_NullManagedAndRepeated_AreNoOps
// Rust twin: ownership makes a second release impossible (the wrapper is consumed). The counter must not move when a
// Com is released, when a None is dropped, or when a windows-crate interface that was never wrapped is dropped.
#[test]
#[ignore = "native: needs WIC"]
fn release_null_managed_and_repeated_are_no_ops() {
    let factory = create_factory().expect("factory");
    let before = outstanding_on_this_thread();
    let scaler = create_bitmap_scaler(&factory).expect("scaler");
    assert_eq!(outstanding_on_this_thread(), before + 1);
    scaler.release();
    assert_eq!(outstanding_on_this_thread(), before);
    drop(None::<Com<IWICBitmapScaler>>);
    assert_eq!(outstanding_on_this_thread(), before);
    // SAFETY: the factory is live; the returned windows-crate interface is owned and dropped at the end of the scope.
    let unwrapped: IWICBitmapScaler = unsafe { factory.CreateBitmapScaler() }.expect("scaler");
    drop(unwrapped);
    assert_eq!(outstanding_on_this_thread(), before);
}

// C#: WicGeneratedComTests.NullPointer_WrapThrows_WrapOrNullIsNull
#[test]
fn null_pointer_wrap_throws_wrap_or_none_is_none() {
    // SAFETY: a null pointer is the documented input of both functions; nothing is dereferenced.
    let wrapped = unsafe { Com::<IWICBitmapScaler>::wrap(ptr::null_mut(), "IWICBitmapScaler") };
    assert!(
        matches!(wrapped, Err(WicError::Com { hresult }) if hresult == E_FAIL),
        "got {wrapped:?}"
    );
    // SAFETY: as above.
    let optional = unsafe { Com::<IWICBitmapScaler>::wrap_or_none(ptr::null_mut(), "IWICBitmapScaler") };
    assert!(matches!(optional, Ok(None)), "got {optional:?}");
}

/// Encodes a `width x height` 24bpp BGR gradient as an in-memory JPEG (test-only: `tests/fixtures` has no JPEG).
fn encode_gradient_jpeg(factory: &IWICImagingFactory, width: u32, height: u32) -> Vec<u8> {
    let pixels: Vec<u8> = (0..width * height * 3).map(|i| (i % 251) as u8).collect();
    let target = ComStream::writable_memory();
    let stream: IStream = target.istream();
    // SAFETY: the factory is live; `pixels` outlives the bitmap built from it (it is dropped at the end of this
    // scope, after the encoder that reads it has committed); the stream stays alive until the encoder commits.
    unsafe {
        let bitmap: IWICBitmap = factory
            .CreateBitmapFromMemory(width, height, &guids::PIXEL_FORMAT_24BPP_BGR, width * 3, &pixels)
            .expect("bitmap");
        let encoder: IWICBitmapEncoder = factory
            .CreateEncoder(&guids::CONTAINER_FORMAT_JPEG, ptr::null())
            .expect("JPEG encoder");
        encoder
            .Initialize(&stream, WICBitmapEncoderNoCache)
            .expect("encoder initialize");
        let mut frame: Option<IWICBitmapFrameEncode> = None;
        encoder.CreateNewFrame(&mut frame, ptr::null_mut()).expect("new frame");
        let frame = frame.expect("frame object");
        frame.Initialize(None).expect("frame initialize");
        frame.SetSize(width, height).expect("frame size");
        let mut format = guids::PIXEL_FORMAT_24BPP_BGR;
        frame.SetPixelFormat(&mut format).expect("pixel format");
        let source: IWICBitmapSource = bitmap.cast().expect("bitmap as source");
        frame.WriteSource(&source, ptr::null()).expect("write source");
        frame.Commit().expect("frame commit");
        encoder.Commit().expect("encoder commit");
    }
    drop(stream);
    take_bytes(&target)
}

// C#: WicGeneratedComTests.Cast_IsQueryInterface
#[test]
#[ignore = "native: needs WIC"]
fn cast_is_query_interface() {
    let factory = create_factory().expect("factory");
    let jpeg = encode_gradient_jpeg(&factory, 64, 48);
    let source = ComStream::from_bytes(Arc::from(jpeg));
    let stream: IStream = source.istream();
    let decoder = create_decoder_from_stream(&factory, &stream, WICDecodeOptions(0)).expect("decoder");
    let frame = get_frame(&decoder, 0).expect("frame");

    assert!(
        frame.query::<IWICBitmapSourceTransform>().is_some(),
        "a JPEG frame supports DCT-scaled reads"
    );
    assert!(
        frame.query::<IWICStream>().is_none(),
        "E_NOINTERFACE is None, never an error"
    );
    let mut width = 0u32;
    let mut height = 0u32;
    // SAFETY: the out pointers are locals that live across the call.
    unsafe { frame.GetSize(&mut width, &mut height) }.expect("size");
    assert_eq!((width, height), (64, 48));
}

// C#: WicGeneratedComTests.StatStgNative_MatchesTheNativeLayout
#[test]
fn stat_stg_native_matches_the_native_layout() {
    assert_eq!(size_of::<STATSTG>(), 80);
    assert_eq!(offset_of!(STATSTG, r#type), 8);
    assert_eq!(offset_of!(STATSTG, cbSize), 16);
    assert_eq!(offset_of!(STATSTG, grfMode), 48);
    assert_eq!(offset_of!(STATSTG, clsid), 56);
    assert_eq!(offset_of!(STATSTG, grfStateBits), 72);
}

// ---------------------------------------------------------------------------------------------------------------
// WicDirectFactoryFailureTests (C#: ...; ThrowIfFactoryFailed is check_factory_result here)
// ---------------------------------------------------------------------------------------------------------------

// C#: WicDirectFactoryFailureTests.ThrowIfFactoryFailed_FailureHresultWithNoFactory_ThrowsComExceptionWithThatHresult
#[test]
fn check_factory_result_failure_hresult_with_no_factory_throws_com_with_that_hresult() {
    let result = check_factory_result(WINCODEC_ERR_NOTINITIALIZED, false);
    assert!(
        matches!(result, Err(WicError::Com { hresult }) if hresult == WINCODEC_ERR_NOTINITIALIZED),
        "got {result:?}"
    );
}

// C#: WicDirectFactoryFailureTests.ThrowIfFactoryFailed_FailureHresultEvenWhenAFactoryCameBack_StillThrows
#[test]
fn check_factory_result_failure_hresult_even_when_a_factory_came_back_still_throws() {
    let result = check_factory_result(E_FAIL, true);
    assert!(
        matches!(result, Err(WicError::Com { hresult }) if hresult == E_FAIL),
        "got {result:?}"
    );
}

// C#: WicDirectFactoryFailureTests.ThrowIfFactoryFailed_SuccessWithAFactory_DoesNotThrow
#[test]
fn check_factory_result_success_with_a_factory_does_not_throw() {
    assert!(check_factory_result(0, true).is_ok());
}

// C#: WicDirectFactoryFailureTests.ThrowIfFactoryFailed_SuccessHresultButNoFactory_ThrowsComExceptionInsteadOfLeavingANullFactory
#[test]
fn check_factory_result_success_hresult_but_no_factory_throws_instead_of_a_null_factory() {
    let result = check_factory_result(0, false);
    assert!(
        matches!(result, Err(WicError::Com { hresult }) if hresult == E_FAIL),
        "got {result:?}"
    );
}

// C#: WicDirectFactoryFailureTests.ThrowIfFactoryFailed_StaleErrorInfoOnThread_IsIgnored
#[test]
fn check_factory_result_stale_error_info_on_thread_is_ignored() {
    let stale = "stale error info from an earlier COM call";
    let _guard = StaleErrorInfo::set(stale).expect("stale IErrorInfo");
    let result = check_factory_result(WINCODEC_ERR_NOTINITIALIZED, false);
    match result {
        Err(error) => {
            assert!(matches!(error, WicError::Com { hresult } if hresult == WINCODEC_ERR_NOTINITIALIZED));
            assert!(
                !error.to_string().contains(stale),
                "the stale IErrorInfo text leaked into {error}"
            );
        }
        Ok(()) => panic!("a failing HRESULT must not succeed"),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// New Rust tests (no C# twin)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn com_adopt_maps_code_zero_error_to_e_fail() {
    let adopted = Com::<IWICBitmapScaler>::adopt(Err(windows_core::Error::empty()));
    assert!(
        matches!(adopted, Err(WicError::Com { hresult }) if hresult == E_FAIL),
        "got {adopted:?}"
    );
}

#[test]
fn com_adopt_optional_maps_code_zero_error_to_none() {
    let adopted = Com::<IWICBitmapScaler>::adopt_optional(Err(windows_core::Error::empty()));
    assert!(matches!(adopted, Ok(None)), "got {adopted:?}");
}

#[test]
fn com_adopt_optional_keeps_other_errors() {
    let error = windows_core::Error::from_hresult(HRESULT(0x8898_2F50_u32 as i32));
    let adopted = Com::<IWICBitmapScaler>::adopt_optional(Err(error));
    assert!(
        matches!(adopted, Err(WicError::Com { hresult }) if hresult == 0x8898_2F50_u32 as i32),
        "got {adopted:?}"
    );
}

#[test]
fn wic_error_from_windows_keeps_nonzero_code_and_maps_code_zero_to_e_fail() {
    let keeps = WicError::from_windows(&windows_core::Error::from_hresult(HRESULT(0x8898_2F0C_u32 as i32)));
    assert_eq!(
        keeps,
        WicError::Com {
            hresult: WINCODEC_ERR_NOTINITIALIZED
        }
    );
    let zero: WicError = windows_core::Error::empty().into();
    assert_eq!(zero, WicError::Com { hresult: E_FAIL });
}

#[test]
fn wic_error_display_hresult_and_exception_kind() {
    let com = WicError::Com {
        hresult: WINCODEC_ERR_NOTINITIALIZED,
    };
    assert_eq!(com.to_string(), "WIC call failed (0x88982F0C)");
    assert_eq!(com.hresult(), WINCODEC_ERR_NOTINITIALIZED);
    assert_eq!(com.exception_kind(), DotnetExceptionKind::Com);

    let no_interface = WicError::NoInterface {
        interface: "IWICStream",
    };
    assert_eq!(
        no_interface.to_string(),
        "The COM object does not implement IWICStream (E_NOINTERFACE)."
    );
    assert_eq!(no_interface.hresult(), E_NOINTERFACE);
    assert_eq!(no_interface.exception_kind(), DotnetExceptionKind::InvalidCast);
}

#[test]
fn hresult_from_win32_maps_zero_and_win32_codes() {
    assert_eq!(hresult_from_win32(0), 0);
    assert_eq!(hresult_from_win32(2), 0x8007_0002_u32 as i32);
    assert_eq!(hresult_from_win32(0x12345), 0x8007_2345_u32 as i32);
}

#[test]
fn seek_from_maps_origins_and_rejects_negative_start_and_bad_origins() {
    assert_eq!(seek_from(5, 0).ok(), Some(SeekFrom::Start(5)));
    assert_eq!(seek_from(-3, 1).ok(), Some(SeekFrom::Current(-3)));
    assert_eq!(seek_from(-2, 2).ok(), Some(SeekFrom::End(-2)));
    assert!(matches!(seek_from(-1, 0), Err(StreamFault::SeekBeforeBegin)));
    assert!(matches!(seek_from(0, 3), Err(StreamFault::InvalidSeekOrigin(3))));
    assert!(matches!(
        seek_from(0, u32::MAX),
        Err(StreamFault::InvalidSeekOrigin(u32::MAX))
    ));
}

#[test]
fn stream_fault_io_maps_os_code_then_invalid_input_then_other_kinds() {
    assert_eq!(
        StreamFault::Io(io::Error::from_raw_os_error(32)).hresult(),
        0x8007_0020_u32 as i32
    );
    assert_eq!(
        StreamFault::Io(io::Error::new(ErrorKind::InvalidInput, "bad")).hresult(),
        E_INVALIDARG
    );
    assert_eq!(StreamFault::Io(io::Error::other("io")).hresult(), COR_E_IO);
}

/// A backend that refuses every operation with `ErrorKind::Unsupported`, like a read-only stream.
struct RefusesEverything;

impl StreamBackend for RefusesEverything {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::from(ErrorKind::Unsupported))
    }
    fn write_all(&mut self, _data: &[u8]) -> io::Result<()> {
        Err(io::Error::from(ErrorKind::Unsupported))
    }
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        Err(io::Error::from(ErrorKind::Unsupported))
    }
    fn set_len(&mut self, _len: u64) -> io::Result<()> {
        Err(io::Error::from(ErrorKind::Unsupported))
    }
    fn len(&mut self) -> io::Result<u64> {
        Err(io::Error::from(ErrorKind::Unsupported))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::from(ErrorKind::Unsupported))
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any + Send> {
        self
    }
}

#[test]
fn backend_returning_unsupported_maps_to_cor_e_notsupported() {
    let com = ComStream::new(Box::new(RefusesEverything));
    let fault = com.read(&mut [0u8; 1]).expect_err("the backend refuses reads");
    assert_eq!(fault.hresult(), COR_E_NOTSUPPORTED);
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    let mut byte = 0u8;
    assert_eq!(
        vt_read(raw, (&mut byte as *mut u8).cast(), 1, ptr::null_mut()).0,
        COR_E_NOTSUPPORTED
    );
    assert_eq!(
        vt_write(raw, [1u8].as_ptr().cast(), 1, ptr::null_mut()).0,
        COR_E_NOTSUPPORTED
    );
}

#[test]
fn read_and_write_with_count_above_i32_max_leave_the_buffer_untouched() {
    let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    let mut buffer = [0x5Au8; 4];
    let mut pcb: u32 = SENTINEL_U32;
    assert_eq!(
        vt_read(raw, buffer.as_mut_ptr().cast(), 0x8000_0000, &mut pcb).0,
        COR_E_ARGUMENTOUTOFRANGE
    );
    assert_eq!(pcb, SENTINEL_U32);
    assert_eq!(buffer, [0x5A; 4]);
    let writable = ComStream::writable_memory();
    let writable_stream: IStream = writable.istream();
    assert_eq!(
        vt_write(writable_stream.as_raw(), buffer.as_ptr().cast(), 0x8000_0000, &mut pcb).0,
        COR_E_ARGUMENTOUTOFRANGE
    );
    assert_eq!(pcb, SENTINEL_U32);
}

#[test]
fn com_stream_dropped_while_wic_holds_its_istream_fails_with_object_disposed() {
    let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    drop(com);
    let mut byte = 0u8;
    assert_eq!(
        vt_read(raw, (&mut byte as *mut u8).cast(), 1, ptr::null_mut()).0,
        COR_E_OBJECTDISPOSED
    );
    assert_eq!(
        vt_commit(raw, 0).0,
        0,
        "Commit is S_OK after the owner dropped the stream"
    );
}

#[test]
fn take_last_fault_returns_the_fault_of_the_last_failed_com_call_once() {
    let com = ComStream::from_bytes(Arc::from(DATA.as_slice()));
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    assert_eq!(vt_seek(raw, -1, 0, ptr::null_mut()).0, COR_E_IO);
    assert!(matches!(com.take_last_fault(), Some(StreamFault::SeekBeforeBegin)));
    assert!(com.take_last_fault().is_none(), "the fault is taken once");
}

#[test]
#[ignore = "native: needs COM"]
fn ensure_com_initialized_twice_on_one_thread_is_ok() {
    thread::spawn(|| {
        assert!(ensure_com_initialized().is_ok());
        assert!(ensure_com_initialized().is_ok());
    })
    .join()
    .expect("thread");
}

#[test]
#[ignore = "native: needs COM"]
fn ensure_com_initialized_accepts_a_thread_already_in_an_apartment() {
    thread::spawn(|| {
        // SAFETY: first COM call on this fresh thread; paired with CoUninitialize below.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        assert!(hr.is_ok(), "CoInitializeEx(STA) failed: {hr:?}");
        // RPC_E_CHANGED_MODE (the thread is already STA) is tolerated: Ok, nothing to undo.
        assert!(ensure_com_initialized().is_ok());
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { CoUninitialize() };
    })
    .join()
    .expect("thread");
}

#[test]
#[ignore = "native: needs WIC"]
fn create_factory_succeeds_on_a_fresh_bare_thread_and_counts_its_reference() {
    thread::spawn(|| {
        let before = outstanding_on_this_thread();
        let factory = create_factory().expect("factory on a bare thread");
        assert_eq!(outstanding_on_this_thread(), before + 1);
        drop(factory);
        assert_eq!(outstanding_on_this_thread(), before);
    })
    .join()
    .expect("thread");
}

// ---------------------------------------------------------------------------------------------------------------
// Golden-driven tests
// ---------------------------------------------------------------------------------------------------------------

/// The `windows` crate enum constant of a C# enum member, `None` when the member has no Rust mapping.
fn wic_enum_value(enum_name: &str, member: &str) -> Option<i64> {
    Some(match (enum_name, member) {
        ("WICColorContextType", "Uninitialized") => WICColorContextUninitialized.0 as i64,
        ("WICColorContextType", "Profile") => WICColorContextProfile.0 as i64,
        ("WICColorContextType", "ExifColorSpace") => WICColorContextExifColorSpace.0 as i64,
        ("WICDecodeOptions", "WICDecodeMetadataCacheOnDemand") => WICDecodeMetadataCacheOnDemand.0 as i64,
        ("WICDecodeOptions", "WICDecodeMetadataCacheOnLoad") => WICDecodeMetadataCacheOnLoad.0 as i64,
        ("WICBitmapDitherType", "None") => WICBitmapDitherTypeNone.0 as i64,
        ("WICBitmapPaletteType", "Custom") => WICBitmapPaletteTypeCustom.0 as i64,
        ("WICBitmapInterpolationMode", "NearestNeighbor") => WICBitmapInterpolationModeNearestNeighbor.0 as i64,
        ("WICBitmapInterpolationMode", "Linear") => WICBitmapInterpolationModeLinear.0 as i64,
        ("WICBitmapInterpolationMode", "Cubic") => WICBitmapInterpolationModeCubic.0 as i64,
        ("WICBitmapInterpolationMode", "Fant") => WICBitmapInterpolationModeFant.0 as i64,
        ("WICBitmapInterpolationMode", "HighQualityCubic") => WICBitmapInterpolationModeHighQualityCubic.0 as i64,
        ("WICBitmapTransformOptions", "Rotate0") => WICBitmapTransformRotate0.0 as i64,
        ("WICBitmapTransformOptions", "Rotate90") => WICBitmapTransformRotate90.0 as i64,
        ("WICBitmapTransformOptions", "Rotate180") => WICBitmapTransformRotate180.0 as i64,
        ("WICBitmapTransformOptions", "Rotate270") => WICBitmapTransformRotate270.0 as i64,
        ("WICBitmapTransformOptions", "FlipHorizontal") => WICBitmapTransformFlipHorizontal.0 as i64,
        ("WICBitmapTransformOptions", "FlipVertical") => WICBitmapTransformFlipVertical.0 as i64,
        _ => return None,
    })
}

/// The `windows` crate's `IID` of a golden interface name (`IComStream` is `IStream`).
fn iid_of(name: &str) -> Option<GUID> {
    Some(match name {
        "IWICBitmapSource" => IWICBitmapSource::IID,
        "IWICBitmapSourceTransform" => IWICBitmapSourceTransform::IID,
        "IWICBitmapFrameDecode" => windows::Win32::Graphics::Imaging::IWICBitmapFrameDecode::IID,
        "IWICColorContext" => windows::Win32::Graphics::Imaging::IWICColorContext::IID,
        "IWICColorTransform" => windows::Win32::Graphics::Imaging::IWICColorTransform::IID,
        "IWICBitmapDecoder" => windows::Win32::Graphics::Imaging::IWICBitmapDecoder::IID,
        "IWICFormatConverter" => windows::Win32::Graphics::Imaging::IWICFormatConverter::IID,
        "IWICBitmapScaler" => IWICBitmapScaler::IID,
        "IWICBitmapFlipRotator" => IWICBitmapFlipRotator::IID,
        "IWICMetadataQueryReader" => windows::Win32::Graphics::Imaging::IWICMetadataQueryReader::IID,
        "IComStream" => IStream::IID,
        "IWICStream" => IWICStream::IID,
        "IWICImagingFactory" => IWICImagingFactory::IID,
        _ => return None,
    })
}

/// Byte offset of one golden method in the `windows` crate's vtable of that interface. Inherited methods are reached
/// through `base__` (the golden lists them under the derived interface too). `GetContextType` is `GetType` in the
/// crate. `None` for the two factory slots that are private placeholders (checked by the neighbour test instead).
fn actual_offset(interface: &str, method: &str) -> Option<usize> {
    Some(match (interface, method) {
        // IWICBitmapSource (IUnknown: slots 0..2)
        ("IWICBitmapSource", "GetSize") => offset_of!(IWICBitmapSource_Vtbl, GetSize),
        ("IWICBitmapSource", "GetPixelFormat") => offset_of!(IWICBitmapSource_Vtbl, GetPixelFormat),
        ("IWICBitmapSource", "GetResolution") => offset_of!(IWICBitmapSource_Vtbl, GetResolution),
        ("IWICBitmapSource", "CopyPalette") => offset_of!(IWICBitmapSource_Vtbl, CopyPalette),
        ("IWICBitmapSource", "CopyPixels") => offset_of!(IWICBitmapSource_Vtbl, CopyPixels),
        // IWICBitmapSourceTransform
        ("IWICBitmapSourceTransform", "CopyPixels") => offset_of!(IWICBitmapSourceTransform_Vtbl, CopyPixels),
        ("IWICBitmapSourceTransform", "GetClosestSize") => offset_of!(IWICBitmapSourceTransform_Vtbl, GetClosestSize),
        ("IWICBitmapSourceTransform", "GetClosestPixelFormat") => {
            offset_of!(IWICBitmapSourceTransform_Vtbl, GetClosestPixelFormat)
        }
        ("IWICBitmapSourceTransform", "DoesSupportTransform") => {
            offset_of!(IWICBitmapSourceTransform_Vtbl, DoesSupportTransform)
        }
        // IWICBitmapFrameDecode : IWICBitmapSource
        ("IWICBitmapFrameDecode", "GetMetadataQueryReader") => {
            offset_of!(
                windows::Win32::Graphics::Imaging::IWICBitmapFrameDecode_Vtbl,
                GetMetadataQueryReader
            )
        }
        ("IWICBitmapFrameDecode", "GetColorContexts") => {
            offset_of!(IWICBitmapFrameDecode_Vtbl, GetColorContexts)
        }
        ("IWICBitmapFrameDecode", "GetThumbnail") => offset_of!(IWICBitmapFrameDecode_Vtbl, GetThumbnail),
        ("IWICBitmapFrameDecode", "GetSize") => offset_of!(IWICBitmapFrameDecode_Vtbl, base__.GetSize),
        ("IWICBitmapFrameDecode", "GetPixelFormat") => offset_of!(IWICBitmapFrameDecode_Vtbl, base__.GetPixelFormat),
        ("IWICBitmapFrameDecode", "GetResolution") => offset_of!(IWICBitmapFrameDecode_Vtbl, base__.GetResolution),
        ("IWICBitmapFrameDecode", "CopyPalette") => offset_of!(IWICBitmapFrameDecode_Vtbl, base__.CopyPalette),
        ("IWICBitmapFrameDecode", "CopyPixels") => offset_of!(IWICBitmapFrameDecode_Vtbl, base__.CopyPixels),
        // IWICColorContext
        ("IWICColorContext", "InitializeFromFilename") => offset_of!(IWICColorContext_Vtbl, InitializeFromFilename),
        ("IWICColorContext", "InitializeFromMemory") => offset_of!(IWICColorContext_Vtbl, InitializeFromMemory),
        ("IWICColorContext", "InitializeFromExifColorSpace") => {
            offset_of!(IWICColorContext_Vtbl, InitializeFromExifColorSpace)
        }
        ("IWICColorContext", "GetContextType") => offset_of!(IWICColorContext_Vtbl, GetType),
        ("IWICColorContext", "GetProfileBytes") => offset_of!(IWICColorContext_Vtbl, GetProfileBytes),
        ("IWICColorContext", "GetExifColorSpace") => offset_of!(IWICColorContext_Vtbl, GetExifColorSpace),
        // IWICColorTransform : IWICBitmapSource
        ("IWICColorTransform", "Initialize") => offset_of!(IWICColorTransform_Vtbl, Initialize),
        ("IWICColorTransform", "GetSize") => offset_of!(IWICColorTransform_Vtbl, base__.GetSize),
        ("IWICColorTransform", "GetPixelFormat") => offset_of!(IWICColorTransform_Vtbl, base__.GetPixelFormat),
        ("IWICColorTransform", "GetResolution") => offset_of!(IWICColorTransform_Vtbl, base__.GetResolution),
        ("IWICColorTransform", "CopyPalette") => offset_of!(IWICColorTransform_Vtbl, base__.CopyPalette),
        ("IWICColorTransform", "CopyPixels") => offset_of!(IWICColorTransform_Vtbl, base__.CopyPixels),
        // IWICBitmapDecoder
        ("IWICBitmapDecoder", "QueryCapability") => offset_of!(IWICBitmapDecoder_Vtbl, QueryCapability),
        ("IWICBitmapDecoder", "Initialize") => offset_of!(IWICBitmapDecoder_Vtbl, Initialize),
        ("IWICBitmapDecoder", "GetContainerFormat") => offset_of!(IWICBitmapDecoder_Vtbl, GetContainerFormat),
        ("IWICBitmapDecoder", "GetDecoderInfo") => offset_of!(IWICBitmapDecoder_Vtbl, GetDecoderInfo),
        ("IWICBitmapDecoder", "CopyPalette") => offset_of!(IWICBitmapDecoder_Vtbl, CopyPalette),
        ("IWICBitmapDecoder", "GetMetadataQueryReader") => offset_of!(IWICBitmapDecoder_Vtbl, GetMetadataQueryReader),
        ("IWICBitmapDecoder", "GetPreview") => offset_of!(IWICBitmapDecoder_Vtbl, GetPreview),
        ("IWICBitmapDecoder", "GetColorContexts") => offset_of!(IWICBitmapDecoder_Vtbl, GetColorContexts),
        ("IWICBitmapDecoder", "GetThumbnail") => offset_of!(IWICBitmapDecoder_Vtbl, GetThumbnail),
        ("IWICBitmapDecoder", "GetFrameCount") => offset_of!(IWICBitmapDecoder_Vtbl, GetFrameCount),
        ("IWICBitmapDecoder", "GetFrame") => offset_of!(IWICBitmapDecoder_Vtbl, GetFrame),
        // IWICFormatConverter : IWICBitmapSource
        ("IWICFormatConverter", "Initialize") => offset_of!(IWICFormatConverter_Vtbl, Initialize),
        ("IWICFormatConverter", "CanConvert") => offset_of!(IWICFormatConverter_Vtbl, CanConvert),
        ("IWICFormatConverter", "GetSize") => offset_of!(IWICFormatConverter_Vtbl, base__.GetSize),
        ("IWICFormatConverter", "GetPixelFormat") => offset_of!(IWICFormatConverter_Vtbl, base__.GetPixelFormat),
        ("IWICFormatConverter", "GetResolution") => offset_of!(IWICFormatConverter_Vtbl, base__.GetResolution),
        ("IWICFormatConverter", "CopyPalette") => offset_of!(IWICFormatConverter_Vtbl, base__.CopyPalette),
        ("IWICFormatConverter", "CopyPixels") => offset_of!(IWICFormatConverter_Vtbl, base__.CopyPixels),
        // IWICBitmapScaler : IWICBitmapSource
        ("IWICBitmapScaler", "Initialize") => offset_of!(IWICBitmapScaler_Vtbl, Initialize),
        ("IWICBitmapScaler", "GetSize") => offset_of!(IWICBitmapScaler_Vtbl, base__.GetSize),
        ("IWICBitmapScaler", "GetPixelFormat") => offset_of!(IWICBitmapScaler_Vtbl, base__.GetPixelFormat),
        ("IWICBitmapScaler", "GetResolution") => offset_of!(IWICBitmapScaler_Vtbl, base__.GetResolution),
        ("IWICBitmapScaler", "CopyPalette") => offset_of!(IWICBitmapScaler_Vtbl, base__.CopyPalette),
        ("IWICBitmapScaler", "CopyPixels") => offset_of!(IWICBitmapScaler_Vtbl, base__.CopyPixels),
        // IWICBitmapFlipRotator : IWICBitmapSource
        ("IWICBitmapFlipRotator", "Initialize") => offset_of!(IWICBitmapFlipRotator_Vtbl, Initialize),
        ("IWICBitmapFlipRotator", "GetSize") => offset_of!(IWICBitmapFlipRotator_Vtbl, base__.GetSize),
        ("IWICBitmapFlipRotator", "GetPixelFormat") => offset_of!(IWICBitmapFlipRotator_Vtbl, base__.GetPixelFormat),
        ("IWICBitmapFlipRotator", "GetResolution") => offset_of!(IWICBitmapFlipRotator_Vtbl, base__.GetResolution),
        ("IWICBitmapFlipRotator", "CopyPalette") => offset_of!(IWICBitmapFlipRotator_Vtbl, base__.CopyPalette),
        ("IWICBitmapFlipRotator", "CopyPixels") => offset_of!(IWICBitmapFlipRotator_Vtbl, base__.CopyPixels),
        // IWICMetadataQueryReader
        ("IWICMetadataQueryReader", "GetContainerFormat") => {
            offset_of!(IWICMetadataQueryReader_Vtbl, GetContainerFormat)
        }
        ("IWICMetadataQueryReader", "GetLocation") => offset_of!(IWICMetadataQueryReader_Vtbl, GetLocation),
        ("IWICMetadataQueryReader", "GetMetadataByName") => offset_of!(IWICMetadataQueryReader_Vtbl, GetMetadataByName),
        ("IWICMetadataQueryReader", "GetEnumerator") => offset_of!(IWICMetadataQueryReader_Vtbl, GetEnumerator),
        // IComStream = IStream (its Read/Write come from the ISequentialStream base)
        ("IComStream", "Read") => offset_of!(IStream_Vtbl, base__.Read),
        ("IComStream", "Write") => offset_of!(IStream_Vtbl, base__.Write),
        ("IComStream", "Seek") => offset_of!(IStream_Vtbl, Seek),
        ("IComStream", "SetSize") => offset_of!(IStream_Vtbl, SetSize),
        ("IComStream", "CopyTo") => offset_of!(IStream_Vtbl, CopyTo),
        ("IComStream", "Commit") => offset_of!(IStream_Vtbl, Commit),
        ("IComStream", "Revert") => offset_of!(IStream_Vtbl, Revert),
        ("IComStream", "LockRegion") => offset_of!(IStream_Vtbl, LockRegion),
        ("IComStream", "UnlockRegion") => offset_of!(IStream_Vtbl, UnlockRegion),
        ("IComStream", "Stat") => offset_of!(IStream_Vtbl, Stat),
        ("IComStream", "Clone") => offset_of!(IStream_Vtbl, Clone),
        // IWICStream : IComStream (own slots 14..17, the stream slots are inherited)
        ("IWICStream", "InitializeFromIStream") => offset_of!(IWICStream_Vtbl, InitializeFromIStream),
        ("IWICStream", "InitializeFromFilename") => offset_of!(IWICStream_Vtbl, InitializeFromFilename),
        ("IWICStream", "InitializeFromMemory") => offset_of!(IWICStream_Vtbl, InitializeFromMemory),
        ("IWICStream", "InitializeFromIStreamRegion") => offset_of!(IWICStream_Vtbl, InitializeFromIStreamRegion),
        ("IWICStream", "Read") => offset_of!(IWICStream_Vtbl, base__.base__.Read),
        ("IWICStream", "Write") => offset_of!(IWICStream_Vtbl, base__.base__.Write),
        ("IWICStream", "Seek") => offset_of!(IWICStream_Vtbl, base__.Seek),
        ("IWICStream", "SetSize") => offset_of!(IWICStream_Vtbl, base__.SetSize),
        ("IWICStream", "CopyTo") => offset_of!(IWICStream_Vtbl, base__.CopyTo),
        ("IWICStream", "Commit") => offset_of!(IWICStream_Vtbl, base__.Commit),
        ("IWICStream", "Revert") => offset_of!(IWICStream_Vtbl, base__.Revert),
        ("IWICStream", "LockRegion") => offset_of!(IWICStream_Vtbl, base__.LockRegion),
        ("IWICStream", "UnlockRegion") => offset_of!(IWICStream_Vtbl, base__.UnlockRegion),
        ("IWICStream", "Stat") => offset_of!(IWICStream_Vtbl, base__.Stat),
        ("IWICStream", "Clone") => offset_of!(IWICStream_Vtbl, base__.Clone),
        // IWICImagingFactory (CreateBitmapFromHBITMAP and CreateBitmapFromHICON are private placeholders)
        ("IWICImagingFactory", "CreateDecoderFromFilename") => {
            offset_of!(IWICImagingFactory_Vtbl, CreateDecoderFromFilename)
        }
        ("IWICImagingFactory", "CreateDecoderFromStream") => {
            offset_of!(IWICImagingFactory_Vtbl, CreateDecoderFromStream)
        }
        ("IWICImagingFactory", "CreateDecoderFromFileHandle") => {
            offset_of!(IWICImagingFactory_Vtbl, CreateDecoderFromFileHandle)
        }
        ("IWICImagingFactory", "CreateComponentInfo") => offset_of!(IWICImagingFactory_Vtbl, CreateComponentInfo),
        ("IWICImagingFactory", "CreateDecoder") => offset_of!(IWICImagingFactory_Vtbl, CreateDecoder),
        ("IWICImagingFactory", "CreateEncoder") => offset_of!(IWICImagingFactory_Vtbl, CreateEncoder),
        ("IWICImagingFactory", "CreatePalette") => offset_of!(IWICImagingFactory_Vtbl, CreatePalette),
        ("IWICImagingFactory", "CreateFormatConverter") => offset_of!(IWICImagingFactory_Vtbl, CreateFormatConverter),
        ("IWICImagingFactory", "CreateBitmapScaler") => offset_of!(IWICImagingFactory_Vtbl, CreateBitmapScaler),
        ("IWICImagingFactory", "CreateBitmapClipper") => offset_of!(IWICImagingFactory_Vtbl, CreateBitmapClipper),
        ("IWICImagingFactory", "CreateBitmapFlipRotator") => {
            offset_of!(IWICImagingFactory_Vtbl, CreateBitmapFlipRotator)
        }
        ("IWICImagingFactory", "CreateStream") => offset_of!(IWICImagingFactory_Vtbl, CreateStream),
        ("IWICImagingFactory", "CreateColorContext") => offset_of!(IWICImagingFactory_Vtbl, CreateColorContext),
        ("IWICImagingFactory", "CreateColorTransformer") => offset_of!(IWICImagingFactory_Vtbl, CreateColorTransformer),
        ("IWICImagingFactory", "CreateBitmap") => offset_of!(IWICImagingFactory_Vtbl, CreateBitmap),
        ("IWICImagingFactory", "CreateBitmapFromSource") => offset_of!(IWICImagingFactory_Vtbl, CreateBitmapFromSource),
        ("IWICImagingFactory", "CreateBitmapFromSourceRect") => {
            offset_of!(IWICImagingFactory_Vtbl, CreateBitmapFromSourceRect)
        }
        ("IWICImagingFactory", "CreateBitmapFromMemory") => offset_of!(IWICImagingFactory_Vtbl, CreateBitmapFromMemory),
        ("IWICImagingFactory", "CreateComponentEnumerator") => {
            offset_of!(IWICImagingFactory_Vtbl, CreateComponentEnumerator)
        }
        ("IWICImagingFactory", "CreateFastMetadataEncoderFromDecoder") => {
            offset_of!(IWICImagingFactory_Vtbl, CreateFastMetadataEncoderFromDecoder)
        }
        ("IWICImagingFactory", "CreateFastMetadataEncoderFromFrameDecode") => {
            offset_of!(IWICImagingFactory_Vtbl, CreateFastMetadataEncoderFromFrameDecode)
        }
        ("IWICImagingFactory", "CreateQueryWriter") => offset_of!(IWICImagingFactory_Vtbl, CreateQueryWriter),
        ("IWICImagingFactory", "CreateQueryWriterFromReader") => {
            offset_of!(IWICImagingFactory_Vtbl, CreateQueryWriterFromReader)
        }
        _ => return None,
    })
}

/// One interface of the `wic-interop-constants` golden.
struct GoldenInterface {
    name: String,
    iid: String,
    bases: Vec<String>,
    first_slot: usize,
    methods: Vec<String>,
}

fn golden_interfaces(section: &Value) -> Vec<GoldenInterface> {
    section["interfaces"]
        .as_array()
        .expect("interfaces")
        .iter()
        .map(|i| GoldenInterface {
            name: golden::str_of(i, "interface").to_owned(),
            iid: golden::str_of(i, "iid").to_owned(),
            bases: i["bases"]
                .as_array()
                .expect("bases")
                .iter()
                .map(|b| b.as_str().expect("base").to_owned())
                .collect(),
            first_slot: golden::i64_of(i, "first_slot") as usize,
            methods: i["methods"]
                .as_array()
                .expect("methods")
                .iter()
                .map(|m| m["method"].as_str().expect("method").to_owned())
                .collect(),
        })
        .collect()
}

fn has_method(all: &[GoldenInterface], name: &str, method: &str) -> bool {
    all.iter()
        .find(|i| i.name == name)
        .is_some_and(|i| i.methods.iter().any(|m| m == method) || i.bases.iter().any(|b| has_method(all, b, method)))
}

/// The vtable slot the golden states for `method` of `name`: own methods are `first_slot + position among the own
/// methods`; an inherited method takes the slot of its base interface.
fn golden_slot(all: &[GoldenInterface], name: &str, method: &str) -> Option<usize> {
    let interface = all.iter().find(|i| i.name == name)?;
    if let Some(base) = interface.bases.iter().find(|b| has_method(all, b, method)) {
        return golden_slot(all, base, method);
    }
    let own: Vec<&String> = interface
        .methods
        .iter()
        .filter(|m| !interface.bases.iter().any(|b| has_method(all, b, m)))
        .collect();
    let position = own.iter().position(|m| m.as_str() == method)?;
    Some(interface.first_slot + position)
}

// Golden: wic-interop-constants (guids, enums, interfaces, statstg, sdk)
#[test]
fn guids_match_golden_wic_interop_constants() {
    let doc = golden::load("wic-interop-constants");
    let section = doc
        .cases
        .iter()
        .find(|c| c["section"] == "guids")
        .expect("guids section");
    let expected = section["guids"].as_array().expect("guids");
    assert_eq!(
        expected.len(),
        GUID_ALL.len(),
        "the guids list has one entry per WicGuids field"
    );
    for (entry, (name, value)) in expected.iter().zip(GUID_ALL.iter()) {
        assert_eq!(golden::str_of(entry, "field"), *name, "declaration order");
        assert_eq!(golden::str_of(entry, "value"), guid_text(value), "{name}");
    }
}

#[test]
fn enums_match_golden_wic_interop_constants() {
    let doc = golden::load("wic-interop-constants");
    let section = doc
        .cases
        .iter()
        .find(|c| c["section"] == "enums")
        .expect("enums section");
    let mut failures = Vec::new();
    for entry in section["enums"].as_array().expect("enums") {
        let enum_name = golden::str_of(entry, "enum");
        for member in entry["members"].as_array().expect("members") {
            let name = golden::str_of(member, "member");
            let value = golden::i64_of(member, "value");
            match wic_enum_value(enum_name, name) {
                Some(got) if got == value => {}
                Some(got) => failures.push(format!("{enum_name}.{name}: golden {value}, port {got}")),
                None => failures.push(format!("{enum_name}.{name}: no mapping in the port")),
            }
        }
    }
    assert!(failures.is_empty(), "enum mismatches:\n{}", failures.join("\n"));
}

#[test]
fn interface_iids_and_vtable_slots_match_golden_wic_interop_constants() {
    let doc = golden::load("wic-interop-constants");
    let section = doc
        .cases
        .iter()
        .find(|c| c["section"] == "interfaces")
        .expect("interfaces section");
    let all = golden_interfaces(section);
    let word = size_of::<usize>();
    let mut failures = Vec::new();
    for interface in &all {
        match iid_of(&interface.name) {
            Some(iid) if guid_text(&iid) == interface.iid => {}
            Some(iid) => failures.push(format!(
                "{}: IID golden {} port {}",
                interface.name,
                interface.iid,
                guid_text(&iid)
            )),
            None => failures.push(format!("{}: no IID mapping", interface.name)),
        }
        for method in &interface.methods {
            let expected = golden_slot(&all, &interface.name, method).expect("golden slot") * word;
            match actual_offset(&interface.name, method) {
                Some(offset) if offset == expected => {}
                Some(offset) => failures.push(format!(
                    "{}.{method}: golden offset {expected}, port {offset}",
                    interface.name
                )),
                None => {
                    // The two private factory placeholders: their neighbours carry the check (see the neighbour test).
                    assert!(
                        interface.name == "IWICImagingFactory"
                            && (method == "CreateBitmapFromHBITMAP" || method == "CreateBitmapFromHICON"),
                        "no offset mapping for {}.{method}",
                        interface.name
                    );
                }
            }
        }
    }
    assert!(failures.is_empty(), "vtable mismatches:\n{}", failures.join("\n"));
}

/// The `IWICImagingFactory` slots 18 and 19 (CreateBitmapFromHBITMAP, CreateBitmapFromHICON) are private placeholders
/// in the crate; their public neighbours fix them: CreateComponentEnumerator is 3 slots after CreateBitmapFromMemory.
#[test]
fn factory_placeholder_slots_are_fixed_by_their_public_neighbours() {
    let word = size_of::<usize>();
    assert_eq!(
        offset_of!(IWICImagingFactory_Vtbl, CreateComponentEnumerator)
            - offset_of!(IWICImagingFactory_Vtbl, CreateBitmapFromMemory),
        3 * word
    );
}

#[test]
fn statstg_layout_matches_golden_wic_interop_constants() {
    let doc = golden::load("wic-interop-constants");
    let section = doc
        .cases
        .iter()
        .find(|c| c["section"] == "statstg")
        .expect("statstg section");
    assert_eq!(golden::i64_of(section, "pointer_size") as usize, size_of::<usize>());
    assert_eq!(golden::i64_of(section, "size") as usize, size_of::<STATSTG>());
    let offset = |field: &str| -> usize {
        match field {
            "pwcsName" => offset_of!(STATSTG, pwcsName),
            "type" => offset_of!(STATSTG, r#type),
            "cbSize" => offset_of!(STATSTG, cbSize),
            "mtime" => offset_of!(STATSTG, mtime),
            "ctime" => offset_of!(STATSTG, ctime),
            "atime" => offset_of!(STATSTG, atime),
            "grfMode" => offset_of!(STATSTG, grfMode),
            "grfLocksSupported" => offset_of!(STATSTG, grfLocksSupported),
            "clsid" => offset_of!(STATSTG, clsid),
            "grfStateBits" => offset_of!(STATSTG, grfStateBits),
            "reserved" => offset_of!(STATSTG, reserved),
            other => panic!("unknown STATSTG field {other}"),
        }
    };
    for field in section["fields"].as_array().expect("fields") {
        let name = golden::str_of(field, "field");
        assert_eq!(offset(name), golden::i64_of(field, "offset") as usize, "STATSTG.{name}");
    }
}

#[test]
fn factory_entry_point_and_sdk_version_match_golden_wic_interop_constants() {
    let doc = golden::load("wic-interop-constants");
    let section = doc.cases.iter().find(|c| c["section"] == "sdk").expect("sdk section");
    assert_eq!(golden::i64_of(section, "WINCODEC_SDK_VERSION1") as u32, 0x0236);
    assert_eq!(
        golden::str_of(section, "factory_entry_point"),
        "WICCreateImagingFactory_Proxy"
    );
    assert_eq!(pr_platform_win::wic::interop::WINCODEC_SDK_VERSION1, 0x0236);
}

// Golden: wic-hresult-exception (every HRESULT: .NET type, base types)
#[test]
fn golden_wic_hresult_exception_maps_every_hresult() {
    let doc = golden::load("wic-hresult-exception");
    let mut failures = Vec::new();
    let mut checked = 0usize;
    for case in &doc.cases {
        let group = golden::str_of(case, "name");
        for (index, result) in case["results"].as_array().expect("results").iter().enumerate() {
            checked += 1;
            let hresult = hex_i32(golden::str_of(result, "hresult"));
            let kind = exception_kind_for_hresult(hresult);
            let expected_type = result["type"].as_str();
            let got_type = kind.map(|k| k.type_name());
            if got_type != expected_type {
                failures.push(format!(
                    "{group}[{index}] {hresult:#010x}: golden type {expected_type:?}, port {got_type:?}"
                ));
                continue;
            }
            let Some(kind) = kind else { continue };
            // Exception HResult and the type of a bare WicError::Com with the same code agree (INV-12 input).
            let via_error = WicError::Com { hresult }.exception_kind();
            if via_error != kind {
                failures.push(format!(
                    "{group}[{index}] {hresult:#010x}: WicError::Com kind {via_error:?}"
                ));
            }
            let bases: Vec<&str> = result["base_types"]
                .as_array()
                .map(|b| b.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let is = |name: &str| expected_type == Some(name) || bases.contains(&name);
            let checks = [
                ("is_argument", kind.is_argument(), is("System.ArgumentException")),
                ("is_io", kind.is_io(), is("System.IO.IOException")),
                (
                    "is_not_supported",
                    kind.is_not_supported(),
                    is("System.NotSupportedException"),
                ),
                (
                    "is_invalid_operation",
                    kind.is_invalid_operation(),
                    is("System.InvalidOperationException"),
                ),
                ("is_arithmetic", kind.is_arithmetic(), is("System.ArithmeticException")),
            ];
            for (helper, got, want) in checks {
                if got != want {
                    failures.push(format!(
                        "{group}[{index}] {hresult:#010x} {helper}: golden {want}, port {got}"
                    ));
                }
            }
        }
    }
    assert!(checked > 0, "the golden has rows");
    assert!(
        failures.is_empty(),
        "{} mismatches of {checked}:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// Golden: wic-managed-istream (33 scripts through the real IStream vtable of a ComStream)

/// The subject of one script: the live `ComStream`, or the backend it returned on its first `dispose`.
struct Subject {
    com: ComStream,
    gone: Option<Box<dyn StreamBackend>>,
    disposed: bool,
}

impl Subject {
    /// Position, length and full content, through the safe methods while live, through the kept backend after dispose.
    fn probe(&mut self) -> (u64, u64, Vec<u8>) {
        if let Some(backend) = self.gone.as_mut() {
            let position = backend.seek(SeekFrom::Current(0)).expect("position");
            let length = backend.len().expect("length");
            backend.seek(SeekFrom::Start(0)).expect("rewind");
            let mut content = vec![0u8; length as usize];
            let mut filled = 0;
            while filled < content.len() {
                match backend.read(&mut content[filled..]).expect("read content") {
                    0 => break,
                    n => filled += n,
                }
            }
            backend.seek(SeekFrom::Start(position)).expect("restore position");
            return (position, length, content);
        }
        let position = self.com.seek(0, 1).expect("position");
        let length = self.com.stat().expect("stat").cbSize;
        self.com.seek(0, 0).expect("rewind");
        let mut content = vec![0u8; length as usize];
        let mut filled = 0;
        while filled < content.len() {
            match self.com.read(&mut content[filled..]).expect("read content") {
                0 => break,
                n => filled += n,
            }
        }
        self.com.seek(position as i64, 0).expect("restore position");
        (position, length, content)
    }
}

/// Runs one golden script; returns its mismatches (empty when it matches).
fn run_istream_script(case: &Value) -> Vec<String> {
    let name = golden::str_of(case, "name");
    let backend = &case["backend"];
    let data = bytes_of(&backend["data"]);
    let start = golden::i64_of(backend, "position") as u64;
    let com = if golden::str_of(backend, "kind") == "readonly" {
        ComStream::from_bytes(Arc::from(data.as_slice()))
    } else {
        let com = ComStream::writable_memory();
        com.write(&data).expect("initial content");
        com
    };
    com.seek(start as i64, 0).expect("initial position");
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    let mut subject = Subject {
        com,
        gone: None,
        disposed: false,
    };

    let ops = case["ops"].as_array().expect("ops");
    let results = case["results"].as_array().expect("results");
    let mut failures = Vec::new();
    for (index, (op, expected)) in ops.iter().zip(results.iter()).enumerate() {
        let label = format!("{name} op#{index} {}", golden::str_of(op, "op"));
        let null_out = op["null_out"].as_bool().unwrap_or(false);
        let mut out_u32: Option<u32> = None;
        let mut out_u64: Option<u64> = None;
        let mut buffer: Option<Vec<u8>> = None;
        let mut stat_bytes: Option<Vec<u8>> = None;
        let hr: i32 = match golden::str_of(op, "op") {
            "read" => {
                let cb = golden::i64_of(op, "cb");
                let mut bytes = vec![UNREAD; if cb > 0 { cb as usize } else { 1 }];
                let mut pcb: u32 = SENTINEL_U32;
                let pcb_ptr = if null_out {
                    ptr::null_mut()
                } else {
                    &mut pcb as *mut u32
                };
                let hr = vt_read(raw, bytes.as_mut_ptr().cast(), cb as u32, pcb_ptr).0;
                out_u32 = Some(pcb);
                buffer = Some(bytes);
                hr
            }
            "seek" => {
                let mut pnew: u64 = SENTINEL_U64;
                let pnew_ptr = if null_out {
                    ptr::null_mut()
                } else {
                    &mut pnew as *mut u64
                };
                let hr = vt_seek(
                    raw,
                    golden::i64_of(op, "move"),
                    golden::i64_of(op, "origin") as u32,
                    pnew_ptr,
                )
                .0;
                out_u64 = Some(pnew);
                hr
            }
            "write" => {
                let bytes = op.get("data").map(bytes_of).unwrap_or_default();
                let cb = op.get("cb").and_then(Value::as_i64).unwrap_or(bytes.len() as i64);
                let mut pcb: u32 = SENTINEL_U32;
                let pcb_ptr = if null_out {
                    ptr::null_mut()
                } else {
                    &mut pcb as *mut u32
                };
                let hr = vt_write(raw, bytes.as_ptr().cast(), cb as u32, pcb_ptr).0;
                out_u32 = Some(pcb);
                hr
            }
            "set_size" => vt_set_size(raw, golden::i64_of(op, "size") as u64).0,
            "stat" => {
                let mut stat = [SENTINEL_U64; 10];
                let hr = vt_stat(raw, stat.as_mut_ptr().cast(), golden::i64_of(op, "flag") as u32).0;
                if hr == 0 {
                    stat_bytes = Some(stat.iter().flat_map(|w| w.to_ne_bytes()).collect());
                }
                hr
            }
            "commit" => vt_commit(raw, 0).0,
            "revert" => vt_revert(raw).0,
            "lock" => vt_lock(raw, 0, 1, 0).0,
            "unlock" => vt_unlock(raw, 0, 1, 0).0,
            "clone" => {
                let mut out: *mut c_void = ptr::null_mut();
                vt_clone(raw, &mut out).0
            }
            "copy_to" => vt_copy_to(raw, ptr::null_mut(), 1, ptr::null_mut(), ptr::null_mut()).0,
            "dispose" => {
                let got = subject.com.dispose();
                if subject.disposed {
                    if got.is_some() {
                        failures.push(format!("{label}: a second dispose must return None"));
                    }
                } else {
                    subject.gone = got;
                    subject.disposed = true;
                }
                0
            }
            other => panic!("unknown golden op {other}"),
        };

        let expected_hr = hex_i32(golden::str_of(expected, "hr"));
        if hr != expected_hr {
            failures.push(format!("{label}: hr golden {expected_hr:#010x}, port {hr:#010x}"));
        }
        if let (Some(golden_out), Some(got)) = (expected.get("out_u32"), out_u32) {
            if u64_of(golden_out) != u64::from(got) {
                failures.push(format!("{label}: out_u32 golden {}, port {got}", u64_of(golden_out)));
            }
        }
        if let (Some(golden_out), Some(got)) = (expected.get("out_u64"), out_u64) {
            if u64_of(golden_out) != got {
                failures.push(format!("{label}: out_u64 golden {}, port {got}", u64_of(golden_out)));
            }
        }
        if let (Some(golden_buffer), Some(got)) = (expected.get("buffer"), buffer) {
            let want = bytes_of(golden_buffer);
            if got[..want.len()] != want[..] {
                failures.push(format!(
                    "{label}: buffer golden {want:?}, port {:?}",
                    &got[..want.len()]
                ));
            }
        }
        if let (Some(golden_stat), Some(got)) = (expected.get("stat_bytes"), stat_bytes) {
            let hex: String = got.iter().map(|b| format!("{b:02x}")).collect();
            // The golden writes upper-case hex (C# `Convert.ToHexString`); the bytes are what is compared.
            if !golden::str_of(expected, "stat_bytes").eq_ignore_ascii_case(&hex) {
                failures.push(format!("{label}: stat_bytes golden {golden_stat}, port {hex}"));
            }
        }
        let (position, length, content) = subject.probe();
        if position != golden::i64_of(expected, "position") as u64 {
            failures.push(format!(
                "{label}: position golden {}, port {position}",
                golden::i64_of(expected, "position")
            ));
        }
        if length != golden::i64_of(expected, "length") as u64 {
            failures.push(format!(
                "{label}: length golden {}, port {length}",
                golden::i64_of(expected, "length")
            ));
        }
        if let Some(golden_content) = expected.get("content") {
            let want = bytes_of(golden_content);
            if content != want {
                failures.push(format!("{label}: content golden {want:?}, port {content:?}"));
            }
        }
    }
    failures
}

#[test]
fn golden_wic_managed_istream_scripts_match_the_csharp_managed_stream() {
    let doc = golden::load("wic-managed-istream");
    let mut failures = Vec::new();
    for case in &doc.cases {
        failures.extend(run_istream_script(case));
    }
    assert!(
        failures.is_empty(),
        "{} mismatches over {} scripts:\n{}",
        failures.len(),
        doc.cases.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Repair round 1 (verifier findings): .NET sweep of the HRESULT table, FileStream semantics of the `File`
// backend, and tests that kill the surviving mutants of the write path.
// ---------------------------------------------------------------------------------------------------------------

/// Every HRESULT in 20 facilities x 65 536 codes for which .NET 10 `Marshal.GetExceptionForHR(hr, -1)` creates a
/// type other than `COMException` (oracle sweep run by the W4-11a verifier; every other code is a COMException).
const DOTNET_SPECIFIC_EXCEPTION_TYPES: &str = "
8000000F System.TypeLoadException
80000013 System.ObjectDisposedException
8000211D System.Reflection.AmbiguousMatchException
80004001 System.NotImplementedException
80004002 System.InvalidCastException
80004003 System.NullReferenceException
8002000E System.Reflection.TargetParameterCountException
80020012 System.DivideByZeroException
80030003 System.IO.DirectoryNotFoundException
80070002 System.IO.FileNotFoundException
80070003 System.IO.DirectoryNotFoundException
80070004 System.IO.FileLoadException
80070005 System.UnauthorizedAccessException
8007000B System.BadImageFormatException
8007000E System.OutOfMemoryException
80070015 System.IO.FileNotFoundException
80070020 System.IO.FileLoadException
80070021 System.IO.FileLoadException
80070026 System.IO.EndOfStreamException
80070035 System.IO.FileNotFoundException
80070043 System.IO.FileNotFoundException
80070057 System.ArgumentException
8007006E System.IO.FileLoadException
8007007B System.IO.FileNotFoundException
8007007E System.IO.FileNotFoundException
800700B6 System.BadImageFormatException
800700C0 System.BadImageFormatException
800700C1 System.BadImageFormatException
800700CE System.IO.PathTooLongException
80070216 System.ArithmeticException
800703E6 System.BadImageFormatException
800703E9 System.StackOverflowException
800703ED System.IO.FileLoadException
800703EE System.IO.FileLoadException
80070459 System.ArgumentOutOfRangeException
8007045A System.IO.FileLoadException
80070482 System.BadImageFormatException
80070485 System.IO.FileNotFoundException
80070570 System.BadImageFormatException
80070571 System.IO.FileLoadException
80070574 System.IO.FileNotFoundException
800C0004 System.IO.FileNotFoundException
800C0005 System.IO.FileNotFoundException
800C0006 System.IO.FileNotFoundException
800C0007 System.IO.FileNotFoundException
800C0008 System.IO.FileNotFoundException
800C000B System.IO.FileNotFoundException
800C000D System.IO.FileNotFoundException
800A0006 System.OverflowException
800A0007 System.OutOfMemoryException
800A0009 System.IndexOutOfRangeException
800A000B System.DivideByZeroException
800A001C System.StackOverflowException
800A0035 System.IO.FileNotFoundException
800A0039 System.IO.IOException
800A003E System.IO.EndOfStreamException
800A0046 System.Security.SecurityException
800A004B System.UnauthorizedAccessException
800A004C System.IO.DirectoryNotFoundException
800A014F System.UnauthorizedAccessException
800A01A3 System.Security.SecurityException
800A01B6 System.NotSupportedException
800A01BD System.NotSupportedException
800A01C1 System.ArgumentException
800A01C2 System.ArgumentException
800A01CA System.NotSupportedException
800A01CB System.NotSupportedException
800A01CD System.MissingMemberException
800A7919 System.OutOfMemoryException
800A793C System.IO.IOException
800A793D System.IO.IOException
80131013 System.TypeUnloadedException
80131016 System.IO.FileLoadException
80131018 System.BadImageFormatException
8013101B System.BadImageFormatException
80131040 System.IO.FileLoadException
80131047 System.IO.FileLoadException
80131058 System.BadImageFormatException
8013106A System.Runtime.AmbiguousImplementationException
80131107 System.BadImageFormatException
8013110E System.BadImageFormatException
80131124 System.BadImageFormatException
80131192 System.BadImageFormatException
801311E6 System.MethodAccessException
8013141A System.Security.SecurityException
8013141D System.BadImageFormatException
8013141E System.Security.SecurityException
80131420 System.Security.SecurityException
80131430 System.Security.Cryptography.CryptographicException
80131500 System.Exception
80131501 System.SystemException
80131502 System.ArgumentOutOfRangeException
80131503 System.ArrayTypeMismatchException
80131506 System.ExecutionEngineException
80131507 System.FieldAccessException
80131508 System.IndexOutOfRangeException
80131509 System.InvalidOperationException
8013150A System.Security.SecurityException
8013150C System.Runtime.Serialization.SerializationException
8013150D System.Security.VerificationException
80131510 System.MethodAccessException
80131511 System.MissingFieldException
80131512 System.MissingMemberException
80131513 System.MissingMethodException
80131514 System.MulticastNotSupportedException
80131515 System.NotSupportedException
80131516 System.OverflowException
80131517 System.RankException
80131518 System.Threading.SynchronizationLockException
80131519 System.Threading.ThreadInterruptedException
8013151A System.MemberAccessException
80131520 System.Threading.ThreadStateException
80131522 System.TypeLoadException
80131523 System.EntryPointNotFoundException
80131524 System.DllNotFoundException
80131525 System.Threading.ThreadStartException
80131527 System.Runtime.InteropServices.InvalidComObjectException
80131528 System.NotFiniteNumberException
80131529 System.DuplicateWaitObjectException
80131530 System.Threading.ThreadAbortException
80131531 System.Runtime.InteropServices.InvalidOleVariantTypeException
80131532 System.Resources.MissingManifestResourceException
80131533 System.Runtime.InteropServices.SafeArrayTypeMismatchException
80131534 System.TypeInitializationException
80131535 System.Runtime.InteropServices.MarshalDirectiveException
80131537 System.FormatException
80131538 System.Runtime.InteropServices.SafeArrayRankMismatchException
80131539 System.PlatformNotSupportedException
8013153A System.InvalidProgramException
8013153B System.OperationCanceledException
8013153E System.MissingMethodException
80131541 System.DataMisalignedException
80131542 System.Diagnostics.Contracts.ContractException
80131543 System.TypeAccessException
80131578 System.InsufficientExecutionStackException
80131600 System.ApplicationException
80131601 System.Reflection.InvalidFilterCriteriaException
80131602 System.MissingMethodException
80131603 System.Reflection.TargetException
80131604 System.MissingMethodException
80131605 System.Reflection.CustomAttributeFormatException
80131620 System.IO.IOException
80131621 System.IO.FileLoadException
80131622 System.ObjectDisposedException
";

/// Facility prefixes the oracle swept in full (`hr = base | low16`); round 2 adds FACILITY_INTERNET (0x800C, the
/// INET_E_* codes .NET maps to FileNotFoundException).
const SWEPT_FACILITIES: [u32; 20] = [
    0x800C_0000,
    0x8000_0000,
    0x8001_0000,
    0x8002_0000,
    0x8003_0000,
    0x8004_0000,
    0x8007_0000,
    0x800A_0000,
    0x8013_0000,
    0x8898_0000,
    0x8899_0000,
    0xC000_0000,
    0xC007_0000,
    0x8008_0000,
    0x8009_0000,
    0x800B_0000,
    0x8010_0000,
    0x801F_0000,
    0x8DDD_0000,
    0x887A_0000,
];

#[test]
fn exception_kind_full_facility_sweep_matches_dotnet() {
    let mut expected = std::collections::HashMap::new();
    for line in DOTNET_SPECIFIC_EXCEPTION_TYPES.lines().filter(|l| !l.trim().is_empty()) {
        let (hr, name) = line.split_once(' ').expect("`HRESULT TYPE` line");
        expected.insert(u32::from_str_radix(hr, 16).expect("hex HRESULT"), name);
    }
    assert_eq!(expected.len(), 144);
    let mut bad = Vec::new();
    for base in SWEPT_FACILITIES {
        for low in 0..=0xFFFF_u32 {
            let hr = base | low;
            let want = expected
                .get(&hr)
                .copied()
                .unwrap_or("System.Runtime.InteropServices.COMException");
            let got = exception_kind_for_hresult(hr as i32).map(DotnetExceptionKind::type_name);
            if got != Some(want) {
                bad.push(format!("{hr:08X}: rust {got:?}, dotnet {want}"));
            }
        }
    }
    // Odd codes outside the swept facilities (oracle: all COMException).
    for hr in [
        0xFFFF_FFFF_u32,
        0xA007_0002,
        0x9007_0002,
        0xE007_0002,
        0xC000_0005,
        0x8000_000B,
    ] {
        if exception_kind_for_hresult(hr as i32) != Some(DotnetExceptionKind::Com) {
            bad.push(format!("{hr:08X}: expected COMException"));
        }
    }
    assert!(bad.is_empty(), "{} mismatches:\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn new_exception_kinds_follow_the_dotnet_base_types() {
    use DotnetExceptionKind as K;
    let kind = |hr: u32| exception_kind_for_hresult(hr as i32).expect("failing code");
    // DuplicateWaitObjectException : ArgumentException; NotFiniteNumberException : ArithmeticException.
    assert_eq!(kind(0x8013_1529), K::DuplicateWaitObject);
    assert!(K::DuplicateWaitObject.is_argument());
    assert_eq!(kind(0x8013_1528), K::NotFiniteNumber);
    assert!(K::NotFiniteNumber.is_arithmetic());
    // FileLoadException (ERROR_TOO_MANY_OPEN_FILES) and FileNotFoundException (ERROR_INVALID_NAME) are IOExceptions.
    assert_eq!(kind(0x8007_0004), K::FileLoad);
    assert!(K::FileLoad.is_io());
    assert_eq!(kind(0x8007_007B), K::FileNotFound);
    assert!(K::FileNotFound.is_io());
    // ERROR_NO_UNICODE_TRANSLATION is an ArgumentOutOfRangeException.
    assert_eq!(kind(0x8007_0459), K::ArgumentOutOfRange);
    assert!(K::ArgumentOutOfRange.is_argument());
    // None of the reflection/threading/interop types is an Argument/IO/NotSupported/InvalidOperation/Arithmetic one.
    for k in [
        K::TypeLoad,
        K::AmbiguousMatch,
        K::TargetParameterCount,
        K::TypeUnloaded,
        K::AmbiguousImplementation,
        K::MethodAccess,
        K::Cryptographic,
        K::ExecutionEngine,
        K::FieldAccess,
        K::Serialization,
        K::Verification,
        K::MissingField,
        K::MissingMember,
        K::MulticastNotSupported,
        K::Rank,
        K::SynchronizationLock,
        K::ThreadInterrupted,
        K::MemberAccess,
        K::ThreadState,
        K::ThreadStart,
        K::InvalidComObject,
        K::ThreadAbort,
        K::InvalidOleVariantType,
        K::MissingManifestResource,
        K::SafeArrayTypeMismatch,
        K::TypeInitialization,
        K::MarshalDirective,
        K::SafeArrayRankMismatch,
        K::InvalidProgram,
        K::DataMisaligned,
        K::Contract,
        K::TypeAccess,
        K::InsufficientExecutionStack,
        K::Application,
        K::InvalidFilterCriteria,
        K::Target,
        K::CustomAttributeFormat,
    ] {
        assert!(
            !(k.is_argument() || k.is_io() || k.is_not_supported() || k.is_invalid_operation() || k.is_arithmetic()),
            "{k:?}"
        );
    }
}

/// A temp file with known content, removed on drop (each test uses its own name).
struct TempFile(std::path::PathBuf);

impl TempFile {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("pr-w411a-{tag}-{}.bin", std::process::id()));
        std::fs::write(&path, [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10]).expect("write temp file");
        Self(path)
    }

    fn open_read(&self) -> std::fs::File {
        std::fs::File::open(&self.0).expect("open read-only")
    }

    fn open_write(&self) -> std::fs::File {
        std::fs::OpenOptions::new()
            .write(true)
            .open(&self.0)
            .expect("open write-only")
    }

    fn open_read_write(&self) -> std::fs::File {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.0)
            .expect("open read-write")
    }

    fn content(&self) -> Vec<u8> {
        std::fs::read(&self.0).expect("read temp file")
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn hr_of<T>(result: Result<T, StreamFault>) -> Result<T, i32> {
    result.map_err(|fault| fault.hresult())
}

#[test]
fn file_backend_reads_writes_seeks_and_reports_its_length() {
    let file = TempFile::new("rw-basic");
    let com = ComStream::from_file(file.open_read_write());
    let mut buf = [0u8; 4];
    assert_eq!(hr_of(com.read(&mut buf)), Ok(4));
    assert_eq!(buf, [1, 2, 3, 4]);
    assert_eq!(hr_of(com.seek(6, 0)), Ok(6));
    assert_eq!(hr_of(com.seek(-2, 1)), Ok(4));
    assert_eq!(hr_of(com.seek(-1, 2)), Ok(9));
    assert_eq!(com.stat().map(|s| s.cbSize).ok(), Some(10));
    assert_eq!(hr_of(com.seek(2, 0)), Ok(2));
    assert_eq!(hr_of(com.write(&[0xAA, 0xBB])), Ok(()));
    assert_eq!(hr_of(com.seek(0, 1)), Ok(4));
    assert_eq!(hr_of(com.commit()), Ok(()));
    assert_eq!(hr_of(com.set_size(12)), Ok(()));
    assert_eq!(com.stat().map(|s| s.cbSize).ok(), Some(12));
    // Through the vtable too: Stat and Read of the file.
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    let mut statstg = STATSTG::default();
    assert_eq!(vt_stat(raw, &mut statstg, 1).0, 0);
    assert_eq!(statstg.cbSize, 12);
    let mut byte = [0u8; 3];
    let mut pcb = SENTINEL_U32;
    assert_eq!(vt_read(raw, byte.as_mut_ptr().cast(), 3, &mut pcb).0, 0);
    assert_eq!((pcb, byte), (3, [5, 6, 7]));
    drop(stream);
    drop(com);
    assert_eq!(file.content(), vec![1, 2, 0xAA, 0xBB, 5, 6, 7, 8, 9, 10, 0, 0]);
}

// .NET FileStream.SetLength: the position is clamped to a shorter length and kept for a longer one.
#[test]
fn file_backend_set_len_clamps_or_keeps_the_position() {
    let file = TempFile::new("setlen");
    let com = ComStream::from_file(file.open_read_write());
    assert_eq!(hr_of(com.seek(8, 0)), Ok(8));
    assert_eq!(hr_of(com.set_size(3)), Ok(()));
    assert_eq!(hr_of(com.seek(0, 1)), Ok(3));
    assert_eq!(hr_of(com.seek(8, 0)), Ok(8));
    assert_eq!(hr_of(com.set_size(20)), Ok(()));
    assert_eq!(hr_of(com.seek(0, 1)), Ok(8));
    assert_eq!(hr_of(com.seek(i64::MAX, 0)), Ok(i64::MAX as u64));
    drop(com);
    assert_eq!(file.content().len(), 20);
}

// .NET FileStream.Seek before the beginning: IOException with HResult 0x80070057 (oracle), position unchanged.
#[test]
fn file_backend_seek_before_begin_has_the_filestream_hresult() {
    let file = TempFile::new("seek");
    let com = ComStream::from_file(file.open_read());
    assert_eq!(hr_of(com.seek(2, 0)), Ok(2));
    assert_eq!(hr_of(com.seek(-1, 0)), Err(E_INVALIDARG), "begin -1");
    assert_eq!(hr_of(com.seek(-5, 1)), Err(E_INVALIDARG), "current -5");
    assert_eq!(hr_of(com.seek(-20, 2)), Err(E_INVALIDARG), "end -20");
    assert_eq!(hr_of(com.seek(i64::MAX, 1)), Err(E_INVALIDARG), "current overflow");
    assert_eq!(hr_of(com.seek(0, 1)), Ok(2));
    // Through the vtable: the out position is not written on failure.
    let stream: IStream = com.istream();
    let mut position = SENTINEL_U64;
    assert_eq!(vt_seek(stream.as_raw(), -5, 1, &mut position).0, E_INVALIDARG);
    assert_eq!(position, SENTINEL_U64);
    // The memory backends keep the MemoryStream HRESULT (COR_E_IO).
    let memory = ComStream::from_bytes(Arc::from(DATA.as_slice()));
    assert_eq!(hr_of(memory.seek(-1, 0)), Err(COR_E_IO));
    assert_eq!(hr_of(memory.seek(-1, 1)), Err(COR_E_IO));
    assert_eq!(hr_of(memory.seek(-9, 2)), Err(COR_E_IO));
}

// .NET FileStream opened FileAccess.Read: Write / SetLength throw NotSupportedException, also for 0 bytes.
#[test]
fn read_only_file_write_and_set_size_are_not_supported() {
    let file = TempFile::new("ro");
    let com = ComStream::from_file(file.open_read());
    assert_eq!(hr_of(com.write(&[1, 2, 3])), Err(COR_E_NOTSUPPORTED), "write 3");
    assert_eq!(hr_of(com.write(&[])), Err(COR_E_NOTSUPPORTED), "write 0");
    assert_eq!(hr_of(com.set_size(3)), Err(COR_E_NOTSUPPORTED), "set_size 3");
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    let mut pcb = SENTINEL_U32;
    assert_eq!(vt_write(raw, [1u8].as_ptr().cast(), 1, &mut pcb).0, COR_E_NOTSUPPORTED);
    assert_eq!(vt_write(raw, [1u8].as_ptr().cast(), 0, &mut pcb).0, COR_E_NOTSUPPORTED);
    assert_eq!(pcb, SENTINEL_U32);
    assert_eq!(vt_set_size(raw, 3).0, COR_E_NOTSUPPORTED);
    drop(stream);
    drop(com);
    assert_eq!(file.content(), vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
}

// .NET FileStream opened FileAccess.Write: Read throws NotSupportedException, also for 0 bytes.
#[test]
fn write_only_file_read_is_not_supported() {
    let file = TempFile::new("wo");
    let com = ComStream::from_file(file.open_write());
    let mut buf = [0u8; 3];
    assert_eq!(hr_of(com.read(&mut buf)), Err(COR_E_NOTSUPPORTED));
    assert_eq!(hr_of(com.read(&mut [])), Err(COR_E_NOTSUPPORTED));
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    let mut pcb = SENTINEL_U32;
    assert_eq!(vt_read(raw, buf.as_mut_ptr().cast(), 3, &mut pcb).0, COR_E_NOTSUPPORTED);
    assert_eq!(vt_read(raw, buf.as_mut_ptr().cast(), 0, &mut pcb).0, COR_E_NOTSUPPORTED);
    assert_eq!(pcb, SENTINEL_U32);
    // Writing still works, and a zero-byte write on a writable file is S_OK.
    assert_eq!(hr_of(com.write(&[7])), Ok(()));
    assert_eq!(hr_of(com.write(&[])), Ok(()));
    drop(stream);
    drop(com);
    assert_eq!(file.content()[0], 7);
}

#[test]
fn zero_byte_read_of_a_readable_file_is_ok() {
    let file = TempFile::new("r0");
    let com = ComStream::from_file(file.open_read());
    assert_eq!(hr_of(com.read(&mut [])), Ok(0));
    let stream: IStream = com.istream();
    let mut pcb = SENTINEL_U32;
    let mut byte = 0u8;
    assert_eq!(
        vt_read(stream.as_raw(), (&mut byte as *mut u8).cast(), 0, &mut pcb).0,
        0
    );
    assert_eq!(pcb, 0);
}

// .NET Win32Marshal: ERROR_OPERATION_ABORTED (995) is an OperationCanceledException (0x8013153B); every other code
// is HRESULT_FROM_WIN32 (oracle checked 1..19999). OS error 0 has no failing HRESULT: COR_E_IO.
#[test]
fn io_fault_os_codes_map_like_filestream() {
    assert_eq!(
        StreamFault::Io(io::Error::from_raw_os_error(995)).hresult(),
        0x8013_153B_u32 as i32
    );
    assert_eq!(
        StreamFault::Io(io::Error::from_raw_os_error(994)).hresult(),
        0x8007_03E2_u32 as i32
    );
    assert_eq!(
        StreamFault::Io(io::Error::from_raw_os_error(996)).hresult(),
        0x8007_03E4_u32 as i32
    );
    assert_eq!(StreamFault::Io(io::Error::from_raw_os_error(0)).hresult(), COR_E_IO);
}

/// Records every backend call; `flush` fails with ERROR_WRITE_FAULT (29) when asked to.
struct Recorder {
    log: Arc<std::sync::Mutex<Vec<String>>>,
    fail_flush: bool,
}

impl Recorder {
    fn push(&self, entry: String) {
        self.log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(entry);
    }
}

impl StreamBackend for Recorder {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.push(format!("read {}", buf.len()));
        Ok(0)
    }
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.push(format!("write {}", data.len()));
        Ok(())
    }
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.push(format!("seek {pos:?}"));
        Ok(0)
    }
    fn set_len(&mut self, len: u64) -> io::Result<()> {
        self.push(format!("set_len {len}"));
        Ok(())
    }
    fn len(&mut self) -> io::Result<u64> {
        Ok(0)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.push("flush".to_owned());
        if self.fail_flush {
            Err(io::Error::from_raw_os_error(29))
        } else {
            Ok(())
        }
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any + Send> {
        self
    }
}

fn recorder(fail_flush: bool) -> (ComStream, Arc<std::sync::Mutex<Vec<String>>>) {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let com = ComStream::new(Box::new(Recorder {
        log: Arc::clone(&log),
        fail_flush,
    }));
    (com, log)
}

fn entries(log: &Arc<std::sync::Mutex<Vec<String>>>) -> Vec<String> {
    std::mem::take(&mut *log.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}

#[test]
fn commit_flushes_the_backend_and_reports_its_failure() {
    let (com, log) = recorder(false);
    assert_eq!(hr_of(com.commit()), Ok(()));
    let stream: IStream = com.istream();
    assert_eq!(vt_commit(stream.as_raw(), 0).0, 0);
    assert_eq!(entries(&log), ["flush", "flush"]);
    let (failing, log) = recorder(true);
    assert_eq!(hr_of(failing.commit()), Err(0x8007_001D_u32 as i32));
    let failing_stream: IStream = failing.istream();
    assert_eq!(vt_commit(failing_stream.as_raw(), 0).0, 0x8007_001D_u32 as i32);
    assert_eq!(entries(&log), ["flush", "flush"]);
    assert!(failing.take_last_fault().is_some(), "the failed COM Commit is recorded");
    drop(failing.dispose());
    assert_eq!(vt_commit(failing_stream.as_raw(), 0).0, 0, "S_OK after dispose");
}

#[test]
fn set_size_of_i64_max_reaches_the_backend_and_above_is_out_of_range() {
    let (com, log) = recorder(false);
    assert_eq!(hr_of(com.set_size(i64::MAX as u64)), Ok(()));
    assert_eq!(hr_of(com.set_size(i64::MAX as u64 + 1)), Err(COR_E_ARGUMENTOUTOFRANGE));
    let stream: IStream = com.istream();
    assert_eq!(vt_set_size(stream.as_raw(), i64::MAX as u64).0, 0);
    assert_eq!(vt_set_size(stream.as_raw(), u64::MAX).0, COR_E_ARGUMENTOUTOFRANGE);
    let want = format!("set_len {}", i64::MAX);
    assert_eq!(entries(&log), [want.clone(), want]);
}

#[test]
fn zero_byte_read_and_write_reach_the_backend() {
    let (com, log) = recorder(false);
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    let mut pcb = SENTINEL_U32;
    assert_eq!(vt_read(raw, ptr::null_mut(), 0, &mut pcb).0, 0);
    assert_eq!(pcb, 0);
    assert_eq!(vt_write(raw, ptr::null(), 0, &mut pcb).0, 0);
    assert_eq!(entries(&log), ["read 0", "write 0"]);
}

// `cb == i32::MAX` is a valid C# `int`: the call reaches the backend (2 GiB buffer, zero-filled lazily).
#[test]
fn read_and_write_with_count_i32_max_are_accepted() {
    let size = i32::MAX as usize;
    let Ok(layout) = std::alloc::Layout::from_size_align(size, 1) else {
        panic!("layout");
    };
    // SAFETY: `layout` has a non-zero size.
    let buffer = unsafe { std::alloc::alloc_zeroed(layout) };
    if buffer.is_null() {
        eprintln!("skipped: no 2 GiB of address space for the i32::MAX buffer");
        return;
    }
    let (com, log) = recorder(false);
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    let mut pcb = SENTINEL_U32;
    let read = vt_read(raw, buffer.cast(), i32::MAX as u32, &mut pcb).0;
    let write = vt_write(raw, buffer.cast_const().cast(), i32::MAX as u32, &mut pcb).0;
    // SAFETY: `buffer` was allocated above with `layout` and is no longer used.
    unsafe { std::alloc::dealloc(buffer, layout) };
    assert_eq!((read, write, pcb), (0, 0, i32::MAX as u32));
    assert_eq!(entries(&log), [format!("read {size}"), format!("write {size}")]);
}

#[test]
fn stream_backend_is_empty_follows_the_length() {
    let mut empty: Box<dyn StreamBackend> = Box::new(Cursor::new(Vec::<u8>::new()));
    assert_eq!(empty.is_empty().ok(), Some(true));
    let mut full: Box<dyn StreamBackend> = Box::new(Cursor::new(Arc::<[u8]>::from(DATA.as_slice())));
    assert_eq!(full.is_empty().ok(), Some(false));
    let file = TempFile::new("empty");
    let mut on_disk: Box<dyn StreamBackend> = Box::new(file.open_read());
    assert_eq!(on_disk.len().ok(), Some(10));
    assert_eq!(on_disk.is_empty().ok(), Some(false));
}

#[test]
fn com_as_raw_and_debug_show_the_wrapped_pointer() {
    let com = ComStream::writable_memory();
    let stream: IStream = com.istream();
    let raw = stream.as_raw();
    // SAFETY: `raw` is live (held by `stream`); the added reference is handed over to `wrap`.
    let wrapped = unsafe {
        add_ref_raw(raw);
        Com::<IStream>::wrap(raw, "IStream")
    }
    .expect("an IStream is an IStream");
    assert_eq!(wrapped.as_raw(), raw);
    let text = format!("{wrapped:?}");
    assert!(text.starts_with("Com") && text.contains("raw"), "{text}");
    assert!(format!("{com:?}").starts_with("ComStream"));
}

#[test]
fn stream_fault_display_and_source() {
    use std::error::Error as _;
    let faults = [
        StreamFault::Disposed,
        StreamFault::NotSupported,
        StreamFault::SeekBeforeBegin,
        StreamFault::InvalidSeekOrigin(7),
        StreamFault::SizeOutOfRange(u64::MAX),
        StreamFault::CountOutOfRange(u32::MAX),
        StreamFault::Io(io::Error::other("disk on fire")),
        StreamFault::SeekOutOfRange(i64::MIN),
    ];
    let texts: Vec<String> = faults.iter().map(ToString::to_string).collect();
    assert!(texts[0].contains("closed"));
    assert!(texts[1].contains("not supported"));
    assert!(texts[2].contains("before the beginning"));
    assert!(texts[3].contains('7'));
    assert!(texts[4].contains(&u64::MAX.to_string()));
    assert!(texts[5].contains(&u32::MAX.to_string()));
    assert!(texts[6].contains("disk on fire"));
    assert!(texts[7].contains(&i64::MIN.to_string()));
    for (fault, text) in faults.iter().zip(&texts) {
        assert_eq!(fault.source().is_some(), matches!(fault, StreamFault::Io(_)), "{text}");
    }
    assert_eq!(
        faults[6].source().map(ToString::to_string).as_deref(),
        Some("disk on fire")
    );
    assert_eq!(faults[7].hresult(), COR_E_ARGUMENTOUTOFRANGE);
}

// ---------------------------------------------------------------------------------------------------------------
// Repair round 2 (verifier findings): .NET `MemoryStream` int.MaxValue limits, `FileStream.SetLength` too big,
// INET_E_* -> FileNotFoundException (table above). Expected values from the .NET 10 oracle.
// ---------------------------------------------------------------------------------------------------------------

const FOUR: [u8; 4] = [1, 2, 3, 4];

/// Read-only memory stream over 4 bytes, position 1.
fn read_only_at_1() -> ComStream {
    let s = ComStream::from_bytes(Arc::from(&FOUR[..]));
    assert_eq!(s.seek(1, 0).ok(), Some(1));
    s
}

/// Growable memory stream holding 4 bytes, position 1.
fn growable_at_1() -> ComStream {
    let s = ComStream::writable_memory();
    assert!(s.write(&FOUR).is_ok());
    assert_eq!(s.seek(1, 0).ok(), Some(1));
    s
}

/// `MemoryStream.Seek`: a target above int.MaxValue is `ArgumentOutOfRangeException` (0x80131502), position kept;
/// exactly int.MaxValue is allowed.
#[test]
fn memory_seek_above_int_max_is_out_of_range_and_keeps_the_position() {
    for (name, s) in [("read-only", read_only_at_1()), ("growable", growable_at_1())] {
        for (offset, origin) in [
            (2_147_483_648_i64, 0_u32),
            (3_000_000_000, 0),
            (i64::MAX, 0),
            (2_147_483_647, 1),
            (4_294_967_296, 1),
            (i64::MAX, 1),
            (2_147_483_644, 2),
            (i64::MAX, 2),
        ] {
            assert_eq!(
                hr_of(s.seek(offset, origin)),
                Err(COR_E_ARGUMENTOUTOFRANGE),
                "{name} {offset} {origin}"
            );
            assert_eq!(s.seek(0, 1).ok(), Some(1), "{name}: position kept");
        }
        // The edges that are still in range.
        assert_eq!(s.seek(2_147_483_646, 1).ok(), Some(2_147_483_647), "{name}");
        assert_eq!(s.seek(2_147_483_643, 2).ok(), Some(2_147_483_647), "{name}");
        assert_eq!(s.seek(2_147_483_647, 0).ok(), Some(2_147_483_647), "{name}");
        // From int.MaxValue, any positive move is out of range; a negative one is fine.
        assert_eq!(hr_of(s.seek(1, 1)), Err(COR_E_ARGUMENTOUTOFRANGE), "{name}");
        assert_eq!(s.seek(-2_147_483_647, 1).ok(), Some(0), "{name}");
        // Before the beginning stays COR_E_IO.
        assert_eq!(hr_of(s.seek(-4_294_967_291, 1)), Err(COR_E_IO), "{name}");
        assert_eq!(hr_of(s.seek(i64::MIN, 1)), Err(COR_E_IO), "{name}");
        assert_eq!(hr_of(s.seek(-5, 2)), Err(COR_E_IO), "{name}");
        assert_eq!(s.seek(0, 1).ok(), Some(0), "{name}");
    }
}

#[test]
fn memory_seek_out_of_range_via_vtable() {
    let s = read_only_at_1();
    let stream: IStream = s.istream();
    let mut new_pos = SENTINEL_U64;
    assert_eq!(
        vt_seek(stream.as_raw(), 2_147_483_648, 0, &mut new_pos).0,
        COR_E_ARGUMENTOUTOFRANGE
    );
    assert_eq!(new_pos, SENTINEL_U64, "not written on failure");
    assert!(matches!(
        s.take_last_fault(),
        Some(StreamFault::SeekOutOfRange(2_147_483_648))
    ));
}

/// `MemoryStream.SetLength`: the range check (> int.MaxValue) comes before the writable check.
#[test]
fn memory_set_size_above_int_max_is_out_of_range_before_not_supported() {
    let ro = read_only_at_1();
    assert_eq!(hr_of(ro.set_size(2_147_483_648)), Err(COR_E_ARGUMENTOUTOFRANGE));
    assert_eq!(hr_of(ro.set_size(i64::MAX as u64)), Err(COR_E_ARGUMENTOUTOFRANGE));
    assert_eq!(hr_of(ro.set_size(2_147_483_647)), Err(COR_E_NOTSUPPORTED));
    assert_eq!(hr_of(ro.set_size(0)), Err(COR_E_NOTSUPPORTED));
    let grow = growable_at_1();
    assert_eq!(hr_of(grow.set_size(2_147_483_648)), Err(COR_E_ARGUMENTOUTOFRANGE));
    assert_eq!(hr_of(grow.set_size(1 << 40)), Err(COR_E_ARGUMENTOUTOFRANGE));
    assert_eq!(
        grow.stat().map(|st| st.cbSize).ok(),
        Some(4),
        "nothing allocated or changed"
    );
    assert_eq!(grow.seek(0, 1).ok(), Some(1));
    assert!(grow.take_last_fault().is_none(), "safe calls do not record");
    let stream: IStream = grow.istream();
    assert_eq!(vt_set_size(stream.as_raw(), 2_147_483_648).0, COR_E_ARGUMENTOUTOFRANGE);
    assert!(matches!(
        grow.take_last_fault(),
        Some(StreamFault::SizeOutOfRange(2_147_483_648))
    ));
    // In range still works (shrink keeps the position rule).
    assert_eq!(hr_of(grow.set_size(2)), Ok(()));
    assert_eq!(grow.stat().map(|st| st.cbSize).ok(), Some(2));
}

/// `MemoryStream.Write`: `position + count` above int.MaxValue is `IOException` "Stream was too long" (COR_E_IO),
/// length and position unchanged, nothing allocated.
#[test]
fn growable_write_past_int_max_is_io_without_allocating() {
    let s = growable_at_1();
    assert_eq!(s.seek(2_147_483_647, 0).ok(), Some(2_147_483_647));
    assert_eq!(hr_of(s.write(&[1])), Err(COR_E_IO));
    assert_eq!(s.stat().map(|st| st.cbSize).ok(), Some(4));
    assert_eq!(s.seek(0, 1).ok(), Some(2_147_483_647));
    assert_eq!(s.seek(2_147_483_000, 0).ok(), Some(2_147_483_000));
    assert_eq!(hr_of(s.write(&[0u8; 648])), Err(COR_E_IO), "one byte past int.MaxValue");
    assert_eq!(s.stat().map(|st| st.cbSize).ok(), Some(4));
    assert_eq!(s.seek(2_147_483_645, 0).ok(), Some(2_147_483_645));
    let stream: IStream = s.istream();
    let mut pcb = SENTINEL_U32;
    assert_eq!(vt_write(stream.as_raw(), FOUR.as_ptr().cast(), 4, &mut pcb).0, COR_E_IO);
    assert_eq!(pcb, SENTINEL_U32);
    assert_eq!(s.stat().map(|st| st.cbSize).ok(), Some(4));
}

/// `MemoryStream.Write` past the end zero-fills the gap; a 0-byte write past the end still sets the length.
#[test]
fn growable_write_past_the_end_zero_fills_and_sets_the_length() {
    let s = growable_at_1();
    assert_eq!(s.seek(6, 0).ok(), Some(6));
    assert_eq!(hr_of(s.write(&[9, 9])), Ok(()));
    assert_eq!(s.seek(0, 1).ok(), Some(8));
    assert_eq!(s.seek(10, 0).ok(), Some(10));
    assert_eq!(hr_of(s.write(&[])), Ok(()));
    assert_eq!(s.stat().map(|st| st.cbSize).ok(), Some(10));
    assert_eq!(s.seek(1, 0).ok(), Some(1));
    assert_eq!(hr_of(s.write(&[7])), Ok(()), "overwrite inside");
    let Some(backend) = s.dispose() else {
        panic!("backend");
    };
    let Ok(cursor) = backend.into_any().downcast::<Cursor<Vec<u8>>>() else {
        panic!("Cursor<Vec<u8>>");
    };
    assert_eq!(cursor.into_inner(), [1, 7, 3, 4, 0, 0, 9, 9, 0, 0]);
}

/// .NET `FileStream.SetLength` too big for NTFS: `ArgumentOutOfRangeException` (RandomAccess.SetFileLength maps
/// ERROR_INVALID_PARAMETER); huge Seek Current/End stays `0x80070057`, position kept.
#[test]
fn file_set_size_too_big_is_out_of_range_and_huge_seek_is_invalid_parameter() {
    let file = TempFile::new("too-big");
    let s = ComStream::from_file(file.open_read_write());
    for size in [1u64 << 44, 1u64 << 62, i64::MAX as u64] {
        assert_eq!(hr_of(s.set_size(size)), Err(COR_E_ARGUMENTOUTOFRANGE), "{size}");
        assert!(s.take_last_fault().is_none(), "safe calls do not record");
    }
    assert_eq!(s.seek(5, 0).ok(), Some(5));
    assert_eq!(hr_of(s.seek(i64::MAX, 1)), Err(E_INVALIDARG));
    assert_eq!(hr_of(s.seek(i64::MAX, 2)), Err(E_INVALIDARG));
    assert_eq!(s.seek(0, 1).ok(), Some(5));
    assert_eq!(s.stat().map(|st| st.cbSize).ok(), Some(10));
    drop(s);
    assert_eq!(file.content(), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
}

#[test]
fn inet_codes_are_file_not_found() {
    for hr in [
        0x800C_0004_u32,
        0x800C_0005,
        0x800C_0006,
        0x800C_0007,
        0x800C_0008,
        0x800C_000B,
        0x800C_000D,
    ] {
        assert_eq!(
            exception_kind_for_hresult(hr as i32),
            Some(DotnetExceptionKind::FileNotFound),
            "{hr:08X}"
        );
    }
    for hr in [0x800C_0003_u32, 0x800C_0009, 0x800C_000A, 0x800C_000C, 0x800C_000E] {
        assert_eq!(
            exception_kind_for_hresult(hr as i32),
            Some(DotnetExceptionKind::Com),
            "{hr:08X}"
        );
    }
}
