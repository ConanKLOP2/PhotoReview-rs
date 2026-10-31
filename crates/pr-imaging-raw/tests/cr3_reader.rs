//! Red tests for W5-10b (`readers/cr3.rs`): `Cr3ContainerReader` (Canon CR3 container: CMT1/CMT2/CMT3/CMT4 EXIF blocks, PRVW and THMB
//! previews, the track-1 JPEG; C# baseline d92d6aae, `src/PhotoReview.Imaging.Raw/Bmff/Cr3ContainerReader.cs`).
//!
//! Every C# test of the contract's list is ported with `// C#: <Class>.<Method>`, plus the golden `raw-cr3-reader`
//! (321 rows, `golden_raw_cr3_reader`), the two CR3 fuzz families (`Fuzz_Cr3ContainerReader_Robust`,
//! `Fuzz_Cr3RealLayout_StrictSource_FailsOnlyCleanlyAndConsumersNeverThrow`, the CR3 row of `Fuzz_RawContainerReaders_FailOnlyCleanly`)
//! and the Rust-only tests the contract asks for. Stub bodies are `todo!()`, so every test that reaches the reader fails until it is filled.
//!
//! Documented skips and deviations:
//! - The `Read`-side `Cr3AndRafReaderTests` checks use `can_read` on the first 64 bytes plus `read`; the `RawContainerReaderRegistry`
//!   lookup (`FindReader`) is W5-11's.
//! - The `RawExif` / `PreviewSelector` consumer calls of the two fuzz families are W5-11's and are not called here.
//! - The corpus tests skip when the RAW corpus is missing (`PHOTOREVIEW_RS_RAW_CORPUS`, default `tests/fixtures/raw-corpus`), and fail
//!   when `PHOTOREVIEW_RAW_CORPUS_STRICT` is `1` / `true`. The C# SHA-256 pin of the corpus files is NOT re-checked (no hash crate in the
//!   workspace): the golden's `file_length` + `file_fnv1a64` already pin the exact bytes of each row.
//! - The C# per-case (10 s) and per-corpus watchdog of `BinaryFuzz.Run` is replaced by timing each case with `Instant`
//!   (a hang cannot be preempted in a plain test; no thread is spawned).

#[path = "common/corpus_skip.rs"]
mod corpus_skip;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use pr_core::sync::CancelToken;
use pr_imaging_raw::formats::{RawContainerInfo, RawFormat, limits};
use pr_imaging_raw::jpeg_probe::try_read_lossy_frame;
use pr_imaging_raw::readers::cr3::Cr3ContainerReader;
use pr_imaging_raw::{
    InMemoryRawHeaderSource, RAW_HEADER_BLOCK_SIZE, RawContainerReader, RawHeaderError, RawHeaderSource,
};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

/// Canon `uuid` of the `moov` child that carries `CMT1..CMT4` and `THMB`: 85c0b687-820f-11e0-8111-f4ce462b6a48.
const CANON_UUID: [u8; 16] = [
    0x85, 0xC0, 0xB6, 0x87, 0x82, 0x0F, 0x11, 0xE0, 0x81, 0x11, 0xF4, 0xCE, 0x46, 0x2B, 0x6A, 0x48,
];
/// Canon `uuid` of the top-level preview box that carries `PRVW`: eaf42b5e-1c98-4b88-b9fb-b7dc406e4d16.
const PREVIEW_UUID: [u8; 16] = [
    0xEA, 0xF4, 0x2B, 0x5E, 0x1C, 0x98, 0x4B, 0x88, 0xB9, 0xFB, 0xB7, 0xDC, 0x40, 0x6E, 0x4D, 0x16,
];

/// C# `RawContainerLimits.MaxHeaderBytes` as `i64` (the header budget every source enforces).
const MAX_HEADER_BYTES: i64 = limits::MAX_HEADER_BYTES as i64;
/// C# `SourceRawHeaderSource.BlockSize` / `InMemoryRawHeaderSource` block (64 KiB).
const BLOCK_BYTES: i64 = RAW_HEADER_BLOCK_SIZE as i64;

/// C# `Cat(params byte[][])`: concatenates byte-like parts (`Vec<u8>`, `[u8; N]`, `&[u8]`, ...).
macro_rules! cat {
    ($($part:expr),* $(,)?) => {{
        let mut out: Vec<u8> = Vec::new();
        $( out.extend_from_slice(AsRef::<[u8]>::as_ref(&$part)); )*
        out
    }};
}

// ------------------------------------------------------------------ byte builders (C# `Box`, `U16`, `U32`, `U64`, `Ascii`, `Ftyp`)

/// Literal byte array as `Vec<u8>` (typed here, so the integer literals infer `u8`).
fn b(bytes: &[u8]) -> Vec<u8> {
    bytes.to_vec()
}

fn ascii(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

fn u16be(value: u16) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn u32be(value: u32) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn u64be(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

/// C# `Box(type, parts)`: a 32-bit size box with the given four-character type.
fn bx(kind: &str, payload: impl AsRef<[u8]>) -> Vec<u8> {
    let payload = payload.as_ref();
    let mut out = u32be(u32::try_from(8 + payload.len()).expect("test box fits u32"));
    out.extend_from_slice(kind.as_bytes());
    out.extend_from_slice(payload);
    out
}

/// C# `UuidBox(uuid, parts)`: a `uuid` box whose payload starts with the 16-byte extended type.
fn uuid_bx(uuid: &[u8; 16], payload: impl AsRef<[u8]>) -> Vec<u8> {
    bx("uuid", cat!(uuid, payload))
}

/// C# `Ftyp()`: `ftyp` with major brand `crx `, minor version 1, compatible brand `crx `.
fn ftyp() -> Vec<u8> {
    bx("ftyp", cat!(ascii("crx "), u32be(1), ascii("crx ")))
}

/// C# `SyntheticRawBuilder.CreateMinimalJpeg(width, height)`: SOI APP0 DQT SOF0 DHT SOS data EOI (163 bytes).
fn minimal_jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut v = b(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10]);
    v.extend_from_slice(b"JFIF\0");
    v.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
    v.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    v.extend_from_slice(&[16u8; 64]);
    v.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
    v.extend_from_slice(&height.to_be_bytes());
    v.extend_from_slice(&width.to_be_bytes());
    v.push(3);
    v.extend_from_slice(&[0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
    v.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x1F, 0x00]);
    v.extend_from_slice(&[
        0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]);
    v.extend_from_slice(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B]);
    v.extend_from_slice(&[
        0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00,
    ]);
    v.extend_from_slice(&[0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55]);
    v.extend_from_slice(&[0xFF, 0xD9]);
    v
}

/// C# `Cr3BmffHardeningTests.LosslessJpeg(w, h)`: SOI, SOF3 (length 11, precision 14, h, w, 1 component), 256 filler bytes (271 bytes).
fn lossless_jpeg(width: u16, height: u16) -> Vec<u8> {
    cat!(
        b(&[0xFF, 0xD8, 0xFF, 0xC3, 0x00, 0x0B, 14]),
        u16be(height),
        u16be(width),
        b(&[1, 1, 0x11, 0]),
        vec![0u8; 256]
    )
}

/// C# `Cr3BmffHardeningTests.PreviewUuidBox(jpeg, width, height, declaredJpegSize)`: uuid(prefix `u32 0, u32 1`, PRVW with a 16-byte header).
fn preview_uuid_box(jpeg: &[u8], width: u16, height: u16, declared: Option<u32>) -> Vec<u8> {
    let declared = declared.unwrap_or(u32::try_from(jpeg.len()).expect("jpeg fits u32"));
    let prvw = bx(
        "PRVW",
        cat!(
            u32be(0),
            u16be(1),
            u16be(width),
            u16be(height),
            u16be(1),
            u32be(declared),
            jpeg
        ),
    );
    uuid_bx(&PREVIEW_UUID, cat!(u32be(0), u32be(1), prvw))
}

/// C# `Cr3BmffHardeningTests.ThmbBox(jpeg, width, height)`: `THMB` payload `u32 0, u16 w, u16 h, u32 size, u16 1, u16 0, JPEG`.
fn thmb_box(jpeg: &[u8], width: u16, height: u16) -> Vec<u8> {
    bx(
        "THMB",
        cat!(
            u32be(0),
            u16be(width),
            u16be(height),
            u32be(u32::try_from(jpeg.len()).expect("jpeg fits u32")),
            u16be(1),
            u16be(0),
            jpeg
        ),
    )
}

/// C# `BmffCr3MutationGapTests.PrvwBox(jpeg, width, height)`: a top-level-style `PRVW` payload (no uuid prefix around it).
fn prvw_box(jpeg: &[u8], width: u16, height: u16) -> Vec<u8> {
    bx(
        "PRVW",
        cat!(
            u32be(0),
            u16be(1),
            u16be(width),
            u16be(height),
            u16be(1),
            u32be(u32::try_from(jpeg.len()).expect("jpeg fits u32")),
            jpeg
        ),
    )
}

/// C# `BmffCr3MutationGapTests.Stsz(uniform, count)`.
fn stsz(uniform: u32, count: u32) -> Vec<u8> {
    bx("stsz", cat!(u32be(0), u32be(uniform), u32be(count)))
}

/// C# `Co64(offset)` (`co64` with one entry, a 64-bit offset).
fn co64(offset: u64) -> Vec<u8> {
    bx("co64", cat!(u32be(0), u32be(1), u64be(offset)))
}

/// C# `Trak(stblChildren)`: `trak / mdia / minf / stbl` around the given (already built) children.
fn trak(stbl_children: &[Vec<u8>]) -> Vec<u8> {
    bx("trak", bx("mdia", bx("minf", bx("stbl", stbl_children.concat()))))
}

/// C# `BmffCr3MutationGapTests.TrakWithCo64` / `Cr3BmffHardeningTests.TrakWithCo64`: `stsz(sample, 1)` + `co64(chunk)`.
fn trak_with_co64(sample_size: u32, chunk_offset: u64) -> Vec<u8> {
    trak(&[
        bx("stsz", cat!(u32be(0), u32be(sample_size), u32be(1))),
        co64(chunk_offset),
    ])
}

/// C# `Cr3BmffHardeningTests.Cmt1` payload helper: a little-endian TIFF with IFD0 at 8 holding the given (tag, type, value) entries.
fn le_cmt1(entries: &[(u16, u16, u32)]) -> Vec<u8> {
    let mut v = b(&[0x49, 0x49, 0x2A, 0x00, 8, 0, 0, 0]);
    v.extend_from_slice(&u16::try_from(entries.len()).expect("entry count").to_le_bytes());
    for &(tag, typ, value) in entries {
        v.extend_from_slice(&tag.to_le_bytes());
        v.extend_from_slice(&typ.to_le_bytes());
        v.extend_from_slice(&1u32.to_le_bytes());
        v.extend_from_slice(&value.to_le_bytes());
    }
    v.extend_from_slice(&[0, 0, 0, 0]);
    v
}

/// C# `TiffBytes` entry: `(Tag, Type, Count, Value)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    tag: u16,
    typ: u16,
    count: u32,
    value: u32,
}

/// C# `TiffBytes.Short(tag, value)`: SHORT count 1, left-justified in the value field.
fn short(tag: u16, value: u32) -> Entry {
    Entry {
        tag,
        typ: 3,
        count: 1,
        value,
    }
}

/// C# `TiffBytes.Long(tag, value)`: LONG count 1.
fn long(tag: u16, value: u32) -> Entry {
    Entry {
        tag,
        typ: 4,
        count: 1,
        value,
    }
}

/// C# `TiffBytes.At(tag, type, count, offset)`: an out-of-line entry (the value field is an offset).
fn at_entry(tag: u16, typ: u16, count: u32, offset: u32) -> Entry {
    Entry {
        tag,
        typ,
        count,
        value: offset,
    }
}

/// C# `TiffBytes`: a zero-filled buffer laid out byte by byte at explicit offsets, in the given byte order.
struct TiffBytes {
    little: bool,
    buf: Vec<u8>,
}

impl TiffBytes {
    fn new(little: bool, size: usize) -> Self {
        Self {
            little,
            buf: vec![0; size],
        }
    }

    /// `Header(ifd0Offset)`: `II*\0` or `MM\0*` and the IFD0 offset at 4.
    fn header(mut self, ifd0: u32) -> Self {
        let magic: &[u8] = if self.little { b"II*\0" } else { b"MM\0*" };
        self.buf[..4].copy_from_slice(magic);
        self.write_u32(4, ifd0);
        self
    }

    /// `Put(position, bytes)`: raw bytes at an explicit position.
    fn put(mut self, position: usize, bytes: &[u8]) -> Self {
        self.buf[position..position + bytes.len()].copy_from_slice(bytes);
        self
    }

    fn write_u16(&mut self, position: usize, value: u16) {
        let bytes = if self.little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        self.buf[position..position + 2].copy_from_slice(&bytes);
    }

    fn write_u32(&mut self, position: usize, value: u32) {
        let bytes = if self.little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        self.buf[position..position + 4].copy_from_slice(&bytes);
    }

    /// `Ifd(position, nextIfd, entries)`: the entry count, the entries and the next-IFD pointer.
    fn ifd(mut self, position: usize, next: u32, entries: &[Entry]) -> Self {
        self.write_u16(position, u16::try_from(entries.len()).expect("entry count"));
        for (i, e) in entries.iter().enumerate() {
            let at = position + 2 + i * 12;
            self.write_u16(at, e.tag);
            self.write_u16(at + 2, e.typ);
            self.write_u32(at + 4, e.count);
            if e.typ == 3 && e.count == 1 {
                self.write_u16(at + 8, u16::try_from(e.value).expect("SHORT value"));
            } else {
                self.write_u32(at + 8, e.value);
            }
        }
        let next_at = position + 2 + entries.len() * 12;
        self.write_u32(next_at, next);
        self
    }

    fn to_vec(&self) -> Vec<u8> {
        self.buf.clone()
    }
}

/// C# `Cr3BmffMutationGapTests.Cmt1Tiff(ifd0, entries)`: a standalone little-endian TIFF whose IFD0 sits at `ifd0`.
fn cmt1_tiff(ifd0: u32, entries: &[Entry]) -> Vec<u8> {
    let size = ifd0 as usize + 2 + entries.len() * 12 + 4;
    TiffBytes::new(true, size)
        .header(ifd0)
        .ifd(ifd0 as usize, 0, entries)
        .to_vec()
}

