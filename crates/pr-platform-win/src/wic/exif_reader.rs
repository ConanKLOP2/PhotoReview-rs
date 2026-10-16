//! Port of `PhotoReview.Imaging/Decoding/Wic/WicExifReader.cs` (C# baseline d92d6aae): EXIF orientation and the
//! photo-information fields read through the WIC metadata query reader of a frame the caller already opened.
//! PARITY: I-18 (orientation baked into pixels), S-30 (the EXIF line is never an extra read). See
//! contracts/W4-11d.md.
//!
//! Layering. Everything that decides something is pure and `unsafe`-free:
//! * [`decode_variant`] = C# `ReadVariant`: a 24-byte PROPVARIANT snapshot to the [`QueryValue`] shape WPF's
//!   `BitmapMetadata.GetQuery` returned (so `pr_imaging::exif_query` serves both paths);
//! * [`read_metadata_values`] = C# `ReadMetadataValues`: orientation and photo information are independent;
//! * [`exif_ifd_root_for_container`] = the GUID switch of C# `ExifIfdRootOf`.
//!
//! Only the native seams are `unsafe` (one `// SAFETY:` per block): [`RawMemory`] (raw reads behind a PROPVARIANT)
//! and [`PropVariant::clear`] (`PropVariantClear`). The frame-level wrappers [`read_frame_metadata`] /
//! [`read_orientation`] / [`exif_ifd_root_of`] work over the small traits [`FrameMetadata`], [`MetadataQueryReader`],
//! [`DecoderContainer`]; the W4-11a/W4-11c adapters implement those traits.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, c_char};

use pr_core::catalog::explorer_types::Guid;
use pr_imaging::exif_query::{self, JPEG_IFD_ROOT, QueryValue, TIFF_IFD_ROOT};
use pr_imaging::exif_summary::ExifSummary;
use pr_imaging::orientation::{self, QueryFailed};

/// `GUID_ContainerFormatJpeg` (wincodec.h) `19e4a5aa-5662-4fc5-a0c0-1758028e1057`, in .NET `Guid.ToByteArray()` order.
pub const CONTAINER_FORMAT_JPEG: Guid = Guid([
    0xaa, 0xa5, 0xe4, 0x19, 0x62, 0x56, 0xc5, 0x4f, 0xa0, 0xc0, 0x17, 0x58, 0x02, 0x8e, 0x10, 0x57,
]);

/// `GUID_ContainerFormatTiff` (wincodec.h) `163bcc30-e2e9-4f0b-961d-a3e9fdb788a3`, in .NET `Guid.ToByteArray()` order.
pub const CONTAINER_FORMAT_TIFF: Guid = Guid([
    0x30, 0xcc, 0x3b, 0x16, 0xe9, 0xe2, 0x0b, 0x4f, 0x96, 0x1d, 0xa3, 0xe9, 0xfd, 0xb7, 0x88, 0xa3,
]);

/// `sizeof(PROPVARIANT)` on x64 (C# `PropVariantSize`).
pub const PROP_VARIANT_SIZE: usize = 24;
/// Offset of the value union inside a PROPVARIANT (after `vt` and three reserved WORDs).
pub const PROP_VARIANT_DATA_OFFSET: usize = 8;
/// `VT_VECTOR` flag (`0x1000`).
pub const VT_VECTOR: u16 = 0x1000;

const VT_I2: u16 = 2;
const VT_I4: u16 = 3;
const VT_UI1: u16 = 17;
const VT_UI2: u16 = 18;
const VT_UI4: u16 = 19;
const VT_I8: u16 = 20;
const VT_UI8: u16 = 21;
const VT_LPSTR: u16 = 30;
const VT_LPWSTR: u16 = 31;

// `raw-dylib`: no import library needed.
#[link(name = "ole32", kind = "raw-dylib")]
unsafe extern "system" {
    /// `PropVariantClear` (ole32): frees what the PROPVARIANT owns and sets it to `VT_EMPTY`.
    fn PropVariantClear(pvar: *mut u8) -> i32;
}

/// An HRESULT. Negative = failure (`FAILED`). The W4-11a wrappers define the shared alias; keep in sync.
pub type HResult = i32;

