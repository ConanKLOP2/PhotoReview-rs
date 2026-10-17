//! Shared helpers for the W5-14b decoder tests (`contracts/W5-14b.md`, "Native tests" and "Golden package").
//!
//! Contents: hex and xorshift, a test-local SHA-256 (the workspace has no digest crate and `Cargo.toml` is lead-owned),
//! `ProgressiveGray` (port of the C# helper of `TurboJpegHardeningTests.cs`), the byte transforms of the golden package
//! (`source.transforms`, applied in order), fakes for the seams (memory, codec, fine scaler, decompressor factory), and
//! the native decoder builder that loads `native/x64/turbojpeg.dll` by absolute path.
//!
//! Each test binary includes this module with `mod support;` and uses a different subset, hence the allow below.
#![allow(dead_code, unused_imports)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use pr_imaging::decode_types::ImageInfo;
use pr_imaging::decode_types::{PixelLease, PlatformImage};
use pr_imaging::decoder_traits::{DecodeError, PlatformImageCodec};
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_test_support::golden::{self, Golden};
use pr_turbojpeg::decoder::{
    Decompressor, DecompressorFactory, EnglishTurboJpegTexts, FineScaleError, FineScaler, TurboJpegDecoder,
    TurboJpegTexts,
};
use pr_turbojpeg::native::{TjParam, TjPixelFormat, TjScalingFactor, TurboJpegLibrary};
use serde_json::Value;

/// `NativePixelMemory::live_count()` is process-wide: tests that count it must not run next to a test that allocates
/// pixel buffers. Every decoder builder below calls this, which holds one lock until the test thread ends.
static PIXEL_MEMORY_LOCK: Mutex<()> = Mutex::new(());

thread_local! {
    static PIXEL_MEMORY_GUARD: RefCell<Option<MutexGuard<'static, ()>>> = const { RefCell::new(None) };
}

pub fn serialize_pixel_memory() {
    PIXEL_MEMORY_GUARD.with(|held| {
        let mut held = held.borrow_mut();
        if held.is_none() {
            *held = Some(
                PIXEL_MEMORY_LOCK
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            );
        }
    });
}

/// Decodes a hex string (upper or lower case, no separators) into bytes.
pub fn hex(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0, "odd hex length");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex digit"))
        .collect()
}

/// Deterministic xorshift64 (C# `Random` cannot be reproduced; the golden rows use this same generator shape).
#[derive(Debug, Clone)]
pub struct XorShift(u64);

impl XorShift {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn next_u8(&mut self) -> u8 {
        (self.next_u64() >> 24) as u8
    }
    /// Uniform-ish value in `0..n` (`n > 0`).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
    pub fn fill(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next_u8()).collect()
    }
}

/// SHA-256 of `data` as upper-case hex (the golden files write hashes in upper case).
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
        0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
        0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
        0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|v| format!("{v:08X}")).collect()
}

/// Repository-relative fixture under `tests/fixtures/` (for example `turbojpeg/parity-8x8.jpg`).
pub fn fixture(relative: &str) -> PathBuf {
    golden::repo_root().join("tests").join("fixtures").join(relative)
}

/// The bytes of a fixture (panics with the path when missing: a missing fixture is a broken checkout).
pub fn fixture_bytes(relative: &str) -> Vec<u8> {
    let path = fixture(relative);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
}

