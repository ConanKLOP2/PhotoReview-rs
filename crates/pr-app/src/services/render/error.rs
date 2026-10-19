//! The exceptions that the C# render contracts document, as one error value (C# throws; Rust returns `Result`).
//!
//! Port of the throw sites in `Shell.Rendering` (`D2DRenderSurface`, `D2DDrawContext`, `GpuImageCache`, `DWriteTextRenderer`);
//! baseline d92d6aae. A COM failure of the implementation (C# `ComInterop.Check(hr)` -> `COMException`) is
//! [`RenderError::Device`]. `ArgumentNullException` has no variant: a Rust reference cannot be null.
//!
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// `ObjectDisposedException`. `object` is the C# full type name
    /// (`"PhotoReview.Shell.Rendering.D2DRenderSurface"`, `"PhotoReview.Shell.Rendering.GpuImageCache"`,
    /// `"PhotoReview.Shell.Rendering.DWriteTextRenderer"`, `"PhotoReview.Imaging.Pixels.PixelBuffer"`).
    Disposed { object: &'static str },
    /// `InvalidOperationException` with the C# message verbatim (for example
    /// `"BeginDraw was already called; call EndDrawAndPresent first."`, `"PopClip without PushClip."`, wrong thread).
    InvalidOperation { message: &'static str },
    /// `ArgumentException` (not the out-of-range subclass): `message` verbatim, `param` the C# parameter name (for example
    /// `"layout"` with `"The text layout was not created by the Direct2D text renderer."`, `"hwnd"` for a zero window handle).
    InvalidArgument { param: &'static str, message: &'static str },
    /// `ArgumentOutOfRangeException`: `param` is the C# parameter name (`"value"` for a negative budget, `"style"` for a font size
    /// that is not a finite positive number).
    ArgumentOutOfRange { param: &'static str },
    /// A failed Direct3D / DXGI / Direct2D / DirectWrite call that is not a device loss (C# `COMException`). `hresult` keeps the
    /// raw `HRESULT` (negative `i32`); `operation` names the call for the log.
    Device { hresult: i32, operation: &'static str },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The texts are the C# messages verbatim (contract "Error texts"); the log reads them.
        match self {
            RenderError::Disposed { object } => {
                write!(f, "Cannot access a disposed object.\r\nObject name: '{object}'.")
            }
            RenderError::InvalidOperation { message } => f.write_str(message),
            RenderError::InvalidArgument { param, message } => {
                write!(f, "{message} (Parameter '{param}')")
            }
            RenderError::ArgumentOutOfRange { param } => write!(
                f,
                "Specified argument was out of the range of valid values. (Parameter '{param}')"
            ),
            RenderError::Device { hresult, operation } => {
                write!(f, "{operation} failed with HRESULT 0x{:08X}", *hresult as u32)
            }
        }
    }
}

impl std::error::Error for RenderError {}
