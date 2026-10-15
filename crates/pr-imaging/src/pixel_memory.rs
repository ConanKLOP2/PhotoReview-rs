//! Port of `PhotoReview.Imaging/Pixels/NativePixelMemory.cs`, `Decoding/PixelBufferImageCodec.cs` and
//! `Decoding/ReadOnlyMemoryStreamFactory.cs` (C# baseline d92d6aae). Contract: `contracts/W4-16b.md`.
//!
//! Three small pieces that sit under `PixelBuffer` (W4-16a) and the decoders:
//! - [`NativePixelMemory`]: one 64-byte aligned block, freed exactly once (`Dispose`/`Drop`), with a process-wide live
//!   count. The only `unsafe` of this unit lives here (`std::alloc`, one `// SAFETY:` per block).
//! - [`PixelBufferImageCodec`]: the "pixels" platform image codec (the platform image IS the `PixelBuffer`).
//! - [`ReadOnlyMemoryStreamFactory`]: a non-owning, read-only, seekable stream over the source bytes, no copy.
//!
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::error::Error;
use std::fmt;
use std::io::Cursor;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicPtr, Ordering};

use crate::decode_types::{PixelLease, PlatformImage, SourceBytes}; // W4-01b
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
        match *self {
            Self::InvalidSize { byte_count } => write!(
                f,
                "byteCount ('{byte_count}') must be a non-negative and non-zero value. (Parameter 'byteCount')\r\nActual value was {byte_count}."
            ),
            Self::CannotAddress { byte_count } => write!(f, "Cannot address {byte_count} bytes in this process."),
            Self::OutOfMemory { .. } => f.write_str("Insufficient memory to continue the execution of the program."),
        }
    }
}

impl Error for PixelMemoryError {}

impl From<PixelMemoryError> for DecodeError {
    /// `InvalidSize` => `InvalidArgument`; `CannotAddress` and `OutOfMemory` => `OutOfMemory` (C# `InsufficientMemoryException`
    /// derives `OutOfMemoryException`, so neither is fallbackable: INV-12). The message is `Display` of the source.
    fn from(error: PixelMemoryError) -> Self {
        let message = error.to_string();
        match error {
            PixelMemoryError::InvalidSize { .. } => Self::InvalidArgument { message },
            PixelMemoryError::CannotAddress { .. } | PixelMemoryError::OutOfMemory { .. } => {
                Self::OutOfMemory { message }
            }
        }
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
        Self {
            ptr: AtomicPtr::new(ptr::null_mut()),
            byte_count: 0,
            closed: AtomicBool::new(false),
        }
    }

    /// `NativePixelMemory.Allocate(byteCount)`: `std::alloc::alloc_zeroed` with `Layout::from_size_align(n, ALIGNMENT)`.
    /// Order of checks: `byte_count <= 0` => `InvalidSize`; `byte_count as u64 > usize::MAX as u64` => `CannotAddress`;
    /// `Layout` error or null from the allocator => `OutOfMemory` (never `handle_alloc_error`, never abort). On success
    /// increments the live count by one.
    pub fn allocate(byte_count: i64) -> Result<Self, PixelMemoryError> {
        if byte_count <= 0 {
            return Err(PixelMemoryError::InvalidSize { byte_count });
        }
        // `byte_count > 0` here, so `unsigned_abs` is the value itself.
        if byte_count.unsigned_abs() > usize::MAX as u64 {
            return Err(PixelMemoryError::CannotAddress { byte_count });
        }
        let out_of_memory = PixelMemoryError::OutOfMemory { byte_count };
        let size = usize::try_from(byte_count).map_err(|_| out_of_memory)?;
        let layout = Layout::from_size_align(size, ALIGNMENT).map_err(|_| out_of_memory)?;
        // SAFETY: `layout` has a non-zero size (`byte_count > 0` was checked above) and a valid power-of-two alignment.
        let raw = unsafe { alloc_zeroed(layout) };
        if raw.is_null() {
            return Err(out_of_memory);
        }
        // The pointer is stored in the value before the count moves: there is no failure path in between.
        let memory = Self {
            ptr: AtomicPtr::new(raw),
            byte_count,
            closed: AtomicBool::new(false),
        };
        LIVE_COUNT.fetch_add(1, Ordering::SeqCst);
        Ok(memory)
    }

    /// `NativePixelMemory.LiveCount`.
    #[must_use]
    pub fn live_count() -> i64 {
        LIVE_COUNT.load(Ordering::SeqCst)
    }