/// C# `CanonCmt1(entries)`: `ftyp + moov(canon uuid(CMT1 with IFD0 at 8))`.
fn canon_cmt1_file(entries: &[Entry]) -> Vec<u8> {
    cat!(
        ftyp(),
        bx("moov", uuid_bx(&CANON_UUID, bx("CMT1", cmt1_tiff(8, entries))))
    )
}

/// C# `SyntheticRawBuilder.BuildIsoBmff(jpeg)`: a 16-byte `ftyp` followed by a top-level lowercase `prvw` box holding the JPEG.
fn build_iso_bmff(jpeg: &[u8]) -> Vec<u8> {
    let prvw_len = u32::try_from(8 + jpeg.len()).expect("jpeg fits u32");
    cat!(
        u32be(16),
        ascii("ftyp"),
        ascii("crx "),
        u32be(1),
        u32be(prvw_len),
        ascii("prvw"),
        jpeg
    )
}

/// C# `SyntheticRawBuilder.BuildCanonCr3(prvwJpeg, thumbJpeg, trackJpeg)`: ftyp, moov (Canon uuid with CMT1 + THMB, one trak with
/// stsz/co64), a preview uuid (8-byte prefix + PRVW with a 16-byte header) and the track sample JPEG after it. Defaults: 1620x1080
/// PRVW, 160x120 THMB, 6000x4000 track.
fn build_canon_cr3(prvw: Option<Vec<u8>>, thumb: Option<Vec<u8>>, track: Option<Vec<u8>>) -> Vec<u8> {
    let prvw = prvw.unwrap_or_else(|| minimal_jpeg(1620, 1080));
    let thumb = thumb.unwrap_or_else(|| minimal_jpeg(160, 120));
    let track = track.unwrap_or_else(|| minimal_jpeg(6000, 4000));
    // CMT1: little-endian TIFF with Orientation 6, ImageWidth 6000, ImageLength 4000 (entries LONG/LONG/SHORT as in the C# builder).
    let cmt1 = cat!(
        b(&[0x49, 0x49, 0x2A, 0x00]),
        le_u32(8),
        le_u16(3),
        le_entry(0x0100, 4, 6000),
        le_entry(0x0101, 4, 4000),
        le_entry(0x0112, 3, 6),
        le_u32(0)
    );
    let thmb = bx(
        "THMB",
        cat!(
            u32be(0),
            u16be(160),
            u16be(120),
            u32be(u32::try_from(thumb.len()).expect("fits")),
            u16be(1),
            u16be(0),
            thumb
        ),
    );
    let prvw_payload = bx(
        "PRVW",
        cat!(
            u32be(0),
            u16be(1),
            u16be(1620),
            u16be(1080),
            u16be(1),
            u32be(u32::try_from(prvw.len()).expect("fits")),
            prvw
        ),
    );
    let preview_box = uuid_bx(&PREVIEW_UUID, cat!(u32be(0), u32be(1), prvw_payload));
    let ftyp_box = bx("ftyp", cat!(ascii("crx "), u32be(1), ascii("crx ")));
    let moov = |chunk_offset: u64| {
        bx(
            "moov",
            cat!(
                uuid_bx(&CANON_UUID, cat!(bx("CMT1", &cmt1), &thmb)),
                trak(&[
                    bx(
                        "stsz",
                        cat!(u32be(0), u32be(u32::try_from(track.len()).expect("fits")), u32be(1))
                    ),
                    co64(chunk_offset),
                ])
            ),
        )
    };
    // The chunk offset does not change the moov size: size it once, then place the track JPEG after the preview box.
    let track_offset = (ftyp_box.len() + moov(0).len() + preview_box.len()) as u64;
    cat!(ftyp_box, moov(track_offset), preview_box, track)
}

fn le_u16(value: u16) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

fn le_u32(value: u32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

fn le_entry(tag: u16, typ: u16, value: u32) -> Vec<u8> {
    cat!(le_u16(tag), le_u16(typ), le_u32(1), le_u32(value))
}

/// C# `Cr3BmffHardeningTests.Read` / `Read(file)` helper: the reader over an in-memory source with an uncanceled token.
fn try_read(file: &[u8]) -> Result<RawContainerInfo, RawHeaderError> {
    let mut source = InMemoryRawHeaderSource::new(file.to_vec());
    Cr3ContainerReader.read(&mut source, &CancelToken::none())
}

fn read_info(file: &[u8]) -> RawContainerInfo {
    try_read(file).unwrap_or_else(|e| panic!("the reader rejected a valid test file: {e}"))
}

fn is_invalid_data(err: &RawHeaderError) -> bool {
    matches!(err, RawHeaderError::InvalidData { io: None, .. })
}

/// Offset of the track JPEG of `build_canon_cr3` output: the track JPEG is the last part of the file, so it is the file length minus its length.
fn track_offset_of_canon(file: &[u8], track_len: usize) -> i64 {
    i64::try_from(file.len() - track_len).expect("offset fits i64")
}

// ------------------------------------------------------------------ header sources used by the ported tests

/// Counts every read and records (offset, count) pairs; optionally fails one read (C# `FaultInjecting` sources of the
/// Rust twins: the contract's `failing_source_at_every_read_index_returns_err_not_panic` and
/// `track_probe_io_failure_propagates_but_budget_failure_is_swallowed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    /// An I/O failure wrapped in `InvalidData` (the C# `InvalidDataException` with an `IOException` inner).
    Io,
    /// A plain `InvalidData` (the budget / hostile-file failure).
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    /// The k-th read call (0-based).
    Nth(usize),
    /// The first read at this exact offset.
    Offset(i64),
}

struct FaultSource {
    inner: InMemoryRawHeaderSource,
    trigger: Option<(Trigger, Fault)>,
    reads: Vec<(i64, i32)>,
    /// The fault fires once (the doc of [`Trigger::Offset`] says "the FIRST read at this exact offset").
    fired: bool,
}

impl FaultSource {
    fn new(file: &[u8], trigger: Option<(Trigger, Fault)>) -> Self {
        Self {
            inner: InMemoryRawHeaderSource::new(file.to_vec()),
            trigger,
            reads: Vec::new(),
            fired: false,
        }
    }
}

impl RawHeaderSource for FaultSource {
    fn length(&self) -> i64 {
        self.inner.length()
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        self.reads.push((offset, count));
        let hit = !self.fired
            && match self.trigger {
                Some((Trigger::Nth(k), _)) => self.reads.len() - 1 == k,
                Some((Trigger::Offset(o), _)) => offset == o,
                None => false,
            };
        if hit {
            self.fired = true;
            let fault = self.trigger.map(|(_, f)| f).unwrap_or(Fault::Invalid);
            return Err(match fault {
                Fault::Io => RawHeaderError::invalid_data_io("injected I/O failure", io::Error::other("injected")),
                Fault::Invalid => RawHeaderError::invalid_data("injected invalid data"),
            });
        }
        self.inner.read(offset, count)
    }
}

/// Cancels the token on the first read at or past `from` (the C# `CancelOnRead` hook of the top-level cancellation test).
struct CancelOnRead {
    inner: InMemoryRawHeaderSource,
    token: CancelToken,
    from: i64,
    fired: bool,
}

impl RawHeaderSource for CancelOnRead {
    fn length(&self) -> i64 {
        self.inner.length()
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        if !self.fired && offset >= self.from {
            self.fired = true;
            self.token.cancel();
        }
        self.inner.read(offset, count)
    }
}

/// C# `Robustness/RawContainerFuzzTests.FuzzTrackingHeaderSource`: a line port (no 64 KiB blocks). Offsets outside the source and
/// non-positive counts give an empty slice; in-range reads are clamped; the running total of clamped counts above the budget is
/// `InvalidData`.
struct FuzzTrackingSource {
    bytes: Vec<u8>,
    total: i64,
}

impl FuzzTrackingSource {
    fn new(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.to_vec(),
            total: 0,
        }
    }
}

impl RawHeaderSource for FuzzTrackingSource {
    fn length(&self) -> i64 {
        i64::try_from(self.bytes.len()).expect("fuzz input fits i64")
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        let len = self.length();
        if offset < 0 || offset >= len || count <= 0 {
            return Ok(&[]);
        }
        let available = i64::from(count).min(len - offset);
        self.total += available;
        if self.total > MAX_HEADER_BYTES {
            return Err(RawHeaderError::invalid_data("Exceeded maximum header read budget."));
        }
        let start = usize::try_from(offset).expect("offset checked above");
        let end = start + usize::try_from(available).expect("available is positive");
        Ok(&self.bytes[start..end])
    }
}

/// `TotalBytesRead` of the source that backs a `FuzzTrackingSource` (the fuzz loop reads it after every mutant).
fn fuzz_total(source: &FuzzTrackingSource) -> i64 {
    source.total
}

// ------------------------------------------------------------------ deterministic mutators (C# `JpegBytes.Mutate`, `BinaryFuzz.Mutants`)

/// Deterministic xorshift64 (no `rand` crate in the workspace; the C# `Random(seed)` sequence is not reproduced).
struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform-ish value in `0..n` (`n > 0`).
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next_u64() % u64::try_from(n).expect("n fits u64")).expect("below fits usize")
    }

    /// Value in `lo..=hi` (C# `Random.Next(lo, hi + 1)`).
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }

    fn byte(&mut self) -> u8 {
        (self.next_u64() & 0xFF) as u8
    }
}

/// C# `JpegBytes.Mutate(rng, input)`: 1..=3 rounds, each one of 9 structural or bit-level mutations; an empty buffer gets one random byte.
fn mutate(rng: &mut XorShift64, input: &[u8]) -> Vec<u8> {
    let mut data = input.to_vec();
    let rounds = rng.range(1, 3);
    for _ in 0..rounds {
        if data.is_empty() {
            data.push(rng.byte());
            continue;
        }
        let len = data.len();
        match rng.below(9) {
            0 => {
                let i = rng.below(len);
                data[i] = rng.byte();
            }
            1 => {
                let i = rng.below(len);
                data[i] ^= 1 << rng.below(8);
            }
            2 => {
                let pos = rng.below(len);
                let n = rng.range(1, 8).min(len - pos);
                data.drain(pos..pos + n);
            }
            3 => {
                let at = rng.below(len + 1);
                let n = rng.range(1, 8);
                let fresh: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
                data.splice(at..at, fresh);
            }
            4 => {
                let keep = rng.below(len + 1);
                data.truncate(keep);
            }
            5 => {
                let i = rng.below(len);
                data[i] = 0xFF;
            }
            6 => {
                let i = rng.below(len);
                data[i] = 0x00;
            }
            7 => {
                // Duplicate a slice somewhere else (offsets and loops).
                let start = rng.below(len);
                let n = rng.range(1, 23).min(len - start);
                let slice = data[start..start + n].to_vec();
                let at = rng.below(len + 1);
                data.splice(at..at, slice);
            }
            _ => {
                // Overwrite two bytes with an extreme 16-bit value.
                let pos = rng.below(len);
                let extreme: [u8; 2] = match rng.below(3) {
                    0 => [0xFF, 0xFF],
                    1 => [0x7F, 0xFF],
                    _ => [0x00, 0x00],
                };
                for (i, value) in extreme.iter().enumerate() {
                    if pos + i < data.len() {
                        data[pos + i] = *value;
                    }
                }
            }
        }
    }
    data
}

/// One labelled mutant (C# `BinaryFuzz.Case`).
struct Case {
    label: String,
    bytes: Vec<u8>,
}

/// C# `BinaryFuzz.Mutants(seed, rngSeed, randomCount = 1500)`: the seed first, truncation (every offset of a file of at most 4 000
/// bytes; otherwise the dense head, the dense tail and a spread between), 4-byte extremes in both byte orders and 2-byte extremes at
/// strided positions (`ceil(len / 222)`), then seeded bit flips, byte stores, splice-overs and splice-inserts.
fn binary_fuzz_mutants(seed: &[u8], rng_seed: u64, random_count: usize) -> Vec<Case> {
    const FAMILY_CAP: usize = 4_000;
    let mut cases = vec![Case {
        label: "seed".to_owned(),
        bytes: seed.to_vec(),
    }];
    let len = seed.len();
    // 1. Truncation.
    let offsets: BTreeSet<usize> = if len <= FAMILY_CAP {
        (0..len).collect()
    } else {
        let step = (len / (FAMILY_CAP - 768)).max(1);
        let mut set: BTreeSet<usize> = (0..512).collect();
        set.extend(len - 256..len);
        set.extend((512..len).step_by(step));
        set
    };
    for keep in offsets {
        cases.push(Case {
            label: format!("truncate@{keep}"),
            bytes: seed[..keep].to_vec(),
        });
    }
    // 2. Length / offset field overwrites.
    let extremes32: [u32; 5] = [0, 0x7FFF_FFFF, 0xFFFF_FFFF, 0x8000_0000, 1];
    let extremes16: [u16; 4] = [0, 0x7FFF, 0xFFFF, 0x8000];
    let cap = FAMILY_CAP / (extremes32.len() * 2 + extremes16.len() * 2);
    let step = len.div_ceil(cap.max(1)).max(1);
    for pos in (0..len).step_by(step) {
        if pos + 4 <= len {
            for value in extremes32 {
                let mut le = seed.to_vec();
                le[pos..pos + 4].copy_from_slice(&value.to_le_bytes());
                cases.push(Case {
                    label: format!("u32le@{pos}={value:08X}"),
                    bytes: le,
                });
                let mut be = seed.to_vec();
                be[pos..pos + 4].copy_from_slice(&value.to_be_bytes());
                cases.push(Case {
                    label: format!("u32be@{pos}={value:08X}"),
                    bytes: be,
                });
            }
        }
        if pos + 2 <= len {
            for value in extremes16 {
                let mut le = seed.to_vec();
                le[pos..pos + 2].copy_from_slice(&value.to_le_bytes());
                cases.push(Case {
                    label: format!("u16le@{pos}={value:04X}"),
                    bytes: le,
                });
                let mut be = seed.to_vec();
                be[pos..pos + 2].copy_from_slice(&value.to_be_bytes());
                cases.push(Case {
                    label: format!("u16be@{pos}={value:04X}"),
                    bytes: be,
                });
            }
        }
    }
    // 3. Seeded random mutants.
    let mut rng = XorShift64::new(rng_seed);
    for _ in 0..random_count {
        let mut data = seed.to_vec();
        let label = match rng.below(4) {
            0 => {
                let mut label = String::from("bitflip");
                for _ in 0..rng.range(1, 4) {
                    let p = rng.below(data.len());
                    let bit = rng.below(8);
                    data[p] ^= 1 << bit;
                    label.push_str(&format!(" {p}.{bit}"));
                }
                label
            }
            1 => {
                let p = rng.below(data.len());
                let value = rng.byte();
                data[p] = value;
                format!("store@{p}={value:02X}")
            }
            2 => {
                let from = rng.below(data.len());
                let n = rng.range(1, 64usize.min(data.len() - from));
                let to = rng.below(data.len());
                let n = n.min(data.len() - to);
                data[to..to + n].copy_from_slice(&seed[from..from + n]);
                format!("splice-over from={from} len={n} to={to}")
            }
            _ => {
                let from = rng.below(data.len());
                let n = rng.range(1, 64usize.min(data.len() - from));
                let to = rng.below(data.len() + 1);
                let spliced: Vec<u8> = [&data[..to], &seed[from..from + n], &data[to..]].concat();
                data = spliced;
                format!("splice-insert from={from} len={n} to={to}")
            }
        };
        cases.push(Case { label, bytes: data });
    }
    cases
}

