//! CL-MUT-RAW4 killers for `decoder/fallback.rs` (`RawDecoder::decode_preview_with_fallbacks`, `select_next_preview`,
//! `decode_without_preview`, `decode_fallback_thumbnail`). Fakes only: a scripted inner decoder keyed by the first byte of
//! the preview it receives, a recording `RawPreviewFallback`, an in-memory source reader and a fixed container reader.
//! No golden files, no RAW corpus, no native code.

use std::collections::HashMap;
use std::io::{self, Cursor};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pr_core::abstractions::{ReadSeek, SourceReadPriority, SourceReader};
use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodeBox, DecodeRequest, DecodedImageView, ImageInfo, PlatformImage};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging_raw::decoder::{MAX_FALLBACK_THUMBNAIL_BYTES, RawDecoder, RawDecoderOptions};
use pr_imaging_raw::registry::RawContainerReaderRegistry;
use pr_imaging_raw::{
    EmbeddedPreview, EmbeddedPreviewKind, PreviewColorSpace, RawContainerInfo, RawContainerReader, RawFormat,
    RawHeaderError, RawHeaderSource, RawPreviewFallback,
};
use pr_test_support::TempRoot;

const FILE_LEN: usize = 8192;
const ORIENTATION: i32 = 6;
const THUMB_MARKER: u8 = 0xEE;

struct MemoryReader(Arc<Vec<u8>>);

impl SourceReader for MemoryReader {
    fn open_source(&self, _: &Path, _: SourceReadPriority, _: usize) -> io::Result<Box<dyn ReadSeek>> {
        Ok(Box::new(Cursor::new(self.0.as_ref().clone())))
    }
}

struct FixedReader(RawContainerInfo);

impl RawContainerReader for FixedReader {
    fn can_read(&self, _: &[u8], extension: &str) -> bool {
        !extension.is_empty()
    }
    fn read(&self, _: &mut dyn RawHeaderSource, _: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        Ok(self.0.clone())
    }
}

struct FakeImage {
    width: i32,
    height: i32,
}

impl DecodedImageView for FakeImage {
    fn pixel_width(&self) -> i32 {
        self.width
    }
    fn pixel_height(&self) -> i32 {
        self.height
    }
    fn downscaled(&self) -> bool {
        false
    }
    fn orientation(&self) -> i32 {
        1
    }
    fn original_width(&self) -> i32 {
        self.width
    }
    fn original_height(&self) -> i32 {
        self.height
    }
    fn estimated_bytes(&self) -> i64 {
        i64::from(self.width) * i64::from(self.height) * 4
    }
    fn platform_image(&self) -> &PlatformImage {
        unreachable!("killers only inspect metadata")
    }
}

#[derive(Clone, Copy)]
enum Fail {
    /// `InvalidData`: recoverable and corrupt.
    Corrupt,
    /// `NotSupported`: recoverable, not corrupt, `is_not_supported()`.
    Unsupported,
    /// `Io`: not recoverable.
    Io,
}

fn make_error(fail: Fail, label: &str) -> DecodeError {
    match fail {
        Fail::Corrupt => DecodeError::InvalidData {
            message: format!("corrupt {label}"),
        },
        Fail::Unsupported => DecodeError::NotSupported {
            message: format!("unsupported {label}"),
            user_text: None,
            inner_hresult: None,
        },
        Fail::Io => DecodeError::Io(io::Error::other(format!("io {label}"))),
    }
}

