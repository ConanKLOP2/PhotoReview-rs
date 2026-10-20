//! Tests of the stat, the original-dimension lookup and `PhotoInfo` (W8-07k): `StatNow`, `TryGetOriginalDimensionsAsync` and `PhotoInfo.From` (C# `ImagePresenter.cs`
//! lines 839 to 863, 895 to 925 and 927 to 949, baseline d92d6aae). Contract: `contracts/W8-07k.md`, kind F (fakes, no oracle, no golden).
//!
//! There is no isolated C# test of these helpers; the presenter tests of 07h to 07j cover them end to end. These pin the classification tables.

mod support {
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::io;
use std::sync::Arc;

use pr_app::coordinators::presenter::photo_info::{
    PhotoInfo, StatOutcome, is_expected_dimension_failure, stat_error_means_missing, stat_now,
    try_get_original_dimensions,
};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::text::dotnet_path::PathError;
use pr_imaging::cache::preview_file::PreviewCacheError;
use pr_imaging::cache::source_bytes::SourceBytesError;
use pr_imaging::cache_key::CacheKeyError;
use pr_imaging::decode_types::{DecodedImage, DecodedImageOptions, DecodedImageView, PlatformImage};
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::exif_summary::ExifSummary;
use pr_imaging::service::{PreviewError, SourceChangePhase};
use pr_test_support::TempRoot;
use support::present_helpers::{ScriptedFs, ThrowingInfoDecoder};
use support::presenter_fakes::raw_view;
use support::preview_fakes::{ServiceOptions, build_service_with_metrics, sized_image};

// ---- PhotoInfo --------------------------------------------------------------------------------------------------------------------

// New test (no direct C# test of `PhotoInfo.From`; behaviour from the source and `ZoomDetailTests.RawPreview*`)
#[test]
fn photo_info_from_takes_file_name_original_size_exif_and_raw_preview_size() {
    let exif = ExifSummary {
        camera_make: Some("Canon".to_owned()),
        iso: Some(200),
        ..ExifSummary::default()
    };
    let image = DecodedImage::new(
        PlatformImage::Opaque(Arc::new(0_u8)),
        2000,
        1333,
        2000 * 1333 * 4,
        DecodedImageOptions {
            downscaled: true,
            original_width: 6000,
            original_height: 4000,
            exif: Some(exif.clone()),
            ..DecodedImageOptions::default()
        },
    )
    .unwrap_or_else(|error| panic!("image: {error}"));
    // A RAW preview image: the decode reports the embedded JPEG's size.
    struct Raw(DecodedImage);
    impl DecodedImageView for Raw {
        fn pixel_width(&self) -> i32 {
            self.0.pixel_width()
        }
        fn pixel_height(&self) -> i32 {
            self.0.pixel_height()
        }
        fn downscaled(&self) -> bool {
            self.0.downscaled()
        }
        fn orientation(&self) -> i32 {
            self.0.orientation()
        }
        fn estimated_bytes(&self) -> i64 {
            self.0.estimated_bytes()
        }
        fn platform_image(&self) -> &PlatformImage {
            self.0.platform_image()
        }
        fn original_width(&self) -> i32 {
            self.0.original_width()
        }
        fn original_height(&self) -> i32 {
            self.0.original_height()
        }
        fn exif(&self) -> Option<&ExifSummary> {
            self.0.exif()
        }
        fn embedded_preview_size(&self) -> Option<(i32, i32)> {
            Some((2000, 1333))
        }
    }

    let info = PhotoInfo::from_image("C:\\photos\\sub\\IMG_0001.CR3", &Raw(image));

    assert_eq!(
        info,
        PhotoInfo {
            file_name: "IMG_0001.CR3".to_owned(),
            width: 6000,
            height: 4000,
            exif: Some(exif),
            raw_preview_width: 2000,
            raw_preview_height: 1333,
        }
    );
    assert!(info.has_raw_preview());
}

#[test]
fn photo_info_without_raw_preview_zeroes_only_the_preview_size() {
    let view = raw_view(2000, 1333, true, (2000, 1333));
    let info = PhotoInfo::from_image("C:\\p\\a.cr3", view.as_ref());

    let plain = info.without_raw_preview();

    assert_eq!((plain.raw_preview_width, plain.raw_preview_height), (0, 0));
    assert_eq!(plain.file_name, info.file_name);
    assert_eq!((plain.width, plain.height), (info.width, info.height));
    assert_eq!(plain.exif, info.exif);
    assert!(!plain.has_raw_preview());
    assert!(info.has_raw_preview(), "the original is untouched");
}

#[test]
fn photo_info_from_non_raw_image_has_zero_preview_size() {
    let view = sized_image(600, 400, 6000, 4000, true);

    let info = PhotoInfo::from_image("C:\\p\\photo.jpg", view.as_ref());

    assert_eq!((info.raw_preview_width, info.raw_preview_height), (0, 0));
    assert!(!info.has_raw_preview());
    assert_eq!((info.width, info.height), (6000, 4000));
    assert_eq!(info.exif, None);
}

// ---- stat_now ---------------------------------------------------------------------------------------------------------------------

fn stat_of_failure(error: io::Error) -> StatOutcome {
    let fs = ScriptedFs::new();
    fs.fail_with("C:\\p\\a.jpg", {
        let kind = error.kind();
        let code = error.raw_os_error();
        move || match code {
            Some(code) => io::Error::from_raw_os_error(code),
            None => io::Error::new(kind, "scripted"),
        }
    });
    stat_now(fs.as_ref(), "C:\\p\\a.jpg").outcome
}

// New test (`StatNow` has no isolated test): one case per row of `stat_error_means_missing`
#[test]
fn stat_now_found_missing_and_error_classification() {
    // A path that can never name a file is as good as missing.
    for kind in [
        io::ErrorKind::NotFound,
        io::ErrorKind::InvalidInput,
        io::ErrorKind::Unsupported,
        io::ErrorKind::InvalidFilename,
    ] {
        assert_eq!(
            stat_of_failure(io::Error::new(kind, "x")),
            StatOutcome::Missing,
            "{kind:?}"
        );
    }
    assert_eq!(
        stat_of_failure(io::Error::from_raw_os_error(206)),
        StatOutcome::Missing,
        "ERROR_FILENAME_EXCED_RANGE"
    );
    // Every other failure keeps the photo: a flaky share never silently loses one.
    assert_eq!(
        stat_of_failure(io::Error::new(io::ErrorKind::PermissionDenied, "x")),
        StatOutcome::Error
    );
    assert_eq!(
        stat_of_failure(io::Error::from_raw_os_error(32)),
        StatOutcome::Error,
        "sharing violation"
    );
    assert_eq!(stat_of_failure(io::Error::other("x")), StatOutcome::Error);

    // `Ok(None)` (also a directory) is Missing; `Ok(Some)` is Found with the stat.
    let root = TempRoot::new("stat-now");
    let file = root.join("a.jpg");
    std::fs::write(&file, [1, 2, 3]).unwrap_or_else(|error| panic!("write: {error}"));
    let found = stat_now(&PhysicalFileSystem, &file.to_string_lossy());
    assert_eq!(found.outcome, StatOutcome::Found);
    assert_eq!(found.stat.map(|stat| stat.length), Some(3));
    assert!(found.error.is_none());
    let directory = stat_now(&PhysicalFileSystem, &root.path().to_string_lossy());
    assert_eq!(directory.outcome, StatOutcome::Missing, "a directory is not a photo");
    let gone = stat_now(&PhysicalFileSystem, &root.join("gone.jpg").to_string_lossy());
    assert_eq!(gone.outcome, StatOutcome::Missing);
    assert!(gone.stat.is_none() && gone.error.is_none());
}

#[test]
fn stat_now_error_outcome_carries_the_error() {
    let fs = ScriptedFs::new();
    fs.fail_with("C:\\p\\flaky.jpg", || io::Error::other("share hiccup"));

    let result = stat_now(fs.as_ref(), "C:\\p\\flaky.jpg");

    assert_eq!(result.outcome, StatOutcome::Error);
    assert_eq!(
        result.error.map(|error| error.to_string()),
        Some("share hiccup".to_owned())
    );
    assert!(result.stat.is_none());
}

// New test (`StatNow` has no isolated test)
#[test]
fn stat_now_never_panics_on_a_blank_path() {
    for path in ["", "   ", "\0"] {
        let result = stat_now(&PhysicalFileSystem, path);
        assert_ne!(result.outcome, StatOutcome::Found, "{path:?}");
    }
    assert_eq!(stat_now(&PhysicalFileSystem, "").outcome, StatOutcome::Missing);
}

#[test]
fn stat_error_means_missing_lists_exactly_the_csharp_catch_filter() {
    assert!(stat_error_means_missing(&io::Error::from(io::ErrorKind::NotFound)));
    assert!(stat_error_means_missing(&io::Error::from_raw_os_error(206)));
    assert!(!stat_error_means_missing(&io::Error::from(
        io::ErrorKind::PermissionDenied
    )));
    assert!(!stat_error_means_missing(&io::Error::from(io::ErrorKind::TimedOut)));
    assert!(!stat_error_means_missing(&io::Error::from_raw_os_error(5)));
}

// ---- expected dimension failures --------------------------------------------------------------------------------------------------------

fn decode(error: DecodeError) -> PreviewError {
    PreviewError::Decode(Arc::new(error))
}

// New test (`TryGetOriginalDimensionsAsync` list is tested through the presenter in 07j): every variant in a table
#[test]
fn expected_dimension_failure_matches_the_csharp_catch_filter() {
    let message = || "m".to_owned();
    // Expected: the C# list (IOException, UnauthorizedAccess, NotSupported, Format, InvalidOperation, InvalidData, External, DecoderBusy) and its I/O siblings.
    let expected: Vec<(&str, PreviewError)> = vec![
        ("io", decode(DecodeError::Io(io::Error::other("io")))),
        (
            "permission denied",
            decode(DecodeError::Io(io::Error::from(io::ErrorKind::PermissionDenied))),
        ),
        (
            "not supported",
            decode(DecodeError::NotSupported {
                message: message(),
                user_text: None,
                inner_hresult: None,
            }),
        ),
        ("missing codec", decode(DecodeError::missing_codec())),
        ("format", decode(DecodeError::Format { message: message() })),
        (
            "invalid operation",
            decode(DecodeError::InvalidOperation { message: message() }),
        ),
        ("invalid data", decode(DecodeError::InvalidData { message: message() })),
        (
            "com",
            decode(DecodeError::Com {
                hresult: -1,
                message: message(),
            }),
        ),
        ("busy", decode(DecodeError::busy())),
        (
            "file not found",
            decode(DecodeError::FileNotFound { message: message() }),
        ),
        (
            "directory not found",
            decode(DecodeError::DirectoryNotFound { message: message() }),
        ),
        ("memory admission", decode(DecodeError::memory_admission())),
        ("preview io", PreviewError::Io(Arc::new(io::Error::other("io")))),
        (
            "source changed",
            PreviewError::SourceChanged {
                path: "p".to_owned(),
                phase: SourceChangePhase::ReadingDimensions,
            },
        ),
        (
            "source bytes io",
            PreviewError::SourceBytes(SourceBytesError::Io(Arc::new(io::Error::other("io")))),
        ),
        (
            "key: source no longer exists",
            PreviewError::Key(CacheKeyError::SourceNoLongerExists {
                full_name: "p".to_owned(),
            }),
        ),
    ];
    for (label, error) in expected {
        assert!(is_expected_dimension_failure(&error), "{label} must be expected");
    }

    // Not expected: they must propagate (a bug must not be hidden as "dimensions unknown").
    let unexpected: Vec<(&str, PreviewError)> = vec![
        ("canceled", PreviewError::Canceled),
        ("decode canceled", decode(DecodeError::Canceled)),
        (
            "invalid argument",
            decode(DecodeError::InvalidArgument { message: message() }),
        ),
        ("invalid cast", decode(DecodeError::InvalidCast { message: message() })),
        ("overflow", decode(DecodeError::Overflow { message: message() })),
        ("out of memory", decode(DecodeError::OutOfMemory { message: message() })),
        ("disposed", decode(DecodeError::Disposed { message: message() })),
        ("dll not found", decode(DecodeError::DllNotFound { message: message() })),
        (
            "bad image format",
            decode(DecodeError::BadImageFormat { message: message() }),
        ),
        (
            "entry point not found",
            decode(DecodeError::EntryPointNotFound { message: message() }),
        ),
        ("other", decode(DecodeError::Other { message: message() })),
        ("panicked", PreviewError::Panicked("boom".to_owned())),
        ("raw full decode key", PreviewError::RawFullDecodeKey),
        (
            "key: invalid path",
            PreviewError::Key(CacheKeyError::InvalidPath(PathError::Argument)),
        ),
        ("spawn", PreviewError::Spawn(Arc::new(io::Error::other("no thread")))),
        (
            "disk entry canceled",
            PreviewError::DiskEntry(Arc::new(PreviewCacheError::Canceled)),
        ),
        ("invalid path", PreviewError::InvalidPath(PathError::Argument)),
    ];
    for (label, error) in unexpected {
        assert!(!is_expected_dimension_failure(&error), "{label} must propagate");
    }
}

#[test]
fn try_get_original_dimensions_returns_the_size_none_for_an_expected_failure_and_the_error_otherwise() {
    let root = TempRoot::new("dims");
    let path = root.join("a.png");
    std::fs::write(&path, [0xFF, 0xD8, 0xFF, 0xD9]).unwrap_or_else(|error| panic!("write: {error}"));
    let path = path.to_string_lossy().into_owned();

    for (make, expected) in [
        (
            (|| DecodeError::InvalidData {
                message: "x".to_owned(),
            }) as fn() -> DecodeError,
            Ok(None),
        ),
        (
            || DecodeError::InvalidArgument {
                message: "x".to_owned(),
            },
            Err(()),
        ),
    ] {
        let decoder = ThrowingInfoDecoder::new(sized_image(10, 10, 10, 10, false), make);
        let service = build_service_with_metrics(
            &root.join("cache"),
            decoder,
            ServiceOptions::default(),
            Arc::new(pr_core::diagnostics::metrics::ReviewMetrics::new()),
        );
        let key = service
            .get_current_cache_key(&path)
            .unwrap_or_else(|error| panic!("key: {error}"));

        let result = try_get_original_dimensions(&service, &path, &key);

        match expected {
            Ok(none) => assert_eq!(result.ok(), Some(none)),
            Err(()) => assert!(result.is_err()),
        }
    }

    // A readable header gives the displayed size.
    let service = build_service_with_metrics(
        &root.join("cache2"),
        support::preview_fakes::SizedDecoder::new() as _,
        ServiceOptions::default(),
        Arc::new(pr_core::diagnostics::metrics::ReviewMetrics::new()),
    );
    let key = service
        .get_current_cache_key(&path)
        .unwrap_or_else(|error| panic!("key: {error}"));
    assert_eq!(
        try_get_original_dimensions(&service, &path, &key).ok(),
        Some(Some((6000, 4000)))
    );
}
