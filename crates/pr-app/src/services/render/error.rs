//! The exceptions that the C# render contracts document, as one error value (C# throws; Rust returns `Result`).
//!
//! Port of the throw sites in `Shell.Rendering` (`D2DRenderSurface`, `D2DDrawContext`, `GpuImageCache`, `DWriteTextRenderer`);
//! baseline d92d6aae. A COM failure of the implementation (C# `ComInterop.Check(hr)` -> `COMException`) is
//! [`RenderError::Device`]. `ArgumentNullException` has no variant: a Rust reference cannot be null.
//!
//! Stub: the `Display` body is `todo!()` (the texts are part of the contract, see the contract's "Error texts").

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

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
        todo!()
    }
}

impl std::error::Error for RenderError {}
