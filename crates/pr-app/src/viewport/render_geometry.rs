//! Port of `PhotoReview.Shell.Rendering/RenderGeometry.cs` (235 lines, baseline d92d6aae): the pure geometry of the
//! Direct2D draw path. Parts, in C# order: `RenderColors` (dark canvas), `ImageInterpolations`, `ImageTransforms`
//! (EXIF orientation matrix), `PixelRect`, `TileSpec`, `TileLayout` (texture tiling), `DrawGeometry` (intersect,
//! map to destination, pixel snap).
//!
//! Pure: no GPU, no `windows` crate, no `unsafe`. The draw context (`pr_platform_win::render`, W8-12) only consumes
//! the values computed here, so every rule is tested without a device. Contract: `contracts/W8-01c.md`.
//!
//! Stub: every function body is `todo!()`; the signatures are the contract. The porter removes the `allow` below
//! in the same change that removes the last `todo!()` (contracts/README.md D11).

#![allow(dead_code, unused_variables)]

use crate::input::primitives::RectD;
use pr_core::model::enums::ScalingQuality;

// `ImageTransforms.IsTransposed` is `ExifOrientation.IsTransposed` (5..=8) in C# too: reuse, do not re-port.
pub use pr_imaging::orientation::is_transposed;

/// `ColorF(float R, float G, float B, float A)`: straight (non-premultiplied) components in 0..=1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorF {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// `RenderColors.FromRgb(byte r, byte g, byte b, byte a)`: each channel is `channel / 255f` in f32 (float division).
pub fn color_from_rgb(r: u8, g: u8, b: u8, a: u8) -> ColorF {
    todo!()
}

/// `RenderColors.DarkCanvas`: `Dark.Canvas` #101010, opaque (`color_from_rgb(0x10, 0x10, 0x10, 0xFF)`).
pub fn dark_canvas() -> ColorF {
    todo!()
}

/// `ImageInterpolation` (C# `Shell.Rendering/Contracts/RenderSurfaceContracts.cs`). Owned by this unit because
/// `RenderGeometry` produces it; W8-12a (`render::contracts`) imports it from here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageInterpolation {
    NearestNeighbor = 0,
    Linear = 1,
    HighQualityCubic = 2,
}

/// `ImageInterpolations.FromScalingQuality`: `Linear` -> `Linear`, anything else (`HighQuality`) -> `HighQualityCubic`.
pub fn image_interpolation_from_scaling_quality(quality: ScalingQuality) -> ImageInterpolation {
    todo!()
}

/// Row-vector affine matrix, the same layout as `System.Numerics.Matrix3x2`: `x' = x*m11 + y*m21 + m31`,
/// `y' = x*m12 + y*m22 + m32`. All components f32, as in C#.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix3x2 {
    pub m11: f32,
    pub m12: f32,
    pub m21: f32,
    pub m22: f32,
    pub m31: f32,
    pub m32: f32,
}

impl Matrix3x2 {
    pub const IDENTITY: Matrix3x2 = Matrix3x2 {
        m11: 1.0,
        m12: 0.0,
        m21: 0.0,
        m22: 1.0,
        m31: 0.0,
        m32: 0.0,
    };

    /// `new Matrix3x2(m11, m12, m21, m22, m31, m32)`.
    pub const fn new(m11: f32, m12: f32, m21: f32, m22: f32, m31: f32, m32: f32) -> Self {
        Self {
            m11,
            m12,
            m21,
            m22,
            m31,
            m32,
        }
    }

    /// `Matrix3x2.CreateTranslation(x, y)`.
    pub fn translation(x: f32, y: f32) -> Self {
        todo!()
    }

    /// `Matrix3x2.CreateScale(float)`, uniform. Used by the tests and by the draw path (`device = transform * scale(dpi)`).
    pub fn scale(s: f32) -> Self {
        todo!()
    }

    /// `Matrix3x2.CreateRotation(float radians)` (no centre). Used by the tests only.
    pub fn rotation(radians: f32) -> Self {
        todo!()
    }

    /// C# `a * b` (`System.Numerics` operator): the point is transformed by `self` first, then by `rhs`.
    pub fn compose(self, rhs: Matrix3x2) -> Matrix3x2 {
        todo!()
    }

    /// `Vector2.Transform(point, matrix)` in f32: returns `(x', y')`.
    pub fn transform_point(self, x: f32, y: f32) -> (f32, f32) {
        todo!()
    }
}

/// `ImageTransforms.UnorientedRect`: the rect (origin 0,0) to draw the RAW image into before `for_exif_orientation`.
/// Width and height are swapped for orientations 5..=8.
pub fn unoriented_rect(exif_orientation: i32, destination: RectD) -> RectD {
    todo!()
}

/// `ImageTransforms.ForExifOrientation`: the matrix that maps `unoriented_rect` onto `destination`, with the same
/// pixel mapping as `PixelOps.ApplyOrientation`. Values outside 1..=8 are treated as 1 (identity, then translation).
pub fn for_exif_orientation(exif_orientation: i32, destination: RectD) -> Matrix3x2 {
    todo!()
}

