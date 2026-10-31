//! Review fixes L4 and L8 of `ST-SHELL-REVIEW-2026-10-31` (lane fix-shell-low). New tests, no C# twin.
#![cfg(windows)]
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use pr_core::model::enums::DecoderBackend;
use pr_imaging::decode_types::DecodeRequest;
use pr_imaging::decoder_traits::{DecodeError, ImageDecoderFactory};
use pr_shell::composition::adapters::factory_or_unavailable;
use pr_shell::composition::registry::{RegistryError, shipped_languages_dir};
use pr_test_support::{LogLevel, RecordingLog};

fn failed() -> Result<Arc<dyn ImageDecoderFactory>, RegistryError> {
    Err(RegistryError {
        service: "source_bytes_policy",
        message: "no room".to_owned(),
    })
}

#[test]
fn decoder_factory_error_is_logged_and_answered_with_an_unavailable_factory() {
    // review L4: used to log and then panic on the UI thread inside the Settings dialog loop.
    let log = RecordingLog::new();
    let factory = factory_or_unavailable(failed(), &log);
    assert!(log.contains(LogLevel::Error, "The decoder factory could not be created"));
    assert!(!factory.is_registered(DecoderBackend::WicDirect));
    let decoder = factory.create(DecoderBackend::WicDirect);
    let error = decoder
        .decode(&DecodeRequest::new("x.jpg", 100))
        .err()
        .expect("decode fails");
    assert!(matches!(error, DecodeError::NotSupported { .. }), "{error:?}");
    assert!(decoder.read_info("x.jpg").is_err());
}

#[test]
fn shipped_languages_dir_uses_the_exe_directory() {
    // C# AppContext.BaseDirectory\Languages.
    let log = RecordingLog::new();
    let dir = shipped_languages_dir(Ok(PathBuf::from(r"C:\app\PhotoReviewRs.exe")), None, &log);
    assert_eq!(dir, PathBuf::from(r"C:\app\Languages"));
    assert!(log.entries().is_empty());
}

#[test]
fn shipped_languages_dir_falls_back_to_argv0_with_a_warning_when_current_exe_fails() {
    // review L8: used to become the relative `Languages` (cwd dependent).
    let log = RecordingLog::new();
    let error = std::io::Error::other("no exe");
    let dir = shipped_languages_dir(Err(error), Some(OsString::from(r"D:\tools\PhotoReviewRs.exe")), &log);
    assert_eq!(dir, PathBuf::from(r"D:\tools\Languages"));
    assert!(log.contains(LogLevel::Warn, "current_exe failed"));
}

#[test]
fn shipped_languages_dir_without_any_exe_path_keeps_the_relative_directory() {
    let log = RecordingLog::new();
    let error = std::io::Error::other("no exe");
    assert_eq!(
        shipped_languages_dir(Err(error), None, &log),
        PathBuf::from("Languages")
    );
}
