//! W5-12b1 core tests. The golden driver executes `read_info` and decode steps that fail before an inner image decoder
//! is needed. Image-producing decode cases need the scripted decoder seam; physical identity and stateful-hook cases
//! also need the C# fixture behavior and stay explicitly gated. Physical cache goldens keep a counting reader on
//! RawDecoder while SourceBytesCache uses its default PhysicalSourceReader, matching the C# fixture composition.

use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use pr_core::abstractions::{ReadSeek, SourceReadPriority, SourceReader};
use pr_core::model::enums::DecoderBackend;
use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodeBox, DecodeRequest, DecodedImageView, ImageInfo, PlatformImage};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::exif_summary::ExifSummary;
use pr_imaging_raw::decoder::RawDecoder;
use pr_imaging_raw::registry::RawContainerReaderRegistry;
use pr_imaging_raw::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, PreviewColorSpace, RawContainerInfo, RawContainerReader,
    RawFormat, RawHeaderError, RawHeaderSource,
};
use pr_test_support::golden;
use serde_json::{Value, json};

struct NeverCalledDecoder;

impl ImageDecoder for NeverCalledDecoder {
    fn decode(&self, _: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        panic!("this core-only golden operation must not invoke an image decoder")
    }

    fn read_info(&self, _: &str) -> Result<ImageInfo, DecodeError> {
        panic!("this core-only golden operation must not invoke an image decoder")
    }
}

struct ScriptedImageDecoder {
    result: Value,
    requests: Arc<std::sync::Mutex<Vec<Value>>>,
}

struct ScriptedImage {
    width: i32,
    height: i32,
    original_width: i32,
    original_height: i32,
    downscaled: bool,
    backend: DecoderBackend,
    degraded: bool,
    exif: Option<ExifSummary>,
}

impl DecodedImageView for ScriptedImage {
    fn pixel_width(&self) -> i32 {
        self.width
    }
    fn pixel_height(&self) -> i32 {
        self.height
    }
    fn downscaled(&self) -> bool {
        self.downscaled
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
        unreachable!("golden tests inspect image metadata only")
    }
    fn actual_backend(&self) -> DecoderBackend {
        self.backend
    }
    fn is_degraded_fallback(&self) -> bool {
        self.degraded
    }
    fn exif(&self) -> Option<&ExifSummary> {
        self.exif.as_ref()
    }
}

impl ImageDecoder for ScriptedImageDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let bytes = request.bytes.as_deref().unwrap_or_default();
        self.requests.lock().unwrap().push(json!({
            "length": bytes.len(),
            "first_byte": bytes.first().copied().unwrap_or(0),
            "last_byte": bytes.last().copied().unwrap_or(0),
            "box": [request.target_width, request.target_height],
            "apply_orientation": request.apply_orientation,
            "source_orientation": request.source_orientation,
            "priority": format!("{:?}", request.priority),
        }));
        let result = self.result.get("ok").unwrap_or(&self.result);
        let backend = match result["backend"].as_str().unwrap_or("Wpf") {
            "WicDirect" => DecoderBackend::WicDirect,
            "TurboJpeg" => DecoderBackend::TurboJpeg,
            "LibRaw" => DecoderBackend::LibRaw,
            _ => DecoderBackend::Wpf,
        };
        let exif = result["exif_make"].as_str().map(|make| ExifSummary {
            camera_make: Some(make.to_owned()),
            ..Default::default()
        });
        Ok(Arc::new(ScriptedImage {
            width: result["w"].as_i64().unwrap_or(1) as i32,
            height: result["h"].as_i64().unwrap_or(1) as i32,
            original_width: result["original"][0]
                .as_i64()
                .unwrap_or_else(|| result["w"].as_i64().unwrap_or(1)) as i32,
            original_height: result["original"][1]
                .as_i64()
                .unwrap_or_else(|| result["h"].as_i64().unwrap_or(1)) as i32,
            downscaled: result["downscaled"].as_bool().unwrap_or(false),
            backend,
            degraded: result["degraded"].as_bool().unwrap_or(false),
            exif,
        }))
    }

    fn read_info(&self, _: &str) -> Result<ImageInfo, DecodeError> {
        Err(DecodeError::InvalidOperation {
            message: "not used".into(),
        })
    }
}

