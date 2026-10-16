//! Red tests for `pr_imaging::pixel_memory` (W4-16b). Ports of C# `PixelBufferLifetimeTests` (live count and finalizer),
//! the alignment line of `PixelBufferTests.Allocate_ReportsGeometryAndLayout`, the `PixelBufferImageCodecTests` codec
//! part, and the goldens `native-memory`, `readonly-memory-stream` and `pixel-codec` (C# baseline d92d6aae).
//! Contract: `contracts/W4-16b.md`. Every test fails through the stub's `todo!()` until the porter writes the bodies.
//!
//! The live count is process-wide and the test harness runs tests on several threads: every test that allocates holds
//! [`guard`], so counts read as deltas are exact. Helpers never take the guard (no re-entrant locking).

use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt::Write as _;
use std::io::{Cursor, Read, Seek};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_imaging::decode_types::{PlatformImage, SourceBytes};
use pr_imaging::decoder_traits::{DecodeError, PlatformImageCodec};
use pr_imaging::pixel_memory::{
    ALIGNMENT, NativePixelMemory, PixelBufferImageCodec, PixelMemoryError, ReadOnlyMemoryStreamFactory,
};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

/// Counts allocations and frees of 64-byte aligned blocks of one distinctive size, so a test can see that the block
/// really goes back to the allocator (the live count alone is bookkeeping and would not notice a skipped `dealloc`).
struct CountingAllocator;

/// Size of the blocks [`CountingAllocator`] watches; no other test or runtime allocation uses it.
const WATCHED_SIZE: usize = 777_777;
static WATCHED_ALLOCS: AtomicUsize = AtomicUsize::new(0);
static WATCHED_FREES: AtomicUsize = AtomicUsize::new(0);

fn is_watched(layout: &Layout) -> bool {
    layout.align() == ALIGNMENT && layout.size() == WATCHED_SIZE
}

// SAFETY: every method forwards to `System` unchanged (same layout, same pointer); only atomic counters are touched.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if is_watched(&layout) {
            WATCHED_ALLOCS.fetch_add(1, Ordering::SeqCst);
        }
        // SAFETY: the caller's `GlobalAlloc::alloc` contract is passed through to `System` untouched.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if is_watched(&layout) {
            WATCHED_ALLOCS.fetch_add(1, Ordering::SeqCst);
        }
        // SAFETY: the caller's `GlobalAlloc::alloc_zeroed` contract is passed through to `System` untouched.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if is_watched(&layout) {
            WATCHED_FREES.fetch_add(1, Ordering::SeqCst);
        }
        // SAFETY: `ptr` and `layout` are exactly what the caller got from this allocator.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller's `GlobalAlloc::realloc` contract is passed through to `System` untouched.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Serialises every test that allocates (the live count is shared by the whole test binary).
static GUARD: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    GUARD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Lowercase hex of `bytes` (the golden `*_hex` fields).
fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(text, "{byte:02x}").expect("writing to a String cannot fail");
    }
    text
}

/// Inverse of [`hex`]; the golden inputs are always well formed.
fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("golden hex is well formed"))
        .collect()
}

/// A 3 x 2 pixel buffer (24 bytes), live and not disposed.
fn small_buffer() -> PixelBuffer {
    PixelBuffer::allocate(3, 2, PixelLayout::Pbgra32).expect("3 x 2 buffer allocates")
}

/// The pixel buffer inside a `PlatformImage::Pixels`.
fn pixels_of(image: &PlatformImage) -> &PixelBuffer {
    let arc: &Arc<PixelBuffer> = image.as_pixels().expect("pixel platform image");
    arc
}

/// The .NET exception name a `DecodeError` stands for in the codec goldens.
fn exception_of_decode(error: &DecodeError) -> &'static str {
    match error {
        DecodeError::Disposed { .. } => "ObjectDisposedException",
        DecodeError::InvalidArgument { .. } => "ArgumentException",
        _ => "unexpected DecodeError kind",
    }
}

