//! Red tests for W4-03b `pr_imaging::exif_parser` (port of `PhotoReview.Imaging/Metadata/ExifParser.cs`, C# baseline d92d6aae).
//!
//! Every ported test's first comment line is `// C#: <Class>.<Method>`. Theory rows become separate `#[test]` functions.
//! The golden test replays `tests/golden/exif-parser.json` (146 cases, oracle `exif-parser`).
//!
//! Documented skips (not portable to this unit, see contracts/W4-03b.md):
//! - `Bounded(...)` 10 s hang guards of `ExifParserTests` and `ExifParserHostilePointerTests`: the one-level rule makes a
//!   loop impossible to write, so there is no twin; plain asserts only (AGENTS.md: no sleep polling).
//! - `ExifTestData.EncodeJpegWithExif` / `EncodeJpegWithPartialExif` (WPF `JpegBitmapEncoder`): replaced by `jpeg_with_app1`.
//! - `OrfExifMagicTests.TryReadExif_*`, `CorpusOrfFiles_ExposeExif` and `Cr3ExifMergeTests` other than the `IfdIsExif` test
//!   (they go through `RawExif`, W5-RAW). The `UnknownMagic` rows are covered by the goldens `tiff-magic-*`.
//! - `ExifMetadataMutationGapTests` TiffStructure sections (W4-03a) and ExifQueryInterpreter sections (W4-04b).
//! - `CombinedHeaderScanTests.ScanHeader_*`, `OrientationParityTests`, `DecoderExifTests` (TurboJpeg, W5).
//! - `ExifParser_MakeStoredAsOutOfLineUndefined_IsRead`: covered by golden `jpeg-make-undefined-type-7`.

use pr_imaging::exif_parser as ep;
use pr_imaging::exif_summary::{ExifRational, ExifSummary, RawExifTags, MAX_TEXT_LENGTH};
use pr_test_support::golden;
use serde_json::{json, Value};

use builder::{add_segment, ascii, full_camera, jpeg_with_app1, rational, short, tiff, u16_bytes, u32_bytes};
use tiff_bytes::{at, fake_entry, long, t_short, TiffBytes};

/// C# `ExifTestData`: hand-built EXIF/TIFF blocks in either byte order, for the JPEG-level tests.
mod builder {
    use pr_imaging::exif_parser as ep;
    use pr_imaging::exif_summary::{ExifRational, ExifSummary};

    /// C# `ExifTestData.Entry`: one IFD entry with its value bytes (already in the file's byte order).
    #[derive(Clone, Debug)]
    pub struct Entry {
        pub tag: u16,
        pub typ: u16,
        pub count: u32,
        pub value: Vec<u8>,
    }

    pub fn ascii(tag: u16, text: &str) -> Entry {
        let mut value = text.as_bytes().to_vec();
        value.push(0);
        Entry {
            tag,
            typ: 2,
            count: value.len() as u32,
            value,
        }
    }

    pub fn short(tag: u16, value: u16, little: bool) -> Entry {
        Entry {
            tag,
            typ: 3,
            count: 1,
            value: u16_bytes(value, little),
        }
    }

    pub fn rational(tag: u16, numerator: u32, denominator: u32, little: bool) -> Entry {
        let mut value = u32_bytes(numerator, little);
        value.extend(u32_bytes(denominator, little));
        Entry {
            tag,
            typ: 5,
            count: 1,
            value,
        }
    }

    pub fn u16_bytes(value: u16, little: bool) -> Vec<u8> {
        if little {
            value.to_le_bytes().to_vec()
        } else {
            value.to_be_bytes().to_vec()
        }
    }

    pub fn u32_bytes(value: u32, little: bool) -> Vec<u8> {
        if little {
            value.to_le_bytes().to_vec()
        } else {
            value.to_be_bytes().to_vec()
        }
    }

    /// TIFF block: header, IFD0 (plus an Exif-IFD pointer when `exif` is non-empty), the Exif IFD, then the out-of-line
    /// values. Offsets are relative to the block start, as in a real APP1 segment.
    pub fn tiff(little: bool, ifd0: &[Entry], exif: &[Entry]) -> Vec<u8> {
        let mut ifd0_entries: Vec<Entry> = ifd0.to_vec();
        let has_exif = !exif.is_empty();
        if has_exif {
            ifd0_entries.push(Entry {
                tag: ep::TAG_EXIF_IFD,
                typ: 4,
                count: 1,
                value: vec![0; 4],
            }); // patched in write_ifd
        }

        let ifd0_offset = 8usize;
        let ifd0_size = 2 + ifd0_entries.len() * 12 + 4;
        let exif_offset = ifd0_offset + ifd0_size;
        let exif_size = if has_exif { 2 + exif.len() * 12 + 4 } else { 0 };
        let data_offset = exif_offset + exif_size;

        let mut output: Vec<u8> = Vec::new();
        let mut data: Vec<u8> = Vec::new();
        output.extend_from_slice(if little { b"II" } else { b"MM" });
        output.extend(u16_bytes(42, little));
        output.extend(u32_bytes(ifd0_offset as u32, little));

        write_ifd(&mut output, &mut data, &ifd0_entries, little, exif_offset, data_offset);
        if has_exif {
            write_ifd(&mut output, &mut data, exif, little, exif_offset, data_offset);
        }
        output.extend_from_slice(&data);
        output
    }

    fn write_ifd(
        output: &mut Vec<u8>,
        data: &mut Vec<u8>,
        entries: &[Entry],
        little: bool,
        exif_offset: usize,
        data_offset: usize,
    ) {
        output.extend(u16_bytes(entries.len() as u16, little));
        for entry in entries {
            let value = if entry.tag == ep::TAG_EXIF_IFD {
                u32_bytes(exif_offset as u32, little)
            } else {
                entry.value.clone()
            };
            output.extend(u16_bytes(entry.tag, little));
            output.extend(u16_bytes(entry.typ, little));
            output.extend(u32_bytes(entry.count, little));
            if value.len() <= 4 {
                output.extend_from_slice(&value);
                output.resize(output.len() + 4 - value.len(), 0);
            } else {
                output.extend(u32_bytes((data_offset + data.len()) as u32, little));
                data.extend_from_slice(&value);
                if data.len() % 2 == 1 {
                    data.push(0);
                }
            }
        }
        output.extend(u32_bytes(0, little)); // no next IFD
    }

    /// C# `ExifTestData.AddSegment`: marker, big-endian length (payload + 2), payload.
    pub fn add_segment(output: &mut Vec<u8>, marker: u8, payload: &[u8]) {
        output.push(0xFF);
        output.push(marker);
        output.extend(u16_bytes((payload.len() + 2) as u16, false));
        output.extend_from_slice(payload);
    }