#[derive(Default)]
struct ReadStats {
    opens: AtomicUsize,
    bytes: AtomicUsize,
}

struct MemorySourceReader {
    bytes: Arc<[u8]>,
    fail_reads_with: Option<String>,
    stats: Arc<ReadStats>,
}

impl SourceReader for MemorySourceReader {
    fn open_source(&self, _: &Path, _: SourceReadPriority, _: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.stats.opens.fetch_add(1, Ordering::SeqCst);
        let stream: Box<dyn ReadSeek> = match &self.fail_reads_with {
            Some(message) => Box::new(FaultyStream {
                length: self.bytes.len() as u64,
                message: message.clone(),
            }),
            None => Box::new(CountingCursor {
                cursor: Cursor::new(Arc::clone(&self.bytes)),
                stats: Arc::clone(&self.stats),
            }),
        };
        Ok(stream)
    }
}

struct CountingCursor {
    cursor: Cursor<Arc<[u8]>>,
    stats: Arc<ReadStats>,
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

struct FaultyStream {
    length: u64,
    message: String,
}

impl Read for FaultyStream {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other(self.message.clone()))
    }
}

impl Seek for FaultyStream {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        match position {
            SeekFrom::Start(offset) if offset <= self.length => Ok(offset),
            SeekFrom::End(offset) if offset <= 0 && offset.unsigned_abs() <= self.length => {
                Ok(self.length - offset.unsigned_abs())
            }
            SeekFrom::Current(0) => Ok(0),
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "seek outside fake source")),
        }
    }
}

#[derive(Clone)]
enum ReaderResult {
    Fixed(RawContainerInfo),
    Throws(String),
    None,
}

struct GoldenReader {
    result: ReaderResult,
    reads: Arc<AtomicUsize>,
}

impl RawContainerReader for GoldenReader {
    fn can_read(&self, _: &[u8], extension: &str) -> bool {
        !matches!(self.result, ReaderResult::None) && extension.eq_ignore_ascii_case(".dng")
    }

    fn read(&self, _: &mut dyn RawHeaderSource, _: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        match &self.result {
            ReaderResult::Fixed(info) => Ok(info.clone()),
            ReaderResult::Throws(message) => Err(RawHeaderError::invalid_data(message.clone())),
            ReaderResult::None => Err(RawHeaderError::invalid_data("unreachable empty reader")),
        }
    }
}

fn raw_info(value: &Value) -> RawContainerInfo {
    let format = match value["format"].as_str().unwrap_or("Dng") {
        "Cr2" => RawFormat::Cr2,
        "Cr3" => RawFormat::Cr3,
        "Nef" => RawFormat::Nef,
        "Nrw" => RawFormat::Nrw,
        "Arw" => RawFormat::Arw,
        "Dng" => RawFormat::Dng,
        "Raf" => RawFormat::Raf,
        "Orf" => RawFormat::Orf,
        "Rw2" => RawFormat::Rw2,
        "Pef" => RawFormat::Pef,
        _ => RawFormat::Unknown,
    };
    let pair = |field: &str| {
        let values = value[field].as_array().expect("golden size pair");
        (
            values[0].as_i64().expect("size side") as i32,
            values[1].as_i64().expect("size side") as i32,
        )
    };
    let (sensor_width, sensor_height) = pair("sensor");
    let previews = value["previews"]
        .as_array()
        .expect("previews")
        .iter()
        .map(|preview| EmbeddedPreview {
            index: preview["index"].as_i64().expect("index") as i32,
            offset: preview["offset"].as_i64().expect("offset"),
            length: preview["length"].as_i64().expect("length"),
            kind: EmbeddedPreviewKind::Jpeg,
            width: preview["width"].as_i64().expect("width") as i32,
            height: preview["height"].as_i64().expect("height") as i32,
            color_space: match preview["color_space"].as_str().unwrap_or("Unknown") {
                "Srgb" => PreviewColorSpace::Srgb,
                "AdobeRgb" => PreviewColorSpace::AdobeRgb,
                _ => PreviewColorSpace::Unknown,
            },
            header_resolved: false,
        })
        .collect();
    let exif_blocks = value["exif_blocks"]
        .as_array()
        .expect("EXIF blocks")
        .iter()
        .map(|block| ExifBlock {
            offset: block["offset"].as_i64().expect("EXIF offset"),
            length: block["length"].as_i64().expect("EXIF length"),
            is_tiff_header: block["tiff"].as_bool().unwrap_or(false),
            ifd_is_exif: block["ifd_is_exif"].as_bool().unwrap_or(false),
        })
        .collect();
    RawContainerInfo {
        format,
        sensor_width,
        sensor_height,
        orientation: value["orientation"].as_i64().expect("orientation") as i32,
        previews,
        exif_blocks,
    }
}

