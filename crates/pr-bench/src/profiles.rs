//! Port of `PhotoReview.Benchmarking/BenchmarkProfiles.cs` (C# baseline d92d6aae, 60 lines): the registry of the 30
//! built-in benchmark profiles plus `Find`, `Runnable` and `IsNotImplemented`.
//!
//! Unit W7-01b. Contract: `contracts/W7-01b.md`. The profile record itself (`BenchmarkProfile`, `BenchmarkWorkload`) is
//! W7-01a's `crate::models`; this file only owns the data and the three lookups.
//!
//! The registry is built once (`OnceLock`) and handed out as `&'static` so `find` returns the same instance every time,
//! as the C# static list does. Order is the C# list order and is observable (`--benchmark-all`, the Benchmark window).
#![allow(unused_variables, dead_code)] // D11: stub with todo!() bodies; the porter removes this line with the last todo!().

use crate::models::BenchmarkProfile;

/// C# `BenchmarkProfiles.All`: the 30 profiles, in the C# order (see the table in contract.md).
pub fn all() -> &'static [BenchmarkProfile] {
    todo!("W7-01b: build the 30 profiles once in a OnceLock<Vec<BenchmarkProfile>>")
}

/// C# `BenchmarkProfiles.Runnable`: `all()` without `correctness_only` profiles and without `is_not_implemented` ones,
/// in `all()` order (27 profiles).
pub fn runnable() -> &'static [BenchmarkProfile] {
    todo!("W7-01b: filtered clone of all(), built once in a OnceLock")
}

/// C# `BenchmarkProfiles.Find(id)`: first profile whose id equals `id` under OrdinalIgnoreCase
/// (`pr_core::text::eq_ignore_case`; no trimming; `""` and unknown ids give `None`).
pub fn find(id: &str) -> Option<&'static BenchmarkProfile> {
    todo!("W7-01b: all().iter().find(|p| eq_ignore_case(&p.id, id))")
}

/// C# `BenchmarkProfiles.IsNotImplemented(profile)`: `workload == Correctness` AND the id is exactly (ordinal,
/// case-sensitive) `"explorer-reindex"` or `"cache-recovery"`. Looks at the argument's own fields, so a modified copy
/// (`with { Workload = .. }` in C#) is judged by its own workload and id, not by the registry.
pub fn is_not_implemented(profile: &BenchmarkProfile) -> bool {
    todo!("W7-01b: profile.workload == Correctness && matches!(profile.id.as_str(), ..)")
}