// ------------------------------------------------------------------ golden box language (C# `Commands.W510bEmit`, oracle-cs)

/// C# `W510bBuild`: marks recorded by pass 1 are resolved by pass 2 (box sizes never depend on an offset value).
#[derive(Default)]
struct GoldenBuild {
    marks: BTreeMap<String, i64>,
    resolved: Option<BTreeMap<String, i64>>,
}

impl GoldenBuild {
    /// A scratch build that shares the resolved marks of its parent (C# `new W510bBuild { Resolved = b.Resolved }`).
    fn scratch(&self) -> Self {
        Self {
            marks: BTreeMap::new(),
            resolved: self.resolved.clone(),
        }
    }

    /// C# `Mark(name)`: 0 in pass 1 (sizes only), the resolved offset in pass 2.
    fn mark(&self, name: &str) -> i64 {
        match &self.resolved {
            None => 0,
            Some(map) => *map.get(name).unwrap_or_else(|| panic!("unknown mark {name}")),
        }
    }
}

/// Port of `W510bTiff`: a zero-filled TIFF buffer with the golden `ifd` / `u16` / `u32` / `hex` ops.
struct GoldenTiff {
    little: bool,
    buf: Vec<u8>,
}

impl GoldenTiff {
    fn write_u16(&mut self, position: usize, value: u16) {
        let bytes = if self.little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        self.buf[position..position + 2].copy_from_slice(&bytes);
    }

    fn write_u32(&mut self, position: usize, value: u32) {
        let bytes = if self.little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        self.buf[position..position + 4].copy_from_slice(&bytes);
    }

    /// The golden `ifd` op: `pad` filler SHORT entries (tag 0xF000, value 1) come BEFORE the listed entries; a `count` override
    /// replaces the entry count written at `at`.
    fn ifd(
        &mut self,
        at: usize,
        next: u32,
        entries: &[Value],
        pad: usize,
        build: &GoldenBuild,
        count_override: Option<i64>,
    ) {
        let total = pad + entries.len();
        let count = count_override.map_or(total, |c| usize::try_from(c).unwrap_or(total));
        self.write_u16(at, u16::try_from(count).expect("golden count fits u16"));
        for i in 0..total {
            let p = at + 2 + i * 12;
            let (tag, typ, count, value): (u16, u16, u32, u32) = if i < pad {
                (0xF000, 3, 1, 1)
            } else {
                let e = &entries[i - pad];
                let kind = str_value(&e["k"]);
                let tag = u16::try_from(i64_value(&e["tag"])).expect("golden tag fits u16");
                let (typ, count) = match kind {
                    "short" => (3, 1),
                    "long" => (4, 1),
                    "at" => (
                        u16::try_from(i64_value(&e["type"])).expect("golden type fits u16"),
                        u32::try_from(i64_value(&e["count"])).expect("golden count fits u32"),
                    ),
                    other => panic!("unknown entry kind {other}"),
                };
                let value = match e.get("v_mark") {
                    Some(mark) => {
                        let plus = e.get("plus").map_or(0, i64_value);
                        (build.mark(str_value(mark)) + plus) as u32
                    }
                    None => u32::try_from(i64_value(&e["v"])).expect("golden value fits u32"),
                };
                (tag, typ, count, value)
            };
            assert!(p + 12 <= self.buf.len(), "IFD entry beyond the TIFF buffer");
            self.write_u16(p, tag);
            self.write_u16(p + 2, typ);
            self.write_u32(p + 4, count);
            if typ == 3 && count == 1 {
                self.write_u16(p + 8, value as u16);
            } else {
                self.write_u32(p + 8, value);
            }
        }
        let next_at = at + 2 + total * 12;
        if next_at + 4 <= self.buf.len() {
            self.write_u32(next_at, next);
        }
    }
}

fn str_value(v: &Value) -> &str {
    v.as_str()
        .unwrap_or_else(|| panic!("golden value is not a string: {v}"))
}

fn i64_value(v: &Value) -> i64 {
    v.as_i64()
        .unwrap_or_else(|| panic!("golden value is not an integer: {v}"))
}

/// Decodes a hex string (either case; the empty string is no bytes).
fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0, "odd-length hex string");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap_or_else(|e| panic!("bad hex {text}: {e}")))
        .collect()
}

/// C# `W510bTiffNode`.
fn golden_tiff(node: &Value, build: &GoldenBuild) -> Vec<u8> {
    let little = node.get("le").and_then(Value::as_bool).unwrap_or(true);
    let size = usize::try_from(i64_value(&node["size"])).expect("golden size");
    let mut tiff = GoldenTiff {
        little,
        buf: vec![0; size],
    };
    let signature = node
        .get("signature_hex")
        .map(|v| unhex(str_value(v)))
        .unwrap_or_default();
    let magic = if signature.is_empty() {
        if little { b"II*\0".to_vec() } else { b"MM\0*".to_vec() }
    } else {
        signature
    };
    tiff.buf[..magic.len()].copy_from_slice(&magic);
    if let Some(m) = node.get("magic") {
        tiff.write_u16(2, i64_value(m) as u16);
    }
    tiff.write_u32(4, i64_value(&node["ifd0"]) as u32);
    for op in node["ops"].as_array().map_or(&[][..], Vec::as_slice) {
        let at = usize::try_from(i64_value(&op["at"])).expect("golden at");
        match str_value(&op["op"]) {
            "ifd" => {
                let next = op.get("next").map_or(0, i64_value) as u32;
                let entries = op["entries"].as_array().map_or(&[][..], Vec::as_slice);
                let pad = op.get("pad").map_or(0, |v| usize::try_from(i64_value(v)).expect("pad"));
                let count = op.get("count").map(i64_value);
                tiff.ifd(at, next, entries, pad, build, count);
            }
            "u16" => tiff.write_u16(at, i64_value(&op["v"]) as u16),
            "u32" => tiff.write_u32(at, i64_value(&op["v"]) as u32),
            "hex" => {
                let bytes = unhex(str_value(&op["hex"]));
                tiff.buf[at..at + bytes.len()].copy_from_slice(&bytes);
            }
            other => panic!("unknown tiff op {other}"),
        }
    }
    tiff.buf
}

/// C# `W510bUuidBytes(which)`.
fn golden_uuid(which: &str) -> Vec<u8> {
    match which {
        "canon" => CANON_UUID.to_vec(),
        "preview" => PREVIEW_UUID.to_vec(),
        "foreign" => vec![0; 16],
        other => match other.strip_prefix("hex:") {
            Some(hex) => unhex(hex),
            None => panic!("unknown uuid {other}"),
        },
    }
}

/// C# `W510bEmitList`: concatenation, every node at its absolute offset.
fn emit_list(nodes: &[Value], at: i64, build: &mut GoldenBuild) -> Vec<u8> {
    let mut out = Vec::new();
    for node in nodes {
        let bytes = emit(node, at + i64::try_from(out.len()).expect("length fits i64"), build);
        out.extend_from_slice(&bytes);
    }
    out
}

/// C# `W510bEmit`: the box language (a node is an array, or an object with exactly one of the keys below).
fn emit(node: &Value, at: i64, build: &mut GoldenBuild) -> Vec<u8> {
    if let Some(nodes) = node.as_array() {
        return emit_list(nodes, at, build);
    }
    let o = node
        .as_object()
        .unwrap_or_else(|| panic!("golden node is neither array nor object: {node}"));
    let plus = o.get("plus").map_or(0, i64_value);
    if let Some(hex) = o.get("hex") {
        return unhex(str_value(hex));
    }
    if let Some(zeros) = o.get("zeros") {
        return vec![0; usize::try_from(i64_value(zeros)).expect("zeros")];
    }
    if let Some(text) = o.get("ascii") {
        return str_value(text)
            .chars()
            .map(|c| u8::try_from(u32::from(c)).expect("ascii node is Latin-1"))
            .collect();
    }
    if let Some(v) = o.get("u16") {
        return u16be(i64_value(v) as u16);
    }
    if let Some(v) = o.get("u32") {
        return u32be(i64_value(v) as u32);
    }
    if let Some(v) = o.get("u64") {
        return u64be(str_value(v).parse().expect("u64 decimal string"));
    }
    if let Some(name) = o.get("mark") {
        build.marks.insert(str_value(name).to_owned(), at);
        return Vec::new();
    }
    if let Some(name) = o.get("off32") {
        return u32be((build.mark(str_value(name)) + plus) as u32);
    }
    if let Some(name) = o.get("off64") {
        return u64be((build.mark(str_value(name)) + plus) as u64);
    }
    if let Some(len_of) = o.get("u32_len_of") {
        let mut scratch = build.scratch();
        let len = emit(len_of, at, &mut scratch).len() as i64;
        return u32be((len + plus) as u32);
    }
    if let Some(dims) = o.get("jpeg") {
        return minimal_jpeg(i64_value(&dims[0]) as u16, i64_value(&dims[1]) as u16);
    }
    if let Some(dims) = o.get("sof3") {
        return lossless_jpeg(i64_value(&dims[0]) as u16, i64_value(&dims[1]) as u16);
    }
    if let Some(tiff) = o.get("tiff") {
        return golden_tiff(tiff, build);
    }
    if let Some(times) = o.get("repeat") {
        let mut scratch = build.scratch();
        let one = emit(&o["node"], at, &mut scratch);
        return one.repeat(usize::try_from(i64_value(times)).expect("repeat count"));
    }
    if let Some(slice) = o.get("slice") {
        let bytes = emit(slice, at, build);
        let from = o
            .get("from")
            .map_or(0, |v| usize::try_from(i64_value(v)).expect("from"));
        let to = o
            .get("to")
            .map_or(bytes.len(), |v| usize::try_from(i64_value(v)).expect("to"));
        return bytes[from..to].to_vec();
    }
    let is_box = o.contains_key("box");
    let is_uuid = o.contains_key("uuid");
    if is_box || is_uuid {
        let large = o.get("large").and_then(Value::as_bool).unwrap_or(false);
        let header: i64 = (if large { 16 } else { 8 }) + if is_uuid { 16 } else { 0 };
        let children: &[Value] = o
            .get("children")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        let payload = emit_list(children, at + header, build);
        let total = header + i64::try_from(payload.len()).expect("payload fits i64");
        let kind = if is_box { str_value(&o["box"]) } else { "uuid" };
        let size32: u32 = match o.get("size32") {
            Some(v) => i64_value(v) as u32,
            None if large => 1,
            None => total as u32,
        };
        let mut out = u32be(size32);
        out.extend_from_slice(kind.as_bytes());
        if large {
            let size64: u64 = match o.get("size64") {
                Some(v) => str_value(v).parse().expect("size64 decimal string"),
                None => (total - if is_uuid { 16 } else { 0 }) as u64,
            };
            out.extend(u64be(size64));
        }
        if is_uuid {
            out.extend(golden_uuid(str_value(&o["uuid"])));
        }
        out.extend(payload);
        return out;
    }
    panic!("unknown golden node {node}");
}

/// FNV-1a 64 of the built bytes, lower-case hex (C# `W510bFnv1a64`).
fn fnv1a64_hex(data: &[u8]) -> String {
    let mut h: u64 = 14_695_981_039_346_656_037;
    for &x in data {
        h ^= u64::from(x);
        h = h.wrapping_mul(1_099_511_628_211);
    }
    format!("{h:016x}")
}

/// `PHOTOREVIEW_RS_RAW_CORPUS`, else `<repo>/tests/fixtures/raw-corpus` (C# `RawCorpus`).
fn corpus_dir() -> PathBuf {
    match std::env::var("PHOTOREVIEW_RS_RAW_CORPUS") {
        Ok(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => golden::repo_root().join("tests").join("fixtures").join("raw-corpus"),
    }
}

/// `PHOTOREVIEW_RAW_CORPUS_STRICT` is `1` or `true` (trimmed, case-insensitive): a missing corpus file is a failure, not a skip.
fn corpus_strict() -> bool {
    std::env::var("PHOTOREVIEW_RAW_CORPUS_STRICT")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true"
        })
        .unwrap_or(false)
}

/// Bytes of a corpus file, or `None` when it is missing and the case is optional (a skipped golden row). Missing and not optional,
/// or missing in strict mode, panics naming the file.
fn corpus_bytes(file: &str, optional: bool) -> Option<Vec<u8>> {
    let path = corpus_dir().join(file);
    match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            if optional && !corpus_strict() {
                corpus_skip::skipped(&format!("optional sample {file} absent"));
                None
            } else {
                panic!("RAW corpus sample missing (fetch it first): {} ({e})", path.display())
            }
        }
    }
}