fn file_bytes(case: &Value) -> Arc<[u8]> {
    let length = case["file"]["zero_length"]
        .as_u64()
        .or_else(|| case["file"]["size"].as_u64())
        .unwrap_or(0) as usize;
    let mut bytes = vec![0; length];
    if let Some(segments) = case["file"]["segments"].as_array() {
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
                let at = offset + index;
                if at >= bytes.len() {
                    bytes.resize(at + 1, 0);
                }
                bytes[at] = digit(pair[0]) * 16 + digit(pair[1]);
            }
        }
    }
    Arc::from(bytes)
}

fn error_type(error: &DecodeError) -> &'static str {
    match error.kind() {
        DecodeError::InvalidData { .. } => "InvalidDataException",
        DecodeError::NotSupported { .. } => "NotSupportedException",
        DecodeError::Io(io) if io.kind() == io::ErrorKind::UnexpectedEof => "EndOfStreamException",
        DecodeError::Io(_) => "IOException",
        DecodeError::InvalidArgument { .. } => "ArgumentException",
        DecodeError::Canceled => "OperationCanceledException",
        _ => "Other",
    }
}

fn localized_key(error: &DecodeError) -> Value {
    if !error.is_localized() {
        return Value::Null;
    }
    let message = error.describe();
    if message.starts_with("Corrupt or invalid RAW") {
        json!("image.error.rawCorrupt")
    } else if message.starts_with("Unsupported RAW container") {
        json!("image.error.rawUnsupported")
    } else if message.starts_with("This RAW file has no embedded preview") {
        json!("image.error.rawNoPreview")
    } else if message.starts_with("The file changed while it was being read:") {
        json!("err.io.fileChangedWhileReading")
    } else {
        Value::Null
    }
}

fn expected_info_matches(info: ImageInfo, expected: &Value) -> bool {
    expected["pixel"] == json!([info.pixel_width, info.pixel_height])
        && expected["orientation"] == json!(info.orientation)
        && expected["size"] == json!([info.width(), info.height()])
}

fn decode_fails_before_inner(case: &Value) -> bool {
    let kind = case["container"]["kind"].as_str().unwrap_or("none");
    matches!(kind, "none" | "throws")
        || case["file"]["faulty_read"].is_string()
        || matches!(
            case["name"].as_str(),
            Some(
                "unsupported-default-registry-3-bytes"
                    | "malformed-default-registry-12-bytes"
                    | "range-past-end"
                    | "range-negative-offset"
                    | "range-longer-than-cap"
                    | "range-adobe-rgb-not-a-jpeg"
            )
        )
}

