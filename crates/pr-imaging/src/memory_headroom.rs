//! Port of `PhotoReview.Imaging/Decoding/MemoryHeadroom.cs` (C# baseline d92d6aae): the RAM-budget check shared by the
//! decoders' upfront output-size guards. An estimated peak must fit into the memory the process can still use, so a
//! legitimately huge original (100+ MP) decodes on a machine that has the RAM for it, while a header claiming more than
//! the machine can hold is refused before anything big is allocated. PARITY: I-12 (see contracts/drafts-w4/W4-07b).
//!
//! Target path: `crates/pr-imaging/src/memory_headroom.rs` (lead adds `pub mod memory_headroom;` to `lib.rs`).
//!
//! Pure arithmetic only. The OS query (C# `GC.GetGCMemoryInfo`) is the [`MemoryInfoSource`] seam; the Windows
//! implementation (GlobalMemoryStatusEx) lives in `pr-platform-win` and is NOT part of this unit.
#![allow(unused_variables, dead_code)] // D11: only while a body is `todo!()`; the porter removes this line.

/// Output buffers below this never consult the memory guard (C# `GuardThresholdBytes`, 128 MiB).
pub const GUARD_THRESHOLD_BYTES: i64 = 128 * 1024 * 1024;

/// Peak of a decode in units of the output buffer (C# `OutputPeakFactor`): the native BGRX scratch plus the bitmap the
/// decoder copies it into. The source bytes are already resident and counted in the memory load.
pub const OUTPUT_PEAK_FACTOR: i64 = 2;

/// The C# tuple `(TotalAvailable, Load)` of `ReadGcMemoryInfo`, both in bytes (C# `long`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryInfo {
    /// Physical RAM or the container limit (`TotalAvailableMemoryBytes`). `<= 0` means unknown: nothing is refused.
    pub total_available: i64,
    /// Memory in use (`MemoryLoadBytes`). Negative values are treated as 0 by [`has_headroom`].
    pub load: i64,
}

/// The seam for C# `Func<(long TotalAvailable, long Load)> MemoryInfo` (the injectable property of the decoders, default
/// `MemoryHeadroom.ReadGcMemoryInfo`). Implemented by the Windows probe in `pr-platform-win` and by test doubles; a
/// blanket impl lets a closure `|| MemoryInfo { .. }` be used directly, like the C# lambdas.
pub trait MemoryInfoSource: Send + Sync {
    /// Reads the current total and load. Called once per guard check, only after the cheap size threshold has passed.
    fn read(&self) -> MemoryInfo;
}

impl<F> MemoryInfoSource for F
where
    F: Fn() -> MemoryInfo + Send + Sync,
{
    fn read(&self) -> MemoryInfo {
        self()
    }
}

/// C# `OutputHasHeadroom`: true when a decode whose output buffer is `buffer_length` bytes fits
/// (peak = [`OUTPUT_PEAK_FACTOR`] x the buffer, see [`has_headroom`]).
///
/// A negative (already wrapped) length, or one whose doubled peak would overflow an `i64`, never wraps into "fits":
/// the answer is `total_available_bytes <= 0` (refused unless the total is unknown).
pub fn output_has_headroom(buffer_length: i64, total_available_bytes: i64, memory_load_bytes: i64) -> bool {
    todo!("buffer_length < 0 || buffer_length > i64::MAX / 2 => total <= 0, else has_headroom(buffer_length * 2, ..)")
}

/// C# `HasHeadroom`: true when `estimated_bytes` fits into `total_available_bytes - max(0, memory_load_bytes)`.
/// An unknown total (`<= 0`) never refuses. The comparison is `<=` (an exact fit passes); a load above the total makes
/// the budget negative and refuses every non-negative estimate.
pub fn has_headroom(estimated_bytes: i64, total_available_bytes: i64, memory_load_bytes: i64) -> bool {
    todo!("total <= 0 => true; estimated <= total - max(0, load)")
}

/// Convenience over [`has_headroom`] that reads the source once (the shape of the decoders' `EnsureMemoryHeadroom`).
pub fn has_headroom_from(source: &dyn MemoryInfoSource, estimated_bytes: i64) -> bool {
    todo!("let m = source.read(); has_headroom(estimated_bytes, m.total_available, m.load)")
}

/// Convenience over [`output_has_headroom`] that reads the source once.
pub fn output_has_headroom_from(source: &dyn MemoryInfoSource, buffer_length: i64) -> bool {
    todo!("let m = source.read(); output_has_headroom(buffer_length, m.total_available, m.load)")
}