#[derive(Clone, Copy)]
enum Rule {
    Ok(i32, i32),
    Fail(Fail),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Call {
    marker: Option<u8>,
    len: usize,
    source_orientation: Option<i32>,
}

/// Inner decoder: the rule is chosen by the first byte of `request.bytes`; an unknown or missing marker is a hard `Io` failure.
struct ScriptDecoder {
    rules: HashMap<u8, Rule>,
    calls: Mutex<Vec<Call>>,
}

impl ImageDecoder for ScriptDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let marker = request.bytes.as_ref().and_then(|bytes| bytes.first().copied());
        self.calls.lock().unwrap().push(Call {
            marker,
            len: request.bytes.as_ref().map_or(0, |bytes| bytes.len()),
            source_orientation: request.source_orientation,
        });
        match marker.and_then(|m| self.rules.get(&m)) {
            Some(Rule::Ok(width, height)) => Ok(Arc::new(FakeImage {
                width: *width,
                height: *height,
            })),
            Some(Rule::Fail(fail)) => Err(make_error(*fail, &format!("m{}", marker.unwrap_or(0)))),
            None => Err(DecodeError::Io(io::Error::other("unexpected marker"))),
        }
    }

    fn read_info(&self, _: &str) -> Result<ImageInfo, DecodeError> {
        Err(DecodeError::InvalidOperation {
            message: "not used".into(),
        })
    }
}

struct FullDecoder {
    calls: AtomicUsize,
}

impl ImageDecoder for FullDecoder {
    fn decode(&self, _: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(FakeImage {
            width: 6000,
            height: 4000,
        }))
    }
    fn read_info(&self, _: &str) -> Result<ImageInfo, DecodeError> {
        Err(DecodeError::InvalidOperation {
            message: "not used".into(),
        })
    }
}

enum Thumb {
    /// A thumbnail of this many bytes, every byte `THUMB_MARKER`.
    Bytes(usize),
    Fail(Fail),
}

struct FakeThumbnail {
    plan: Thumb,
    calls: AtomicUsize,
}

impl RawPreviewFallback for FakeThumbnail {
    fn read_jpeg_thumbnail(&self, _: &str, _: RawFormat) -> Result<Vec<u8>, DecodeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.plan {
            Thumb::Bytes(count) => Ok(vec![THUMB_MARKER; *count]),
            Thumb::Fail(fail) => Err(make_error(*fail, "thumb")),
        }
    }
}

/// `(marker, offset, length, width, height)`: the bytes of the file in `[offset, offset + length)` are all `marker`.
type PreviewSpec = (u8, i64, i64, i32, i32);

struct Harness {
    decoder: RawDecoder,
    inner: Arc<ScriptDecoder>,
    full: Option<Arc<FullDecoder>>,
    thumb: Option<Arc<FakeThumbnail>>,
    root: TempRoot,
}

fn harness(
    format: RawFormat,
    previews: &[PreviewSpec],
    rules: &[(u8, Rule)],
    thumb: Option<Thumb>,
    with_full: bool,
) -> Harness {
    let mut file = vec![0_u8; FILE_LEN];
    let mut list = Vec::new();
    for (index, &(marker, offset, length, width, height)) in previews.iter().enumerate() {
        file[offset as usize..(offset + length) as usize].fill(marker);
        list.push(EmbeddedPreview::new(
            index as i32,
            offset,
            length,
            EmbeddedPreviewKind::Jpeg,
            width,
            height,
            PreviewColorSpace::Srgb,
        ));
    }
    let info = RawContainerInfo {
        format,
        sensor_width: 6000,
        sensor_height: 4000,
        orientation: ORIENTATION,
        previews: list,
        exif_blocks: Vec::new(),
    };
    let registry = Arc::new(RawContainerReaderRegistry::with_readers([
        Box::new(FixedReader(info)) as Box<dyn RawContainerReader>
    ]));
    let inner = Arc::new(ScriptDecoder {
        rules: rules.iter().copied().collect(),
        calls: Mutex::new(Vec::new()),
    });
    let full = with_full.then(|| {
        Arc::new(FullDecoder {
            calls: AtomicUsize::new(0),
        })
    });
    let thumb = thumb.map(|plan| {
        Arc::new(FakeThumbnail {
            plan,
            calls: AtomicUsize::new(0),
        })
    });
    let decoder = RawDecoder::with_options(
        inner.clone(),
        RawDecoderOptions {
            source_reader: Some(Arc::new(MemoryReader(Arc::new(file)))),
            registry: Some(registry),
            preview_fallback: thumb.clone().map(|t| t as Arc<dyn RawPreviewFallback>),
            no_preview_decoder: full.clone().map(|f| f as Arc<dyn ImageDecoder>),
            ..Default::default()
        },
    );
    Harness {
        decoder,
        inner,
        full,
        thumb,
        root: TempRoot::new("mut-raw4"),
    }
}

