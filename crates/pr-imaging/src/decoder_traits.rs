//! Port of `PhotoReview.Imaging/Decoding/{DecoderBusyException, DecoderMemoryAdmissionException,
//! MissingImageCodecException, ICancellableImageDecoder, IImageDecoder, IImageDecoderFactory, IPlatformImageCodec}.cs`
//! (C# baseline d92d6aae).
//!
//! Seams only: Wave 4 tier B/C and Wave 5 implement them. The C# exception hierarchy becomes ONE error enum,
//! [`DecodeError`]. `FallbackImageDecoder.IsFallbackable` (W4-12b) matches on its variants; the three
//! decoder-specific refusals (`Busy`, `MemoryAdmission`, plus `Canceled`/`OutOfMemory`/not-found) are NOT
//! fallbackable (INV-12).
//!
//! Amendments applied here (contracts/README.md, A1 and A2): `UserText` closures on `NotSupported`/`MissingCodec`
//! (`not_supported_localized`, `missing_codec_localized`, `describe`, `is_localized`) and the
//! `WithSourceBytes` wrapper (`attach_source_bytes`, `source_bytes`, `kind`).

use std::fmt;
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;

use pr_core::model::enums::DecoderBackend;
use pr_core::sync::CancelToken;

use crate::decode_types::{
    DecodeFailureSourceBytes, DecodeRequest, DecodedImageView, ImageInfo, PixelLease, PlatformImage, SourceBytes,
};
use crate::pixels::PixelBuffer;

/// Default message of `DecoderBusyException()`.
pub const DECODER_BUSY_MESSAGE: &str = "The decoder is busy; this background decode was skipped.";
/// Default message of `DecoderMemoryAdmissionException()`.
pub const DECODER_MEMORY_ADMISSION_MESSAGE: &str = "The decode was refused: not enough memory for the output.";
/// Default message of `MissingImageCodecException()`.
pub const MISSING_IMAGE_CODEC_MESSAGE: &str = "The Windows codec for this image format is not installed.";

// D29: `PlatformImage` is the W4-01b enum (`crate::decode_types::PlatformImage`), not a type alias.

/// A UI sentence that is produced at display time (C# `UserFacingError.Localized` / `Func<string>`), so a language
/// switch applies to an error already held by the status bar. Identical to `pr_l10n::LocalizedText` (D41: no
/// `pr-l10n` dependency here; the composition root injects the closure).
pub type UserText = Arc<dyn Fn() -> String + Send + Sync>;

/// Everything a decoder, the factory or the platform codec can fail with. One variant per C# exception type that
/// the fallback whitelist, the preload loop or a handler distinguishes. `Display` is the C# `Exception.Message`.
pub enum DecodeError {
    /// `DecoderBusyException`: a background decode was refused because the bounded queue is full. Transient: skip now,
    /// retry later; never fall back, never treat the file as corrupt (INV-12).
    Busy { message: String },
    /// `DecoderMemoryAdmissionException` (an `IOException` in C#, see [`DecodeError::is_io_exception`]): the output does
    /// not fit in memory; the file is fine; never fall back (the fallback has no admission).
    MemoryAdmission { message: String },
    /// `MissingImageCodecException` (a `NotSupportedException` in C#, see [`DecodeError::is_not_supported`]): WebP/HEIC/HEIF
    /// without the Windows codec. `message` carries the English guidance; `user_text` the localized one, if any.
    MissingCodec {
        message: String,
        user_text: Option<UserText>,
    },
    /// `FileNotFoundException`: never fallbackable.
    FileNotFound { message: String },
    /// `DirectoryNotFoundException`: never fallbackable.
    DirectoryNotFound { message: String },
    /// `OperationCanceledException`: never fallbackable.
    Canceled,
    /// `OutOfMemoryException`: never fallbackable.
    OutOfMemory { message: String },
    /// `NotSupportedException` (fallbackable). `user_text` is the localized sentence, if the throw site had one (A1).
    NotSupported {
        message: String,
        user_text: Option<UserText>,
    },
    /// `InvalidCastException`, e.g. a failed COM interface cast (fallbackable).
    InvalidCast { message: String },
    /// `OverflowException`, checked size math (fallbackable).
    Overflow { message: String },
    /// `FormatException` / WPF `FileFormatException` (fallbackable).
    Format { message: String },
    /// `InvalidDataException` (fallbackable).
    InvalidData { message: String },
    /// `COMException` with its HRESULT (fallbackable; the WIC "allowed codec errors").
    Com { hresult: i32, message: String },
    /// `DllNotFoundException` (fallbackable).
    DllNotFound { message: String },
    /// `BadImageFormatException` (fallbackable).
    BadImageFormat { message: String },
    /// `EntryPointNotFoundException` (fallbackable).
    EntryPointNotFound { message: String },
    /// `InvalidOperationException`: a programming error, never fallbackable.
    InvalidOperation { message: String },
    /// `ArgumentException`, e.g. `PlatformImageCodec::to_pixels` given a foreign image type.
    InvalidArgument { message: String },
    /// `ObjectDisposedException`, e.g. a disposed `PixelBuffer`.
    Disposed { message: String },
    /// Any other `IOException` (sharing violation, access denied, short read): not fallbackable by the C# whitelist.
    Io(io::Error),
    /// Anything else (C# exception types the whitelist does not name).
    Other { message: String },
    /// A failure that carries the whole source file the primary decoder already read (C# `Exception.Data` key
    /// `PhotoReview.Imaging.SourceBytes`, A2), so the fallback decodes the copy instead of reading the file again.
    /// Transparent: every predicate and the message delegate to `error`; only [`DecodeError::kind`] peels it.
    WithSourceBytes {
        error: Box<DecodeError>,
        bytes: DecodeFailureSourceBytes,
    },
    /// A UI sentence attached to any other variant (C# `UserFacingError.Localized(exception, text)` works on every
    /// exception type: `InvalidDataException`, `FileNotFoundException`, `IOException`, `ArgumentException`...). Transparent
    /// like `WithSourceBytes`: every predicate and the message delegate to `error`; only [`DecodeError::describe`] and
    /// [`DecodeError::kind`] look at the wrapper. Build it with [`DecodeError::with_user_text`].
    Localized { error: Box<DecodeError>, text: UserText },
}

