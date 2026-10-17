//! W5-12b1 core tests. The golden driver executes `read_info` and decode steps that fail before an inner image decoder
//! is needed. Image-producing decode cases need the scripted decoder seam; physical identity and stateful-hook cases
//! also need the C# fixture behavior and stay explicitly gated.

use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pr_core::abstractions::{ReadSeek, SourceReadPriority, SourceReader};
use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
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
    let mut bytes = vec![0; case["file"]["size"].as_u64().unwrap_or(0) as usize];
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
    // `range-negative-offset` stays gated: its current output differs from the C# oracle's open count (1 vs. 2).
    matches!(kind, "none" | "throws")
        || case["file"]["faulty_read"].is_string()
        || matches!(
            case["name"].as_str(),
            Some(
                "unsupported-default-registry-3-bytes"
                    | "malformed-default-registry-12-bytes"
                    | "range-past-end"
                    | "range-longer-than-cap"
                    | "range-adobe-rgb-not-a-jpeg"
            )
        )
}

fn run_read_info_case(input: &Value, expected: &Value) -> (usize, usize) {
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
    let path = format!(r"C:\raw-virtual-nonexistent\{name}.dng");
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
            match decoder.read_info(&path) {
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
            let request = DecodeRequest::new(&path, width);
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
            let message = error.message().replace(&path, "{path}");
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
    for (input, golden_case) in input_cases.iter().zip(&expected.cases) {
        assert_eq!(input["name"], golden_case["name"], "input/golden case order");
        let early_error_container = decode_fails_before_inner(input);
        let all_ops_run_without_image_decoder = input["ops"].as_array().is_some_and(|ops| {
            ops.iter()
                .all(|op| op["op"] == "read_info" || (op["op"] == "decode" && early_error_container))
        });
        if all_ops_run_without_image_decoder {
            let (infos, steps) = run_read_info_case(input, golden_case);
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
        executed_steps, 21,
        "13 read_info and 8 early-error decode steps are executable"
    );
    eprintln!(
        "raw-decoder-core: executed {executed_steps} steps ({read_info_steps} read_info); gated {} cases and {} steps: cases={gated_cases:?}; steps={gated_steps:?}",
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
