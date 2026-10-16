//! Port of `PhotoReview.Imaging.TurboJpeg/PixelAreaResampler.cs` (C# baseline d92d6aae): the final shrink of a
//! DCT-scaled JPEG (WP-05). Exact area averaging of a `Bgr32` [`PixelBuffer`] in pure integer arithmetic: every output
//! channel is the correctly rounded mean of the source area it covers, so the result is deterministic and needs no WIC.
//! It only ever shrinks (an axis that would grow keeps the source size).
//!
//! No `unsafe`, no native dependency: this file is plain safe Rust inside the FFI crate (the FFI itself is W5-13).
//!
//! PARITY: I-02 / Q-DEC-TURBOJPEG (fine scale after the DCT scale), INV-1 (the result is a NEW buffer with a new `id`).

use std::fmt;

use pr_imaging::pixels::{PixelBuffer, PixelBufferError, PixelLayout};

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
    let mut first = Vec::with_capacity(tw);
    let mut offset = Vec::with_capacity(tw + 1);
    let mut weights: Vec<i64> = Vec::with_capacity(sw + 2 * tw);
    // Every product mixes an output index with a width: all in i64 (C# `(long)x * sw`).
    let (sw64, tw64) = (sw as i64, tw as i64);
    for x in 0..tw {
        let lo = x as i64 * sw64;
        let hi = lo + sw64;
        let s0 = lo / tw64;
        let s1 = (hi - 1) / tw64;
        first.push(s0 as usize);
        offset.push(weights.len());
        for s in s0..=s1 {
            weights.push(hi.min((s + 1) * tw64) - lo.max(s * tw64));
        }
    }
    offset.push(weights.len());
    AreaWeights { first, offset, weights }
}

/// C# `PixelAreaResampler.Resize(src, targetWidth, targetHeight)`.
///
/// Check order: layout, then `target_width < 1`, then `target_height < 1`. `tw = min(target_width, src.width())`,
/// `th = min(target_height, src.height())` (never upscales, per axis). Allocates a new `tw x th` `Bgr32` buffer (an
/// allocation error is `ResizeError::Buffer(OutOfMemory)`), copies row by row when `tw == sw && th == sh` (an independent
/// copy with a new `id`, alpha byte copied as is), else area-averages. Every output pixel's alpha byte is `0xFF` on the
/// averaging path (the source's fourth byte is ignored). `src` is only read.
pub fn resize(src: &PixelBuffer, target_width: i32, target_height: i32) -> Result<PixelBuffer, ResizeError> {
    if src.layout() != PixelLayout::Bgr32 {
        return Err(ResizeError::SourceNotBgr32 { layout: src.layout() });
    }
    if target_width < 1 {
        return Err(ResizeError::TargetWidthOutOfRange { width: target_width });
    }
    if target_height < 1 {
        return Err(ResizeError::TargetHeightOutOfRange { height: target_height });
    }
    // Never upscales: an axis that would grow keeps the source size.
    let (sw, sh) = (src.width(), src.height());
    let (tw, th) = (target_width.min(sw), target_height.min(sh));

    // The result is allocated before any read of `src`, so a disposed source fails after the allocation (as in C#).
    let mut dst = PixelBuffer::allocate(tw, th, PixelLayout::Bgr32)?;
    if tw == sw && th == sh {
        for y in 0..sh {
            let from = src.get_row(y)?;
            dst.get_row_mut(y)?.copy_from_slice(from);
        }
        return Ok(dst);
    }

    resize_core(src, &mut dst, sw as usize, sh as usize, tw as usize, th as usize)?;
    Ok(dst)
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
    let table = build_area_weights(sw, tw);
    // Source rows are streamed once; `total`, `half` and the accumulators are exact `i64` values (no float).
    let (sw64, sh64, th64) = (sw as i64, sh as i64, th as i64);
    let total = sw64 * sh64;
    let half = total / 2;
    let mut row_sums = vec![0i64; tw * 3];
    let mut acc_current = vec![0i64; tw * 3];
    let mut acc_next = vec![0i64; tw * 3];
    let mut output_row: usize = 0;

    for sy in 0..sh {
        let src_row = src.get_row(sy as i32)?;
        horizontal_pass(src_row, &table, tw, &mut row_sums);

        // Output rows overlapped by source row `sy`: at most two, because the image only shrinks.
        let sy64 = sy as i64;
        let src_lo = sy64 * th64;
        let src_hi = src_lo + th64;
        let oy0 = (src_lo / sh64) as usize;
        let oy1 = ((src_hi - 1) / sh64) as usize;
        for oy in oy0..=oy1 {
            if oy >= th {
                break;
            }
            let oy64 = oy as i64;
            let overlap = src_hi.min((oy64 + 1) * sh64) - src_lo.max(oy64 * sh64);
            let acc = if oy == output_row {
                &mut acc_current
            } else {
                &mut acc_next
            };
            for (a, &sum) in acc.iter_mut().zip(&row_sums) {
                *a += sum * overlap;
            }
        }

        if src_hi >= (output_row as i64 + 1) * sh64 {
            emit_row(dst.get_row_mut(output_row as i32)?, &acc_current, tw, total, half);
            std::mem::swap(&mut acc_current, &mut acc_next);
            acc_next.fill(0);
            output_row += 1;
        }
    }
    Ok(())
}

/// C# `HorizontalPass`: for each output column the weighted `(b, g, r)` sums of one source row (`sums.len() == tw * 3`).
fn horizontal_pass(row: &[u8], table: &AreaWeights, tw: usize, sums: &mut [i64]) {
    for x in 0..tw {
        let (mut b, mut g, mut r) = (0i64, 0i64, 0i64);
        // Byte index of the first source pixel of output pixel `x`; the footprint is inside the row by construction.
        let mut s = table.first[x] * 4;
        for &weight in &table.weights[table.offset[x]..table.offset[x + 1]] {
            b += i64::from(row[s]) * weight;
            g += i64::from(row[s + 1]) * weight;
            r += i64::from(row[s + 2]) * weight;
            s += 4;
        }
        sums[x * 3] = b;
        sums[x * 3 + 1] = g;
        sums[x * 3 + 2] = r;
    }
}

/// C# `EmitRow`: `byte = (acc + half) / total` per channel (`half = total / 2`, `total = sw * sh`, integer division of
/// non-negative values), alpha `0xFF`.
fn emit_row(row: &mut [u8], acc: &[i64], tw: usize, total: i64, half: i64) {
    for x in 0..tw {
        let o = x * 3;
        let d = x * 4;
        // `acc <= 255 * total`, so each quotient is at most 255 and the `as u8` never truncates a real value.
        row[d] = ((acc[o] + half) / total) as u8;
        row[d + 1] = ((acc[o + 1] + half) / total) as u8;
        row[d + 2] = ((acc[o + 2] + half) / total) as u8;
        row[d + 3] = 0xFF;
    }
}
