//! Red tests for W5-14a (`pr_turbojpeg::area_resampler`): port of `PhotoReview.Imaging.TurboJpeg/PixelAreaResampler.cs`
//! (C# baseline d92d6aae), tests from `tests/PhotoReview.Imaging.Tests/Pixels/PixelAreaResamplerTests.cs`, plus the
//! golden-driven test over `tests/golden/pixel-area-resample.json` (inputs in `tests/golden-inputs/`).
//!
//! Every test here compiles against the stub and fails through `todo!()` until the porter fills the bodies.
//!
//! Documented skips (not portable to Rust, see the contract W5-14a): the null half of
//! `Resize_RejectsBadArguments_AndDoesNotLeak` (`ArgumentNullException` has no Rust twin: `src` is a reference), and the
//! golden case `error-null-source` (`rust: false` in the inputs). The `PixelAssert`/`Fill` helpers of the C# test support
//! are re-implemented locally (the pr-test-support crate has no pixel helpers).

use std::collections::HashMap;

use pr_imaging::pixels::{BYTES_PER_PIXEL, PixelBuffer, PixelBufferError, PixelLayout};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use pr_turbojpeg::area_resampler::{AreaWeights, ResizeError, build_area_weights, resize};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------------
// Local helpers (twins of C# `PixelAreaResamplerTests.Fill`, `Reference`, `PixelAssert`)
// ---------------------------------------------------------------------------------------------------------------------

/// Byte offset of output pixel `x`, channel `channel` (0 = B, 1 = G, 2 = R, 3 = fourth byte) in a Bgr32 row.
fn at(x: i32, channel: usize) -> usize {
    x as usize * BYTES_PER_PIXEL as usize + channel
}

/// A Bgr32 buffer filled with `value(x, y, channel)`, fourth byte `alpha` (C# `Fill`, which uses alpha 0xFF).
fn fill_with_alpha(width: i32, height: i32, alpha: u8, value: impl Fn(i32, i32, i32) -> u8) -> PixelBuffer {
    let mut buffer = PixelBuffer::allocate(width, height, PixelLayout::Bgr32).expect("valid geometry");
    for y in 0..height {
        let row = buffer.get_row_mut(y).expect("not disposed");
        for x in 0..width {
            for channel in 0..3 {
                row[at(x, channel as usize)] = value(x, y, channel);
            }
            row[at(x, 3)] = alpha;
        }
    }
    buffer
}

fn fill(width: i32, height: i32, value: impl Fn(i32, i32, i32) -> u8) -> PixelBuffer {
    fill_with_alpha(width, height, 0xFF, value)
}

/// FNV-1a 64 over all output bytes, row after row, `width * 4` bytes per row (the golden `fnv1a64`).
fn fnv1a64(buffer: &PixelBuffer) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let row_bytes = buffer.width() as usize * BYTES_PER_PIXEL as usize;
    for y in 0..buffer.height() {
        for &byte in &buffer.get_row(y).expect("not disposed")[..row_bytes] {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// All bytes of the buffer, row after row, `width * 4` per row.
fn snapshot(buffer: &PixelBuffer) -> Vec<u8> {
    let row_bytes = buffer.width() as usize * BYTES_PER_PIXEL as usize;
    (0..buffer.height())
        .flat_map(|y| buffer.get_row(y).expect("not disposed")[..row_bytes].to_vec())
        .collect()
}

/// Line port of the C# brute-force reference: every channel is the rounded mean over the exact overlap areas.
fn reference(src: &PixelBuffer, tw: i32, th: i32, x: i32, y: i32, channel: usize) -> u8 {
    let (sw, sh) = (i64::from(src.width()), i64::from(src.height()));
    let (tw, th) = (i64::from(tw), i64::from(th));
    let (x, y) = (i64::from(x), i64::from(y));
    let mut sum: i64 = 0;
    for sy in 0..sh {
        let oy = ((y + 1) * sh).min((sy + 1) * th) - (y * sh).max(sy * th);
        if oy <= 0 {
            continue;
        }
        let row = src.get_row(sy as i32).expect("not disposed");
        for sx in 0..sw {
            let ox = ((x + 1) * sw).min((sx + 1) * tw) - (x * sw).max(sx * tw);
            if ox <= 0 {
                continue;
            }
            sum += i64::from(row[sx as usize * 4 + channel]) * ox * oy;
        }
    }
    let total = sw * sh;
    u8::try_from((sum + total / 2) / total).expect("a mean of bytes fits in u8")
}

/// Panics with the first differing row (C# `PixelAssert.Equal`: geometry, layout and every byte).
fn assert_same_pixels(expected: &PixelBuffer, actual: &PixelBuffer) {
    assert_eq!(
        (expected.width(), expected.height(), expected.layout()),
        (actual.width(), actual.height(), actual.layout()),
        "geometry differs"
    );
    assert_eq!(snapshot(expected), snapshot(actual), "pixel bytes differ");
}

fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn alpha_all_ff(buffer: &PixelBuffer) -> bool {
    (0..buffer.height()).all(|y| {
        buffer
            .get_row(y)
            .expect("not disposed")
            .iter()
            .skip(3)
            .step_by(4)
            .all(|&alpha| alpha == 0xFF)
    })
}

/// Input cases of `tests/golden-inputs/<name>.json`, keyed by case name.
fn input_cases(name: &str) -> HashMap<String, Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .iter()
        .map(|case| (str_of(case, "name").to_owned(), case.clone()))
        .collect()
}

fn input_for<'a>(inputs: &'a HashMap<String, Value>, name: &str) -> &'a Value {
    inputs
        .get(name)
        .unwrap_or_else(|| panic!("no golden input named '{name}'"))
}

