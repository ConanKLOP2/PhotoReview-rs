//! Synthetic reader boundary regressions; no camera files or native decoder required.
use pr_imaging_raw::readers::rw2::{ifd0_orientation, image_extent};

#[test]
fn libraw_handoff_keeps_downscale_backend_and_original_dimensions() {
    // Mutants: pr-libraw/src/decoder.rs:506-510, hand_off option field deletions.
    use pr_core::model::DecoderBackend;
    use pr_imaging::decode_types::{PixelLease, PlatformImage};
    use pr_imaging::decoder_traits::{DecodeError, PlatformImageCodec};
    use pr_imaging::memory_headroom::MemoryInfo;
    use pr_imaging::pixels::{PixelBuffer, PixelLayout};
    use pr_libraw::decoder::{LibRawApi, LibRawDecoder, LibRawSession, StagedPixels};
    use std::sync::Arc;
    struct NoNative;
    impl LibRawApi for NoNative {
        fn init(&self) -> Result<Box<dyn LibRawSession>, DecodeError> {
            panic!("pure handoff must not initialize LibRaw")
        }
        fn format_error(&self, _: i32) -> String {
            panic!("pure handoff must not query LibRaw")
        }
    }
    struct Codec;
    impl PlatformImageCodec for Codec {
        fn name(&self) -> &'static str {
            "mutation-pixels"
        }
        fn image_from_pixels(&self, pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
            Ok(PlatformImage::Pixels(Arc::new(pixels)))
        }
        fn to_pixels<'a>(&self, _: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
            panic!("handoff must not copy pixels back")
        }
    }
    struct Stage(Option<PixelBuffer>);
    impl StagedPixels for Stage {
        fn take_pixels(&mut self) -> Result<PixelBuffer, DecodeError> {
            Ok(self.0.take().expect("one handoff"))
        }
    }
    let decoder = LibRawDecoder::new(
        Arc::new(Codec),
        Arc::new(NoNative),
        Arc::new(|| MemoryInfo {
            total_available: 0,
            load: 0,
        }),
    );
    let mut stage = Stage(Some(PixelBuffer::allocate(2, 3, PixelLayout::Bgr32).unwrap()));
    let image = decoder.hand_off(&mut stage, true, 4000, 6000, &|| true).unwrap();
    assert!(image.downscaled());
    assert_eq!(image.orientation(), 1);
    assert_eq!(image.actual_backend(), DecoderBackend::LibRaw);
    assert_eq!((image.original_width(), image.original_height()), (4000, 6000));
}

