//! M5 killers (mutation close-out 2026-11-04) for the private `sha256` of `diagnostics/perf.rs`, reached through
//! the public `path_id`. Expected ids are the first 8 bytes of SHA-256 over the upper-cased UTF-8 path, computed
//! outside the code under test (Python `hashlib`). The lengths sit on the padding boundaries (55/56/63/64 bytes in
//! one block, 119/120/128 across blocks) so the padding loop, the bit-length trailer and the multi-block chaining
//! are all observable.

#![cfg(windows)]

use pr_core::diagnostics::perf::path_id;

#[test]
fn path_id_matches_independent_sha256_across_padding_boundaries() {
    // (number of 'a' after `C:\`, expected id)
    let cases: &[(usize, &str)] = &[
        (1, "9deb0d7965b929d8"),
        (2, "45b488df293c27e6"),
        (12, "7add06e2f07cf1b8"),
        (13, "f1a08598d91aab9d"),
        (52, "765b48e5ee0187f1"),
        (53, "08d546b5426043a7"),
        (54, "1d0b6b4b97e129c5"),
        (55, "6ce954f9bd2924f6"),
        (56, "0a925606396b233c"),
        (57, "75f33b1793b49810"),
        (60, "2d8167efaed51424"),
        (61, "f2fee77be636d085"),
        (62, "d50395b2320fa381"),
        (63, "0a986bc1ae104ba3"),
        (64, "3216b5518b39153d"),
        (65, "74a639b133ca54c6"),
        (66, "06fe35e57d3701be"),
        (67, "504b5845c04da86c"),
        (68, "ca1b82fb8073994a"),
        (100, "66ae87e0702158a1"),
        (117, "f0eeb92eb231a38f"),
        (118, "921172283fc8347b"),
        (119, "44a5e96e2384f4ac"),
        (120, "59b6f00f0751b99b"),
        (121, "e836a6aad6cc2b31"),
        (125, "55cb1256138c26e7"),
        (128, "48aae060831983b6"),
        (129, "2a0e161442b005a7"),
        (200, "66019e05ba8e5ec3"),
    ];
    for (n, expected) in cases {
        let path = format!("C:{}{}", char::from(92), "a".repeat(*n));
        assert_eq!(path_id(&path), *expected, "path of {} chars after the drive root", n);
    }
}