    /// C# `ExifTestData.JpegWithApp1`: SOI, optional APP0, APP1 "Exif" with `tiff`, SOS, EOI.
    pub fn jpeg_with_app1(tiff: &[u8], app0: Option<&[u8]>) -> Vec<u8> {
        let mut output = vec![0xFF, 0xD8];
        if let Some(app0) = app0 {
            add_segment(&mut output, 0xE0, app0);
        }
        let mut payload = b"Exif\0\0".to_vec();
        payload.extend_from_slice(tiff);
        add_segment(&mut output, 0xE1, &payload);
        output.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]);
        output
    }

    /// C# `ExifTestData.FullCamera`: the camera the "full" fixtures describe.
    pub fn full_camera(little: bool) -> (Vec<Entry>, Vec<Entry>) {
        (
            vec![
                ascii(ep::TAG_MAKE, "Canon"),
                ascii(ep::TAG_MODEL, "Canon EOS R5"),
                ascii(ep::TAG_DATE_TIME, "2024:06:30 09:00:00"),
            ],
            vec![
                rational(ep::TAG_EXPOSURE_TIME, 1, 250, little),
                rational(ep::TAG_F_NUMBER, 28, 10, little),
                short(ep::TAG_ISO, 400, little),
                ascii(ep::TAG_DATE_TIME_ORIGINAL, "2024:05:01 14:03:22"),
                rational(ep::TAG_FOCAL_LENGTH, 50, 1, little),
                ascii(ep::TAG_LENS_MODEL, "RF24-70mm F2.8 L IS USM"),
            ],
        )
    }

    /// `date_taken` as the `yyyy-MM-ddTHH:mm:ss` text the oracle prints (the `Z` of `to_json` removed).
    pub fn date_text(summary: &ExifSummary) -> Option<String> {
        summary
            .date_taken
            .map(|date| date.to_json().trim_end_matches('Z').to_owned())
    }

    /// C# `ExifTestData.AssertFullCamera`.
    pub fn assert_full_camera(summary: Option<ExifSummary>) {
        let summary = summary.expect("the full camera EXIF must parse");
        assert_eq!(summary.camera_make.as_deref(), Some("Canon"));
        assert_eq!(summary.camera_model.as_deref(), Some("Canon EOS R5"));
        // DateTimeOriginal wins over DateTime
        assert_eq!(date_text(&summary).as_deref(), Some("2024-05-01T14:03:22"));
        assert_eq!(summary.lens_model.as_deref(), Some("RF24-70mm F2.8 L IS USM"));
        assert_eq!(summary.iso, Some(400));
        assert_eq!(summary.focal_length, Some(ExifRational::new(50, 1)));
        assert_eq!(summary.f_number, Some(ExifRational::new(28, 10)));
        assert_eq!(summary.exposure_time, Some(ExifRational::new(1, 250)));
    }
}

/// C# `Raw/Tiff/TiffBytes.cs`: lays out a TIFF-family block byte by byte at explicit offsets, so hostile structures
/// (huge counts, dangling pointers) can be written exactly. Only the members the EXIF tests use are ported.
mod tiff_bytes {
    /// C# `TiffBytes.Entry`: a SHORT with count 1 is written as u16 at the value field, everything else as u32.
    #[derive(Clone, Copy, Debug)]
    pub struct Entry {
        pub tag: u16,
        pub typ: u16,
        pub count: u32,
        pub value: u32,
    }

    pub fn t_short(tag: u16, value: u32) -> Entry {
        Entry {
            tag,
            typ: 3,
            count: 1,
            value,
        }
    }

    pub fn long(tag: u16, value: u32) -> Entry {
        Entry {
            tag,
            typ: 4,
            count: 1,
            value,
        }
    }

    /// Out-of-line entry (array, rational, undefined blob) whose data lives at `offset`.
    pub fn at(tag: u16, typ: u16, count: u32, offset: u32) -> Entry {
        Entry {
            tag,
            typ,
            count,
            value: offset,
        }
    }

    /// C# `TiffBytes.FakeEntry` style: a raw 12-byte little-endian entry for hand-placed bytes.
    pub fn fake_entry(tag: u16, typ: u16, count: u32, value: u32) -> [u8; 12] {
        let mut bytes = [0u8; 12];
        bytes[0..2].copy_from_slice(&tag.to_le_bytes());
        bytes[2..4].copy_from_slice(&typ.to_le_bytes());
        bytes[4..8].copy_from_slice(&count.to_le_bytes());
        bytes[8..12].copy_from_slice(&value.to_le_bytes());
        bytes
    }

    pub struct TiffBytes {
        little: bool,
        buffer: Vec<u8>,
    }

    impl TiffBytes {
        pub fn new(little: bool, size: usize) -> Self {
            Self {
                little,
                buffer: vec![0; size],
            }
        }

        /// `II*\0` or `MM\0*` followed by the IFD0 offset.
        pub fn header(self, ifd0: u32) -> Self {
            let magic: &[u8] = if self.little { b"II*\0" } else { b"MM\0*" };
            self.header_with(ifd0, magic)
        }

        /// Header with an explicit signature (ORF `IIRO`, `MMOR`, ...), then the IFD0 offset.
        pub fn header_with(mut self, ifd0: u32, signature: &[u8]) -> Self {
            self.buffer[..signature.len()].copy_from_slice(signature);
            self.u32(4, ifd0)
        }

        pub fn put(mut self, position: usize, bytes: &[u8]) -> Self {
            self.buffer[position..position + bytes.len()].copy_from_slice(bytes);
            self
        }

        pub fn u16(self, position: usize, value: u16) -> Self {
            let bytes = if self.little {
                value.to_le_bytes()
            } else {
                value.to_be_bytes()
            };
            self.put(position, &bytes)
        }

        pub fn u32(self, position: usize, value: u32) -> Self {
            let bytes = if self.little {
                value.to_le_bytes()
            } else {
                value.to_be_bytes()
            };
            self.put(position, &bytes)
        }

        /// An IFD of `entries` at `position`, followed by the next-IFD pointer.
        pub fn ifd(self, position: usize, next_ifd: u32, entries: &[Entry]) -> Self {
            let mut this = self.u16(position, entries.len() as u16);
            for (i, e) in entries.iter().enumerate() {
                let at = position + 2 + i * 12;
                this = this.u16(at, e.tag).u16(at + 2, e.typ).u32(at + 4, e.count);
                this = if e.typ == 3 && e.count == 1 {
                    this.u16(at + 8, e.value as u16)
                } else {
                    this.u32(at + 8, e.value)
                };
            }
            this.u32(position + 2 + entries.len() * 12, next_ifd)
        }

        pub fn to_vec(&self) -> Vec<u8> {
            self.buffer.clone()
        }
    }
}

/// Deterministic xorshift64 (no `rand` crate; the seeds differ from .NET `Random`, which is fine for no-panic and
/// round-trip properties).
struct XorShift(u64);

impl XorShift {
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
        (self.next_u64() % n as u64) as usize
    }
}

fn utf16_len(text: Option<&str>) -> usize {
    text.map_or(0, |text| text.encode_utf16().count())
}

// ---------------------------------------------------------------- ExifParserTests

#[test]
fn reads_all_fields_little_endian() {
    // C#: ExifParserTests.ReadsAllFields (little = true; the JPEG carries an APP0 JFIF segment)
    let (ifd0, exif) = full_camera(true);
    let jpeg = jpeg_with_app1(&tiff(true, &ifd0, &exif), Some(b"JFIF\0\x01\x01"));

    builder::assert_full_camera(ep::try_parse_jpeg(&jpeg));
}