/// C# `Commands.W510bBuildSource`: the bytes of one golden row (`None` for an optional corpus file that is missing).
fn build_source(src: &Value, optional: bool) -> Option<Vec<u8>> {
    match str_value(&src["kind"]) {
        "nodes" => {
            let nodes = src["nodes"].as_array().expect("golden nodes array");
            let mut pass1 = GoldenBuild::default();
            emit_list(nodes, 0, &mut pass1);
            let mut pass2 = GoldenBuild {
                marks: BTreeMap::new(),
                resolved: Some(pass1.marks),
            };
            let mut data = emit_list(nodes, 0, &mut pass2);
            if let Some(to) = src.get("truncate_to") {
                data.truncate(usize::try_from(i64_value(to)).expect("truncate_to"));
            }
            Some(data)
        }
        "corpus" => corpus_bytes(str_value(&src["file"]), optional),
        other => panic!("unknown golden source kind {other}"),
    }
}

/// `tests/golden-inputs/raw-cr3-reader.json`: the case names whose corpus file may be missing (`"optional": true`).
fn optional_cases() -> BTreeSet<String> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("raw-cr3-reader.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .expect("golden-inputs cases")
        .iter()
        .filter(|c| c.get("optional").and_then(Value::as_bool) == Some(true))
        .map(|c| str_value(&c["name"]).to_owned())
        .collect()
}

// ------------------------------------------------------------------ more builders (C# `BmffCr3MutationGapTests`, `Cr3BmffBudgetAndStblGapTests`, `BmffChildBoxScopeTests`)

/// C# `PreviewUuidWith(jpeg, declared, trailing)`: a preview uuid whose PRVW declares `declared` JPEG bytes and carries `trailing` after the JPEG.
fn preview_uuid_with(jpeg: &[u8], declared: u32, trailing: &[u8]) -> Vec<u8> {
    uuid_bx(
        &PREVIEW_UUID,
        cat!(
            u32be(0),
            u32be(1),
            bx(
                "PRVW",
                cat!(
                    u32be(0),
                    u16be(1),
                    u16be(100),
                    u16be(50),
                    u16be(1),
                    u32be(declared),
                    jpeg,
                    trailing
                )
            )
        ),
    )
}

/// C# `Stsz(uniform, count, tail)`: a `stsz` payload with extra bytes after its 12-byte header.
fn stsz_with_tail(uniform: u32, count: u32, tail: &[u8]) -> Vec<u8> {
    bx("stsz", cat!(u32be(0), u32be(uniform), u32be(count), tail))
}

/// C# `Box("stco", U32(0), U32(count), U32(offset))`: an `stco` with one offset entry.
fn stco(offset: u32, count: u32) -> Vec<u8> {
    bx("stco", cat!(u32be(0), u32be(count), u32be(offset)))
}

/// C# `TrackFile(jpeg, trak)`: `ftyp + moov(trak(chunk))` followed by the JPEG; the trak closure receives the JPEG's file offset.
/// The moov size does not depend on the offset (co64 and stco are fixed-size), so it is sized once with offset 0.
fn track_file(jpeg: &[u8], trak_for: impl Fn(u64) -> Vec<u8>) -> Vec<u8> {
    let offset = cat!(ftyp(), bx("moov", trak_for(0))).len() as u64;
    cat!(ftyp(), bx("moov", trak_for(offset)), jpeg)
}

/// C# `FileWithTrack(stblChildren)` (the `Cr3BmffBudgetAndStblGapTests` / `RawContainerGapRound2Tests` variant): the JPEG 640x480 follows
/// the moov and 512 zero bytes pad the file. `stbl_children` receives the JPEG offset and returns the stbl payload.
fn file_with_track(stbl_children: impl Fn(u64) -> Vec<u8>) -> Vec<u8> {
    let moov = |chunk: u64| {
        bx(
            "moov",
            bx("trak", bx("mdia", bx("minf", bx("stbl", stbl_children(chunk))))),
        )
    };
    let jpeg = minimal_jpeg(640, 480);
    let offset = cat!(ftyp(), moov(0)).len() as u64;
    cat!(ftyp(), moov(offset), jpeg, vec![0u8; 512])
}

/// C# `BmffChildBoxScopeTests.BuildCr3(widthValueOffset, trailerForFreeOffset, widthInsidePayload)`: `ftyp + moov(canon uuid(CMT1)) + free`.
/// CMT1 is a 64-byte little-endian TIFF: ImageWidth as a LONG x2 whose value offset is `width_value_offset`, Orientation 6 inline.
fn build_cr3_out_of_line(
    width_value_offset: u32,
    trailer: impl FnOnce(usize) -> Vec<u8>,
    width_inside_payload: bool,
) -> Vec<u8> {
    let mut cmt1 = vec![0u8; 64];
    let head = cat!(
        b(&[0x49, 0x49, 0x2A, 0x00]),
        le_u32(8),
        le_u16(2),
        le_raw_entry(0x0100, 4, 2, width_value_offset),
        le_raw_entry(0x0112, 3, 1, 6),
        le_u32(0)
    );
    cmt1[..head.len()].copy_from_slice(&head);
    if width_inside_payload {
        let at = width_value_offset as usize;
        cmt1[at..at + 4].copy_from_slice(&6000u32.to_le_bytes());
    }
    let ftyp_box = ftyp();
    let moov = bx("moov", uuid_bx(&CANON_UUID, bx("CMT1", &cmt1)));
    let free_payload_offset = ftyp_box.len() + moov.len() + 8;
    cat!(ftyp_box, moov, bx("free", trailer(free_payload_offset)))
}

fn le_raw_entry(tag: u16, typ: u16, count: u32, value: u32) -> Vec<u8> {
    cat!(le_u16(tag), le_u16(typ), le_u32(count), le_u32(value))
}

/// `ftyp` then `count` eight-byte `free` boxes (the flood of `Cr3BmffBudgetAndStblGapTests.FloodOfTinyBoxes`).
fn flood_file(count: usize) -> Vec<u8> {
    let mut flood = Vec::with_capacity(count * 8);
    for _ in 0..count {
        flood.extend_from_slice(&8u32.to_be_bytes());
        flood.extend_from_slice(b"free");
    }
    cat!(ftyp(), flood)
}

/// `Cr3BmffBudgetAndStblGapTests.FloodOfTinyBoxes` count.
const FLOOD_BOXES: usize = 1_200_000;

// ------------------------------------------------------------------ BmffCr3MutationGapTests (reader half, 26 tests)

// C#: Cr3ContainerReader.CanRead (CanRead_TwelveByteCrxFtypHeader_IsAccepted)
#[test]
fn can_read_twelve_byte_crx_ftyp_header_is_accepted() {
    let head = cat!(u32be(16), ascii("ftypcrx "));
    assert_eq!(head.len(), 12);
    assert!(Cr3ContainerReader.can_read(&head, ".cr3"));
}

// C#: Cr3ContainerReader.CanRead (CanRead_ElevenBytes_IsRejected)
#[test]
fn can_read_eleven_bytes_is_rejected() {
    let full = cat!(u32be(16), ascii("ftypcrx "));
    assert!(!Cr3ContainerReader.can_read(&full[..11], ".cr3"));
}

// C#: Cr3ContainerReader.CanRead (CanRead_AnySingleBrandByteWrong_IsRejected, theory positions 4..=11)
#[test]
fn can_read_any_single_brand_byte_wrong_is_rejected() {
    for position in 4..=11usize {
        let mut head = cat!(u32be(16), ascii("ftypcrx "), [0u8; 4]);
        head[position] ^= 0x20;
        assert!(
            !Cr3ContainerReader.can_read(&head, ".cr3"),
            "brand byte {position} flipped must be rejected"
        );
    }
}

// C#: Cr3ContainerReader.CanRead (CanRead_ValidBrandWithWrongExtension_IsRejected)
#[test]
fn can_read_valid_brand_with_wrong_extension_is_rejected() {
    let head = cat!(u32be(16), ascii("ftypcrx "), [0u8; 4]);
    assert!(Cr3ContainerReader.can_read(&head, ".CR3"));
    assert!(!Cr3ContainerReader.can_read(&head, ".cr2"));
}

// C#: Cr3ContainerReader.Read (Read_SixteenByteFileWithOnlyFtyp_ReturnsEmptyInfoInsteadOfThrowing)
#[test]
fn read_sixteen_byte_file_with_only_ftyp_returns_empty_info_instead_of_throwing() {
    let info = read_info(&ftyp());
    assert!(info.previews.is_empty());
    assert_eq!(info.orientation, 1);
    assert_eq!((info.sensor_width, info.sensor_height), (0, 0));
}

// C#: Cr3ContainerReader.Read (Read_FifteenByteFile_ThrowsInvalidData)
#[test]
fn read_fifteen_byte_file_throws_invalid_data() {
    let file = ftyp();
    let err = try_read(&file[..15]).expect_err("a 15-byte file is too short for CR3");
    assert!(is_invalid_data(&err), "expected InvalidData, got {err}");
}

// C#: Cr3ContainerReader.Read (Read_ForeignUuidBoxAtTopLevel_IsNotTreatedAsPreviewUuid)
#[test]
fn read_foreign_uuid_box_at_top_level_is_not_treated_as_preview_uuid() {
    let jpeg = minimal_jpeg(1620, 1080);
    let foreign = uuid_bx(&[0u8; 16], cat!(u32be(0), u32be(1), prvw_box(&jpeg, 1620, 1080)));
    assert!(read_info(&cat!(ftyp(), foreign)).previews.is_empty());
    let own = uuid_bx(&PREVIEW_UUID, cat!(u32be(0), u32be(1), prvw_box(&jpeg, 1620, 1080)));
    assert_eq!(read_info(&cat!(ftyp(), own)).previews.len(), 1);
}

// C#: Cr3ContainerReader.Read (Read_ForeignUuidBoxInsideMoov_IsNotTreatedAsCanonUuid)
#[test]
fn read_foreign_uuid_box_inside_moov_is_not_treated_as_canon_uuid() {
    let foreign = uuid_bx(&[0u8; 16], bx("CMT1", cmt1_tiff(8, &[short(0x0112, 6)])));
    let info = read_info(&cat!(ftyp(), bx("moov", foreign)));
    assert_eq!(info.orientation, 1);
    assert!(info.exif_blocks.is_empty());
}

// C#: Cr3ContainerReader.Read (Read_TopLevelPrvwWithPayloadExactlyTheSoi_IsAPreviewOfTwoBytes)
#[test]
fn read_top_level_prvw_with_payload_exactly_the_soi_is_a_preview_of_two_bytes() {
    let file = cat!(ftyp(), bx("PRVW", b(&[0xFF, 0xD8])));
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 1);
    let preview = &info.previews[0];
    assert_eq!(preview.offset, file.len() as i64 - 2);
    assert_eq!(preview.length, 2);
    assert_eq!((preview.width, preview.height), (0, 0));
}

// C#: Cr3ContainerReader.Read (Read_PrvwWithHeaderAndOnlyTheSoi_IsAPreviewOfTwoBytesWithHeaderSize)
#[test]
fn read_prvw_with_header_and_only_the_soi_is_a_preview_of_two_bytes_with_header_size() {
    // 16-byte header + FFD8 = 18-byte payload: the smallest headered preview.
    let prvw = bx(
        "PRVW",
        cat!(
            u32be(0),
            u16be(1),
            u16be(1620),
            u16be(1080),
            u16be(1),
            u32be(0),
            b(&[0xFF, 0xD8])
        ),
    );
    let file = cat!(ftyp(), uuid_bx(&PREVIEW_UUID, cat!(u32be(0), u32be(1), prvw)));
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 1);
    let preview = &info.previews[0];
    assert_eq!(preview.offset, file.len() as i64 - 2);
    assert_eq!(preview.length, 2);
    assert_eq!((preview.width, preview.height), (1620, 1080));
}

// C#: Cr3ContainerReader.Read (Read_PrvwPayloadShorterThanHeaderPlusSoiAtEndOfFile_IsSkippedWithoutThrowing, rows 14, 15, 16, 17)
#[test]
fn read_prvw_payload_shorter_than_header_plus_soi_at_end_of_file_is_skipped_without_throwing() {
    for payload_bytes in [14usize, 15, 16, 17] {
        let file = cat!(ftyp(), bx("PRVW", vec![0u8; payload_bytes]));
        assert!(
            read_info(&file).previews.is_empty(),
            "payload of {payload_bytes} bytes must be skipped"
        );
    }
}

// C#: Cr3ContainerReader.Read (Read_PrvwDeclaredSizeSmallerThanBoxPayload_UsesDeclaredSize)
#[test]
fn read_prvw_declared_size_smaller_than_box_payload_uses_declared_size() {
    let jpeg = minimal_jpeg(64, 48);
    let file = cat!(
        ftyp(),
        preview_uuid_with(&jpeg, u32::try_from(jpeg.len()).expect("fits"), &[0u8; 10])
    );
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.previews[0].length, jpeg.len() as i64);
}

// C#: Cr3ContainerReader.Read (Read_PrvwDeclaredSizeZero_UsesWholeBoxPayload)
#[test]
fn read_prvw_declared_size_zero_uses_whole_box_payload() {
    let jpeg = minimal_jpeg(64, 48);
    let file = cat!(ftyp(), preview_uuid_with(&jpeg, 0, &[0u8; 10]));
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.previews[0].length, jpeg.len() as i64 + 10);
}

// C#: Cr3ContainerReader.Read (Read_PrvwDeclaredSizeOfOneByte_IsRejectedEvenWithValidSoi)
#[test]
fn read_prvw_declared_size_of_one_byte_is_rejected_even_with_valid_soi() {
    let jpeg = minimal_jpeg(64, 48);
    let file = cat!(ftyp(), preview_uuid_with(&jpeg, 1, &[]));
    assert!(read_info(&file).previews.is_empty());
}

// C#: Cr3ContainerReader.Read (Read_Cmt1SecondOrientationEntry_OverridesOnlyWhenWithinOneToEight, theory rows (second, expected))
#[test]
fn read_cmt1_second_orientation_entry_overrides_only_when_within_one_to_eight() {
    for (second, expected) in [(1u32, 1), (8, 8), (0, 6), (9, 6)] {
        let info = read_info(&canon_cmt1_file(&[short(0x0112, 6), short(0x0112, second)]));
        assert_eq!(info.orientation, expected, "second orientation entry {second}");
    }
}