impl Harness {
    fn decode(&self) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let extension = "x.orf";
        self.decoder.decode(&DecodeRequest::with_box(
            self.root.join(extension),
            DecodeBox::new(0, 0),
        ))
    }
    fn calls(&self) -> Vec<Call> {
        self.inner.calls.lock().unwrap().clone()
    }
    fn markers(&self) -> Vec<Option<u8>> {
        self.calls().iter().map(|c| c.marker).collect()
    }
    fn thumb_calls(&self) -> usize {
        self.thumb.as_ref().map_or(0, |t| t.calls.load(Ordering::SeqCst))
    }
    fn full_calls(&self) -> usize {
        self.full.as_ref().map_or(0, |f| f.calls.load(Ordering::SeqCst))
    }
}

fn err_of(result: Result<Arc<dyn DecodedImageView>, DecodeError>) -> DecodeError {
    match result {
        Ok(_) => panic!("expected a decode failure"),
        Err(error) => error,
    }
}

// Preview A is the largest (chosen first), B the next best. Both "viewable" (long side >= 1000).
const A: PreviewSpec = (0xA1, 100, 200, 4000, 3000);
const B: PreviewSpec = (0xB2, 400, 300, 2000, 1500);
const OK_A: (u8, Rule) = (0xA1, Rule::Ok(4000, 3000));
const OK_B: (u8, Rule) = (0xB2, Rule::Ok(2000, 1500));

// fallback.rs:178:36 `>=` -> `<` (the attempt cap makes the very first iteration break) and :191:64 `>` -> `==`/`>=`
// (first-try success must NOT be flagged degraded).
#[test]
fn first_preview_success_is_decoded_once_and_not_degraded() {
    let h = harness(RawFormat::Dng, &[A, B], &[OK_A, OK_B], None, false);
    let image = h.decode().expect("first preview decodes");
    assert_eq!(h.markers(), vec![Some(0xA1)]);
    assert!(!image.is_degraded_fallback());
    assert_eq!(image.embedded_preview_size(), Some((4000, 3000)));
}

// fallback.rs:191:64 `>` -> `<` (a success after a failure must be flagged degraded), :194:31 guard -> `false`
// (a recoverable failure must not propagate), :278:9 select_next_preview -> Ok(None) / Ok(Some(default)).
#[test]
fn recoverable_failure_moves_to_the_next_best_preview_and_is_degraded() {
    let h = harness(
        RawFormat::Dng,
        &[A, B],
        &[(0xA1, Rule::Fail(Fail::Corrupt)), OK_B],
        None,
        false,
    );
    let image = h.decode().expect("second preview decodes");
    assert_eq!(h.markers(), vec![Some(0xA1), Some(0xB2)]);
    assert!(image.is_degraded_fallback());
    assert_eq!(image.embedded_preview_size(), Some((2000, 1500)));
    // The bytes of the failed attempt are charged on top of the accepted one.
    let single = harness(RawFormat::Dng, &[B], &[OK_B], None, false).decode().unwrap();
    assert_eq!(
        image.source_bytes_read().unwrap() - single.source_bytes_read().unwrap(),
        A.2,
        "failed attempt A adds its length to the bytes read"
    );
}