#[test]
fn reads_all_fields_big_endian() {
    // C#: ExifParserTests.ReadsAllFields (little = false; the JPEG carries an APP0 JFIF segment)
    let (ifd0, exif) = full_camera(false);
    let jpeg = jpeg_with_app1(&tiff(false, &ifd0, &exif), Some(b"JFIF\0\x01\x01"));

    builder::assert_full_camera(ep::try_parse_jpeg(&jpeg));
}

#[test]
fn missing_tags_are_skipped() {
    // C#: ExifParserTests.MissingTagsAreSkipped
    let jpeg = jpeg_with_app1(
        &tiff(
            true,
            &[ascii(ep::TAG_MODEL, "ILCE-7M3")],
            &[short(ep::TAG_ISO, 100, true)],
        ),
        None,
    );

    let summary = ep::try_parse_jpeg(&jpeg).expect("model and ISO are present");

    assert_eq!(summary.camera_model.as_deref(), Some("ILCE-7M3"));
    assert_eq!(summary.iso, Some(100));
    assert_eq!(summary.camera_make, None);
    assert_eq!(summary.lens_model, None);
    assert_eq!(summary.date_taken, None);
    assert_eq!(summary.f_number, None);
    assert_eq!(summary.exposure_time, None);
    assert_eq!(summary.focal_length, None);
}

#[test]
fn date_falls_back_to_ifd0_date_time() {
    // C#: ExifParserTests.DateFallsBackToIfd0DateTime
    let jpeg = jpeg_with_app1(
        &tiff(true, &[ascii(ep::TAG_DATE_TIME, "2023:12:24 18:30:00")], &[]),
        None,
    );

    let summary = ep::try_parse_jpeg(&jpeg).expect("DateTime is present");

    assert_eq!(builder::date_text(&summary).as_deref(), Some("2023-12-24T18:30:00"));
}

