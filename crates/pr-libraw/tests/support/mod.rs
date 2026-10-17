//! Shared helpers for the `pr-libraw` decoder tests (W5-16c): golden input loaders, the xorshift junk generator, scripted
//! fakes for the LibRaw seams (`LibRawApi`, `LibRawSession`, `ProcessedImage`, `LibRawProbe`), a recording
//! `PlatformImageCodec`, a `StagedPixels` double and the process-wide gate lock (contract trap T9).
//!
//! Every test binary includes this module with `mod support;`. Items unused by one binary are allowed.
#![allow(dead_code)] // each test binary uses a different subset of these helpers

pub mod raw_orientation_patcher;

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

use pr_core::abstractions::SourceReadPriority;
use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{PixelLease, PlatformImage, SourceBytes};
use pr_imaging::decoder_traits::{DecodeError, PlatformImageCodec};
use pr_imaging::memory_headroom::MemoryInfo;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_libraw::decoder::{LibRawApi, LibRawProbe, LibRawSession, ProcessedImage, ProcessedImageHeader, StagedPixels};
use pr_libraw::memory_guard::RawStructure;
use serde_json::Value;

/// Hang guard for every bounded wait in the fake-gate tests (contract trap T10: `recv_timeout`, never a sleep loop).
pub const WAIT_LIMIT: Duration = Duration::from_secs(60);

/// Process-wide serialisation of every test that reads the PROCESS-WIDE gate (C# `[Collection(LibRawNativeDecodeGate.Name)]`).
static GLOBAL_GATE: Mutex<()> = Mutex::new(());

/// Holds the process-wide gate lock for the duration of one test (contract trap T9).
pub fn global_gate_lock() -> MutexGuard<'static, ()> {
    GLOBAL_GATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `NativePixelMemory::live_count()` is process-wide, so the tests that assert it (exclusive) must not overlap with any test that
/// allocates or frees pixel buffers (shared). C# serialises the same way through its xUnit collection.
static PIXEL_MEMORY: RwLock<()> = RwLock::new(());

/// Taken by every test that allocates or frees pixel buffers.
pub fn pixel_memory_shared() -> RwLockReadGuard<'static, ()> {
    PIXEL_MEMORY.read().unwrap_or_else(PoisonError::into_inner)
}

/// Taken by the tests that assert `NativePixelMemory::live_count()` deltas.
pub fn pixel_memory_exclusive() -> RwLockWriteGuard<'static, ()> {
    PIXEL_MEMORY.write().unwrap_or_else(PoisonError::into_inner)
}

// ------------------------------------------------------------------------------------------------ golden inputs

/// `tests/golden-inputs/<file>.json`, the input recipe the C# oracle was run with (`tests/golden/<file>.json` holds the
/// expected values). Returns the `cases` array.
pub fn golden_inputs(file: &str) -> Vec<Value> {
    let path = pr_test_support::golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{file}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .clone()
}

/// The input recipe of the golden case called `name` in `golden-inputs/<file>.json`.
pub fn golden_input(file: &str, name: &str) -> Value {
    golden_inputs(file)
        .into_iter()
        .find(|case| case["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("golden-inputs/{file}.json has no case '{name}'"))
}

/// The bytes of a trim input: `hex` verbatim, or `fill.length` copies of `fill.byte_hex` with each `patches[i].hex` written
/// at `patches[i].at` (C# `TrimInput` helper; the `trim-window-*` rows straddle the 64 KiB search window).
pub fn trim_input_bytes(input: &Value) -> Vec<u8> {
    if let Some(hex) = input["hex"].as_str() {
        return pr_test_support::raw::from_hex(hex);
    }
    let fill = &input["fill"];
    let length = fill["length"].as_u64().expect("fill.length") as usize;
    let fill_byte = pr_test_support::raw::from_hex(fill["byte_hex"].as_str().expect("fill.byte_hex"))[0];
    let mut bytes = vec![fill_byte; length];
    if let Some(patches) = input["patches"].as_array() {
        for patch in patches {
            let at = patch["at"].as_u64().expect("patch.at") as usize;
            let data = pr_test_support::raw::from_hex(patch["hex"].as_str().expect("patch.hex"));
            bytes[at..at + data.len()].copy_from_slice(&data);
        }
    }
    bytes
}

/// Lower-case hex of `bytes` (for the golden `input_sha256` check).
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 of `bytes` as lower-case hex (`pr_core::session::store::sha256`, D33).
pub fn sha256_hex(bytes: &[u8]) -> String {
    to_hex(&pr_core::session::store::sha256(bytes))
}

/// The xorshift32 junk file of golden semantics 9: `x = seed` (0 becomes 1); per byte `x ^= x << 13; x ^= x >> 17; x ^= x << 5`
/// (u32 wrapping), byte = `x & 0xFF`. Shared with the C# oracle (`oracle/W5-16c.cs`): NOT .NET `Random`.
pub fn xorshift32_bytes(seed: u32, length: usize) -> Vec<u8> {
    let mut x: u32 = if seed == 0 { 1 } else { seed };
    (0..length)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x & 0xFF) as u8
        })
        .collect()
}

