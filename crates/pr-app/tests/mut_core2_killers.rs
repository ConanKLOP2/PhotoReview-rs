//! cl-mut-core2 (2026-10-31): killers for `zoom_detail.rs` mutants that survived the CL-MUT-CORE2 rerun.

#[path = "support/preview_fakes.rs"]
mod preview_fakes;

use std::sync::Arc;

use pr_app::coordinators::zoom_detail::{BackgroundJob, BackgroundRunner, ZoomDetailLoader};
use pr_core::catalog::generation::GenerationClock;
use pr_test_support::TempRoot;

struct NeverRunner;

impl BackgroundRunner for NeverRunner {
    fn run(&self, _: &'static str, _job: BackgroundJob) {}
}

// zoom_detail.rs:282 `<impl Debug for ZoomDetailLoader>::fmt -> Ok(Default::default())`: the mutant writes nothing.
#[test]
fn zoom_detail_loader_debug_names_the_type_and_its_state() {
    let root = TempRoot::new("cl-mut-core2-debug");
    let service = preview_fakes::build_service(
        &root.join("cache"),
        preview_fakes::SizedDecoder::new(),
        Default::default(),
    );
    let mut loader = ZoomDetailLoader::new(
        service,
        Arc::new(GenerationClock::default()),
        Arc::new(NeverRunner),
        Arc::new(|| {}),
    );
    let _ = loader.set_zoom(Some(2.0));
    let text = format!("{loader:?}");
    assert!(text.contains("ZoomDetailLoader"), "{text}");
    assert!(text.contains("zoom: Some(2.0)"), "{text}");
    assert!(text.contains("showing_original: false"), "{text}");
    assert!(text.contains("has_original: false"), "{text}");
}