/// Asserts that a refused allocation has the exception kind the golden names.
fn assert_refusal(result: &Result<NativePixelMemory, PixelMemoryError>, exception: &str, name: &str) {
    let ok = match exception {
        "ArgumentOutOfRangeException" => matches!(result, Err(PixelMemoryError::InvalidSize { .. })),
        "InsufficientMemoryException" => matches!(result, Err(PixelMemoryError::CannotAddress { .. })),
        "OutOfMemoryException" => matches!(result, Err(PixelMemoryError::OutOfMemory { .. })),
        other => panic!("{name}: unknown golden exception {other}"),
    };
    assert!(ok, "{name}: expected {exception}, got {result:?}");
}

// C#: PixelBufferLifetimeTests.AllocateAndDispose_ChangeLiveCountByExactlyOne
#[test]
fn allocate_and_dispose_change_live_count_by_exactly_one() {
    let _guard = guard();
    let before = NativePixelMemory::live_count();

    // C# `PixelBuffer.Allocate(8, 8, Bgr32)`: 8 * 4 * 8 bytes.
    let block = NativePixelMemory::allocate(8 * 4 * 8).expect("256 bytes allocate");
    assert_eq!(NativePixelMemory::live_count(), before + 1);

    block.dispose();
    assert_eq!(NativePixelMemory::live_count(), before);

    block.dispose();
    assert_eq!(NativePixelMemory::live_count(), before);
}

// C#: PixelBufferLifetimeTests.InvalidAllocate_DoesNotLeak
// The C# second refusal (`PixelLayout` 9) is a layout check of `PixelBuffer`, which has no Rust twin here.
#[test]
fn invalid_allocate_does_not_leak() {
    let _guard = guard();
    let before = NativePixelMemory::live_count();

    assert!(matches!(
        NativePixelMemory::allocate(0),
        Err(PixelMemoryError::InvalidSize { .. })
    ));
    assert!(matches!(
        NativePixelMemory::allocate(-1),
        Err(PixelMemoryError::InvalidSize { .. })
    ));

    assert_eq!(NativePixelMemory::live_count(), before);
}

// C#: PixelBufferLifetimeTests.UnallocatedBlock_IsNotCounted_AndDisposesAsNoOp
#[test]
fn unallocated_block_is_not_counted_and_disposes_as_noop() {
    let _guard = guard();
    let before = NativePixelMemory::live_count();
    let block = NativePixelMemory::unallocated();

    assert!(block.is_invalid());
    block.dispose();

    assert_eq!(NativePixelMemory::live_count(), before);
}

// C#: PixelBufferLifetimeTests.Finalizer_ReleasesForgottenBuffers
// Rust twin: `Drop` is the finalizer. Blocks are dropped without `dispose`; no GC loop is needed.
#[test]
fn finalizer_releases_forgotten_buffers() {
    let _guard = guard();
    let before = NativePixelMemory::live_count();

    let mut forgotten = Vec::new();
    for _ in 0..16 {
        // C# `PixelBuffer.Allocate(64, 64, Pbgra32)`: 64 * 4 * 64 bytes.
        forgotten.push(NativePixelMemory::allocate(64 * 4 * 64).expect("16 KiB allocates"));
    }
    assert_eq!(NativePixelMemory::live_count(), before + 16);

    drop(forgotten);

    assert_eq!(NativePixelMemory::live_count(), before);
}

// C#: PixelBufferTests.Allocate_ReportsGeometryAndLayout (only the `Address % NativePixelMemory.Alignment` line)
#[test]
fn allocate_is_64_byte_aligned() {
    let _guard = guard();
    assert_eq!(ALIGNMENT, 64);

    for size in [1_i64, 3, 63, 64, 65, 4096, 100_003] {
        let block = NativePixelMemory::allocate(size).expect("small allocation succeeds");
        assert_ne!(block.address(), 0, "size {size}: address is not null");
        assert_eq!(
            block.address() % ALIGNMENT,
            0,
            "size {size}: address is 64-byte aligned"
        );
        assert_eq!(
            block.byte_count(),
            size,
            "size {size}: byte_count is the requested size"
        );
    }
}