/// A fresh, unique temporary folder for one test (removed by the caller).
pub fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("pr-libraw-{label}-{}-{nanos}", std::process::id()))
}

/// The C# `FormatError` table of `tests/golden/libraw-decoder-native.json` (`format-error` case), as a closure for the pure
/// helpers (`check_result`, `create_failure`, `open_file_with`). Codes outside the table get the library's own `Unknown error code` text (`libraw_strerror`).
pub fn golden_format_error() -> impl Fn(i32) -> String {
    let golden = pr_test_support::golden::load("libraw-decoder-native");
    let table = golden
        .cases
        .iter()
        .find(|case| case["op"].as_str() == Some("format-error"))
        .expect("libraw-decoder-native.json has a format-error case")["messages"]
        .as_array()
        .expect("format-error.messages")
        .clone();
    move |code| {
        table
            .iter()
            .find(|row| row["code"].as_i64() == Some(i64::from(code)))
            .and_then(|row| row["message"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "Unknown error code".to_owned())
    }
}

/// Maps a golden `throws.type` (a .NET exception name) to the `DecodeError` variant name it must be.
pub fn expected_kind(golden_type: &str) -> &'static str {
    match golden_type {
        "InvalidDataException" => "InvalidData",
        "ArgumentException" => "InvalidArgument",
        "NotSupportedException" => "NotSupported",
        "InvalidOperationException" => "InvalidOperation",
        "OperationCanceledException" => "Canceled",
        other => panic!("unmapped golden exception type {other}"),
    }
}

/// The variant name of a `DecodeError` (the `kind()` of a wrapped error).
pub fn kind_name(error: &DecodeError) -> &'static str {
    match error.kind() {
        DecodeError::InvalidData { .. } => "InvalidData",
        DecodeError::InvalidArgument { .. } => "InvalidArgument",
        DecodeError::NotSupported { .. } => "NotSupported",
        DecodeError::InvalidOperation { .. } => "InvalidOperation",
        DecodeError::Canceled => "Canceled",
        DecodeError::Busy { .. } => "Busy",
        DecodeError::OutOfMemory { .. } => "OutOfMemory",
        DecodeError::DllNotFound { .. } => "DllNotFound",
        DecodeError::EntryPointNotFound { .. } => "EntryPointNotFound",
        DecodeError::Disposed { .. } => "Disposed",
        DecodeError::Overflow { .. } => "Overflow",
        DecodeError::MemoryAdmission { .. } => "MemoryAdmission",
        other => panic!("unexpected DecodeError kind: {other:?}"),
    }
}

/// Asserts that `error` is the golden `throws` (kind and, except for cancellation, the exact message).
pub fn assert_golden_throws(error: &DecodeError, golden: &Value, name: &str) {
    let expected_type = golden["type"].as_str().expect("throws.type");
    assert_eq!(
        kind_name(error),
        expected_kind(expected_type),
        "{name}: wrong error kind ({error:?})"
    );
    if let Some(message) = golden["message"].as_str() {
        if expected_type != "OperationCanceledException" {
            assert_eq!(error.message(), message, "{name}: wrong message");
        }
    }
}

// ------------------------------------------------------------------------------------------------ recording

/// Ordered log of everything the fakes observed, plus the cancel tokens given to `set_progress_handler`.
#[derive(Clone, Default)]
pub struct Log {
    lines: Arc<Mutex<Vec<String>>>,
    tokens: Arc<Mutex<Vec<CancelToken>>>,
}