// fallback.rs:194:31 guard -> `true`: a non-recoverable failure (Io) must stop the chain at once.
#[test]
fn non_recoverable_failure_stops_the_chain() {
    let h = harness(
        RawFormat::Dng,
        &[A, B],
        &[(0xA1, Rule::Fail(Fail::Io)), OK_B],
        None,
        false,
    );
    let error = err_of(h.decode());
    assert!(matches!(error.kind(), DecodeError::Io(_)), "got {error:?}");
    assert_eq!(h.markers(), vec![Some(0xA1)]);
}

// fallback.rs:178:36 boundary: exactly MAX_PREVIEW_ATTEMPTS (4) previews are tried, the fifth never is; the FIRST failure wins.
#[test]
fn at_most_four_previews_are_tried_and_the_first_failure_propagates() {
    let sizes = [(5000, 3750), (4500, 3375), (4000, 3000), (3500, 2625), (3000, 2250)];
    let mut previews = Vec::new();
    let mut rules = Vec::new();
    for (index, (width, height)) in sizes.iter().enumerate() {
        let marker = 0x10 + index as u8;
        previews.push((marker, 100 + 200 * index as i64, 100, *width, *height));
        rules.push((marker, Rule::Fail(Fail::Corrupt)));
    }
    let h = harness(RawFormat::Dng, &previews, &rules, None, false);
    let error = err_of(h.decode());
    assert_eq!(
        h.markers(),
        vec![Some(0x10), Some(0x11), Some(0x12), Some(0x13)],
        "four attempts in size order, no fifth"
    );
    assert_eq!(
        error.message(),
        "corrupt m16",
        "the first failure (marker 0x10 = 16) propagates"
    );
}

// fallback.rs:200:60 `==` -> `!=`: the failed long side comes from the preview that failed (A, 4000), not from another
// candidate (thumbnail B, 500). With 4000 the thumbnail is not usable and the failure is not corrupt => no retry.
#[test]
fn unsupported_failure_does_not_retry_a_smaller_thumbnail() {
    let thumb_preview: PreviewSpec = (0xC3, 1000, 100, 500, 333);
    let h = harness(
        RawFormat::Dng,
        &[A, thumb_preview],
        &[(0xA1, Rule::Fail(Fail::Unsupported)), (0xC3, Rule::Ok(500, 333))],
        None,
        false,
    );
    let error = err_of(h.decode());
    assert!(
        matches!(error.kind(), DecodeError::NotSupported { .. }),
        "got {error:?}"
    );
    assert_eq!(
        h.markers(),
        vec![Some(0xA1)],
        "the thumbnail-sized preview is never retried"
    );
}

// A corrupt failure DOES fall back to a thumbnail-sized preview (counterpart of the test above; :278 and the corrupt flag).
#[test]
fn corrupt_failure_falls_back_to_a_thumbnail_sized_preview() {
    let thumb_preview: PreviewSpec = (0xC3, 1000, 100, 500, 333);
    let h = harness(
        RawFormat::Dng,
        &[A, thumb_preview],
        &[(0xA1, Rule::Fail(Fail::Corrupt)), (0xC3, Rule::Ok(500, 333))],
        None,
        false,
    );
    let image = h
        .decode()
        .expect("thumbnail-sized preview used after a corrupt failure");
    assert_eq!(h.markers(), vec![Some(0xA1), Some(0xC3)]);
    assert!(image.is_degraded_fallback());
}