/// One JPEG marker segment: `FF marker`, the big-endian length (payload + 2), then the payload.
pub fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let length = u16::try_from(payload.len() + 2).expect("segment length fits u16");
    let mut out = vec![0xFF, marker];
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// C# `ProgressiveGray(refineLevels)` (`TurboJpegHardeningTests.cs`): an 8x8 single-component progressive JPEG with one
/// DC scan, one AC-first scan per coefficient 1..63 and `refine_levels` refinement scans per coefficient: 64 scans at 0
/// levels, 512 at 7. Every block is zero, so every scan is one Huffman bit.
pub fn progressive_gray(refine_levels: i32) -> Vec<u8> {
    let mut output = vec![0xFF, 0xD8];
    let mut dqt = vec![0u8];
    dqt.extend(std::iter::repeat_n(1u8, 64));
    output.extend(segment(0xDB, &dqt));
    output.extend(segment(0xC2, &[8, 0, 8, 0, 8, 1, 1, 0x11, 0]));
    let counts = [1u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut dht_dc = vec![0x00u8];
    dht_dc.extend_from_slice(&counts);
    dht_dc.push(0x00);
    output.extend(segment(0xC4, &dht_dc));
    let mut dht_ac = vec![0x10u8];
    dht_ac.extend_from_slice(&counts);
    dht_ac.push(0x00);
    output.extend(segment(0xC4, &dht_ac));

    let scan = |ss: u8, se: u8, ah: u8, al: u8, out: &mut Vec<u8>| {
        out.extend(segment(0xDA, &[1, 1, 0x00, ss, se, (ah << 4) | al]));
        out.push(0x7F);
    };
    let refine = refine_levels;
    let coefficient = |k: u8, out: &mut Vec<u8>| {
        scan(k, k, 0, refine as u8, out);
        for ah in (1..=refine).rev() {
            scan(k, k, ah as u8, (ah - 1) as u8, out);
        }
    };
    coefficient(0, &mut output);
    for k in 1..=63u8 {
        coefficient(k, &mut output);
    }
    output.extend_from_slice(&[0xFF, 0xD9]);
    output
}

// ---------------------------------------------------------------------------------------------------------------
// Golden byte transforms (`source.transforms`, applied in order)
// ---------------------------------------------------------------------------------------------------------------

fn read_u16_be(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([bytes[at], bytes[at + 1]])
}

fn write_u16_be(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

/// Index of the first SOF0/SOF2 marker at or after offset 2.
fn find_sof(bytes: &[u8]) -> usize {
    (2..bytes.len().saturating_sub(1))
        .find(|&j| bytes[j] == 0xFF && (bytes[j + 1] == 0xC0 || bytes[j + 1] == 0xC2))
        .expect("golden transform: no SOF marker")
}

/// Index of the first SOS (`FF DA`) marker at or after offset 2.
fn find_sos(bytes: &[u8]) -> usize {
    (2..bytes.len().saturating_sub(1))
        .find(|&j| bytes[j] == 0xFF && bytes[j + 1] == 0xDA)
        .expect("golden transform: no SOS marker")
}

/// A COM segment whose TOTAL length (marker included) is `total`.
fn com_total(total: usize) -> Vec<u8> {
    assert!(total >= 4, "a COM segment is at least 4 bytes");
    let payload = total - 4;
    let mut out = vec![0xFF, 0xFE];
    let length = u16::try_from(payload + 2).expect("COM length");
    out.extend_from_slice(&length.to_be_bytes());
    out.extend(std::iter::repeat_n(0u8, payload));
    out
}

/// Applies the golden transform list to `bytes` in order. The `op` names and fields are those of the golden package
/// (`contracts/W5-14b.md`, "Golden package"); an unknown op panics so a new golden row cannot pass silently.
pub fn apply_transforms(mut bytes: Vec<u8>, transforms: &[Value]) -> Vec<u8> {
    for t in transforms {
        match t["op"].as_str().expect("transform op") {
            "claim_size" => {
                let p = find_sof(&bytes);
                let height = t["height"].as_u64().expect("height") as u16;
                let width = t["width"].as_u64().expect("width") as u16;
                write_u16_be(&mut bytes, p + 5, height);
                write_u16_be(&mut bytes, p + 7, width);
            }
            "bad_precision" => {
                let p = find_sof(&bytes);
                bytes[p + 4] = 3;
            }
            "truncate" => {
                let num = t["num"].as_u64().expect("num");
                let den = t["den"].as_u64().expect("den");
                let n = (bytes.len() as u64 * num / den) as usize;
                bytes.truncate(n);
            }
            "set_orientation" => {
                let value = t["value"].as_u64().expect("value") as u16;
                let app1 = (2..bytes.len().saturating_sub(10))
                    .find(|&j| bytes[j] == 0xFF && bytes[j + 1] == 0xE1 && &bytes[j + 4..j + 10] == b"Exif\0\0")
                    .expect("golden transform: no Exif APP1");
                let tiff = app1 + 10;
                let little = &bytes[tiff..tiff + 2] == b"II";
                let u16_at = |b: &[u8], at: usize| {
                    if little {
                        u16::from_le_bytes([b[at], b[at + 1]])
                    } else {
                        u16::from_be_bytes([b[at], b[at + 1]])
                    }
                };
                let u32_at = |b: &[u8], at: usize| {
                    if little {
                        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
                    } else {
                        u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
                    }
                };
                let ifd = tiff + u32_at(&bytes, tiff + 4) as usize;
                let count = u16_at(&bytes, ifd) as usize;
                let entry = (0..count)
                    .map(|e| ifd + 2 + 12 * e)
                    .find(|&at| u16_at(&bytes, at) == 0x0112)
                    .expect("golden transform: no Orientation entry");
                let encoded = if little {
                    value.to_le_bytes()
                } else {
                    value.to_be_bytes()
                };
                bytes[entry + 8..entry + 10].copy_from_slice(&encoded);
            }
            "destroy_entropy_from_half" => {
                let len = bytes.len();
                for b in &mut bytes[len / 2..len - 2] {
                    *b = 0x55;
                }
            }
            "lead_com" => {
                let count = t["count"].as_u64().expect("count") as usize;
                let size = t["size"].as_u64().expect("size") as usize;
                let mut inserted = Vec::new();
                for _ in 0..count {
                    inserted.extend(com_total(size + 4));
                }
                bytes.splice(2..2, inserted);
            }
            "pad_com_before_sos" => {
                let k = t["sos_bytes_in_first_chunk"].as_u64().expect("k") as usize;
                let sos = find_sos(&bytes);
                let total = 65536 - k - sos;
                bytes.splice(2..2, com_total(total));
            }
            "sos_at" => {
                let offset = t["offset"].as_u64().expect("offset") as usize;
                let sos = find_sos(&bytes);
                bytes.splice(2..2, com_total(offset - sos));
            }
            "pad_com_bytes" => {
                let target = t["bytes"].as_u64().expect("bytes") as usize;
                let payload = t["payload"].as_u64().expect("payload") as usize;
                let keep_tail = t.get("keep_tail").and_then(Value::as_bool).unwrap_or(true);
                let mut out = vec![0xFF, 0xD8];
                while out.len() < target {
                    out.extend(com_total(payload + 4));
                }
                if keep_tail {
                    out.extend_from_slice(&bytes[2..]);
                }
                bytes = out;
            }
            other => panic!("unknown golden transform op {other}"),
        }
    }
    bytes
}

// ---------------------------------------------------------------------------------------------------------------
// Fakes for the seams
// ---------------------------------------------------------------------------------------------------------------

/// `MemoryInfoSource` that counts its reads (golden field `consulted`, and every "does not consult" test).
#[derive(Debug)]
pub struct CountingMemory {
    pub info: MemoryInfo,
    pub reads: AtomicUsize,
}

impl MemoryInfoSource for CountingMemory {
    fn read(&self) -> MemoryInfo {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.info
    }
}

/// Memory injected as `(total, load)` in bytes; the golden `memory` object.
pub fn memory(total: i64, load: i64) -> Arc<CountingMemory> {
    Arc::new(CountingMemory {
        info: MemoryInfo {
            total_available: total,
            load,
        },
        reads: AtomicUsize::new(0),
    })
}

/// Records how many images the codec received (the "exactly one buffer" tests).
#[derive(Debug, Default)]
pub struct RecordingCodec {
    pub images: AtomicUsize,
}

impl PlatformImageCodec for RecordingCodec {
    fn name(&self) -> &'static str {
        "recording"
    }
    fn image_from_pixels(&self, pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
        self.images.fetch_add(1, Ordering::SeqCst);
        Ok(PlatformImage::Pixels(Arc::new(pixels)))
    }
    fn to_pixels<'a>(&self, platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
        match platform_image.as_pixels() {
            Some(pixels) => Ok(PixelLease::borrowed(pixels.as_ref())),
            None => Err(DecodeError::InvalidArgument {
                message: "not a pixel image".to_owned(),
            }),
        }
    }
}