impl Log {
    pub fn push(&self, line: impl Into<String>) {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.into());
    }

    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn push_token(&self, token: CancelToken) {
        self.tokens.lock().unwrap_or_else(PoisonError::into_inner).push(token);
    }

    pub fn tokens(&self) -> Vec<CancelToken> {
        self.tokens.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Only the `stage:<name>` entries, in order (what the C# stage observer saw).
    pub fn stages(&self) -> Vec<String> {
        self.lines()
            .into_iter()
            .filter_map(|line| line.strip_prefix("stage:").map(str::to_owned))
            .collect()
    }
}

/// A stage observer that logs `stage:<name>` (C# `Action<string> stageObserver`).
pub fn stage_logger(log: &Log) -> pr_libraw::decoder::StageObserver {
    let log = log.clone();
    Arc::new(move |stage: &str| {
        log.push(format!("stage:{stage}"));
        Ok(())
    })
}

// ------------------------------------------------------------------------------------------------ scripted native fakes

/// One processed image the fake returns from `make_mem_image` / `make_mem_thumb`.
#[derive(Clone, Debug)]
pub struct FakeImage {
    pub header: ProcessedImageHeader,
    pub data: Vec<u8>,
}

impl FakeImage {
    /// An 8-bit bitmap of `width x height x colors` bytes (`LIBRAW_IMAGE_BITMAP`).
    pub fn bitmap(width: u16, height: u16, colors: u16) -> Self {
        let data = vec![0x80; usize::from(width) * usize::from(height) * usize::from(colors)];
        Self {
            header: ProcessedImageHeader {
                image_type: pr_libraw::native::IMAGE_BITMAP,
                height,
                width,
                colors,
                bits: 8,
                data_size: data.len() as u32,
            },
            data,
        }
    }

    /// An embedded JPEG (`LIBRAW_IMAGE_JPEG`) with exactly `data` as payload.
    pub fn jpeg(data: Vec<u8>) -> Self {
        Self {
            header: ProcessedImageHeader {
                image_type: pr_libraw::native::IMAGE_JPEG,
                height: 0,
                width: 0,
                colors: 0,
                bits: 0,
                data_size: data.len() as u32,
            },
            data,
        }
    }
}

impl ProcessedImage for FakeImage {
    fn header(&self) -> ProcessedImageHeader {
        self.header
    }

    fn data(&self) -> &[u8] {
        &self.data
    }
}

/// Everything one fake LibRaw handle answers. Codes are LibRaw return values (`0` = success).
#[derive(Clone, Debug)]
pub struct Script {
    /// `true`: `init` gives a null handle (`InvalidOperation`, `INIT_FAILED_MESSAGE`).
    pub init_null: bool,
    /// `true`: `init` must never be reached (a test that expects no handle); reaching it panics.
    pub init_panics: bool,
    /// Return codes for successive `open_wfile` / `open_buffer` calls; empty means `0`.
    pub open_codes: VecDeque<i32>,
    pub unpack_code: i32,
    pub unpack_thumb_code: i32,
    pub process_code: i32,
    pub adjust_code: i32,
    pub image: Result<FakeImage, i32>,
    pub thumb: Result<FakeImage, i32>,
    pub width: i32,
    pub height: i32,
    pub raw_width: i32,
    pub raw_height: i32,
    pub decoder_name: Option<String>,
    pub structure: Option<RawStructure>,
    pub camera_wb_ok: bool,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            init_null: false,
            init_panics: false,
            open_codes: VecDeque::new(),
            unpack_code: 0,
            unpack_thumb_code: 0,
            process_code: 0,
            adjust_code: 0,
            image: Ok(FakeImage::bitmap(2, 2, 3)),
            thumb: Ok(FakeImage::jpeg(vec![0xFF, 0xD8, 0xFF, 0xD9])),
            width: 2,
            height: 2,
            raw_width: 2,
            raw_height: 2,
            decoder_name: Some("unpacked_load_raw()".to_owned()),
            structure: Some(RawStructure::new(3, 0)),
            camera_wb_ok: true,
        }
    }
}

/// The scripted `LibRawApi` (C# `LibRawNativeMethods` behind the seam). Every session it creates logs into `log`.
pub struct FakeApi {
    pub script: Arc<Mutex<Script>>,
    pub log: Log,
}

