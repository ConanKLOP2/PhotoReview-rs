//! Upfront memory estimate for a full LibRaw decode (port of `PhotoReview.Imaging.LibRaw.DecodeMemoryGuard`, W5-16b, C# baseline d92d6aae).
//!
//! Target path: `crates/pr-libraw/src/memory_guard.rs` (lead adds `pub mod memory_guard;` to `lib.rs`, see `mod-lines.txt`).
//!
//! Peak estimate = `width x height x (8 + 3) + rawWidth x rawHeight x (2 | 8 | 24) + targetWidth x targetHeight x 4` bytes. Pure integer
//! arithmetic and string matching: no FFI, no `unsafe`, no OS query, no dependency. The comparison with the free memory is NOT here:
//! the caller passes the estimate to `pr_imaging::memory_headroom::has_headroom` (W4-07b), fed by a `MemoryInfoSource`.

/// LibRaw's working image (4 x 16-bit channels), alive until the handle is closed (C# `WorkingImageBytesPerPixel`).
pub const WORKING_IMAGE_BYTES_PER_PIXEL: i64 = 8;
/// The 8-bit RGB output buffer (`libraw_dcraw_make_mem_image`) that coexists with the working image (C# `RgbOutputBytesPerPixel`).
pub const RGB_OUTPUT_BYTES_PER_PIXEL: i64 = 3;
/// The output bitmap, written in place (C# `BitmapBytesPerPixel`).
pub const BITMAP_BYTES_PER_PIXEL: i64 = 4;
/// One 16-bit sample per sensor pixel (C# `BayerRawBytesPerPixel`).
pub const BAYER_RAW_BYTES_PER_PIXEL: i64 = 2;
/// Non-Bayer integer data, 4 x 16-bit per pixel (C# `LinearRawBytesPerPixel`).
pub const LINEAR_RAW_BYTES_PER_PIXEL: i64 = 8;
/// Conservative peak of a floating-point DNG (C# `FloatRawBytesPerPixel`).
pub const FLOAT_RAW_BYTES_PER_PIXEL: i64 = 24;

/// How LibRaw stores the unpacked raw data (C# `RawBufferFamily`), which decides the size of the buffer it keeps until the handle is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RawBufferFamily {
    /// One 16-bit sample per sensor pixel (`raw_image`), 2 B/px; also X-Trans.
    Bayer,
    /// Non-Bayer integer data (linear DNG, ProRAW-style): up to 4 x 16-bit per pixel, 8 B/px.
    Linear,
    /// Floating-point DNG: float buffer plus the 16-bit copy alive together, 24 B/px (conservative).
    Float,
}

impl RawBufferFamily {
    /// Bytes per sensor pixel of the unpacked raw buffer (the `switch` of C# `EstimatePeakBytes`).
    pub fn raw_bytes_per_pixel(self) -> i64 {
        match self {
            Self::Bayer => BAYER_RAW_BYTES_PER_PIXEL,
            Self::Linear => LINEAR_RAW_BYTES_PER_PIXEL,
            Self::Float => FLOAT_RAW_BYTES_PER_PIXEL,
        }
    }
}

/// What the file's own header says about its sampling (C# `RawStructure`, from `libraw_iparams_t`): `colors` and the CFA `filters` (0 = no mosaic).
/// Public fields, value equality: the C# record struct. `libraw_iparams_t.colors` is a C `int`; `filters` is `unsigned`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RawStructure {
    pub colors: i32,
    pub filters: u32,
}

impl RawStructure {
    pub fn new(colors: i32, filters: u32) -> Self {
        Self { colors, filters }
    }

    /// C# `IsBayer`: a mosaic (`filters != 0`) of 1 to 4 colours (inclusive): LibRaw keeps a single 16-bit sample per pixel.
    pub fn is_bayer(&self) -> bool {
        self.filters != 0 && (1..=4).contains(&self.colors)
    }
}