/// A fine scaler that must never be called (pure tests: the scaled size already equals the target).
pub fn no_scaler() -> Arc<dyn FineScaler> {
    Arc::new(
        |_: &PixelBuffer, _: i32, _: i32| -> Result<PixelBuffer, FineScaleError> {
            Err(FineScaleError::Failed(DecodeError::Other {
                message: "fine scale not expected in a pure test".to_owned(),
            }))
        },
    )
}

/// The English catalog texts (the golden `user_text` column).
pub fn english() -> Arc<dyn TurboJpegTexts> {
    Arc::new(EnglishTurboJpegTexts)
}

/// Scripted [`Decompressor`]. Every field has a neutral default (`Default`); tests override the field of the step they
/// exercise. Errors are fn pointers so the struct stays `Send` without cloning `DecodeError`.
#[derive(Debug)]
pub struct FakeDecompressor {
    /// `Err` returned by every `set` call (`None` = Ok).
    pub set_error: Option<fn() -> DecodeError>,
    /// `Err` returned by `set(ScanLimit, _)` only.
    pub scan_limit_error: Option<fn() -> DecodeError>,
    /// Raw return of `set(StopOnWarning, _)`; `0` = strict mode enabled.
    pub strict_code: i32,
    /// `Err` returned by `decompress_header`.
    pub header_error: Option<fn() -> DecodeError>,
    /// Raw return of `decompress_header`; `0` = success.
    pub header_code: i32,
    /// `JpegWidth` / `JpegHeight` values.
    pub width: i32,
    pub height: i32,
    /// `Err` returned by `get`.
    pub get_error: Option<fn() -> DecodeError>,
    /// Raw return of `set_scaling_factor`; non-zero = refused.
    pub scale_code: i32,
    /// Raw return of `decompress8`; non-zero = failure.
    pub decompress_code: i32,
    /// `Err` returned by `decompress8`.
    pub decompress_error: Option<fn() -> DecodeError>,
    /// `error_message()`: the native text of the last failure (`None` = null pointer).
    pub error: Option<String>,
}

