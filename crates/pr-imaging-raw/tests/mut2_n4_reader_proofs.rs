//! N4 (2026-11-05): run-based equivalence proofs for the five survivors of the ARW + CR2 mutation run
//! (`readers/arw.rs` 124:32, 172:21, 243:15 twice; `readers/cr2.rs` 145:32). Inline bytes only: no golden file, no corpus.
//!
//! Each test passes on the unmutated code AND under the mutant it is named after (the harness reports that mutant as
//! MISSED, which for these tests is the proof: the observable result is identical on the whole domain on which the mutant
//! and the original differ). The domain is stated in each test comment.

use pr_core::sync::CancelToken;
use pr_imaging_raw::readers::arw::ArwContainerReader;
use pr_imaging_raw::readers::cr2::Cr2ContainerReader;
use pr_imaging_raw::tiff_nav::{read_ifd_entries, try_read_exif_pixel_dimensions};
use pr_imaging_raw::{InMemoryRawHeaderSource, RawContainerInfo, RawContainerReader};
use pr_test_support::raw::{TiffBytes, XorShift64, at, long, short};

const CR2_MARKER: &[u8; 8] = b"CR\x02\0\0\0\0\0";

fn read_arw(data: Vec<u8>) -> RawContainerInfo {
    let mut source = InMemoryRawHeaderSource::new(data);
    ArwContainerReader
        .read(&mut source, &CancelToken::new())
        .expect("synthetic ARW parses")
}

fn read_cr2(data: Vec<u8>) -> RawContainerInfo {
    let mut source = InMemoryRawHeaderSource::new(data);
    Cr2ContainerReader::new()
        .read(&mut source, &CancelToken::new())
        .expect("synthetic CR2 parses")
}

/// The only input on which `exif_offset > 0` and `exif_offset >= 0` differ is `exif_offset == 0`. In that case the mutant calls
/// `read_ifd_entries(source, 0, ..)` and `try_read_exif_pixel_dimensions(source, 0, ..)`. Both are proven here, over 600 random
/// sources of every length 0..=299 plus both byte orders, to give "no entries" and "no pixel size" for offset 0 (the guard
/// `ifd_offset < 8` answers before any byte is read).
#[test]
fn ifd_reads_at_offset_zero_are_empty_for_any_source() {
    let mut rng = XorShift64::new(0x4E34_0001);
    for length in 0..300usize {
        for little in [true, false] {
            let bytes: Vec<u8> = (0..length).map(|_| (rng.next_u64() & 0xFF) as u8).collect();
            let mut source = InMemoryRawHeaderSource::new(bytes.clone());
            let read = read_ifd_entries(&mut source, 0, little).expect("offset 0 never reads");
            assert!(read.entries.is_empty(), "length {length} little {little}");
            assert_eq!(read.next_ifd_offset, 0);
            let mut source = InMemoryRawHeaderSource::new(bytes);
            assert_eq!(
                try_read_exif_pixel_dimensions(&mut source, 0, little).expect("offset 0 never reads"),
                None
            );
        }
    }
}

/// arw.rs:124:32 `exif_offset > 0` -> `>= 0`: an ARW whose Exif pointer is 0 reads like one without an Exif IFD: the sensor
/// size comes from the raw IFD, no maker note is latched, no preview, one Exif block. Byte order and a second IFD vary.
#[test]
fn arw_exif_pointer_zero_reads_like_no_exif_ifd() {
    for little in [true, false] {
        for second_ifd in [false, true] {
            let next = if second_ifd { 200 } else { 0 };
            let mut builder = TiffBytes::new(little, 400).header(16).ifd(
                16,
                next,
                &[short(0x0100, 120), short(0x0101, 80), long(0x8769, 0)],
            );
            if second_ifd {
                builder = builder.ifd(200, 0, &[short(0x0100, 30), short(0x0101, 20)]);
            }
            let info = read_arw(builder.into_vec());
            assert_eq!((info.sensor_width, info.sensor_height), (120, 80), "little {little}");
            assert!(info.previews.is_empty());
            assert_eq!(info.exif_blocks.len(), 1);
            assert_eq!(info.orientation, 1);
        }
    }
}