// fallback.rs:224:24 `==` -> `!=`, :360:13 (field `bytes`), :361:13 (field `source_orientation`), :345:9 (whole function):
// ORF + fallback present + unsupported preview => LibRaw thumbnail decoded through the inner decoder.
#[test]
fn orf_unsupported_preview_uses_the_thumbnail_fallback() {
    let h = harness(
        RawFormat::Orf,
        &[A],
        &[
            (0xA1, Rule::Fail(Fail::Unsupported)),
            (THUMB_MARKER, Rule::Ok(160, 120)),
        ],
        Some(Thumb::Bytes(77)),
        false,
    );
    let image = h.decode().expect("thumbnail fallback decodes");
    assert_eq!(h.thumb_calls(), 1);
    let calls = h.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1],
        Call {
            marker: Some(THUMB_MARKER),
            len: 77,
            source_orientation: Some(ORIENTATION)
        },
        "the thumbnail bytes and the container orientation reach the inner decoder"
    );
    assert!(image.is_degraded_fallback());
    assert_eq!(image.embedded_preview_size(), Some((160, 120)));
    // header bytes + failed preview A + thumbnail (77): compare with a plain single-preview decode.
    let single = harness(RawFormat::Orf, &[B], &[OK_B], None, false).decode().unwrap();
    assert_eq!(
        image.source_bytes_read().unwrap() - single.source_bytes_read().unwrap(),
        A.2 + 77 - B.2,
        "failed bytes + thumbnail length are charged"
    );
}

// fallback.rs:224:24 and :224:42 `&&` -> `||`: a non-ORF format never uses the thumbnail fallback.
#[test]
fn thumbnail_fallback_is_not_used_for_non_orf_formats() {
    let h = harness(
        RawFormat::Dng,
        &[A],
        &[
            (0xA1, Rule::Fail(Fail::Unsupported)),
            (THUMB_MARKER, Rule::Ok(160, 120)),
        ],
        Some(Thumb::Bytes(77)),
        false,
    );
    let error = err_of(h.decode());
    assert!(
        matches!(error.kind(), DecodeError::NotSupported { .. }),
        "got {error:?}"
    );
    assert_eq!(h.thumb_calls(), 0);
}

// fallback.rs:224:77 `&&` -> `||`: the last failure must be "unsupported"; a corrupt one does not trigger the fallback.
#[test]
fn thumbnail_fallback_needs_an_unsupported_last_failure() {
    let h = harness(
        RawFormat::Orf,
        &[A],
        &[(0xA1, Rule::Fail(Fail::Corrupt)), (THUMB_MARKER, Rule::Ok(160, 120))],
        Some(Thumb::Bytes(77)),
        false,
    );
    let error = err_of(h.decode());
    assert!(matches!(error.kind(), DecodeError::InvalidData { .. }), "got {error:?}");
    assert_eq!(h.thumb_calls(), 0);
}

// fallback.rs:224:42 / :224:77 `&&` -> `||`: ORF with an unsupported failure but NO fallback returns the first failure,
// not the "thumbnail unavailable" InvalidOperation.
#[test]
fn orf_without_a_fallback_returns_the_first_failure() {
    let h = harness(
        RawFormat::Orf,
        &[A],
        &[(0xA1, Rule::Fail(Fail::Unsupported))],
        None,
        false,
    );
    let error = err_of(h.decode());
    assert!(
        matches!(error.kind(), DecodeError::NotSupported { .. }),
        "got {error:?}"
    );
}

// fallback.rs:242:31 guard -> `false`: a recoverable thumbnail failure falls through to the full decode.
#[test]
fn orf_recoverable_thumbnail_failure_falls_through_to_the_full_decode() {
    let h = harness(
        RawFormat::Orf,
        &[A],
        &[(0xA1, Rule::Fail(Fail::Unsupported))],
        Some(Thumb::Bytes(0)),
        true,
    );
    let image = h.decode().expect("full decode is the last resort");
    assert_eq!(h.thumb_calls(), 1);
    assert_eq!(h.full_calls(), 1);
    assert_eq!(
        image.embedded_preview_size(),
        Some((0, 0)),
        "a full decode reports no embedded preview"
    );
    assert!(!image.is_degraded_fallback());
}

// fallback.rs:242:31 guard -> `true`: a non-recoverable thumbnail failure propagates and the full decode is not tried.
#[test]
fn orf_non_recoverable_thumbnail_failure_propagates() {
    let h = harness(
        RawFormat::Orf,
        &[A],
        &[(0xA1, Rule::Fail(Fail::Unsupported))],
        Some(Thumb::Fail(Fail::Io)),
        true,
    );
    let error = err_of(h.decode());
    assert!(matches!(error.kind(), DecodeError::Io(_)), "got {error:?}");
    assert_eq!(h.full_calls(), 0);
}