impl FakeApi {
    pub fn new(script: Script, log: &Log) -> Self {
        Self {
            script: Arc::new(Mutex::new(script)),
            log: log.clone(),
        }
    }

    pub fn arc(script: Script, log: &Log) -> Arc<dyn LibRawApi> {
        Arc::new(Self::new(script, log))
    }

    /// Edits the script of a live fake (used to change a code between two calls).
    pub fn edit(&self, change: impl FnOnce(&mut Script)) {
        change(&mut self.script.lock().unwrap_or_else(PoisonError::into_inner));
    }
}

impl LibRawApi for FakeApi {
    fn init(&self) -> Result<Box<dyn LibRawSession>, DecodeError> {
        self.log.push("init");
        let script = self.script.lock().unwrap_or_else(PoisonError::into_inner).clone();
        assert!(!script.init_panics, "libraw_init must not be reached in this test");
        if script.init_null {
            return Err(DecodeError::InvalidOperation {
                message: pr_libraw::decoder::INIT_FAILED_MESSAGE.to_owned(),
            });
        }
        Ok(Box::new(FakeSession {
            script: Arc::clone(&self.script),
            log: self.log.clone(),
        }))
    }

    fn format_error(&self, error_code: i32) -> String {
        format!("LibRaw error {error_code}")
    }
}

/// One fake `libraw_data_t`. `Drop` logs `libraw_close` (the ordering tests rely on it).
pub struct FakeSession {
    script: Arc<Mutex<Script>>,
    log: Log,
}

