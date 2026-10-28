//! Port of `PhotoReview.App.Shared/Coordinators/ImagePresenter.cs` lines 839 to 863, 895 to 925 and 927 to 949
//! (baseline d92d6aae): `StatOutcome`, `StatResult`, `StatNow`, `TryGetOriginalDimensionsAsync` (blocking form) and the
//! `PhotoInfo` record with `PhotoInfo.From`. Contract: `contracts/W8-07k.md`. Target:
//! `crates/pr-app/src/coordinators/presenter/photo_info.rs` (`coordinators::presenter::photo_info`).
//!
//! Pure module: no `unsafe`, no `windows` crate. Everything here is a plain function or value; the presenter calls the two
//! blocking functions from worker jobs (never on the UI thread, Q-R29 option C).

use std::io;
use std::path::Path;
use std::sync::Arc;

use pr_core::abstractions::{FileStat, FileSystem};
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::get_file_name;
use pr_imaging::cache_key::ImageCacheKey;
use pr_imaging::decode_types::DecodedImageView;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::exif_summary::ExifSummary;
use pr_imaging::service::{PreviewError, PreviewImageService};

use crate::misc::app_log::AppLog;

use super::state::{ImagePresenter, PresenterEvent, StatPurpose};

/// C# `private enum StatOutcome { Found, Missing, Error }` (made visible to the presenter's other files).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatOutcome {
    /// The file exists; `StatResult::stat` is set.
    Found,
    /// Not there, or a path that can never name a file: the catalog drops the item.
    Missing,
    /// Any other failure (share hiccup, access denied, I/O error): the item stays in the catalog; `StatResult::error` is set.
    Error,
}

/// C# `private readonly record struct StatResult(StatOutcome Outcome, FileStat? Stat, Exception? Error)`.
#[derive(Debug)]
pub struct StatResult {
    pub outcome: StatOutcome,
    /// Set for `Found` only.
    pub stat: Option<FileStat>,
    /// Set for `Error` only (C# `Exception?`); `Display` is the C# `Exception.Message`.
    pub error: Option<io::Error>,
}

impl StatResult {
    /// `new StatResult(Found, stat, null)`.
    pub fn found(stat: FileStat) -> Self {
        Self {
            outcome: StatOutcome::Found,
            stat: Some(stat),
            error: None,
        }
    }

    /// `new StatResult(Missing, null, null)`.
    pub fn missing() -> Self {
        Self {
            outcome: StatOutcome::Missing,
            stat: None,
            error: None,
        }
    }

    /// `new StatResult(Error, null, ex)`.
    pub fn failed(error: io::Error) -> Self {
        Self {
            outcome: StatOutcome::Error,
            stat: None,
            error: Some(error),
        }
    }
}

/// C# `StatNow(path)` (lines 901 to 925): ONE stat through `file_system.try_get_file_stat` (the Rust image of C#
/// `IFileSystem.GetFileStat` that may throw; the caller passes `PhysicalFileSystem` when the presenter has no injected file
/// system, C# `new FileInfo(path)`). `Ok(Some)` -> `Found`; `Ok(None)` -> `Missing` (also a directory, as `FileInfo.Exists`);
/// `Err(e)` -> `Missing` when [`stat_error_means_missing`] is true, else `Error` carrying `e`. Never panics, never throws.
/// Blocking: call it only on a worker.
pub fn stat_now(file_system: &dyn FileSystem, path: &str) -> StatResult {
    match file_system.try_get_file_stat(Path::new(path)) {
        Ok(Some(stat)) => StatResult::found(stat),
        Ok(None) => StatResult::missing(),
        Err(error) if stat_error_means_missing(&error) => StatResult::missing(),
        Err(error) => StatResult::failed(error),
    }
}

