//! Red tests for W4-11e: `wic::scaler` (C# `Pixels/WicPixelScaler.cs`, baseline d92d6aae). Port of the `WicPixelScalerTests` class
//! of `tests/PhotoReview.Imaging.Tests/Pixels/Wp06BridgeSplitTests.cs` (7 tests; `Resize_EqualsWpfScaleTransform` has no Rust
//! twin: WPF `TransformedBitmap` is dropped, and the golden `fnv` column of `wic-scale-resize` replaces it) plus the golden
//! tests over `tests/golden/wic-scale-resize.json` (15 cases; inputs in `tests/golden-inputs/wic-scale-resize.json`).
//!
//! Every test fails through `todo!()` until the porter implements the bodies. Native tests (they call WIC) are
//! `#[ignore = "native: needs WIC"]`; run them with `-- --ignored` on Windows. Pure tests run in the normal suite.
#![cfg(windows)]

use pr_imaging::pixel_memory::NativePixelMemory;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::wic::scaler::{ResizePlan, ScaleError, plan_resize, resize};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};

// ---------------------------------------------------------------------------------------------------------------------
// Helpers (twins of C# `PixelAssert.CreatePattern`; golden pattern of `tools/oracle-cs/Commands/W4-11e.cs`)
// ---------------------------------------------------------------------------------------------------------------------

/// Byte `i` (row-major, no padding) = `(i * 131 + seed * 17 + i / 251) mod 256`, 64-bit arithmetic.
fn pattern_byte(index: u64, seed: u64) -> u8 {
    ((index * 131 + seed * 17 + index / 251) % 256) as u8
}

/// The golden pattern. With `alpha` (Pbgra32 only) byte 3 of pixel (x, y) is `255 - x * 255 / max(width - 1, 1)` and the colour
/// bytes are clamped to it.
fn pattern(width: i32, height: i32, layout: PixelLayout, seed: u64, alpha: bool) -> PixelBuffer {
    let mut px = PixelBuffer::allocate(width, height, layout).expect("valid geometry");
    for (index, byte) in px.bytes_mut().expect("not disposed").iter_mut().enumerate() {
        *byte = pattern_byte(index as u64, seed);
    }
    if alpha && layout == PixelLayout::Pbgra32 {
        for y in 0..height {
            let row = px.get_row_mut(y).expect("not disposed");
            for x in 0..width as usize {
                let a = (255 - (x as i64 * 255 / i64::from((width - 1).max(1)))) as u8;
                row[x * 4 + 3] = a;
                for c in 0..3 {
                    row[x * 4 + c] = row[x * 4 + c].min(a);
                }
            }
        }
    }
    px
}

/// The meaningful bytes (B, G, R of Bgr32; all four of Pbgra32) of every pixel, row-major. Byte 3 of Bgr32 is WIC-defined.
fn meaningful_bytes(px: &PixelBuffer) -> Vec<u8> {
    let channels = if px.layout() == PixelLayout::Pbgra32 { 4 } else { 3 };
    let mut out = Vec::new();
    for y in 0..px.height() {
        let row = px.get_row(y).expect("not disposed");
        for x in 0..px.width() as usize {
            out.extend_from_slice(&row[x * 4..x * 4 + channels]);
        }
    }
    out
}

/// FNV-1a 64 over `bytes`, 16 lowercase hex digits (the golden `fnv` column).
fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for &byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211);
    }
    format!("{hash:016x}")
}

fn layout_of(text: &str) -> PixelLayout {
    if text == "pbgra32" {
        PixelLayout::Pbgra32
    } else {
        PixelLayout::Bgr32
    }
}

fn layout_name(layout: PixelLayout) -> &'static str {
    if layout == PixelLayout::Pbgra32 {
        "pbgra32"
    } else {
        "bgr32"
    }
}

