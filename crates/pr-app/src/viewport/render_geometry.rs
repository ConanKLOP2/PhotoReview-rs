//! Port of `PhotoReview.Shell.Rendering/RenderGeometry.cs` (235 lines, baseline d92d6aae): the pure geometry of the
//! Direct2D draw path. Parts, in C# order: `RenderColors` (dark canvas), `ImageInterpolations`, `ImageTransforms`
//! (EXIF orientation matrix), `PixelRect`, `TileSpec`, `TileLayout` (texture tiling), `DrawGeometry` (intersect,
//! map to destination, pixel snap).
//!
//! Pure: no GPU, no `windows` crate, no `unsafe`. The draw context (`pr_platform_win::render`, W8-12) only consumes
//! the values computed here, so every rule is tested without a device. Contract: `contracts/W8-01c.md`.

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
    ColorF {
        r: f32::from(r) / 255.0_f32,
        g: f32::from(g) / 255.0_f32,
        b: f32::from(b) / 255.0_f32,
        a: f32::from(a) / 255.0_f32,
    }
}

/// `RenderColors.DarkCanvas`: `Dark.Canvas` #101010, opaque (`color_from_rgb(0x10, 0x10, 0x10, 0xFF)`).
pub fn dark_canvas() -> ColorF {
    color_from_rgb(0x10, 0x10, 0x10, 0xFF)
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
    if quality == ScalingQuality::Linear {
        ImageInterpolation::Linear
    } else {
        ImageInterpolation::HighQualityCubic
    }
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
        Self::new(1.0, 0.0, 0.0, 1.0, x, y)
    }

    /// `Matrix3x2.CreateScale(float)`, uniform. Used by the tests and by the draw path (`device = transform * scale(dpi)`).
    pub fn scale(s: f32) -> Self {
        Self::new(s, 0.0, 0.0, s, 0.0, 0.0)
    }

    /// `Matrix3x2.CreateRotation(float radians)` (no centre). Used by the tests only.
    pub fn rotation(radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self::new(cos, sin, -sin, cos, 0.0, 0.0)
    }

    /// C# `a * b` (`System.Numerics` operator): the point is transformed by `self` first, then by `rhs`.
    pub fn compose(self, rhs: Matrix3x2) -> Matrix3x2 {
        let a = self;
        let b = rhs;
        Matrix3x2 {
            m11: a.m11 * b.m11 + a.m12 * b.m21,
            m12: a.m11 * b.m12 + a.m12 * b.m22,
            m21: a.m21 * b.m11 + a.m22 * b.m21,
            m22: a.m21 * b.m12 + a.m22 * b.m22,
            m31: a.m31 * b.m11 + a.m32 * b.m21 + b.m31,
            m32: a.m31 * b.m12 + a.m32 * b.m22 + b.m32,
        }
    }

    /// `Vector2.Transform(point, matrix)` in f32: returns `(x', y')`.
    pub fn transform_point(self, x: f32, y: f32) -> (f32, f32) {
        (
            x * self.m11 + y * self.m21 + self.m31,
            x * self.m12 + y * self.m22 + self.m32,
        )
    }
}

/// `ImageTransforms.UnorientedRect`: the rect (origin 0,0) to draw the RAW image into before `for_exif_orientation`.
/// Width and height are swapped for orientations 5..=8.
pub fn unoriented_rect(exif_orientation: i32, destination: RectD) -> RectD {
    if is_transposed(exif_orientation) {
        RectD {
            x: 0.0,
            y: 0.0,
            width: destination.height,
            height: destination.width,
        }
    } else {
        RectD {
            x: 0.0,
            y: 0.0,
            width: destination.width,
            height: destination.height,
        }
    }
}