// Rust-only: INV-12. Allocation refusals stay `OutOfMemory` (not fallbackable), `InvalidSize` is an argument error.
#[test]
fn allocation_errors_map_to_decode_error_kinds_without_fallback() {
    let invalid = PixelMemoryError::InvalidSize { byte_count: 0 };
    let invalid_mapped: DecodeError = invalid.into();
    assert!(matches!(invalid_mapped, DecodeError::InvalidArgument { .. }));

    let cannot = PixelMemoryError::CannotAddress { byte_count: 1 << 40 };
    let cannot_mapped: DecodeError = cannot.into();
    assert!(matches!(cannot_mapped, DecodeError::OutOfMemory { .. }));
    assert_eq!(cannot_mapped.message(), cannot.to_string());

    let refused = PixelMemoryError::OutOfMemory { byte_count: 1 << 62 };
    let refused_mapped: DecodeError = refused.into();
    assert!(matches!(refused_mapped, DecodeError::OutOfMemory { .. }));
    assert_eq!(refused_mapped.message(), refused.to_string());
}

// golden: native-memory.json, every `allocate` case that succeeds (8 cases)
#[test]
fn golden_native_memory_successful_allocations_match_csharp() {
    let golden = golden::load("native-memory");
    let _guard = guard();

    for case in golden
        .cases
        .iter()
        .filter(|case| str_of(case, "kind") == "allocate" && case["exception"].is_null())
    {
        let name = str_of(case, "name");
        let before = NativePixelMemory::live_count();

        let block = NativePixelMemory::allocate(i64_of(case, "byte_count_in"))
            .unwrap_or_else(|error| panic!("{name}: allocate failed: {error}"));
        assert_eq!(block.is_invalid(), bool_of(case, "is_invalid"), "{name}: is_invalid");
        assert_eq!(block.byte_count(), i64_of(case, "byte_count"), "{name}: byte_count");
        assert_eq!(
            i64::try_from(block.address() % ALIGNMENT).expect("below 64"),
            i64_of(case, "address_mod_alignment"),
            "{name}: address_mod_alignment"
        );
        assert_ne!(block.address(), 0, "{name}: address is not null");
        assert_eq!(
            NativePixelMemory::live_count() - before,
            i64_of(case, "live_delta_after_allocate"),
            "{name}: live delta after allocate"
        );
        assert_eq!(
            block.is_closed(),
            bool_of(case, "is_closed_before_dispose"),
            "{name}: is_closed before dispose"
        );

        block.dispose();
        assert_eq!(
            NativePixelMemory::live_count() - before,
            i64_of(case, "live_delta_after_dispose"),
            "{name}: live delta after dispose"
        );
        assert_eq!(
            block.is_closed(),
            bool_of(case, "is_closed_after_dispose"),
            "{name}: is_closed after dispose"
        );

        block.dispose();
        assert_eq!(
            NativePixelMemory::live_count() - before,
            i64_of(case, "live_delta_after_second_dispose"),
            "{name}: live delta after second dispose"
        );
    }
}

// golden: native-memory.json, every `allocate` case that is refused (3 rejections, 2 allocator refusals)
#[test]
fn golden_native_memory_refused_allocations_match_csharp() {
    let golden = golden::load("native-memory");
    let _guard = guard();

    for case in golden
        .cases
        .iter()
        .filter(|case| str_of(case, "kind") == "allocate" && !case["exception"].is_null())
    {
        let name = str_of(case, "name");
        let before = NativePixelMemory::live_count();

        let result = NativePixelMemory::allocate(i64_of(case, "byte_count_in"));
        assert_refusal(&result, str_of(case, "exception"), name);
        drop(result);

        assert_eq!(
            NativePixelMemory::live_count() - before,
            i64_of(case, "live_delta_after_failure"),
            "{name}: live delta after failure"
        );
    }
}