/// cr2.rs:145:32 `exif_offset > 0` -> `>= 0`. With offset 0 the mutant would push the Exif block inside the loop; the original
/// pushes it after the loop (`exif_blocks.is_empty()`), from the same call. The result is the same for both byte orders and for
/// one or two IFDs: one block, the sensor size from the raw IFD, no preview.
#[test]
fn cr2_exif_pointer_zero_reads_like_no_exif_ifd() {
    for little in [true, false] {
        for second_ifd in [false, true] {
            let next = if second_ifd { 200 } else { 0 };
            let mut builder = TiffBytes::new(little, 400).header(16).put(8, CR2_MARKER).ifd(
                16,
                next,
                &[short(0x0100, 120), short(0x0101, 80), long(0x8769, 0)],
            );
            if second_ifd {
                builder = builder.ifd(200, 0, &[short(0x0100, 30), short(0x0101, 20)]);
            }
            let info = read_cr2(builder.into_vec());
            assert_eq!((info.sensor_width, info.sensor_height), (120, 80), "little {little}");
            assert!(info.previews.is_empty());
            assert_eq!(info.exif_blocks.len(), 1);
            assert_eq!(info.orientation, 1);
        }
    }
}

/// arw.rs:172:21 `note > 0` -> `>= 0`: the only differing input is a latched maker note offset 0. `parse_sony_maker_note_preview`
/// then returns at once for `note_offset < 8`. An Exif IFD whose MakerNote entry (count 10) points at offset 0 gives no preview,
/// although a valid Sony PreviewImage IFD sits at offset 8 and a JPEG at 300.
#[test]
fn arw_maker_note_offset_zero_lists_no_preview() {
    for little in [true, false] {
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.resize(100, 0);
        let data = TiffBytes::new(little, 600)
            .header(100)
            .ifd(100, 0, &[short(0x0100, 50), short(0x0101, 40), long(0x8769, 200)])
            .ifd(200, 0, &[at(0x927C, 7, 10, 0)])
            .ifd(8, 0, &[at(0x2001, 7, 100, 300)])
            .put(300, &jpeg)
            .into_vec();
        let info = read_arw(data);
        assert!(info.previews.is_empty(), "little {little}");
        assert_eq!((info.sensor_width, info.sensor_height), (50, 40));
    }
}

/// arw.rs:243:15 `length < 4` -> `== 4` / `<= 4` (after the guard `entry.count <= 4`). The mutants differ from the original only
/// for `length <= 4`, and `length` is `count`, which the guard has already sent away for `count <= 4`. Run over every `count`
/// in 0..=70 000 and the edges 2^17, 2^20, 2^24, 2^31, u32::MAX: counts 0..=4 list nothing, count 5..=100 with a valid JPEG
/// that long lists the preview, and no count panics. (The full u32 range is not enumerated; see the evidence file.)
#[test]
fn arw_sony_preview_entry_counts_up_to_four_list_nothing() {
    let mut jpeg = vec![0xFF, 0xD8];
    jpeg.resize(100, 0);
    let build = |count: u32| {
        TiffBytes::new(true, 600)
            .header(100)
            .ifd(100, 0, &[long(0x8769, 200)])
            .ifd(200, 0, &[at(0x927C, 7, 100, 8)])
            .ifd(8, 0, &[at(0x2001, 7, count, 300)])
            .put(300, &jpeg)
            .into_vec()
    };
    let counts = (0..=70_000u32).chain([1 << 17, 1 << 20, 1 << 24, 1 << 31, u32::MAX]);
    for count in counts {
        let info = read_arw(build(count));
        let expected = if (5..=300).contains(&count) { 1 } else { 0 };
        // The JPEG starts at 300 and the file is 600 bytes: a range up to 300 bytes fits; 301.. does not.
        assert_eq!(info.previews.len(), expected, "count {count}");
    }
}