// ---------------------------------------------------------------- decode_without_preview (no previews in the container)

// fallback.rs:299:9 whole function, :360/:361 on the no-preview path, :345:9.
#[test]
fn no_preview_container_uses_the_thumbnail_and_counts_its_bytes() {
    let h = harness(
        RawFormat::Dng,
        &[],
        &[(THUMB_MARKER, Rule::Ok(160, 120))],
        Some(Thumb::Bytes(55)),
        false,
    );
    let image = h.decode().expect("thumbnail decodes");
    assert_eq!(
        h.calls(),
        vec![Call {
            marker: Some(THUMB_MARKER),
            len: 55,
            source_orientation: Some(ORIENTATION)
        }]
    );
    assert!(!image.is_degraded_fallback());
    assert_eq!(image.embedded_preview_size(), Some((160, 120)));
    let none = harness(RawFormat::Dng, &[], &[], None, true).decode().unwrap();
    assert_eq!(
        image.source_bytes_read().unwrap() - none.source_bytes_read().unwrap(),
        55,
        "the thumbnail length is charged once"
    );
}

// fallback.rs:317:31 guard -> `false`: a recoverable thumbnail failure falls through to the full decode.
#[test]
fn no_preview_recoverable_thumbnail_failure_falls_through_to_the_full_decode() {
    let h = harness(RawFormat::Dng, &[], &[], Some(Thumb::Fail(Fail::Corrupt)), true);
    let image = h.decode().expect("full decode");
    assert_eq!(h.full_calls(), 1);
    assert_eq!(image.embedded_preview_size(), Some((0, 0)));
}

// fallback.rs:317:31 guard -> `true` and :317:65 `&&` -> `||`: a non-recoverable failure propagates even with a full decoder.
#[test]
fn no_preview_non_recoverable_thumbnail_failure_propagates_despite_a_full_decoder() {
    let h = harness(RawFormat::Dng, &[], &[], Some(Thumb::Fail(Fail::Io)), true);
    let error = err_of(h.decode());
    assert!(matches!(error.kind(), DecodeError::Io(_)), "got {error:?}");
    assert_eq!(h.full_calls(), 0);
}

// fallback.rs:317:31 guard -> `true` and :317:65 `&&` -> `||`: a recoverable failure with NO full decoder propagates as it is
// (not the "full decoder unavailable" InvalidOperation).
#[test]
fn no_preview_recoverable_thumbnail_failure_without_a_full_decoder_propagates() {
    let h = harness(RawFormat::Dng, &[], &[], Some(Thumb::Fail(Fail::Corrupt)), false);
    let error = err_of(h.decode());
    assert_eq!(error.message(), "corrupt thumb");
}

// fallback.rs:352:29 `||` -> `&&`: an empty thumbnail is refused (with the "no preview" sentence on the no-preview path).
#[test]
fn empty_thumbnail_is_refused_with_the_no_preview_text() {
    let h = harness(
        RawFormat::Dng,
        &[],
        &[(THUMB_MARKER, Rule::Ok(1, 1))],
        Some(Thumb::Bytes(0)),
        false,
    );
    let error = err_of(h.decode());
    assert!(matches!(error.kind(), DecodeError::InvalidData { .. }), "got {error:?}");
    assert_eq!(error.message(), "RAW fallback thumbnail size is invalid: 0 bytes.");
    assert_eq!(
        error.describe(),
        "This RAW file has no embedded preview, so it cannot be shown."
    );
    assert!(
        h.calls().is_empty(),
        "an invalid thumbnail never reaches the inner decoder"
    );
}