#[test]
fn libraw_jpeg_structure_handles_standalone_fill_and_truncated_segments() {
    // Mutants: pr-libraw/src/decoder.rs:694/701/704/708/712/716, comparisons and marker/segment arithmetic.
    // A channel timeout is a hang guard, not a performance assertion: malformed progress mutants must fail the test.
    fn find_end_of_image(bytes: &[u8]) -> Result<usize, pr_imaging::decoder_traits::DecodeError> {
        let bytes = bytes.to_vec();
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(pr_libraw::decoder::find_end_of_image(&bytes));
        });
        receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("pure parser hang guard")
    }
    for marker in [0x00, 0x01, 0xD8, 0xD0, 0xD7] {
        let bytes = [
            0xFF, 0xD8, 0xFF, marker, 0xFF, 0xD9, 0x12, 0xFF, 0xD9, 0x44, 0xFF, 0xDA, 0, 2,
        ];
        assert_eq!(find_end_of_image(&bytes).unwrap(), 6);
    }
    for marker in [0xE0, 0xC0, 0xFE] {
        let bytes = [
            0xFF, 0xD8, 0xFF, marker, 0, 2, 0xFF, 0xD9, 0x12, 0xFF, 0xD9, 0x44, 0xFF, 0xDA, 0, 2,
        ];
        assert_eq!(find_end_of_image(&bytes).unwrap(), 8);
    }
    // Mutants: decoder.rs:708 + -> * and segment/progress arithmetic after a long standalone prefix.
    let mut prefixed = vec![0xFF, 0xD8];
    for _ in 0..10 {
        prefixed.extend_from_slice(&[0xFF, 0x01]);
    }
    prefixed.extend_from_slice(&[0xFF, 0xE0, 0, 2, 0xFF, 0xD9, 0x12, 0xFF, 0xD9, 0x44, 0xFF, 0xDA, 0, 2]);
    assert_eq!(find_end_of_image(&prefixed).unwrap(), 28);
    // Mutants: decoder.rs:711 << -> >> and :712 < -> >, high length byte and a nonempty APP segment.
    let mut long_segment = vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 0];
    long_segment.extend_from_slice(&[0x11; 254]);
    long_segment.extend_from_slice(&[0xFF, 0xD9, 0x12, 0xFF, 0xD9, 0x44, 0xFF, 0xDA, 0, 2]);
    assert_eq!(find_end_of_image(&long_segment).unwrap(), 262);
    assert_eq!(
        find_end_of_image(&[
            0xFF, 0xD8, 0xFF, 0xE0, 0, 3, 0x55, 0xFF, 0xD9, 0x12, 0xFF, 0xD9, 0x44, 0xFF, 0xDA, 0, 2
        ])
        .unwrap(),
        9
    );
    // Mutants: decoder.rs:751-762, fallback window/marker/tail comparisons and arithmetic.
    assert_eq!(find_end_of_image(&[0xFF, 0xD8, 0x11, 0xFF, 0xD9, 0x44]).unwrap(), 5);
    // Mutants: decoder.rs:752:27 - -> /; :757:29 + -> - or *; :757:38 - -> /.
    assert_eq!(
        find_end_of_image(&[0xFF, 0xD8, 0x11, 0x11, 0x11, 0x11, 0xFF, 0xD9, 0x44]).unwrap(),
        8
    );
    assert!(find_end_of_image(&[0xFF, 0xD8, 0x11, 0xFF, 0xD9, 0xFF, 0xE0, 0, 2]).is_err());
    assert_eq!(
        find_end_of_image(&[0xFF, 0xD8, 0x11, 0xFF, 0xE0, 0xFF, 0xD9, 0x44]).unwrap(),
        7
    );
    assert_eq!(
        find_end_of_image(&[0xFF, 0xD8, 0x11, 0xFF, 0xD9, 0x44, 0xFF]).unwrap(),
        5
    );
    for marker in [0xD8, 0xDA, 0xDB, 0xDD, 0xFE, 0xC0, 0xCF, 0xE0, 0xEF] {
        assert!(find_end_of_image(&[0xFF, 0xD8, 0x11, 0xFF, 0xD9, 0x44, 0xFF, marker, 0, 2]).is_err());
    }
    for marker in [0x00, 0xD0, 0xD7, 0x01] {
        assert_eq!(
            find_end_of_image(&[0xFF, 0xD8, 0x11, 0xFF, 0xD9, 0x44, 0xFF, marker, 0x12]).unwrap(),
            5
        );
    }
    // Mutants: decoder.rs:724-739, entropy marker guards, byte progress and stuffed/restart match arms.
    // C#: LibRawThumbnailTrimMutationGapTests (the same adversarial streams as decoder_repair).
    for (hex, expected) in [
        ("FFD8FFDA0002FFFF0011FFD92233FFD944", 12),
        ("FFD8FFDA0002FF00FFD92233FFD944", 10),
        ("FFD8FFDA0002AABBFF00CCFFD92233FFD944", 13),
        ("FFD8FFDA0002FFD0FFD92233FFD944", 10),
        ("FFD8FFDA0002AAFFD0BBFFD92233FFD944", 12),
    ] {
        let bytes: Vec<u8> = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        assert_eq!(find_end_of_image(&bytes).unwrap(), expected);
    }
    for (offset, accepted) in [(3, false), (4, true)] {
        let mut bytes = vec![0x11; 65_540];
        bytes[..2].copy_from_slice(&[0xFF, 0xD8]);
        bytes[offset..offset + 2].copy_from_slice(&[0xFF, 0xD9]);
        let result = find_end_of_image(&bytes);
        if accepted {
            assert_eq!(result.unwrap(), offset + 2);
        } else {
            assert!(result.is_err());
        }
    }
    assert_eq!(find_end_of_image(&[0xFF, 0xD8, 0xFF, 0xFF, 0xD9, 0x12]).unwrap(), 5);
    for bytes in [
        &b"\xFF\xD8\xFF\xFF"[..],
        &b"\xFF\xD8\xFF\xE0\x00"[..],
        &b"\xFF\xD8\xFF\xE0\x00\x01"[..],
        &b"\xFF\xD8\xFF\xE0\x00\x02"[..],
        &b"\xFF\xD8\xFF\xE0\x01\x00"[..],
        &b"\xFF\xD8\xFF\xDA\x00\x02\xFF"[..],
        &b"\xFF\xD8\xFF\xDA\x00\x02\xFF\x01"[..],
    ] {
        assert!(find_end_of_image(bytes).is_err());
    }
}