// golden: native-memory.json, the `unallocated` case (1 case)
#[test]
fn golden_native_memory_unallocated_block_matches_csharp() {
    let golden = golden::load("native-memory");
    let _guard = guard();

    for case in golden.cases.iter().filter(|case| str_of(case, "kind") == "unallocated") {
        let name = str_of(case, "name");
        let before = NativePixelMemory::live_count();

        let block = NativePixelMemory::unallocated();
        assert_eq!(block.is_invalid(), bool_of(case, "is_invalid"), "{name}: is_invalid");
        assert_eq!(block.byte_count(), i64_of(case, "byte_count"), "{name}: byte_count");
        assert_eq!(
            i64::try_from(block.address() % ALIGNMENT).expect("below 64"),
            i64_of(case, "address_mod_alignment"),
            "{name}: address_mod_alignment"
        );
        assert_eq!(
            NativePixelMemory::live_count() - before,
            i64_of(case, "live_delta_after_allocate"),
            "{name}: live delta after construction"
        );
        assert_eq!(
            block.is_closed(),
            bool_of(case, "is_closed_before_dispose"),
            "{name}: is_closed before dispose"
        );

        block.dispose();
        assert_eq!(
            NativePixelMemory::live_count() - before,
            i64_of(case, "live_delta_after_dispose"),
            "{name}: live delta after dispose"
        );
        assert_eq!(
            block.is_closed(),
            bool_of(case, "is_closed_after_dispose"),
            "{name}: is_closed after dispose"
        );
        block.dispose();
        assert_eq!(
            NativePixelMemory::live_count() - before,
            i64_of(case, "live_delta_after_second_dispose"),
            "{name}: live delta after second dispose"
        );
    }
}

// C#: PixelBufferImageCodecTests.Codec_IsSingleton_NamedPixels
// `Assert.Same` has no meaning for a unit struct: only the name is asserted, through both entry points.
#[test]
fn codec_is_singleton_named_pixels() {
    assert_eq!(PixelBufferImageCodec::INSTANCE.name(), "pixels");
    let shared: Arc<dyn PlatformImageCodec> = PixelBufferImageCodec::shared();
    assert_eq!(shared.name(), "pixels");
}

// C#: PixelBufferImageCodecTests.FromPixels_ReturnsTheSameBuffer_WithoutDisposingIt
#[test]
fn from_pixels_returns_the_same_buffer_without_disposing_it() {
    let _guard = guard();
    let codec = PixelBufferImageCodec::INSTANCE;
    let buffer = small_buffer();
    let (id, address) = (buffer.id(), buffer.address().expect("live buffer"));

    let image = codec.image_from_pixels(buffer).expect("a live buffer is accepted");

    let pixels = pixels_of(&image);
    assert_eq!(pixels.id(), id, "same buffer id");
    assert_eq!(pixels.address().expect("live buffer"), address, "same block address");
    assert!(!pixels.is_disposed());
}

// C#: PixelBufferImageCodecTests.FromPixels_NullOrDisposed_Throws
// The null half has no Rust twin: a `PixelBuffer` value cannot be null.
#[test]
fn from_pixels_disposed_throws_disposed() {
    let _guard = guard();
    let codec = PixelBufferImageCodec::INSTANCE;
    let mut buffer = small_buffer();
    buffer.dispose();

    match codec.image_from_pixels(buffer) {
        Err(DecodeError::Disposed { .. }) => {}
        other => panic!("expected Disposed, got {other:?}"),
    }
}

// C#: PixelBufferImageCodecTests.ToPixels_LendsTheBuffer_AndDisposingTheLeaseKeepsIt
#[test]
fn to_pixels_lends_the_buffer_and_disposing_the_lease_keeps_it() {
    let _guard = guard();
    let codec = PixelBufferImageCodec::INSTANCE;
    let image = PlatformImage::Pixels(Arc::new(small_buffer()));

    let lease = codec.to_pixels(&image).expect("a live buffer is lent");
    assert!(!lease.is_owned());
    assert!(ptr::eq(lease.pixels().expect("lent pixels"), pixels_of(&image)));

    drop(lease);
    assert!(!pixels_of(&image).is_disposed());
}

// C#: PixelBufferImageCodecTests.ToPixels_RoundTripsFromPixels
#[test]
fn to_pixels_round_trips_from_pixels() {
    let _guard = guard();
    let codec = PixelBufferImageCodec::INSTANCE;
    let pixels = PixelBuffer::allocate(5, 4, PixelLayout::Pbgra32).expect("5 x 4 buffer allocates");

    let image = codec.image_from_pixels(pixels).expect("a live buffer is accepted");
    let lease = codec.to_pixels(&image).expect("the platform image is lent back");

    assert!(ptr::eq(lease.pixels().expect("lent pixels"), pixels_of(&image)));
}