/// `ImageTransforms.ForExifOrientation`: the matrix that maps `unoriented_rect` onto `destination`, with the same
/// pixel mapping as `PixelOps.ApplyOrientation`. Values outside 1..=8 are treated as 1 (identity, then translation).
pub fn for_exif_orientation(exif_orientation: i32, destination: RectD) -> Matrix3x2 {
    let raw = unoriented_rect(exif_orientation, destination);
    let w = raw.width as f32;
    let h = raw.height as f32;

    let orient = match exif_orientation {
        2 => Matrix3x2::new(-1.0, 0.0, 0.0, 1.0, w, 0.0),
        3 => Matrix3x2::new(-1.0, 0.0, 0.0, -1.0, w, h),
        4 => Matrix3x2::new(1.0, 0.0, 0.0, -1.0, 0.0, h),
        5 => Matrix3x2::new(0.0, 1.0, 1.0, 0.0, 0.0, 0.0),
        6 => Matrix3x2::new(0.0, 1.0, -1.0, 0.0, h, 0.0),
        7 => Matrix3x2::new(0.0, -1.0, -1.0, 0.0, h, w),
        8 => Matrix3x2::new(0.0, -1.0, 1.0, 0.0, 0.0, w),
        _ => Matrix3x2::IDENTITY,
    };

    orient.compose(Matrix3x2::translation(destination.x as f32, destination.y as f32))
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
        self.x + self.width
    }

    /// `Y + Height`.
    pub fn bottom(self) -> i32 {
        self.y + self.height
    }

    /// `Width <= 0 || Height <= 0` (integers: no NaN case).
    pub fn is_empty(self) -> bool {
        self.width <= 0 || self.height <= 0
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
/// (`ParamName` = `width`, `height`, `maxTextureSize`, `gutter`).
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

impl std::fmt::Display for TileLayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TileLayoutError::Width(v) => write!(f, "width must be at least 1 (got {v})"),
            TileLayoutError::Height(v) => write!(f, "height must be at least 1 (got {v})"),
            TileLayoutError::MaxTextureSize(v) => write!(f, "maxTextureSize must be at least 4 (got {v})"),
            TileLayoutError::Gutter(v) => write!(f, "gutter must not be negative (got {v})"),
        }
    }
}

impl std::error::Error for TileLayoutError {}

/// `TileLayout.Compute(width, height, maxTextureSize, gutter)`. The gutter is first clamped to `max_texture_size / 4`.
/// Tiles are returned rows outer, columns inner (C# order). An image that fits in one texture gives one tile with
/// `texture == core` and no gutter. Pure: allocates only the result vector.
pub fn compute_tiles(
    width: i32,
    height: i32,
    max_texture_size: i32,
    gutter: i32,
) -> Result<Vec<TileSpec>, TileLayoutError> {
    if width < 1 {
        return Err(TileLayoutError::Width(width));
    }
    if height < 1 {
        return Err(TileLayoutError::Height(height));
    }
    if max_texture_size < 4 {
        return Err(TileLayoutError::MaxTextureSize(max_texture_size));
    }
    if gutter < 0 {
        return Err(TileLayoutError::Gutter(gutter));
    }
    let gutter = gutter.min(max_texture_size / 4);

    let columns = split_axis(width, max_texture_size, gutter);
    let rows = split_axis(height, max_texture_size, gutter);
    let mut tiles = Vec::with_capacity(columns.len() * rows.len());
    for row in &rows {
        for column in &columns {
            tiles.push(TileSpec {
                texture: PixelRect::new(column.tex_start, row.tex_start, column.tex_length, row.tex_length),
                core: PixelRect::new(column.start, row.start, column.length, row.length),
            });
        }
    }
    Ok(tiles)
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
    if length <= max {
        return vec![AxisSpan {
            start: 0,
            length,
            tex_start: 0,
            tex_length: length,
        }];
    }

    // DIVERGENCE: C# computes `length + core - 1` in int, which overflows for lengths near i32::MAX and then throws
    // OverflowException. Rust computes the ceiling in i64 and returns the tiles (contract D-W8c-6; no real image
    // reaches that range and no golden case covers it).
    let core = i64::from(max) - 2 * i64::from(gutter);
    let length64 = i64::from(length);
    let count = (length64 + core - 1) / core;
    let mut result = Vec::with_capacity(count as usize);
    for i in 0..count {
        let start = i * core;
        let end = (start + core).min(length64);
        let tex_start = (start - i64::from(gutter)).max(0);
        let tex_end = (end + i64::from(gutter)).min(length64);
        result.push(AxisSpan {
            start: start as i32,
            length: (end - start) as i32,
            tex_start: tex_start as i32,
            tex_length: (tex_end - tex_start) as i32,
        });
    }
    result
}

/// `DrawGeometry`'s epsilon (1e-6, f64) used by `TrySnapPixelExact`.
const EPSILON: f64 = 1e-6;

/// `Math.Max(double, double)`: NaN propagates (.NET Core 3.0+). Not `f64::max`, which ignores NaN.
fn math_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a > b { a } else { b }
}

/// `Math.Min(double, double)`: NaN propagates. Not `f64::min`.
fn math_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a < b { a } else { b }
}