#[test]
fn nothing_usable_gives_null() {
    // C#: ExifParserTests.NothingUsableGivesNull
    assert_eq!(ep::try_parse_jpeg(&[]), None);
    assert_eq!(
        ep::try_parse_jpeg(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
        None
    );
    assert_eq!(
        ep::try_parse_jpeg(&[0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]),
        None
    );
    assert_eq!(ep::try_parse_jpeg(&jpeg_with_app1(&tiff(true, &[], &[]), None)), None);
    // Placeholder values cameras write when unset: empty strings, zero rationals, "0000:00:00" dates.
    let placeholders = tiff(
        true,
        &[
            ascii(ep::TAG_MAKE, "   "),
            ascii(ep::TAG_DATE_TIME, "0000:00:00 00:00:00"),
        ],
        &[
            rational(ep::TAG_F_NUMBER, 0, 0, true),
            rational(ep::TAG_EXPOSURE_TIME, 1, 0, true),
        ],
    );
    assert_eq!(ep::try_parse_jpeg(&jpeg_with_app1(&placeholders, None)), None);
}

#[test]
fn stops_at_start_of_scan() {
    // C#: ExifParserTests.StopsAtStartOfScan
    let (ifd0, exif) = full_camera(true);
    let mut output = vec![0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02];
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend(tiff(true, &ifd0, &exif));
    add_segment(&mut output, 0xE1, &payload);

    assert_eq!(ep::try_parse_jpeg(&output), None);
}

#[test]
fn huge_entry_count_is_bounded() {
    // C#: ExifParserTests.HugeEntryCountIsBounded
    let mut block = tiff(true, &[ascii(ep::TAG_MAKE, "Nikon")], &[]);
    block[8] = 0xFF;
    block[9] = 0xFF; // IFD0 count = 65535, the block holds one entry

    let summary = ep::try_parse_jpeg(&jpeg_with_app1(&block, None)).expect("the one real entry is read");

    assert_eq!(summary.camera_make.as_deref(), Some("Nikon"));
}

#[test]
fn entry_cut_by_end_of_block_is_not_read() {
    // C#: ExifParserTests.EntryCutByEndOfBlockIsNotRead
    let block = tiff(true, &[ascii(ep::TAG_MODEL, "GR")], &[]); // inline value, no data area
                                                                // Declare a 2nd entry: Make, ASCII, count 2 -- but the block ends 8 bytes into it (its inline value is missing).
    let mut cut = block[..8 + 2 + 12].to_vec();
    cut[8] = 2;
    cut.extend(u16_bytes(ep::TAG_MAKE, true));
    cut.extend(u16_bytes(2, true));
    cut.extend(u32_bytes(2, true));

    let summary = ep::try_parse_jpeg(&jpeg_with_app1(&cut, None));

    assert_eq!(summary.as_ref().and_then(|s| s.camera_model.as_deref()), Some("GR"));
    assert_eq!(summary.as_ref().and_then(|s| s.camera_make.as_deref()), None);
}

#[test]
fn value_running_past_end_is_skipped() {
    // C#: ExifParserTests.ValueRunningPastEndIsSkipped
    let mut block = tiff(
        true,
        &[ascii(ep::TAG_MODEL, "EOS R6 Mark II"), ascii(ep::TAG_MAKE, "Canon")],
        &[],
    );
    // Model (entry #1) now points 3 bytes before the end of the block, with its 15-byte count.
    let len = block.len();
    block[8 + 2 + 8..8 + 2 + 8 + 4].copy_from_slice(&u32_bytes((len - 3) as u32, true));

    let summary = ep::try_parse_jpeg(&jpeg_with_app1(&block, None));

    assert_eq!(summary.as_ref().and_then(|s| s.camera_model.as_deref()), None);
    assert_eq!(summary.as_ref().and_then(|s| s.camera_make.as_deref()), Some("Canon"));
}

#[test]
fn out_of_range_value_is_skipped() {
    // C#: ExifParserTests.OutOfRangeValueIsSkipped
    let mut block = tiff(
        true,
        &[ascii(ep::TAG_MODEL, "EOS R6 Mark II"), ascii(ep::TAG_MAKE, "Canon")],
        &[],
    );
    // First entry (Model): count = 0xFFFFFFFF, offset = 0x7FFFFFF0.
    block[8 + 2 + 4..8 + 2 + 8].copy_from_slice(&u32_bytes(0xFFFF_FFFF, true));
    block[8 + 2 + 8..8 + 2 + 12].copy_from_slice(&u32_bytes(0x7FFF_FFF0, true));

    let summary = ep::try_parse_jpeg(&jpeg_with_app1(&block, None)).expect("Make is still read");

    assert_eq!(summary.camera_model, None);
    assert_eq!(summary.camera_make.as_deref(), Some("Canon"));
}

/// C# `ExifParserTests.BadExifPointerIsIgnored` (theory rows 8, 0, 0xFFFFFFF0; one test per row below).
fn bad_exif_pointer_is_ignored(exif_ifd_offset: u32) {
    let mut block = tiff(true, &[ascii(ep::TAG_MAKE, "Canon")], &[short(ep::TAG_ISO, 800, true)]);
    // IFD0 entry #2 is the Exif pointer (LONG, inline value at +8).
    block[8 + 2 + 12 + 8..8 + 2 + 12 + 12].copy_from_slice(&u32_bytes(exif_ifd_offset, true));

    let summary = ep::try_parse_jpeg(&jpeg_with_app1(&block, None));

    assert_eq!(summary.as_ref().and_then(|s| s.camera_make.as_deref()), Some("Canon"));
    assert_eq!(summary.as_ref().and_then(|s| s.iso), None);
}

#[test]
fn bad_exif_pointer_is_ignored_for_eight() {
    // C#: ExifParserTests.BadExifPointerIsIgnored (row 8u: points at IFD0 itself)
    bad_exif_pointer_is_ignored(8);
}

#[test]
fn bad_exif_pointer_is_ignored_for_zero() {
    // C#: ExifParserTests.BadExifPointerIsIgnored (row 0u: inside the header)
    bad_exif_pointer_is_ignored(0);
}

#[test]
fn bad_exif_pointer_is_ignored_for_0xfffffff0() {
    // C#: ExifParserTests.BadExifPointerIsIgnored (row 0xFFFFFFF0u: past the end)
    bad_exif_pointer_is_ignored(0xFFFF_FFF0);
}

#[test]
fn truncated_input_never_panics() {
    // C#: ExifParserTests.TruncatedInputNeverThrows (Rust: never panics)
    let (ifd0, exif) = full_camera(false);
    let jpeg = jpeg_with_app1(&tiff(false, &ifd0, &exif), None);

    for length in 0..jpeg.len() {
        let _ = ep::try_parse_jpeg(&jpeg[..length]);
    }
    builder::assert_full_camera(ep::try_parse_jpeg(&jpeg));
}

#[test]
fn corrupted_input_never_panics() {
    // C#: ExifParserTests.CorruptedInputNeverThrows (3000 rounds; the mutations differ from .NET Random: no-panic property)
    let (ifd0, exif) = full_camera(true);
    let original = jpeg_with_app1(&tiff(true, &ifd0, &exif), None);
    let mut rng = XorShift(20240501);

    for _round in 0..3_000 {
        let mut mutated = original.clone();
        let flips = 1 + rng.below(8);
        for _ in 0..flips {
            let index = 4 + rng.below(mutated.len() - 4);
            mutated[index] = rng.next_u64() as u8;
        }
        if let Some(summary) = ep::try_parse_jpeg(&mutated) {
            assert!(!summary.is_empty(), "a returned summary must hold a field");
            assert!(utf16_len(summary.camera_model.as_deref()) <= MAX_TEXT_LENGTH);
        }
    }
}

#[test]
fn text_is_sanitised() {
    // C#: ExifParserTests.TextIsSanitised
    let jpeg = jpeg_with_app1(
        &tiff(
            true,
            &[
                ascii(ep::TAG_MAKE, "  Canon\t\u{1}  "),
                ascii(ep::TAG_MODEL, &"X".repeat(300)),
            ],
            &[],
        ),
        None,
    );

    let summary = ep::try_parse_jpeg(&jpeg).expect("text fields are present");

    assert_eq!(summary.camera_make.as_deref(), Some("Canon"));
    assert_eq!(summary.camera_model, Some("X".repeat(MAX_TEXT_LENGTH)));
}

#[test]
fn signed_rational_is_accepted() {
    // C#: ExifParserTests.SignedRationalIsAccepted
    let mut entry = rational(ep::TAG_FOCAL_LENGTH, 35, 1, true);
    entry.typ = 10; // SRATIONAL

    let summary = ep::try_parse_jpeg(&jpeg_with_app1(&tiff(true, &[], &[entry]), None));

    assert_eq!(summary.and_then(|s| s.focal_length), Some(ExifRational::new(35, 1)));
}

#[test]
fn try_read_orientation_from_jpeg_reads_ifd0_orientation_little_endian() {
    // C#: ExifParserTests.TryReadOrientationFromJpeg_ReadsIfd0Orientation (little = true, orientation 6)
    let jpeg = jpeg_with_app1(&tiff(true, &[short(ep::TAG_ORIENTATION, 6, true)], &[]), None);

    assert_eq!(ep::try_read_orientation_from_jpeg(&jpeg), Some(6));
}

#[test]
fn try_read_orientation_from_jpeg_reads_ifd0_orientation_big_endian() {
    // C#: ExifParserTests.TryReadOrientationFromJpeg_ReadsIfd0Orientation (little = false, orientation 8)
    let jpeg = jpeg_with_app1(&tiff(false, &[short(ep::TAG_ORIENTATION, 8, false)], &[]), None);

    assert_eq!(ep::try_read_orientation_from_jpeg(&jpeg), Some(8));
}

/// C# `ExifParserTests.TryReadOrientationFromJpeg_InvalidOrMissing_ReturnsNull` (rows 0 and 9).
fn invalid_or_missing_orientation_is_null(orientation: u16) {
    let jpeg = jpeg_with_app1(&tiff(true, &[short(ep::TAG_ORIENTATION, orientation, true)], &[]), None);
    assert_eq!(ep::try_read_orientation_from_jpeg(&jpeg), None);
    let without = jpeg_with_app1(&tiff(true, &[ascii(ep::TAG_MODEL, "X")], &[]), None);
    assert_eq!(ep::try_read_orientation_from_jpeg(&without), None);
    assert_eq!(
        ep::try_read_orientation_from_jpeg(&[0x00, 0x01, 0x02, 0x03, 0x04]),
        None
    );
    assert_eq!(ep::try_read_orientation_from_jpeg(&[]), None);
}

#[test]
fn try_read_orientation_from_jpeg_invalid_or_missing_returns_null_for_zero() {
    // C#: ExifParserTests.TryReadOrientationFromJpeg_InvalidOrMissing_ReturnsNull (row 0)
    invalid_or_missing_orientation_is_null(0);
}

#[test]
fn try_read_orientation_from_jpeg_invalid_or_missing_returns_null_for_nine() {
    // C#: ExifParserTests.TryReadOrientationFromJpeg_InvalidOrMissing_ReturnsNull (row 9)
    invalid_or_missing_orientation_is_null(9);
}

// ---------------------------------------------------------------- ExifParserHostilePointerTests

#[test]
fn try_parse_jpeg_exif_pointer_back_to_ifd0_terminates_and_keeps_ifd0_fields() {
    // C#: ExifParserHostilePointerTests.TryParseJpeg_ExifPointerBackToIfd0_TerminatesAndKeepsIfd0Fields
    let mut block = tiff(true, &[ascii(ep::TAG_MAKE, "Nikon")], &[short(ep::TAG_ISO, 100, true)]);
    // IFD0 = [Make, ExifPointer]; the pointer entry is the second one: its value field is at 8 + 2 + 12 + 8.
    block[8 + 2 + 12 + 8..8 + 2 + 12 + 12].copy_from_slice(&8u32.to_le_bytes()); // pointer -> IFD0 itself

    let summary = ep::try_parse_jpeg(&jpeg_with_app1(&block, None)).expect("IFD0 Make is still read");

    assert_eq!(summary.camera_make.as_deref(), Some("Nikon"));
    assert_eq!(summary.iso, None); // the ISO lived in the old sub-IFD, which is no longer referenced
}

#[test]
fn try_parse_jpeg_exif_sub_ifd_that_contains_a_pointer_to_itself_is_read_once_without_looping() {
    // C#: ExifParserHostilePointerTests.TryParseJpeg_ExifSubIfdThatContainsAPointerToItself_IsReadOnceWithoutLooping
    // The builder writes the value of ANY 0x8769 entry as the Exif IFD offset, so this entry inside the Exif IFD
    // points at the Exif IFD itself.
    let exif = vec![
        builder::Entry {
            tag: ep::TAG_EXIF_IFD,
            typ: 4,
            count: 1,
            value: vec![0; 4],
        },
        short(ep::TAG_ISO, 640, true),
    ];
    let block = tiff(true, &[ascii(ep::TAG_MAKE, "Sony")], &exif);

    let summary = ep::try_parse_jpeg(&jpeg_with_app1(&block, None));

    assert_eq!(summary.as_ref().and_then(|s| s.camera_make.as_deref()), Some("Sony"));
    assert_eq!(summary.as_ref().and_then(|s| s.iso), Some(640));
}

#[test]
fn try_parse_jpeg_exif_sub_ifd_with_count_65535_on_a_small_block_reads_only_the_entries_inside_the_block() {
    // C#: ExifParserHostilePointerTests.TryParseJpeg_ExifSubIfdWithCount65535OnASmallBlock_ReadsOnlyTheEntriesInsideTheBlock
    let mut block = tiff(true, &[ascii(ep::TAG_MAKE, "Leica")], &[short(ep::TAG_ISO, 800, true)]);
    // Exif IFD starts right after IFD0 (8 + 2 + 2 * 12 + 4: IFD0 has Make + pointer).
    let exif_ifd_offset = 8 + 2 + (2 * 12) + 4;
    block[exif_ifd_offset] = 0xFF;
    block[exif_ifd_offset + 1] = 0xFF;

    let summary = ep::try_parse_jpeg(&jpeg_with_app1(&block, None));

    assert_eq!(summary.as_ref().and_then(|s| s.camera_make.as_deref()), Some("Leica"));
    assert_eq!(summary.as_ref().and_then(|s| s.iso), Some(800));
}

#[test]
fn try_read_orientation_from_jpeg_ifd0_count_of_65535_on_a_small_block_still_finds_the_orientation_inside_the_block() {
    // C#: ExifParserHostilePointerTests.TryReadOrientationFromJpeg_Ifd0CountOf65535OnASmallBlock_StillFindsTheOrientationInsideTheBlock
    let mut block = tiff(true, &[short(ep::TAG_ORIENTATION, 6, true)], &[]);
    block[8] = 0xFF;
    block[9] = 0xFF;

    assert_eq!(
        ep::try_read_orientation_from_jpeg(&jpeg_with_app1(&block, None)),
        Some(6)
    );
}

#[test]
fn try_parse_jpeg_every_truncation_of_a_self_pointing_block_never_panics() {
    // C#: ExifParserHostilePointerTests.TryParseJpeg_EveryTruncationOfASelfPointingBlock_NeverThrows
    // (the C# body calls TryParseTiffBlock on every prefix; Rust: never panics, no hang guard, see the header)
    let mut block = tiff(true, &[ascii(ep::TAG_MAKE, "Nikon")], &[short(ep::TAG_ISO, 100, true)]);
    block[8 + 2 + 12 + 8..8 + 2 + 12 + 12].copy_from_slice(&8u32.to_le_bytes());

    for length in 0..=block.len() {
        let _ = ep::try_parse_tiff_block(&block[..length]);
    }
}

// ---------------------------------------------------------------- ExifPointerRangeGapTests

/// IFD0 with two Exif pointers (LONG each) and the Exif IFD at 38 holding ISO 400.
fn range_gap_block(first_pointer: u32, second_pointer: u32) -> Vec<u8> {
    TiffBytes::new(true, 80)
        .header(8)
        .ifd(8, 0, &[long(0x8769, first_pointer), long(0x8769, second_pointer)])
        .ifd(38, 0, &[t_short(0x8827, 400)])
        .to_vec()
}

#[test]
fn too_small_first_pointer_is_skipped_and_the_next_valid_pointer_is_used() {
    // C#: ExifPointerRangeGapTests.TooSmallFirstPointerIsSkippedAndTheNextValidPointerIsUsed
    let summary = ep::try_parse_tiff_block(&range_gap_block(4, 38));

    assert_eq!(summary.and_then(|s| s.iso), Some(400));
}

#[test]
fn first_pointer_within_range_wins_even_when_it_points_nowhere() {
    // C#: ExifPointerRangeGapTests.FirstPointerWithinRangeWinsEvenWhenItPointsNowhere
    // 0xFFFFFFFF is the largest accepted pointer value; it is taken (and fails to resolve), so the later valid one is not used.
    assert_eq!(ep::try_parse_tiff_block(&range_gap_block(u32::MAX, 38)), None);
}

#[test]
fn first_valid_pointer_wins_over_a_later_one() {
    // C#: ExifPointerRangeGapTests.FirstValidPointerWinsOverALaterOne
    let summary = ep::try_parse_tiff_block(&range_gap_block(38, 4));

    assert_eq!(summary.and_then(|s| s.iso), Some(400));
}

// ---------------------------------------------------------------- ExifParserRoundTripPropertyTests

fn random_ascii(rng: &mut XorShift, max_length: usize) -> String {
    let length = rng.below(max_length + 1);
    (0..length).map(|_| (0x20 + rng.below(0x5F)) as u8 as char).collect()
}

fn random_date(rng: &mut XorShift) -> String {
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        1990 + rng.below(41),
        1 + rng.below(12),
        1 + rng.below(28),
        rng.below(24),
        rng.below(60),
        rng.below(60)
    )
}