/// C# `Classify`: classifies the raw buffer by STRUCTURE, not by decoder name alone.
///
/// Order (each step ends the function): `None` name => Bayer; name contains `_fp_dng` => Float; name contains `deflate_dng`, `lossless_dng`,
/// `packed_dng` or `lossy_dng` (DNG family) => Bayer when `structure` is `Some` and `is_bayer()`, else Linear (also when `structure` is
/// `None`: the conservative 8 B/px); any other name (also the empty string) => Bayer. Matching is an ORDINAL, case-sensitive substring test
/// (`str::contains`; no `to_lowercase`, no trimming). `uncompressed_fp_dng_load_raw()` contains `_fp_dng` only, `deflate_fp_dng_load_raw()`
/// contains both `_fp_dng` and no `deflate_dng`: the Float test runs FIRST and the DNG family names never overlap it.
pub fn classify(decoder_name: Option<&str>, structure: Option<RawStructure>) -> RawBufferFamily {
    let Some(name) = decoder_name else {
        return RawBufferFamily::Bayer;
    };
    if name.contains("_fp_dng") {
        return RawBufferFamily::Float;
    }
    let dng = ["deflate_dng", "lossless_dng", "packed_dng", "lossy_dng"]
        .iter()
        .any(|n| name.contains(n));
    if dng {
        return match structure {
            Some(s) if s.is_bayer() => RawBufferFamily::Bayer,
            _ => RawBufferFamily::Linear,
        };
    }
    RawBufferFamily::Bayer
}

/// C# `EstimatePeakBytes` with every optional argument supplied (`raw_width` / `raw_height` = 0 and `RawBufferFamily::Bayer` are the C# defaults).
///
/// `width <= 0 || height <= 0` => 0. Otherwise (all arithmetic exact, no wrap; use `u128`/`i128`, the C# `decimal`):
/// `source = width * height * (8 + 3)`; `raw_pixels = raw_width * raw_height` when BOTH `raw_width > 0` and `raw_height > 0`, else `width * height`;
/// `raw = raw_pixels * family.raw_bytes_per_pixel()`; `target = max(0, target_width) * max(0, target_height) * 4`;
/// `total = source + raw + target`; the result saturates at `i64::MAX` (`total >= i64::MAX` => `i64::MAX`).
pub fn estimate_peak_bytes(
    width: i32,
    height: i32,
    target_width: i32,
    target_height: i32,
    raw_width: i32,
    raw_height: i32,
    family: RawBufferFamily,
) -> i64 {
    if width <= 0 || height <= 0 {
        return 0;
    }
    let (w, h) = (width as u128, height as u128);
    let source = w * h * ((WORKING_IMAGE_BYTES_PER_PIXEL + RGB_OUTPUT_BYTES_PER_PIXEL) as u128);
    let raw_pixels = if raw_width > 0 && raw_height > 0 {
        (raw_width as u128) * (raw_height as u128)
    } else {
        w * h
    };
    let raw = raw_pixels * (family.raw_bytes_per_pixel() as u128);
    let target = (target_width.max(0) as u128) * (target_height.max(0) as u128) * (BITMAP_BYTES_PER_PIXEL as u128);
    let total = source + raw + target;
    i64::try_from(total).unwrap_or(i64::MAX)
}

/// The C# call with the defaults `rawWidth = 0, rawHeight = 0, family = Bayer` (tests call `EstimatePeakBytes(w, h, tw, th)`): the raw buffer is
/// sized from the OUTPUT size at 2 B/px. Equals `estimate_peak_bytes(width, height, target_width, target_height, 0, 0, RawBufferFamily::Bayer)`.
pub fn estimate_peak_bytes_output_sized(width: i32, height: i32, target_width: i32, target_height: i32) -> i64 {
    estimate_peak_bytes(width, height, target_width, target_height, 0, 0, RawBufferFamily::Bayer)
}