// C#: PixelBufferImageCodecTests.ToPixels_UnknownPlatformImage_ThrowsArgumentException
// The WPF bitmap and the null argument have no Rust twin (no WPF; no null references).
#[test]
fn to_pixels_unknown_platform_image_throws_argument_exception() {
    let codec = PixelBufferImageCodec::INSTANCE;

    let text = PlatformImage::Opaque(Arc::new(String::from("not an image")));
    match codec.to_pixels(&text) {
        Err(DecodeError::InvalidArgument { .. }) => {}
        other => panic!("expected InvalidArgument for a string, got {other:?}"),
    }

    let object = PlatformImage::Opaque(Arc::new(()));
    match codec.to_pixels(&object) {
        Err(DecodeError::InvalidArgument { .. }) => {}
        other => panic!("expected InvalidArgument for a foreign object, got {other:?}"),
    }
}

// C#: PixelBufferImageCodecTests.ToPixels_DisposedBuffer_ThrowsObjectDisposed
#[test]
fn to_pixels_disposed_buffer_throws_object_disposed() {
    let _guard = guard();
    let codec = PixelBufferImageCodec::INSTANCE;
    let mut buffer = small_buffer();
    buffer.dispose();
    let image = PlatformImage::Pixels(Arc::new(buffer));

    match codec.to_pixels(&image) {
        Err(DecodeError::Disposed { .. }) => {}
        other => panic!("expected Disposed, got {other:?}"),
    }
}

// golden: pixel-codec.json (8 cases; the exception kind of each operation is compared by name)
#[test]
fn golden_pixel_codec_cases_match_csharp() {
    let golden = golden::load("pixel-codec");
    let _guard = guard();
    let codec = PixelBufferImageCodec::INSTANCE;

    for case in &golden.cases {
        let name = str_of(case, "name");
        let mut observed: Option<&'static str> = None;

        match str_of(case, "op") {
            "name" => {
                assert_eq!(codec.name(), str_of(case, "codec_name"), "{name}: name");
                assert_eq!(
                    PixelBufferImageCodec::shared().name(),
                    str_of(case, "codec_name"),
                    "{name}: shared name"
                );
            }
            "from_live" => {
                let buffer = small_buffer();
                let (id, address) = (buffer.id(), buffer.address().expect("live buffer"));
                match codec.image_from_pixels(buffer) {
                    Ok(image) => {
                        let pixels = pixels_of(&image);
                        let same = pixels.id() == id && pixels.address().expect("live buffer") == address;
                        assert_eq!(same, bool_of(case, "same_ref"), "{name}: same_ref");
                        assert_eq!(
                            pixels.is_disposed(),
                            bool_of(case, "is_disposed_after"),
                            "{name}: is_disposed_after"
                        );
                    }
                    Err(error) => observed = Some(exception_of_decode(&error)),
                }
            }
            "from_disposed" => {
                let mut buffer = small_buffer();
                buffer.dispose();
                match codec.image_from_pixels(buffer) {
                    Ok(_) => {}
                    Err(error) => observed = Some(exception_of_decode(&error)),
                }
            }
            "to_live" => {
                let image = PlatformImage::Pixels(Arc::new(small_buffer()));
                match codec.to_pixels(&image) {
                    Ok(lease) => {
                        let same = ptr::eq(lease.pixels().expect("lent pixels"), pixels_of(&image));
                        assert_eq!(same, bool_of(case, "same_ref"), "{name}: same_ref");
                        assert_eq!(lease.is_owned(), bool_of(case, "owned"), "{name}: owned");
                        drop(lease);
                        assert_eq!(
                            pixels_of(&image).is_disposed(),
                            bool_of(case, "is_disposed_after_lease_dispose"),
                            "{name}: is_disposed_after_lease_dispose"
                        );
                    }
                    Err(error) => observed = Some(exception_of_decode(&error)),
                }
            }
            "to_disposed" => {
                let mut buffer = small_buffer();
                buffer.dispose();
                let image = PlatformImage::Pixels(Arc::new(buffer));
                match codec.to_pixels(&image) {
                    Ok(_) => {}
                    Err(error) => observed = Some(exception_of_decode(&error)),
                }
            }
            "to_foreign_string" => {
                let image = PlatformImage::Opaque(Arc::new(String::from("not an image")));
                match codec.to_pixels(&image) {
                    Ok(_) => {}
                    Err(error) => observed = Some(exception_of_decode(&error)),
                }
            }
            "to_foreign_object" => {
                let image = PlatformImage::Opaque(Arc::new(()));
                match codec.to_pixels(&image) {
                    Ok(_) => {}
                    Err(error) => observed = Some(exception_of_decode(&error)),
                }
            }
            "roundtrip" => {
                let image = codec
                    .image_from_pixels(small_buffer())
                    .unwrap_or_else(|error| panic!("{name}: image_from_pixels failed: {error}"));
                let lease = codec
                    .to_pixels(&image)
                    .unwrap_or_else(|error| panic!("{name}: to_pixels failed: {error}"));
                let same = ptr::eq(lease.pixels().expect("lent pixels"), pixels_of(&image));
                assert_eq!(same, bool_of(case, "same_ref"), "{name}: same_ref");
                assert_eq!(lease.is_owned(), bool_of(case, "owned"), "{name}: owned");
            }
            other => panic!("{name}: unknown op {other}"),
        }

        assert_eq!(observed, case["exception"].as_str(), "{name}: exception");
    }
}

