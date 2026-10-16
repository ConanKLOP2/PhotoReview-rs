//! Red golden tests for W5-13a (`contracts/W5-13a.md`, "Golden package"): `turbojpeg-native-types` (pure) and
//! `turbojpeg-native-handle` (pure, no DLL) run in the default test run; `turbojpeg-native-decode` (146 rows) needs the
//! pinned `native/x64/turbojpeg.dll` and is `#[ignore = "native"]`. Goldens are written by `tools/oracle.ps1`
//! (oracle-cs at d92d6aae); porters never edit them. A failing row is a behaviour difference from C#.
//!
//! Known gaps, deliberate: the `stride` and `buffer_length` fields of `output-buffer` are the decoder's arithmetic
//! (`calculate_output_buffer`, W5-14b), not this unit's; they are not asserted here.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pr_test_support::golden;
use pr_turbojpeg::native::{
    DLL_NAME, NativeError, TjInit, TjParam, TjPixelFormat, TjScalingFactor, TurboJpegHandle, TurboJpegLibrary,
    wrap_init_result,
};
use serde_json::Value;

// ---- shared helpers ----

const ALL_PIXEL_FORMATS: [TjPixelFormat; 12] = [
    TjPixelFormat::Rgb,
    TjPixelFormat::Bgr,
    TjPixelFormat::Rgbx,
    TjPixelFormat::Bgrx,
    TjPixelFormat::Xbgr,
    TjPixelFormat::Xrgb,
    TjPixelFormat::Gray,
    TjPixelFormat::Rgba,
    TjPixelFormat::Bgra,
    TjPixelFormat::Abgr,
    TjPixelFormat::Argb,
    TjPixelFormat::Cmyk,
];

/// Maps a golden `(name, value)` member list onto the Rust variant list of the same enum.
fn assert_members(type_name: &str, rust: &[(&str, i32)], golden_case: &Value) {
    let members = golden_case["members"]
        .as_array()
        .unwrap_or_else(|| panic!("{type_name}: members"));
    assert_eq!(rust.len(), members.len(), "{type_name}: member count");
    for ((rust_name, rust_value), member) in rust.iter().zip(members) {
        let name = golden::str_of(member, "name");
        let value = golden::i64_of(member, "value");
        assert_eq!(*rust_name, name, "{type_name}: member order and names");
        assert_eq!(i64::from(*rust_value), value, "{type_name}.{name}");
    }
}

fn scaling(value: &Value) -> TjScalingFactor {
    let pair = value.as_array().expect("factor is [num, denom]");
    let num = i32::try_from(pair[0].as_i64().expect("num")).expect("num fits i32");
    let denom = i32::try_from(pair[1].as_i64().expect("denom")).expect("denom fits i32");
    TjScalingFactor::new(num, denom)
}

fn hash_of<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn csharp_type(error: &NativeError) -> &'static str {
    match error {
        NativeError::DllNotFound { .. } => "DllNotFoundException",
        NativeError::BadImageFormat { .. } => "BadImageFormatException",
        NativeError::EntryPointNotFound { .. } => "EntryPointNotFoundException",
        NativeError::InvalidOperation { .. } => "InvalidOperationException",
        NativeError::Disposed { .. } => "ObjectDisposedException",
        NativeError::InvalidArgument { .. } => "ArgumentException",
    }
}

/// Golden `dispose_errors` entry: `null` for success, `{type, message}` for the failure the seam raised.
fn assert_dispose_result(result: Result<(), NativeError>, expected: &Value) {
    if expected.is_null() {
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    } else {
        let error = result.expect_err("expected the destroy failure");
        assert_eq!(csharp_type(&error), golden::str_of(expected, "type"));
        assert_eq!(error.message(), golden::str_of(expected, "message"));
    }
}

// ---- turbojpeg-native-types ----