impl Default for FakeDecompressor {
    fn default() -> Self {
        Self {
            set_error: None,
            scan_limit_error: None,
            strict_code: 0,
            header_error: None,
            header_code: 0,
            width: 8,
            height: 8,
            get_error: None,
            scale_code: 0,
            decompress_code: 0,
            decompress_error: None,
            error: None,
        }
    }
}

impl Decompressor for FakeDecompressor {
    fn set(&mut self, param: TjParam, value: i32) -> Result<i32, DecodeError> {
        if let Some(e) = self.set_error {
            return Err(e());
        }
        if param == TjParam::ScanLimit {
            if let Some(e) = self.scan_limit_error {
                return Err(e());
            }
            return Ok(0);
        }
        if param == TjParam::StopOnWarning {
            return Ok(self.strict_code);
        }
        let _ = value;
        Ok(0)
    }
    fn get(&mut self, param: TjParam) -> Result<i32, DecodeError> {
        if let Some(e) = self.get_error {
            return Err(e());
        }
        Ok(match param {
            TjParam::JpegWidth => self.width,
            TjParam::JpegHeight => self.height,
            _ => 0,
        })
    }
    fn decompress_header(&mut self, _jpeg: &[u8]) -> Result<i32, DecodeError> {
        if let Some(e) = self.header_error {
            return Err(e());
        }
        Ok(self.header_code)
    }
    fn set_scaling_factor(&mut self, _factor: TjScalingFactor) -> Result<i32, DecodeError> {
        Ok(self.scale_code)
    }
    fn decompress8(
        &mut self,
        _jpeg: &[u8],
        _dst: &mut [u8],
        _pitch: i32,
        _pixel_format: TjPixelFormat,
    ) -> Result<i32, DecodeError> {
        if let Some(e) = self.decompress_error {
            return Err(e());
        }
        Ok(self.decompress_code)
    }
    fn error_message(&mut self) -> Option<String> {
        self.error.clone()
    }
}

