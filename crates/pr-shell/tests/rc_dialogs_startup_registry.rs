//! Review-confirm tests of group G15 (`docs/reviews/G7-G15-2026-11-03.md`): startup review L2, the cached `decoder_factory` closures hold the settings store and not the
//! registry itself, so no `Arc` cycle keeps the graph alive after the owner drops it. New (Rust only ownership regression, no C# twin: the C# GC collects cycles).
// The shared fixture offers more helpers than this one test uses.
#[allow(dead_code)]
#[path = "common/composition_fixture.rs"]
mod composition_fixture;

use std::sync::Arc;

#[test]
fn decoder_factory_does_not_keep_the_service_graph_alive_after_the_owner_drops_it() {
    let (services, _root, log) = composition_fixture::shared_services();
    let store = services.settings_store();
    let weak_store = Arc::downgrade(&store);
    let factory = services.decoder_factory().expect("decoder factory");
    drop(store);
    drop(log);
    drop(services);
    // The factory legitimately holds the settings store for its decorator closure; only the registry (`Inner`) must be gone.
    drop(factory);
    assert!(
        weak_store.upgrade().is_none(),
        "the cached decoder factory closure kept the registry (an Arc cycle) alive"
    );
}