// C#: Cr3ContainerReader.Read (Read_Cmt1SensorWidthAndHeightOfIntMax_AreAccepted)
#[test]
fn read_cmt1_sensor_width_and_height_of_int_max_are_accepted() {
    let max = u32::try_from(i32::MAX).expect("i32::MAX fits u32");
    let info = read_info(&canon_cmt1_file(&[long(0x0100, max), long(0x0101, max)]));
    assert_eq!((info.sensor_width, info.sensor_height), (i32::MAX, i32::MAX));
}

// C#: Cr3ContainerReader.Read (Read_Cmt1ZeroSensorSize_DoesNotReplaceEarlierValue)
#[test]
fn read_cmt1_zero_sensor_size_does_not_replace_earlier_value() {
    let info = read_info(&canon_cmt1_file(&[
        long(0x0100, 6000),
        long(0x0101, 4000),
        long(0x0100, 0),
        long(0x0101, 0),
    ]));
    assert_eq!((info.sensor_width, info.sensor_height), (6000, 4000));
}

// C#: Cr3ContainerReader.Read (Read_Cmt1Ifd0OffsetInsideTheTiffHeader_IsRejected)
#[test]
fn read_cmt1_ifd0_offset_inside_the_tiff_header_is_rejected() {
    // ifd0 = 2 would read the count from the magic bytes and the IFD would overlap the header.
    let tiff = TiffBytes::new(true, 40)
        .header(2)
        .put(16, &[0x12, 0x01, 0x03, 0x00, 1, 0, 0, 0, 6, 0, 0, 0])
        .to_vec();
    let file = cat!(ftyp(), bx("moov", uuid_bx(&CANON_UUID, bx("CMT1", tiff))));
    assert_eq!(read_info(&file).orientation, 1);
}

// C#: Cr3ContainerReader.Read (Read_Cmt1EntryStraddlingPayloadEnd_IsNotRead)
#[test]
fn read_cmt1_entry_straddling_payload_end_is_not_read() {
    // Entry 0 (width 6000) fits in the 31-byte payload, entry 1 (orientation 6) ends 3 bytes past it, in bytes after the box.
    let tiff = TiffBytes::new(true, 38)
        .header(8)
        .ifd(8, 0, &[short(0x0100, 6000), short(0x0112, 6)])
        .to_vec();
    let file = cat!(
        ftyp(),
        bx("moov", uuid_bx(&CANON_UUID, bx("CMT1", &tiff[..31]))),
        &tiff[31..],
        vec![0u8; 8]
    );
    let info = read_info(&file);
    assert_eq!(info.sensor_width, 6000);
    assert_eq!(info.orientation, 1);
}

// C#: Cr3ContainerReader.Read (Read_Cmt1OutOfLineValue_MustLieFullyInsidePayload, rows (36, 6000), (37, 0))
#[test]
fn read_cmt1_out_of_line_value_must_lie_fully_inside_payload() {
    for (value_offset, expected_width) in [(36u32, 6000), (37, 0)] {
        let mut tiff = TiffBytes::new(true, 40)
            .header(8)
            .ifd(8, 0, &[at_entry(0x0100, 4, 2, value_offset)])
            .to_vec();
        if value_offset == 36 {
            tiff[36] = 0x70; // 0x1770 = 6000, little-endian, in the last four payload bytes
            tiff[37] = 0x17;
        } else {
            tiff[37] = 1; // the value would run past the payload
            tiff[38] = 0;
            tiff[39] = 0;
        }
        let file = cat!(ftyp(), bx("moov", uuid_bx(&CANON_UUID, bx("CMT1", tiff))), vec![0u8; 8]);
        assert_eq!(
            read_info(&file).sensor_width,
            expected_width,
            "value offset {value_offset}"
        );
    }
}

// C#: Cr3ContainerReader.Read (Read_StcoChunkOffset_YieldsTrackPreview)
#[test]
fn read_stco_chunk_offset_yields_track_preview() {
    let jpeg = minimal_jpeg(640, 480);
    let file = track_file(&jpeg, |off| {
        trak(&[
            stsz(u32::try_from(jpeg.len()).expect("fits"), 1),
            stco(u32::try_from(off).expect("fits"), 1),
        ])
    });
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 1);
    let preview = &info.previews[0];
    assert_eq!(preview.offset, file.len() as i64 - jpeg.len() as i64);
    assert_eq!(preview.length, jpeg.len() as i64);
    assert_eq!((preview.width, preview.height), (640, 480));
}

// C#: Cr3ContainerReader.Read (Read_StcoWithoutOffsetEntry_DoesNotFallBackToCo64)
#[test]
fn read_stco_without_offset_entry_does_not_fall_back_to_co64() {
    let jpeg = minimal_jpeg(640, 480);
    let file = track_file(&jpeg, |off| {
        trak(&[
            stsz(u32::try_from(jpeg.len()).expect("fits"), 1),
            bx("stco", cat!(u32be(0), u32be(1))),
            co64(off),
        ])
    });
    assert!(read_info(&file).previews.is_empty());
}

// C#: Cr3ContainerReader.Read (Read_StszWithPerSampleTable_UsesFirstSampleSize)
#[test]
fn read_stsz_with_per_sample_table_uses_first_sample_size() {
    let jpeg = minimal_jpeg(640, 480);
    // Uniform size 0, count 1, then the single 4-byte entry: the payload is exactly 16 bytes.
    let file = track_file(&jpeg, |off| {
        trak(&[
            bx(
                "stsz",
                cat!(
                    u32be(0),
                    u32be(0),
                    u32be(1),
                    u32be(u32::try_from(jpeg.len()).expect("fits"))
                ),
            ),
            co64(off),
        ])
    });
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.previews[0].length, jpeg.len() as i64);
}

// C#: Cr3ContainerReader.Read (Read_StszWithZeroSamplesAndZeroUniformSize_YieldsNoPreview)
#[test]
fn read_stsz_with_zero_samples_and_zero_uniform_size_yields_no_preview() {
    let jpeg = minimal_jpeg(640, 480);
    let file = track_file(&jpeg, |off| {
        trak(&[
            bx(
                "stsz",
                cat!(
                    u32be(0),
                    u32be(0),
                    u32be(0),
                    u32be(u32::try_from(jpeg.len()).expect("fits"))
                ),
            ),
            co64(off),
        ])
    });
    assert!(read_info(&file).previews.is_empty());
}

// C#: Cr3ContainerReader.Read (Read_Co64WithZeroEntryCount_YieldsNoPreviewEvenWhenOffsetBytesArePresent)
#[test]
fn read_co64_with_zero_entry_count_yields_no_preview_even_when_offset_bytes_are_present() {
    let jpeg = minimal_jpeg(640, 480);
    let file = track_file(&jpeg, |off| {
        trak(&[
            stsz(u32::try_from(jpeg.len()).expect("fits"), 1),
            bx("co64", cat!(u32be(0), u32be(0), u64be(off))),
        ])
    });
    assert!(read_info(&file).previews.is_empty());
}

// C#: Cr3ContainerReader.Read (Read_SampleSizeOneByteBeyondEndOfFile_YieldsNoPreview)
#[test]
fn read_sample_size_one_byte_beyond_end_of_file_yields_no_preview() {
    let jpeg = minimal_jpeg(640, 480);
    let file = track_file(&jpeg, |off| {
        trak(&[stsz(u32::try_from(jpeg.len() + 1).expect("fits"), 1), co64(off)])
    });
    assert!(read_info(&file).previews.is_empty());
}

// ------------------------------------------------------------------ BmffChildBoxScopeTests (2 reader tests)

// C#: Cr3ContainerReader.Read (Cmt1_OutOfLineWidthInsideThePayload_IsReadRelativeToThePayload, via BmffChildBoxScopeTests.BuildCr3)
#[test]
fn cmt1_out_of_line_width_inside_the_payload_is_read_relative_to_the_payload() {
    let info = read_info(&build_cr3_out_of_line(40, |_| vec![0u8; 64], true));
    assert_eq!(info.sensor_width, 6000);
    assert_eq!(info.orientation, 6);
}

// C#: Cr3ContainerReader.Read (Cmt1_OutOfLineWidthPointingOutsideThePayload_IsIgnoredEvenIfThatFileOffsetHoldsAWidth)
#[test]
fn cmt1_out_of_line_width_pointing_outside_the_payload_is_ignored_even_if_that_file_offset_holds_a_width() {
    // Two passes: the moov size does not depend on the offset value.
    let probe = build_cr3_out_of_line(0, |_| vec![0u8; 64], false);
    let ftyp_and_moov = probe.len() - (8 + 64);
    let absolute_of_planted = u32::try_from(ftyp_and_moov + 8).expect("fits u32");
    let data = build_cr3_out_of_line(
        absolute_of_planted,
        |_| {
            let mut free = vec![0u8; 64];
            free[..4].copy_from_slice(&6000u32.to_le_bytes());
            free
        },
        false,
    );
    let info = read_info(&data);
    assert_eq!(info.sensor_width, 0);
    assert_eq!(info.orientation, 6, "inline values are unaffected");
}

// ------------------------------------------------------------------ Cr3BmffBudgetAndStblGapTests (reader tests + the flood twin)

// C#: Cr3ContainerReader.Read (Read_MoreThanTheChildCapOfSiblingMoovBoxes_StillReadsTheLastOneWithBoundedHeaderReads)
#[test]
fn read_more_than_the_child_cap_of_sibling_moov_boxes_still_reads_the_last_one_with_bounded_header_reads() {
    let cmt1 = b(&[
        0x49, 0x49, 0x2A, 0x00, 8, 0, 0, 0, // little-endian TIFF header, IFD0 at 8
        1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, // one SHORT Orientation = 6 entry
        0, 0, 0, 0,
    ]);
    let last_moov = bx("moov", uuid_bx(&CANON_UUID, bx("CMT1", &cmt1)));
    let empties: Vec<Vec<u8>> = (0..300).map(|_| bx("moov", Vec::<u8>::new())).collect();
    let file = cat!(ftyp(), empties.concat(), last_moov);
    let mut source = InMemoryRawHeaderSource::new(file.clone());
    let info = Cr3ContainerReader
        .read(&mut source, &CancelToken::none())
        .unwrap_or_else(|e| panic!("the reader rejected a valid file: {e}"));
    assert_eq!(info.orientation, 6);
    // A single block holds the whole small file: every byte is charged exactly once.
    assert_eq!(source.total_bytes_read(), file.len() as i64);
    assert!(source.total_bytes_read() <= BLOCK_BYTES);
}

// C#: Cr3ContainerReader.Read (Read_FloodOfTinyTopLevelBoxes_StopsAtTheHeaderBudgetWithAnInvalidDataExceptionNotAnIoFailure)
// The file-backed variant (`SourceRawHeaderSource`) is W5-12's.
#[test]
fn read_flood_of_tiny_top_level_boxes_stops_at_the_header_budget_with_an_invalid_data_exception_not_an_io_failure() {
    let file = flood_file(FLOOD_BOXES);
    let mut source = InMemoryRawHeaderSource::new(file);
    let err = Cr3ContainerReader
        .read(&mut source, &CancelToken::none())
        .expect_err("1 200 000 boxes exceed the header budget");
    // A budget stop is "corrupt RAW", never mistaken for a disk failure: InvalidData with no inner error.
    assert!(is_invalid_data(&err), "expected InvalidData, got {err}");
    assert!(!err.is_io_failure());
    let charged = source.total_bytes_read();
    assert!(
        charged <= MAX_HEADER_BYTES,
        "charged {charged} bytes, budget {MAX_HEADER_BYTES}"
    );
    assert!(
        charged > MAX_HEADER_BYTES - BLOCK_BYTES,
        "the walk must run to the cap, charged {charged}"
    );
}

// C#: Cr3ContainerReader.Read (Read_StszUniformSizeAndChunkOffsetValid_YieldsTheTrackPreview_Control)
#[test]
fn read_stsz_uniform_size_and_chunk_offset_valid_yields_the_track_preview_control() {
    let jpeg = minimal_jpeg(640, 480);
    let file = file_with_track(|off| cat!(stsz(u32::try_from(jpeg.len()).expect("fits"), 1), co64(off)));
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.previews[0].length, jpeg.len() as i64);
}

// C#: Cr3ContainerReader.Read (Read_StszUniformSizeZeroAndCountZero_YieldsNoPreviewAndDoesNotThrow, rows false / true)
#[test]
fn read_stsz_uniform_size_zero_and_count_zero_yields_no_preview_and_does_not_throw() {
    let jpeg = minimal_jpeg(640, 480);
    for with_table_entry in [false, true] {
        // false: a 12-byte payload with no per-sample table; true: a table entry exists but the count says there are no samples.
        let tail = if with_table_entry {
            u32be(u32::try_from(jpeg.len()).expect("fits"))
        } else {
            Vec::new()
        };
        let file = file_with_track(|off| cat!(stsz_with_tail(0, 0, &tail), co64(off)));
        assert!(
            read_info(&file).previews.is_empty(),
            "with_table_entry={with_table_entry}"
        );
    }
}

// C#: Cr3ContainerReader.Read (Read_StcoCountZero_YieldsNoPreviewAndDoesNotThrow, rows false / true)
#[test]
fn read_stco_count_zero_yields_no_preview_and_does_not_throw() {
    let jpeg = minimal_jpeg(640, 480);
    for with_offset_entry in [false, true] {
        let file = file_with_track(|off| {
            let mut payload = cat!(u32be(0), u32be(0));
            if with_offset_entry {
                payload.extend(u32be(u32::try_from(off).expect("fits")));
            }
            cat!(stsz(u32::try_from(jpeg.len()).expect("fits"), 1), bx("stco", payload))
        });
        assert!(
            read_info(&file).previews.is_empty(),
            "with_offset_entry={with_offset_entry}"
        );
    }
}

