//! Red tests for `pr_imaging::memory_headroom` (W4-07b). Port of `PhotoReview.Imaging/Decoding/MemoryHeadroom.cs`
//! (C# baseline d92d6aae). Every test is expected to FAIL through `todo!()` until the porter fills the bodies.
//!
//! Each test names the C# test it ports in a `// C#:` comment. Tests that exercise the decoders' guards
//! (`TurboJpegGuardMutationTests`, `WicDirectInternalsMutationTests.EnsureOutputFits_*`, ...) and the production
//! `ReadGcMemoryInfo` reader (Rust twin in `pr-platform-win`) are NOT here; see the skip notes at the end.

use std::sync::atomic::{AtomicU32, Ordering};

use pr_imaging::memory_headroom::{
    GUARD_THRESHOLD_BYTES, MemoryInfo, MemoryInfoSource, OUTPUT_PEAK_FACTOR, has_headroom, has_headroom_from,
    output_has_headroom, output_has_headroom_from,
};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};

/// `Gb` of the C# theory (1024^3 bytes).
const GB: i64 = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------------------------------------------------
// Constants (published values; the golden `constants` row checks them too)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn guard_threshold_bytes_is_128_mib() {
    // C#: MemoryHeadroom.GuardThresholdBytes (128 MiB)
    assert_eq!(GUARD_THRESHOLD_BYTES, 134_217_728);
}

#[test]
fn output_peak_factor_is_two() {
    // C#: MemoryHeadroom.OutputPeakFactor
    assert_eq!(OUTPUT_PEAK_FACTOR, 2);
}