fn run_read_info_case(input: &Value, expected: &Value, path: &str) -> (usize, usize) {
    let file = file_bytes(input);
    let stats = Arc::new(ReadStats::default());
    let reader_reads = Arc::new(AtomicUsize::new(0));
    let source_reader = Arc::new(MemorySourceReader {
        bytes: file,
        fail_reads_with: input["file"]["faulty_read"].as_str().map(str::to_owned),
        stats: Arc::clone(&stats),
    });
    let result = match input["container"]["kind"].as_str().unwrap_or("none") {
        "fixed" => ReaderResult::Fixed(raw_info(&input["container"]["info"])),
        "throws" => ReaderResult::Throws(
            input["container"]["message"]
                .as_str()
                .unwrap_or("RAW header read exceeded hard limit.")
                .to_owned(),
        ),
        "none" => ReaderResult::None,
        "default" => ReaderResult::None,
        other => panic!("core golden harness does not support container kind {other}"),
    };
    let registry = if input["container"]["kind"] == "default" && input["file"]["faulty_read"].is_null() {
        Arc::new(RawContainerReaderRegistry::new())
    } else {
        match result {
            ReaderResult::None => Arc::new(RawContainerReaderRegistry::with_readers(Vec::<
                Box<dyn RawContainerReader>,
            >::new())),
            result => Arc::new(RawContainerReaderRegistry::with_readers([Box::new(GoldenReader {
                result,
                reads: Arc::clone(&reader_reads),
            })
                as Box<dyn RawContainerReader>])),
        }
    };
    let decoder = RawDecoder::with_options(
        Arc::new(NeverCalledDecoder),
        pr_imaging_raw::decoder::RawDecoderOptions {
            source_reader: Some(source_reader),
            registry: Some(registry),
            ..Default::default()
        },
    );
    let name = input["name"].as_str().expect("case name");
    let ops = input["ops"].as_array().expect("ops");
    let steps = expected["steps"].as_array().expect("golden steps");
    let mut read_info_steps = 0;
    let mut executed_steps = 0;
    for (index, op) in ops.iter().enumerate() {
        let is_read_info = op["op"] == "read_info";
        let is_safe_early_decode = op["op"] == "decode" && decode_fails_before_inner(input);
        if !is_read_info && !is_safe_early_decode {
            continue;
        }
        let before_opens = stats.opens.load(Ordering::SeqCst);
        let before_bytes = stats.bytes.load(Ordering::SeqCst);
        let before_reads = reader_reads.load(Ordering::SeqCst);
        let step = &steps[index];
        assert_eq!(step["op"], op["op"], "case {name}: golden op alignment");
        let error = if is_read_info {
            match decoder.read_info(path) {
                Ok(info) => {
                    assert!(
                        expected_info_matches(info, &step["info"]),
                        "case {name}, step {index}: {info:?} != {}",
                        step["info"]
                    );
                    None
                }
                Err(error) => Some(error),
            }
        } else {
            let width = op["box"]["width"].as_i64().unwrap_or(0) as i32;
            let request = DecodeRequest::new(path, width);
            match decoder.decode(&request) {
                Ok(_) => panic!("case {name}, step {index}: early container error expected"),
                Err(error) => Some(error),
            }
        };
        if let Some(error) = error {
            assert_eq!(
                step["error"]["type"],
                error_type(&error),
                "case {name}, step {index}: {error:?}"
            );
            let message = error.message().replace(path, "{path}");
            assert_eq!(step["error"]["message"], message, "case {name}, step {index}");
            assert_eq!(
                step["error"]["localized_key"],
                localized_key(&error),
                "case {name}, step {index}"
            );
        }
        assert_eq!(
            step["opens"],
            json!(stats.opens.load(Ordering::SeqCst) - before_opens),
            "case {name}, step {index}"
        );
        assert_eq!(
            step["stream_bytes_read"],
            json!(stats.bytes.load(Ordering::SeqCst) - before_bytes),
            "case {name}, step {index}"
        );
        assert_eq!(
            step["container_reads_total"],
            json!(reader_reads.load(Ordering::SeqCst)),
            "case {name}, step {index}"
        );
        assert!(
            reader_reads.load(Ordering::SeqCst) - before_reads <= 1,
            "one lookup must invoke at most one container read"
        );
        if is_read_info {
            read_info_steps += 1;
        }
        executed_steps += 1;
    }
    (read_info_steps, executed_steps)
}