/// The `catch (Exception ex) when (ex is FileNotFoundException or DirectoryNotFoundException or ArgumentException or
/// NotSupportedException or PathTooLongException)` filter of `StatNow`: a path that can never name a file is as good as missing.
/// Rust form: `ErrorKind::NotFound`, `ErrorKind::InvalidInput` (blank / malformed path, C# `ArgumentException`),
/// `ErrorKind::Unsupported` (C# `NotSupportedException`), `ErrorKind::InvalidFilename` and raw OS error 206
/// (`ERROR_FILENAME_EXCED_RANGE`, C# `PathTooLongException`, see `pr_core::io::physical`). Access denied, sharing violations and
/// every other kind are `Error`.
pub fn stat_error_means_missing(error: &io::Error) -> bool {
    /// `ERROR_FILENAME_EXCED_RANGE`: the C# `PathTooLongException`.
    const ERROR_FILENAME_EXCED_RANGE: i32 = 206;
    matches!(
        error.kind(),
        io::ErrorKind::NotFound
            | io::ErrorKind::InvalidInput
            | io::ErrorKind::Unsupported
            | io::ErrorKind::InvalidFilename
    ) || error.raw_os_error() == Some(ERROR_FILENAME_EXCED_RANGE)
}

impl ImagePresenter {
    /// C# `StatOffUiThreadAsync(path, cancellationToken)` (lines 886 to 893): `stat_now(file_system, path)` on `self.stat_worker` (the dedicated FIFO stat thread of
    /// `pr_core::io::navigation_stat`), NEVER on the calling (UI) thread. The job sends `PresenterEvent::Stat { token, purpose, result }` into the presenter inbox. A
    /// `cancel` that fires before the job starts drops it without touching the disk; the closure-owned guard sends a narrow abandonment completion so the canceled
    /// await can retire without treating the path as missing. `purpose` tells `pump` which continuation resumes.
    pub(super) fn submit_stat(&self, token: i64, path: &str, purpose: StatPurpose, cancel: &CancelToken) {
        let file_system = Arc::clone(&self.file_system);
        let inbox = self.inbox.clone();
        let path = path.to_owned();
        // The worker runs the job itself and sends the event from its own thread; the task handle is not waited on.
        let completion = StatDropCompletion {
            inbox: inbox.clone(),
            token,
            purpose,
            armed: true,
        };
        drop(self.stat_worker.run(
            move || {
                let result = stat_now(file_system.as_ref(), &path);
                let mut completion = completion;
                completion.armed = false;
                inbox.send(PresenterEvent::Stat { token, purpose, result });
            },
            cancel,
        ));
    }
}

/// Reports completion when NavigationStatWorker drops queued work after cancellation. Capturing this guard by the entire job
/// closure is essential: `run` drops that closure both for an already-canceled token and when a queued token is canceled.
struct StatDropCompletion {
    inbox: crate::coordinators::zoom_detail::CompletionInbox<super::state::PresenterEvent>,
    token: i64,
    purpose: StatPurpose,
    armed: bool,
}

impl Drop for StatDropCompletion {
    fn drop(&mut self) {
        if self.armed {
            self.inbox.send(super::state::PresenterEvent::StatAbandoned {
                token: self.token,
                purpose: self.purpose,
                panicked: std::thread::panicking(),
            });
        }
    }
}

