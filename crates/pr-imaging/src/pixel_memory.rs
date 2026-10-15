//! Port of `PhotoReview.Imaging/Pixels/NativePixelMemory.cs`, `Decoding/PixelBufferImageCodec.cs` and
//! `Decoding/ReadOnlyMemoryStreamFactory.cs` (C# baseline d92d6aae). Contract: `contracts/W4-16b.md`.
//!
//! Three small pieces that sit under `PixelBuffer` (W4-16a) and the decoders:
//! - [`NativePixelMemory`]: one 64-byte aligned block, freed exactly once (`Dispose`/`Drop`), with a process-wide live
//!   count. The only `unsafe` of this unit lives here (`std::alloc`, one `// SAFETY:` per block).
//! - [`PixelBufferImageCodec`]: the "pixels" platform image codec (the platform image IS the `PixelBuffer`).
//! - [`ReadOnlyMemoryStreamFactory`]: a non-owning, read-only, seekable stream over the source bytes, no copy.
//!
//! Stub: every body that is not trivial is `todo!()`; signatures and data types are final. Remove the `#![allow(...)]`
//! line when implemented.

#![allow(unused_variables, dead_code, clippy::unused_self)]

use std::error::Error;
use std::fmt;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicPtr};

use crate::decode_types::PlatformImage; // W4-01b
use crate::decode_types::{PixelLease, SourceBytes}; // W4-01b
use crate::decoder_traits::{DecodeError, PlatformImageCodec}; // W4-01c
use crate::pixels::PixelBuffer; // W4-16a: needs `is_disposed(&self) -> bool`

/// `NativePixelMemory.Alignment`: 64 bytes (one cache line; enough for AVX-512 and for WIC/D2D).
pub const ALIGNMENT: usize = 64;

/// What `NativePixelMemory::allocate` can fail with. One variant per C# exception type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelMemoryError {
    /// `ArgumentOutOfRangeException` from `ThrowIfNegativeOrZero(byteCount)`: `byte_count <= 0`.
    InvalidSize { byte_count: i64 },
    /// `InsufficientMemoryException`: `byte_count` exceeds the process address space (`usize::MAX`, 32-bit builds only).
    CannotAddress { byte_count: i64 },
    /// `OutOfMemoryException`: the system refused (`alloc` returned null, or the size cannot form a valid `Layout`).
    OutOfMemory { byte_count: i64 },
}

impl fmt::Display for PixelMemoryError {
    /// `InvalidSize`: the .NET `ArgumentOutOfRangeException` text for `byteCount`; `CannotAddress`:
    /// `"Cannot address {n} bytes in this process."`; `OutOfMemory`: `"Insufficient memory to continue the execution of the program."`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl Error for PixelMemoryError {}

impl From<PixelMemoryError> for DecodeError {
    /// `InvalidSize` => `InvalidArgument`; `CannotAddress` and `OutOfMemory` => `OutOfMemory` (C# `InsufficientMemoryException`
    /// derives `OutOfMemoryException`, so neither is fallbackable: INV-12). The message is `Display` of the source.
    fn from(error: PixelMemoryError) -> Self {
        todo!()
    }
}

/// `NativePixelMemory`: the native block of one `PixelBuffer`. `allocate` hands out a block whose contents are
/// zero-initialised (C# leaves them uninitialised: callers must not rely on either); the block is freed exactly once, by
/// [`NativePixelMemory::dispose`] or by `Drop`. Not reference counted. `Send + Sync` falls out of the atomics: no
/// `unsafe impl` is needed.
///
/// `GC.AddMemoryPressure` has no Rust twin and is not ported.
#[derive(Debug)]
pub struct NativePixelMemory {
    /// Start of the block; null for the unallocated block and after release. Swapped to null on release so a
    /// concurrent second `dispose` frees nothing (exactly-once).
    ptr: AtomicPtr<u8>,
    /// Bytes asked for (0 for the unallocated block). Immutable after construction.
    byte_count: i64,
    /// C# `SafeHandle.IsClosed`: set by the first `dispose`, also for the unallocated block.
    closed: AtomicBool,
}

/// Process-wide count of blocks allocated and not yet freed (C# `_liveCount`, always on, also in release builds).
static LIVE_COUNT: AtomicI64 = AtomicI64::new(0);

impl NativePixelMemory {
    /// `new NativePixelMemory()`: the unallocated block (null pointer, `byte_count() == 0`, `is_invalid()`, `dispose` is a
    /// no-op that does not touch the live count). Only for tests that need a big fake `PixelBuffer` without allocating.
    #[must_use]
    pub fn unallocated() -> Self {
        todo!()
    }