fn i32_of(case: &Value, key: &str) -> i32 {
    i32::try_from(i64_of(case, key)).unwrap_or_else(|_| panic!("'{key}' does not fit i32: {case}"))
}

fn flag(case: &Value, key: &str) -> bool {
    case.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// Fails once, listing every mismatching golden case.
fn check_all(label: &str, failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{label}: {} mismatching case(s)\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Line port of the C# `W514aBuild` (tools/oracle-cs/Commands/W5-14a.cs): the source buffer described by a golden input.
/// Kinds: `formula`, `const`, `lcg`, `checker`, `bytes`; `src_alpha` (default 255) is the fourth byte of every pixel.
fn build_source(input: &Value) -> PixelBuffer {
    let sw = i32_of(input, "sw");
    let sh = i32_of(input, "sh");
    let layout = if input.get("layout").and_then(Value::as_str) == Some("Pbgra32") {
        PixelLayout::Pbgra32
    } else {
        PixelLayout::Bgr32
    };
    let alpha =
        u8::try_from(input.get("src_alpha").and_then(Value::as_i64).unwrap_or(255)).expect("src_alpha is a byte");
    let spec = &input["src"];
    let kind = str_of(spec, "kind");
    let mut buffer = PixelBuffer::allocate(sw, sh, layout).expect("valid golden geometry");
    let mut state: u32 = if kind == "lcg" { i64_of(spec, "seed") as u32 } else { 0 };
    let values: Vec<u8> = if kind == "bytes" {
        spec["values"]
            .as_array()
            .expect("bytes kind has values")
            .iter()
            .map(|v| u8::try_from(v.as_i64().expect("byte value")).expect("byte value"))
            .collect()
    } else {
        Vec::new()
    };
    for y in 0..sh {
        let row = buffer.get_row_mut(y).expect("not disposed");
        for x in 0..sw {
            for channel in 0..3usize {
                let value: u8 = match kind {
                    "formula" => {
                        let (x, y, ch) = (i64::from(x), i64::from(y), channel as i64);
                        ((x * i64_of(spec, "a")
                            + y * i64_of(spec, "b")
                            + ch * i64_of(spec, "k")
                            + x * y * i64_of(spec, "m")
                            + i64_of(spec, "seed"))
                            & 255) as u8
                    }
                    "const" => i64_of(spec, ["b", "g", "r"][channel]) as u8,
                    "lcg" => {
                        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        (state >> 24) as u8
                    }
                    "checker" => {
                        if (x + y) & 1 == 0 {
                            spec.get("lo").and_then(Value::as_i64).unwrap_or(0) as u8
                        } else {
                            spec.get("hi").and_then(Value::as_i64).unwrap_or(255) as u8
                        }
                    }
                    "bytes" => values[(y as usize * sw as usize + x as usize) * 3 + channel],
                    other => panic!("unknown src kind {other}"),
                };
                row[at(x, channel)] = value;
            }
            row[at(x, 3)] = alpha;
        }
    }
    buffer
}

/// Checks one golden case (C# `PixelAreaResample` output fields) and records every mismatch.
fn check_golden_case(name: &str, input: &Value, expected: &Value, failures: &mut Vec<String>) {
    let mut src = build_source(input);
    let before = fnv1a64(&src);
    if flag(input, "dispose_src") {
        src.dispose();
    }
    let result = resize(&src, i32_of(input, "tw"), i32_of(input, "th"));

    let actual_error = match &result {
        Ok(_) => None,
        Err(error) => Some(error.dotnet_type_name()),
    };
    let expected_error = expected["error"].as_str();
    if actual_error != expected_error {
        failures.push(format!(
            "{name}: error expected {expected_error:?}, got {actual_error:?}"
        ));
    }
    if bool_of(expected, "ok") != result.is_ok() {
        failures.push(format!("{name}: ok expected {}", bool_of(expected, "ok")));
    }
    let Ok(dst) = result else {
        return;
    };

    let mut mismatch = |field: &str, want: String, got: String| {
        if want != got {
            failures.push(format!("{name}: {field} expected {want}, got {got}"));
        }
    };
    mismatch(
        "width",
        i64_of(expected, "width").to_string(),
        i64::from(dst.width()).to_string(),
    );
    mismatch(
        "height",
        i64_of(expected, "height").to_string(),
        i64::from(dst.height()).to_string(),
    );
    mismatch(
        "layout",
        str_of(expected, "layout").to_owned(),
        format!("{:?}", dst.layout()),
    );
    mismatch(
        "fnv1a64",
        str_of(expected, "fnv1a64").to_owned(),
        format!("{:016x}", fnv1a64(&dst)),
    );
    let row_bytes = dst.width() as usize * BYTES_PER_PIXEL as usize;
    let first = hex_upper(&dst.get_row(0).expect("not disposed")[..4]);
    let last_row = dst.get_row(dst.height() - 1).expect("not disposed");
    let last = hex_upper(&last_row[row_bytes - 4..row_bytes]);
    mismatch("first_pixel", str_of(expected, "first_pixel").to_owned(), first);
    mismatch("last_pixel", str_of(expected, "last_pixel").to_owned(), last);
    mismatch(
        "alpha_all_ff",
        bool_of(expected, "alpha_all_ff").to_string(),
        alpha_all_ff(&dst).to_string(),
    );
    mismatch(
        "src_unchanged",
        bool_of(expected, "src_unchanged").to_string(),
        (fnv1a64(&src) == before).to_string(),
    );
    mismatch(
        "new_buffer",
        bool_of(expected, "new_buffer").to_string(),
        (dst.id() != src.id()).to_string(),
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported C# tests (PixelAreaResamplerTests.cs)
// ---------------------------------------------------------------------------------------------------------------------

// C#: PixelAreaResamplerTests.Resize_MatchesBruteForceAreaAverage (theory, 9 InlineData tuples (sw, sh, tw, th))
#[test]
fn resize_matches_brute_force_area_average() {
    const CASES: [(i32, i32, i32, i32); 9] = [
        (3, 1, 2, 1),
        (2, 2, 1, 1),
        (7, 5, 3, 2),
        (10, 10, 10, 4),
        (10, 10, 4, 10),
        (31, 17, 30, 16),
        (64, 48, 13, 11),
        (1, 9, 1, 3),
        (9, 1, 4, 1),
    ];
    for (sw, sh, tw, th) in CASES {
        let src = fill(sw, sh, |x, y, c| ((x * 37 + y * 91 + c * 53 + x * y * 7) % 256) as u8);
        let dst = resize(&src, tw, th).unwrap_or_else(|e| panic!("{sw}x{sh} -> {tw}x{th}: {e}"));

        assert_eq!((dst.width(), dst.height()), (tw, th), "size for {:?}", (sw, sh, tw, th));
        assert_eq!(dst.layout(), PixelLayout::Bgr32, "layout for {:?}", (sw, sh, tw, th));
        for y in 0..th {
            let row = dst.get_row(y).expect("not disposed");
            for x in 0..tw {
                for c in 0..3 {
                    assert_eq!(
                        row[at(x, c)],
                        reference(&src, tw, th, x, y, c),
                        "{:?} pixel ({x},{y}) channel {c}",
                        (sw, sh, tw, th)
                    );
                }
                assert_eq!(row[at(x, 3)], 0xFF, "alpha for {:?} pixel ({x},{y})", (sw, sh, tw, th));
            }
        }
    }
}

// C#: PixelAreaResamplerTests.Resize_HandComputedThreeToTwo
#[test]
fn resize_hand_computed_three_to_two() {
    // Row of three pixels 0, 90, 255 -> two: (2*0 + 1*90) / 3 = 30 and (1*90 + 2*255) / 3 = 200.
    let src = fill(3, 1, |x, _, _| match x {
        0 => 0,
        1 => 90,
        _ => 255,
    });

    let dst = resize(&src, 2, 1).expect("valid arguments");

    assert_eq!(
        &dst.get_row(0).expect("not disposed")[..8],
        &[30, 30, 30, 0xFF, 200, 200, 200, 0xFF]
    );
}

// C#: PixelAreaResamplerTests.Resize_SameSize_IsAnIndependentCopy (Assert.NotSame becomes a new-id check)
#[test]
fn resize_same_size_is_an_independent_copy() {
    let mut src = fill(5, 4, |x, y, c| (x + y * 10 + c) as u8);

    let dst = resize(&src, 5, 4).expect("valid arguments");

    assert_ne!(dst.id(), src.id(), "the copy must be a new buffer");
    assert_same_pixels(&src, &dst);
    src.get_row_mut(0).expect("not disposed")[0] ^= 0xFF;
    assert_ne!(
        src.get_row(0).expect("not disposed")[0],
        dst.get_row(0).expect("not disposed")[0],
        "flipping the source must not change the copy"
    );
}

// C#: PixelAreaResamplerTests.Resize_NeverUpscales_AnAxisKeepsTheSourceSize
#[test]
fn resize_never_upscales_an_axis_keeps_the_source_size() {
    let src = fill(8, 8, |x, y, _| (x * y) as u8);

    let dst = resize(&src, 20, 4).expect("valid arguments");

    assert_eq!((dst.width(), dst.height()), (8, 4));
}

// C#: PixelAreaResamplerTests.Resize_ConstantImage_StaysConstant_AndLeavesTheSourceUntouched
#[test]
fn resize_constant_image_stays_constant_and_leaves_the_source_untouched() {
    let src = fill(101, 67, |_, _, c| (200 + c) as u8);
    let before = snapshot(&src);

    let dst = resize(&src, 33, 29).expect("valid arguments");

    for y in 0..dst.height() {
        let row = dst.get_row(y).expect("not disposed");
        for x in 0..dst.width() {
            assert_eq!(
                &row[at(x, 0)..at(x, 0) + 4],
                &[200, 201, 202, 0xFF],
                "pixel ({x},{y}) of the constant image"
            );
        }
    }
    assert_eq!(snapshot(&src), before, "the source must not be modified");
}

// C#: PixelAreaResamplerTests.Resize_RejectsBadArguments_AndDoesNotLeak (the null half has no Rust twin)
#[test]
fn resize_rejects_bad_arguments_and_does_not_leak() {
    let pbgra = PixelBuffer::allocate(2, 2, PixelLayout::Pbgra32).expect("valid geometry");
    let bgr = PixelBuffer::allocate(2, 2, PixelLayout::Bgr32).expect("valid geometry");

    assert_eq!(
        resize(&pbgra, 1, 1).err(),
        Some(ResizeError::SourceNotBgr32 {
            layout: PixelLayout::Pbgra32
        })
    );
    assert_eq!(
        resize(&bgr, 0, 1).err(),
        Some(ResizeError::TargetWidthOutOfRange { width: 0 })
    );
    assert_eq!(
        resize(&bgr, 1, 0).err(),
        Some(ResizeError::TargetHeightOutOfRange { height: 0 })
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden-driven test (tests/golden/pixel-area-resample.json, 78 cases)
// ---------------------------------------------------------------------------------------------------------------------

// Golden `pixel-area-resample` (78 cases, inputs in `golden-inputs/pixel-area-resample.json`). Skips the `slow` case
// (see `golden_pixel_area_resample_slow_12m_pixel_case`) and `rust: false` (the null source has no Rust twin).
#[test]
fn golden_pixel_area_resample() {
    let inputs = input_cases("pixel-area-resample");
    let golden = golden::load("pixel-area-resample");
    let mut failures = Vec::new();
    for expected in &golden.cases {
        let name = str_of(expected, "name");
        let input = input_for(&inputs, name);
        if input.get("rust").and_then(Value::as_bool) == Some(false) || flag(input, "slow") {
            continue;
        }
        check_golden_case(name, input, expected, &mut failures);
    }
    check_all("golden pixel-area-resample", &failures);
}

// Golden `pixel-area-resample`, the 12 M pixel case (4032x3024 -> 1008x756). Run with `--release --include-ignored`.
#[test]
#[ignore = "slow: 12M pixel case, run with --release"]
fn golden_pixel_area_resample_slow_12m_pixel_case() {
    let inputs = input_cases("pixel-area-resample");
    let golden = golden::load("pixel-area-resample");
    let mut failures = Vec::new();
    for expected in &golden.cases {
        let name = str_of(expected, "name");
        let input = input_for(&inputs, name);
        if !flag(input, "slow") {
            continue;
        }
        check_golden_case(name, input, expected, &mut failures);
    }
    check_all("golden pixel-area-resample (slow)", &failures);
}

// ---------------------------------------------------------------------------------------------------------------------
// New tests that pin what the C# only implies (not in the C# suite; see the contract W5-14a, section 4)
// ---------------------------------------------------------------------------------------------------------------------

/// Deterministic per-column table invariants: every column's overlaps sum to `sw`, `first` and `offset` are monotone,
/// every overlap is in `1..=tw`.
#[test]
fn weights_of_every_column_sum_to_the_source_width() {
    for (sw, tw) in [
        (100_000usize, 33_333usize),
        (7, 3),
        (5, 5),
        (3, 2),
        (1, 1),
        (10, 4),
        (9, 9),
    ] {
        let table: AreaWeights = build_area_weights(sw, tw);

        assert_eq!(table.first.len(), tw, "first length for ({sw}, {tw})");
        assert_eq!(table.offset.len(), tw + 1, "offset length for ({sw}, {tw})");
        assert_eq!(
            table.offset.last().copied(),
            Some(table.weights.len()),
            "offset[tw] must equal the weights length for ({sw}, {tw})"
        );
        for x in 0..tw {
            assert!(
                table.offset[x] <= table.offset[x + 1],
                "offset monotone at {x} for ({sw}, {tw})"
            );
            let weights = &table.weights[table.offset[x]..table.offset[x + 1]];
            assert_eq!(
                weights.iter().sum::<i64>(),
                sw as i64,
                "column {x} of ({sw}, {tw}) must cover the whole source width"
            );
            assert!(
                weights.iter().all(|&w| (1..=tw as i64).contains(&w)),
                "column {x} of ({sw}, {tw}) has an overlap outside 1..=tw: {weights:?}"
            );
            if x > 0 {
                assert!(
                    table.first[x - 1] <= table.first[x],
                    "first monotone at {x} for ({sw}, {tw})"
                );
            }
        }
    }
}

/// The hand-worked footprint of 3 source columns to 2: column 0 covers source 0 (overlap 2) and source 1 (overlap 1),
/// column 1 covers source 1 (overlap 1) and source 2 (overlap 2).
#[test]
fn build_area_weights_hand_table_three_to_two() {
    let table = build_area_weights(3, 2);

    assert_eq!(
        table,
        AreaWeights {
            first: vec![0, 1],
            offset: vec![0, 2, 4],
            weights: vec![2, 1, 1, 2],
        }
    );
}

/// Argument checks run in the C# order: layout first, then target width, then target height.
#[test]
fn error_order_layout_before_width_before_height() {
    let pbgra = PixelBuffer::allocate(2, 2, PixelLayout::Pbgra32).expect("valid geometry");
    let bgr = PixelBuffer::allocate(2, 2, PixelLayout::Bgr32).expect("valid geometry");

    assert_eq!(
        resize(&pbgra, 0, 0).err(),
        Some(ResizeError::SourceNotBgr32 {
            layout: PixelLayout::Pbgra32
        }),
        "layout is checked before the target size"
    );
    assert_eq!(
        resize(&bgr, 0, 0).err(),
        Some(ResizeError::TargetWidthOutOfRange { width: 0 }),
        "width is checked before height"
    );
    assert_eq!(
        resize(&bgr, 5, -1).err(),
        Some(ResizeError::TargetHeightOutOfRange { height: -1 })
    );
}

/// A disposed source is reported as `Buffer(Disposed)` on both paths, never a panic.
#[test]
fn disposed_source_is_reported_not_panicked() {
    let mut src = fill(4, 4, |x, y, c| (x + y + c) as u8);
    src.dispose();

    assert_eq!(
        resize(&src, 2, 2).err(),
        Some(ResizeError::Buffer(PixelBufferError::Disposed)),
        "shrink path"
    );
    assert_eq!(
        resize(&src, 4, 4).err(),
        Some(ResizeError::Buffer(PixelBufferError::Disposed)),
        "same-size path"
    );
}

/// The averaging path writes alpha 0xFF whatever the source says; the same-size copy keeps the source alpha as is.
#[test]
fn shrink_alpha_is_ff_but_copy_keeps_source_alpha() {
    for alpha in [0u8, 77] {
        let src = fill_with_alpha(4, 4, alpha, |x, y, c| (x * 9 + y * 5 + c) as u8);

        let shrunk = resize(&src, 2, 2).expect("valid arguments");
        assert!(
            alpha_all_ff(&shrunk),
            "shrink must write alpha 0xFF for source alpha {alpha}"
        );

        let copied = resize(&src, 4, 4).expect("valid arguments");
        assert!(
            copied
                .get_row(0)
                .expect("not disposed")
                .iter()
                .skip(3)
                .step_by(4)
                .all(|&a| a == alpha),
            "same-size copy must keep source alpha {alpha}"
        );
    }
}

/// Every result is a Bgr32 buffer with its own id, on the shrink, copy and clamped-oversized paths.
#[test]
fn result_is_always_bgr32_and_a_new_buffer() {
    let src = fill(6, 6, |x, y, c| (x + y + c) as u8);

    for (tw, th) in [(3, 3), (6, 6), (60, 60), (6, 2)] {
        let dst = resize(&src, tw, th).expect("valid arguments");
        assert_eq!(dst.layout(), PixelLayout::Bgr32, "layout for target ({tw}, {th})");
        assert_ne!(dst.id(), src.id(), "a new buffer for target ({tw}, {th})");
    }
}

/// Half-up rounding of `EmitRow`: `(sum + total / 2) / total` on single-row inputs of one output pixel.
#[test]
fn half_up_rounding_cases() {
    for (values, expected) in [
        (vec![0u8, 1], 1u8),
        (vec![1, 2], 2),
        (vec![0, 0, 1], 0),
        (vec![0, 1, 1], 1),
    ] {
        let src = fill(values.len() as i32, 1, |x, _, _| values[x as usize]);

        let dst = resize(&src, 1, 1).expect("valid arguments");

        assert_eq!(
            &dst.get_row(0).expect("not disposed")[..4],
            &[expected, expected, expected, 0xFF],
            "mean of {values:?}"
        );
    }
}

/// Deterministic xorshift64 (no `rand` crate), the generator state is passed explicitly.
fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// 300 random geometries with `sw, sh <= 40`, `tw <= sw`, `th <= sh`, random content: every channel equals the reference.
#[test]
fn random_sizes_match_brute_force() {
    let mut rng: u64 = 0x9E37_79B9_7F4A_7C15;
    for case in 0..300 {
        let sw = 1 + (xorshift64(&mut rng) % 40) as i32;
        let sh = 1 + (xorshift64(&mut rng) % 40) as i32;
        let tw = 1 + (xorshift64(&mut rng) % sw as u64) as i32;
        let th = 1 + (xorshift64(&mut rng) % sh as u64) as i32;
        let values: Vec<u8> = (0..sw * sh * 3).map(|_| xorshift64(&mut rng) as u8).collect();
        let src = fill(sw, sh, |x, y, c| values[((y * sw + x) * 3 + c) as usize]);

        let dst = resize(&src, tw, th).unwrap_or_else(|e| panic!("case {case}: {e}"));

        assert_eq!((dst.width(), dst.height()), (tw, th), "case {case} size");
        for y in 0..th {
            let row = dst.get_row(y).expect("not disposed");
            for x in 0..tw {
                for c in 0..3 {
                    assert_eq!(
                        row[at(x, c as usize)],
                        reference(&src, tw, th, x, y, c as usize),
                        "case {case} ({sw}x{sh} -> {tw}x{th}) pixel ({x},{y}) channel {c}"
                    );
                }
            }
        }
    }
}

/// A constant image stays constant for every target size on a small grid, for the values 0, 1, 254 and 255.
#[test]
fn constant_image_stays_constant_for_many_ratios() {
    for (sw, sh) in [(1, 1), (2, 3), (7, 5), (9, 9)] {
        for value in [0u8, 1, 254, 255] {
            let src = fill(sw, sh, |_, _, _| value);
            for tw in 1..=sw {
                for th in 1..=sh {
                    let dst = resize(&src, tw, th).expect("valid arguments");
                    for y in 0..th {
                        let row = dst.get_row(y).expect("not disposed");
                        for x in 0..tw {
                            assert_eq!(
                                &row[at(x, 0)..at(x, 0) + 4],
                                &[value, value, value, 0xFF],
                                "{sw}x{sh} value {value} -> {tw}x{th} pixel ({x},{y})"
                            );
                        }
                    }
                }
            }
        }
    }
}
