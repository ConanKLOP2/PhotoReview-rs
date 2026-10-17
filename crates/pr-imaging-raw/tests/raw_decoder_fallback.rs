//! W5-12b2 fallback-chain integration goldens. Each case uses fake decoders, source reads, and container metadata so
//! the production fallback decisions are exercised without native codecs or access to a user's files.

use std::fs::{self, OpenOptions};
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use pr_core::abstractions::{ReadSeek, SourceReadPriority, SourceReader};
use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodeBox, DecodeRequest, DecodedImageView, ImageInfo, PlatformImage};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging_raw::decoder::{RawDecoder, RawDecoderOptions};
use pr_imaging_raw::registry::RawContainerReaderRegistry;
use pr_imaging_raw::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, PreviewColorSpace, RawContainerInfo, RawContainerReader,
    RawFormat, RawHeaderError, RawHeaderSource, RawPreviewFallback,
};
use pr_test_support::golden;
use serde_json::{Value, json};

#[derive(Default)]
struct IoStats {
    opens: AtomicUsize,
    bytes: AtomicUsize,
    container_reads: AtomicUsize,
}

struct MemoryReader {
    bytes: Arc<[u8]>,
    stats: Arc<IoStats>,
}

impl SourceReader for MemoryReader {
    fn open_source(&self, _: &Path, _: SourceReadPriority, _: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.stats.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(CountingCursor {
            cursor: Cursor::new(Arc::clone(&self.bytes)),
            stats: Arc::clone(&self.stats),
        }))
    }
}

struct CountingCursor {
    cursor: Cursor<Arc<[u8]>>,
    stats: Arc<IoStats>,
}

impl Read for CountingCursor {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = self.cursor.read(output)?;
        self.stats.bytes.fetch_add(count, Ordering::SeqCst);
        Ok(count)
    }
}

impl Seek for CountingCursor {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.cursor.seek(position)
    }
}

struct FixedReader {
    info: Option<RawContainerInfo>,
    stats: Arc<IoStats>,
}

impl RawContainerReader for FixedReader {
    fn can_read(&self, _: &[u8], extension: &str) -> bool {
        self.info.is_some() && !extension.is_empty()
    }

    fn read(&self, _: &mut dyn RawHeaderSource, _: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        self.stats.container_reads.fetch_add(1, Ordering::SeqCst);
        self.info
            .clone()
            .ok_or_else(|| RawHeaderError::invalid_data("no RAW reader"))
    }
}

struct FakeImage {
    width: i32,
    height: i32,
    original_width: i32,
    original_height: i32,
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
        self.original_width
    }
    fn original_height(&self) -> i32 {
        self.original_height
    }
    fn estimated_bytes(&self) -> i64 {
        i64::from(self.width) * i64::from(self.height) * 4
    }
    fn platform_image(&self) -> &PlatformImage {
        unreachable!("golden tests only inspect metadata")
    }
}

#[derive(Default)]
struct DecoderStats {
    preview_lengths: Mutex<Vec<usize>>,
    preview_requests: Mutex<Vec<Value>>,
    full_calls: AtomicUsize,
}

struct FakeDecoder {
    spec: Value,
    stats: Arc<DecoderStats>,
    full: bool,
    physical_path: Option<PathBuf>,
}

impl FakeDecoder {
    fn result(&self, value: &Value) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        if let Some(error) = value.get("throw") {
            return Err(make_error(error));
        }
        let dimensions = value.get("ok").unwrap_or(value);
        let width = dimensions["w"].as_i64().unwrap_or(1) as i32;
        let height = dimensions["h"].as_i64().unwrap_or(1) as i32;
        let original_width = dimensions["original"][0].as_i64().unwrap_or(width as i64) as i32;
        let original_height = dimensions["original"][1].as_i64().unwrap_or(height as i64) as i32;
        Ok(Arc::new(FakeImage {
            width,
            height,
            original_width,
            original_height,
        }))
    }
}