/// `tests/golden-inputs/wic-scale-resize.json` case by name (the golden outputs do not repeat the inputs).
fn input_case(name: &str) -> serde_json::Value {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("wic-scale-resize.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .expect("cases array")
        .iter()
        .find(|c| c["name"].as_str() == Some(name))
        .cloned()
        .unwrap_or_else(|| panic!("golden input {name} missing"))
}

// ---------------------------------------------------------------------------------------------------------------------
// Pure tests
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn plan_resize_checks_run_in_contract_order() {
    // Contract note 2 and the "New Rust tests" list: disposed before width, width before height, byte_count last.
    // C#: WicPixelScaler.Resize check order (ObjectDisposedException, ArgumentOutOfRangeException x2, ArgumentException).
    assert_eq!(
        plan_resize(true, 4, 4, 64, 0, 0),
        Err(ScaleError::Disposed),
        "disposed before width"
    );
    assert_eq!(
        plan_resize(false, 4, 4, 64, 0, 2),
        Err(ScaleError::WidthOutOfRange { width: 0 })
    );
    assert_eq!(
        plan_resize(false, 4, 4, 64, 0, 0),
        Err(ScaleError::WidthOutOfRange { width: 0 }),
        "width before height"
    );
    assert_eq!(
        plan_resize(false, 4, 4, 64, 2, 0),
        Err(ScaleError::HeightOutOfRange { height: 0 })
    );
    assert_eq!(
        plan_resize(false, 4, 4, 64, -3, 2),
        Err(ScaleError::WidthOutOfRange { width: -3 })
    );

    let too_large = i64::from(u32::MAX) + 1;
    assert_eq!(
        plan_resize(false, 4, 4, too_large, 2, 2),
        Err(ScaleError::TooLarge { byte_count: too_large }),
        "byte_count > u32::MAX"
    );
    assert_eq!(
        plan_resize(false, 4, 4, too_large, 0, 2),
        Err(ScaleError::WidthOutOfRange { width: 0 }),
        "byte_count is checked after the width"
    );
}

#[test]
fn plan_resize_same_size_is_copy_else_wic() {
    // Contract note 2: width == source.width && height == source.height is a COPY; everything else, enlargement included, is WIC.
    assert_eq!(plan_resize(false, 17, 9, 17 * 9 * 4, 17, 9), Ok(ResizePlan::Copy));
    assert_eq!(
        plan_resize(false, 17, 9, 17 * 9 * 4, 17, 5),
        Ok(ResizePlan::Wic),
        "one axis changed goes through WIC"
    );
    assert_eq!(
        plan_resize(false, 8, 6, 8 * 6 * 4, 24, 18),
        Ok(ResizePlan::Wic),
        "enlargement is allowed"
    );
}

#[test]
fn resize_same_size_is_a_copy() {
    // C#: WicPixelScalerTests.Resize_SameSize_IsACopy: an independent buffer with the same bytes (no WIC call).
    let source = pattern(17, 9, PixelLayout::Bgr32, 2, false);

    let copy = resize(&source, 17, 9).expect("same size is a copy");

    assert_eq!(
        copy.bytes().expect("not disposed"),
        source.bytes().expect("not disposed")
    );
    assert_ne!(
        copy.address().expect("not disposed"),
        source.address().expect("not disposed")
    );
    assert_ne!(copy.id(), source.id());
}

#[test]
fn resize_invalid_arguments_throw_and_leak_nothing() {
    // C#: WicPixelScalerTests.Resize_InvalidArguments_Throw_AndLeakNothing (the ArgumentNullException half has no Rust twin).
    let before = NativePixelMemory::live_count();
    let mut source = PixelBuffer::allocate(4, 4, PixelLayout::Bgr32).expect("valid geometry");
    assert!(matches!(
        resize(&source, 0, 2),
        Err(ScaleError::WidthOutOfRange { width: 0 })
    ));
    assert!(matches!(
        resize(&source, 2, 0),
        Err(ScaleError::HeightOutOfRange { height: 0 })
    ));
    source.dispose();
    assert!(matches!(resize(&source, 2, 2), Err(ScaleError::Disposed)));
    drop(source);
    assert_eq!(NativePixelMemory::live_count(), before, "no buffer is left behind");
}

#[test]
fn golden_wic_scale_resize_checks_and_copy_plan_are_pure() {
    // Golden wic-scale-resize: the throw rows are pure decisions of plan_resize; a sameAsSource row is a Copy plan, every other
    // non-throwing row a Wic plan. Inputs come from tests/golden-inputs/wic-scale-resize.json.
    let golden = golden::load("wic-scale-resize");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = input_case(name);
        let layout = layout_of(str_of(&input, "layout"));
        let (sw, sh) = (i64_of(&input, "srcWidth") as i32, i64_of(&input, "srcHeight") as i32);
        let (w, h) = (i64_of(&input, "width") as i32, i64_of(&input, "height") as i32);
        let byte_count = i64::from(sw) * i64::from(sh) * 4;
        let disposed = input["disposeFirst"].as_bool().unwrap_or(false);

        let plan = plan_resize(disposed, sw, sh, byte_count, w, h);
        match case["throws"].as_str() {
            Some("ArgumentOutOfRangeException") if w < 1 => {
                assert_eq!(plan, Err(ScaleError::WidthOutOfRange { width: w }), "{name}")
            }
            Some("ArgumentOutOfRangeException") => {
                assert_eq!(plan, Err(ScaleError::HeightOutOfRange { height: h }), "{name}")
            }
            Some("ObjectDisposedException") => assert_eq!(plan, Err(ScaleError::Disposed), "{name}"),
            Some(other) => panic!("{name}: unexpected golden throw {other}"),
            None => {
                let expected = if bool_of(case, "sameAsSource") {
                    ResizePlan::Copy
                } else {
                    ResizePlan::Wic
                };
                assert_eq!(plan, Ok(expected), "{name} (layout {})", layout_name(layout));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Native tests (WIC). Each one is ignored until run on Windows with `-- --ignored`.
// ---------------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs WIC"]
fn resize_produces_requested_size_and_layout() {
    // C#: WicPixelScalerTests.Resize_ProducesRequestedSize_AndLayout (three rows).
    for (layout, sw, sh, tw, th) in [
        (PixelLayout::Bgr32, 64, 48, 16, 12),
        (PixelLayout::Pbgra32, 64, 48, 33, 7),
        (PixelLayout::Bgr32, 5, 3, 1, 1),
    ] {
        let source = pattern(sw, sh, layout, 4, false);

        let scaled = resize(&source, tw, th).expect("resize");

        assert_eq!((scaled.width(), scaled.height()), (tw, th));
        assert_eq!(scaled.layout(), layout);
        assert!(!source.is_disposed(), "the source is never disposed");
    }
}

#[test]
#[ignore = "native: needs WIC"]
fn resize_flat_colour_stays_flat() {
    // C#: WicPixelScalerTests.Resize_FlatColour_StaysFlat: no edge bleed, no channel swap.
    let mut source = PixelBuffer::allocate(40, 30, PixelLayout::Bgr32).expect("valid geometry");
    for y in 0..30 {
        let row = source.get_row_mut(y).expect("not disposed");
        for x in 0..40 {
            row[x * 4] = 10;
            row[x * 4 + 1] = 120;
            row[x * 4 + 2] = 230;
            row[x * 4 + 3] = 255;
        }
    }

    let scaled = resize(&source, 13, 9).expect("resize");

    for y in 0..9 {
        let row = scaled.get_row(y).expect("not disposed");
        for x in 0..13 {
            assert_eq!(row[x * 4], 10, "B at ({x},{y})");
            assert_eq!(row[x * 4 + 1], 120, "G at ({x},{y})");
            assert_eq!(row[x * 4 + 2], 230, "R at ({x},{y})");
        }
    }
}

#[test]
#[ignore = "native: needs WIC"]
fn resize_pbgra_keeps_alpha() {
    // C#: WicPixelScalerTests.Resize_Pbgra_KeepsAlpha: premultiplied alpha survives the Fant scaler.
    let mut source = PixelBuffer::allocate(8, 8, PixelLayout::Pbgra32).expect("valid geometry");
    for y in 0..8 {
        let row = source.get_row_mut(y).expect("not disposed");
        for x in 0..8 {
            row[x * 4] = 50;
            row[x * 4 + 1] = 25;
            row[x * 4 + 2] = 10;
            row[x * 4 + 3] = 100;
        }
    }

    let scaled = resize(&source, 2, 2).expect("resize");

    let px = scaled.get_row(0).expect("not disposed");
    assert_eq!(px[3], 100);
    assert_eq!(px[0], 50);
}

#[test]
#[ignore = "native: needs WIC"]
fn resize_holds_only_the_result() {
    // C#: WicPixelScalerTests.Resize_HoldsOnlyTheResult: live pixel count +1 while the result lives, then back.
    let source = pattern(32, 32, PixelLayout::Bgr32, 1, false);
    let before = NativePixelMemory::live_count();

    let scaled = resize(&source, 8, 8).expect("resize");
    assert_eq!(NativePixelMemory::live_count(), before + 1);

    drop(scaled);
    assert_eq!(NativePixelMemory::live_count(), before);
}

#[test]
#[ignore = "native: needs WIC"]
fn golden_wic_scale_resize_matches_csharp() {
    // Golden wic-scale-resize.json (15 cases). Successful rows: size, layout, FNV of the meaningful bytes, same-as-source
    // (bytes equal) and distinct buffer. The C# `Resize_EqualsWpfScaleTransform` is replaced by the fnv column.
    let golden = golden::load("wic-scale-resize");
    for case in &golden.cases {
        let name = str_of(case, "name");
        if case["throws"].as_str().is_some() {
            continue; // pure rows are checked by golden_wic_scale_resize_checks_and_copy_plan_are_pure
        }
        let input = input_case(name);
        let layout = layout_of(str_of(&input, "layout"));
        let seed = input["seed"].as_u64().unwrap_or(0);
        let alpha = input["alpha"].as_bool().unwrap_or(false);
        let source = pattern(
            i64_of(&input, "srcWidth") as i32,
            i64_of(&input, "srcHeight") as i32,
            layout,
            seed,
            alpha,
        );
        let source_fnv = fnv1a_hex(&meaningful_bytes(&source));

        let scaled = resize(&source, i64_of(&input, "width") as i32, i64_of(&input, "height") as i32)
            .unwrap_or_else(|e| panic!("{name}: {e}"));

        assert_eq!(
            (scaled.width() as i64, scaled.height() as i64),
            (i64_of(case, "width"), i64_of(case, "height")),
            "{name}"
        );
        assert_eq!(layout_name(scaled.layout()), str_of(case, "layout"), "{name}: layout");
        assert_eq!(
            fnv1a_hex(&meaningful_bytes(&scaled)),
            str_of(case, "fnv"),
            "{name}: fnv"
        );
        let same_as_source = scaled.width() == source.width()
            && scaled.height() == source.height()
            && fnv1a_hex(&meaningful_bytes(&scaled)) == source_fnv;
        assert_eq!(same_as_source, bool_of(case, "sameAsSource"), "{name}: sameAsSource");
        assert_eq!(
            scaled.address().expect("not disposed") != source.address().expect("not disposed"),
            bool_of(case, "distinctBuffer"),
            "{name}: distinctBuffer"
        );
    }
}
