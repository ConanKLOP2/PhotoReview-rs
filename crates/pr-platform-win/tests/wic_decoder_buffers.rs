#![cfg(windows)]
//! Red ports of the native-buffer lifetime tests of `WicDirectDecoder` (C# baseline d92d6aae):
//! `Decoding/WicDirectDecoderWp03ReviewTests.cs` (the three `AllocateAndFill_*` tests) and
//! `Decoding/WicDirectDecoderCopyCountTests.cs` (the one-copy guarantee, WP-03).
//!
//! The live-buffer counter (`NativePixelMemory::live_count`, W4-16a) is process-wide, so this file is its own test binary and every
//! test takes [`serial`] before it reads the counter. Run it with `--test-threads=1` as well if a test is added that does not use the
//! lock. The decoder tests need real WIC and are `#[ignore = "native: needs WIC"]`.

use std::fs;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_core::abstractions::{PhysicalSourceReader, SourceReader};
use pr_imaging::decode_types::{DecodeBox, DecodeRequest, PixelLease, PlatformImage};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, PlatformImageCodec};
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::pixel_memory::{NativePixelMemory, PixelBufferImageCodec};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::wic::decoder::{ICC_TRANSFORM_FAILED_PREFIX, WicDirectDecoder, allocate_and_fill};
use pr_test_support::golden;

/// The live-buffer counter is process-wide: the tests of this binary run one at a time.
static COUNTER_LOCK: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    COUNTER_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

fn live() -> i64 {
    NativePixelMemory::live_count()
}

fn fixture(name: &str) -> std::path::PathBuf {
    golden::repo_root()
        .join("tests")
        .join("fixtures")
        .join("wic-direct")
        .join(name)
}

fn memory() -> Arc<dyn MemoryInfoSource> {
    Arc::new(|| MemoryInfo {
        total_available: 64 * 1024 * 1024 * 1024,
        load: 0,
    })
}

/// A codec double that counts `image_from_pixels` calls and records the id of every buffer it receives. Otherwise it is the
/// production `PixelBufferImageCodec`.
struct CountingCodec {
    inner: PixelBufferImageCodec,
    buffer_ids: Mutex<Vec<i64>>,
}

impl CountingCodec {
    fn new() -> Self {
        Self {
            inner: PixelBufferImageCodec::INSTANCE,
            buffer_ids: Mutex::new(Vec::new()),
        }
    }

    fn ids(&self) -> Vec<i64> {
        self.buffer_ids.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl PlatformImageCodec for CountingCodec {
    fn name(&self) -> &'static str {
        "pixels"
    }

    fn image_from_pixels(&self, pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
        self.buffer_ids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(pixels.id());
        self.inner.image_from_pixels(pixels)
    }

    fn to_pixels<'a>(&self, platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
        self.inner.to_pixels(platform_image)
    }
}

fn decoder_for(codec: Arc<dyn PlatformImageCodec>) -> WicDirectDecoder {
    WicDirectDecoder::new(
        codec,
        Some(Arc::new(PhysicalSourceReader) as Arc<dyn SourceReader>),
        memory(),
    )
}

// ---------------------------------------------------------------------------------------------------------------
// C# WicDirectDecoderWp03ReviewTests: the allocate-and-fill step, no WIC needed
// ---------------------------------------------------------------------------------------------------------------

// C#: WicDirectDecoderWp03ReviewTests.AllocateAndFill_CopyThrows_BufferFreedAtOnce
#[test]
fn allocate_and_fill_copy_throws_buffer_freed_at_once() {
    let _guard = serial();
    let before = live();
    let result = allocate_and_fill(
        64,
        48,
        PixelLayout::Bgr32,
        |_| {
            Err(DecodeError::Com {
                hresult: -2_147_467_259,
                message: "CopyPixels failed".to_owned(),
            })
        },
        false,
    );
    assert!(result.is_err(), "the failed copy is the error");
    assert_eq!(
        live(),
        before,
        "the native buffer is freed before the error is returned"
    );
}