/// `PixelRect` (C# `internal readonly record struct`): integer rectangle (tile, upload region).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl PixelRect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }

    /// `X + Width`.
    pub fn right(self) -> i32 {
        todo!()
    }

    /// `Y + Height`.
    pub fn bottom(self) -> i32 {
        todo!()
    }

    /// `Width <= 0 || Height <= 0` (integers: no NaN case).
    pub fn is_empty(self) -> bool {
        todo!()
    }
}

/// `TileSpec`: `texture` = the region of the texture (core plus overlapping gutter); `core` = the part this tile draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileSpec {
    pub texture: PixelRect,
    pub core: PixelRect,
}

/// `TileLayout.DefaultGutter`: enough for cubic (2 px) and pre-shrink of `HighQualityCubic` down to about 1/30.
pub const DEFAULT_GUTTER: i32 = 64;

/// `TileLayout.D3D11MaxTextureDimension` (`D3D11_REQ_TEXTURE2D_U_OR_V_DIMENSION`, feature level 11).
pub const D3D11_MAX_TEXTURE_DIMENSION: i32 = 16384;

/// The `ArgumentOutOfRangeException` of `TileLayout.Compute`, one variant per checked parameter
/// (`ParamName` = `width`, `height`, `maxTextureSize`, `gutter`). The porter adds `Display` and `std::error::Error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileLayoutError {
    /// `width < 1`.
    Width(i32),
    /// `height < 1`.
    Height(i32),
    /// `maxTextureSize < 4`.
    MaxTextureSize(i32),
    /// `gutter < 0`.
    Gutter(i32),
}

/// `TileLayout.Compute(width, height, maxTextureSize, gutter)`. The gutter is first clamped to `max_texture_size / 4`.
/// Tiles are returned rows outer, columns inner (C# order). An image that fits in one texture gives one tile with
/// `texture == core` and no gutter. Pure: allocates only the result vector.
pub fn compute_tiles(
    width: i32,
    height: i32,
    max_texture_size: i32,
    gutter: i32,
) -> Result<Vec<TileSpec>, TileLayoutError> {
    todo!()
}

/// One axis of `TileLayout.Split` (private): `start`/`length` = core span, `tex_start`/`tex_length` = texture span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AxisSpan {
    start: i32,
    length: i32,
    tex_start: i32,
    tex_length: i32,
}

/// `TileLayout.Split` (private). `length <= max`: one span, no gutter. Otherwise `core = max - 2 * gutter` and
/// `count = ceil(length / core)`. Intermediate arithmetic in i64 (see the contract's traps).
fn split_axis(length: i32, max: i32, gutter: i32) -> Vec<AxisSpan> {
    todo!()
}

/// `DrawGeometry`'s epsilon (1e-6, f64) used by `TrySnapPixelExact`.
const EPSILON: f64 = 1e-6;

/// `Math.Max(double, double)`: NaN propagates (.NET Core 3.0+). Not `f64::max`, which ignores NaN.
fn math_max(a: f64, b: f64) -> f64 {
    todo!()
}

/// `Math.Min(double, double)`: NaN propagates. Not `f64::min`.
fn math_min(a: f64, b: f64) -> f64 {
    todo!()
}

/// `DrawGeometry.Intersect(a, b)`: overlap of two rects, or `RectD { 0, 0, 0, 0 }` (C# `default`) when the overlap is
/// empty. A NaN coordinate yields the default rect.
pub fn intersect(a: RectD, b: RectD) -> RectD {
    todo!()
}

/// `DrawGeometry.IsEmpty(rect)` = `!(rect.Width > 0 && rect.Height > 0)`. A NaN width or height counts as empty.
pub fn rect_is_empty(rect: RectD) -> bool {
    todo!()
}

/// `DrawGeometry.ToRectD(PixelRect)`: integer rect to f64 rect.
pub fn pixel_rect_to_rect_d(rect: PixelRect) -> RectD {
    todo!()
}

/// `DrawGeometry.MapToDestination(source, destination, part)`: maps `part` (inside `source`) linearly into
/// `destination`. `sx = dest.w / src.w`, `sy = dest.h / src.h`; the result is `(dest.x + (part.x - src.x) * sx, ...)`.
pub fn map_to_destination(source: RectD, destination: RectD, part: RectD) -> RectD {
    todo!()
}

/// Result of `DrawGeometry.TrySnapPixelExact`: `snapped` is always set (equal to the input destination when `exact`
/// is false); `exact` is the C# `bool` return.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelSnap {
    pub snapped: RectD,
    pub exact: bool,
}

/// `DrawGeometry.TrySnapPixelExact(deviceTransform, destination, source, out snapped)`: true when drawing `source`
/// into `destination` under `device_transform` (local -> device pixels, DPI included) maps one source pixel to one
/// device pixel (axis-aligned scale 1 or 90-degree swap). Then the destination corner is moved by at most 0.5 device
/// pixel onto the pixel grid (inverse of the linear part) and `exact` is true. Every other case returns `exact: false`.
pub fn try_snap_pixel_exact(device_transform: Matrix3x2, destination: RectD, source: RectD) -> PixelSnap {
    todo!()
}