    /// `NativePixelMemory.Allocate(byteCount)`: `std::alloc::alloc_zeroed` with `Layout::from_size_align(n, ALIGNMENT)`.
    /// Order of checks: `byte_count <= 0` => `InvalidSize`; `byte_count as u64 > usize::MAX as u64` => `CannotAddress`;
    /// `Layout` error or null from the allocator => `OutOfMemory` (never `handle_alloc_error`, never abort). On success
    /// increments the live count by one.
    pub fn allocate(byte_count: i64) -> Result<Self, PixelMemoryError> {
        todo!()
    }

    /// `NativePixelMemory.LiveCount`.
    #[must_use]
    pub fn live_count() -> i64 {
        todo!()
    }

    /// `ByteCount`: bytes asked for (0 for the unallocated block).
    #[must_use]
    pub fn byte_count(&self) -> i64 {
        todo!()
    }

    /// `IsInvalid`: no block was ever attached (`unallocated()`), or it has been released.
    #[must_use]
    pub fn is_invalid(&self) -> bool {
        todo!()
    }

    /// `IsClosed`: `dispose` ran (or `Drop`).
    #[must_use]
    pub fn is_closed(&self) -> bool {
        todo!()
    }

    /// `DangerousGetHandle()`: the block address, null when unallocated or released. The block stays valid only while this
    /// value (or the `PixelBuffer` that owns it) is alive and not disposed; W4-16a builds its `&[u8]` rows from it.
    #[must_use]
    pub fn as_mut_ptr(&self) -> *mut u8 {
        todo!()
    }

    /// `ptr as usize` (C# `nint Address`), 0 when unallocated or released. Always a multiple of [`ALIGNMENT`] otherwise.
    #[must_use]
    pub fn address(&self) -> usize {
        todo!()
    }

    /// `Dispose()`/`ReleaseHandle`: frees the block and decrements the live count exactly once; later calls (also from
    /// another thread) are no-ops. Callers must not hold slices into the block (the caller's contract, as in C#).
    pub fn dispose(&self) {
        todo!()
    }
}

impl Drop for NativePixelMemory {
    /// The C# critical finalizer: a forgotten buffer is released on drop (calls the same release path as `dispose`).
    fn drop(&mut self) {
        todo!()
    }
}

/// `PixelBufferImageCodec` (C-02): the Win32 build's platform image codec, where the "platform image" is the
/// `PixelBuffer` itself. Stateless, so `Send + Sync`. C# `Instance` is [`PixelBufferImageCodec::INSTANCE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PixelBufferImageCodec(());

impl PixelBufferImageCodec {
    /// `PixelBufferImageCodec.Instance`.
    pub const INSTANCE: Self = Self(());

    /// `Instance` as the shared trait object consumers take by constructor (they never default one).
    #[must_use]
    pub fn shared() -> Arc<dyn PlatformImageCodec> {
        todo!()
    }
}

impl PlatformImageCodec for PixelBufferImageCodec {
    /// `Name`: `"pixels"`.
    fn name(&self) -> &'static str {
        todo!()
    }

    /// `FromPixels`: `Err(DecodeError::Disposed)` when `pixels.is_disposed()` (the buffer is dropped, which frees nothing
    /// more: it is already disposed); otherwise `Ok(Arc::new(pixels))` as the platform image (ownership moves into it, no
    /// copy, no dispose). The "null" argument of C# cannot occur.
    fn image_from_pixels(&self, pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
        todo!()
    }

    /// `ToPixels`: `platform_image` that is not a `PixelBuffer` => `Err(DecodeError::InvalidArgument)`; a disposed
    /// buffer => `Err(DecodeError::Disposed)`; otherwise `PixelLease::borrowed(buffer)` (`is_owned() == false`: dropping
    /// the lease leaves the cached image alone).
    fn to_pixels<'a>(&self, platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
        todo!()
    }
}

/// `ReadOnlyMemoryStreamFactory`: wraps the source bytes in a non-owning, read-only stream with position 0. In C# the
/// fast path avoids a copy and a slice falls back to `ToArray()`; in Rust the bytes are always an `Arc<[u8]>` or a
/// `&[u8]`, so there is never a copy and no fallback branch. Writing is impossible by type (`Cursor<Arc<[u8]>>` and
/// `Cursor<&[u8]>` implement `Read + Seek`, not `Write`).
#[derive(Debug, Clone, Copy)]
pub struct ReadOnlyMemoryStreamFactory;

impl ReadOnlyMemoryStreamFactory {
    /// `Create(ReadOnlyMemory<byte>)` over shared source bytes: a cursor holding one more `Arc` reference (no copy).
    /// `Length` is `bytes.len()`, `Position` 0; an empty `bytes` gives an empty stream (reads return 0).
    #[must_use]
    pub fn create(bytes: &SourceBytes) -> Cursor<Arc<[u8]>> {
        todo!()
    }

    /// `Create` over a borrowed region (the C# "slice" shape): a cursor over exactly these bytes, still no copy.
    #[must_use]
    pub fn create_from_slice(bytes: &[u8]) -> Cursor<&[u8]> {
        todo!()
    }
}
