//! N4 (2026-11-05): equivalence proofs and killers for `pr_libraw::decoder`, found by the mutation run of the whole file.
//! Hand-written, no golden file, no native DLL, no sleep on the passing path.

use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use pr_imaging::decode_types::DecodedImageOptions;
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_libraw::decoder::{SHARED_READ_CHUNK_BYTES, SharedReadError, find_end_of_image, read_file_shared};
use pr_test_support::TempRoot;

/// Proof by evaluation for the mutant `decoder.rs:562:13` (`delete field orientation from struct DecodedImageOptions` in
/// `LibRawDecoder::hand_off`): `hand_off` writes `orientation: 1` and fills the rest from `DecodedImageOptions::default()`.
/// The deleted field is replaced by the default, so the two programs differ only if the default orientation is not 1. The
/// domain of the difference is this single constant; this test pins it, and it passes with and without the mutant.
#[test]
fn default_decoded_image_options_orientation_is_one_so_hand_off_orientation_one_is_redundant() {
    assert_eq!(DecodedImageOptions::default().orientation, 1);
}

// ---------------------------------------------------------------------------------------------- pure JPEG end finder

/// Runs `work` on its own thread and fails the test if it has not finished within `seconds`. A mutant that turns one of the
/// scanning loops into an endless loop then FAILS fast (it is caught) instead of hanging the harness until its timeout.
fn bounded<T: Send + 'static>(seconds: u64, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(work());
    });
    receiver
        .recv_timeout(Duration::from_secs(seconds))
        .unwrap_or_else(|_| panic!("did not finish within {seconds} s (an endless loop)"))
}

/// An independent port of the C# `FindEndOfImage` contract (structure walk, then the structure-free tail), written from the
/// documented rules rather than from the Rust body.
fn reference_end(b: &[u8]) -> Option<usize> {
    let len = b.len();
    if len < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut end = len;
    while end > 2 && (b[end - 1] == 0x00 || b[end - 1] == 0xFF) {
        end -= 1;
    }
    if end >= 4 && b[end - 1] == 0xD9 && b[end - 2] == 0xFF {
        return Some(end);
    }
    reference_structure(b).or_else(|| reference_tail(b))
}

fn reference_structure(b: &[u8]) -> Option<usize> {
    let len = b.len();
    let mut i = 2usize;
    loop {
        if i >= len || b[i] != 0xFF {
            return None;
        }
        while i < len && b[i] == 0xFF {
            i += 1;
        }
        let marker = *b.get(i)?;
        i += 1;
        match marker {
            0xD9 => return Some(i),
            0x00 | 0x01 | 0xD8 | 0xD0..=0xD7 => continue,
            _ => {}
        }
        let hi = *b.get(i)?;
        let lo = *b.get(i + 1)?;
        let segment = usize::from(hi) * 256 + usize::from(lo);
        if segment < 2 {
            return None;
        }
        i += segment;
        if i > len {
            return None;
        }
        if marker != 0xDA {
            continue;
        }
        loop {
            let byte = *b.get(i)?;
            if byte != 0xFF {
                i += 1;
                continue;
            }
            match *b.get(i + 1)? {
                0x00 | 0xD0..=0xD7 => i += 2,
                0xFF => i += 1,
                _ => break,
            }
        }
    }
}

