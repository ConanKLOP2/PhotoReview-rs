//! Port of `PhotoReview.Imaging/Decoding/Wic/WicCodecAvailability.cs` (C# baseline d92d6aae): which optional Windows
//! codecs this PC has for the WIC-decoded extra formats (Q-FMT-WEBP-HEIC). PARITY: see contracts/W4-11b.md.
//!
//! The decision logic ([`probe`], [`WicCodecSupport`]) is pure and has no `unsafe`. Only the two native seams in
//! [`native`] (`IWICImagingFactory::CreateComponentEnumerator` walk and `MFTEnumEx`) are `unsafe`; they are written
//! directly against the `windows` crate (one `// SAFETY:` per block).

use std::io;
use std::sync::OnceLock;

use pr_core::catalog::explorer_types::Guid;

/// `GUID_ContainerFormatWebp` (wincodec.h) `e094b0e2-67f2-45b3-b0ea-115337ca7cf3`, in .NET `Guid.ToByteArray()` order.
pub const CONTAINER_FORMAT_WEBP: Guid = Guid([
    0xe2, 0xb0, 0x94, 0xe0, 0xf2, 0x67, 0xb3, 0x45, 0xb0, 0xea, 0x11, 0x53, 0x37, 0xca, 0x7c, 0xf3,
]);

/// `GUID_ContainerFormatHeif` (wincodec.h) `e1e62521-6787-405b-a339-500715b5763f`.
pub const CONTAINER_FORMAT_HEIF: Guid = Guid([
    0x21, 0x25, 0xe6, 0xe1, 0x87, 0x67, 0x5b, 0x40, 0xa3, 0x39, 0x50, 0x07, 0x15, 0xb5, 0x76, 0x3f,
]);

/// `MFVideoFormat_HEVC` (`'HEVC'` FourCC subtype) `43564548-0000-0010-8000-00aa00389b71`.
pub const MEDIA_SUBTYPE_HEVC: Guid = Guid([
    0x48, 0x45, 0x56, 0x43, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
]);

/// `MFT_CATEGORY_VIDEO_DECODER` `d6c02d4b-6833-45b4-971a-05a4b04bab91`.
pub const MFT_CATEGORY_VIDEO_DECODER: Guid = Guid([
    0x4b, 0x2d, 0xc0, 0xd6, 0x33, 0x68, 0xb4, 0x45, 0x97, 0x1a, 0x05, 0xa4, 0xb0, 0x4b, 0xab, 0x91,
]);

/// `MFMediaType_Video` `73646976-0000-0010-8000-00aa00389b71`.
pub const MEDIA_TYPE_VIDEO: Guid = Guid([
    0x76, 0x69, 0x64, 0x73, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
]);

/// `MFT_ENUM_FLAG_SYNCMFT | ASYNCMFT | HARDWARE | LOCALMFT | SORTANDFILTER`: every decoder an app could actually use.
pub const MFT_ENUM_FLAGS: u32 = 0x1 | 0x2 | 0x4 | 0x10 | 0x40;

/// `WICDecoder` component type (`WICComponentType`).
pub const WIC_DECODER_COMPONENT: u32 = 0x1;
/// `WICComponentEnumerateDefault`.
pub const WIC_COMPONENT_ENUMERATE_DEFAULT: u32 = 0x0;

// D35: one home for the probe result, `pr_imaging::webp_heic` (the pure router needs it; pr-platform-win depends on pr-imaging).
pub use pr_imaging::webp_heic::WicCodecSupport;

/// C# `DecoderEntry`: one registered WIC decoder (container format plus, for diagnostics, name and extensions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderEntry {
    pub container_format: Guid,
    pub friendly_name: String,
    pub file_extensions: String,
}