/// The 24-byte PROPVARIANT buffer WIC fills for one query (C# `Marshal.AllocHGlobal(24)` + `ZeroPropVariant`).
/// 8-byte aligned (WIC writes pointers and `ULONGLONG`s into it). Owned by the caller of the query.
#[repr(C, align(8))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropVariant {
    bytes: [u8; PROP_VARIANT_SIZE],
}

impl PropVariant {
    /// All 24 bytes zero (`VT_EMPTY`): the state the buffer must be in before every query.
    pub const fn zeroed() -> Self {
        Self {
            bytes: [0; PROP_VARIANT_SIZE],
        }
    }

    /// Wraps a snapshot (tests and the golden harness build variants from bytes).
    pub const fn from_bytes(bytes: [u8; PROP_VARIANT_SIZE]) -> Self {
        Self { bytes }
    }

    /// The raw 24 bytes.
    pub const fn as_bytes(&self) -> &[u8; PROP_VARIANT_SIZE] {
        &self.bytes
    }

    /// Pointer to hand to `IWICMetadataQueryReader::GetMetadataByName` (`pvarValue`).
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.bytes.as_mut_ptr()
    }

    /// `PropVariantClear(pvar)` (ole32) and then zero the 24 bytes again (C# `QueryValue`'s `finally`). Idempotent on a
    /// zeroed buffer. The return HRESULT of the C# call is ignored.
    pub fn clear(&mut self) {
        // SAFETY: `self.bytes` is a live, 8-byte aligned, 24-byte buffer (the size of a PROPVARIANT); it is either all
        // zero (`VT_EMPTY`) or was filled by WIC through `as_mut_ptr`, so `PropVariantClear` may free what it owns.
        let _ = unsafe { PropVariantClear(self.as_mut_ptr()) };
        self.bytes = [0; PROP_VARIANT_SIZE];
    }
}

/// Memory that the pointers inside a PROPVARIANT refer to (string text, vector elements). A trait so the decoding
/// stays pure and testable: golden/unit tests answer from a `HashMap<usize, Vec<u8>>`, [`RawMemory`] answers from the
/// process address space.
pub trait VariantMemory {
    /// `Marshal.PtrToStringUTF8(addr)`: bytes up to the first NUL decoded as UTF-8, invalid sequences replaced by
    /// U+FFFD (maximal-subpart rule, same as `String::from_utf8_lossy`). Never called with 0.
    fn utf8_z(&self, addr: usize) -> String;
    /// `Marshal.PtrToStringUni(addr)`: UTF-16 code units up to the first NUL (lone surrogates become U+FFFD:
    /// accepted D5 divergence, see contract trap 5). Never called with 0.
    fn utf16_z(&self, addr: usize) -> String;
    /// `Marshal.ReadInt16(addr)` as `u16` (the element read of a `VT_UI2` vector).
    fn u16_at(&self, addr: usize) -> u16;
    /// `Marshal.ReadInt32(addr)` as `u32`.
    fn u32_at(&self, addr: usize) -> u32;
    /// `Marshal.ReadInt64(addr)` as `u64`.
    fn u64_at(&self, addr: usize) -> u64;
}

/// [`VariantMemory`] over the real address space. Only valid for pointers that WIC wrote into a PROPVARIANT that has
/// not been cleared yet.
#[derive(Debug)]
pub struct RawMemory {
    _private: (),
}

impl RawMemory {
    /// # Safety
    /// The caller guarantees that every address later passed to the trait methods came from a live (not yet cleared)
    /// PROPVARIANT filled by WIC: NUL-terminated text for the two string reads, and at least the element size in
    /// bytes for the element reads.
    pub unsafe fn new() -> Self {
        Self { _private: () }
    }
}

impl VariantMemory for RawMemory {
    fn utf8_z(&self, addr: usize) -> String {
        // SAFETY: `RawMemory::new` guarantees `addr` is a live, NUL-terminated `VT_LPSTR` text of a PROPVARIANT.
        let text = unsafe { CStr::from_ptr(addr as *const c_char) };
        String::from_utf8_lossy(text.to_bytes()).into_owned()
    }