/// `DrawGeometry.Intersect(a, b)`: overlap of two rects, or `RectD { 0, 0, 0, 0 }` (C# `default`) when the overlap is
/// empty. A NaN coordinate yields the default rect.
pub fn intersect(a: RectD, b: RectD) -> RectD {
    let x = math_max(a.x, b.x);
    let y = math_max(a.y, b.y);
    let right = math_min(a.x + a.width, b.x + b.width);
    let bottom = math_min(a.y + a.height, b.y + b.height);
    if right > x && bottom > y {
        RectD {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    } else {
        RectD::default()
    }
}

/// `DrawGeometry.IsEmpty(rect)` = `!(rect.Width > 0 && rect.Height > 0)`. A NaN width or height counts as empty.
pub fn rect_is_empty(rect: RectD) -> bool {
    !(rect.width > 0.0 && rect.height > 0.0)
}

/// `DrawGeometry.ToRectD(PixelRect)`: integer rect to f64 rect.
pub fn pixel_rect_to_rect_d(rect: PixelRect) -> RectD {
    RectD {
        x: f64::from(rect.x),
        y: f64::from(rect.y),
        width: f64::from(rect.width),
        height: f64::from(rect.height),
    }
}

/// `DrawGeometry.MapToDestination(source, destination, part)`: maps `part` (inside `source`) linearly into
/// `destination`. `sx = dest.w / src.w`, `sy = dest.h / src.h`; the result is `(dest.x + (part.x - src.x) * sx, ...)`.
pub fn map_to_destination(source: RectD, destination: RectD, part: RectD) -> RectD {
    let sx = destination.width / source.width;
    let sy = destination.height / source.height;
    RectD {
        x: destination.x + ((part.x - source.x) * sx),
        y: destination.y + ((part.y - source.y) * sy),
        width: part.width * sx,
        height: part.height * sy,
    }
}

/// Result of `DrawGeometry.TrySnapPixelExact`: `snapped` is always set (equal to the input destination when `exact`
/// is false); `exact` is the C# `bool` return.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelSnap {
    pub snapped: RectD,
    pub exact: bool,
}

/// `is_zero(v) = |v| < EPSILON` (C# `IsZero`).
fn is_zero(value: f64) -> bool {
    value.abs() < EPSILON
}

/// `is_unit(v) = | |v| - 1 | < EPSILON` (C# `IsUnit`).
fn is_unit(value: f64) -> bool {
    (value.abs() - 1.0).abs() < EPSILON
}

/// `DrawGeometry.TrySnapPixelExact(deviceTransform, destination, source, out snapped)`: true when drawing `source`
/// into `destination` under `device_transform` (local -> device pixels, DPI included) maps one source pixel to one
/// device pixel (axis-aligned scale 1 or 90-degree swap). Then the destination corner is moved by at most 0.5 device
/// pixel onto the pixel grid (inverse of the linear part) and `exact` is true. Every other case returns `exact: false`.
pub fn try_snap_pixel_exact(device_transform: Matrix3x2, destination: RectD, source: RectD) -> PixelSnap {
    let not_exact = PixelSnap {
        snapped: destination,
        exact: false,
    };
    if rect_is_empty(source) || rect_is_empty(destination) {
        return not_exact;
    }

    let sx = destination.width / source.width;
    let sy = destination.height / source.height;

    // Linear part of diag(sx, sy) * L, all f64. Each f32 entry is widened BEFORE the multiply (C# does the same).
    let m11 = f64::from(device_transform.m11);
    let m12 = f64::from(device_transform.m12);
    let m21 = f64::from(device_transform.m21);
    let m22 = f64::from(device_transform.m22);
    let a = sx * m11;
    let b = sx * m12;
    let c = sy * m21;
    let d = sy * m22;
    let axis_aligned = is_zero(b) && is_zero(c) && is_unit(a) && is_unit(d);
    let swapped = is_zero(a) && is_zero(d) && is_unit(b) && is_unit(c);
    if !axis_aligned && !swapped {
        return not_exact;
    }

    // Device position of the corner, f64 (left-to-right sums).
    let corner_x = (destination.x * m11) + (destination.y * m21) + f64::from(device_transform.m31);
    let corner_y = (destination.x * m12) + (destination.y * m22) + f64::from(device_transform.m32);
    // C# Math.Round(double) is MidpointRounding.ToEven: round_ties_even, not round (trap T1).
    let dx = corner_x.round_ties_even() - corner_x;
    let dy = corner_y.round_ties_even() - corner_y;
    if is_zero(dx) && is_zero(dy) {
        return PixelSnap {
            snapped: destination,
            exact: true,
        };
    }

    // Move the device offset back to local units through the inverse of the device linear part.
    let det = (m11 * m22) - (m12 * m21);
    if det.abs() < EPSILON {
        return not_exact;
    }
    let lx = ((dx * m22) - (dy * m21)) / det;
    let ly = ((dy * m11) - (dx * m12)) / det;
    PixelSnap {
        snapped: RectD {
            x: destination.x + lx,
            y: destination.y + ly,
            width: destination.width,
            height: destination.height,
        },
        exact: true,
    }
}