impl ImageDecoder for FakeDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        if self.full {
            self.stats.full_calls.fetch_add(1, Ordering::SeqCst);
            return self.result(&self.spec);
        }
        let length = request.bytes.as_ref().map_or(0, |bytes| bytes.len());
        self.stats.preview_lengths.lock().unwrap().push(length);
        let (first_byte, last_byte) = request.bytes.as_ref().map_or((0, 0), |bytes| {
            (bytes.first().copied().unwrap_or(0), bytes.last().copied().unwrap_or(0))
        });
        self.stats.preview_requests.lock().unwrap().push(json!({
            "length": length,
            "first_byte": first_byte,
            "last_byte": last_byte,
            "box": [request.target_width, request.target_height],
            "apply_orientation": request.apply_orientation,
            "source_orientation": request.source_orientation,
            "priority": format!("{:?}", request.priority),
        }));
        let call_index = self.stats.preview_requests.lock().unwrap().len().saturating_sub(1);
        let first_call = call_index == 0 && self.spec.get("first_call").is_some();
        let rule = if first_call {
            &self.spec["first_call"]
        } else {
            self.spec["by_length"]
                .get(length.to_string())
                .unwrap_or(&self.spec["default"])
        };
        if first_call && rule["touch"] == true {
            let path = self
                .physical_path
                .as_deref()
                .expect("touch requires a test-owned physical file");
            set_modified_time(path, Duration::from_secs(60))?;
        }
        self.result(rule)
    }

    fn read_info(&self, _: &str) -> Result<ImageInfo, DecodeError> {
        Err(DecodeError::InvalidOperation {
            message: "not used".into(),
        })
    }
}

struct PhysicalGoldenFile {
    directory: PathBuf,
    path: PathBuf,
}

impl PhysicalGoldenFile {
    fn new(name: &str, contents: &[u8]) -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!("photoreview-rs-fallback-{}-{id}", std::process::id()));
        fs::create_dir(&directory).expect("create test-owned fallback directory");
        let path = directory.join(name);
        fs::write(&path, contents).expect("write test-owned fallback fixture");
        set_modified_time(&path, Duration::ZERO).expect("set deterministic initial fixture timestamp");
        Self { directory, path }
    }
}

impl Drop for PhysicalGoldenFile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn set_modified_time(path: &Path, offset: Duration) -> io::Result<()> {
    let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000) + offset;
    OpenOptions::new()
        .write(true)
        .open(path)?
        .set_times(fs::FileTimes::new().set_modified(modified))
}

struct FakeThumbnail {
    spec: Value,
    stats: Arc<AtomicUsize>,
}

impl RawPreviewFallback for FakeThumbnail {
    fn read_jpeg_thumbnail(&self, _: &str, _: RawFormat) -> Result<Vec<u8>, DecodeError> {
        self.stats.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.spec.get("throw") {
            return Err(make_error(error));
        }
        let length = self.spec["bytes"].as_u64().unwrap_or(0) as usize;
        Ok(vec![0; length])
    }
}

fn make_error(value: &Value) -> DecodeError {
    let message = value["message"].as_str().unwrap_or("failure").to_owned();
    match value["type"].as_str().unwrap_or("InvalidData") {
        "InvalidData" => DecodeError::InvalidData { message },
        "Format" | "FileFormat" => DecodeError::Format { message },
        "NotSupported" => DecodeError::NotSupported {
            message,
            user_text: None,
            inner_hresult: value.get("inner_hresult").and_then(Value::as_i64).map(|hr| hr as i32),
        },
        "MissingCodec" => DecodeError::missing_codec(),
        "Canceled" => DecodeError::Canceled,
        "OutOfMemory" => DecodeError::OutOfMemory { message },
        "Io" => DecodeError::Io(io::Error::other(message)),
        "InvalidCast" => DecodeError::InvalidCast { message },
        "Com" => DecodeError::Com {
            hresult: parse_hresult(value),
            message,
        },
        other => panic!("unsupported fake error kind {other}"),
    }
}

