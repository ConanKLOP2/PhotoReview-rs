//! Red tests for W4-16a: the native block lifetime of `PixelBuffer` (C# `PixelBufferLifetimeTests`, `[Collection("GlobalState")]`),
//! observed through `NativePixelMemory::live_count`. The counter is process-wide, so this is its own integration test
//! binary (its own process) and every test takes `SERIAL` before it reads the counter.
//!
//! Also holds the "source is disposed" half of `PixelOpsOrientationTests.ApplyOrientation_NonIdentity_ReturnsNewBuffer_AndDisposesSource`
//! and the `source_disposed` field of the golden `pixel-ops` orient cases, which need the same counter.
//!
//! Each test compiles against the stub and fails through `todo!()` until the porter fills the bodies.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use pr_imaging::pixel_memory::NativePixelMemory;
use pr_imaging::pixels::{PixelBuffer, PixelBufferError, PixelLayout, apply_orientation, has_alpha};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

/// Serialises the tests of this binary: they share `NativePixelMemory::live_count`.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

fn live() -> i64 {
    NativePixelMemory::live_count()
}

fn create_pattern(width: i32, height: i32, layout: PixelLayout, seed: u64) -> PixelBuffer {
    let mut buffer = PixelBuffer::allocate(width, height, layout).expect("valid geometry");
    for (index, byte) in buffer.bytes_mut().expect("not disposed").iter_mut().enumerate() {
        *byte = ((index as u64 * 131 + seed * 17 + index as u64 / 251) % 256) as u8;
    }
    buffer
}

fn i32_of(case: &Value, key: &str) -> i32 {
    i32::try_from(i64_of(case, key)).unwrap_or_else(|_| panic!("'{key}' does not fit i32: {case}"))
}

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

// C#: PixelBufferLifetimeTests.AllocateAndDispose_ChangeLiveCountByExactlyOne
#[test]
fn allocate_and_dispose_change_live_count_by_exactly_one() {
    let _guard = serial();
    let before = live();

    let mut buffer = PixelBuffer::allocate(8, 8, PixelLayout::Bgr32).expect("valid size");
    assert_eq!(live(), before + 1);

    buffer.dispose();
    assert_eq!(live(), before);

    buffer.dispose();
    assert_eq!(live(), before);
}

// C#: PixelBufferLifetimeTests.InvalidAllocate_DoesNotLeak
// Every rejected size fails BEFORE the native allocation: the counter must not move, not even by one.
#[test]
fn invalid_allocate_does_not_leak() {
    let _guard = serial();
    let before = live();

    assert!(PixelBuffer::allocate(0, 4, PixelLayout::Bgr32).is_err());
    assert!(PixelBuffer::allocate(4, 0, PixelLayout::Pbgra32).is_err());
    assert!(PixelBuffer::allocate(i32::MIN, 4, PixelLayout::Bgr32).is_err());
    assert!(PixelBuffer::allocate(i32::MAX / 4 + 1, 1, PixelLayout::Bgr32).is_err());
    assert!(PixelBuffer::allocate(i32::MAX, 1, PixelLayout::Pbgra32).is_err());
    assert_eq!(PixelLayout::from_i32(9), Err(PixelBufferError::UnknownLayout(9)));

    assert_eq!(live(), before);
}

// C#: PixelBufferLifetimeTests.UnallocatedBlock_IsNotCounted_AndDisposesAsNoOp
#[test]
fn unallocated_block_is_not_counted_and_disposes_as_noop() {
    let _guard = serial();
    let before = live();
    let block = NativePixelMemory::unallocated();

    assert!(block.is_invalid());
    block.dispose();

    assert_eq!(live(), before);
}

// C#: PixelBufferLifetimeTests.Finalizer_ReleasesForgottenBuffers
// No GC in Rust: the buffers are freed by `Drop` at the end of their scope, so the count is checked right after it.
#[test]
fn finalizer_releases_forgotten_buffers() {
    let _guard = serial();
    let before = live();

    {
        let mut buffers = Vec::new();
        for _ in 0..16 {
            buffers.push(PixelBuffer::allocate(64, 64, PixelLayout::Pbgra32).expect("valid size"));
        }
        assert_eq!(live(), before + 16);
    }

    assert_eq!(live(), before);
}

// C#: PixelOpsOrientationTests.ApplyOrientation_NonIdentity_ReturnsNewBuffer_AndDisposesSource (the disposal half)
// The source is moved into `apply_orientation`. Non-identity: one new block, the source freed, so the count stays at
// `base + 1`; a source that was not released would show `base + 2`. Identity: the same buffer comes back, `base + 1`.
#[test]
fn apply_orientation_non_identity_disposes_source_observed_by_live_count() {
    let _guard = serial();
    for orientation in [2, 3, 4, 5, 6, 7, 8] {
        for (width, height) in [(1, 1), (4, 3), (33, 31)] {
            let base = live();
            let src = create_pattern(width, height, PixelLayout::Pbgra32, orientation as u64);
            assert_eq!(live(), base + 1, "source allocated");

            let out = apply_orientation(src, orientation).expect("valid orientation");

            assert_eq!(
                live(),
                base + 1,
                "orientation {orientation} {width}x{height}: source released, one result alive"
            );
            drop(out);
            assert_eq!(
                live(),
                base,
                "orientation {orientation} {width}x{height}: nothing left behind"
            );
        }
    }
}

// Golden `pixel-ops` kind `orient`: the `source_disposed` field, observed through the live counter (see above).
#[test]
fn golden_orient_source_disposed_matches_live_count() {
    let _guard = serial();
    let inputs = input_cases("pixel-ops");
    let golden = golden::load("pixel-ops");
    let mut failures = Vec::new();
    for case in golden.cases.iter().filter(|c| str_of(c, "kind") == "orient") {
        let name = str_of(case, "name");
        let input = inputs
            .get(name)
            .unwrap_or_else(|| panic!("no golden input named '{name}'"));
        let result = &case["result"];
        let layout = PixelLayout::from_i32(i32_of(input, "layout")).expect("golden layouts are 0 or 1");

        let base = live();
        let src = create_pattern(
            i32_of(input, "width"),
            i32_of(input, "height"),
            layout,
            i64_of(input, "seed") as u64,
        );
        let src_id = src.id();
        let out = match apply_orientation(src, i32_of(input, "orientation")) {
            Ok(out) => out,
            Err(error) => {
                failures.push(format!("{name}: unexpected error {}", error.error));
                continue;
            }
        };
        let same_instance = out.id() == src_id;
        // A moved source that was released leaves exactly the result alive; a leaked one adds a second block.
        let source_disposed = !same_instance && live() == base + 1;
        if source_disposed != bool_of(result, "source_disposed") {
            failures.push(format!(
                "{name}: source_disposed observed {source_disposed}, live {}",
                live() - base
            ));
        }
        drop(out);
        if live() != base {
            failures.push(format!("{name}: {} block(s) left alive", live() - base));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatching case(s)\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// An `Err` from a disposed buffer (alpha scan) must not leave a block behind either.
#[test]
fn disposed_alpha_check_does_not_leak() {
    let _guard = serial();
    let before = live();
    let mut buffer = PixelBuffer::allocate(2, 2, PixelLayout::Pbgra32).expect("valid size");
    buffer.dispose();
    assert_eq!(has_alpha(&buffer), Err(PixelBufferError::Disposed));
    drop(buffer);
    assert_eq!(live(), before);
}