// Golden kinds: `enum` (TjInit, TjParam, TjPixelFormat) and `dll-name`.
#[test]
fn tj_enums_match_the_libjpeg_turbo_values() {
    let golden = golden::load("turbojpeg-native-types");
    let mut enums = 0;
    for case in golden.cases.iter().filter(|c| golden::str_of(c, "kind") == "enum") {
        let rust: Vec<(&str, i32)> = match golden::str_of(case, "type") {
            "TjInit" => vec![
                ("Compress", TjInit::Compress.as_i32()),
                ("Decompress", TjInit::Decompress.as_i32()),
                ("Transform", TjInit::Transform.as_i32()),
            ],
            "TjParam" => vec![
                ("StopOnWarning", TjParam::StopOnWarning.as_i32()),
                ("BottomUp", TjParam::BottomUp.as_i32()),
                ("NoRealloc", TjParam::NoRealloc.as_i32()),
                ("Quality", TjParam::Quality.as_i32()),
                ("Subsamp", TjParam::Subsamp.as_i32()),
                ("JpegWidth", TjParam::JpegWidth.as_i32()),
                ("JpegHeight", TjParam::JpegHeight.as_i32()),
                ("Precision", TjParam::Precision.as_i32()),
                ("Colorspace", TjParam::Colorspace.as_i32()),
                ("FastUpsample", TjParam::FastUpsample.as_i32()),
                ("FastDct", TjParam::FastDct.as_i32()),
                ("Optimize", TjParam::Optimize.as_i32()),
                ("Progressive", TjParam::Progressive.as_i32()),
                ("ScanLimit", TjParam::ScanLimit.as_i32()),
                ("Arithmetic", TjParam::Arithmetic.as_i32()),
                ("Lossless", TjParam::Lossless.as_i32()),
                ("LosslessPsv", TjParam::LosslessPsv.as_i32()),
                ("LosslessPt", TjParam::LosslessPt.as_i32()),
                ("RestartBlocks", TjParam::RestartBlocks.as_i32()),
                ("RestartRows", TjParam::RestartRows.as_i32()),
                ("XDensity", TjParam::XDensity.as_i32()),
                ("YDensity", TjParam::YDensity.as_i32()),
                ("DensityUnits", TjParam::DensityUnits.as_i32()),
            ],
            "TjPixelFormat" => ALL_PIXEL_FORMATS
                .iter()
                .map(|format| (pixel_format_name(*format), format.as_i32()))
                .collect(),
            other => panic!("unknown enum in golden: {other}"),
        };
        assert_members(golden::str_of(case, "type"), &rust, case);
        enums += 1;
    }
    assert_eq!(enums, 3, "TjInit, TjParam and TjPixelFormat are all in the golden");
}

fn pixel_format_name(format: TjPixelFormat) -> &'static str {
    match format {
        TjPixelFormat::Rgb => "Rgb",
        TjPixelFormat::Bgr => "Bgr",
        TjPixelFormat::Rgbx => "Rgbx",
        TjPixelFormat::Bgrx => "Bgrx",
        TjPixelFormat::Xbgr => "Xbgr",
        TjPixelFormat::Xrgb => "Xrgb",
        TjPixelFormat::Gray => "Gray",
        TjPixelFormat::Rgba => "Rgba",
        TjPixelFormat::Bgra => "Bgra",
        TjPixelFormat::Abgr => "Abgr",
        TjPixelFormat::Argb => "Argb",
        TjPixelFormat::Cmyk => "Cmyk",
    }
}

// Golden kind `dll-name`.
#[test]
fn native_dll_name_matches_the_golden() {
    let golden = golden::load("turbojpeg-native-types");
    let case = golden
        .cases
        .iter()
        .find(|c| golden::str_of(c, "kind") == "dll-name")
        .expect("dll-name case");
    assert_eq!(DLL_NAME, golden::str_of(case, "dll_name"));
}

