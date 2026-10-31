#![cfg(windows)]

//! Row `X-composition-texts-adapters` (lane `st-texts-adapters`): gaps left by `composition_adapters.rs` and `w8_texts_killers.rs`.
//! New Rust tests (the C# container had no adapter twin): `LocalizedFormat::to_local_time` properties, successful `LazyPrewarmPreviews` image
//! forwarding, and `LazyDecoderFactory` laziness plus forwarding. The registry-`Err` branches are NOT reachable here (see the lane report).

// This existing fixture exports seams for several registry test binaries.
#[allow(dead_code)]
#[path = "common/composition_fixture.rs"]
mod composition_fixture;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use pr_app::services::prewarm::PrewarmPreviews;
use pr_app::viewmodels::main::FormatContextSource;
use pr_core::model::DecoderBackend;
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};
use pr_imaging::decode_types::{
    DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo, PlatformImage,
};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, ImageDecoderFactory};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_shell::composition::adapters::{LazyDecoderFactory, LazyPrewarmPreviews, LocalizedFormat};

fn fixed_utc(text: &str) -> DateTimeUtc {
    DateTimeUtc::parse_json(text).expect("fixed ISO instant").0
}

#[test]
fn to_local_time_is_a_bounded_whole_minute_offset_independent_of_the_separator() {
    // new: adapters.rs `to_local_time` forwards `utc_to_local`; the offset is a real time-zone offset (|offset| <= 14 h, whole minutes) and the separator plays no part.
    let by_new = LocalizedFormat::new();
    let by_fixed = LocalizedFormat::with_group_separator("\u{a0}".to_owned());
    for text in ["2024-01-15T12:34:56Z", "2024-07-15T12:34:56Z", "2000-02-29T00:00:00Z"] {
        let utc = fixed_utc(text);
        let local = by_new.to_local_time(utc);
        assert_eq!(by_fixed.to_local_time(utc), local, "{text}");
        let offset_seconds = (local.ticks() - utc.ticks()) / TICKS_PER_SECOND;
        assert!(offset_seconds.abs() <= 14 * 3600, "{text}: offset {offset_seconds} s");
        assert_eq!(
            offset_seconds % 60,
            0,
            "{text}: offset {offset_seconds} s is not whole minutes"
        );
    }
}

/// A decoder that answers with a real 4x4 image, so the preview worker has a success to publish.
struct ImageDecoderStub;

impl ImageDecoder for ImageDecoderStub {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let pixels = PixelBuffer::allocate(4, 4, PixelLayout::Bgr32).expect("test buffer");
        let options = DecodedImageOptions {
            downscaled: false,
            orientation: 1,
            actual_backend: DecoderBackend::Wpf,
            original_width: 0,
            original_height: 0,
            exif: None,
            is_degraded_fallback: false,
        };
        let image = DecodedImage::new(PlatformImage::Pixels(Arc::new(pixels)), 4, 4, 64, options).expect("test image");
        Ok(Arc::new(image))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Err(DecodeError::InvalidData {
            message: "info unused".to_owned(),
        })
    }
}

struct ImageFactory;

impl ImageDecoderFactory for ImageFactory {
    fn create(&self, _backend: DecoderBackend) -> Arc<dyn ImageDecoder> {
        Arc::new(ImageDecoderStub)
    }

    fn is_registered(&self, _backend: DecoderBackend) -> bool {
        true
    }
}

#[test]
fn lazy_previews_forward_a_successful_viewer_preview_through_the_handle() {
    // new: adapters.rs `start_viewer_preview` Ok branch; the sibling test in `w8_texts_killers` only covers the decoder-error result.
    // The timeout is a hang guard, not an assertion about decode speed.
    let (services, root, _) =
        composition_fixture::shared_services_with(None, Some(Arc::new(|_| Arc::new(ImageFactory))), None);
    let path = root.join("success.jpg");
    std::fs::write(&path, b"fixed source bytes").expect("TempRoot is writable");
    let path = path.to_str().expect("UTF-8 TempRoot path");
    let previews = LazyPrewarmPreviews::new(services);
    let key = previews.current_cache_key(path, None).expect("source exists");
    let result = previews
        .start_viewer_preview(path, &key)
        .wait_timeout(Duration::from_secs(30))
        .unwrap_or_else(|_| panic!("preview worker exceeded the hang guard"));
    let image = match result {
        Ok(image) => image,
        Err(error) => panic!("expected the injected image, got {error:?}"),
    };
    assert_eq!((image.pixel_width(), image.pixel_height()), (4, 4));
}

#[derive(Default)]
struct RecordingFactory {
    created: Mutex<Vec<DecoderBackend>>,
    asked: Mutex<Vec<DecoderBackend>>,
}

struct RecordingForward(Arc<RecordingFactory>);

impl ImageDecoderFactory for RecordingForward {
    fn create(&self, backend: DecoderBackend) -> Arc<dyn ImageDecoder> {
        self.0.created.lock().expect("lock").push(backend);
        Arc::new(ImageDecoderStub)
    }

    fn is_registered(&self, backend: DecoderBackend) -> bool {
        self.0.asked.lock().expect("lock").push(backend);
        backend == DecoderBackend::LibRaw
    }
}

#[test]
fn lazy_decoder_factory_builds_nothing_until_the_first_call_then_forwards_both_methods() {
    // new: adapters.rs `LazyDecoderFactory::{create,is_registered,resolve}` success path (D-W8B6-154).
    let recording = Arc::new(RecordingFactory::default());
    let wrapped = Arc::clone(&recording);
    let resolutions = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&resolutions);
    let (services, _root, _) = composition_fixture::shared_services_with(
        None,
        Some(Arc::new(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            Arc::new(RecordingForward(Arc::clone(&wrapped)))
        })),
        None,
    );
    let lazy = LazyDecoderFactory::new(services);
    assert_eq!(
        resolutions.load(Ordering::SeqCst),
        0,
        "construction must not resolve the factory"
    );

    assert!(lazy.is_registered(DecoderBackend::LibRaw));
    assert!(!lazy.is_registered(DecoderBackend::Wpf));
    let decoder = lazy.create(DecoderBackend::WicDirect);
    assert_eq!(
        resolutions.load(Ordering::SeqCst),
        1,
        "the registry builds the factory once"
    );
    assert_eq!(
        *recording.asked.lock().expect("lock"),
        vec![DecoderBackend::LibRaw, DecoderBackend::Wpf]
    );
    assert_eq!(
        *recording.created.lock().expect("lock"),
        vec![DecoderBackend::WicDirect]
    );
    assert!(decoder.decode(&DecodeRequest::new("x.jpg", 1)).is_ok());
}
