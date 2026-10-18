//! Port of `PhotoReview.Benchmarking/BenchmarkProfiles.cs` (C# baseline d92d6aae, 60 lines): the registry of the 30
//! built-in benchmark profiles plus `Find`, `Runnable` and `IsNotImplemented`.
//!
//! Unit W7-01b. Contract: `contracts/W7-01b.md`. The profile record itself (`BenchmarkProfile`, `BenchmarkWorkload`) is
//! W7-01a's `crate::models`; this file only owns the data and the three lookups.
//!
//! The registry is built once (`OnceLock`) and handed out as `&'static` so `find` returns the same instance every time,
//! as the C# static list does. Order is the C# list order and is observable (`--benchmark-all`, the Benchmark window).

use std::sync::OnceLock;

use pr_core::model::enums::LoadingMode;
use pr_core::settings::perf_options::MEMORY_RESERVE_BYTES;
use pr_core::text::eq_ignore_case;

use crate::models::{BenchmarkProfile, BenchmarkWorkload, DEFAULT_ITERATIONS, DEFAULT_WARMUP_COUNT};

/// 4 GiB reserve of the `low-memory` profile.
const LOW_MEMORY_RESERVE_BYTES: i64 = 4 * 1024 * 1024 * 1024;

static ALL: OnceLock<Vec<BenchmarkProfile>> = OnceLock::new();
static RUNNABLE: OnceLock<Vec<BenchmarkProfile>> = OnceLock::new();

/// C# `BenchmarkProfiles.P(...)`: a profile with the record defaults (`disk_cache = false`, warm-up 1, 30 iterations,
/// not correctness-only, 2 GiB reserve). Argument order is the C# order; do not swap `next` and `previous`.
#[allow(clippy::too_many_arguments)] // mirrors the nine-argument C# helper; the table reads row by row in that order
fn p(
    id: &str,
    name: &str,
    description: &str,
    loading_mode: LoadingMode,
    workers: i32,
    next_window: i32,
    previous_window: i32,
    full_folder: bool,
    workload: BenchmarkWorkload,
) -> BenchmarkProfile {
    BenchmarkProfile {
        id: id.to_owned(),
        name: name.to_owned(),
        description: description.to_owned(),
        loading_mode,
        workers,
        next_window,
        previous_window,
        full_folder,
        memory_reserve_bytes: MEMORY_RESERVE_BYTES,
        disk_cache: false,
        detailed_logging: false,
        workload,
        warmup_count: DEFAULT_WARMUP_COUNT,
        iterations: DEFAULT_ITERATIONS,
        correctness_only: false,
    }
}