#[test]
fn container_recognition_requires_matching_extension_and_complete_magic() {
    // Mutants: readers/{nef,orf,dng,rw2,raf}.rs can_read, bool replacement and &&/|| changes.
    use pr_imaging_raw::RawContainerReader;
    use pr_imaging_raw::readers::{
        dng::DngContainerReader, nef::NefContainerReader, orf::OrfContainerReader, raf::RafContainerReader,
        rw2::Rw2ContainerReader,
    };
    let cases: Vec<(Box<dyn RawContainerReader>, &str, &[u8])> = vec![
        (Box::new(NefContainerReader), ".NEF", b"II*\0"),
        (Box::new(OrfContainerReader::new()), ".ORF", b"IIRO"),
        (Box::new(DngContainerReader::new()), ".DNG", b"MM\0*"),
        (Box::new(Rw2ContainerReader::new()), ".RW2", b"IIU\0"),
        (Box::new(RafContainerReader::new()), ".RAF", b"FUJIFILMCCD-RAW "),
    ];
    for (reader, extension, magic) in cases {
        assert!(reader.can_read(magic, extension));
        assert!(!reader.can_read(magic, ".jpg"));
        assert!(!reader.can_read(&[0; 64], extension));
        for length in 0..magic.len() {
            assert!(!reader.can_read(&magic[..length], extension));
        }
    }
}

#[test]
fn raf_sensor_choice_keeps_priority_and_rejects_oversized_candidates() {
    // Mutants: readers/raf.rs:259-267 choose_sensor_size, bool and comparison changes.
    use pr_imaging_raw::readers::raf::{CfaSizes, choose_sensor_size};
    for (full, cropped, image_size, expected) in [
        ((100, 80), (90, 70), (80, 60), (90, 70)),
        ((100, 80), (101, 70), (80, 60), (80, 60)),
        ((100, 80), (90, 81), (100, 80), (100, 80)),
        ((100, 80), (0, 0), (101, 80), (100, 80)),
        ((100, 80), (0, 0), (100, 81), (100, 80)),
        ((0, 0), (90, 70), (80, 60), (90, 70)),
        ((0, 0), (0, 0), (80, 60), (80, 60)),
        ((100, 80), (100, 80), (80, 60), (100, 80)),
        ((100, 80), (0, 0), (0, 0), (100, 80)),
    ] {
        assert_eq!(
            choose_sensor_size(&CfaSizes {
                full,
                cropped,
                image_size
            }),
            expected
        );
    }
}

#[test]
fn rw2_exif_lengths_are_bounded_by_block_and_remaining_file() {
    // Mutants: readers/rw2.rs:101/108, return replacement.
    use pr_imaging_raw::readers::rw2::{exif_block_length, exif_read_length};
    assert_eq!(exif_block_length(200_000), 131_072);
    assert_eq!(exif_block_length(17), 17);
    assert_eq!(exif_read_length(200_000, 100, 1000), 900);
    assert_eq!(exif_read_length(200_000, 100, 300_000), 131_072);
    assert_eq!(exif_read_length(17, 100, 300_000), 17);
}

#[test]
fn rw2_extent_prefers_valid_borders_and_falls_back_to_size() {
    // Mutants: readers/rw2.rs:74-84, comparisons, && -> ||, subtraction and return replacement.
    let max = i64::from(i32::MAX);
    for (start, end, size, expected) in [
        (Some(2), Some(9), Some(99), 7),
        (Some(0), Some(max), None, i32::MAX),
        (Some(-1), Some(9), Some(99), 99),
        (Some(9), Some(9), Some(99), 99),
        (Some(10), Some(9), Some(99), 99),
        (Some(0), Some(max + 1), Some(99), 99),
        (None, Some(9), Some(max), i32::MAX),
        (Some(0), None, Some(1), 1),
        (None, None, Some(0), 0),
        (None, None, Some(-1), 0),
        (None, None, Some(max + 1), 0),
        (None, None, None, 0),
    ] {
        assert_eq!(image_extent(start, end, size), expected);
    }
}

#[test]
fn rw2_orientation_accepts_only_one_through_eight() {
    // Mutants: readers/rw2.rs:92-96, Option return replacement and range boundary changes.
    for value in [-1, 0, 1, 4, 8, 9, i64::MAX] {
        let expected = if (1..=8).contains(&value) {
            Some(value as i32)
        } else {
            None
        };
        assert_eq!(ifd0_orientation(Some(value)), expected);
    }
    assert_eq!(ifd0_orientation(None), None);
}