fn reference_tail(b: &[u8]) -> Option<usize> {
    let len = b.len();
    if len < 2 {
        return None;
    }
    let lowest = len.saturating_sub(64 * 1024).max(2);
    let mut i = len - 2;
    loop {
        if i < lowest {
            return None;
        }
        if b[i] == 0xFF && b[i + 1] == 0xD9 {
            let structured = (i + 2..len - 1).any(|j| {
                b[j] == 0xFF && matches!(b[j + 1], 0xD8 | 0xDA | 0xDB | 0xDD | 0xFE | 0xC0..=0xCF | 0xE0..=0xEF)
            });
            return if structured { None } else { Some(i + 2) };
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

fn agree(bytes: &[u8]) {
    let got = find_end_of_image(bytes).ok();
    let want = reference_end(bytes);
    assert_eq!(got, want, "{bytes:02X?}");
}

/// Hand-made inputs that each exercise exactly one scanning loop, with the answer fixed by hand: 728 (strip of trailing 00/FF),
/// 750 (fill bytes), 783, 790, 791 (entropy data with a stuffed FF 00, a restart FF D0 and a doubled FF FF).
fn loop_cases() -> Vec<(&'static str, Vec<u8>, Option<usize>)> {
    vec![
        ("strip", vec![0xFF, 0xD8, 0xFF, 0xD9, 0x00, 0x00, 0x00], Some(4)),
        ("fill", vec![0xFF, 0xD8, 0xFF, 0xFF, 0xFF, 0xD9, 0x12], Some(6)),
        (
            "scan",
            vec![
                0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x03, 0x01, 0x11, 0xFF, 0x00, 0x22, 0xFF, 0xD0, 0x33, 0xFF, 0xFF, 0xFF,
                0xD9, 0x99,
            ],
            Some(18),
        ),
    ]
}

fn check_loop_cases() {
    for (name, bytes, expected) in loop_cases() {
        let got = bounded(10, {
            let bytes = bytes.clone();
            move || find_end_of_image(&bytes).ok()
        });
        assert_eq!(got, expected, "{name}");
        assert_eq!(reference_end(&bytes), expected, "reference {name}");
    }
}

/// Kills the endless-loop mutants of the fill-byte loop (750:15), the entropy scan (783:19, 790:41, 791:27) and the trailing
/// 00/FF strip (728:13) without hanging.
#[test]
fn the_end_finder_loops_terminate_with_the_documented_answers() {
    check_loop_cases();
}

/// `read_file_shared` loop mutants (890:18 `<` -> `==`/`<=`, 894:37 `+` -> `*`): an empty file, tiny files and files around one
/// chunk are read to the end without hanging.
#[test]
fn read_file_shared_terminates_for_empty_small_and_multi_chunk_files() {
    let memory: Arc<dyn MemoryInfoSource> = Arc::new(|| MemoryInfo {
        total_available: 64 << 30,
        load: 0,
    });
    let root = TempRoot::new("n4-read-shared");
    for size in [
        0usize,
        1,
        2,
        SHARED_READ_CHUNK_BYTES - 1,
        SHARED_READ_CHUNK_BYTES,
        SHARED_READ_CHUNK_BYTES + 1,
    ] {
        let path = root.join(format!("f{size}.raw"));
        let content: Vec<u8> = (0..size).map(|i| (i % 253) as u8).collect();
        std::fs::write(&path, &content).expect("fixture");
        let memory = Arc::clone(&memory);
        let read = bounded(20, move || {
            read_file_shared(&path, &pr_core::sync::CancelToken::none(), memory.as_ref())
        });
        match read {
            Ok(bytes) => assert_eq!(&*bytes, content.as_slice(), "size {size}"),
            Err(SharedReadError::Io(e)) => panic!("size {size}: {e}"),
            Err(other) => panic!("size {size}: {other:?}"),
        }
    }
}

/// decoder.rs:727:15 (`end > 2` -> `>= 2`), 763:18 (`i + 2 > len` -> `>=`), 766:55 (`|` -> `^`), 771:14 (`i > len` -> `==`/`>=`),
/// 803:12 (`len < 2` -> `==`/`<=`): differential against the independent reference over
///  * every `FF D8` + tail of up to 6 bytes drawn from 8 marker-relevant byte values (about 300 000 inputs),
///  * every `FF D8` + 00/FF tail of up to 14 bytes (the only inputs for which the strip loop reaches `end == 2`),
///  * every 16-bit segment length of an APP0 segment followed by the segment body and `FF D9 12` (65 536 inputs, 766),
///  * every (marker, length high, length low) triple of a 6-byte file, the whole domain of the `i + 2 == len` boundary
///    (2^24 inputs, 763), and
///  * segments that end 3 before, 2 before, 1 before and exactly at the end of the data, for lengths 2..=600 and the edges (771).
///
/// All of these agree on the original and under each mutant. Inputs shorter than 4 bytes are refused before the tail scan (803).
#[test]
fn the_end_finder_agrees_with_the_reference_on_the_boundary_families() {
    // First the hang guard: a mutant that makes a loop endless fails here after 10 s instead of hanging the big run below.
    check_loop_cases();
    bounded(60, || {
        for len in 0..=2usize {
            let total = 1u32 << (8 * len);
            for v in 0..total {
                let bytes: Vec<u8> = (0..len).map(|k| (v >> (8 * k)) as u8).collect();
                assert!(find_end_of_image(&bytes).is_err(), "{bytes:02X?}");
            }
        }
        for v in 0..(1u32 << 24) {
            let bytes = [v as u8, (v >> 8) as u8, (v >> 16) as u8];
            assert!(find_end_of_image(&bytes).is_err());
        }

        let alphabet = [0x00u8, 0x01, 0xD0, 0xD9, 0xDA, 0xFF, 0xC0, 0x12];
        for tail_len in 0..=6usize {
            let total = alphabet.len().pow(tail_len as u32);
            for n in 0..total {
                let mut bytes = vec![0xFF, 0xD8];
                let mut rest = n;
                for _ in 0..tail_len {
                    bytes.push(alphabet[rest % alphabet.len()]);
                    rest /= alphabet.len();
                }
                agree(&bytes);
            }
        }
        for tail_len in 0..=14usize {
            for n in 0..(1u32 << tail_len) {
                let mut bytes = vec![0xFF, 0xD8];
                bytes.extend((0..tail_len).map(|k| if (n >> k) & 1 == 1 { 0xFF } else { 0x00 }));
                agree(&bytes);
            }
        }
        for length in 0..=u16::MAX {
            let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
            bytes.extend_from_slice(&length.to_be_bytes());
            if length >= 2 {
                bytes.resize(4 + usize::from(length), 0x55);
            }
            bytes.extend_from_slice(&[0xFF, 0xD9, 0x12]);
            agree(&bytes);
        }
        for marker in 0..=u8::MAX {
            for length in 0..=u16::MAX {
                let l = length.to_be_bytes();
                agree(&[0xFF, 0xD8, 0xFF, marker, l[0], l[1]]);
            }
        }
        let lengths: Vec<usize> = (2..=600usize)
            .chain([1023, 1024, 1025, 4095, 4096, 65534, 65535])
            .collect();
        for marker in [0xE0u8, 0xDA, 0xC0, 0xFE] {
            for &length in &lengths {
                for body in [length.saturating_sub(3), length - 2, length - 1, length] {
                    let mut bytes = vec![0xFF, 0xD8, 0xFF, marker, (length >> 8) as u8, length as u8];
                    bytes.resize(6 + body, 0x55);
                    agree(&bytes);
                }
            }
        }
    });
}