// Golden kind `factor-equality`: equality on both fields, non-normalised; `hash_equal` is only meaningful for equal
// factors (contract trap T12: C# hashing is seeded per process).
#[test]
fn tj_scaling_factor_equality_golden() {
    let golden = golden::load("turbojpeg-native-types");
    let mut rows = 0;
    for case in golden
        .cases
        .iter()
        .filter(|c| golden::str_of(c, "kind") == "factor-equality")
    {
        let a = scaling(&case["a"]);
        let b = scaling(&case["b"]);
        let name = golden::str_of(case, "name");
        let equal = golden::bool_of(case, "equals");

        assert_eq!(a == b, equal, "{name}: ==");
        assert_eq!(a.eq(&b), equal, "{name}: Equals(object)");
        assert_eq!(golden::bool_of(case, "op_equal"), equal, "{name}: golden self-check");
        assert_eq!(
            golden::bool_of(case, "op_not_equal"),
            !equal,
            "{name}: golden self-check"
        );
        assert_eq!(
            golden::bool_of(case, "equals_object"),
            equal,
            "{name}: golden self-check"
        );
        if equal {
            assert_eq!(hash_of(&a), hash_of(&b), "{name}: equal factors hash alike");
            assert!(
                golden::bool_of(case, "hash_equal"),
                "{name}: C# hashed equal factors alike"
            );
        }
        rows += 1;
    }
    assert_eq!(rows, 11);
}

// Golden kind `factor-to-string`: C# invariant `ToString()` is `"{num}/{denom}"` with plain integer formatting.
#[test]
fn tj_scaling_factor_to_string_golden() {
    let golden = golden::load("turbojpeg-native-types");
    for case in golden
        .cases
        .iter()
        .filter(|c| golden::str_of(c, "kind") == "factor-to-string")
    {
        let factor = scaling(&case["factor"]);
        assert_eq!(
            factor.to_string(),
            golden::str_of(case, "text"),
            "{}",
            golden::str_of(case, "name")
        );
    }
}

// Golden kind `output-buffer`: `scaled(dimension)` must reproduce the decoder's `width`/`height` (`TJSCALED` in 64-bit).
// Rows with an `exception` carry the scaled size as "{w}x{h}" in the message (C# CalculateOutputBuffer's overflow
// check); the test parses that and compares both sides.
#[test]
fn scaled_reproduces_the_output_buffer_golden() {
    let golden = golden::load("turbojpeg-native-types");
    let mut rows = 0;
    for case in golden
        .cases
        .iter()
        .filter(|c| golden::str_of(c, "kind") == "output-buffer")
    {
        let name = golden::str_of(case, "name");
        let factor = scaling(&case["factor"]);
        let width_in = i32::try_from(golden::i64_of(case, "width_in")).expect("width_in fits i32");
        let height_in = i32::try_from(golden::i64_of(case, "height_in")).expect("height_in fits i32");

        if let Some(exception) = case.get("exception") {
            let message = golden::str_of(exception, "message");
            let (w, h) = parse_scaled_size(message);
            assert_eq!(
                factor.scaled(width_in),
                Some(w),
                "{name}: width from the overflow message"
            );
            assert_eq!(
                factor.scaled(height_in),
                Some(h),
                "{name}: height from the overflow message"
            );
        } else {
            let width = golden::i64_of(case, "width");
            let height = golden::i64_of(case, "height");
            assert_eq!(factor.scaled(width_in), Some(width), "{name}: width");
            assert_eq!(factor.scaled(height_in), Some(height), "{name}: height");
        }
        rows += 1;
    }
    assert_eq!(rows, 24, "output-buffer rows of the 46-case golden");
}

/// `"...: 536870912x1 (2147483648 bytes)."` -> `(536870912, 1)`.
fn parse_scaled_size(message: &str) -> (i64, i64) {
    let after_colon = message.rsplit(": ").next().expect("message has a size");
    let size = after_colon.split(' ').next().expect("size token");
    let (w, h) = size.split_once('x').unwrap_or_else(|| panic!("no WxH in {message}"));
    (w.parse().expect("w"), h.parse().expect("h"))
}