impl FakeSession {
    fn with_script<T>(&self, read: impl FnOnce(&Script) -> T) -> T {
        read(&self.script.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Drop for FakeSession {
    fn drop(&mut self) {
        self.log.push("libraw_close");
    }
}

impl LibRawSession for FakeSession {
    fn set_progress_handler(&mut self, cancel: CancelToken) {
        self.log.push("progress");
        self.log.push_token(cancel);
    }

    fn set_output_color(&mut self, value: i32) {
        self.log.push(format!("set_output_color({value})"));
    }

    fn set_output_bps(&mut self, value: i32) {
        self.log.push(format!("set_output_bps({value})"));
    }

    fn set_no_auto_bright(&mut self, value: i32) {
        self.log.push(format!("set_no_auto_bright({value})"));
    }

    fn try_set_use_camera_wb(&mut self, output_color: i32, output_bps: i32, no_auto_bright: i32) -> bool {
        self.log.push(format!(
            "try_set_use_camera_wb({output_color},{output_bps},{no_auto_bright})"
        ));
        self.with_script(|s| s.camera_wb_ok)
    }

    fn open_wfile(&mut self, path: &OsStr) -> i32 {
        self.log.push(format!("open_wfile:{}", path.to_string_lossy()));
        self.script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .open_codes
            .pop_front()
            .unwrap_or(0)
    }

    fn open_buffer(&mut self, bytes: SourceBytes) -> i32 {
        self.log.push(format!("open_buffer:{}", bytes.len()));
        self.script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .open_codes
            .pop_front()
            .unwrap_or(0)
    }

    fn unpack(&mut self) -> i32 {
        self.log.push("unpack");
        self.with_script(|s| s.unpack_code)
    }

    fn unpack_thumb(&mut self) -> i32 {
        self.log.push("unpack_thumb");
        self.with_script(|s| s.unpack_thumb_code)
    }

    fn dcraw_process(&mut self) -> i32 {
        self.log.push("dcraw_process");
        self.with_script(|s| s.process_code)
    }

    fn adjust_sizes_info_only(&mut self) -> i32 {
        self.log.push("adjust_sizes_info_only");
        self.with_script(|s| s.adjust_code)
    }

    fn iwidth(&self) -> i32 {
        self.with_script(|s| s.width)
    }

    fn iheight(&self) -> i32 {
        self.with_script(|s| s.height)
    }

    fn raw_width(&self) -> i32 {
        self.with_script(|s| s.raw_width)
    }

    fn raw_height(&self) -> i32 {
        self.with_script(|s| s.raw_height)
    }

    fn decoder_name(&self) -> Option<String> {
        self.with_script(|s| s.decoder_name.clone())
    }

    fn raw_structure(&self) -> Option<RawStructure> {
        self.with_script(|s| s.structure)
    }

    fn make_mem_image(&mut self) -> Result<Box<dyn ProcessedImage>, i32> {
        self.log.push("make_mem_image");
        self.with_script(|s| s.image.clone())
            .map(|image| Box::new(image) as Box<dyn ProcessedImage>)
    }

    fn make_mem_thumb(&mut self) -> Result<Box<dyn ProcessedImage>, i32> {
        self.log.push("make_mem_thumb");
        self.with_script(|s| s.thumb.clone())
            .map(|image| Box::new(image) as Box<dyn ProcessedImage>)
    }
}

/// A `LibRawProbe` double that logs every read (C# `ILibRawProbe` fake: the admission reads are asserted by the tests).
pub struct FakeProbe {
    pub width: i32,
    pub height: i32,
    pub raw_width: i32,
    pub raw_height: i32,
    pub decoder_name: Option<String>,
    pub structure: Option<RawStructure>,
    pub log: Log,
}

impl FakeProbe {
    /// A mosaic (Bayer) probe of the given output size.
    pub fn bayer(width: i32, height: i32, log: &Log) -> Self {
        Self {
            width,
            height,
            raw_width: width,
            raw_height: height,
            decoder_name: Some("unpacked_load_raw()".to_owned()),
            structure: Some(RawStructure::new(3, 0x9494_9494)),
            log: log.clone(),
        }
    }
}

impl LibRawProbe for FakeProbe {
    fn width(&self) -> i32 {
        self.log.push("probe:width");
        self.width
    }

    fn height(&self) -> i32 {
        self.log.push("probe:height");
        self.height
    }

    fn raw_width(&self) -> i32 {
        self.log.push("probe:raw_width");
        self.raw_width
    }

    fn raw_height(&self) -> i32 {
        self.log.push("probe:raw_height");
        self.raw_height
    }

    fn decoder_name(&self) -> Option<String> {
        self.log.push("probe:decoder_name");
        self.decoder_name.clone()
    }

    fn structure(&self) -> Option<RawStructure> {
        self.log.push("probe:structure");
        self.structure
    }
}

// ------------------------------------------------------------------------------------------------ codec and pixels

/// A `PlatformImageCodec` that records what it was handed (C# `IPlatformImageCodec` fake). `fail` makes it throw after taking
/// the buffer (the buffer is then dropped, C# `buffer.Dispose()`).
pub struct FakeCodec {
    pub log: Log,
    pub fail: bool,
}

impl PlatformImageCodec for FakeCodec {
    fn name(&self) -> &'static str {
        "fake-codec"
    }

    fn image_from_pixels(&self, pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
        self.log.push(format!(
            "codec:{}x{}:{}",
            pixels.width(),
            pixels.height(),
            pixels.address().map(|p| p as usize).unwrap_or(0)
        ));
        if self.fail {
            drop(pixels);
            return Err(DecodeError::InvalidOperation {
                message: "codec refused the pixels".to_owned(),
            });
        }
        Ok(PlatformImage::Pixels(Arc::new(pixels)))
    }

    fn to_pixels<'a>(&self, platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
        Ok(match platform_image.as_pixels() {
            Some(pixels) => PixelLease::borrowed(pixels),
            None => PixelLease::empty(),
        })
    }
}

/// `expect_err` for results whose success type is not `Debug` (a decoded image view): the failure is returned, a success
/// panics with `message`.
pub trait ExpectError<T> {
    fn expect_error(self, message: &str) -> DecodeError;
}

impl<T> ExpectError<T> for Result<T, DecodeError> {
    fn expect_error(self, message: &str) -> DecodeError {
        match self {
            Ok(_) => panic!("{message}: expected an error"),
            Err(error) => error,
        }
    }
}

/// A `StagedPixels` double over a real `PixelBuffer` (`Bgr32`). `take_pixels` moves the buffer out once; the second call is
/// `Disposed`. `owned()` tells whether the buffer is still held (the stage observer asks, see the hand-off tests).
pub struct FakeStaged {
    pixels: Option<PixelBuffer>,
    /// Shared with the test: `true` while the staging buffer is still owned here (set false by `take_pixels`).
    owned_flag: Arc<AtomicBool>,
    pub width: i32,
    pub height: i32,
    address: usize,
}

impl FakeStaged {
    pub fn new(width: i32, height: i32) -> Self {
        let pixels = PixelBuffer::allocate(width, height, PixelLayout::Bgr32).expect("allocate fake staging buffer");
        let address = pixels.address().map(|p| p as usize).unwrap_or(0);
        Self {
            pixels: Some(pixels),
            owned_flag: Arc::new(AtomicBool::new(true)),
            width,
            height,
            address,
        }
    }