/// The stream golden's source: eight bytes, `slice-*` cases take windows of it.
const STREAM_SOURCE: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

/// `(offset, length)` of the `slice` golden cases inside [`STREAM_SOURCE`].
fn slice_window(name: &str) -> (usize, usize) {
    match name {
        "slice-middle" => (2, 4),
        "slice-head" => (0, 3),
        "slice-tail" => (6, 2),
        "slice-single" => (7, 1),
        "slice-empty-at-end" => (8, 0),
        other => panic!("no slice window for golden case {other}"),
    }
}

/// Checks one stream against its golden case: position 0, length, seekability, readability, full content and tail.
/// `can_write` cannot be probed at run time in Rust: the stream has no `Write` impl, so the golden is only checked to
/// agree that the stream is read-only.
fn assert_stream_matches<T: AsRef<[u8]>>(mut stream: Cursor<T>, case: &Value) {
    let name = str_of(case, "name");

    assert_eq!(
        i64::try_from(stream.stream_position().expect("seekable")).expect("fits"),
        i64_of(case, "position"),
        "{name}: position"
    );
    assert_eq!(
        i64::try_from(stream.get_ref().as_ref().len()).expect("fits"),
        i64_of(case, "length"),
        "{name}: length"
    );
    assert_eq!(
        stream.stream_position().is_ok(),
        bool_of(case, "can_seek"),
        "{name}: can_seek"
    );
    assert_eq!(
        stream.read(&mut [0_u8; 0]).is_ok(),
        bool_of(case, "can_read"),
        "{name}: can_read"
    );
    assert!(
        !bool_of(case, "can_write"),
        "{name}: Rust streams cannot write (no Write impl)"
    );

    let mut content = Vec::new();
    stream.read_to_end(&mut content).expect("read to end");
    assert_eq!(hex(&content), str_of(case, "content_hex"), "{name}: content_hex");
    let tail = &content[content.len().saturating_sub(2)..];
    assert_eq!(hex(tail), str_of(case, "tail_hex"), "{name}: tail_hex");
}

// golden: readonly-memory-stream.json (11 cases: whole, empty and slice shapes)
#[test]
fn golden_readonly_memory_stream_cases_match_csharp() {
    let golden = golden::load("readonly-memory-stream");

    for case in &golden.cases {
        let name = str_of(case, "name");
        match str_of(case, "shape") {
            // The whole-array shapes: the golden content is the whole source array.
            "whole" => {
                let bytes: SourceBytes = Arc::from(unhex(str_of(case, "content_hex")).as_slice());
                assert_stream_matches(ReadOnlyMemoryStreamFactory::create(&bytes), case);
            }
            // `ReadOnlyMemory.Empty`.
            "empty" => assert_stream_matches(ReadOnlyMemoryStreamFactory::create_from_slice(&[]), case),
            "slice" => {
                let (offset, length) = slice_window(name);
                let window = &STREAM_SOURCE[offset..offset + length];
                assert_stream_matches(ReadOnlyMemoryStreamFactory::create_from_slice(window), case);
            }
            other => panic!("{name}: unknown shape {other}"),
        }
    }
}

