//! Mutation killers for lane cl-rest2 (report `docs/reviews/CL-REST2-2026-10-30.md`). Fakes and `TempRoot` only; no real Recycle Bin, no GUI.
//! The wait is a bounded `yield_now` poll (no sleeps) because `dispose` deletes the scratch directory on a background thread.

mod support {
    pub mod preview_fakes;
}

use std::sync::Arc;
use std::time::{Duration, Instant};

use pr_app::presenters::benchmark::{BenchmarkProfileSession, BenchmarkSessionDeps, ProductionBenchmarkSession};
use pr_bench::profiles;
use pr_core::abstractions::RecycleBin;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_test_support::{FakeFileSystem, FakeRecycleBin, TempRoot};
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

fn deps(scratch_parent: Option<std::path::PathBuf>) -> BenchmarkSessionDeps {
    BenchmarkSessionDeps {
        platform_codec: Arc::new(PixelsCodec),
        payload_codec: Arc::new(FakePayloadCodec),
        decoder_factory: Arc::new(|| Arc::new(NoDecoder) as Arc<dyn ImageDecoder>),
        real_recycle_bin: Arc::new(|| {
            Arc::new(FakeRecycleBin::new(Arc::new(FakeFileSystem::new()))) as Arc<dyn RecycleBin>
        }),
        physical_memory_bytes: 1 << 30,
        memory_probe: None,
        scratch_parent,
    }
}

fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::yield_now();
    }
    done()
}

#[test]
fn dispose_deletes_the_scratch_directory_under_the_injected_parent() {
    // Kills benchmark.rs `ProductionBenchmarkSession::dispose -> ()` and delete `!` in `if !self.disposed` (neither would start the teardown).
    let root = TempRoot::new("cl_rest2_dispose");
    let profile = profiles::all().first().expect("profiles").clone();
    let mut session =
        ProductionBenchmarkSession::new(&profile, &[], &deps(Some(root.path().to_path_buf()))).expect("session");
    let scratch = session.scratch_directory().to_path_buf();
    assert_eq!(
        scratch.parent(),
        Some(root.path()),
        "the scratch directory is created under the injected parent"
    );
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    std::fs::write(scratch.join("x.bin"), b"x").expect("file");
    session.dispose();
    assert!(
        wait_until(|| !scratch.exists()),
        "dispose must remove the scratch directory"
    );
}

#[test]
fn dispose_is_idempotent_and_session_without_dispose_keeps_the_directory() {
    // Kills the `delete !` mutant from the other side: a fresh session must NOT have torn down anything, and a second dispose is a no-op.
    let root = TempRoot::new("cl_rest2_dispose_twice");
    let profile = profiles::all().first().expect("profiles").clone();
    let mut session =
        ProductionBenchmarkSession::new(&profile, &[], &deps(Some(root.path().to_path_buf()))).expect("session");
    let scratch = session.scratch_directory().to_path_buf();
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    assert!(
        scratch.exists(),
        "constructing a session does not delete its scratch directory"
    );
    session.dispose();
    assert!(wait_until(|| !scratch.exists()));
    std::fs::create_dir_all(&scratch).expect("recreate");
    session.dispose();
    // The second dispose must not start another teardown: give a would-be thread a bounded chance to run, then require the directory.
    for _ in 0..2000 {
        std::thread::yield_now();
    }
    assert!(scratch.exists(), "a second dispose is a no-op (disposed flag)");
}