// C#: WicDirectDecoderWp03ReviewTests.AllocateAndFill_ComFailureWithTransform_IsNotSupportedAndFreed
#[test]
fn allocate_and_fill_com_failure_with_transform_is_not_supported_and_freed() {
    let _guard = serial();
    let before = live();
    let result = allocate_and_fill(
        64,
        48,
        PixelLayout::Pbgra32,
        |_| {
            Err(DecodeError::Com {
                hresult: -2_003_292_336,
                message: "converter exploded".to_owned(),
            })
        },
        true,
    );
    match result.err().as_ref().map(DecodeError::kind) {
        Some(DecodeError::NotSupported { message, .. }) => {
            assert!(message.starts_with(ICC_TRANSFORM_FAILED_PREFIX), "{message}");
        }
        other => panic!("expected the ICC fallback error, got {other:?}"),
    }
    assert_eq!(live(), before, "the buffer is freed on the failure path");
}

// C#: WicDirectDecoderWp03ReviewTests.AllocateAndFill_Success_ReturnsTheLiveBuffer
#[test]
fn allocate_and_fill_success_returns_the_live_buffer() {
    let _guard = serial();
    let before = live();
    let buffer = allocate_and_fill(
        64,
        48,
        PixelLayout::Bgr32,
        |buf| {
            buf.bytes_mut().expect("live buffer").fill(7);
            Ok(())
        },
        false,
    )
    .expect("a successful fill returns the buffer");
    assert_eq!(live(), before + 1, "the caller owns one live native buffer");
    assert_eq!((buffer.width(), buffer.height()), (64, 48));
    assert_eq!(
        buffer.bytes().expect("live buffer")[0],
        7,
        "the copy wrote into this buffer"
    );
    drop(buffer);
    assert_eq!(live(), before);
}

// ---------------------------------------------------------------------------------------------------------------
// C# WicDirectDecoderCopyCountTests (one native copy, WP-03): real WIC, codec double
// ---------------------------------------------------------------------------------------------------------------

// C#: WicDirectDecoderCopyCountTests.PixelCodec_TheDecodedImageIsTheVeryBufferWicFilled
#[test]
#[ignore = "native: needs WIC"]
fn pixel_codec_the_decoded_image_is_the_very_buffer_wic_filled() {
    let _guard = serial();
    let codec = Arc::new(CountingCodec::new());
    let decoder = decoder_for(codec.clone());
    let before = live();
    let view = decoder
        .decode(&DecodeRequest::new(fixture("plain-64x48.jpg"), 0))
        .expect("plain JPEG decodes");
    let ids = codec.ids();
    assert_eq!(
        ids.len(),
        1,
        "exactly one image_from_pixels call: one copy of the pixels"
    );
    let pixels = view.platform_image().as_pixels().expect("pixels platform image");
    assert_eq!(pixels.id(), ids[0], "the decoded image is the buffer WIC filled");
    assert_eq!(live(), before + 1, "one live native buffer for the decoded image");
    drop(view);
    assert_eq!(live(), before, "dropping the image frees its buffer");
}

// C#: WicDirectDecoderCopyCountTests.FailedDecode_LeavesNoNativeBufferBehind_AndNeverReachesTheCodec
#[test]
#[ignore = "native: needs WIC"]
fn failed_decode_leaves_no_native_buffer_behind_and_never_reaches_the_codec() {
    let _guard = serial();
    let codec = Arc::new(CountingCodec::new());
    let decoder = decoder_for(codec.clone());
    let before = live();
    let result = decoder.decode(&DecodeRequest::new(fixture("text.jpg"), 0));
    assert!(result.is_err(), "a text file is not an image");
    assert!(codec.ids().is_empty(), "the codec is never reached");
    assert_eq!(live(), before, "no native buffer survives the failed decode");
}

// C#: WicDirectDecoderCopyCountTests.BytesRequest_AlsoProducesExactlyOneBuffer
#[test]
#[ignore = "native: needs WIC"]
fn bytes_request_also_produces_exactly_one_buffer() {
    let _guard = serial();
    let codec = Arc::new(CountingCodec::new());
    let decoder = decoder_for(codec.clone());
    let bytes = fs::read(fixture("plain-64x48.jpg")).expect("fixture");
    let request =
        DecodeRequest::with_box(fixture("plain-64x48.jpg"), DecodeBox::UNBOUNDED).with_bytes(Arc::from(bytes));
    let before = live();
    let view = decoder.decode(&request).expect("bytes decode");
    assert_eq!(codec.ids().len(), 1, "one copy on the bytes path too");
    assert_eq!(live(), before + 1);
    drop(view);
    assert_eq!(live(), before);
}