// Rust-only: INV-8. The stream shares the source bytes the cache holds; it never copies them.
#[test]
fn create_shares_source_bytes_without_copying() {
    let bytes: SourceBytes = Arc::from(vec![9_u8, 8, 7].as_slice());

    let stream = ReadOnlyMemoryStreamFactory::create(&bytes);

    assert!(Arc::ptr_eq(stream.get_ref(), &bytes));
    assert_eq!(stream.get_ref().len(), 3);
}

// Rust-only: pins the exact .NET message texts (contract) so Display cannot silently change or go empty.
#[test]
fn error_messages_match_dotnet_text() {
    let invalid = PixelMemoryError::InvalidSize { byte_count: -1 };
    assert_eq!(
        invalid.to_string(),
        "byteCount ('-1') must be a non-negative and non-zero value. (Parameter 'byteCount')\r\nActual value was -1."
    );
    let cannot = PixelMemoryError::CannotAddress { byte_count: 5 };
    assert_eq!(cannot.to_string(), "Cannot address 5 bytes in this process.");
    let oom = PixelMemoryError::OutOfMemory { byte_count: 5 };
    assert_eq!(
        oom.to_string(),
        "Insufficient memory to continue the execution of the program."
    );

    let mapped: DecodeError = invalid.into();
    assert_eq!(mapped.message(), invalid.to_string());
    let mapped: DecodeError = oom.into();
    assert_eq!(
        mapped.message(),
        "Insufficient memory to continue the execution of the program."
    );
}

// Rust-only: the disposed-buffer message names the .NET object.
#[test]
fn disposed_messages_match_dotnet_text() {
    let _guard = guard();
    let codec = PixelBufferImageCodec::INSTANCE;
    let expected = "Cannot access a disposed object.\r\nObject name: 'PhotoReview.Imaging.Pixels.PixelBuffer'.";

    let mut buffer = small_buffer();
    buffer.dispose();
    match codec.image_from_pixels(buffer) {
        Err(error @ DecodeError::Disposed { .. }) => assert_eq!(error.message(), expected),
        other => panic!("expected Disposed, got {other:?}"),
    }

    let mut buffer = small_buffer();
    buffer.dispose();
    let image = PlatformImage::Pixels(Arc::new(buffer));
    match codec.to_pixels(&image) {
        Err(error @ DecodeError::Disposed { .. }) => assert_eq!(error.message(), expected),
        other => panic!("expected Disposed, got {other:?}"),
    }
}

// Rust-only: contract note 2, the block is zero-initialised (readers hold `&[u8]` over it).
#[test]
fn allocated_block_is_zeroed() {
    let _guard = guard();
    let size = 1usize << 20;
    let memory = NativePixelMemory::allocate(i64::try_from(size).expect("fits")).expect("1 MiB allocates");
    let address = memory.address();
    assert_ne!(address, 0);
    // SAFETY: `address` is the start of a live, 1 MiB block owned by `memory`, which outlives this slice.
    let bytes = unsafe { std::slice::from_raw_parts(address as *const u8, size) };
    assert!(bytes.iter().all(|&b| b == 0));
}

// Rust-only: kills the `alloc_zeroed` -> `alloc` mutant. Small blocks come back from the allocator's free lists, so
// a dirtied-and-freed block is reused with its old bytes unless the allocation zeroes it.
#[test]
fn reused_small_block_is_zeroed() {
    let _guard = guard();
    for round in 0..64 {
        let first = NativePixelMemory::allocate(256).expect("256 bytes allocate");
        // SAFETY: the pointer is the start of a live 256 byte block owned by `first`; no other reference exists.
        unsafe { ptr::write_bytes(first.as_mut_ptr(), 0xAB, 256) };
        first.dispose();

        let second = NativePixelMemory::allocate(256).expect("256 bytes allocate");
        // SAFETY: `second` owns a live 256 byte block that outlives this slice.
        let bytes = unsafe { std::slice::from_raw_parts(second.as_mut_ptr().cast_const(), 256) };
        assert!(
            bytes.iter().all(|&b| b == 0),
            "round {round}: reused block must be zeroed"
        );
    }
}