    /// `ByteCount`: bytes asked for (0 for the unallocated block).
    #[must_use]
    pub fn byte_count(&self) -> i64 {
        self.byte_count
    }

    /// `IsInvalid`: no block was ever attached (`unallocated()`), or it has been released.
    #[must_use]
    pub fn is_invalid(&self) -> bool {
        self.ptr.load(Ordering::Acquire).is_null()
    }

    /// `IsClosed`: `dispose` ran (or `Drop`).
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// `DangerousGetHandle()`: the block address, null when unallocated or released. The block stays valid only while this
    /// value (or the `PixelBuffer` that owns it) is alive and not disposed; W4-16a builds its `&[u8]` rows from it.
    #[must_use]
    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.ptr.load(Ordering::Acquire)
    }

    /// `ptr as usize` (C# `nint Address`), 0 when unallocated or released. Always a multiple of [`ALIGNMENT`] otherwise.
    #[must_use]
    pub fn address(&self) -> usize {
        self.as_mut_ptr() as usize
    }

    /// `Dispose()`/`ReleaseHandle`: frees the block and decrements the live count exactly once; later calls (also from
    /// another thread) are no-ops. Callers must not hold slices into the block (the caller's contract, as in C#).
    pub fn dispose(&self) {
        self.closed.store(true, Ordering::Release);
        // Exactly once: only the caller that swaps a non-null pointer out frees the block and decrements the count.
        let raw = self.ptr.swap(ptr::null_mut(), Ordering::AcqRel);
        if raw.is_null() {
            return;
        }
        // `allocate` validated the size and it never changes, so this cannot fail; if it somehow did, leaking is
        // safer than freeing with a wrong layout.
        if let Ok(size) = usize::try_from(self.byte_count)
            && let Ok(layout) = Layout::from_size_align(size, ALIGNMENT)
        {
            // SAFETY: `raw` came from `alloc_zeroed` with this same size and alignment (both immutable after
            // construction) and the swap above guarantees no other caller frees it.
            unsafe { dealloc(raw, layout) };
        }
        LIVE_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Drop for NativePixelMemory {
    /// The C# critical finalizer: a forgotten buffer is released on drop (calls the same release path as `dispose`).
    fn drop(&mut self) {
        self.dispose();
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
        Arc::new(Self::INSTANCE)
    }
}

impl PlatformImageCodec for PixelBufferImageCodec {
    /// `Name`: `"pixels"`.
    fn name(&self) -> &'static str {
        "pixels"
    }

    /// `FromPixels`: `Err(DecodeError::Disposed)` when `pixels.is_disposed()` (the buffer is dropped, which frees nothing
    /// more: it is already disposed); otherwise `Ok(Arc::new(pixels))` as the platform image (ownership moves into it, no
    /// copy, no dispose). The "null" argument of C# cannot occur.
    fn image_from_pixels(&self, pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
        if pixels.is_disposed() {
            return Err(disposed_pixel_buffer());
        }
        Ok(PlatformImage::Pixels(Arc::new(pixels)))
    }

    /// `ToPixels`: `platform_image` that is not a `PixelBuffer` => `Err(DecodeError::InvalidArgument)`; a disposed
    /// buffer => `Err(DecodeError::Disposed)`; otherwise `PixelLease::borrowed(buffer)` (`is_owned() == false`: dropping
    /// the lease leaves the cached image alone).
    fn to_pixels<'a>(&self, platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
        let buffer: &'a PixelBuffer = match platform_image {
            PlatformImage::Pixels(buffer) => buffer,
            PlatformImage::Opaque(object) => {
                object
                    .downcast_ref::<PixelBuffer>()
                    .ok_or_else(|| DecodeError::InvalidArgument {
                        message: "Expected a PixelBuffer, got a foreign platform image.".to_owned(),
                    })?
            }
        };
        if buffer.is_disposed() {
            return Err(disposed_pixel_buffer());
        }
        Ok(PixelLease::borrowed(buffer))
    }
}

/// `ObjectDisposedException` of a disposed `PixelBuffer` (.NET wording, as the C# codec throws it).
fn disposed_pixel_buffer() -> DecodeError {
    DecodeError::Disposed {
        message: "Cannot access a disposed object.\r\nObject name: 'PhotoReview.Imaging.Pixels.PixelBuffer'."
            .to_owned(),
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
        Cursor::new(Arc::clone(bytes))
    }

    /// `Create` over a borrowed region (the C# "slice" shape): a cursor over exactly these bytes, still no copy.
    #[must_use]
    pub fn create_from_slice(bytes: &[u8]) -> Cursor<&[u8]> {
        Cursor::new(bytes)
    }
}