// ---- turbojpeg-native-handle (pure: the seam destroy, no DLL) ----

// Golden kinds: `public-ctor` (0/-1 only), `seam` (recording or throwing destroy), `set-invalid`, `wrap` (init result).
#[test]
fn turbojpeg_native_handle_golden() {
    let golden = golden::load("turbojpeg-native-handle");
    for case in &golden.cases {
        let name = golden::str_of(case, "name");
        let raw = isize::try_from(golden::i64_of(case, "raw")).expect("raw fits isize");
        let owns = golden::bool_of(case, "owns");
        match golden::str_of(case, "kind") {
            "public-ctor" => {
                let mut handle = TurboJpegHandle::from_raw(raw, owns);
                assert_eq!(handle.is_invalid(), golden::bool_of(case, "is_invalid"), "{name}");
                assert_dispose_result(handle.close(), &case["dispose_error"]);
                assert_eq!(handle.is_closed(), golden::bool_of(case, "is_closed_after"), "{name}");
            }
            kind @ ("seam" | "set-invalid") => {
                let throws = golden::str_of(case, "destroy") == "throw";
                let log = Arc::new(Mutex::new(Vec::<i64>::new()));
                let sink = Arc::clone(&log);
                let mut handle = TurboJpegHandle::with_destroy(raw, owns, move |value| {
                    sink.lock().unwrap().push(i64::try_from(value).expect("value fits i64"));
                    if throws {
                        Err(NativeError::InvalidOperation {
                            message: "destroy failed".to_owned(),
                        })
                    } else {
                        Ok(())
                    }
                });
                assert_eq!(
                    handle.is_invalid(),
                    golden::bool_of(case, "is_invalid"),
                    "{name}: is_invalid"
                );
                assert_eq!(
                    handle.is_closed(),
                    golden::bool_of(case, "is_closed_before"),
                    "{name}: closed before"
                );
                if kind == "set-invalid" {
                    handle.set_handle_as_invalid();
                    assert_eq!(
                        handle.is_closed(),
                        golden::bool_of(case, "is_closed_after_set_invalid"),
                        "{name}: closed after set_handle_as_invalid"
                    );
                }
                for expected in case["dispose_errors"].as_array().expect("dispose_errors") {
                    assert_dispose_result(handle.close(), expected);
                }
                let destroyed: Vec<i64> = log.lock().unwrap().clone();
                let golden_destroyed: Vec<i64> = case["destroyed"]
                    .as_array()
                    .expect("destroyed")
                    .iter()
                    .map(|v| v.as_i64().expect("destroyed value"))
                    .collect();
                assert_eq!(destroyed, golden_destroyed, "{name}: destroy calls");
                assert_eq!(
                    handle.is_closed(),
                    golden::bool_of(case, "is_closed_after"),
                    "{name}: closed after"
                );
                assert_eq!(
                    handle.raw_value() as i64,
                    golden::i64_of(case, "raw_after"),
                    "{name}: raw after close"
                );
            }
            "wrap" => {
                if let Some(exception) = case.get("exception") {
                    match wrap_init_result(raw) {
                        Err(error) => {
                            assert_eq!(csharp_type(&error), golden::str_of(exception, "type"), "{name}");
                            assert_eq!(error.message(), golden::str_of(exception, "message"), "{name}");
                        }
                        Ok(_) => panic!("{name}: a NULL init result must throw"),
                    }
                } else {
                    let mut handle = wrap_init_result(raw).expect("wrap");
                    assert_eq!(handle.is_invalid(), golden::bool_of(case, "is_invalid"), "{name}");
                    assert_eq!(handle.is_closed(), golden::bool_of(case, "is_closed"), "{name}");
                    assert_eq!(handle.raw_value() as i64, golden::i64_of(case, "raw_value"), "{name}");
                    handle.set_handle_as_invalid();
                    assert_eq!(
                        handle.is_closed(),
                        golden::bool_of(case, "is_closed_after_set_invalid"),
                        "{name}"
                    );
                }
            }
            other => panic!("{name}: unknown handle kind {other}"),
        }
    }
}

