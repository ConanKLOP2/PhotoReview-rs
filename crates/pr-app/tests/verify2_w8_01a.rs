// Adversarial verification test (kept out of the commit).
use pr_app::viewport::state::{LayoutEngine, ViewportState};
use pr_app::viewport::types::{ViewportInput, ViewportLayout};

struct NanOffsets;
impl LayoutEngine for NanOffsets {
    fn compute(&self, _: &ViewportInput) -> ViewportLayout {
        ViewportLayout::default()
    }
    fn clamp_offset(&self, _: &ViewportLayout, _: f64, _: f64) -> (f64, f64) {
        (f64::NAN, f64::NAN)
    }
}

#[test]
fn update_layout_nan_offsets_report_changed_like_csharp_tuple_inequality() {
    // Oracle (.NET, d92d6aae ViewportState.cs): UpdateLayout with offsets NaN -> NaN returns True, because the tuple
    // `previous != (Layout, H, V)` compares doubles with the IEEE `!=` operator (NaN != NaN), not double.Equals.
    let mut s = ViewportState::new(NanOffsets);
    assert!(s.update_layout());
    s.scroll_to(1.0, 1.0);
    assert!(
        s.update_layout(),
        "C# returns true when an offset is NaN before and after"
    );
}