/// C# `Probe(enumerateDecoders, hasVideoDecoder)`: the pure decision. Never fails: an error from either closure means
/// "not available" and is folded into `detail`. `has_video_decoder` is called exactly once, with
/// [`MEDIA_SUBTYPE_HEVC`], and also when no HEIF container decoder was listed (C# asks unconditionally).
/// An `io::Error` here stands for any C# exception except `OutOfMemoryException` (which propagates there; in Rust
/// allocation failure aborts, so there is no equivalent).
pub fn probe<E, H>(enumerate_decoders: E, has_video_decoder: H) -> WicCodecSupport
where
    E: FnOnce() -> io::Result<Vec<DecoderEntry>>,
    H: FnOnce(&Guid) -> io::Result<bool>,
{
    let decoders = match enumerate_decoders() {
        Ok(list) => list,
        Err(e) => return WicCodecSupport::none(format!("WIC decoder enumeration failed: {e}")),
    };
    let webp = decoders.iter().any(|d| d.container_format == CONTAINER_FORMAT_WEBP);
    let heif_container = decoders.iter().any(|d| d.container_format == CONTAINER_FORMAT_HEIF);

    // Asked unconditionally (parity with C#), exactly once.
    let (hevc_decoder, hevc_text) = match has_video_decoder(&MEDIA_SUBTYPE_HEVC) {
        Ok(true) => (true, "found".to_owned()),
        Ok(false) => (false, "not found".to_owned()),
        Err(e) => (false, format!("probe failed: {e}")),
    };
    let found = |b: bool| if b { "found" } else { "not found" };
    let detail = format!(
        "WIC decoders: {}; WebP decoder: {}; HEIF decoder: {}; HEVC video decoder: {}",
        decoders.len(),
        found(webp),
        found(heif_container),
        hevc_text
    );
    WicCodecSupport {
        webp,
        heif_container,
        hevc_decoder,
        detail,
    }
}

/// C# `WicCodecAvailability.Current`: the process-wide cached probe of the real machine (lazy, once, thread-safe;
/// a codec installed while the app runs is seen after a restart). Never runs on the startup path: the first caller pays.
pub fn current() -> &'static WicCodecSupport {
    static PROBE: OnceLock<WicCodecSupport> = OnceLock::new();
    PROBE.get_or_init(|| probe(native::enumerate_decoders, native::has_media_foundation_decoder))
}

/// The two native probes. `unsafe` COM / FFI lives here only, behind safe functions.
pub mod native {
    use super::{DecoderEntry, Guid};
    use std::ffi::c_void;
    use std::io;

    use windows::Win32::Graphics::Imaging::{IWICBitmapCodecInfo, IWICImagingFactory};
    use windows::Win32::Media::MediaFoundation::{IMFActivate, MFT_ENUM_FLAG, MFT_REGISTER_TYPE_INFO, MFTEnumEx};
    use windows::Win32::System::Com::{
        COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize, IEnumUnknown,
    };
    use windows_core::{GUID, HRESULT, Interface};

    use super::{
        MEDIA_TYPE_VIDEO, MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAGS, WIC_COMPONENT_ENUMERATE_DEFAULT,
        WIC_DECODER_COMPONENT,
    };

    /// `WINCODEC_SDK_VERSION1` (W4-11a's `interop::WINCODEC_SDK_VERSION1`, `WicInterop.cs:496`).
    const WINCODEC_SDK_VERSION1: u32 = 0x0236;
    /// `E_FAIL`, the result for a null factory with a non-negative HRESULT.
    const E_FAIL: u32 = 0x8000_4005;
    /// Longest string (in UTF-16 units, NUL included) the probe accepts from a codec-info getter.
    const MAX_STRING_CHARS: u32 = 4096;

    // `raw-dylib`: no import library needed. Same entry point as C# `WICCreateImagingFactory_Proxy`.
    #[link(name = "windowscodecs", kind = "raw-dylib")]
    unsafe extern "system" {
        fn WICCreateImagingFactory_Proxy(sdk_version: u32, factory: *mut *mut c_void) -> HRESULT;
    }

    /// Makes sure COM is initialised on this thread for the lifetime of the guard (a bare Rust thread has no
    /// apartment). `RPC_E_CHANGED_MODE` (an STA thread) means "already initialised": continue, no `CoUninitialize`.
    struct ComApartment {
        paired: bool,
    }

    impl ComApartment {
        fn enter() -> Self {
            // SAFETY: plain FFI call with no pointers; S_OK / S_FALSE are paired with CoUninitialize in Drop.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            Self { paired: hr.is_ok() }
        }
    }

    // Mutation waiver: skipping `CoUninitialize` here, or leaking/freeing the `MFTEnumEx` array differently, is a pure
    // resource-balance change that no test can observe; they are non-equivalent but untestable.
    impl Drop for ComApartment {
        fn drop(&mut self) {
            if self.paired {
                // SAFETY: pairs the successful CoInitializeEx of `enter` on this same thread.
                unsafe { CoUninitialize() };
            }
        }
    }

    fn com_error(what: &str, e: &windows_core::Error) -> io::Error {
        io::Error::other(format!("{what} failed: 0x{:08X} {}", e.code().0 as u32, e.message()))
    }