// ---- turbojpeg-native-decode (native, 146 rows) ----

/// Lower-case hex SHA-256 (FIPS 180-4), test-local because the workspace has no SHA-256 crate (D33). Used for the DLL
/// pin and the `pixels_sha256` of every row.
fn sha256_hex(bytes: &[u8]) -> String {
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
    let mut message = bytes.to_vec();
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());
    for block in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
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
        for (state, add) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *state = state.wrapping_add(add);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

// Test of the test helper: FIPS 180-4 vectors, so a wrong hash cannot hide a pixel difference.
#[test]
fn sha256_hex_matches_the_fips_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

fn tj_param_named(name: &str) -> TjParam {
    match name {
        "Subsamp" => TjParam::Subsamp,
        "JpegWidth" => TjParam::JpegWidth,
        "JpegHeight" => TjParam::JpegHeight,
        "Precision" => TjParam::Precision,
        "Colorspace" => TjParam::Colorspace,
        "Progressive" => TjParam::Progressive,
        "Arithmetic" => TjParam::Arithmetic,
        "Lossless" => TjParam::Lossless,
        other => panic!("unknown TjParam in golden params: {other}"),
    }
}

fn pixel_format_of(value: i64) -> TjPixelFormat {
    ALL_PIXEL_FORMATS
        .into_iter()
        .find(|format| i64::from(format.as_i32()) == value)
        .unwrap_or_else(|| panic!("no TjPixelFormat with value {value}"))
}

