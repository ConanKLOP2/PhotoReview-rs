//! Mutation killers for `pr-bench` (lane cl-mut-bench3). The `tool_path_guard` killer is Windows only (Win32 share modes).

#[path = "../../pr-imaging/tests/support/preview_service.rs"]
mod preview_service;

use std::sync::Arc;

use pr_bench::image_executor::{
    BenchmarkExecutorOptions, BenchmarkImageExecutor, BenchmarkProfileScope, ExecutorError, TeardownHandle,
    WindowedPreloadTarget,
};
use pr_core::catalog::types::CatalogEntry;
use pr_core::sync::CancelToken;
use pr_imaging::cache_key::ImageCacheKey;
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::preload::target::{KeyError, PreloadTarget};
use pr_test_support::TempRoot;
use preview_service::{FakeDecoder, FakePayloadCodec, PHYSICAL_32_GIB, PixelsCodec};

#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;

#[cfg(windows)]
use pr_bench::cli::tool_path_guard::write_report_file;

/// tool_path_guard.rs:245:25 (`==` to `!=` in `replace_leaf`): a target another handle holds open WITHOUT FILE_SHARE_DELETE is not
/// seen by `leaf_refuses_replacement` (the DELETE probe fails), so `rename` itself answers `PermissionDenied`. That must read like
/// `File.Move`: the path-less "Access to the path is denied." (with `!=` it becomes the `from_io` text that names the path).
#[cfg(windows)]
#[test]
fn replace_leaf_rename_permission_denied_is_the_pathless_access_denied() {
    let root = TempRoot::new("mb3-sharing");
    let leaf = root.join("l.json");
    std::fs::write(&leaf, "x").expect("file");
    // read + write sharing, no delete sharing
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&leaf)
        .expect("open");
    let r = write_report_file(&leaf.to_string_lossy(), "y");
    drop(handle);
    let e = r.expect_err("a leaf held open without FILE_SHARE_DELETE cannot be replaced");
    assert_eq!(e.type_name, "UnauthorizedAccessException");
    assert_eq!(e.message, "Access to the path is denied.");
    assert_eq!(std::fs::read_to_string(&leaf).expect("read"), "x");
}

fn profile() -> pr_bench::models::BenchmarkProfile {
    pr_bench::models::BenchmarkProfile {
        id: "mb3".to_owned(),
        loading_mode: pr_core::model::enums::LoadingMode::Fast,
        workers: 1,
        next_window: 0,
        previous_window: 0,
        full_folder: false,
        memory_reserve_bytes: 0,
        disk_cache: false,
        detailed_logging: false,
        name: "mb3".to_owned(),
        description: String::new(),
        workload: pr_bench::models::BenchmarkWorkload::Sequential,
        warmup_count: 1,
        iterations: 1,
        correctness_only: false,
    }
}

fn build_error(files: &[String], decoder: Option<Arc<dyn ImageDecoder>>, root: &TempRoot) -> ExecutorError {
    let options = BenchmarkExecutorOptions {
        has_headroom: Some(Arc::new(|_: f64| true)),
        decoder,
        scratch_parent: Some(root.join("scratch")),
        physical_memory_bytes: PHYSICAL_32_GIB,
        ..Default::default()
    };
    match BenchmarkImageExecutor::new(
        &profile(),
        files,
        Arc::new(PixelsCodec),
        Arc::new(FakePayloadCodec),
        options,
    ) {
        Ok(_) => panic!("construction must fail"),
        Err(error) => error,
    }
}

/// image_executor.rs:76:9 (`ExecutorError::fmt` -> `Ok(Default::default())`): both variants print their text.
#[test]
fn executor_error_display_names_the_cause() {
    let root = TempRoot::new("mb3-executor-error");
    let invalid = build_error(
        &[String::new()],
        Some(FakeDecoder::new() as Arc<dyn ImageDecoder>),
        &root,
    );
    assert!(matches!(invalid, ExecutorError::InvalidPath(_)));
    assert!(
        invalid.to_string().starts_with("invalid benchmark file path: "),
        "{invalid}"
    );
    assert!(invalid.to_string().len() > "invalid benchmark file path: ".len());

    let service = build_error(&[], None, &root);
    assert!(matches!(service, ExecutorError::Service(_)));
    assert!(!service.to_string().is_empty(), "the service error text must be shown");
    assert!(!service.to_string().contains("invalid benchmark file path"));
}