    fn utf16_z(&self, addr: usize) -> String {
        let base = addr as *const u16;
        let mut units: Vec<u16> = Vec::new();
        loop {
            // SAFETY: `RawMemory::new` guarantees `addr` is a live, NUL-terminated UTF-16 text; the walk stops at the
            // terminator, so every unit read lies inside it (unaligned read: WIC does not promise alignment).
            let unit = unsafe { base.add(units.len()).read_unaligned() };
            if unit == 0 {
                break;
            }
            units.push(unit);
        }
        // DIVERGENCE: C# `PtrToStringUni` keeps a lone surrogate, `from_utf16_lossy` gives U+FFFD (contract Open
        // question 3; the persisted cache bytes are the same because the C# codec emits U+FFFD too).
        String::from_utf16_lossy(&units)
    }

    fn u16_at(&self, addr: usize) -> u16 {
        // SAFETY: `RawMemory::new` guarantees `addr` points at an element of at least 2 readable bytes.
        unsafe { (addr as *const u16).read_unaligned() }
    }

    fn u32_at(&self, addr: usize) -> u32 {
        // SAFETY: `RawMemory::new` guarantees `addr` points at an element of at least 4 readable bytes.
        unsafe { (addr as *const u32).read_unaligned() }
    }

    fn u64_at(&self, addr: usize) -> u64 {
        // SAFETY: `RawMemory::new` guarantees `addr` points at an element of at least 8 readable bytes.
        unsafe { (addr as *const u64).read_unaligned() }
    }
}

/// C# `ReadVariant(IntPtr pvar)`: PROPVARIANT to the managed shapes WPF's `BitmapMetadata.GetQuery` returns.
///
/// `vt` = the first `u16` (little endian) of `raw`; the value union starts at byte 8. Table (anything else is `None`):
///
/// | vt | name | result |
/// |---|---|---|
/// | 2 | VT_I2 | `I16` |
/// | 3 | VT_I4 | `I32` |
/// | 17 | VT_UI1 | `U8` |
/// | 18 | VT_UI2 | `U16` |
/// | 19 | VT_UI4 | `U32` |
/// | 20 | VT_I8 (SRATIONAL) | `I64` |
/// | 21 | VT_UI8 (RATIONAL) | `U64` |
/// | 30 | VT_LPSTR | `Text` of `utf8_z(ptr at 8)`; pointer 0 gives `None` (empty text stays `Some("")`) |
/// | 31 | VT_LPWSTR | `Text` of `utf16_z(ptr at 8)`; pointer 0 gives `None` |
/// | 0x1000 \| 18 | UI2 vector | `U16s([first])` |
/// | 0x1000 \| 19 | UI4 vector | `U32s([first])` |
/// | 0x1000 \| 20 | I8 vector | `I64s([first])` |
/// | 0x1000 \| 21 | UI8 vector | `U64s([first])` |
///
/// Vector layout: `count: i32` at byte 8, element pointer (`usize`) at byte `8 + size_of::<usize>()`. `None` when
/// `count <= 0` (signed!) or the pointer is 0. Only the first element is read. Other vector element types
/// (`VT_UI1`, `VT_LPSTR`, ...) and any value with other flag bits (`VT_ARRAY`) are `None`.
pub fn decode_variant(raw: &[u8; PROP_VARIANT_SIZE], memory: &dyn VariantMemory) -> Option<QueryValue> {
    const POINTER_WIDTH: usize = std::mem::size_of::<usize>();
    let vt = u16::from_le_bytes([raw[0], raw[1]]);
    let at = PROP_VARIANT_DATA_OFFSET;
    let read_pointer = |offset: usize| -> usize {
        let mut bytes = [0u8; POINTER_WIDTH];
        bytes.copy_from_slice(&raw[offset..offset + POINTER_WIDTH]);
        usize::from_le_bytes(bytes)
    };
    let read_u64 = || -> u64 {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&raw[at..at + 8]);
        u64::from_le_bytes(bytes)
    };
    let read_i32 = || i32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]]);
    match vt {
        VT_I2 => Some(QueryValue::I16(i16::from_le_bytes([raw[at], raw[at + 1]]))),
        VT_I4 => Some(QueryValue::I32(read_i32())),
        VT_UI1 => Some(QueryValue::U8(raw[at])),
        VT_UI2 => Some(QueryValue::U16(u16::from_le_bytes([raw[at], raw[at + 1]]))),
        VT_UI4 => Some(QueryValue::U32(u32::from_le_bytes([
            raw[at],
            raw[at + 1],
            raw[at + 2],
            raw[at + 3],
        ]))),
        // intentional `as i64`: the same 64 bits read as signed (SRATIONAL), `exif_query::as_rational` tells them apart.
        VT_I8 => Some(QueryValue::I64(read_u64() as i64)),
        VT_UI8 => Some(QueryValue::U64(read_u64())),
        VT_LPSTR => match read_pointer(at) {
            0 => None,
            addr => Some(QueryValue::Text(memory.utf8_z(addr))),
        },
        VT_LPWSTR => match read_pointer(at) {
            0 => None,
            addr => Some(QueryValue::Text(memory.utf16_z(addr))),
        },
        v if v == (VT_VECTOR | VT_UI2)
            || v == (VT_VECTOR | VT_UI4)
            || v == (VT_VECTOR | VT_I8)
            || v == (VT_VECTOR | VT_UI8) =>
        {
            // The element count is a SIGNED int32 (C# `Marshal.ReadInt32`): zero and negative give `None`.
            if read_i32() <= 0 {
                return None;
            }
            let addr = read_pointer(at + POINTER_WIDTH);
            if addr == 0 {
                return None;
            }
            Some(match v & !VT_VECTOR {
                VT_UI2 => QueryValue::U16s(vec![memory.u16_at(addr)]),
                VT_UI4 => QueryValue::U32s(vec![memory.u32_at(addr)]),
                // intentional `as i64`: the same 64 bits read as signed.
                VT_I8 => QueryValue::I64s(vec![memory.u64_at(addr) as i64]),
                _ => QueryValue::U64s(vec![memory.u64_at(addr)]),
            })
        }
        _ => None,
    }
}