fn parse_hresult(value: &Value) -> i32 {
    if let Some(value) = value["hresult"].as_i64() {
        return value as i32;
    }
    value["hresult"]
        .as_str()
        .and_then(|h| u32::from_str_radix(h.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0) as i32
}

fn raw_info(value: &Value) -> RawContainerInfo {
    let format = match value["format"].as_str().unwrap_or("Dng") {
        "Orf" => RawFormat::Orf,
        "Nef" => RawFormat::Nef,
        "Raf" => RawFormat::Raf,
        "Arw" => RawFormat::Arw,
        "Cr2" => RawFormat::Cr2,
        "Cr3" => RawFormat::Cr3,
        "Rw2" => RawFormat::Rw2,
        "Dng" => RawFormat::Dng,
        _ => RawFormat::Unknown,
    };
    let sensor = value["sensor"].as_array().expect("sensor pair");
    let previews = value["previews"]
        .as_array()
        .expect("previews")
        .iter()
        .map(|p| EmbeddedPreview {
            index: p["index"].as_i64().unwrap_or(0) as i32,
            offset: p["offset"].as_i64().expect("preview offset"),
            length: p["length"].as_i64().expect("preview length"),
            kind: EmbeddedPreviewKind::Jpeg,
            width: p["width"].as_i64().unwrap_or(0) as i32,
            height: p["height"].as_i64().unwrap_or(0) as i32,
            color_space: match p["color_space"].as_str().unwrap_or("") {
                "Srgb" => PreviewColorSpace::Srgb,
                "AdobeRgb" => PreviewColorSpace::AdobeRgb,
                _ => PreviewColorSpace::Unknown,
            },
            header_resolved: false,
        })
        .collect();
    let exif_blocks = value["exif_blocks"]
        .as_array()
        .expect("exif blocks")
        .iter()
        .map(|b| ExifBlock {
            offset: b["offset"].as_i64().unwrap_or(0),
            length: b["length"].as_i64().unwrap_or(0),
            is_tiff_header: b["tiff"].as_bool().unwrap_or(false),
            ifd_is_exif: b["ifd_is_exif"].as_bool().unwrap_or(false),
        })
        .collect();
    RawContainerInfo {
        format,
        sensor_width: sensor[0].as_i64().unwrap_or(0) as i32,
        sensor_height: sensor[1].as_i64().unwrap_or(0) as i32,
        orientation: value["orientation"].as_i64().unwrap_or(1) as i32,
        previews,
        exif_blocks,
    }
}

fn fixture_bytes(file: &Value) -> Arc<[u8]> {
    let mut bytes = vec![0; file["size"].as_u64().unwrap_or(0) as usize];
    if let Some(segments) = file["segments"].as_array() {
        for segment in segments {
            let offset = segment["at"].as_u64().expect("segment offset") as usize;
            let hex = segment["hex"].as_str().expect("segment hex").as_bytes();
            for (index, pair) in hex.chunks_exact(2).enumerate() {
                let digit = |b: u8| match b {
                    b'0'..=b'9' => b - b'0',
                    b'a'..=b'f' => b - b'a' + 10,
                    b'A'..=b'F' => b - b'A' + 10,
                    _ => panic!("bad hex"),
                };
                if offset + index >= bytes.len() {
                    bytes.resize(offset + index + 1, 0);
                }
                bytes[offset + index] = digit(pair[0]) * 16 + digit(pair[1]);
            }
        }
    }
    Arc::from(bytes)
}

fn error_type(error: &DecodeError) -> &'static str {
    match error.kind() {
        DecodeError::InvalidData { .. } => "InvalidDataException",
        DecodeError::NotSupported { .. } | DecodeError::MissingCodec { .. } => "NotSupportedException",
        DecodeError::Io(io) if io.kind() == io::ErrorKind::UnexpectedEof => "EndOfStreamException",
        DecodeError::Io(_) => "IOException",
        DecodeError::Canceled => "OperationCanceledException",
        DecodeError::OutOfMemory { .. } => "OutOfMemoryException",
        DecodeError::Format { .. } => "FileFormatException",
        DecodeError::Com { .. } => "COMException",
        DecodeError::InvalidCast { .. } => "InvalidCastException",
        _ => "Other",
    }
}