impl fmt::Debug for DecodeError {
    /// `UserText` is a closure and has no `Debug`, so the derive is replaced by this: the variant name and the message.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WithSourceBytes { error, .. } => f.debug_tuple("WithSourceBytes").field(error).finish(),
            Self::Localized { error, .. } => f.debug_tuple("Localized").field(error).finish(),
            other => f
                .debug_struct(other.variant_name())
                .field("message", &other.message())
                .finish(),
        }
    }
}

impl DecodeError {
    /// `new DecoderBusyException()`.
    pub fn busy() -> Self {
        Self::Busy {
            message: DECODER_BUSY_MESSAGE.to_owned(),
        }
    }

    /// `new DecoderBusyException(message)`.
    pub fn busy_with(message: impl Into<String>) -> Self {
        Self::Busy {
            message: message.into(),
        }
    }

    /// `new DecoderMemoryAdmissionException()`.
    pub fn memory_admission() -> Self {
        Self::MemoryAdmission {
            message: DECODER_MEMORY_ADMISSION_MESSAGE.to_owned(),
        }
    }

    /// `new DecoderMemoryAdmissionException(message)`.
    pub fn memory_admission_with(message: impl Into<String>) -> Self {
        Self::MemoryAdmission {
            message: message.into(),
        }
    }

    /// `new MissingImageCodecException()`.
    pub fn missing_codec() -> Self {
        Self::MissingCodec {
            message: MISSING_IMAGE_CODEC_MESSAGE.to_owned(),
            user_text: None,
        }
    }

    /// `new MissingImageCodecException(message)`.
    pub fn missing_codec_with(message: impl Into<String>) -> Self {
        Self::MissingCodec {
            message: message.into(),
            user_text: None,
        }
    }

    /// A `MissingCodec` whose UI sentence is `text` (A1); `message` stays the English log text.
    pub fn missing_codec_localized(message: impl Into<String>, text: UserText) -> Self {
        Self::MissingCodec {
            message: message.into(),
            user_text: Some(text),
        }
    }

    /// A `NotSupported` (C# `NotSupportedException`) whose UI sentence is `text` (A1).
    pub fn not_supported_localized(message: impl Into<String>, text: UserText) -> Self {
        Self::NotSupported {
            message: message.into(),
            user_text: Some(text),
        }
    }

    /// C# `UserFacingError.Localized(exception, text)` for ANY variant: `NotSupported`/`MissingCodec` store the text in
    /// their field, `WithSourceBytes` keeps its place outermost, any other variant is wrapped in
    /// [`DecodeError::Localized`]. A second call replaces the first text (C# `Data[key] = text`).
    #[must_use]
    pub fn with_user_text(self, text: UserText) -> Self {
        match self {
            Self::NotSupported { message, .. } => Self::NotSupported {
                message,
                user_text: Some(text),
            },
            Self::MissingCodec { message, .. } => Self::MissingCodec {
                message,
                user_text: Some(text),
            },
            Self::WithSourceBytes { error, bytes } => Self::WithSourceBytes {
                error: Box::new(error.with_user_text(text)),
                bytes,
            },
            Self::Localized { error, .. } => Self::Localized { error, text },
            other => Self::Localized {
                error: Box::new(other),
                text,
            },
        }
    }

