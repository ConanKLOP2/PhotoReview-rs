//! Port of `PhotoReview.Imaging/Decoding/{DecoderBusyException, DecoderMemoryAdmissionException,
//! MissingImageCodecException, ICancellableImageDecoder, IImageDecoder, IImageDecoderFactory, IPlatformImageCodec}.cs`
//! (C# baseline d92d6aae).
//!
//! Seams only: Wave 4 tier B/C and Wave 5 implement them. The C# exception hierarchy becomes ONE error enum,
//! [`DecodeError`]. `FallbackImageDecoder.IsFallbackable` (W4-12b) matches on its variants; the three
//! decoder-specific refusals (`Busy`, `MemoryAdmission`, plus `Canceled`/`OutOfMemory`/not-found) are NOT
//! fallbackable (INV-12).

use std::fmt;
use std::io;
use std::sync::Arc;

use pr_core::model::enums::DecoderBackend;
use pr_core::sync::CancelToken;

use crate::decode_types::{DecodeRequest, DecodedImageView, ImageInfo, PixelLease, PlatformImage};
use crate::pixels::PixelBuffer;

/// Default message of `DecoderBusyException()`.
pub const DECODER_BUSY_MESSAGE: &str = "The decoder is busy; this background decode was skipped.";
/// Default message of `DecoderMemoryAdmissionException()`.
pub const DECODER_MEMORY_ADMISSION_MESSAGE: &str = "The decode was refused: not enough memory for the output.";
/// Default message of `MissingImageCodecException()`.
pub const MISSING_IMAGE_CODEC_MESSAGE: &str = "The Windows codec for this image format is not installed.";

// D29: `PlatformImage` is the W4-01b enum (`crate::decode_types::PlatformImage`), not a type alias.

/// Everything a decoder, the factory or the platform codec can fail with. One variant per C# exception type that
/// the fallback whitelist, the preload loop or a handler distinguishes. `Display` is the C# `Exception.Message`.
#[derive(Debug)]
pub enum DecodeError {
    /// `DecoderBusyException`: a background decode was refused because the bounded queue is full. Transient: skip now,
    /// retry later; never fall back, never treat the file as corrupt (INV-12).
    Busy { message: String },
    /// `DecoderMemoryAdmissionException` (an `IOException` in C#, see [`DecodeError::is_io_exception`]): the output does
    /// not fit in memory; the file is fine; never fall back (the fallback has no admission).
    MemoryAdmission { message: String },
    /// `MissingImageCodecException` (a `NotSupportedException` in C#, see [`DecodeError::is_not_supported`]): WebP/HEIC/HEIF
    /// without the Windows codec. `message` carries the localized install guidance.
    MissingCodec { message: String },
    /// `FileNotFoundException`: never fallbackable.
    FileNotFound { message: String },
    /// `DirectoryNotFoundException`: never fallbackable.
    DirectoryNotFound { message: String },
    /// `OperationCanceledException`: never fallbackable.
    Canceled,
    /// `OutOfMemoryException`: never fallbackable.
    OutOfMemory { message: String },
    /// `NotSupportedException` (fallbackable).
    NotSupported { message: String },
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
        }
    }

    /// `new MissingImageCodecException(message)`.
    pub fn missing_codec_with(message: impl Into<String>) -> Self {
        Self::MissingCodec {
            message: message.into(),
        }
    }

    /// The C# `Exception.Message`.
    pub fn message(&self) -> String {
        match self {
            Self::Canceled => "The operation was canceled.".to_owned(),
            Self::Io(error) => error.to_string(),
            Self::Busy { message }
            | Self::MemoryAdmission { message }
            | Self::MissingCodec { message }
            | Self::FileNotFound { message }
            | Self::DirectoryNotFound { message }
            | Self::OutOfMemory { message }
            | Self::NotSupported { message }
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
        matches!(
            self,
            Self::MemoryAdmission { .. } | Self::FileNotFound { .. } | Self::DirectoryNotFound { .. } | Self::Io(_)
        )
    }

    /// C# `ex is NotSupportedException`: true for `NotSupported` and `MissingCodec`.
    pub fn is_not_supported(&self) -> bool {
        matches!(self, Self::NotSupported { .. } | Self::MissingCodec { .. })
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