// Accepted difference from C#: after `Dispose`, .NET `SafeHandle` keeps `IsInvalid == false` and a non-zero
// `DangerousGetHandle()` (the dangling value). Contract note 7 pins the safer Rust behaviour: a released block reads
// as invalid with a null pointer, so no consumer can be handed a dangling address. `byte_count` is kept, as in C#.
#[test]
fn after_dispose_block_is_invalid_with_null_pointer_by_contract() {
    let _guard = guard();
    let memory = NativePixelMemory::allocate(4096).expect("4096 bytes allocate");
    assert!(!memory.is_invalid());
    assert!(!memory.is_closed());
    memory.dispose();
    assert!(memory.is_closed());
    assert!(memory.is_invalid());
    assert_eq!(memory.address(), 0);
    assert!(memory.as_mut_ptr().is_null());
    assert_eq!(memory.byte_count(), 4096);
}

// Kills "the block is never handed back to the allocator" (dealloc skipped, count still decremented): the live count
// is bookkeeping, the counting global allocator sees the real `dealloc`. Rust-only (C# has no allocator hook).
#[test]
fn dispose_and_drop_hand_the_block_back_to_the_allocator_exactly_once() {
    let _guard = guard();
    let size = i64::try_from(WATCHED_SIZE).expect("fits");
    let (allocs, frees) = (
        WATCHED_ALLOCS.load(Ordering::SeqCst),
        WATCHED_FREES.load(Ordering::SeqCst),
    );

    let block = NativePixelMemory::allocate(size).expect("watched size allocates");
    assert_eq!(WATCHED_ALLOCS.load(Ordering::SeqCst), allocs + 1);
    assert_eq!(WATCHED_FREES.load(Ordering::SeqCst), frees, "nothing freed while alive");

    block.dispose();
    assert_eq!(
        WATCHED_FREES.load(Ordering::SeqCst),
        frees + 1,
        "dispose frees the block"
    );
    block.dispose();
    drop(block);
    assert_eq!(
        WATCHED_FREES.load(Ordering::SeqCst),
        frees + 1,
        "dispose then drop frees once"
    );

    let forgotten = NativePixelMemory::allocate(size).expect("watched size allocates");
    drop(forgotten);
    assert_eq!(
        WATCHED_FREES.load(Ordering::SeqCst),
        frees + 2,
        "drop frees a block that was never disposed"
    );
    assert_eq!(WATCHED_ALLOCS.load(Ordering::SeqCst), allocs + 2);
}

// Kills "the Opaque arm refuses (or accepts) everything": a PixelBuffer wrapped as an opaque platform image is still a
// PixelBuffer (C# `platformImage is PixelBuffer`), a disposed one is refused as Disposed, a foreign one as InvalidArgument.
#[test]
fn to_pixels_accepts_a_pixel_buffer_held_as_an_opaque_platform_image() {
    let _guard = guard();
    let codec = PixelBufferImageCodec::INSTANCE;

    let shared = Arc::new(small_buffer());
    let image = PlatformImage::Opaque(Arc::clone(&shared) as Arc<dyn std::any::Any + Send + Sync>);
    let lease = codec.to_pixels(&image).expect("an opaque PixelBuffer is lent");
    assert!(!lease.is_owned());
    assert!(ptr::eq(lease.pixels().expect("lent pixels"), &*shared));
    drop(lease);
    assert!(!shared.is_disposed());

    let mut dead = small_buffer();
    dead.dispose();
    let image = PlatformImage::Opaque(Arc::new(dead));
    assert!(matches!(codec.to_pixels(&image), Err(DecodeError::Disposed { .. })));

    let foreign = PlatformImage::Opaque(Arc::new(42_u32));
    assert!(matches!(
        codec.to_pixels(&foreign),
        Err(DecodeError::InvalidArgument { .. })
    ));
}