/// A factory that builds a fresh [`FakeDecompressor`] from `make` on every `create` call.
pub fn factory_of(make: impl Fn() -> FakeDecompressor + Send + Sync + 'static) -> Arc<dyn DecompressorFactory> {
    Arc::new(move || -> Result<Box<dyn Decompressor>, DecodeError> {
        let decompressor: Box<dyn Decompressor> = Box::new(make());
        Ok(decompressor)
    })
}

/// A factory whose every `create` fails with `error` (the factory error arm of semantics 13).
pub fn failing_factory(error: fn() -> DecodeError) -> Arc<dyn DecompressorFactory> {
    Arc::new(move || -> Result<Box<dyn Decompressor>, DecodeError> { Err(error()) })
}

/// A decoder over fakes (no DLL, no WIC): the pure half of the tests.
pub fn pure_decoder(factory: Arc<dyn DecompressorFactory>, memory: Arc<dyn MemoryInfoSource>) -> TurboJpegDecoder {
    serialize_pixel_memory();
    TurboJpegDecoder::new(
        Arc::new(RecordingCodec::default()),
        no_scaler(),
        memory,
        factory,
        english(),
    )
}

/// The W5-13a DLL path, by absolute path (`tools/fetch-native.ps1` puts it there).
pub fn native_dll_path() -> PathBuf {
    golden::repo_root().join("native").join("x64").join("turbojpeg.dll")
}

/// The fine scaler of production: WIC Fant over `pr_platform_win::wic::scaler::resize` (Windows only).
#[cfg(windows)]
pub fn wic_scaler() -> Arc<dyn FineScaler> {
    Arc::new(
        |source: &PixelBuffer, width: i32, height: i32| -> Result<PixelBuffer, FineScaleError> {
            pr_platform_win::wic::scaler::resize(source, width, height).map_err(|error| match error {
                pr_platform_win::wic::scaler::ScaleError::InsufficientMemory { .. } => FineScaleError::OutOfMemory,
                other => FineScaleError::Failed(DecodeError::Other {
                    message: format!("{other:?}"),
                }),
            })
        },
    )
}

/// A decoder over the real libjpeg-turbo DLL loaded from `native/x64/`. `None` (after printing the reason) when the DLL
/// cannot be loaded: the native tests then degrade like the C# availability probe instead of failing (contract "Native
/// tests"). Production uses `TurboJpegDecoder::native`; tests never rely on the exe-folder search.
pub fn native_decoder_with(memory: Arc<dyn MemoryInfoSource>) -> Option<TurboJpegDecoder> {
    serialize_pixel_memory();
    let path = native_dll_path();
    match TurboJpegLibrary::load(&path) {
        Ok(library) => Some(TurboJpegDecoder::new(
            PixelBufferImageCodec::shared(),
            native_fine_scaler(),
            memory,
            Arc::new(pr_turbojpeg::decoder::NativeDecompressors::with_library(library)),
            english(),
        )),
        Err(error) => {
            eprintln!("skipped: {} not usable ({error:?})", path.display());
            None
        }
    }
}

#[cfg(windows)]
fn native_fine_scaler() -> Arc<dyn FineScaler> {
    wic_scaler()
}

#[cfg(not(windows))]
fn native_fine_scaler() -> Arc<dyn FineScaler> {
    no_scaler()
}

/// Writes `bytes` to `dir/name` and returns the path (temp files live under `TempRoot`).
pub fn write_under(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    path
}

/// Loads a golden file and returns its cases (the manifest and the case loops).
pub fn cases(name: &str) -> Vec<Value> {
    let golden: Golden = golden::load(name);
    golden.cases
}

/// Pixel layout name of the golden (`buffer_layout`).
pub fn layout_name(layout: PixelLayout) -> &'static str {
    match layout {
        PixelLayout::Bgr32 => "Bgr32",
        PixelLayout::Pbgra32 => "Pbgra32",
    }
}

/// `ImageInfo` fields the golden compares.
pub fn info_fields(info: &ImageInfo) -> (i32, i32, i32, i32, i32) {
    (
        info.pixel_width,
        info.pixel_height,
        info.orientation,
        info.width(),
        info.height(),
    )
}
