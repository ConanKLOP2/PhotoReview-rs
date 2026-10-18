//! Render contracts (lead-owned index).

pub mod error;
pub mod gpu_image;
pub mod surface;
pub mod text;
pub mod tiled;

pub use crate::viewport::render_geometry::{ColorF, ImageInterpolation};
pub use error::RenderError;
pub use gpu_image::{GpuImage, GpuImageCache};
pub use surface::{
    DrawContext, OrientedDrawContext, PresentResult, RenderSurface, RenderSurfaceFactory, RenderSurfaceOptions,
};
pub use text::{TextLayout, TextRenderer, TextStyle, TextTrimming};
