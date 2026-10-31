//! Mutation killers for pr-bench (lane cl-mut-bench2). All file I/O stays inside TempRoot; no golden files.

use std::error::Error;
use std::sync::Arc;

use pr_bench::workload_runner::WorkloadError;
use pr_imaging::service::PreviewError;

// workload_runner.rs:56 `<impl Display for WorkloadError>::fmt -> Ok(Default::default())`: each variant shows its text.
#[test]
fn workload_error_display_shows_the_inner_text() {
    let io = WorkloadError::Io(std::io::Error::other("disk on fire"));
    assert_eq!(io.to_string(), "disk on fire");
    let decode = WorkloadError::Decode(PreviewError::Canceled);
    assert_eq!(decode.to_string(), PreviewError::Canceled.to_string());
    assert!(!decode.to_string().is_empty());
    let invalid = WorkloadError::InvalidArgument("no files here".to_owned());
    assert_eq!(invalid.to_string(), "no files here");
}

// workload_runner.rs:66 `<impl Error for WorkloadError>::source -> None`: Io and Decode expose their cause, InvalidArgument has none.
#[test]
fn workload_error_source_exposes_the_cause() {
    let io = WorkloadError::Io(std::io::Error::other("disk on fire"));
    assert_eq!(io.source().expect("io source").to_string(), "disk on fire");
    let decode = WorkloadError::Decode(PreviewError::Spawn(Arc::new(std::io::Error::other("no thread"))));
    assert!(decode.source().is_some());
    assert!(WorkloadError::InvalidArgument("x".to_owned()).source().is_none());
}