/// C# `ExifIfdRootOf` after the `GetContainerFormat` call: JPEG gives `exif_query::JPEG_IFD_ROOT`, TIFF gives
/// `exif_query::TIFF_IFD_ROOT`, every other container `None` (no EXIF read at all).
pub fn exif_ifd_root_for_container(container: &Guid) -> Option<&'static str> {
    if *container == CONTAINER_FORMAT_JPEG {
        Some(JPEG_IFD_ROOT)
    } else if *container == CONTAINER_FORMAT_TIFF {
        Some(TIFF_IFD_ROOT)
    } else {
        None
    }
}

/// What one frame read produced (C# return value plus `out ExifSummary? exif`).
#[derive(Debug, Clone, PartialEq)]
pub struct MetadataRead {
    /// 1..=8; 1 when not requested, absent, invalid or unreadable.
    pub orientation: i32,
    /// `None` when `exif_ifd_root` was `None`, nothing usable was found, or a query failed.
    pub exif: Option<ExifSummary>,
}

/// C# `ReadMetadataValues(Func<string, object?> query, bool readOrientation, string? exifIfdRoot, out exif)`.
///
/// `query(name)`: `Ok(Some(v))` = value, `Ok(None)` = absent/unreadable (C# `null`), `Err(QueryFailed)` = the C# query
/// delegate threw. Behaviour, in this order (the order is observable through a recording `query`):
/// 1. `read_orientation`: `orientation::read_from_wic` with `exif_query::as_integer` on each answer: the EXIF tag
///    `/app1/ifd/{ushort=274}` first, `System.Photo.Orientation` only when that value is not in 1..=8. When not
///    requested, orientation is 1 and the orientation paths are never queried.
/// 2. `exif_ifd_root` is `Some(root)`: `exif_query::read(&adapter, root)`. The FIRST `Err` aborts the read
///    (no further `query` call, like a C# throw) and gives `exif: None`. Orientation is unaffected: a failure in one
///    half never discards the other.
///
/// Never panics for a query fault.
pub fn read_metadata_values<Q>(mut query: Q, read_orientation: bool, exif_ifd_root: Option<&str>) -> MetadataRead
where
    Q: FnMut(&str) -> Result<Option<QueryValue>, QueryFailed>,
{
    // The orientation half. A query failure gives 1 (the `catch` of `ReadFromWic`) and the EXIF half still runs.
    let orientation = if read_orientation {
        orientation::read_from_wic(|name| query(name).map(|value| exif_query::as_integer(value.as_ref())))
    } else {
        1
    };

    // The EXIF half. `exif_query::read` has no failure channel: the first `Err` is remembered and every later query is
    // answered `None` WITHOUT calling `query` (a C# throw aborts the interpreter), then the whole summary is dropped.
    let exif = match exif_ifd_root {
        None => None,
        Some(root) => {
            let query = RefCell::new(query);
            let failed = Cell::new(false);
            let adapter = |name: &str| -> Option<QueryValue> {
                if failed.get() {
                    return None;
                }
                let answer = (*query.borrow_mut())(name);
                match answer {
                    Ok(value) => value,
                    Err(QueryFailed) => {
                        failed.set(true);
                        None
                    }
                }
            };
            let summary = exif_query::read(&adapter, root);
            if failed.get() { None } else { summary }
        }
    };

    MetadataRead { orientation, exif }
}