// fallback.rs:352:44 `>` -> `==` / `<` / `>=`: exactly the limit is accepted, one byte more is refused.
#[test]
fn thumbnail_of_exactly_the_limit_is_accepted_and_one_more_byte_is_refused() {
    let at_limit = harness(
        RawFormat::Dng,
        &[],
        &[(THUMB_MARKER, Rule::Ok(1, 1))],
        Some(Thumb::Bytes(MAX_FALLBACK_THUMBNAIL_BYTES)),
        false,
    );
    at_limit.decode().expect("a thumbnail of exactly 32 MiB is accepted");
    assert_eq!(at_limit.calls().len(), 1);
    assert_eq!(at_limit.calls()[0].len, MAX_FALLBACK_THUMBNAIL_BYTES);

    let over = harness(
        RawFormat::Dng,
        &[],
        &[(THUMB_MARKER, Rule::Ok(1, 1))],
        Some(Thumb::Bytes(MAX_FALLBACK_THUMBNAIL_BYTES + 1)),
        false,
    );
    let error = err_of(over.decode());
    assert_eq!(
        error.message(),
        format!(
            "RAW fallback thumbnail size is invalid: {} bytes.",
            MAX_FALLBACK_THUMBNAIL_BYTES + 1
        )
    );
    assert!(over.calls().is_empty());
}

// ORF path text: an invalid thumbnail on the preview chain carries the "corrupt" sentence (the other `user_text` closure).
#[test]
fn orf_invalid_thumbnail_without_full_decoder_returns_the_first_failure() {
    let h = harness(
        RawFormat::Orf,
        &[A],
        &[(0xA1, Rule::Fail(Fail::Unsupported))],
        Some(Thumb::Bytes(0)),
        false,
    );
    let error = err_of(h.decode());
    assert!(
        matches!(error.kind(), DecodeError::NotSupported { .. }),
        "got {error:?}"
    );
    assert_eq!(h.thumb_calls(), 1);
}

// ---------------------------------------------------------------- preview_selector.rs (shard 0/2 survivors)

mod selector {
    use pr_imaging::decode_types::DecodeBox;
    use pr_imaging_raw::preview_selector::{select_preview, select_preview_resolved, walk_jpeg_header};
    use pr_imaging_raw::{EmbeddedPreview, EmbeddedPreviewKind, InMemoryRawHeaderSource, PreviewColorSpace};

    fn preview(index: i32, length: i64, width: i32, height: i32) -> EmbeddedPreview {
        EmbeddedPreview::new(
            index,
            0,
            length,
            EmbeddedPreviewKind::Jpeg,
            width,
            height,
            PreviewColorSpace::Unknown,
        )
    }