// C#: Cr3ContainerReader.Read (Read_StszPayloadBetweenTwelveAndFifteenBytes_DoesNotReadASampleSizeFromTheNextBytes, rows 0..=3)
#[test]
fn read_stsz_payload_between_twelve_and_fifteen_bytes_does_not_read_a_sample_size_from_the_next_bytes() {
    // stsz with uniform size 0 and count 1 needs a 16-byte payload for its first table entry. With a 12..15-byte payload the
    // bytes that follow belong to whatever comes next (here a stray 0x00000100), not to this box.
    let stray = [0u8, 0, 1, 0];
    for extra in 0..=3usize {
        let file = file_with_track(|off| cat!(co64(off), stsz_with_tail(0, 1, &stray[..extra]), &stray[extra..]));
        assert!(read_info(&file).previews.is_empty(), "extra payload bytes {extra}");
    }
}

// ------------------------------------------------------------------ Cr3BmffHardeningTests (15 reader tests)

// C#: Cr3ContainerReader.Read (Read_RealCanonCr3Corpus_ReturnsPrvwPreviewFirstThenThumbnail)
// Rust twin: the C# `PreviewSelector.TryReadJpegFrame` is W5-11's; `try_read_lossy_frame` stands in (it agrees on real SOF0 previews).
// The libjpeg-turbo `ReadInfo` agreement belongs to W5-14, so this twin is NOT ignored.
#[test]
fn read_real_canon_cr3_corpus_returns_prvw_preview_first_then_thumbnail() {
    let dir = corpus_dir();
    let mut files = corpus_cr3_files(&dir);
    if files.is_empty() {
        return;
    }
    files.sort();
    for file in files {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = fs::read(&file).unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        let info = read_info(&bytes);
        // PRVW (1620x1080) is listed first, THMB (160x120) last.
        let prvw = info.previews.first().expect("a preview");
        assert_eq!((prvw.width, prvw.height), (1620, 1080), "{name}: first preview");
        let thumb = info.previews.last().expect("a preview");
        assert_eq!((thumb.width, thumb.height), (160, 120), "{name}: last preview");
        for (i, preview) in info.previews.iter().enumerate() {
            assert_eq!(preview.index, i32::try_from(i).expect("index fits"), "{name}: index");
        }
        for preview in [prvw, thumb] {
            let start = usize::try_from(preview.offset).expect("offset");
            let end = start + usize::try_from(preview.length).expect("length");
            let jpeg = &bytes[start..end];
            assert!(
                jpeg[0] == 0xFF && jpeg[1] == 0xD8,
                "{name}: preview at {} lacks JPEG SOI",
                preview.offset
            );
            let mut source = InMemoryRawHeaderSource::new(jpeg.to_vec());
            let frame = try_read_lossy_frame(&mut source, 0, jpeg.len() as i64)
                .unwrap_or_else(|e| panic!("{name}: probe failed: {e}"))
                .unwrap_or_else(|| panic!("{name}: no lossy frame in the preview"));
            assert_eq!(
                (frame.width, frame.height),
                (preview.width, preview.height),
                "{name}: frame size"
            );
        }
    }
}

// C#: Cr3ContainerReader.Read (Read_PreviewUuidWithEightBytePrefix_FindsPrvwAtSixteenByteHeaderOffset)
#[test]
fn read_preview_uuid_with_eight_byte_prefix_finds_prvw_at_sixteen_byte_header_offset() {
    let jpeg = minimal_jpeg(640, 480);
    let file = cat!(ftyp(), preview_uuid_box(&jpeg, 1620, 1080, None));
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 1);
    let preview = &info.previews[0];
    assert_eq!((preview.width, preview.height), (1620, 1080));
    assert_eq!(preview.length, jpeg.len() as i64);
    assert!(file[preview.offset as usize..].starts_with(&jpeg));
}

// C#: Cr3ContainerReader.Read (Read_PrvwWithoutSoiAtHeaderOffset_IsSkipped)
#[test]
fn read_prvw_without_soi_at_header_offset_is_skipped() {
    let file = cat!(ftyp(), preview_uuid_box(&[0u8; 64], 1620, 1080, None));
    assert!(read_info(&file).previews.is_empty());
}

// C#: Cr3ContainerReader.Read (Read_PrvwDeclaredJpegSizeLargerThanBox_IsClampedToBoxPayload)
#[test]
fn read_prvw_declared_jpeg_size_larger_than_box_is_clamped_to_box_payload() {
    let jpeg = minimal_jpeg(64, 48);
    let file = cat!(ftyp(), preview_uuid_box(&jpeg, 64, 48, Some(u32::MAX)));
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.previews[0].length, jpeg.len() as i64);
}

// C#: Cr3ContainerReader.Read (Read_PrvwAndThmb_ListsPrvwBeforeThumbnailEvenWhenMoovComesFirst)
#[test]
fn read_prvw_and_thmb_lists_prvw_before_thumbnail_even_when_moov_comes_first() {
    let thumb = minimal_jpeg(160, 120);
    let prvw = minimal_jpeg(1620, 1080);
    let file = cat!(
        ftyp(),
        bx("moov", uuid_bx(&CANON_UUID, thmb_box(&thumb, 160, 120))),
        preview_uuid_box(&prvw, 1620, 1080, None)
    );
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 2);
    assert_eq!((info.previews[0].width, info.previews[0].height), (1620, 1080));
    assert_eq!((info.previews[1].width, info.previews[1].height), (160, 120));
    let indices: Vec<i32> = info.previews.iter().map(|p| p.index).collect();
    assert_eq!(indices, vec![0, 1]);
}

// C#: Cr3ContainerReader.Read (Read_ThmbDimensionsComeFromHeaderNotHardCodedConstants)
#[test]
fn read_thmb_dimensions_come_from_header_not_hard_coded_constants() {
    let jpeg = minimal_jpeg(320, 240);
    let file = cat!(ftyp(), bx("moov", uuid_bx(&CANON_UUID, thmb_box(&jpeg, 320, 240))));
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 1);
    assert_eq!((info.previews[0].width, info.previews[0].height), (320, 240));
    assert_eq!(info.previews[0].length, jpeg.len() as i64);
}

// C#: Cr3ContainerReader.Read (Read_ThmbTruncatedAtEndOfFile_DoesNotThrowAndYieldsNoPreview)
#[test]
fn read_thmb_truncated_at_end_of_file_does_not_throw_and_yields_no_preview() {
    // A THMB payload of 13 bytes (> 12) at EOF: an old fixed 32-byte read ran past the file end.
    let file = cat!(ftyp(), bx("moov", uuid_bx(&CANON_UUID, bx("THMB", vec![0u8; 13]))));
    assert!(read_info(&file).previews.is_empty());
}

// C#: Cr3ContainerReader.Read (Read_Cmt1SensorSizeAboveIntMax_IsIgnoredInsteadOfGoingNegative)
#[test]
fn read_cmt1_sensor_size_above_int_max_is_ignored_instead_of_going_negative() {
    let cmt1 = le_cmt1(&[(0x0100, 4, 0xFFFF_FFFF), (0x0101, 4, 4000), (0x0112, 3, 6)]);
    let file = cat!(ftyp(), bx("moov", uuid_bx(&CANON_UUID, bx("CMT1", cmt1))));
    let info = read_info(&file);
    assert_eq!(info.sensor_width, 0);
    assert_eq!(info.sensor_height, 4000);
    assert_eq!(info.orientation, 6);
}

// C#: Cr3ContainerReader.Read (Read_Cmt1Ifd0OffsetOutsidePayload_IsNotFollowedIntoUnrelatedFileBytes)
#[test]
fn read_cmt1_ifd0_offset_outside_payload_is_not_followed_into_unrelated_file_bytes() {
    // The CMT1 payload is 16 bytes with IFD0 pointing 200 bytes past its start, where the rest of the file holds a plausible IFD (orientation 6).
    let mut cmt1_payload = vec![0u8; 16];
    cmt1_payload[0] = b'I';
    cmt1_payload[1] = b'I';
    cmt1_payload[2] = 0x2A;
    cmt1_payload[4..8].copy_from_slice(&200u32.to_le_bytes());
    let head = cat!(ftyp(), bx("moov", uuid_bx(&CANON_UUID, bx("CMT1", &cmt1_payload))));
    let cmt1_payload_offset = head.windows(4).position(|w| w == b"CMT1").expect("CMT1 in head") + 4;
    let mut file = vec![0u8; cmt1_payload_offset + 200 + 64];
    file[..head.len()].copy_from_slice(&head);
    let ifd = cmt1_payload_offset + 200;
    file[ifd..ifd + 2].copy_from_slice(&1u16.to_le_bytes());
    file[ifd + 2..ifd + 4].copy_from_slice(&0x0112u16.to_le_bytes());
    file[ifd + 4..ifd + 6].copy_from_slice(&3u16.to_le_bytes());
    file[ifd + 6..ifd + 10].copy_from_slice(&1u32.to_le_bytes());
    file[ifd + 10..ifd + 14].copy_from_slice(&6u32.to_le_bytes());
    assert_eq!(read_info(&file).orientation, 1);
}

// C#: Cr3ContainerReader.Read (Read_Co64ChunkOffsetNearLongMax_DoesNotOverflowAndYieldsNoPreview, rows i64::MAX-1, i64::MAX-50)
#[test]
fn read_co64_chunk_offset_near_long_max_does_not_overflow_and_yields_no_preview() {
    for chunk in [i64::MAX - 1, i64::MAX - 50] {
        let file = cat!(ftyp(), bx("moov", trak_with_co64(100, chunk as u64)));
        assert!(read_info(&file).previews.is_empty(), "chunk offset {chunk}");
    }
}

// C#: Cr3ContainerReader.Read (Read_Co64ChunkOffsetAboveLongMax_YieldsNoPreview)
#[test]
fn read_co64_chunk_offset_above_long_max_yields_no_preview() {
    let file = cat!(ftyp(), bx("moov", trak_with_co64(100, u64::MAX - 3)));
    assert!(read_info(&file).previews.is_empty());
}

// C#: Cr3ContainerReader.Read (Read_Co64PointingAtValidJpegInsideFile_YieldsTrackPreview)
#[test]
fn read_co64_pointing_at_valid_jpeg_inside_file_yields_track_preview() {
    let jpeg = minimal_jpeg(640, 480);
    let file = track_file(&jpeg, |off| {
        trak_with_co64(u32::try_from(jpeg.len()).expect("fits"), off)
    });
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.previews[0].offset, file.len() as i64 - jpeg.len() as i64);
    assert_eq!(info.previews[0].length, jpeg.len() as i64);
}

// C#: Cr3ContainerReader.Read (Read_TrakSampleIsLosslessJpegSof3_IsNotAPreviewCandidate)
#[test]
fn read_trak_sample_is_lossless_jpeg_sof3_is_not_a_preview_candidate() {
    // The CR3 raw track is a lossless JPEG (FFD8 FFC3 ...): it shares the SOI with real previews but cannot be decoded as one.
    let raw = lossless_jpeg(4000, 3000);
    let prvw = preview_uuid_box(&minimal_jpeg(1620, 1080), 1620, 1080, None);
    let raw_len = u32::try_from(raw.len()).expect("fits");
    let prefix = cat!(ftyp(), &prvw, bx("moov", trak_with_co64(raw_len, 0)));
    let chunk = prefix.len() as u64;
    let file = cat!(ftyp(), &prvw, bx("moov", trak_with_co64(raw_len, chunk)), &raw);
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 1, "only the PRVW survives");
    assert_eq!((info.previews[0].width, info.previews[0].height), (1620, 1080));
    assert!(info.previews.iter().all(|p| p.offset != chunk as i64));
}

// C#: Cr3ContainerReader.Read (Read_TrakSampleIsLosslessJpegSof3AndPrvwCorrupt_YieldsNoPreviewRangeCoveringRawTrack)
#[test]
fn read_trak_sample_is_lossless_jpeg_sof3_and_prvw_corrupt_yields_no_preview_range_covering_raw_track() {
    let raw = lossless_jpeg(4000, 3000);
    let bad_prvw = preview_uuid_box(&[0u8; 64], 1620, 1080, None);
    let raw_len = u32::try_from(raw.len()).expect("fits");
    let prefix = cat!(ftyp(), &bad_prvw, bx("moov", trak_with_co64(raw_len, 0)));
    let chunk = prefix.len() as u64;
    let file = cat!(ftyp(), &bad_prvw, bx("moov", trak_with_co64(raw_len, chunk)), &raw);
    assert!(read_info(&file).previews.is_empty());
}

// C#: Cr3ContainerReader.Read (Read_TrakSampleIsLossyJpeg_UsesProbedDimensions)
#[test]
fn read_trak_sample_is_lossy_jpeg_uses_probed_dimensions() {
    let jpeg = minimal_jpeg(640, 480);
    let file = track_file(&jpeg, |off| {
        trak_with_co64(u32::try_from(jpeg.len()).expect("fits"), off)
    });
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 1);
    assert_eq!((info.previews[0].width, info.previews[0].height), (640, 480));
}

// ------------------------------------------------------------------ RawContainerGapRound2Tests (7 reader tests)

// C#: Cr3ContainerReader.Read (Read_SixteenByteFile_IsAcceptedAsAContainerWithoutPreviews)
#[test]
fn read_sixteen_byte_file_is_accepted_as_a_container_without_previews() {
    let file = cat!(bx("free", Vec::<u8>::new()), bx("free", Vec::<u8>::new()));
    assert_eq!(file.len(), 16);
    let info = read_info(&file);
    assert!(info.previews.is_empty());
    assert!(info.exif_blocks.is_empty());
}

// C#: Cr3ContainerReader.Read (Read_FifteenByteFile_IsRejected)
#[test]
fn read_fifteen_byte_file_is_rejected() {
    let err = try_read(&[0u8; 15]).expect_err("a 15-byte file is too short");
    assert!(is_invalid_data(&err), "expected InvalidData, got {err}");
}

// C#: Cr3ContainerReader.Read (Read_Cmt2Box_IsReportedAsAnExifIfdBlock)
#[test]
fn read_cmt2_box_is_reported_as_an_exif_ifd_block() {
    let cmt2_payload = TiffBytes::new(true, 20).header(8).ifd(8, 0, &[]).to_vec();
    let file = cat!(ftyp(), bx("moov", uuid_bx(&CANON_UUID, bx("CMT2", &cmt2_payload))));
    let info = read_info(&file);
    assert_eq!(info.exif_blocks.len(), 1);
    let block = info.exif_blocks[0];
    assert!(block.is_tiff_header);
    assert!(block.ifd_is_exif);
    assert_eq!(block.length, cmt2_payload.len() as i64);
}

