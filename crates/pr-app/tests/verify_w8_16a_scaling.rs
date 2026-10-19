//! Adversarial verification (NOT committed): kills cargo-mutants survivor `scaling_policy.rs:47:32 || -> &&`.
use pr_app::viewing::scaling_policy::is_heavy_downscale;

// C#: `sourcePixelWidth <= 0 || sourcePixelHeight <= 0` returns false even when both are negative (product would be positive).
#[test]
fn both_source_sides_negative_is_not_heavy() {
    assert!(!is_heavy_downscale(-10_000, -10_000, 100.0, 100.0));
}