    /// The attached UI sentence closure, if any (outermost wins).
    #[must_use]
    pub fn user_text(&self) -> Option<&UserText> {
        match self {
            Self::NotSupported { user_text, .. } | Self::MissingCodec { user_text, .. } => user_text.as_ref(),
            Self::Localized { text, .. } => Some(text),
            Self::WithSourceBytes { error, .. } => error.user_text(),
            _ => None,
        }
    }

    /// Attaches the whole source file (A2). Replaces bytes attached earlier, as C# `Exception.Data[key] = value`
    /// does. Only a fallbackable failure may be wrapped (C# TurboJpeg rule); the caller decides, this does not check.
    #[must_use]
    pub fn attach_source_bytes(self, bytes: SourceBytes) -> Self {
        let mut source = DecodeFailureSourceBytes::none();
        source.attach(bytes);
        match self {
            Self::WithSourceBytes { error, .. } => Self::WithSourceBytes { error, bytes: source },
            other => Self::WithSourceBytes {
                error: Box::new(other),
                bytes: source,
            },
        }
    }

    /// The source file attached by [`DecodeError::attach_source_bytes`], if any (an attached empty buffer is `Some`).
    #[must_use]
    pub fn source_bytes(&self) -> Option<&SourceBytes> {
        match self {
            Self::WithSourceBytes { error, bytes } => bytes.try_get().or_else(|| error.source_bytes()),
            Self::Localized { error, .. } => error.source_bytes(),
            _ => None,
        }
    }

    /// The error without the [`DecodeError::WithSourceBytes`] wrapper (C# `Exception.Data` does not change the type).
    #[must_use]
    pub fn kind(&self) -> &DecodeError {
        match self {
            Self::WithSourceBytes { error, .. } | Self::Localized { error, .. } => error.kind(),
            other => other,
        }
    }

    /// True when a UI sentence is attached (A1). Inspect with [`DecodeError::describe`] for the text itself.
    #[must_use]
    pub fn is_localized(&self) -> bool {
        self.user_text().is_some()
    }

    /// The text a user sees (A1, C# `UserFacingError.Describe`): the closure's result when one is attached, else
    /// [`DecodeError::message`]. A closure that panics falls back to the message instead of unwinding the caller.
    #[must_use]
    pub fn describe(&self) -> String {
        match self.user_text() {
            Some(text) => {
                let text: &UserText = text;
                panic::catch_unwind(AssertUnwindSafe(|| text())).unwrap_or_else(|_| self.message())
            }
            None => self.message(),
        }
    }

    /// The C# `Exception.Message`.
    pub fn message(&self) -> String {
        match self {
            Self::WithSourceBytes { error, .. } | Self::Localized { error, .. } => error.message(),
            Self::Canceled => "The operation was canceled.".to_owned(),
            Self::Io(error) => error.to_string(),
            Self::Busy { message }
            | Self::MemoryAdmission { message }
            | Self::MissingCodec { message, .. }
            | Self::FileNotFound { message }
            | Self::DirectoryNotFound { message }
            | Self::OutOfMemory { message }
            | Self::NotSupported { message, .. }
            | Self::InvalidCast { message }
            | Self::Overflow { message }
            | Self::Format { message }
            | Self::InvalidData { message }
            | Self::Com { message, .. }
            | Self::DllNotFound { message }
            | Self::BadImageFormat { message }
            | Self::EntryPointNotFound { message }
            | Self::InvalidOperation { message }
            | Self::InvalidArgument { message }
            | Self::Disposed { message }
            | Self::Other { message } => message.clone(),
        }
    }

    /// C# `ex is IOException`: true for `MemoryAdmission` (derives `IOException`), `FileNotFound`,
    /// `DirectoryNotFound` and `Io`. Handlers that treat "failed read/decode" as "cannot show this image" use it.
    pub fn is_io_exception(&self) -> bool {
        match self {
            Self::WithSourceBytes { error, .. } | Self::Localized { error, .. } => error.is_io_exception(),
            _ => matches!(
                self,
                Self::MemoryAdmission { .. } | Self::FileNotFound { .. } | Self::DirectoryNotFound { .. } | Self::Io(_)
            ),
        }
    }

