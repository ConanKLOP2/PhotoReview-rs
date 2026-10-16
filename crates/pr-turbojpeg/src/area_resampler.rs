//! Port of `PhotoReview.Imaging.TurboJpeg/PixelAreaResampler.cs` (C# baseline d92d6aae): the final shrink of a
//! DCT-scaled JPEG (WP-05). Exact area averaging of a `Bgr32` [`PixelBuffer`] in pure integer arithmetic: every output
//! channel is the correctly rounded mean of the source area it covers, so the result is deterministic and needs no WIC.
//! It only ever shrinks (an axis that would grow keeps the source size).
//!
//! No `unsafe`, no native dependency: this file is plain safe Rust inside the FFI crate (the FFI itself is W5-13).
//!
//! PARITY: I-02 / Q-DEC-TURBOJPEG (fine scale after the DCT scale), INV-1 (the result is a NEW buffer with a new `id`).

// Stub scaffolding (D11): the porter removes this line together with the last `todo!()`.
#![allow(unused_variables, unused_imports, dead_code)]

use std::fmt;

use pr_imaging::pixels::{BYTES_PER_PIXEL, PixelBuffer, PixelBufferError, PixelLayout};

/// Failures of [`resize`]. The .NET exception each variant stands for is [`ResizeError::dotnet_type_name`].
/// (C# `ArgumentNullException` has no Rust twin: `src` is a reference.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResizeError {
    /// `src.layout() != Bgr32` (checked FIRST, before the target size). `ArgumentException`.
    SourceNotBgr32 { layout: PixelLayout },
    /// `target_width < 1`. `ArgumentOutOfRangeException`.
    TargetWidthOutOfRange { width: i32 },
    /// `target_height < 1`. `ArgumentOutOfRangeException`.
    TargetHeightOutOfRange { height: i32 },
    /// Allocation of the result failed (`OutOfMemory`) or a buffer was disposed (`Disposed`, C# `ObjectDisposedException`
    /// from `GetRow`). Propagated unchanged from `pixels`.
    Buffer(PixelBufferError),
}

impl ResizeError {
    /// The C# exception type name the golden test compares.
    pub fn dotnet_type_name(&self) -> &'static str {
        match self {
            Self::SourceNotBgr32 { .. } => "ArgumentException",
            Self::TargetWidthOutOfRange { .. } | Self::TargetHeightOutOfRange { .. } => "ArgumentOutOfRangeException",
            Self::Buffer(e) => e.dotnet_type_name(),
        }
    }
}

impl fmt::Display for ResizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceNotBgr32 { layout } => write!(f, "Only Bgr32 buffers are resampled (got {layout:?})."),
            Self::TargetWidthOutOfRange { width } => {
                write!(f, "targetWidth ('{width}') must be greater than or equal to '1'.")
            }
            Self::TargetHeightOutOfRange { height } => {
                write!(f, "targetHeight ('{height}') must be greater than or equal to '1'.")
            }
            Self::Buffer(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ResizeError {}

impl From<PixelBufferError> for ResizeError {
    fn from(e: PixelBufferError) -> Self {
        Self::Buffer(e)
    }
}

/// Horizontal footprint table (C# locals `first`, `offset`, `w` of `ResizeCore`). Output pixel `x` covers source pixels
/// `first[x] .. first[x] + (offset[x + 1] - offset[x])`, with integer overlaps (in `1/tw` pixel units, summing to `sw`)
/// at `weights[offset[x] .. offset[x + 1]]`. `pub` only so the integration test can pin it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AreaWeights {
    /// Length `tw`: index of the first source pixel of output pixel `x`.
    pub first: Vec<usize>,
    /// Length `tw + 1`: start of the weights of output pixel `x`; `offset[tw] == weights.len()`.
    pub offset: Vec<usize>,
    /// Flattened overlaps, each in `1..=tw`.
    pub weights: Vec<i64>,
}

/// Builds the horizontal table for `sw` source and `tw` target columns (`1 <= tw <= sw`). Pure, no allocation sized from
/// anything but `sw` and `tw`. For every `x`: `lo = x * sw`, `hi = lo + sw`, `s0 = lo / tw`, `s1 = (hi - 1) / tw`, and for
/// `s in s0..=s1` the weight is `min(hi, (s + 1) * tw) - max(lo, s * tw)` (use `i64`: `x * sw` overflows `i32`).
pub fn build_area_weights(sw: usize, tw: usize) -> AreaWeights {
    todo!("W5-14a: ResizeCore, footprint loop")
}

/// C# `PixelAreaResampler.Resize(src, targetWidth, targetHeight)`.
///
/// Check order: layout, then `target_width < 1`, then `target_height < 1`. `tw = min(target_width, src.width())`,
/// `th = min(target_height, src.height())` (never upscales, per axis). Allocates a new `tw x th` `Bgr32` buffer (an
/// allocation error is `ResizeError::Buffer(OutOfMemory)`), copies row by row when `tw == sw && th == sh` (an independent
/// copy with a new `id`, alpha byte copied as is), else area-averages. Every output pixel's alpha byte is `0xFF` on the
/// averaging path (the source's fourth byte is ignored). `src` is only read.
pub fn resize(src: &PixelBuffer, target_width: i32, target_height: i32) -> Result<PixelBuffer, ResizeError> {
    todo!("W5-14a: PixelAreaResampler.Resize")
}

/// C# `ResizeCore`: streams the source rows once, horizontal pass into `i64` sums, vertical accumulation into two
/// `i64` row accumulators (current and next output row), emits a row when the source rows seen reach the end of its
/// footprint. `sw > tw || sh > th` here (the equal case is the copy path). Private: reached through [`resize`].
fn resize_core(
    src: &PixelBuffer,
    dst: &mut PixelBuffer,
    sw: usize,
    sh: usize,
    tw: usize,
    th: usize,
) -> Result<(), ResizeError> {
    todo!("W5-14a: ResizeCore")
}

/// C# `HorizontalPass`: for each output column the weighted `(b, g, r)` sums of one source row (`sums.len() == tw * 3`).
fn horizontal_pass(row: &[u8], table: &AreaWeights, tw: usize, sums: &mut [i64]) {
    todo!("W5-14a: HorizontalPass")
}

/// C# `EmitRow`: `byte = (acc + half) / total` per channel (`half = total / 2`, `total = sw * sh`, integer division of
/// non-negative values), alpha `0xFF`.
fn emit_row(row: &mut [u8], acc: &[i64], tw: usize, total: i64, half: i64) {
    todo!("W5-14a: EmitRow")
}
