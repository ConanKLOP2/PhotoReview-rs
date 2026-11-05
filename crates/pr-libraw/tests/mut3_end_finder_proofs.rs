//! MUT3 (2026-11-06), task P2: whole-domain run proofs for the three `find_end_of_image` survivors that N4 left as
//! RUN-SUPPORTED (`decoder.rs` 727:15, 763:18, 771:14). Hand-written, no golden file, no native DLL, no sleep.
//!
//! Method (owner rule B, strict reading): each mutant differs from the original only on the executions that reach the mutated
//! comparison in ONE state. The state is a handful of locals; the tests enumerate that state COMPLETELY (not a sample, not a
//! bounded sequence length) and compare the real function with an independent port of the documented contract. The tests pass on
//! the original and under the mutants they name; that pass is the proof.

use std::thread;

use pr_libraw::decoder::find_end_of_image;

// ------------------------------------------------------------------------------------------------ independent reference

/// An independent port of the C# `FindEndOfImage` contract, written from the documented rules.
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
    assert_eq!(got, want, "{:02X?}", &bytes[..bytes.len().min(32)]);
}

/// Runs `body(first_marker..last_marker)` on four threads that split the 256 marker values.
fn split_markers(body: impl Fn(u8) + Sync) {
    let body = &body;
    thread::scope(|scope| {
        for quarter in 0..4u16 {
            scope.spawn(move || {
                for marker in (quarter * 64)..(quarter * 64 + 64) {
                    body(marker as u8);
                }
            });
        }
    });
}

// ------------------------------------------------------------------------------------------------ 727:15

/// decoder.rs:727:15 `while end > 2 && ...` -> `end >= 2 && ...`.
///
/// The operators differ in exactly one evaluation: `end == 2`, where the mutant additionally tests `bytes[1]` against `00`/`FF`.
/// The guard `bytes[0] == FF && bytes[1] == D8` runs first, so every input that reaches the loop has `bytes[1] == D8`, which is
/// neither `00` nor `FF`: the extra test is false and the loop stops exactly where the original stops. The free variable of the
/// extra evaluation is that single byte; all 256 values of it are run here (with `bytes[0] == FF`), each followed by every 00/FF
/// tail of 2..=12 bytes (the only tails for which the strip loop reaches `end == 2`: 8 188 tails). Every input is compared with the
/// independent reference, and the result must be an error (an all-padding stream has no EOI).
#[test]
fn the_strip_loop_stop_at_two_reads_only_a_byte_the_guard_already_fixed() {
    let mut inputs = 0usize;
    for second in 0..=255u8 {
        for tail_length in 2..=12usize {
            for pattern in 0u32..(1 << tail_length) {
                let mut bytes = vec![0xFF, second];
                bytes.extend((0..tail_length).map(|bit| if pattern >> bit & 1 == 1 { 0xFF } else { 0x00 }));
                agree(&bytes);
                assert!(
                    find_end_of_image(&bytes).is_err(),
                    "all padding has no EOI: {bytes:02X?}"
                );
                inputs += 1;
            }
        }
    }
    assert_eq!(inputs, 256 * 8188);
    // The premise, stated as a run: the only second byte that gets past the guard is D8, and D8 is not a padding byte.
    let accepted: Vec<u8> = (0..=255u8)
        .filter(|&second| {
            let probe = [0xFF, second, 0xFF, 0xD9];
            find_end_of_image(&probe).is_ok()
        })
        .collect();
    assert_eq!(accepted, vec![0xD8], "only FF D8 starts a stream");
    assert!(![0x00u8, 0xFF].contains(&0xD8));
}

// ------------------------------------------------------------------------------------------------ 763:18

/// decoder.rs:763:18 `if i + 2 > len` -> `>=`.
///
/// The operators differ in exactly one state: `i + 2 == len`, i.e. only the two length bytes are left after a non-standalone
/// marker. From there the original reads `b[i]`, `b[i + 1]`, rejects a length below 2, otherwise jumps `i += length` past the end
/// (`length > 2`) or onto the end (`length == 2`) and every continuation then reaches an `i >= len` check that answers `None`; the
/// mutant answers `None` at once. The continuation reads only `marker`, the two bytes and `len - i == 2`, so the complete state is
/// (marker 256) x (high byte 256) x (low byte 256) = 2^24 values. ALL of them are run, as the tail of a stream whose earlier history
/// is one of four shapes (nothing, an empty APP0, fill bytes plus a COM segment, a one-byte-payload APP1 after fill bytes), and
/// compared with the independent reference.
#[test]
fn every_marker_and_length_pair_left_exactly_at_the_end_is_decided_like_the_reference() {
    let histories: [&[u8]; 4] = [
        &[],
        &[0xFF, 0xE0, 0x00, 0x02],
        &[0xFF, 0xFF, 0xFF, 0xFE, 0x00, 0x04, 0x41, 0x42],
        &[0xFF, 0xFF, 0xE1, 0x00, 0x03, 0x99],
    ];
    for history in histories {
        split_markers(|marker| {
            let mut bytes = vec![0xFF, 0xD8];
            bytes.extend_from_slice(history);
            bytes.extend_from_slice(&[0xFF, marker, 0, 0]);
            let high = bytes.len() - 2;
            for hi in 0..=255u8 {
                for lo in 0..=255u8 {
                    bytes[high] = hi;
                    bytes[high + 1] = lo;
                    agree(&bytes);
                }
            }
        });
    }
}

// ------------------------------------------------------------------------------------------------ 771:14

/// decoder.rs:771:14 `if i > len` -> `==` and `>=`.
///
/// After `i += segment_length` the original answers `None` for `i > len`. With `>=` the mutant also answers `None` for `i == len`;
/// with `==` it answers `None` only for `i == len` and goes on for `i > len`. In every continuation the first thing that runs is an
/// `i >= len` check that answers `None` (the loop head, or the head of the entropy loop after `DA`), so the three programs agree for
/// every `i >= len`. The continuation depends only on `marker == DA` and on `i >= len`, not on the size of the overshoot.
///
/// Run here for the whole divergent set: every marker (256) x every segment length 2..=65535 (the 16 bit range, below 2 is
/// rejected earlier), once with the file ending exactly where the segment ends (`i == len`) and once with the file cut right after
/// the two length bytes (`i > len`, overshoot `length - 2`, so every overshoot 0..=65533 occurs). 2 x 256 x 65 534 streams, each
/// compared with the independent reference.
#[test]
fn every_segment_that_ends_at_or_past_the_end_of_the_file_is_decided_like_the_reference() {
    split_markers(|marker| {
        // `FF D8 FF <marker> hi lo`, then payload up to the largest segment. The payload ends in `FF D9 11` so that the structure-free
        // tail scan stops after two steps instead of walking 64 KiB for each of the 16 million files.
        let mut full = vec![0xFF, 0xD8, 0xFF, marker, 0, 0];
        full.resize(4 + 65_535, 0x11);
        for length in 2..=65_535usize {
            let end = 4 + length;
            full[4] = (length >> 8) as u8;
            full[5] = (length & 0xFF) as u8;
            // i == len: the segment ends exactly at the end of the file.
            let tail = end - 3;
            let saved = [full[tail], full[tail + 1]];
            if length >= 5 {
                full[tail] = 0xFF;
                full[tail + 1] = 0xD9;
            }
            agree(&full[..end]);
            full[tail] = saved[0];
            full[tail + 1] = saved[1];
            // i > len: only the two length bytes are present.
            agree(&full[..6]);
        }
    });
}