fn random_rational(rng: &mut XorShift) -> Option<ExifRational> {
    if rng.below(3) == 0 {
        None
    } else {
        Some(ExifRational::new(
            (1 + rng.below(99_999)) as u32,
            (1 + rng.below(99_999)) as u32,
        ))
    }
}

fn shuffle<T>(rng: &mut XorShift, items: &mut [T]) {
    for i in (1..items.len()).rev() {
        let j = rng.below(i + 1);
        items.swap(i, j);
    }
}

#[test]
fn random_valid_exif_round_trips() {
    // C#: ExifParserRoundTripPropertyTests.RandomValidExif_RoundTrips (6000 iterations, expected = ExifSummary.Create of the raw values)
    let mut rng = XorShift(0xE1F);
    let mut parsed_fields = 0usize;
    for i in 0..6_000 {
        let little = rng.below(2) == 0;
        let make = if rng.below(4) == 0 {
            None
        } else {
            Some(random_ascii(&mut rng, 100))
        };
        let model = if rng.below(4) == 0 {
            None
        } else {
            Some(random_ascii(&mut rng, 100))
        };
        let lens = if rng.below(4) == 0 {
            None
        } else {
            Some(random_ascii(&mut rng, 100))
        };
        let date_original = if rng.below(3) == 0 {
            None
        } else {
            Some(random_date(&mut rng))
        };
        let date_time = if rng.below(3) == 0 {
            None
        } else {
            Some(random_date(&mut rng))
        };
        let iso: Option<i64> = if rng.below(3) == 0 {
            None
        } else {
            Some(1 + rng.below(2_999_999) as i64)
        };
        let focal = random_rational(&mut rng);
        let f_number = random_rational(&mut rng);
        let exposure = random_rational(&mut rng);

        let mut ifd0 = Vec::new();
        let mut exif = Vec::new();
        if let Some(make) = make.as_deref().filter(|text| !text.is_empty()) {
            ifd0.push(ascii(ep::TAG_MAKE, make));
        }
        if let Some(model) = model.as_deref().filter(|text| !text.is_empty()) {
            ifd0.push(ascii(ep::TAG_MODEL, model));
        }
        if let Some(date_time) = &date_time {
            ifd0.push(ascii(ep::TAG_DATE_TIME, date_time));
        }
        // orientation: unknown to the summary
        ifd0.push(short(ep::TAG_ORIENTATION, 1 + rng.below(8) as u16, little));
        // Artist: unknown to the summary
        ifd0.push(builder::Entry {
            tag: 0x013B,
            typ: 2,
            count: 5,
            value: b"Anna\0".to_vec(),
        });
        if let Some(lens) = lens.as_deref().filter(|text| !text.is_empty()) {
            exif.push(ascii(ep::TAG_LENS_MODEL, lens));
        }
        if let Some(date_original) = &date_original {
            exif.push(ascii(ep::TAG_DATE_TIME_ORIGINAL, date_original));
        }
        if let Some(iso) = iso {
            if iso <= i64::from(u16::MAX) {
                exif.push(short(ep::TAG_ISO, iso as u16, little));
            } else {
                exif.push(builder::Entry {
                    tag: ep::TAG_ISO,
                    typ: 4,
                    count: 1,
                    value: u32_bytes(iso as u32, little),
                });
            }
        }
        if let Some(f) = focal {
            exif.push(rational(ep::TAG_FOCAL_LENGTH, f.numerator, f.denominator, little));
        }
        if let Some(n) = f_number {
            exif.push(rational(ep::TAG_F_NUMBER, n.numerator, n.denominator, little));
        }
        if let Some(e) = exposure {
            exif.push(rational(ep::TAG_EXPOSURE_TIME, e.numerator, e.denominator, little));
        }
        // ExposureBias: unknown to the summary
        exif.push(short(0x9204, 0, little));
        if exif.len() == 1 {
            exif.clear(); // only the unknown tag: no Exif sub-IFD at all
        }
        shuffle(&mut rng, &mut ifd0);
        shuffle(&mut rng, &mut exif);

        let jpeg = jpeg_with_app1(&tiff(little, &ifd0, &exif), None);
        let actual = ep::try_parse_jpeg(&jpeg);

        let expected = ExifSummary::create(&RawExifTags {
            date_taken: date_original.as_deref().or(date_time.as_deref()),
            make: make.as_deref(),
            model: model.as_deref(),
            lens: lens.as_deref(),
            iso,
            focal_length: focal,
            f_number,
            exposure_time: exposure,
        });
        assert_eq!(actual, expected, "iteration {i} little={little}");
        if actual.is_some() {
            parsed_fields += 1;
        }
    }

    assert!(
        parsed_fields > 5_000,
        "only {parsed_fields} iterations produced a summary"
    );
}