fn build_all() -> Vec<BenchmarkProfile> {
    use BenchmarkWorkload as W;
    use LoadingMode::{Fast, Original, Preview};

    vec![
        p(
            "instant-review",
            "Instant Review",
            "Prioritize the first image",
            Fast,
            2,
            4,
            1,
            false,
            W::FirstFrame,
        ),
        p(
            "fast-sequential",
            "Fast Sequential",
            "Continuous Next",
            Fast,
            8,
            32,
            8,
            false,
            W::Sequential,
        ),
        p(
            "fast-balanced",
            "Fast Balanced",
            "Balance Next and Previous",
            Fast,
            4,
            16,
            8,
            false,
            W::WarmNext,
        ),
        p(
            "fast-aggressive",
            "Fast Aggressive",
            "Deep preload with many workers",
            Fast,
            16,
            64,
            16,
            true,
            W::Preload,
        ),
        p(
            "preview-light",
            "Preview Light",
            "Small preview, low RAM",
            Preview,
            4,
            16,
            4,
            false,
            W::Sequential,
        ),
        p(
            "preview-balanced",
            "Preview Balanced",
            "Viewport-sized preview",
            Preview,
            8,
            32,
            8,
            false,
            W::WarmNext,
        ),
        p(
            "preview-quality",
            "Preview Quality",
            "Large preview, quality first",
            Preview,
            4,
            16,
            8,
            false,
            W::WarmNext,
        ),
        p(
            "preview-high-quality",
            "Preview High Quality",
            "Close to Original quality",
            Preview,
            8,
            16,
            8,
            false,
            W::FirstFrame,
        ),
        p(
            "no-preload-baseline",
            "No Preload Baseline",
            "Baseline without preload",
            Preview,
            1,
            0,
            0,
            false,
            W::Sequential,
        ),
        p(
            "nearby-only",
            "Nearby Only",
            "Preload nearby images only",
            Preview,
            4,
            8,
            4,
            false,
            W::Preload,
        ),
        p(
            "full-folder-warm",
            "Full Folder Warm",
            "Warm up the whole folder",
            Preview,
            8,
            32,
            8,
            true,
            W::Preload,
        ),
        p(
            "large-folder-safe",
            "Large Folder Safe",
            "Cap RAM for large folders",
            Preview,
            4,
            8,
            2,
            false,
            W::Preload,
        ),
        p(
            "huge-image-safe",
            "Huge Image Safe",
            "Lower concurrency for huge images",
            Preview,
            2,
            4,
            2,
            false,
            W::FirstFrame,
        ),
        p(
            "ssd-throughput",
            "SSD High Throughput",
            "Parallel reads on SSD",
            Preview,
            12,
            32,
            8,
            false,
            W::Sequential,
        ),
        p(
            "hdd-conservative",
            "HDD Conservative",
            "Fewer seeks on HDD",
            Preview,
            2,
            4,
            1,
            false,
            W::Sequential,
        ),
        p(
            "network-safe",
            "Network Safe",
            "Careful reads on network drives",
            Preview,
            2,
            4,
            1,
            false,
            W::FirstFrame,
        ),
        BenchmarkProfile {
            memory_reserve_bytes: LOW_MEMORY_RESERVE_BYTES,
            ..p(
                "low-memory",
                "Low Memory",
                "Keep a large RAM reserve",
                Preview,
                2,
                4,
                2,
                false,
                W::Sequential,
            )
        },
        p(
            "ram-maximizer",
            "RAM Maximizer",
            "Use RAM up to the safe limit",
            Preview,
            12,
            64,
            16,
            true,
            W::Preload,
        ),
        p(
            "rapid-key-press",
            "Rapid Key Press",
            "Rapid Next presses, no skipping",
            Preview,
            8,
            16,
            4,
            false,
            W::WarmNext,
        ),
        p(
            "random-navigation",
            "Random Navigation",
            "Random navigation",
            Preview,
            6,
            16,
            8,
            false,
            W::Random,
        ),
        p(
            "cache-recovery",
            "Cache Recovery",
            "Clear and rebuild the cache",
            Preview,
            4,
            8,
            2,
            false,
            W::Correctness,
        ),
        p(
            "explorer-reindex",
            "Explorer Reindex",
            "Check the native order",
            Preview,
            4,
            16,
            8,
            false,
            W::Correctness,
        ),
        BenchmarkProfile {
            detailed_logging: true,
            ..p(
                "logging-on",
                "Logging On",
                "Measure the detailed-logging overhead",
                Preview,
                4,
                16,
                8,
                false,
                W::Sequential,
            )
        },
        p(
            "logging-off",
            "Logging Off",
            "Measure without detailed logging",
            Preview,
            4,
            16,
            8,
            false,
            W::Sequential,
        ),
        p(
            "recommended-auto",
            "Recommended Auto",
            "Choose automatically by folder and RAM",
            Preview,
            8,
            32,
            8,
            false,
            W::WarmNext,
        ),
        p(
            "action-move",
            "Move Race",
            "Move while decoding",
            Preview,
            4,
            8,
            2,
            false,
            W::FileAction,
        ),
        p(
            "action-delete",
            "Delete To Recycle Bin Race",
            "Delete while decoding, no retry",
            Preview,
            4,
            8,
            2,
            false,
            W::FileAction,
        ),
        p(
            "action-copy",
            "Copy During Decode",
            "Copy while decoding",
            Preview,
            4,
            8,
            2,
            false,
            W::FileAction,
        ),
        p(
            "action-interleaved",
            "Interleaved Actions",
            "Next, Move, Delete, Copy interleaved",
            Preview,
            4,
            8,
            2,
            false,
            W::FileAction,
        ),
        // Row 30 uses the full constructor in C# (`new(..., Reserve, false, true, Correctness, 0, 3, true)`): no warm-up,
        // 3 iterations, correctness-only, and detailed logging ON (the contract's table said false; the C# source and the
        // golden say true, and C# decides).
        BenchmarkProfile {
            id: "original-correctness".to_owned(),
            name: "Original Correctness".to_owned(),
            description: "Check quality and cache identity".to_owned(),
            loading_mode: Original,
            workers: 1,
            next_window: 0,
            previous_window: 0,
            full_folder: false,
            memory_reserve_bytes: MEMORY_RESERVE_BYTES,
            disk_cache: false,
            detailed_logging: true,
            workload: W::Correctness,
            warmup_count: 0,
            iterations: 3,
            correctness_only: true,
        },
    ]
}

/// C# `BenchmarkProfiles.All`: the 30 profiles, in the C# order (see the table in contracts/W7-01b.md).
pub fn all() -> &'static [BenchmarkProfile] {
    ALL.get_or_init(build_all).as_slice()
}

/// C# `BenchmarkProfiles.Runnable`: `all()` without `correctness_only` profiles and without `is_not_implemented` ones,
/// in `all()` order (27 profiles).
pub fn runnable() -> &'static [BenchmarkProfile] {
    RUNNABLE
        .get_or_init(|| {
            all()
                .iter()
                .filter(|p| !p.correctness_only && !is_not_implemented(p))
                .cloned()
                .collect()
        })
        .as_slice()
}

/// C# `BenchmarkProfiles.Find(id)`: first profile whose id equals `id` under OrdinalIgnoreCase
/// (`pr_core::text::eq_ignore_case`; no trimming; `""` and unknown ids give `None`).
pub fn find(id: &str) -> Option<&'static BenchmarkProfile> {
    all().iter().find(|p| eq_ignore_case(&p.id, id))
}

/// C# `BenchmarkProfiles.IsNotImplemented(profile)`: `workload == Correctness` AND the id is exactly (ordinal,
/// case-sensitive) `"explorer-reindex"` or `"cache-recovery"`. Looks at the argument's own fields, so a modified copy
/// (`with { Workload = .. }` in C#) is judged by its own workload and id, not by the registry.
pub fn is_not_implemented(profile: &BenchmarkProfile) -> bool {
    profile.workload == BenchmarkWorkload::Correctness
        && matches!(profile.id.as_str(), "explorer-reindex" | "cache-recovery")
}