// ---------------------------------------------------------------------------------------------------------------
// Native seams (implemented by the W4-11a wrappers; the three functions below are the only entry points that touch COM)
// ---------------------------------------------------------------------------------------------------------------

/// `IWICMetadataQueryReader` as far as this file needs it.
pub trait MetadataQueryReader {
    /// `GetMetadataByName(name, pvarValue)` (PreserveSig: the raw HRESULT, a missing tag is a normal negative
    /// `WINCODEC_ERR_PROPERTYNOTFOUND`). The implementation encodes `name` as NUL-terminated UTF-16 and passes
    /// `value.as_mut_ptr()`.
    fn get_metadata_by_name(&self, name: &str, value: &mut PropVariant) -> HResult;
}

/// `IWICBitmapFrameDecode.GetMetadataQueryReader` as far as this file needs it.
pub trait FrameMetadata {
    /// The reader type; released (COM `Release`) by `Drop`.
    type Reader: MetadataQueryReader;
    /// `Err(hr)` when the frame has no metadata reader (or the call failed).
    fn metadata_query_reader(&self) -> Result<Self::Reader, HResult>;
}

/// `IWICBitmapDecoder.GetContainerFormat` as far as this file needs it.
pub trait DecoderContainer {
    /// `Err(hr)` when the call failed.
    fn container_format(&self) -> Result<Guid, HResult>;
}

/// C# `ExifIfdRootOf(IWICBitmapDecoder)`: [`exif_ifd_root_for_container`] of the decoder's container; a failing
/// `GetContainerFormat` gives `None` (the C# `catch (COMException)`).
pub fn exif_ifd_root_of<D: DecoderContainer>(decoder: &D) -> Option<&'static str> {
    decoder
        .container_format()
        .ok()
        .and_then(|container| exif_ifd_root_for_container(&container))
}

/// C# `ReadFrameMetadata(frame, readOrientation, exifIfdRoot, out exif)`: opens ONE metadata query reader of the
/// frame, owns one zeroed [`PropVariant`], answers every query through it (`GetMetadataByName`; a negative HRESULT is
/// `None`, otherwise [`decode_variant`] with [`RawMemory`]; [`PropVariant::clear`] after EVERY query, success or
/// not), then calls [`read_metadata_values`]. A frame without a metadata reader gives orientation 1 and
/// `exif: None` whatever was requested. Never fails.
pub fn read_frame_metadata<F: FrameMetadata>(
    frame: &F,
    read_orientation: bool,
    exif_ifd_root: Option<&str>,
) -> MetadataRead {
    // C# catch-all: a frame without a metadata reader is orientation 1 and no EXIF, whatever was requested.
    let Ok(reader) = frame.metadata_query_reader() else {
        return MetadataRead {
            orientation: 1,
            exif: None,
        };
    };
    let mut variant = PropVariant::zeroed();
    let result = read_metadata_values(
        |name| {
            let hr = reader.get_metadata_by_name(name, &mut variant);
            let value = if hr < 0 {
                None
            } else {
                // SAFETY: the addresses inside `variant` come only from the query that just succeeded and are decoded
                // BEFORE `clear` below; the decoded `QueryValue` owns copies, so nothing outlives the buffer.
                let memory = unsafe { RawMemory::new() };
                decode_variant(variant.as_bytes(), &memory)
            };
            // The C# `finally`: clear after every query, which also zeroes the buffer for the next one.
            variant.clear();
            Ok(value)
        },
        read_orientation,
        exif_ifd_root,
    );
    variant.clear();
    drop(reader);
    result
}

/// C# `ReadOrientation(frame)`: `read_frame_metadata(frame, true, None).orientation`.
pub fn read_orientation<F: FrameMetadata>(frame: &F) -> i32 {
    read_frame_metadata(frame, true, None).orientation
}