    /// C# `ex is NotSupportedException`: true for `NotSupported` and `MissingCodec`.
    pub fn is_not_supported(&self) -> bool {
        match self {
            Self::WithSourceBytes { error, .. } | Self::Localized { error, .. } => error.is_not_supported(),
            _ => matches!(self, Self::NotSupported { .. } | Self::MissingCodec { .. }),
        }
    }

    /// Variant name for `Debug` only.
    fn variant_name(&self) -> &'static str {
        match self {
            Self::Busy { .. } => "Busy",
            Self::MemoryAdmission { .. } => "MemoryAdmission",
            Self::MissingCodec { .. } => "MissingCodec",
            Self::FileNotFound { .. } => "FileNotFound",
            Self::DirectoryNotFound { .. } => "DirectoryNotFound",
            Self::Canceled => "Canceled",
            Self::OutOfMemory { .. } => "OutOfMemory",
            Self::NotSupported { .. } => "NotSupported",
            Self::InvalidCast { .. } => "InvalidCast",
            Self::Overflow { .. } => "Overflow",
            Self::Format { .. } => "Format",
            Self::InvalidData { .. } => "InvalidData",
            Self::Com { .. } => "Com",
            Self::DllNotFound { .. } => "DllNotFound",
            Self::BadImageFormat { .. } => "BadImageFormat",
            Self::EntryPointNotFound { .. } => "EntryPointNotFound",
            Self::InvalidOperation { .. } => "InvalidOperation",
            Self::InvalidArgument { .. } => "InvalidArgument",
            Self::Disposed { .. } => "Disposed",
            Self::Io(_) => "Io",
            Self::Other { .. } => "Other",
            Self::WithSourceBytes { .. } => "WithSourceBytes",
            Self::Localized { .. } => "Localized",
        }
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::WithSourceBytes { error, .. } | Self::Localized { error, .. } => error.source(),
            _ => None,
        }
    }
}

impl From<io::Error> for DecodeError {
    /// `NotFound` maps to [`DecodeError::FileNotFound`] (C# `FileNotFoundException`); everything else to `Io`.
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::NotFound {
            Self::FileNotFound {
                message: error.to_string(),
            }
        } else {
            Self::Io(error)
        }
    }
}

/// `IImageDecoder`: an image decoding engine. Stateless/shareable (the factory builds each backend once).
pub trait ImageDecoder: Send + Sync {
    /// Decodes the requested image (C# `Decode(DecodeRequest)`).
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError>;

    /// Reads header metadata (dimensions, orientation) without decoding pixel data (C# `ReadInfo`).
    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError>;

    /// Replaces C# `decoder is ICancellableImageDecoder`: `Some(self)` for a decoder that also implements
    /// [`CancellableImageDecoder`]. Wrappers (decorators, `FallbackImageDecoder`) return `None` unless they forward.
    fn as_cancellable(&self) -> Option<&dyn CancellableImageDecoder> {
        None
    }
}

/// `ICancellableImageDecoder`: can stop an in-progress native decode when its request is superseded.
pub trait CancellableImageDecoder: ImageDecoder {
    /// Decodes and observes `token` throughout; a cancelled decode is `Err(DecodeError::Canceled)`.
    fn decode_cancellable(
        &self,
        request: &DecodeRequest,
        token: &CancelToken,
    ) -> Result<Arc<dyn DecodedImageView>, DecodeError>;
}

/// `IImageDecoderFactory`: creates decoders per backend.
pub trait ImageDecoderFactory: Send + Sync {
    /// Resolves the decoder for `backend`, wrapped in the WPF fallback where required. An unregistered backend still
    /// decodes (via the `Wpf` slot) and logs a warning. Repeated calls return the same shared instance.
    fn create(&self, backend: DecoderBackend) -> Arc<dyn ImageDecoder>;

    /// Whether `backend` has an explicit provider registered (Settings uses it to reflect reality to the user).
    fn is_registered(&self, backend: DecoderBackend) -> bool;
}

/// `IPlatformImageCodec` (C-02): converts between pixels and the platform image `IDecodedImage.PlatformImage`
/// carries. Thread-safe; called on decode/persist threads, never on the UI thread in a hot path. Consumers receive it
/// by constructor, never a default.
pub trait PlatformImageCodec: Send + Sync {
    /// `"wpf"` or `"pixels"` (log/metrics only).
    fn name(&self) -> &'static str;

    /// TAKES ownership of `pixels` (WPF: copy then drop; pixels: returned as is).
    fn image_from_pixels(&self, pixels: PixelBuffer) -> Result<PlatformImage, DecodeError>;

    /// For cache encoding; `Err(DecodeError::InvalidArgument)` for a foreign image type, `Err(Disposed)` for a
    /// disposed buffer.
    fn to_pixels<'a>(&self, platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError>;
}