// C#: Cr3ContainerReader.Read (Read_Cmt1OrientationEntryWithZeroCount_IsIgnored)
#[test]
fn read_cmt1_orientation_entry_with_zero_count_is_ignored() {
    let tiff = TiffBytes::new(true, 8 + 2 + 12 + 4)
        .header(8)
        .ifd(
            8,
            0,
            &[Entry {
                tag: 0x0112,
                typ: 3,
                count: 0,
                value: 6,
            }],
        )
        .to_vec();
    let file = cat!(ftyp(), bx("moov", uuid_bx(&CANON_UUID, bx("CMT1", tiff))));
    assert_eq!(read_info(&file).orientation, 1);
}

// C#: Cr3ContainerReader.Read (Read_Cmt1AtEndOfFileWithIfdPointerJustPastThePayload_DoesNotThrow)
#[test]
fn read_cmt1_at_end_of_file_with_ifd_pointer_just_past_the_payload_does_not_throw() {
    let tiff = TiffBytes::new(true, 16).header(17).to_vec();
    let file = cat!(ftyp(), bx("moov", uuid_bx(&CANON_UUID, bx("CMT1", tiff))));
    let info = read_info(&file);
    assert_eq!(info.orientation, 1);
    assert_eq!(info.exif_blocks.len(), 1);
}

// C#: Cr3ContainerReader.Read (Read_StszTooShortForItsHeader_YieldsNoPreviewEvenWhenFollowingBytesLookLikeOne)
#[test]
fn read_stsz_too_short_for_its_header_yields_no_preview_even_when_following_bytes_look_like_one() {
    let jpeg = minimal_jpeg(640, 480);
    let file = track_file(&jpeg, |off| {
        trak(&[
            bx("stsz", cat!(u32be(0), u32be(u32::try_from(jpeg.len()).expect("fits")))), // 8-byte payload: shorter than the 12-byte header
            stco(u32::try_from(off).expect("fits"), 1),
        ])
    });
    assert!(read_info(&file).previews.is_empty());
}

// C#: Cr3ContainerReader.Read (Read_StcoTooShort_FallsBackToCo64)
#[test]
fn read_stco_too_short_falls_back_to_co64() {
    let jpeg = minimal_jpeg(640, 480);
    let file = track_file(&jpeg, |off| {
        trak(&[
            stsz(u32::try_from(jpeg.len()).expect("fits"), 1),
            bx("stco", u32be(0)), // 4-byte payload: under the 8 bytes stco needs
            co64(off),
        ])
    });
    let info = read_info(&file);
    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.previews[0].width, 640);
}

// ------------------------------------------------------------------ Cr3AndRafReaderTests / Cr3AndRafCorpusTests (CR3 halves)

// C#: Cr3ContainerReader.Read (Cr3Reader_Synthetic_CanReadPreview; the registry `FindReader` half is W5-11's)
#[test]
fn cr3_reader_synthetic_can_read_preview() {
    let jpeg = minimal_jpeg(640, 480);
    let data = build_iso_bmff(&jpeg);
    assert!(Cr3ContainerReader.can_read(&data[..data.len().min(64)], ".cr3"));
    let info = read_info(&data);
    assert_eq!(info.format, RawFormat::Cr3);
    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.previews[0].length, jpeg.len() as i64);
}

// C#: Cr3ContainerReader.Read (AllCorpusCr3AndRafSamples_CanBeParsed, CR3 half only: every `*.cr3` of the corpus)
// The C# strict count of 5 covers CR3 and RAF together; this CR3-only twin requires the 2 CR3 samples in strict mode.
#[test]
fn corpus_all_cr3_samples_can_be_parsed() {
    let dir = corpus_dir();
    let files = corpus_cr3_files(&dir);
    let mut parsed = 0usize;
    for file in &files {
        let bytes = fs::read(file).unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        assert!(
            Cr3ContainerReader.can_read(&bytes[..bytes.len().min(64)], ".cr3"),
            "{}: can_read",
            file.display()
        );
        let info = read_info(&bytes);
        assert!(!info.previews.is_empty(), "{} had 0 previews", file.display());
        assert!(
            info.previews[0].length > 0,
            "{}: first preview is empty",
            file.display()
        );
        parsed += 1;
    }
    if corpus_strict() {
        assert!(parsed >= 2, "expected at least 2 CR3 corpus files, parsed {parsed}");
    }
}

/// The `*.cr3` files of the corpus directory. Missing or empty: skipped (`Ok(empty)`), or a failure in strict mode.
fn corpus_cr3_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let files: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("cr3")))
            .collect(),
        Err(e) => {
            assert!(
                !corpus_strict(),
                "RAW corpus missing in strict mode: {} ({e})",
                dir.display()
            );
            corpus_skip::skipped(&format!("RAW corpus not present at {}", dir.display()));
            return Vec::new();
        }
    };
    if files.is_empty() {
        assert!(!corpus_strict(), "no *.cr3 files in strict mode: {}", dir.display());
        corpus_skip::skipped(&format!("no *.cr3 files in {}", dir.display()));
    }
    files
}

// ------------------------------------------------------------------ fuzz (C# `Robustness/RawContainerFuzzTests`, `Robustness/BinaryReaderFuzzTests`, CR3 rows)

/// Mutants per fuzz test (C# `RawContainerFuzzTests.IterationsPerFormat`).
const FUZZ_ITERATIONS: usize = 10_000;
/// Warm-up iterations excluded from the p95 (C# `WarmUpIterations`).
const FUZZ_WARM_UP: usize = 500;
/// p95 parse-time budget (C# `P95Budget`): a hang guard, not a benchmark.
const FUZZ_P95_BUDGET: Duration = Duration::from_millis(2000);
/// Wall-clock bound for ONE mutated case (C# `BinaryFuzz.CaseBound`).
const CASE_BOUND: Duration = Duration::from_secs(10);

/// Any `InvalidData`, whatever its inner error (the documented clean failure of a hostile file).
fn is_invalid_any(err: &RawHeaderError) -> bool {
    matches!(err, RawHeaderError::InvalidData { .. })
}

/// C# `RawContainerFuzzTests.RunFuzzLoop` p95: the sorted timings' element at `ceil(count * 0.95) - 1`.
fn p95(timings: &mut [Duration]) -> Duration {
    timings.sort();
    let index = ((timings.len() as f64 * 0.95).ceil() as usize).max(1) - 1;
    timings[index]
}

// C#: Fuzz_Cr3ContainerReader_Robust (seed BuildIsoBmff() = ftyp + prvw(800x600), rng seed 107, 10 000 JpegBytes.Mutate mutants)
#[test]
fn fuzz_cr3_container_reader_robust() {
    let seed = build_iso_bmff(&minimal_jpeg(800, 600));
    let mut rng = XorShift64::new(107);
    let mut timings = Vec::with_capacity(FUZZ_ITERATIONS - FUZZ_WARM_UP);
    for i in 0..FUZZ_ITERATIONS {
        let mutated = mutate(&mut rng, &seed);
        let mut source = FuzzTrackingSource::new(&mutated);
        let started = Instant::now();
        if Cr3ContainerReader.can_read(&mutated[..mutated.len().min(64)], ".cr3") {
            match Cr3ContainerReader.read(&mut source, &CancelToken::none()) {
                Ok(info) => assert!(
                    (1..=8).contains(&info.orientation),
                    "mutant {i}: orientation {}",
                    info.orientation
                ),
                Err(e) if is_invalid_any(&e) => {}
                Err(e) => panic!("mutant {i}: the reader threw an unexpected error: {e}"),
            }
        }
        let elapsed = started.elapsed();
        if i >= FUZZ_WARM_UP {
            timings.push(elapsed);
        }
        assert!(
            fuzz_total(&source) <= MAX_HEADER_BYTES,
            "mutant {i}: exceeded MaxHeaderBytes ({} > {MAX_HEADER_BYTES})",
            fuzz_total(&source)
        );
    }
    let slowest_p95 = p95(&mut timings);
    assert!(
        slowest_p95 < FUZZ_P95_BUDGET,
        "p95 parse time was {slowest_p95:?} (budget {FUZZ_P95_BUDGET:?})"
    );
}

// C#: Fuzz_Cr3RealLayout_StrictSource_FailsOnlyCleanlyAndConsumersNeverThrow (reader half; the RawExif / PreviewSelector calls are W5-11's)
#[test]
fn fuzz_cr3_real_layout_strict_source_fails_only_cleanly_and_consumers_never_throw() {
    let seed = build_canon_cr3(None, None, None);
    let mut rng = XorShift64::new(109);
    let mut parsed = 0usize;
    for i in 0..FUZZ_ITERATIONS {
        let mutated = mutate(&mut rng, &seed);
        if !Cr3ContainerReader.can_read(&mutated[..mutated.len().min(64)], ".cr3") {
            continue;
        }
        let mut source = InMemoryRawHeaderSource::new(mutated.clone());
        let info = match Cr3ContainerReader.read(&mut source, &CancelToken::none()) {
            Ok(info) => info,
            Err(e) if is_invalid_any(&e) => continue, // a clean failure on a hostile file
            Err(e) => panic!("mutant {i}: only InvalidData may escape the reader: {e}"),
        };
        parsed += 1;
        let len = i64::try_from(mutated.len()).expect("fits i64");
        assert!(
            (1..=8).contains(&info.orientation),
            "mutant {i}: orientation {}",
            info.orientation
        );
        assert!(
            info.sensor_width >= 0 && info.sensor_height >= 0,
            "mutant {i}: sensor dimensions went negative"
        );
        for p in &info.previews {
            assert!(
                p.offset >= 0 && p.length > 0 && p.length <= len - p.offset,
                "mutant {i}: preview {}+{} escapes the {len}-byte file",
                p.offset,
                p.length
            );
            assert!(p.width >= 0 && p.height >= 0, "mutant {i}: negative preview size");
        }
    }
    assert!(
        parsed > FUZZ_ITERATIONS / 10,
        "only {parsed} mutated files parsed; the fuzz seed is not exercising the reader"
    );
}

/// One CR3 mutant of the systematic corpus: `Ok(true)` accepted, `Ok(false)` rejected cleanly, `Err` a defect.
fn fuzz_cr3_case(bytes: &[u8]) -> Result<bool, String> {
    if !Cr3ContainerReader.can_read(&bytes[..bytes.len().min(64)], ".cr3") {
        return Ok(false);
    }
    let mut source = InMemoryRawHeaderSource::new(bytes.to_vec());
    let info = match Cr3ContainerReader.read(&mut source, &CancelToken::none()) {
        Ok(info) => info,
        Err(e) if is_invalid_any(&e) => return Ok(false),
        Err(e) => return Err(format!("unexpected error: {e}")),
    };
    if !(1..=8).contains(&info.orientation) {
        return Err(format!("accepted with orientation {}", info.orientation));
    }
    if info.sensor_width < 0 || info.sensor_height < 0 {
        return Err("accepted with a negative sensor size".to_owned());
    }
    let len = i64::try_from(bytes.len()).expect("fits i64");
    for p in &info.previews {
        if !(p.offset >= 0 && p.length > 0 && p.length <= len - p.offset) {
            return Err(format!("preview {}+{} escapes the {len}-byte file", p.offset, p.length));
        }
    }
    Ok(true)
}

// C#: Fuzz_RawContainerReaders_FailOnlyCleanly (the CR3 row only: seed BuildCanonCr3(160x120, 32x24), rng seed 4401; the
// systematic `BinaryFuzz.Mutants` corpus, not JpegBytes.Mutate)
#[test]
fn fuzz_raw_container_readers_fail_only_cleanly_cr3_row() {
    let seed = build_canon_cr3(Some(minimal_jpeg(160, 120)), Some(minimal_jpeg(32, 24)), None);
    let cases = binary_fuzz_mutants(&seed, 4401, 1_500);
    let (mut accepted, mut total) = (0usize, 0usize);
    let mut defects = Vec::new();
    for case in &cases {
        total += 1;
        let started = Instant::now();
        let outcome = fuzz_cr3_case(&case.bytes);
        let elapsed = started.elapsed();
        assert!(
            elapsed <= CASE_BOUND,
            "one case ran for {elapsed:?} (per-case bound {CASE_BOUND:?}) -- stuck on [{}]",
            case.label
        );
        match outcome {
            Ok(true) => accepted += 1,
            Ok(false) => {}
            Err(defect) => {
                if defects.len() < 8 {
                    defects.push(format!("[{}] ({} bytes) {defect}", case.label, case.bytes.len()));
                }
            }
        }
    }
    assert!(
        defects.is_empty(),
        "CR3 container reader: {} non-documented failure(s) in {total} cases:\n{}",
        defects.len(),
        defects.join("\n")
    );
    assert!(
        accepted > 0,
        "no mutant was accepted -- the corpus no longer reaches the parser"
    );
    assert!(total > 500, "only {total} cases ran");
}

// ------------------------------------------------------------------ golden `raw-cr3-reader` (321 rows, `tests/golden/raw-cr3-reader.json`)