// ---------------------------------------------------------------- ExifMetadataMutationGapTests (ExifParser section)

const ABC_INLINE: u32 = 0x0043_4241; // "ABC\0" little-endian

/// C# `ExifMetadataMutationGapTests.DatesTiff`: IFD0 holds the Exif pointer (and DateTime when asked), the Exif IFD
/// holds DateTimeOriginal and DateTimeDigitized when asked.
fn dates_tiff(original: bool, digitized: bool, date_time: bool) -> Vec<u8> {
    let mut exif = Vec::new();
    if original {
        exif.push(at(0x9003, 2, 20, 300));
    }
    if digitized {
        exif.push(at(0x9004, 2, 20, 330));
    }
    let mut ifd0 = vec![long(0x8769, 100)];
    if date_time {
        ifd0.push(at(0x0132, 2, 20, 360));
    }
    TiffBytes::new(true, 400)
        .header(8)
        .ifd(8, 0, &ifd0)
        .ifd(100, 0, &exif)
        .put(300, b"2024:05:01 14:03:22\0")
        .put(330, b"2023:04:02 13:02:21\0")
        .put(360, b"2022:03:03 12:01:20\0")
        .to_vec()
}

#[test]
fn try_read_orientation_byte_order_mark_with_only_one_matching_letter_is_rejected() {
    // C#: ExifMetadataMutationGapTests.TryReadOrientation_ByteOrderMarkWithOnlyOneMatchingLetter_IsRejected
    let mut little = TiffBytes::new(true, 64)
        .header(8)
        .ifd(8, 0, &[t_short(0x0112, 6)])
        .to_vec();
    let mut big = TiffBytes::new(false, 64)
        .header(8)
        .ifd(8, 0, &[t_short(0x0112, 6)])
        .to_vec();
    assert_eq!(ep::try_read_orientation(&little), Some(6));
    assert_eq!(ep::try_read_orientation(&big), Some(6));

    little[1] = b'M'; // "IM"
    big[1] = b'I'; // "MI"

    assert_eq!(ep::try_read_orientation(&little), None);
    assert_eq!(ep::try_read_orientation(&big), None);
}

#[test]
fn try_parse_tiff_block_byte_order_mark_with_only_one_matching_letter_is_rejected() {
    // C#: ExifMetadataMutationGapTests.TryParseTiffBlock_ByteOrderMarkWithOnlyOneMatchingLetter_IsRejected
    let mut little = TiffBytes::new(true, 64)
        .header(8)
        .ifd(8, 0, &[at(0x010F, 2, 4, ABC_INLINE)])
        .to_vec();
    let mut big = TiffBytes::new(false, 64)
        .header(8)
        .ifd(8, 0, &[at(0x010F, 2, 4, 0x4142_4300)])
        .to_vec();
    assert_eq!(
        ep::try_parse_tiff_block(&little).and_then(|s| s.camera_make).as_deref(),
        Some("ABC")
    );
    assert_eq!(
        ep::try_parse_tiff_block(&big).and_then(|s| s.camera_make).as_deref(),
        Some("ABC")
    );

    little[1] = b'M';
    big[1] = b'I';

    assert_eq!(ep::try_parse_tiff_block(&little), None);
    assert_eq!(ep::try_parse_tiff_block(&big), None);
}