/// C# `TryGetOriginalDimensionsAsync(path, currentKey)` (lines 848 to 863), blocking: `service.get_original_dimensions(path, key)`.
/// `Ok(Some((w, h)))` on success. An EXPECTED header-read failure ([`is_expected_dimension_failure`]) is `Ok(None)` and is logged
/// with `AppLog` (`ShowImage original dimensions unknown path=..: <kind>: <message>`) when logging is enabled. Any other error is
/// returned (`Err`): the C# code lets a bug such as a `NullReferenceException` or an `ArgumentException` surface, and the presenter
/// turns it into the generic failure branch (the status is NOT reported as ready).
pub fn try_get_original_dimensions(
    service: &PreviewImageService,
    path: &str,
    key: &ImageCacheKey,
) -> Result<Option<(i32, i32)>, PreviewError> {
    match service.get_original_dimensions(path, key) {
        Ok(dimensions) => Ok(Some(dimensions)),
        // The expected header-read failures only (I/O, unsupported/corrupt/undecodable file, busy decoder); a bug must surface instead of
        // being turned into "dimensions unknown".
        Err(error) if is_expected_dimension_failure(&error) => {
            let log = AppLog::global();
            if log.enabled() {
                log.info(&format!(
                    "ShowImage original dimensions unknown path={path}: {}: {error}",
                    error_kind_name(&error)
                ));
            }
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// The C# `ex.GetType().Name` of a log line: the `io::ErrorKind` or the variant name.
fn error_kind_name(error: &PreviewError) -> String {
    let debug = match error {
        PreviewError::Io(error) => format!("{:?}", error.kind()),
        PreviewError::Decode(error) => match error.kind() {
            DecodeError::Io(error) => format!("{:?}", error.kind()),
            other => format!("{other:?}"),
        },
        other => format!("{other:?}"),
    };
    debug.chars().take_while(|c| c.is_alphanumeric()).collect()
}

/// The C# filter `ex is IOException or UnauthorizedAccessException or NotSupportedException or FormatException or
/// InvalidOperationException or InvalidDataException or ExternalException or DecoderBusyException`, applied to the Rust error:
/// - `PreviewError::is_io_exception()` (covers `SourceChanged`, `Io`, `SourceBytes(Io)`, `Key(SourceNoLongerExists)`, `Decode(e)` with `e.is_io_exception()`);
/// - `PreviewError::is_busy()`;
/// - `Decode(e)` where `e.kind()` is `NotSupported`, `MissingCodec`, `Format`, `InvalidData`, `Com`, `InvalidOperation`
///   (`UnauthorizedAccess` has no Rust variant: it arrives as `Io` with `ErrorKind::PermissionDenied`, already covered by the first rule).
///
/// NOT expected (they must propagate): `Canceled`, `Decode(InvalidArgument | InvalidCast | Overflow | OutOfMemory | Disposed | DllNotFound | BadImageFormat | EntryPointNotFound | Other | Canceled)`,
/// `Panicked`, `RawFullDecodeKey`, `Key(InvalidPath)`, `Spawn`.
pub fn is_expected_dimension_failure(error: &PreviewError) -> bool {
    if error.is_io_exception() || error.is_busy() {
        return true;
    }
    match error {
        PreviewError::Decode(decode) => {
            decode.is_not_supported()
                || matches!(
                    decode.kind(),
                    DecodeError::Format { .. }
                        | DecodeError::InvalidData { .. }
                        | DecodeError::Com { .. }
                        | DecodeError::InvalidOperation { .. }
                )
        }
        _ => false,
    }
}

/// C# `public sealed record PhotoInfo(string FileName, int Width, int Height, ExifSummary? Exif, int RawPreviewWidth = 0,
/// int RawPreviewHeight = 0)`: the input of the photo information line. `raw_preview_*` (Q-RAW-03) is the pixel size of the
/// embedded RAW JPEG while THAT is the displayed image; 0 = none (non-RAW, a cache-restored image that cannot tell, or the full
/// decode is shown). Value equality (C# record).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhotoInfo {
    pub file_name: String,
    pub width: i32,
    pub height: i32,
    pub exif: Option<ExifSummary>,
    pub raw_preview_width: i32,
    pub raw_preview_height: i32,
}

impl PhotoInfo {
    /// C# `PhotoInfo.From(path, image)`: `file_name = Path.GetFileName(path)` (`pr_core::text::dotnet_path::get_file_name`),
    /// `width/height = image.original_width()/original_height()`, `exif = image.exif().cloned()`,
    /// `raw_preview_* = image.embedded_preview_size()` or `(0, 0)` when it is `None`.
    pub fn from_image(path: &str, image: &dyn DecodedImageView) -> PhotoInfo {
        let (raw_preview_width, raw_preview_height) = image.embedded_preview_size().unwrap_or((0, 0));
        PhotoInfo {
            file_name: get_file_name(path).to_owned(),
            width: image.original_width(),
            height: image.original_height(),
            exif: image.exif().cloned(),
            raw_preview_width,
            raw_preview_height,
        }
    }

    /// C# `info with { RawPreviewWidth = 0, RawPreviewHeight = 0 }`.
    #[must_use]
    pub fn without_raw_preview(&self) -> PhotoInfo {
        PhotoInfo {
            raw_preview_width: 0,
            raw_preview_height: 0,
            ..self.clone()
        }
    }

    /// C# pattern `{ RawPreviewWidth: > 0 }`.
    pub fn has_raw_preview(&self) -> bool {
        self.raw_preview_width > 0
    }
}