/// Checks one `read` row against the oracle: the container info (or the exception) and the header budget charged.
fn check_golden_read(name: &str, case: &Value, data: &[u8], cancel: bool) {
    let mut source = InMemoryRawHeaderSource::new(data.to_vec());
    let token = CancelToken::new();
    if cancel {
        token.cancel();
    }
    let result = Cr3ContainerReader.read(&mut source, &token);
    assert_eq!(
        source.total_bytes_read(),
        i64_of(case, "bytes_charged"),
        "{name}: bytes_charged"
    );
    let expected_ok = case["ok"].as_bool() == Some(true);
    match result {
        Ok(info) => {
            assert!(expected_ok, "{name}: expected an exception, got Ok");
            assert_eq!(format!("{:?}", info.format), str_of(case, "format"), "{name}: format");
            assert_eq!(
                i64::from(info.sensor_width),
                i64_of(case, "sensor_width"),
                "{name}: sensor_width"
            );
            assert_eq!(
                i64::from(info.sensor_height),
                i64_of(case, "sensor_height"),
                "{name}: sensor_height"
            );
            assert_eq!(
                i64::from(info.orientation),
                i64_of(case, "orientation"),
                "{name}: orientation"
            );
            let expected_previews = case["previews"].as_array().expect("golden previews");
            assert_eq!(info.previews.len(), expected_previews.len(), "{name}: preview count");
            for (p, g) in info.previews.iter().zip(expected_previews) {
                assert_eq!(i64::from(p.index), i64_of(g, "index"), "{name}: preview index");
                assert_eq!(p.offset, i64_of(g, "offset"), "{name}: preview offset");
                assert_eq!(p.length, i64_of(g, "length"), "{name}: preview length");
                assert_eq!(format!("{:?}", p.kind), str_of(g, "kind"), "{name}: preview kind");
                assert_eq!(i64::from(p.width), i64_of(g, "width"), "{name}: preview width");
                assert_eq!(i64::from(p.height), i64_of(g, "height"), "{name}: preview height");
                assert_eq!(
                    format!("{:?}", p.color_space),
                    str_of(g, "color_space"),
                    "{name}: colour space"
                );
                assert_eq!(
                    p.header_resolved,
                    bool_of(g, "header_resolved"),
                    "{name}: header_resolved"
                );
            }
            let expected_blocks = case["exif_blocks"].as_array().expect("golden exif_blocks");
            assert_eq!(
                info.exif_blocks.len(),
                expected_blocks.len(),
                "{name}: exif block count"
            );
            for (block, g) in info.exif_blocks.iter().zip(expected_blocks) {
                assert_eq!(block.offset, i64_of(g, "offset"), "{name}: exif offset");
                assert_eq!(block.length, i64_of(g, "length"), "{name}: exif length");
                assert_eq!(
                    block.is_tiff_header,
                    bool_of(g, "is_tiff_header"),
                    "{name}: is_tiff_header"
                );
                assert_eq!(block.ifd_is_exif, bool_of(g, "ifd_is_exif"), "{name}: ifd_is_exif");
            }
        }
        Err(err) => {
            assert!(!expected_ok, "{name}: unexpected error: {err}");
            let exception = match &err {
                RawHeaderError::InvalidData { .. } => "InvalidDataException",
                RawHeaderError::ArgumentOutOfRange { .. } => "ArgumentOutOfRangeException",
                RawHeaderError::Canceled => "OperationCanceledException",
            };
            assert_eq!(exception, str_of(case, "exception"), "{name}: exception type");
            let reader_message = match &err {
                RawHeaderError::InvalidData { message, .. } if message == "File too short for CR3 container." => {
                    Some(message.as_str())
                }
                _ => None,
            };
            assert_eq!(
                reader_message,
                case["reader_message"].as_str(),
                "{name}: reader_message"
            );
            assert_eq!(err.is_io_failure(), bool_of(case, "io_failure"), "{name}: io_failure");
        }
    }
}

// Golden `raw-cr3-reader`: every non-skipped row must agree (the builder bytes first, then can_read, then read).
#[test]
fn golden_raw_cr3_reader() {
    let doc = golden::load("raw-cr3-reader");
    assert_eq!(doc.generator, "oracle-cs raw-cr3-reader");
    let optional = optional_cases();
    let mut checked = 0usize;
    let mut skipped: Vec<String> = Vec::new();
    for case in &doc.cases {
        let name = str_of(case, "name");
        if case.get("skipped").and_then(Value::as_bool) == Some(true) {
            skipped.push(name.to_owned());
            continue;
        }
        let Some(data) = build_source(&case["source"], optional.contains(name)) else {
            skipped.push(name.to_owned());
            continue;
        };
        assert_eq!(
            i64_of(case, "file_length"),
            i64::try_from(data.len()).expect("fits"),
            "{name}: built file length drifted"
        );
        assert_eq!(
            str_of(case, "file_fnv1a64"),
            fnv1a64_hex(&data),
            "{name}: built bytes drifted"
        );
        let can_read = Cr3ContainerReader.can_read(&data[..data.len().min(64)], str_of(case, "ext"));
        assert_eq!(bool_of(case, "can_read"), can_read, "{name}: can_read");
        if str_of(case, "op") == "read" {
            check_golden_read(name, case, &data, bool_of(case, "cancel"));
        }
        checked += 1;
    }
    println!(
        "golden raw-cr3-reader: {checked} rows checked, {} skipped {skipped:?}",
        skipped.len()
    );
    assert!(checked > 0, "no golden row was checked");
}

// ------------------------------------------------------------------ Rust-only tests (no C# twin; each pins a C# fact named in its comment)

// Rust-only (`RawContainerReader: Send + Sync`, the registry shares one reader across threads).
#[test]
fn cr3_reader_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Cr3ContainerReader>();
}

// Rust-only (C# `ReadChildBoxes`/`TryReadBox` bounds checks, end to end): every truncation of the real layout is `Ok` or `InvalidData`.
#[test]
fn read_never_panics_on_any_truncation_of_the_real_layout() {
    let file = build_canon_cr3(None, None, None);
    for len in 0..=file.len() {
        match try_read(&file[..len]) {
            Ok(_) => {}
            Err(e) if is_invalid_any(&e) => {}
            Err(e) => panic!("truncation to {len} bytes escaped as {e}"),
        }
    }
}

// Rust-only: extreme 32-bit and 16-bit values written over the headers of the real layout never panic (`Ok` or `InvalidData`).
#[test]
fn read_never_panics_on_extreme_u32_overwrites_in_the_headers() {
    let file = build_canon_cr3(None, None, None);
    let window = 1200usize.min(file.len());
    let check = |mutant: &[u8], what: String| match try_read(mutant) {
        Ok(_) => {}
        Err(e) if is_invalid_any(&e) => {}
        Err(e) => panic!("{what} escaped as {e}"),
    };
    for pos in 0..window.saturating_sub(3) {
        for value in [0u32, 1, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF_FFFF] {
            let mut mutant = file.clone();
            mutant[pos..pos + 4].copy_from_slice(&value.to_be_bytes());
            check(&mutant, format!("u32 {value:08X} at {pos}"));
        }
    }
    for pos in 0..window.saturating_sub(1) {
        for value in [0x0000u16, 0x7FFF, 0xFFFF] {
            let mut mutant = file.clone();
            mutant[pos..pos + 2].copy_from_slice(&value.to_be_bytes());
            check(&mutant, format!("u16 {value:04X} at {pos}"));
        }
    }
}

// Rust-only: a source that fails the k-th read (for every k of a clean run) gives `Err`, never a panic. An injected I/O failure is always an
// I/O failure; a plain `InvalidData` is `Err` too, except on the reads of the track JPEG probe, which swallow it (C# `TryProbeTrackJpeg`).
#[test]
fn failing_source_at_every_read_index_returns_err_not_panic() {
    let file = build_canon_cr3(None, None, None);
    let track_len = minimal_jpeg(6000, 4000).len();
    let track_offset = track_offset_of_canon(&file, track_len);
    let mut clean = FaultSource::new(&file, None);
    Cr3ContainerReader
        .read(&mut clean, &CancelToken::none())
        .unwrap_or_else(|e| panic!("the clean run must succeed: {e}"));
    let reads = clean.reads.clone();
    assert!(!reads.is_empty());
    for (k, &(offset, _)) in reads.iter().enumerate() {
        let mut io = FaultSource::new(&file, Some((Trigger::Nth(k), Fault::Io)));
        match Cr3ContainerReader.read(&mut io, &CancelToken::none()) {
            Err(e) => assert!(
                e.is_io_failure(),
                "read {k}: an injected I/O failure must stay an I/O failure: {e}"
            ),
            Ok(_) => panic!("read {k}: an injected I/O failure must not be swallowed"),
        }
        // The canon layout ends with the bare track JPEG, so the top-level scan reads a box header at `track_offset` as the LAST read of a
        // clean run (a hostile-looking size, `None`): that read is not the probe's and propagates.
        let in_probe = k + 1 != reads.len()
            && offset >= track_offset
            && offset < track_offset + i64::try_from(track_len).expect("fits");
        let mut plain = FaultSource::new(&file, Some((Trigger::Nth(k), Fault::Invalid)));
        match Cr3ContainerReader.read(&mut plain, &CancelToken::none()) {
            Ok(info) => {
                assert!(
                    in_probe,
                    "read {k} at offset {offset}: a plain InvalidData was swallowed outside the track probe"
                );
                assert!(
                    info.previews.iter().all(|p| p.offset != track_offset),
                    "read {k}: a swallowed probe must not leave a track preview"
                );
            }
            Err(e) => {
                assert!(
                    !in_probe,
                    "read {k} at offset {offset} is a track probe read and must be swallowed"
                );
                assert!(
                    is_invalid_any(&e) && !e.is_io_failure(),
                    "read {k}: expected a plain InvalidData, got {e}"
                );
            }
        }
    }
}

// Rust-only (C# `TryProbeTrackJpeg`'s `catch ... when (!IsIoFailure)`): fail only the read at the track JPEG offset.
#[test]
fn track_probe_io_failure_propagates_but_budget_failure_is_swallowed() {
    let file = build_canon_cr3(None, None, None);
    let track_offset = track_offset_of_canon(&file, minimal_jpeg(6000, 4000).len());
    let mut io = FaultSource::new(&file, Some((Trigger::Offset(track_offset), Fault::Io)));
    match Cr3ContainerReader.read(&mut io, &CancelToken::none()) {
        Err(e) => assert!(e.is_io_failure(), "the probe's I/O failure must propagate: {e}"),
        Ok(_) => panic!("the probe's I/O failure was swallowed"),
    }
    let mut plain = FaultSource::new(&file, Some((Trigger::Offset(track_offset), Fault::Invalid)));
    let info = Cr3ContainerReader
        .read(&mut plain, &CancelToken::none())
        .unwrap_or_else(|e| panic!("a plain InvalidData in the probe must be swallowed: {e}"));
    assert!(
        info.previews.iter().all(|p| p.offset != track_offset),
        "no track preview after a swallowed probe"
    );
    assert_eq!(info.previews.len(), 2, "PRVW and THMB remain");
}

// Rust-only (C# `ParseMoovBox` + `PreviewCollector.ToOrderedList`): `index == position` in the final list of PRVW, two tracks and two THMB.
#[test]
fn previews_are_reindexed_by_final_position() {
    let prvw = preview_uuid_box(&minimal_jpeg(1620, 1080), 1620, 1080, None);
    let track_a = minimal_jpeg(640, 480);
    let track_b = minimal_jpeg(320, 240);
    let thumb_1 = minimal_jpeg(160, 120);
    let thumb_2 = minimal_jpeg(32, 24);
    let len_a = u32::try_from(track_a.len()).expect("fits");
    let len_b = u32::try_from(track_b.len()).expect("fits");
    let moov = |chunk_a: u64, chunk_b: u64| {
        bx(
            "moov",
            cat!(
                uuid_bx(
                    &CANON_UUID,
                    cat!(thmb_box(&thumb_1, 160, 120), thmb_box(&thumb_2, 32, 24))
                ),
                trak_with_co64(len_a, chunk_a),
                trak_with_co64(len_b, chunk_b)
            ),
        )
    };
    let base = (ftyp().len() + prvw.len() + moov(0, 0).len()) as u64;
    let file = cat!(
        ftyp(),
        &prvw,
        moov(base, base + track_a.len() as u64),
        &track_a,
        &track_b
    );
    let info = read_info(&file);
    let dims: Vec<(i32, i32)> = info.previews.iter().map(|p| (p.width, p.height)).collect();
    assert_eq!(dims, vec![(1620, 1080), (640, 480), (320, 240), (160, 120), (32, 24)]);
    for (position, preview) in info.previews.iter().enumerate() {
        assert_eq!(
            preview.index,
            i32::try_from(position).expect("fits"),
            "preview at position {position}"
        );
    }
}

// Rust-only (C# `ParseCanonMoovUuid` and the top-level cancellation check): the token is read once per top-level iteration.
#[test]
fn cancel_is_checked_once_per_top_level_iteration() {
    let moov = bx(
        "moov",
        uuid_bx(&CANON_UUID, bx("CMT1", cmt1_tiff(8, &[short(0x0112, 6)]))),
    );
    // The hook cancels on the first read at or past offset 24 (ftyp 16 bytes + the moov header): that read is inside the moov.
    let token = CancelToken::new();
    let mut source = CancelOnRead {
        inner: InMemoryRawHeaderSource::new(cat!(ftyp(), &moov)),
        token: token.clone(),
        from: 24,
        fired: false,
    };
    let info = Cr3ContainerReader
        .read(&mut source, &token)
        .unwrap_or_else(|e| panic!("the moov is the last box, so no check follows it: {e}"));
    assert_eq!(info.orientation, 6);
    // The same file plus a trailing free box: the next top-level iteration checks the token and stops.
    let token = CancelToken::new();
    let mut source = CancelOnRead {
        inner: InMemoryRawHeaderSource::new(cat!(ftyp(), &moov, bx("free", Vec::<u8>::new()))),
        token: token.clone(),
        from: 24,
        fired: false,
    };
    let result = Cr3ContainerReader.read(&mut source, &token);
    assert!(
        matches!(result, Err(RawHeaderError::Canceled)),
        "the next top-level iteration must observe the cancellation"
    );
}

// Rust-only (C# `BmffBoxNavigator.TryReadBox` skip cost): a skipped box costs its header only, not its payload.
#[test]
fn read_does_not_touch_the_payload_of_skipped_boxes() {
    let file = cat!(
        ftyp(),
        bx("free", vec![0u8; 300_000]),
        bx(
            "moov",
            uuid_bx(&CANON_UUID, bx("CMT1", cmt1_tiff(8, &[short(0x0112, 6)])))
        )
    );
    let mut source = InMemoryRawHeaderSource::new(file.clone());
    let info = Cr3ContainerReader
        .read(&mut source, &CancelToken::none())
        .unwrap_or_else(|e| panic!("valid file rejected: {e}"));
    assert_eq!(info.orientation, 6);
    assert!(
        source.total_bytes_read() < i64::try_from(file.len()).expect("fits") / 2,
        "charged {} bytes of a {}-byte file",
        source.total_bytes_read(),
        file.len()
    );
}