#[test]
fn try_read_orientation_ifd0_at_offset_sixteen_is_read() {
    // C#: ExifMetadataMutationGapTests.TryReadOrientation_Ifd0AtOffsetSixteen_IsRead
    let tiff_block = TiffBytes::new(true, 64)
        .header(16)
        .ifd(16, 0, &[t_short(0x0112, 6)])
        .to_vec();

    assert_eq!(ep::try_read_orientation(&tiff_block), Some(6));
}

#[test]
fn try_read_orientation_ifd0_offset_inside_the_header_is_rejected() {
    // C#: ExifMetadataMutationGapTests.TryReadOrientation_Ifd0OffsetInsideTheHeader_IsRejected
    // IFD at 2: count = 42 (from the magic), entry 1 lies at offset 16 and is a valid orientation entry.
    let tiff_block = TiffBytes::new(true, 64)
        .header(2)
        .put(16, &fake_entry(0x0112, 3, 1, 6))
        .to_vec();

    assert_eq!(ep::try_read_orientation(&tiff_block), None);
}

#[test]
fn try_parse_tiff_block_ifd0_offset_inside_the_header_is_rejected() {
    // C#: ExifMetadataMutationGapTests.TryParseTiffBlock_Ifd0OffsetInsideTheHeader_IsRejected
    let tiff_block = TiffBytes::new(true, 64)
        .header(2)
        .put(16, &fake_entry(0x010F, 2, 4, ABC_INLINE))
        .to_vec();

    assert_eq!(ep::try_parse_tiff_block(&tiff_block), None);
}

#[test]
fn try_parse_tiff_block_ifd0_at_offset_sixteen_is_read() {
    // C#: ExifMetadataMutationGapTests.TryParseTiffBlock_Ifd0AtOffsetSixteen_IsRead
    let tiff_block = TiffBytes::new(true, 64)
        .header(16)
        .ifd(16, 0, &[at(0x010F, 2, 4, ABC_INLINE)])
        .to_vec();

    assert_eq!(
        ep::try_parse_tiff_block(&tiff_block)
            .and_then(|s| s.camera_make)
            .as_deref(),
        Some("ABC")
    );
}

#[test]
fn try_read_orientation_bytes_after_the_counted_entries_that_look_like_an_entry_are_not_read() {
    // C#: ExifMetadataMutationGapTests.TryReadOrientation_BytesAfterTheCountedEntriesThatLookLikeAnEntry_AreNotRead
    // One counted entry (not orientation); its next-IFD pointer and the bytes after it spell a valid orientation entry.
    let tiff_block = TiffBytes::new(true, 40)
        .header(8)
        .ifd(8, 0, &[t_short(0x0100, 5)])
        .put(22, &fake_entry(0x0112, 3, 1, 6))
        .to_vec();

    assert_eq!(ep::try_read_orientation(&tiff_block), None);
}

#[test]
fn try_parse_tiff_block_bytes_after_the_counted_entries_that_look_like_an_entry_are_not_read() {
    // C#: ExifMetadataMutationGapTests.TryParseTiffBlock_BytesAfterTheCountedEntriesThatLookLikeAnEntry_AreNotRead
    let ifd0 = TiffBytes::new(true, 40)
        .header(8)
        .ifd(8, 0, &[t_short(0x0100, 5)])
        .put(22, &fake_entry(0x010F, 2, 4, ABC_INLINE))
        .to_vec();
    let exif_ifd = TiffBytes::new(true, 40)
        .header(8)
        .ifd(8, 0, &[t_short(0x0100, 5)])
        .put(22, &fake_entry(0x8827, 3, 1, 640))
        .to_vec();

    assert_eq!(ep::try_parse_tiff_block(&ifd0), None);
    assert_eq!(ep::try_parse_tiff_block_with(&exif_ifd, true), None);
}

/// C# `TryParseTiffBlock_Dates_OriginalBeatsDigitizedBeatsDateTime` (5 rows, checked in order).
#[test]
fn try_parse_tiff_block_dates_original_beats_digitized_beats_date_time() {
    // C#: ExifMetadataMutationGapTests.TryParseTiffBlock_Dates_OriginalBeatsDigitizedBeatsDateTime
    let rows = [
        ((true, true, true), "2024-05-01T14:03:22"),
        ((true, false, true), "2024-05-01T14:03:22"),
        ((false, true, true), "2023-04-02T13:02:21"),
        ((false, true, false), "2023-04-02T13:02:21"),
        ((false, false, true), "2022-03-03T12:01:20"),
    ];
    for ((original, digitized, date_time), expected) in rows {
        let summary = ep::try_parse_tiff_block(&dates_tiff(original, digitized, date_time));
        let actual = summary.as_ref().and_then(builder::date_text);
        assert_eq!(
            actual.as_deref(),
            Some(expected),
            "original={original} digitized={digitized} date_time={date_time}"
        );
    }
}

#[test]
fn try_parse_tiff_block_exif_pointer_of_eight_is_followed_once() {
    // C#: ExifMetadataMutationGapTests.TryParseTiffBlock_ExifPointerOfEight_IsFollowedOnce
    // The pointer addresses IFD0 itself; read in Exif mode it yields the ISO that sits in IFD0.
    let tiff_block = TiffBytes::new(true, 128)
        .header(8)
        .ifd(
            8,
            0,
            &[at(0x010F, 2, 4, ABC_INLINE), long(0x8769, 8), t_short(0x8827, 100)],
        )
        .to_vec();

    let summary = ep::try_parse_tiff_block(&tiff_block).expect("IFD0 fields are read");

    assert_eq!(summary.camera_make.as_deref(), Some("ABC"));
    assert_eq!(summary.iso, Some(100));
}

#[test]
fn try_parse_tiff_block_exif_pointer_below_eight_is_not_followed() {
    // C#: ExifMetadataMutationGapTests.TryParseTiffBlock_ExifPointerBelowEight_IsNotFollowed
    let tiff_block = TiffBytes::new(true, 128)
        .header(8)
        .ifd(
            8,
            0,
            &[at(0x010F, 2, 4, ABC_INLINE), long(0x8769, 7), t_short(0x8827, 100)],
        )
        .to_vec();

    let summary = ep::try_parse_tiff_block(&tiff_block).expect("Make is read");

    assert_eq!(summary.camera_make.as_deref(), Some("ABC"));
    assert_eq!(summary.iso, None);
}

// ---------------------------------------------------------------- OrfExifMagicTests (ExifParser part)

const IIRO: [u8; 4] = [b'I', b'I', 0x52, 0x4F];
const IIRS: [u8; 4] = [b'I', b'I', 0x52, 0x53];
const MMOR: [u8; 4] = [b'M', b'M', 0x4F, 0x52];

/// C# `OrfExifMagicTests.BuildOrf`: whole-file TIFF-shaped block with IFD0 { Make = "OLYMPUS", Model = "E-M1" }.
fn build_orf(little: bool, signature: &[u8]) -> Vec<u8> {
    TiffBytes::new(little, 200)
        .header_with(8, signature)
        .ifd(8, 0, &[at(0x010F, 2, 8, 100), at(0x0110, 2, 5, 120)])
        .put(100, b"OLYMPUS\0")
        .put(120, b"E-M1\0")
        .to_vec()
}