    /// Handle the test keeps to observe ownership from a stage observer (true while the buffer is still held here).
    pub fn owned_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.owned_flag)
    }

    /// `true` while the staging buffer has not been taken yet.
    pub fn owned(&self) -> bool {
        self.pixels.is_some()
    }

    /// The address of the staged pixels before any hand-off (the codec must receive this same address).
    pub fn address(&self) -> usize {
        self.address
    }
}

impl StagedPixels for FakeStaged {
    fn take_pixels(&mut self) -> Result<PixelBuffer, DecodeError> {
        let taken = self.pixels.take().ok_or_else(|| DecodeError::Disposed {
            message: "The staging buffer was already taken.".to_owned(),
        });
        self.owned_flag.store(false, Ordering::SeqCst);
        taken
    }
}

// ------------------------------------------------------------------------------------------------ memory and files

/// A memory source that reports the given total and load (C# `Func<(long, long)> MemoryInfo`).
pub fn memory_source(total_available: i64, load: i64) -> Arc<dyn pr_imaging::memory_headroom::MemoryInfoSource> {
    Arc::new(move || MemoryInfo { total_available, load })
}

/// A memory source that must never be read (C# `() => throw`): the tests assert it is not consulted.
pub fn memory_source_never_read() -> Arc<dyn pr_imaging::memory_headroom::MemoryInfoSource> {
    Arc::new(|| -> MemoryInfo { panic!("the memory source must not be read here") })
}

/// `DecodeRequest` for a path with the default viewer priority and orientation applied.
pub fn request(path: &str) -> pr_imaging::decode_types::DecodeRequest {
    pr_imaging::decode_types::DecodeRequest::new(path, 0)
}

/// The priority the golden `priority` field names.
pub fn priority_of(name: Option<&str>) -> SourceReadPriority {
    match name {
        Some("Preload") => SourceReadPriority::Preload,
        _ => SourceReadPriority::Viewer,
    }
}

/// C# `Pixels/LibRawPixelBufferHandOffTests` helper: the `PixelBuffer` size pair as a `(w, h)` tuple for assertions.
pub fn pixel_size(buffer: &PixelBuffer) -> (i32, i32) {
    (buffer.width(), buffer.height())
}

/// Shorthand: the empty path check input used by the guard tests.
pub fn os(path: &str) -> &OsStr {
    OsStr::new(path)
}

/// Canceled token: `cancel()` already called.
pub fn canceled_token() -> CancelToken {
    let token = CancelToken::new();
    token.cancel();
    token
}

/// C# `LibRawDecoderTests.SelectCorpusFiles` (without the `PHOTOREVIEW_LIBRAW_SAMPLE` branch, which the native test reads itself):
/// only RAW files (`is_raw_extension`) are ever selected; `strict` keeps every one of them, ordered by name ignoring case; the
/// default keeps the first file (by name, ignoring case) of each extension (ignoring case), also ordered by name.
pub fn select_corpus_files(files: &[PathBuf], strict: bool) -> Vec<PathBuf> {
    let key = |path: &PathBuf| path.to_string_lossy().to_lowercase();
    let mut raw: Vec<PathBuf> = files
        .iter()
        .filter(|path| pr_imaging_raw::formats::is_raw_extension(path.to_str()))
        .cloned()
        .collect();
    raw.sort_by_key(key);
    if strict {
        return raw;
    }
    let mut seen = std::collections::HashSet::new();
    let mut picked: Vec<PathBuf> = raw
        .into_iter()
        .filter(|path| {
            let extension = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            seen.insert(extension)
        })
        .collect();
    picked.sort_by_key(key);
    picked
}

/// Bytes of the golden `trim-*` input (`hex` or fill+patches), used by the decoder logic tests too.
pub fn trim_case_bytes(name: &str) -> Vec<u8> {
    trim_input_bytes(&golden_input("libraw-decoder", name))
}

/// The `Vec` allocation identity check of C# `ReferenceEquals` on the trimmed array.
pub fn same_allocation(before: *const u8, after: &[u8]) -> bool {
    std::ptr::eq(before, after.as_ptr())
}