/// The exact native sequence of C# `TurboJpegDecoder.Decode` (contract "Semantics" 10), row by row against the golden.
#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn turbojpeg_native_decode_golden() {
    let golden = golden::load("turbojpeg-native-decode");
    let dll_path: PathBuf = golden::repo_root().join("native").join("x64").join(DLL_NAME);
    assert!(
        dll_path.is_file(),
        "missing {}: run tools/fetch-native.ps1 first",
        dll_path.display()
    );
    let library = TurboJpegLibrary::load(&dll_path).unwrap_or_else(|e| panic!("load: {e}"));
    // Contract T10: pixel hashes are compared only when the loaded DLL is the pinned build; the rows carry its SHA-256.
    let dll_sha = sha256_hex(&std::fs::read(&dll_path).expect("read dll"));
    if golden
        .cases
        .first()
        .is_some_and(|c| golden::str_of(c, "turbojpeg_dll_sha256") != dll_sha)
    {
        eprintln!("turbojpeg.dll SHA-256 {dll_sha} is not the golden's pinned build: pixel hashes are not compared");
    }

    let fixtures = golden::repo_root().join("tests").join("fixtures");
    let mut checked = 0usize;
    for case in &golden.cases {
        let name = golden::str_of(case, "name");
        let bytes = std::fs::read(fixtures.join(golden::str_of(case, "fixture"))).expect("fixture");
        assert_eq!(
            bytes.len() as i64,
            golden::i64_of(case, "fixture_length"),
            "{name}: fixture length"
        );

        let mut handle = library.create_decompressor().expect("tj3Init(decompress)");
        // ScanLimit 500 is set unconditionally; StopOnWarning only for strict rows.
        let _ = handle.set(TjParam::ScanLimit, 500).expect("tj3Set(ScanLimit)");
        if golden::bool_of(case, "strict") {
            assert_eq!(
                handle.set(TjParam::StopOnWarning, 1).expect("set"),
                golden::i64_of(case, "stop_on_warning_result") as i32,
                "{name}"
            );
        }

        let header = handle.decompress_header(&bytes).expect("tj3DecompressHeader");
        assert_eq!(
            i64::from(header),
            golden::i64_of(case, "header_result"),
            "{name}: header result"
        );
        if case.get("header_error").is_some() {
            assert_eq!(
                handle.error_message().as_deref(),
                Some(golden::str_of(case, "header_error")),
                "{name}"
            );
            checked += 1;
            continue; // header-only rows end here
        }
        if let Some(params) = case.get("params").and_then(Value::as_object) {
            for (param, value) in params {
                let got = handle.get(tj_param_named(param)).expect("tj3Get");
                assert_eq!(i64::from(got), value.as_i64().expect("param value"), "{name}: {param}");
            }
        }

        // Declared size above 64 MiB: the C# oracle stops after the header (no scaling call, no decode) and records the
        // 1/1 size only. The row has no `factor`/`scale_result` keys, so it is checked before they are read.
        if case["decompress_skipped"].as_bool().unwrap_or(false) {
            let effective = scaling(&case["effective_factor"]);
            let jpeg_width = handle.get(TjParam::JpegWidth).expect("width");
            let jpeg_height = handle.get(TjParam::JpegHeight).expect("height");
            assert_eq!(
                effective.scaled(jpeg_width),
                Some(golden::i64_of(case, "scaled_width")),
                "{name}: scaled width"
            );
            assert_eq!(
                effective.scaled(jpeg_height),
                Some(golden::i64_of(case, "scaled_height")),
                "{name}: scaled height"
            );
            checked += 1;
            continue;
        }

        let effective = scaling(&case["effective_factor"]);
        // Rows without `factor` are decodes at 1/1: the C# decoder does not call tj3SetScalingFactor for them.
        let applied = if case.get("factor").is_some() {
            let requested = scaling(&case["factor"]);
            let scale_result = handle.set_scaling_factor(requested).expect("tj3SetScalingFactor");
            assert_eq!(
                i64::from(scale_result),
                golden::i64_of(case, "scale_result"),
                "{name}: scale result"
            );
            if scale_result == 0 {
                requested
            } else {
                TjScalingFactor::ONE
            }
        } else {
            TjScalingFactor::ONE
        };
        assert_eq!(applied, effective, "{name}: effective factor");

        let jpeg_width = i64::from(handle.get(TjParam::JpegWidth).expect("width"));
        let jpeg_height = i64::from(handle.get(TjParam::JpegHeight).expect("height"));
        let width = effective.scaled(jpeg_width as i32).expect("denom > 0");
        let height = effective.scaled(jpeg_height as i32).expect("denom > 0");
        assert_eq!(width, golden::i64_of(case, "scaled_width"), "{name}: scaled width");
        assert_eq!(height, golden::i64_of(case, "scaled_height"), "{name}: scaled height");

        let format = pixel_format_of(golden::i64_of(case, "format_value"));
        let pitch = golden::i64_of(case, "pitch");
        let mut pixels = vec![0xCDu8; usize::try_from(pitch * height).expect("buffer size")];
        let golden_result = golden::i64_of(case, "decompress_result");
        match handle.decompress8(&bytes, &mut pixels, pitch as i32, format) {
            Ok(code) => {
                assert_eq!(i64::from(code), golden_result, "{name}: decompress result");
                if code != 0 {
                    assert_eq!(
                        handle.error_message().as_deref(),
                        Some(golden::str_of(case, "decompress_error")),
                        "{name}: decompress error text"
                    );
                }
                if code == 0 && golden::str_of(case, "turbojpeg_dll_sha256") == dll_sha {
                    assert_eq!(
                        sha256_hex(&pixels),
                        golden::str_of(case, "pixels_sha256"),
                        "{name}: pixel hash"
                    );
                }
            }
            // Contract T11: a refusal by the Rust guard where C# would get its own native error text (-1) is allowed.
            Err(NativeError::InvalidArgument { .. }) if golden_result != 0 => {}
            Err(error) => panic!("{name}: unexpected error {error:?}"),
        }
        checked += 1;
    }
    assert_eq!(
        checked,
        golden.cases.len(),
        "every golden row is checked or skipped by rule"
    );
}
