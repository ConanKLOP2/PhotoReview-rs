//! P-393 (2026-11-06): kills the `load_file` `NotFound` guard mutants (decoder.rs, N4 1204:27) through the
//! `TurboJpegDecoder::with_file_opener` fault-injection seam. The seam replaces ONLY the open of the source file; the file
//! still exists, so `file_exists` passes and the open is what fails (the race the guard is for). No `turbojpeg.dll`, no sleep.
//! C# twin: `LoadBytes` maps `FileNotFoundException` to the localized not-found text; every other `IOException` stays itself.

use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::Arc;

use pr_imaging::decode_types::DecodeRequest;
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::PixelBuffer;
use pr_test_support::TempRoot;
use pr_turbojpeg::decoder::{
    Decompressor, DecompressorFactory, EnglishTurboJpegTexts, FineScaleError, FineScaler, TurboJpegDecoder,
};

struct NoScale;

impl FineScaler for NoScale {
    fn resize(&self, _source: &PixelBuffer, _width: i32, _height: i32) -> Result<PixelBuffer, FineScaleError> {
        Err(FineScaleError::Failed(DecodeError::Other {
            message: "unused".to_owned(),
        }))
    }
}

fn decoder(open_file: fn(&Path) -> io::Result<File>) -> TurboJpegDecoder {
    let factory: Arc<dyn DecompressorFactory> = Arc::new(|| -> Result<Box<dyn Decompressor>, DecodeError> {
        Err(DecodeError::Other {
            message: "the file open fails first".to_owned(),
        })
    });
    let memory: Arc<dyn MemoryInfoSource> = Arc::new(|| MemoryInfo {
        total_available: 1 << 36,
        load: 0,
    });
    TurboJpegDecoder::new(
        PixelBufferImageCodec::shared(),
        Arc::new(NoScale),
        memory,
        factory,
        Arc::new(EnglishTurboJpegTexts),
    )
    .with_file_opener(open_file)
}

fn failure(decoder: &TurboJpegDecoder, path: &Path) -> DecodeError {
    match decoder.decode(&DecodeRequest::new(path, 0)) {
        Ok(_) => panic!("the injected open failure must fail the decode"),
        Err(error) => error,
    }
}

fn existing_file(temp: &TempRoot) -> std::path::PathBuf {
    let path = temp.join("present.jpg");
    std::fs::write(&path, b"not read: the injected open fails first").expect("write");
    assert!(path.is_file());
    path
}

fn fail_with_not_found(_path: &Path) -> io::Result<File> {
    Err(io::Error::from(io::ErrorKind::NotFound))
}

fn fail_with_permission_denied(_path: &Path) -> io::Result<File> {
    Err(io::Error::from(io::ErrorKind::PermissionDenied))
}

fn fail_with_other(_path: &Path) -> io::Result<File> {
    Err(io::Error::other("sharing violation"))
}

/// decoder.rs `load_file` guard `error.kind() == NotFound` -> `false`: a file that disappears between the existence check and
/// the open must still end as the localized `FileNotFound`, never as a raw `Io` error.
#[test]
fn a_file_that_vanishes_between_the_existence_check_and_the_open_is_a_localized_file_not_found() {
    let temp = TempRoot::new("p393-tj-notfound");
    let path = existing_file(&temp);
    let error = failure(&decoder(fail_with_not_found), &path);
    assert!(matches!(error.kind(), DecodeError::FileNotFound { .. }), "{error:?}");
    assert!(error.is_localized());
}

/// decoder.rs `load_file` guard -> `true`: only `NotFound` is the not-found outcome; any other open failure keeps its kind as
/// `DecodeError::Io`.
#[test]
fn any_other_open_failure_stays_an_io_error_of_its_own_kind() {
    let temp = TempRoot::new("p393-tj-other");
    let path = existing_file(&temp);
    for (name, opener, kind) in [
        (
            "permission denied",
            fail_with_permission_denied as fn(&Path) -> io::Result<File>,
            io::ErrorKind::PermissionDenied,
        ),
        ("other", fail_with_other, io::ErrorKind::Other),
    ] {
        let error = failure(&decoder(opener), &path);
        match error.kind() {
            DecodeError::Io(inner) => assert_eq!(inner.kind(), kind, "{name}"),
            other => panic!("{name}: expected Io, got {other:?}"),
        }
    }
}