#[test]
fn try_parse_jpeg_exif_block_with_orf_magic_stays_strict() {
    // C#: OrfExifMagicTests.TryParseJpeg_ExifBlockWithOrfMagic_StaysStrict
    let block = build_orf(true, &IIRO);
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(&block);
    let mut jpeg = vec![0xFF, 0xD8];
    add_segment(&mut jpeg, 0xE1, &payload);
    jpeg.extend_from_slice(&[0xFF, 0xD9]);

    assert_eq!(ep::try_parse_jpeg(&jpeg), None);
}

#[test]
fn try_parse_tiff_block_orf_magic_blocks_are_read() {
    // C#: OrfExifMagicTests.TryReadExif_IiroBlock_YieldsMakeAndModel, TryReadExif_IirsBlock_YieldsMakeAndModel,
    // TryReadExif_BigEndianMmorBlock_YieldsMakeAndModel (the lines that call try_parse_tiff_block on the blocks directly)
    let iiro = ep::try_parse_tiff_block(&build_orf(true, &IIRO)).expect("IIRO block is read");
    assert_eq!(iiro.camera_make.as_deref(), Some("OLYMPUS"));
    assert_eq!(iiro.camera_model.as_deref(), Some("E-M1"));

    let iirs = ep::try_parse_tiff_block(&build_orf(true, &IIRS)).expect("IIRS block is read");
    assert_eq!(iirs.camera_make.as_deref(), Some("OLYMPUS"));

    let mmor = ep::try_parse_tiff_block(&build_orf(false, &MMOR)).expect("big-endian MMOR block is read");
    assert_eq!(mmor.camera_model.as_deref(), Some("E-M1"));
}

// ---------------------------------------------------------------- Cr3ExifMergeTests (ExifParser part)

/// C# `Cr3ExifMergeTests.BuildExposureTiff`: IFD0 with ISO 800, f/2.8, 1/250, focal 50, lens name, DateTimeOriginal.
fn build_exposure_tiff() -> Vec<u8> {
    TiffBytes::new(true, 512)
        .header(8)
        .ifd(
            8,
            0,
            &[
                at(0x829A, 5, 1, 200),
                at(0x829D, 5, 1, 208),
                t_short(0x8827, 800),
                at(0x9003, 2, 20, 216),
                at(0x920A, 5, 1, 240),
                at(0xA434, 2, 8, 250),
            ],
        )
        .u32(200, 1)
        .u32(204, 250)
        .u32(208, 28)
        .u32(212, 10)
        .u32(240, 50)
        .u32(244, 1)
        .put(216, b"2020:01:02 03:04:05\0")
        .put(250, b"RF50mm\0\0")
        .to_vec()
}

#[test]
fn try_parse_tiff_block_ifd_is_exif_reads_exposure_from_ifd0() {
    // C#: Cr3ExifMergeTests.TryParseTiffBlock_IfdIsExif_ReadsExposureFromIfd0
    let exif = ep::try_parse_tiff_block_with(&build_exposure_tiff(), true);

    assert_eq!(exif.and_then(|s| s.iso), Some(800));
    assert_eq!(ep::try_parse_tiff_block_with(&build_exposure_tiff(), false), None);
}

// ---------------------------------------------------------------- CombinedHeaderScanTests (ExifParser part)

#[test]
fn try_parse_tiff_block_matches_try_parse_jpeg() {
    // C#: CombinedHeaderScanTests.TryParseTiffBlock_MatchesTryParseJpeg
    // (Rust: EncodeJpegWithExif is replaced by JpegWithApp1(Tiff(true, FullCamera)), see the header)
    assert_eq!(ep::try_parse_tiff_block(&[]), None);

    let (ifd0, exif) = full_camera(true);
    let jpeg = jpeg_with_app1(&tiff(true, &ifd0, &exif), None);
    let tiff_block = ep::find_exif_tiff_block(&jpeg);

    let via_block = ep::try_parse_tiff_block(tiff_block);
    let via_jpeg = ep::try_parse_jpeg(&jpeg);
    builder::assert_full_camera(via_block.clone());
    assert_eq!(via_block, via_jpeg);
}

// ---------------------------------------------------------------- Golden: tests/golden/exif-parser.json

fn hex_bytes(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0, "odd hex length in golden input");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex byte in golden input"))
        .collect()
}

fn rational_value(rational: Option<ExifRational>) -> Value {
    match rational {
        Some(r) => json!({ "numerator": r.numerator, "denominator": r.denominator }),
        None => Value::Null,
    }
}

/// A summary in the golden shape: `null` or `{date_taken, camera_make, camera_model, lens_model, iso,
/// focal_length, f_number, exposure_time}`.
fn summary_value(summary: Option<&ExifSummary>) -> Value {
    let Some(summary) = summary else {
        return Value::Null;
    };
    json!({
        "date_taken": builder::date_text(summary),
        "camera_make": summary.camera_make,
        "camera_model": summary.camera_model,
        "lens_model": summary.lens_model,
        "iso": summary.iso,
        "focal_length": rational_value(summary.focal_length),
        "f_number": rational_value(summary.f_number),
        "exposure_time": rational_value(summary.exposure_time),
    })
}

fn orientation_value(orientation: Option<u8>) -> Value {
    orientation.map_or(Value::Null, Value::from)
}

#[test]
fn golden_exif_parser_matches_csharp_oracle() {
    // Golden: tests/golden/exif-parser.json (146 cases; ops jpeg, tiff, tiff-exif, orientation-jpeg, orientation-tiff)
    let golden = golden::load("exif-parser");
    assert_eq!(golden.cases.len(), 146, "the W4-03b golden package holds 146 cases");

    let mut failures = Vec::new();
    for case in &golden.cases {
        let name = golden::str_of(case, "name");
        let op = golden::str_of(case, "op");
        let input = hex_bytes(golden::str_of(case, "hex"));
        assert!(
            !golden::bool_of(case, "threw"),
            "{name}: the oracle never throws, the port never panics"
        );

        let (actual, expected) = match op {
            "jpeg" => (
                summary_value(ep::try_parse_jpeg(&input).as_ref()),
                case["summary"].clone(),
            ),
            "tiff" => (
                summary_value(ep::try_parse_tiff_block(&input).as_ref()),
                case["summary"].clone(),
            ),
            "tiff-exif" => (
                summary_value(ep::try_parse_tiff_block_with(&input, true).as_ref()),
                case["summary"].clone(),
            ),
            "orientation-jpeg" => (
                orientation_value(ep::try_read_orientation_from_jpeg(&input)),
                case["orientation"].clone(),
            ),
            "orientation-tiff" => (
                orientation_value(ep::try_read_orientation(&input)),
                case["orientation"].clone(),
            ),
            other => panic!("{name}: unknown golden op {other}"),
        };
        if actual != expected {
            failures.push(format!("{name} ({op}): expected {expected}, got {actual}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} golden cases differ from the C# oracle:\n{}",
        failures.len(),
        golden.cases.len(),
        failures.join("\n")
    );
}
