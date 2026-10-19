//! Tests of `pr_app::services::preview_state` (contract `contracts/W8-04c.md`, rules R-PS-1..3).
//!
//! Port of the C# `PreviewStateContext` defaults (no C# test file; the contract's test plan).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pr_app::services::preview_state::PreviewStateContext;
use pr_core::model::enums::DecoderBackend;
use pr_imaging::decode_types::DecodeBox;

// Contract test (R-PS-1, R-PS-2): the C# defaults are not Original, unbounded and the `Wpf` backend.
#[test]
fn defaults_are_not_original_unbounded_wpf() {
    let context = PreviewStateContext::new();

    assert!(!(context.is_original_loading_mode)());
    assert_eq!((context.target_decode_box)(), DecodeBox::UNBOUNDED);
    assert_eq!((context.current_backend)(), DecoderBackend::Wpf);
}

// Contract test (R-PS-3): clones share the captured state; replacing a closure on one clone is not seen by the other.
#[test]
fn clones_share_the_captured_state() {
    let live = Arc::new(AtomicBool::new(false));
    let mut context = PreviewStateContext::new();
    let source = Arc::clone(&live);
    context.is_original_loading_mode = Arc::new(move || source.load(Ordering::Acquire));

    let clone = context.clone();
    live.store(true, Ordering::Release);
    assert!(
        (context.is_original_loading_mode)(),
        "the closure observes the live state"
    );
    assert!((clone.is_original_loading_mode)(), "the clone shares the closure");

    let mut replaced = context.clone();
    replaced.is_original_loading_mode = Arc::new(|| false);
    assert!(!(replaced.is_original_loading_mode)());
    assert!(
        (context.is_original_loading_mode)(),
        "replacing a field on one clone leaves the other clone unchanged"
    );
}