    // preview_selector.rs:185:45 `*` -> `+` in `score`: 500x500 (area 250_000, sum 1000) beats 990x30 (area 29_700, sum 1020);
    // both are tier 0 (long side < 1000), so only the area decides under an unbounded box.
    #[test]
    fn score_ranks_known_sizes_by_area_not_by_width_plus_height() {
        let previews = [preview(0, 10, 990, 30), preview(1, 10, 500, 500)];
        let mut source = InMemoryRawHeaderSource::new(vec![0_u8; 64]);
        let chosen = select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1)
            .unwrap()
            .unwrap();
        assert_eq!(chosen.index, 1);
    }

    /// APP1 EXIF payload whose InteropIndex is "R03" (Adobe RGB), little endian.
    fn adobe_exif() -> Vec<u8> {
        let mut out = b"Exif\0\0II".to_vec();
        let u16le = |out: &mut Vec<u8>, v: u16| out.extend_from_slice(&v.to_le_bytes());
        let u32le = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
        u16le(&mut out, 42);
        u32le(&mut out, 8);
        u16le(&mut out, 1); // IFD0 at 8
        u16le(&mut out, 0x8769);
        u16le(&mut out, 4);
        u32le(&mut out, 1);
        u32le(&mut out, 26);
        u32le(&mut out, 0);
        u16le(&mut out, 1); // Exif IFD at 26
        u16le(&mut out, 0xA005);
        u16le(&mut out, 4);
        u32le(&mut out, 1);
        u32le(&mut out, 44);
        u32le(&mut out, 0);
        u16le(&mut out, 1); // Interop IFD at 44
        u16le(&mut out, 0x0001);
        u16le(&mut out, 2);
        u32le(&mut out, 4);
        out.extend_from_slice(b"R03\0");
        u32le(&mut out, 0);
        out
    }

    fn jpeg_with_adobe_exif(width: u16, height: u16) -> Vec<u8> {
        let exif = adobe_exif();
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        jpeg.extend_from_slice(&u16::try_from(exif.len() + 2).unwrap().to_be_bytes());
        jpeg.extend_from_slice(&exif);
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08]);
        jpeg.extend_from_slice(&height.to_be_bytes());
        jpeg.extend_from_slice(&width.to_be_bytes());
        jpeg.extend_from_slice(&[0x01, 0x01, 0x11, 0x00, 0xFF, 0xD9]);
        jpeg
    }

    // preview_selector.rs:217:33 `!=` -> `==` in `resolve_dimensions`: the colour space found while resolving the size of an
    // unknown-size preview (Adobe RGB APP1 before the SOF) is kept on the resolved copy.
    #[test]
    fn resolving_an_unknown_size_preview_keeps_the_adobe_rgb_colour_space() {
        let jpeg = jpeg_with_adobe_exif(640, 480);
        let previews = [preview(0, jpeg.len() as i64, 0, 0)];
        let mut source = InMemoryRawHeaderSource::new(jpeg);
        let selection = select_preview_resolved(&mut source, &previews, DecodeBox::UNBOUNDED, 1).unwrap();
        let chosen = selection.chosen.unwrap();
        assert_eq!((chosen.width, chosen.height), (640, 480));
        assert_eq!(chosen.color_space, PreviewColorSpace::AdobeRgb);
        assert_eq!(selection.resolved_previews[0].color_space, PreviewColorSpace::AdobeRgb);
    }

    // preview_selector.rs:290:15 `<` -> `==` / `<=`: a range of exactly 4 bytes is read (2 bytes charged to the header budget);
    // the result is the same "no frame", so the budget is the observable.
    #[test]
    fn walk_of_a_four_byte_range_reads_the_soi() {
        let mut source = InMemoryRawHeaderSource::new(vec![0xFF, 0xD8, 0x00, 0x00, 0, 0, 0, 0]);
        let walk = walk_jpeg_header(&mut source, 0, 4, true).unwrap();
        assert_eq!(walk.frame, None);
        assert!(source.total_bytes_read() > 0, "the SOI of a 4-byte range is read");
    }

    // preview_selector.rs:290:19 `||` -> `&&`: a negative offset with a plausible length returns "no frame" instead of reading
    // at the negative offset (which would be ArgumentOutOfRange).
    #[test]
    fn walk_at_a_negative_offset_is_empty_not_an_error() {
        let mut source = InMemoryRawHeaderSource::new(vec![0xFF, 0xD8, 0, 0, 0, 0, 0, 0]);
        let walk = walk_jpeg_header(&mut source, -5, 100, true).expect("no error for a negative offset");
        assert_eq!(walk.frame, None);
        assert_eq!(source.total_bytes_read(), 0);
    }

    // preview_selector.rs:298:33 `<` -> `<=`: a range clamped by the source length to exactly 4 bytes is still read.
    #[test]
    fn walk_of_a_range_clamped_to_four_bytes_reads_the_soi() {
        let mut source = InMemoryRawHeaderSource::new(vec![0, 0, 0, 0, 0xFF, 0xD8, 0x00, 0x00]);
        let walk = walk_jpeg_header(&mut source, 4, 100, true).unwrap();
        assert_eq!(walk.frame, None);
        assert!(source.total_bytes_read() > 0);
    }
}
