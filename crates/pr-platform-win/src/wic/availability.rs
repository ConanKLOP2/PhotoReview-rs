//! Port of `PhotoReview.Imaging/Decoding/Wic/WicCodecAvailability.cs` (C# baseline d92d6aae): which optional Windows
//! codecs this PC has for the WIC-decoded extra formats (Q-FMT-WEBP-HEIC). PARITY: see contracts/drafts-w4/W4-11b.
//!
//! Target path: `crates/pr-platform-win/src/wic/availability.rs` (lead adds `pub mod availability;` to
//! `crates/pr-platform-win/src/wic/mod.rs`, and `pub mod wic;` to `lib.rs`).
//!
//! The decision logic ([`probe`], [`WicCodecSupport`]) is pure and has no `unsafe`. Only the two native seams in
//! [`native`] (`IWICImagingFactory::CreateComponentEnumerator` walk and `MFTEnumEx`) are `unsafe` and are
//! `todo!()` until the porter fills them using the W4-11a COM wrappers (one `// SAFETY:` per block).
#![allow(unused_variables, dead_code)] // D11: only while a body is `todo!()`; the porter removes this line.

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
    todo!()
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
    use std::io;

    /// C# `EnumerateDecoders()`: `WICCreateImagingFactory_Proxy` -> `CreateComponentEnumerator(WICDecoder, Default)` ->
    /// walk `IEnumUnknown::Next(1)`; per item `QueryInterface(IWICBitmapCodecInfo)` (no interface: skip), read the
    /// container format, friendly name and file extensions; a `COMException` on one item skips that item; every
    /// reference handed out is released; the factory and enumerator are released on every path. A factory failure is an
    /// `Err` carrying the HRESULT.
    pub fn enumerate_decoders() -> io::Result<Vec<DecoderEntry>> {
        todo!()
    }

    /// C# `HasMediaFoundationDecoder(videoSubtype)`: `MFTEnumEx(MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAGS,
    /// { MFMediaType_Video, subtype }, null)`; a failed HRESULT is an `Err`; every returned `IMFActivate` is released
    /// and the array freed with `CoTaskMemFree`; the answer is `count > 0`.
    pub fn has_media_foundation_decoder(video_subtype: &Guid) -> io::Result<bool> {
        todo!()
    }
}