static NEXT_PHYSICAL_GOLDEN_ID: AtomicU64 = AtomicU64::new(0);

/// A test-owned directory; Drop removes it even when a golden assertion unwinds.
struct PhysicalGoldenFile {
    directory: PathBuf,
    path: PathBuf,
}

impl PhysicalGoldenFile {
    fn new(name: &str, contents: &[u8]) -> Self {
        let id = NEXT_PHYSICAL_GOLDEN_ID.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!("photoreview-rs-raw-golden-{}-{id}", std::process::id()));
        std::fs::create_dir(&directory).expect("create test-owned temp directory");
        let path = directory.join(name);
        std::fs::write(&path, contents).expect("write test-owned physical golden file");
        Self { directory, path }
    }

    fn path_string(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for PhysicalGoldenFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn run_image_decode_case(input: &Value, expected: &Value, path: &str) {
    let name = input["name"].as_str().expect("case name");
    let bytes = file_bytes(input);
    let stats = Arc::new(ReadStats::default());
    let reader_reads = Arc::new(AtomicUsize::new(0));
    let source_reader = Arc::new(MemorySourceReader {
        bytes,
        fail_reads_with: None,
        stats: Arc::clone(&stats),
    });
    let container = &input["container"];
    let registry = if container["kind"] == "default" {
        Arc::new(RawContainerReaderRegistry::new())
    } else {
        Arc::new(RawContainerReaderRegistry::with_readers([Box::new(GoldenReader {
            result: ReaderResult::Fixed(raw_info(&container["info"])),
            reads: Arc::clone(&reader_reads),
        })
            as Box<dyn RawContainerReader>]))
    };
    let request_log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let inner = Arc::new(ScriptedImageDecoder {
        result: input["inner"]["default"].clone(),
        requests: Arc::clone(&request_log),
    });
    let source_bytes_cache = input["cache"]["capacity"].as_i64().map(|capacity| {
        Arc::new(
            pr_imaging::cache::source_bytes::SourceBytesCache::new(capacity, 0, None)
                .expect("positive golden cache capacity"),
        )
    });
    let decoder = RawDecoder::with_options(
        inner,
        pr_imaging_raw::decoder::RawDecoderOptions {
            source_reader: Some(source_reader),
            registry: Some(registry),
            source_bytes_cache: source_bytes_cache.clone(),
            ..Default::default()
        },
    );
    for (index, op) in input["ops"].as_array().expect("decode operations").iter().enumerate() {
        let opens_before = stats.opens.load(Ordering::SeqCst);
        let bytes_before = stats.bytes.load(Ordering::SeqCst);
        let request_count_before = request_log.lock().expect("request log lock").len();
        let mut request = DecodeRequest::with_box(
            path,
            DecodeBox::new(
                op["box"][0].as_i64().unwrap_or(0) as i32,
                op["box"][1].as_i64().unwrap_or(0) as i32,
            ),
        )
        .with_apply_orientation(op["apply_orientation"].as_bool().unwrap_or(true));
        if let Some(orientation) = op["source_orientation"].as_i64() {
            request = request.with_source_orientation(orientation as i32);
        }
        if op["priority"].as_str() == Some("Preload") {
            request = request.with_priority(SourceReadPriority::Preload);
        }

        let image = decoder
            .decode(&request)
            .unwrap_or_else(|error| panic!("case {name}, step {index}: {error:?}"));
        let step = &expected["steps"][index];
        assert_eq!(step["op"], "decode", "case {name}");
        let info = &step["ok"];
        assert_eq!(
            [image.pixel_width(), image.pixel_height()],
            [
                info["pixel"][0].as_i64().unwrap() as i32,
                info["pixel"][1].as_i64().unwrap() as i32
            ],
            "case {name}: pixel size"
        );
        assert_eq!(
            [image.original_width(), image.original_height()],
            [
                info["original"][0].as_i64().unwrap() as i32,
                info["original"][1].as_i64().unwrap() as i32
            ],
            "case {name}: original size"
        );
        assert_eq!(
            image.downscaled(),
            info["downscaled"].as_bool().unwrap(),
            "case {name}: downscaled"
        );
        assert_eq!(
            image.orientation(),
            info["orientation"].as_i64().unwrap() as i32,
            "case {name}: orientation"
        );
        assert_eq!(
            image.is_degraded_fallback(),
            info["degraded"].as_bool().unwrap(),
            "case {name}: degraded"
        );
        assert_eq!(
            image.estimated_bytes(),
            info["estimated_bytes"].as_i64().unwrap(),
            "case {name}: estimated bytes"
        );
        assert_eq!(
            image.source_bytes_read(),
            Some(info["source_bytes_read"].as_i64().unwrap()),
            "case {name}: source bytes"
        );
        assert_eq!(
            image.embedded_preview_size(),
            Some((
                info["embedded_preview"][0].as_i64().unwrap() as i32,
                info["embedded_preview"][1].as_i64().unwrap() as i32
            )),
            "case {name}: embedded size"
        );
        assert_eq!(
            format!("{:?}", image.actual_backend()),
            info["backend"].as_str().unwrap(),
            "case {name}: backend"
        );
        let exif_make = image.exif().and_then(|exif| exif.camera_make.as_deref());
        assert_eq!(exif_make, info["exif_make"].as_str(), "case {name}: EXIF make");
        assert_eq!(
            json!(request_log.lock().unwrap()[request_count_before..]),
            step["inner_requests"],
            "case {name}: inner request trace"
        );
        assert_eq!(step["fallback_calls"], 0, "case {name}: fallback calls");
        assert_eq!(step["full_calls"], 0, "case {name}: full decode calls");
        assert_eq!(
            step["opens"],
            json!(stats.opens.load(Ordering::SeqCst) - opens_before),
            "case {name}: opens"
        );
        assert_eq!(
            step["stream_bytes_read"],
            json!(stats.bytes.load(Ordering::SeqCst) - bytes_before),
            "case {name}: source reads"
        );
        assert_eq!(
            step["container_reads_total"],
            json!(reader_reads.load(Ordering::SeqCst)),
            "case {name}: container reads"
        );
        if let Some(cache) = &source_bytes_cache {
            assert_eq!(step["cache_count"], json!(cache.count()), "case {name}: cache count");
            assert_eq!(
                step["cache_size"],
                json!(cache.current_size()),
                "case {name}: cache size"
            );
        }
    }
}

#[test]
fn golden_raw_decoder_core_runs_core_only_read_info_cases() {
    let root = golden::repo_root();
    let inputs: Value =
        serde_json::from_slice(&std::fs::read(root.join("tests/golden-inputs/raw-decoder-core.json")).unwrap())
            .unwrap();
    let expected = golden::load("raw-decoder-core");
    let input_cases = inputs["cases"].as_array().expect("input cases");
    assert_eq!(input_cases.len(), 50, "the C# core package has 50 cases");
    assert_eq!(expected.cases.len(), 50, "the expected core golden has 50 cases");
    let mut read_info_steps = 0usize;
    let mut executed_steps = 0usize;
    let mut gated_cases = Vec::new();
    let mut gated_steps = Vec::new();
    let mut image_decode_cases = 0usize;
    for (input, golden_case) in input_cases.iter().zip(&expected.cases) {
        assert_eq!(input["name"], golden_case["name"], "input/golden case order");
        let ops = input["ops"].as_array().expect("operations");
        let physical_info_case =
            input["file"]["physical"].as_bool().unwrap_or(false) && ops.iter().all(|op| op["op"] == "read_info");
        let physical_cache_case = input["file"]["physical"].as_bool().unwrap_or(false)
            && input["cache"]["capacity"].as_i64().is_some()
            && ops.iter().all(|op| op["op"] == "decode")
            && !ops.is_empty()
            && golden_case["steps"]
                .as_array()
                .is_some_and(|steps| steps.len() == ops.len() && steps.iter().all(|step| step["ok"].is_object()));
        let physical_file = (physical_info_case || physical_cache_case).then(|| {
            PhysicalGoldenFile::new(
                input["file"]["name"].as_str().unwrap_or("golden.dng"),
                file_bytes(input).as_ref(),
            )
        });
        let virtual_path = format!(
            r"C:\raw-virtual-nonexistent\{}.dng",
            input["name"].as_str().expect("case name")
        );
        let path = physical_file
            .as_ref()
            .map(PhysicalGoldenFile::path_string)
            .unwrap_or(virtual_path);
        let early_error_container = decode_fails_before_inner(input);
        let all_ops_run_without_image_decoder = input["ops"].as_array().is_some_and(|ops| {
            ops.iter()
                .all(|op| op["op"] == "read_info" || (op["op"] == "decode" && early_error_container))
        });
        let scripted_image_case = (!input["file"]["physical"].as_bool().unwrap_or(false) || physical_cache_case)
            && ops.iter().all(|op| op["op"] == "decode")
            && !ops.is_empty()
            && golden_case["steps"]
                .as_array()
                .is_some_and(|steps| steps.len() == ops.len() && steps.iter().all(|step| step["ok"].is_object()))
            && input["container"]["kind"] != "throws";
        if scripted_image_case {
            run_image_decode_case(input, golden_case, &path);
            image_decode_cases += ops.len();
            executed_steps += ops.len();
        } else if all_ops_run_without_image_decoder {
            let (infos, steps) = run_read_info_case(input, golden_case, &path);
            read_info_steps += infos;
            executed_steps += steps;
        } else {
            let name = input["name"].as_str().unwrap_or("<unnamed>");
            gated_cases.push(name.to_owned());
            for (index, op) in input["ops"].as_array().into_iter().flatten().enumerate() {
                gated_steps.push(format!("{name}[{index}] {}", op["op"].as_str().unwrap_or("?")));
            }
        }
    }
    assert_eq!(
        read_info_steps, 13,
        "the 13 core read_info steps are executable without an image decoder"
    );
    assert_eq!(
        executed_steps,
        22 + image_decode_cases,
        "13 read_info, 9 early-error decode, and all eligible scripted image decode steps are executable"
    );
    eprintln!(
        "raw-decoder-core: executed {executed_steps} steps ({read_info_steps} read_info, {image_decode_cases} scripted image decode); gated {} cases and {} steps: cases={gated_cases:?}; steps={gated_steps:?}",
        gated_cases.len(),
        gated_steps.len()
    );
}

#[test]
fn decode_blank_path_returns_argument_error() {
    // C#: `RawDecoder.Decode` calls `ArgumentException.ThrowIfNullOrWhiteSpace(request.Path)` before I/O.
    let decoder = RawDecoder::new(Arc::new(NeverCalledDecoder));
    let error = match decoder.decode(&DecodeRequest::new("   ", 0)) {
        Ok(_) => panic!("whitespace path must be rejected"),
        Err(error) => error,
    };
    assert!(matches!(error.kind(), DecodeError::InvalidArgument { .. }));
    assert!(error.message().contains("Parameter 'path'"));
}

#[test]
fn read_info_blank_path_returns_argument_error() {
    // C#: `RawDecoder.ReadInfo` calls `ArgumentException.ThrowIfNullOrWhiteSpace(path)` before I/O.
    let decoder = RawDecoder::new(Arc::new(NeverCalledDecoder));
    let error = decoder.read_info("\t\r\n").expect_err("whitespace path is invalid");
    assert!(matches!(error.kind(), DecodeError::InvalidArgument { .. }));
    assert!(error.message().contains("Parameter 'path'"));
}