/// image_executor.rs:139:9 (`BenchmarkExecutorOptions` Debug -> `Ok(Default::default())`).
#[test]
fn executor_options_debug_names_the_type_and_fields() {
    let options = BenchmarkExecutorOptions {
        scratch_parent: Some(std::path::PathBuf::from("scratch-mb3")),
        ..Default::default()
    };
    let text = format!("{options:?}");
    assert!(text.contains("BenchmarkExecutorOptions"), "{text}");
    assert!(text.contains("scratch-mb3"), "{text}");
}

/// image_executor.rs:157:9 (`TeardownHandle` Debug -> `Ok(Default::default())`).
#[test]
fn teardown_handle_debug_names_the_type() {
    let text = format!("{:?}", TeardownHandle::completed());
    assert!(text.contains("TeardownHandle"), "{text}");
}

struct NullTarget;

impl PreloadTarget for NullTarget {
    fn try_get_cached_preview_for_path(&self, _path: &str) -> bool {
        false
    }
    fn try_get_cached_preview(&self, _key: &ImageCacheKey) -> bool {
        false
    }
    fn preload(&self, _path: &str, _cancel: &CancelToken) -> Result<(), DecodeError> {
        Ok(())
    }
    fn get_current_cache_key_for_path(&self, _path: &str) -> Result<ImageCacheKey, KeyError> {
        Err(KeyError::Io(std::io::Error::other("null target")))
    }
    fn get_current_cache_key(&self, _entry: &CatalogEntry) -> Result<ImageCacheKey, KeyError> {
        Err(KeyError::Io(std::io::Error::other("null target")))
    }
    fn cache_count(&self) -> i32 {
        0
    }
    fn cache_bytes(&self) -> i64 {
        0
    }
}

/// image_executor.rs:295:9 (`WindowedPreloadTarget` Debug -> `Ok(Default::default())`).
#[test]
fn windowed_target_debug_shows_the_window() {
    let mut profile = profile();
    profile.next_window = 7;
    profile.previous_window = 3;
    let target = WindowedPreloadTarget::new(Arc::new(NullTarget), &[], &profile).expect("empty list");
    let text = format!("{target:?}");
    assert!(text.contains("WindowedPreloadTarget"), "{text}");
    assert!(text.contains("next: 7") && text.contains("previous: 3"), "{text}");
}

/// image_executor.rs:256:9 (`LoggingScope` Debug -> `Ok(Default::default())`).
#[test]
fn logging_scope_debug_shows_whether_the_restore_is_pending() {
    let mut scope = BenchmarkProfileScope::apply_logging(&profile(), || false, |_| {}, None);
    let text = format!("{scope:?}");
    assert!(
        text.contains("LoggingScope") && text.contains("pending: true"),
        "{text}"
    );
    scope.dispose();
    assert!(format!("{scope:?}").contains("pending: false"));
}

/// image_executor.rs:178:9 (`TeardownHandle::finish` -> `()`): after `dispose` the background pass must complete the handle. The
/// wait is bounded, so the mutant fails fast instead of hanging.
#[test]
fn dispose_completes_the_teardown_handle() {
    let root = TempRoot::new("mb3-teardown");
    let options = BenchmarkExecutorOptions {
        has_headroom: Some(Arc::new(|_: f64| true)),
        decoder: Some(FakeDecoder::new() as Arc<dyn ImageDecoder>),
        scratch_parent: Some(root.join("scratch")),
        physical_memory_bytes: PHYSICAL_32_GIB,
        ..Default::default()
    };
    let executor = BenchmarkImageExecutor::new(
        &profile(),
        &[],
        Arc::new(PixelsCodec),
        Arc::new(FakePayloadCodec),
        options,
    )
    .unwrap_or_else(|_| panic!("executor"));
    assert!(
        executor.teardown_background_task().is_complete(),
        "nothing pending before dispose"
    );
    executor.dispose();
    let handle = executor.teardown_background_task();
    assert!(
        handle.wait_timeout(std::time::Duration::from_secs(20)),
        "the teardown must finish"
    );
    assert!(handle.is_complete());
}
