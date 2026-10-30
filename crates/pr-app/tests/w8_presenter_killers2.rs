//! Mutation killers, second batch, for `pr-app` presenters (lane cl-overlay; report `docs/reviews/CL-OVERLAY-2026-10-30.md`).
//! Every test names the mutant (file:line, operator) it kills. Fakes only; no sleeps, no GUI, no real Recycle Bin.

mod support {
    pub mod preview_fakes;
}

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pr_app::presenters::benchmark::{BenchmarkSessionDeps, ProductionBenchmarkSession};
use pr_bench::profiles;
use pr_core::abstractions::RecycleBin;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_test_support::FakeFileSystem;
use pr_test_support::FakeRecycleBin;
use support::preview_fakes::{FakePayloadCodec, PixelsCodec};

struct NoDecoder;

impl ImageDecoder for NoDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Err(DecodeError::Canceled)
    }
    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Err(DecodeError::Canceled)
    }
}

fn deps(created: &Arc<AtomicUsize>, memory: i64) -> BenchmarkSessionDeps {
    let counter = Arc::clone(created);
    BenchmarkSessionDeps {
        platform_codec: Arc::new(PixelsCodec),
        payload_codec: Arc::new(FakePayloadCodec),
        decoder_factory: Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Arc::new(NoDecoder) as Arc<dyn ImageDecoder>
        }),
        real_recycle_bin: Arc::new(|| {
            Arc::new(FakeRecycleBin::new(Arc::new(FakeFileSystem::new()))) as Arc<dyn RecycleBin>
        }),
        physical_memory_bytes: memory,
        memory_probe: None,
        scratch_parent: None,
    }
}

#[test]
fn benchmark_session_deps_debug_names_the_type_and_the_memory_field() {
    // Kills benchmark.rs:900 `<BenchmarkSessionDeps as Debug>::fmt -> Ok(Default::default())` (empty output).
    let created = Arc::new(AtomicUsize::new(0));
    let text = format!("{:?}", deps(&created, 123_456_789));
    assert!(text.contains("BenchmarkSessionDeps"), "{text}");
    assert!(text.contains("physical_memory_bytes: 123456789"), "{text}");
    assert_eq!(created.load(Ordering::SeqCst), 0, "Debug must not build a decoder");
}

#[test]
fn production_session_debug_names_the_type_and_the_profile_id() {
    // Kills benchmark.rs:927 `<ProductionBenchmarkSession as Debug>::fmt -> Ok(Default::default())` (empty output).
    let created = Arc::new(AtomicUsize::new(0));
    let profile = profiles::all().first().expect("the registry has profiles").clone();
    let session = ProductionBenchmarkSession::new(&profile, &[], &deps(&created, 1 << 30)).expect("session builds");
    let text = format!("{session:?}");
    assert!(text.contains("ProductionBenchmarkSession"), "{text}");
    assert!(text.contains(&format!("profile: {:?}", profile.id)), "{text}");
    assert_eq!(created.load(Ordering::SeqCst), 1, "one fresh decoder per session");
}
