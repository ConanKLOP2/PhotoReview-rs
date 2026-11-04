//! N1 run-proof (mutation close-out 2026-11-05) for the three `diagnostics/perf.rs` `sha256` survivors that M5 classified as
//! equivalent by reading only: `152:30` (`ch = (e & f) ^ (!e & g)`, `^` -> `|`), `159:31` and `159:41` (the two `^` of
//! `maj = (a & b) ^ (a & c) ^ (b & c)`, each -> `|`).
//!
//! Two runs, both meant to PASS with the original code and with each hand-applied mutant:
//! 1. The bit-level truth tables. `&`, `|`, `^` and `!` act on each bit position independently, so the mutated and the
//!    original expression agree on every `u32` iff they agree on the 8 combinations of (x, y, z) bits; the test runs those
//!    8 combinations as all-zero / all-one words (so every bit position sees every combination at once) and 200 000
//!    pseudo-random word triples besides.
//! 2. A differential run of the real `sha256` (reached through the public `path_id`, which is the first 8 bytes of the
//!    digest) against an independent textbook SHA-256 written here with DIFFERENT but equivalent formulas
//!    (`ch = g ^ (e & (f ^ g))`, `maj = (a & b) | (c & (a | b))`), for every path length from 0 to 300 characters after the
//!    drive root (all padding cases of 1 to 6 blocks) and 3 000 further pseudo-random upper-case paths.
//!
//! This is a proof of equivalence, not a killer.

#![cfg(windows)]

use pr_core::diagnostics::perf::path_id;

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

/// Textbook FIPS 180-4 SHA-256 with the alternative (equivalent) forms of Ch and Maj.
fn reference_sha256(data: &[u8]) -> [u8; 32] {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for t in 0..16 {
            w[t] = u32::from_be_bytes([block[4 * t], block[4 * t + 1], block[4 * t + 2], block[4 * t + 3]]);
        }
        for t in 16..64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = s1.wrapping_add(w[t - 7]).wrapping_add(s0).wrapping_add(w[t - 16]);
        }
        let mut v = state;
        for t in 0..64 {
            let [a, b, c, _, e, f, g, h] = v;
            let ch = g ^ (e & (f ^ g));
            let maj = (a & b) | (c & (a | b));
            let t1 = h
                .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add(ch)
                .wrapping_add(K[t])
                .wrapping_add(w[t]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22)).wrapping_add(maj);
            v = [t1.wrapping_add(t2), a, b, c, v[3].wrapping_add(t1), e, f, g];
        }
        for (s, x) in state.iter_mut().zip(v) {
            *s = s.wrapping_add(x);
        }
    }
    let mut out = [0u8; 32];
    for (i, word) in state.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn expected_id(path: &str) -> String {
    let digest = reference_sha256(path.as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// xorshift64*: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn word(&mut self) -> u32 {
        (self.next() >> 16) as u32
    }
}

#[test]
fn the_reference_sha256_matches_the_known_vectors() {
    let hex = |d: [u8; 32]| d.iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(
        hex(reference_sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(reference_sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex(reference_sha256(
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        )),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

#[test]
fn ch_and_maj_agree_with_their_or_forms_on_every_bit_combination() {
    let word = |bit: bool| if bit { u32::MAX } else { 0 };
    let mut combinations = 0;
    for x in [false, true] {
        for y in [false, true] {
            for z in [false, true] {
                let (e, f, g) = (word(x), word(y), word(z));
                // 152:30  (e & f) ^ (!e & g)  vs  (e & f) | (!e & g)
                assert_eq!((e & f) ^ (!e & g), (e & f) | (!e & g), "ch {x} {y} {z}");
                // 159:31  (a & b) ^ (a & c) ^ (b & c)  vs  (a & b) | ((a & c) ^ (b & c))   (`^` binds tighter than `|`)
                // 159:41  (a & b) ^ (a & c) ^ (b & c)  vs  ((a & b) ^ (a & c)) | (b & c)
                let (a, b, c) = (e, f, g);
                let original = (a & b) ^ (a & c) ^ (b & c);
                assert_eq!(original, (a & b) | ((a & c) ^ (b & c)), "maj mutant 159:31 {x} {y} {z}");
                assert_eq!(original, ((a & b) ^ (a & c)) | (b & c), "maj mutant 159:41 {x} {y} {z}");
                combinations += 1;
            }
        }
    }
    assert_eq!(combinations, 8);
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..200_000 {
        let (a, b, c) = (rng.word(), rng.word(), rng.word());
        assert_eq!((a & b) ^ (!a & c), (a & b) | (!a & c));
        let original = (a & b) ^ (a & c) ^ (b & c);
        assert_eq!(original, (a & b) | ((a & c) ^ (b & c)));
        assert_eq!(original, ((a & b) ^ (a & c)) | (b & c));
    }
}

#[test]
fn path_id_matches_the_independent_sha256_for_every_length_up_to_300() {
    let mut rng = Rng(0x0123_4567_89AB_CDEF);
    for n in 0..=300usize {
        let tail: String = (0..n).map(|_| char::from(b'A' + (rng.next() % 26) as u8)).collect();
        let path = format!("C:{}{}", char::from(92), tail);
        assert_eq!(
            path_id(&path),
            expected_id(&path),
            "path of {n} characters after the drive root"
        );
    }
}

#[test]
fn path_id_matches_the_independent_sha256_for_3000_random_paths() {
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-";
    for i in 0..3000 {
        let n = (rng.next() % 400) as usize;
        let tail: String = (0..n)
            .map(|_| char::from(alphabet[(rng.next() % alphabet.len() as u64) as usize]))
            .collect();
        let path = format!("C:{}{}", char::from(92), tail);
        assert_eq!(path_id(&path), expected_id(&path), "random path #{i} of {n} characters");
    }
}