    /// .NET `ToByteArray()` order -> Win32 `GUID`.
    fn to_win_guid(g: &Guid) -> GUID {
        let b = g.0;
        GUID::from_values(
            u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            u16::from_le_bytes([b[4], b[5]]),
            u16::from_le_bytes([b[6], b[7]]),
            [b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]],
        )
    }

    /// Win32 `GUID` -> .NET `ToByteArray()` order.
    fn from_win_guid(g: &GUID) -> Guid {
        let mut out = [0u8; 16];
        out[0..4].copy_from_slice(&g.data1.to_le_bytes());
        out[4..6].copy_from_slice(&g.data2.to_le_bytes());
        out[6..8].copy_from_slice(&g.data3.to_le_bytes());
        out[8..16].copy_from_slice(&g.data4);
        Guid(out)
    }

    /// C# `ReadString`: ask for the length with `(0, null)` (a null pointer is required: WIC answers `E_INVALIDARG`
    /// for a dangling non-null one), then read; length 0 or > 4096 or any COM error is `""`.
    fn read_string(mut getter: impl FnMut(u32, *mut u16, *mut u32) -> HRESULT) -> String {
        let mut length = 0u32;
        if getter(0, std::ptr::null_mut(), &mut length).is_err() || length == 0 || length > MAX_STRING_CHARS {
            return String::new();
        }
        let mut buffer = vec![0u16; length as usize];
        let mut actual = 0u32;
        if getter(length, buffer.as_mut_ptr(), &mut actual).is_err() {
            return String::new();
        }
        String::from_utf16_lossy(&buffer).trim_end_matches('\0').to_owned()
    }

    fn describe(info: &IWICBitmapCodecInfo) -> Option<DecoderEntry> {
        // SAFETY: `info` is a live COM reference owned by the caller; the out-value is a local.
        let container = unsafe { info.GetContainerFormat() }.ok()?;
        let raw = info.as_raw();
        let vtable = Interface::vtable(info);
        let friendly_name = read_string(|cch, buf, actual| {
            // SAFETY: `raw` is the live interface pointer; `buf` is null with `cch == 0` (length query) or points to
            // `cch` writable u16; `actual` points to a local.
            unsafe { (vtable.base__.GetFriendlyName)(raw, cch, windows_core::PWSTR(buf), actual) }
        });
        let file_extensions = read_string(|cch, buf, actual| {
            // SAFETY: as above.
            unsafe { (vtable.GetFileExtensions)(raw, cch, windows_core::PWSTR(buf), actual) }
        });
        Some(DecoderEntry {
            container_format: from_win_guid(&container),
            friendly_name,
            file_extensions,
        })
    }

    /// C# `EnumerateDecoders()`: `WICCreateImagingFactory_Proxy` -> `CreateComponentEnumerator(WICDecoder, Default)` ->
    /// walk `IEnumUnknown::Next(1)`; per item `QueryInterface(IWICBitmapCodecInfo)` (no interface: skip), read the
    /// container format, friendly name and file extensions; a COM error on one item skips that item; every
    /// reference handed out is released; the factory and enumerator are released on every path. A factory failure is an
    /// `Err` carrying the HRESULT.
    pub fn enumerate_decoders() -> io::Result<Vec<DecoderEntry>> {
        let _apartment = ComApartment::enter();

        let mut raw: *mut c_void = std::ptr::null_mut();
        // SAFETY: `raw` is a valid out-pointer; on success the callee hands over one reference.
        let hr = unsafe { WICCreateImagingFactory_Proxy(WINCODEC_SDK_VERSION1, &mut raw) };
        if hr.0 < 0 {
            return Err(io::Error::other(format!(
                "WICCreateImagingFactory_Proxy failed: 0x{:08X}",
                hr.0 as u32
            )));
        }
        if raw.is_null() {
            return Err(io::Error::other(format!(
                "WICCreateImagingFactory_Proxy returned no factory: 0x{E_FAIL:08X}"
            )));
        }
        // SAFETY: `raw` is a non-null IWICImagingFactory pointer we own one reference to; the wrapper releases it.
        let factory = unsafe { IWICImagingFactory::from_raw(raw) };

        // SAFETY: `factory` is live; the arguments are plain integers.
        let enumerator: IEnumUnknown =
            unsafe { factory.CreateComponentEnumerator(WIC_DECODER_COMPONENT, WIC_COMPONENT_ENUMERATE_DEFAULT) }
                .map_err(|e| com_error("CreateComponentEnumerator", &e))?;

        let mut result = Vec::new();
        loop {
            let mut slot = [None];
            let mut fetched = 0u32;
            // SAFETY: `slot` is a one-element buffer and `fetched` a local; a returned item is owned by `slot` and
            // released exactly once when dropped.
            let hr = unsafe { enumerator.Next(&mut slot, Some(&mut fetched)) };
            if hr.0 != 0 || fetched != 1 {
                break; // S_FALSE or a failure just ends the walk with what was collected.
            }
            let Some(item) = slot[0].take() else { break };
            let Ok(info) = item.cast::<IWICBitmapCodecInfo>() else {
                continue; // no codec-info interface: skip
            };
            if let Some(entry) = describe(&info) {
                result.push(entry);
            }
        }
        Ok(result)
    }

    /// C# `HasMediaFoundationDecoder(videoSubtype)`: `MFTEnumEx(MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAGS,
    /// { MFMediaType_Video, subtype }, null)`; a failed HRESULT is an `Err`; every returned `IMFActivate` is released
    /// and the array freed with `CoTaskMemFree`; the answer is `count > 0`. No `MFStartup` (C# parity).
    pub fn has_media_foundation_decoder(video_subtype: &Guid) -> io::Result<bool> {
        let _apartment = ComApartment::enter();

        let input = MFT_REGISTER_TYPE_INFO {
            guidMajorType: to_win_guid(&MEDIA_TYPE_VIDEO),
            guidSubtype: to_win_guid(video_subtype),
        };
        let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        // SAFETY: `input` outlives the call; both out-pointers are locals; the output type is null (any).
        let result = unsafe {
            MFTEnumEx(
                to_win_guid(&MFT_CATEGORY_VIDEO_DECODER),
                MFT_ENUM_FLAG(MFT_ENUM_FLAGS as i32),
                Some(&input),
                None,
                &mut activates,
                &mut count,
            )
        };
        result.map_err(|e| com_error("MFTEnumEx", &e))?;

        if !activates.is_null() {
            for i in 0..count as usize {
                // SAFETY: MFTEnumEx returned `count` initialised slots; each is moved out once and dropped, which
                // releases the IMFActivate (null entries are `None`).
                drop(unsafe { activates.add(i).read() });
            }
            // SAFETY: the array was allocated by MFTEnumEx with CoTaskMemAlloc and is not used afterwards.
            unsafe { CoTaskMemFree(Some(activates as *const c_void)) };
        }
        Ok(count > 0)
    }
    #[cfg(test)]
    mod read_string_tests {
        use super::*;

        fn fake(
            len: u32,
            len_hr: HRESULT,
            text: &'static str,
            read_hr: HRESULT,
        ) -> impl FnMut(u32, *mut u16, *mut u32) -> HRESULT {
            move |cch, buf, actual| {
                if cch == 0 {
                    assert!(buf.is_null());
                    // SAFETY: test-only; `actual` points to the caller's local.
                    unsafe { *actual = len };
                    return len_hr;
                }
                let units: Vec<u16> = text.encode_utf16().collect();
                for (k, u) in units.iter().enumerate().take(cch as usize) {
                    // SAFETY: test-only; `buf` holds `cch` writable u16.
                    unsafe { *buf.add(k) = *u };
                }
                // SAFETY: test-only; `actual` points to the caller's local.
                unsafe { *actual = cch };
                read_hr
            }
        }

        #[test]
        fn reads_and_trims_trailing_nul() {
            assert_eq!(read_string(fake(4, HRESULT(0), "ab\0\0", HRESULT(0))), "ab");
        }

        #[test]
        fn zero_length_is_empty() {
            assert_eq!(read_string(fake(0, HRESULT(0), "x", HRESULT(0))), "");
        }

        #[test]
        fn length_query_error_is_empty_even_with_valid_length() {
            assert_eq!(read_string(fake(1, HRESULT(-1), "x", HRESULT(0))), "");
        }

        #[test]
        fn read_error_is_empty() {
            assert_eq!(read_string(fake(1, HRESULT(0), "x", HRESULT(-1))), "");
        }

        #[test]
        fn boundary_4096_is_read_4097_is_empty() {
            let text: &'static str = Box::leak("a".repeat(4096).into_boxed_str());
            assert_eq!(read_string(fake(4096, HRESULT(0), text, HRESULT(0))).len(), 4096);
            assert_eq!(read_string(fake(4097, HRESULT(0), text, HRESULT(0))), "");
        }
    }
}