fn run_case(case: &Value, expected: &Value) {
    let name = case["name"].as_str().expect("name");
    let bytes = fixture_bytes(&case["file"]);
    let io_stats = Arc::new(IoStats::default());
    let decoder_stats = Arc::new(DecoderStats::default());
    let thumbnail_calls = Arc::new(AtomicUsize::new(0));
    let registry = if case["container"]["kind"] == "default" {
        None
    } else {
        let info = raw_info(&case["container"]["info"]);
        Some(Arc::new(RawContainerReaderRegistry::with_readers([
            Box::new(FixedReader {
                info: Some(info),
                stats: Arc::clone(&io_stats),
            }) as Box<dyn RawContainerReader>,
        ])))
    };
    let physical_file = if case["file"]["physical"] == true {
        let filename = case["file"]["name"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{name}.dng"));
        Some(PhysicalGoldenFile::new(
            &filename,
            fixture_bytes(&case["file"]).as_ref(),
        ))
    } else {
        None
    };
    let path = physical_file
        .as_ref()
        .map_or_else(
            || PathBuf::from(format!(r"C:\raw-fallback-fixtures\{name}.dng")),
            |file| file.path.clone(),
        )
        .to_string_lossy()
        .into_owned();
    // The scripted inner decoder changes this test-owned file's timestamp after its first preview read.
    let inner = Arc::new(FakeDecoder {
        spec: case["inner"].clone(),
        stats: Arc::clone(&decoder_stats),
        full: false,
        physical_path: physical_file.as_ref().map(|file| file.path.clone()),
    });
    let decoder = RawDecoder::with_options(
        inner,
        RawDecoderOptions {
            source_reader: Some(Arc::new(MemoryReader {
                bytes,
                stats: Arc::clone(&io_stats),
            })),
            registry,
            preview_fallback: case.get("fallback").filter(|v| !v.is_null()).map(|spec| {
                Arc::new(FakeThumbnail {
                    spec: spec.clone(),
                    stats: Arc::clone(&thumbnail_calls),
                }) as Arc<dyn RawPreviewFallback>
            }),
            no_preview_decoder: case.get("full").filter(|v| !v.is_null()).map(|spec| {
                Arc::new(FakeDecoder {
                    spec: spec.clone(),
                    stats: Arc::clone(&decoder_stats),
                    full: true,
                    physical_path: None,
                }) as Arc<dyn ImageDecoder>
            }),
            ..Default::default()
        },
    );
    let ops = case["ops"].as_array().expect("ops");
    let steps = expected["steps"].as_array().expect("steps");
    assert_eq!(ops.len(), steps.len(), "{name}: steps align");
    for (index, (op, step)) in ops.iter().zip(steps).enumerate() {
        assert_eq!(op["op"], "decode", "{name}: supported op {index}");
        let width = op["box"]["width"]
            .as_i64()
            .or_else(|| op["box"][0].as_i64())
            .unwrap_or(0) as i32;
        let height = op["box"]["height"]
            .as_i64()
            .or_else(|| op["box"][1].as_i64())
            .unwrap_or(0) as i32;
        let prior_preview_attempts = decoder_stats.preview_requests.lock().unwrap().len();
        let request = DecodeRequest::with_box(&path, DecodeBox::new(width, height));
        let result = decoder.decode(&request);
        if let Some(want) = step.get("error") {
            let error = match result {
                Err(error) => error,
                Ok(_) => panic!("{name} step {index}: golden expects error"),
            };
            assert_eq!(want["type"], error_type(&error), "{name} step {index}: {error:?}");
            // `DecodeError::Canceled` is a unit variant, so unlike the C# fake exception it cannot retain a custom message.
            if !matches!(error.kind(), DecodeError::Canceled) {
                assert_eq!(
                    want["message"],
                    error.message().replace(&path, "{path}"),
                    "{name} step {index}"
                );
            }
        } else {
            let image = result.expect("golden expects decoded image");
            assert_eq!(
                step["ok"]["pixel"],
                json!([image.pixel_width(), image.pixel_height()]),
                "{name} step {index}"
            );
            assert_eq!(
                step["ok"]["original"],
                json!([image.original_width(), image.original_height()]),
                "{name} step {index}"
            );
            assert_eq!(
                step["ok"]["downscaled"],
                json!(image.downscaled()),
                "{name} step {index}"
            );
            assert_eq!(
                step["ok"]["orientation"],
                json!(image.orientation()),
                "{name} step {index}"
            );
            assert_eq!(
                step["ok"]["source_bytes_read"],
                json!(image.source_bytes_read().unwrap_or(0)),
                "{name} step {index}"
            );
            assert_eq!(
                step["ok"]["estimated_bytes"],
                json!(image.estimated_bytes()),
                "{name} step {index}"
            );
            assert_eq!(
                step["ok"]["degraded"],
                json!(image.is_degraded_fallback()),
                "{name} step {index}"
            );
            assert_eq!(
                step["ok"]["embedded_preview"],
                json!(image.embedded_preview_size().unwrap_or((0, 0))),
                "{name} step {index}"
            );
        }
        assert_eq!(
            step["fallback_calls"],
            json!(thumbnail_calls.load(Ordering::SeqCst)),
            "{name} step {index}"
        );
        assert_eq!(
            step["full_calls"],
            json!(decoder_stats.full_calls.load(Ordering::SeqCst)),
            "{name} step {index}"
        );
        assert_eq!(
            step["opens"],
            json!(io_stats.opens.load(Ordering::SeqCst)),
            "{name} step {index}"
        );
        assert_eq!(
            step["stream_bytes_read"],
            json!(io_stats.bytes.load(Ordering::SeqCst)),
            "{name} step {index}"
        );
        assert_eq!(
            step["container_reads_total"],
            json!(io_stats.container_reads.load(Ordering::SeqCst)),
            "{name} step {index}"
        );
        let expected_requests = step["inner_requests"].as_array().expect("inner request trace");
        let observed_requests = decoder_stats.preview_requests.lock().unwrap();
        assert_eq!(
            expected_requests,
            &observed_requests[prior_preview_attempts..],
            "{name} step {index}: full inner DecodeRequest trace"
        );
        // The oracle records source opens and stream bytes per operation; container reads remain cumulative.
        io_stats.opens.store(0, Ordering::SeqCst);
        io_stats.bytes.store(0, Ordering::SeqCst);
    }
}

#[test]
fn golden_raw_decoder_fallback_runs_virtual_fixed_container_cases() {
    let root = golden::repo_root();
    let input: Value =
        serde_json::from_slice(&std::fs::read(root.join("tests/golden-inputs/raw-decoder-fallback.json")).unwrap())
            .unwrap();
    let expected = golden::load("raw-decoder-fallback");
    let cases = input["cases"].as_array().expect("input cases");
    assert_eq!(cases.len(), 75);
    assert_eq!(expected.cases.len(), 75);
    let mut ran = 0;
    let mut gated = Vec::new();
    for (case, expected_case) in cases.iter().zip(&expected.cases) {
        assert_eq!(case["name"], expected_case["name"], "input/golden ordering");
        // Default-registry cases use the real registry against tiny checked-in TIFF fixtures.
        if case["container"]["kind"] != "fixed" && case["container"]["kind"] != "default" {
            gated.push(case["name"].as_str().unwrap_or("?"));
            continue;
        }
        run_case(case, expected_case);
        ran += 1;
    }
    eprintln!("raw-decoder-fallback: ran {ran}/75 cases; gated {gated:?}");
    assert!(
        ran == 75,
        "expected broad fallback integration coverage, got {ran}; gated {}",
        gated.len()
    );
}