// ---------------------------------------------------------------------------------------------------------------------
// TurboJpegHardeningTests.OutputHasHeadroom_FollowsTheRamBudget (theory, 4 rows)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn output_has_headroom_follows_the_ram_budget() {
    // C#: MemoryHeadroom.OutputHasHeadroom via TurboJpegHardeningTests.OutputHasHeadroom_FollowsTheRamBudget
    // Row 1: 12000x9000 BGRX fits 8 GB free in 16 GB total.
    // Row 2: 16000x12000 BGRX fits 4 GB free in 16 GB total.
    // Row 3: 20000x20000 BGRX does NOT fit 14 GB load of 16 GB total.
    // Row 4: unknown total (0) never refuses.
    let rows: [(i64, i64, i64, bool); 4] = [
        (12_000 * 9_000 * 4, 16 * GB, 8 * GB, true),
        (16_000 * 12_000 * 4, 16 * GB, 4 * GB, true),
        (20_000 * 20_000 * 4, 16 * GB, 14 * GB, false),
        (12_000 * 9_000 * 4, 0, 0, true),
    ];
    for (index, (buffer, total, load, expected)) in rows.into_iter().enumerate() {
        assert_eq!(
            output_has_headroom(buffer, total, load),
            expected,
            "row {index}: buffer {buffer}, total {total}, load {load}"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// WicDirectInternalsMutationTests.OutputHasHeadroom_BufferWhosePeakOverflows_IsRefused
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn output_has_headroom_buffer_whose_peak_overflows_is_refused() {
    // C#: WicDirectInternalsMutationTests.OutputHasHeadroom_BufferWhosePeakOverflows_IsRefused (R16)
    assert!(!output_has_headroom(i64::MAX, 64 * GB, 0));
}

// ---------------------------------------------------------------------------------------------------------------------
// LibRawDecodeMemoryTests.HasHeadroom_BayerHundredMegapixel_IsRefusedWithTwelveHundredMegabytesAndAllowedWithFifteenHundred
// Only the two HasHeadroom asserts are ported here. The estimate is the literal the full test computes through
// DecodeMemoryGuard.EstimatePeakBytes (Wave 5), which is 1_300_000_000 for the 11600x8700 Bayer case.
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn has_headroom_bayer_hundred_megapixel_is_refused_with_twelve_hundred_megabytes_and_allowed_with_fifteen_hundred() {
    // C#: LibRawDecodeMemoryTests.HasHeadroom_BayerHundredMegapixel_IsRefusedWithTwelveHundredMegabytesAndAllowedWithFifteenHundred
    const ESTIMATE: i64 = 1_300_000_000;
    assert!(!has_headroom(ESTIMATE, 1_200_000_000, 0));
    assert!(has_headroom(ESTIMATE, 1_500_000_000, 0));
}

// ---------------------------------------------------------------------------------------------------------------------
// Rust-only extras (cheap, kept per contracts/README and W4-07b "Rust-only extras")
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn closure_implements_memory_info_source() {
    // Rust-only: the blanket impl lets a closure `|| MemoryInfo { .. }` be used as a `MemoryInfoSource`.
    let source = || MemoryInfo {
        total_available: 2000,
        load: 1000,
    };
    let as_dyn: &dyn MemoryInfoSource = &source;
    assert_eq!(
        as_dyn.read(),
        MemoryInfo {
            total_available: 2000,
            load: 1000
        }
    );
    assert!(has_headroom_from(&source, 1000), "closure source: exact fit must pass");
    assert!(
        !has_headroom_from(&source, 1001),
        "closure source: one over must be refused"
    );
}

#[test]
fn has_headroom_from_reads_the_source_exactly_once() {
    // Rust-only: `has_headroom_from` consults the source once per call, never twice.
    let reads = AtomicU32::new(0);
    let source = || {
        reads.fetch_add(1, Ordering::SeqCst);
        MemoryInfo {
            total_available: 2000,
            load: 1000,
        }
    };
    assert!(has_headroom_from(&source, 1000));
    assert_eq!(
        reads.load(Ordering::SeqCst),
        1,
        "has_headroom_from must read the source once"
    );
}

#[test]
fn output_has_headroom_from_reads_the_source_exactly_once() {
    // Rust-only: `output_has_headroom_from` consults the source once per call, never twice.
    let reads = AtomicU32::new(0);
    let source = || {
        reads.fetch_add(1, Ordering::SeqCst);
        MemoryInfo {
            total_available: 2000,
            load: 0,
        }
    };
    assert!(output_has_headroom_from(&source, 500));
    assert_eq!(
        reads.load(Ordering::SeqCst),
        1,
        "output_has_headroom_from must read the source once"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden: tests/golden/memory-headroom.json (constants row + 58 has/output cases)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn golden_memory_headroom_matches_csharp() {
    // C#: oracle-cs memory-headroom (constants row + has/output cases; D19: no OS read).
    let golden = golden::load("memory-headroom");
    assert_eq!(golden.cases.len(), 59, "58 cases plus the constants row");

    let mut checked = 0_usize;
    for case in &golden.cases {
        let name = str_of(case, "name");
        if name == "constants" {
            assert_eq!(
                i64_of(case, "guard_threshold_bytes"),
                GUARD_THRESHOLD_BYTES,
                "constants row"
            );
            assert_eq!(i64_of(case, "output_peak_factor"), OUTPUT_PEAK_FACTOR, "constants row");
            checked += 1;
            continue;
        }
        let value = i64_of(case, "value");
        let total = i64_of(case, "total");
        let load = i64_of(case, "load");
        let fits = bool_of(case, "fits");
        let actual = match str_of(case, "fn") {
            "has" => has_headroom(value, total, load),
            "output" => output_has_headroom(value, total, load),
            other => panic!("case {name}: unknown fn {other}"),
        };
        assert_eq!(actual, fits, "case {name}: value {value}, total {total}, load {load}");
        checked += 1;
    }
    assert_eq!(checked, golden.cases.len());
}

// ---------------------------------------------------------------------------------------------------------------------
// Skip notes (not ported here, see contracts/W4-07b.md)
// ---------------------------------------------------------------------------------------------------------------------
// - ReadGcMemoryInfo_ReportsThePhysicalBudgetAndANonNegativeLoad (DecodingHelpersMutationTests): tests the PRODUCTION
//   reader. Its Rust twin belongs to `pr-platform-win` next to `WindowsMemoryProbe`, `#[ignore = "native: GlobalMemoryStatusEx"]`.
//   Not in this unit's files; the lead assigns it.
// - TurboJpegGuardMutationTests, ErrorHandlingReviewDecodingTests, TurboJpegHardeningTests.HugeDeclaredSize_* (Wave 5),
//   WicDirectInternalsMutationTests.EnsureOutputFits_* (W4-11c): they exercise decoder guards, not MemoryHeadroom itself.
// - LibRawDecodeMemoryTests (full test): needs DecodeMemoryGuard.EstimatePeakBytes (Wave 5); the LibRaw unit keeps it.
