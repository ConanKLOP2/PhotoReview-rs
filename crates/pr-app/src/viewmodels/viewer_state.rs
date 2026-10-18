//! Port of `PhotoReview.App.Shared/ViewModels/ViewerState.cs` (450 lines; `public sealed partial class ViewerState : ObservableObject`).
//! Plan row W8-06a, batch B1, contract `contracts/W8-06a.md`.
//!
//! Decisions this file follows (the contract has the full list):
//! - D-W8-1: every C# `[ObservableProperty]` notification becomes a bit in a returned `ViewerChanges` set. A property is in the set
//!   only when its value changed (`f64_same` for doubles: NaN equals NaN, `-0.0` equals `0.0`, as C# `EqualityComparer`).
//!   Dependents named by `[NotifyPropertyChangedFor]` are added whenever their source changed.
//! - The C# `ZoomModeChanged` event becomes `ViewerUpdate::zoom_mode_changed`. The C# `SourceSizeSwapping` event becomes the
//!   `on_swapping` callback of `swap_source_size`, called with the state BEFORE the size changes.
//! - `ViewerStretchMode` is `viewport::types::ViewerStretchMode` (W8-01a, D-W8-01a-2). The C# default of `_stretch` is `Uniform`,
//!   not the `None` default of that enum: `new()` sets it.
//! - `ViewerState` implements the `viewport::fit::FitViewer` seam (W8-01d). The trait method returns `()`, so the impl keeps the
//!   zoom-mode flag in `pending_fit_zoom_mode_changed` (open question OQ-1 in the contract).
#![allow(unused_variables, unused_imports, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use crate::input::primitives::f64_same;
use crate::viewport::fit::FitViewer;
use crate::viewport::types::ViewerStretchMode;
use pr_core::model::enums::{InitialViewMode, ScalingQuality};
use pr_core::settings::app_settings::DEFAULT_CLICK_ZOOM_PERCENT;

/// C# `ViewerState.MinZoom` (click-to-zoom range lower bound, 10 %). `SetZoom` clamps to `[MIN_ZOOM, MAX_ZOOM]`.
pub const MIN_ZOOM: f64 = 0.10;
/// C# `ViewerState.MaxZoom` (800 %).
pub const MAX_ZOOM: f64 = 8.0;
/// C# `ViewerState.MinStepZoom` (25 %): the lowest zoom a step reaches.
pub const MIN_STEP_ZOOM: f64 = 0.25;
/// C# `ViewerState.MaxStepZoom` (400 %): the highest zoom a step reaches.
pub const MAX_STEP_ZOOM: f64 = 4.0;
/// C# `ViewerState.ZoomStep` default (Q-R41: the setting `KeyboardZoomStepPercent` overrides it).
pub const DEFAULT_ZOOM_STEP: f64 = 0.25;
/// C# `ViewerState.ActualSizeZoom` (one source pixel per device pixel, ADR 0008).
pub const ACTUAL_SIZE_ZOOM: f64 = 1.0;

/// C# `ViewerState.MinPlausibleDpiScale` / `MaxPlausibleDpiScale`: outside this closed range a DPI reading is broken (-> 1.0).
const MIN_PLAUSIBLE_DPI_SCALE: f64 = 0.25;
const MAX_PLAUSIBLE_DPI_SCALE: f64 = 16.0;
/// C# `IsFit`: `|Zoom - 1.0| < 0.001` (strict).
const IS_FIT_TOLERANCE: f64 = 0.001;
/// C# `CalculateStepZoom` edge tolerance (`MaxStepZoom - 0.0005` and `MinStepZoom + 0.0005`).
const STEP_EDGE_TOLERANCE: f64 = 0.0005;

/// The `ViewerProperty` names below are the C# property names, used by golden comparisons (`csharp_name`).
/// One variant per C# observable property or computed property that the C# code notifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewerProperty {
    Zoom,
    Stretch,
    SourcePixelWidth,
    SourcePixelHeight,
    DpiScale,
    IsFullscreen,
    ScalingQuality,
    ForceHighQualityScaling,
    MaxImageWidth,
    MaxImageHeight,
    IsFit,
    ImageWidth,
    ImageHeight,
    DisplayZoomPercent,
    EffectiveScalingQuality,
}

impl ViewerProperty {
    /// Every variant, in the fixed order of `ViewerChanges::names`.
    pub const ALL: [ViewerProperty; 15] = [
        ViewerProperty::Zoom,
        ViewerProperty::Stretch,
        ViewerProperty::SourcePixelWidth,
        ViewerProperty::SourcePixelHeight,
        ViewerProperty::DpiScale,
        ViewerProperty::IsFullscreen,
        ViewerProperty::ScalingQuality,
        ViewerProperty::ForceHighQualityScaling,
        ViewerProperty::MaxImageWidth,
        ViewerProperty::MaxImageHeight,
        ViewerProperty::IsFit,
        ViewerProperty::ImageWidth,
        ViewerProperty::ImageHeight,
        ViewerProperty::DisplayZoomPercent,
        ViewerProperty::EffectiveScalingQuality,
    ];

    /// The C# property name (`nameof(ViewerState.X)`), e.g. `"DisplayZoomPercent"`.
    pub fn csharp_name(self) -> &'static str {
        todo!()
    }
}

/// The set of properties a mutating call changed (C# `PropertyChanged` names, as a bit set). `Default` is the empty set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewerChanges(u16);

impl ViewerChanges {
    /// The empty set (nothing changed).
    pub const EMPTY: ViewerChanges = ViewerChanges(0);

    /// Adds one property.
    pub fn insert(&mut self, property: ViewerProperty) {
        todo!()
    }

    /// True when `property` is in the set.
    pub fn contains(self, property: ViewerProperty) -> bool {
        todo!()
    }

    /// True when nothing changed.
    pub fn is_empty(self) -> bool {
        todo!()
    }

    /// Set union.
    pub fn union(self, other: ViewerChanges) -> ViewerChanges {
        todo!()
    }

    /// The C# names of the members of the set, in `ViewerProperty::ALL` order (not in insertion order).
    pub fn names(self) -> Vec<&'static str> {
        todo!()
    }
}

/// What a mutating call did: the properties that changed, and whether the C# `ZoomModeChanged` event would be raised.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewerUpdate {
    /// Properties whose value changed (with their dependents, see the module doc).
    pub changes: ViewerChanges,
    /// C# `ZoomModeChanged` was raised. `SetZoom`, `ResetFit` and `ApplyInitialViewMode` (via them) raise it.
    pub zoom_mode_changed: bool,
}

/// C# `FitAxis` (private enum): which Fit dimension the current zoom came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FitAxis {
    None,
    Width,
    Height,
}

/// C# `ViewerState`: zoom, stretch mode, source size, DPI scale, fullscreen and scaling quality of the viewed image.
/// Single owner (the view model); no shared handles, no observer lists (D-W8-1).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewerState {
    zoom: f64,
    stretch: ViewerStretchMode,
    source_pixel_width: i32,
    source_pixel_height: i32,
    dpi_scale: f64,
    is_fullscreen: bool,
    scaling_quality: ScalingQuality,
    force_high_quality_scaling: bool,
    max_image_width: f64,
    max_image_height: f64,
    zoom_step: f64,
    fit_axis: FitAxis,
    fit_axis_viewport: f64,
    pending_fit_zoom_mode_changed: bool,
}

/// C# `ViewerState.CalculateDisplaySize(originalWidth, originalHeight, zoom, dpiScale)`: the on-screen DIP size of an image of
/// `original_width` x `original_height` source pixels at original-relative `zoom`. `scale = zoom / normalize_dpi(dpi_scale)`.
pub fn calculate_display_size(original_width: i32, original_height: i32, zoom: f64, dpi_scale: f64) -> (f64, f64) {
    todo!()
}

/// C# `ViewerState.CalculateStepZoom(current, step)` (internal static): the zoom one step away, or `None` for "no change".
/// NaN is propagated like C# `Math.Min`/`Math.Max` (NOT `f64::min`, which ignores NaN); see contract trap T3.
pub fn calculate_step_zoom(current: f64, step: f64) -> Option<f64> {
    todo!()
}

impl Default for ViewerState {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewerState {
    /// C# `new ViewerState()` with its field initialisers: zoom 1.0, `Uniform`, size 0 x 0, DPI 1.0, not fullscreen,
    /// `HighQuality`, no force, both viewport limits `+Infinity`, zoom step 0.25.
    pub fn new() -> Self {
        todo!()
    }

    // ---- read-only views (C# get accessors and computed properties) ----

    /// C# `Zoom`: original-relative zoom. In Fit it is 1.0 by convention.
    pub fn zoom(&self) -> f64 {
        todo!()
    }

    /// C# `Stretch`.
    pub fn stretch(&self) -> ViewerStretchMode {
        todo!()
    }

    /// C# `SourcePixelWidth` (0 = unknown).
    pub fn source_pixel_width(&self) -> i32 {
        todo!()
    }

    /// C# `SourcePixelHeight` (0 = unknown).
    pub fn source_pixel_height(&self) -> i32 {
        todo!()
    }

    /// C# `DpiScale` as set (not normalised; `normalize_dpi` applies at the use sites).
    pub fn dpi_scale(&self) -> f64 {
        todo!()
    }

    /// C# `IsFullscreen`.
    pub fn is_fullscreen(&self) -> bool {
        todo!()
    }

    /// C# `ScalingQuality` (the setting).
    pub fn scaling_quality(&self) -> ScalingQuality {
        todo!()
    }

    /// C# `ForceHighQualityScaling` (set by the Refresh command for the image on screen).
    pub fn force_high_quality_scaling(&self) -> bool {
        todo!()
    }

    /// C# `MaxImageWidth` (viewport bound in DIP; `+Infinity` when no Fit bound is set).
    pub fn max_image_width(&self) -> f64 {
        todo!()
    }

    /// C# `MaxImageHeight`.
    pub fn max_image_height(&self) -> f64 {
        todo!()
    }

    /// C# `ZoomStep` (not observable in C#; no change set).
    pub fn zoom_step(&self) -> f64 {
        todo!()
    }

    /// C# `IsFit`: `Stretch == Uniform && |Zoom - 1.0| < 0.001`.
    pub fn is_fit(&self) -> bool {
        todo!()
    }

    /// C# `ImageWidth` (DIP): NaN (auto) in Fit or while the source size is unknown, else `calculate_display_size` at `Zoom`.
    pub fn image_width(&self) -> f64 {
        todo!()
    }

    /// C# `ImageHeight` (DIP); see `image_width`.
    pub fn image_height(&self) -> f64 {
        todo!()
    }

    /// C# `FitZoom`: original-relative zoom that fills the Fit viewport on both axes (0 = unknown).
    pub fn fit_zoom(&self) -> f64 {
        todo!()
    }

    /// C# `FitWidthZoom`: zoom at which the width fills `MaxImageWidth` at the current DPI (0 = unknown).
    pub fn fit_width_zoom(&self) -> f64 {
        todo!()
    }

    /// C# `FitHeightZoom`: zoom at which the height fills `MaxImageHeight` at the current DPI (0 = unknown).
    pub fn fit_height_zoom(&self) -> f64 {
        todo!()
    }

    /// C# `DisplayZoomPercent`: `(int)Math.Round(zoom_or_fit * 100)` with C# default rounding (ties to even, trap T9).
    pub fn display_zoom_percent(&self) -> i32 {
        todo!()
    }

    /// C# `EffectiveZoom`: `None` in Fit, else `Some(Zoom)`.
    pub fn effective_zoom(&self) -> Option<f64> {
        todo!()
    }

    /// C# `EffectiveScalingQuality`: `HighQuality` when `force_high_quality_scaling`, else `scaling_quality`.
    pub fn effective_scaling_quality(&self) -> ScalingQuality {
        todo!()
    }

    // ---- raw property setters (C# generated setters: change only when the value differs, see D-W8-1) ----

    /// C# `Zoom` setter, raw (no clamp, no mode change, no `ZoomModeChanged`). Changes: `Zoom`, `IsFit`, `ImageWidth`,
    /// `ImageHeight`, `DisplayZoomPercent` when the value differs.
    pub fn set_zoom_raw(&mut self, value: f64) -> ViewerChanges {
        todo!()
    }

    /// C# `Stretch` setter. Changes: `Stretch`, `IsFit`, `ImageWidth`, `ImageHeight`, `DisplayZoomPercent` when it differs.
    pub fn set_stretch(&mut self, value: ViewerStretchMode) -> ViewerChanges {
        todo!()
    }

    /// C# `DpiScale` setter. Changes: `DpiScale`, `ImageWidth`, `ImageHeight`, `DisplayZoomPercent` when it differs.
    pub fn set_dpi_scale(&mut self, value: f64) -> ViewerChanges {
        todo!()
    }

    /// C# `MaxImageWidth` setter. Changes: `MaxImageWidth`, `DisplayZoomPercent` when it differs.
    pub fn set_max_image_width(&mut self, value: f64) -> ViewerChanges {
        todo!()
    }

    /// C# `MaxImageHeight` setter. Changes: `MaxImageHeight`, `DisplayZoomPercent` when it differs.
    pub fn set_max_image_height(&mut self, value: f64) -> ViewerChanges {
        todo!()
    }

    /// C# `ScalingQuality` setter. Changes: `ScalingQuality`, `EffectiveScalingQuality` when it differs.
    pub fn set_scaling_quality(&mut self, value: ScalingQuality) -> ViewerChanges {
        todo!()
    }

    /// C# `ForceHighQualityScaling` setter. Changes: `ForceHighQualityScaling`, `EffectiveScalingQuality` when it differs.
    pub fn set_force_high_quality_scaling(&mut self, value: bool) -> ViewerChanges {
        todo!()
    }

    /// C# `IsFullscreen` setter. Changes: `IsFullscreen` when it differs.
    pub fn set_fullscreen(&mut self, value: bool) -> ViewerChanges {
        todo!()
    }

    /// C# `ZoomStep` setter (plain property, no notification, no clamp: negative values are accepted, trap T11).
    pub fn set_zoom_step(&mut self, value: f64) {
        todo!()
    }

    // ---- commands (C# methods) ----

    /// C# `SetSourceSize(width, height, newImage)`. Negative sizes become 0. The Fit-axis memory is forgotten when `new_image` or
    /// the (clamped) size changed. Changes: `SourcePixelWidth`/`SourcePixelHeight` (+ their dependents) when they differ.
    pub fn set_source_size(&mut self, width: i32, height: i32, new_image: bool) -> ViewerUpdate {
        todo!()
    }

    /// C# `SwapSourceSize(width, height)`: the bitmap behind the same image changed its pixel size. When zoomed (not Fit, both old
    /// and new sizes > 0) `on_swapping` is called with the state BEFORE the size changes (C# `SourceSizeSwapping`); the Fit-axis
    /// memory survives and a Fit-width/height zoom is recomputed for the new size. Otherwise it is `set_source_size` without
    /// `new_image`. Same size after clamping: no change and no callback.
    pub fn swap_source_size(
        &mut self,
        width: i32,
        height: i32,
        on_swapping: &mut dyn FnMut(&ViewerState),
    ) -> ViewerUpdate {
        todo!()
    }

    /// C# `SetZoom(value)`: `ZoomStep`-free absolute zoom. NaN is ignored (no change, no event). Otherwise: `Stretch = None`,
    /// both viewport limits to `+Infinity`, the Fit-axis memory is set to `None`, `Zoom = clamp(value, MIN_ZOOM, MAX_ZOOM)`,
    /// `zoom_mode_changed = true`.
    pub fn set_zoom(&mut self, value: f64) -> ViewerUpdate {
        todo!()
    }

    /// C# `ZoomToActualSize()` = `set_zoom(ACTUAL_SIZE_ZOOM)`.
    pub fn zoom_to_actual_size(&mut self) -> ViewerUpdate {
        todo!()
    }

    /// C# `ZoomToFitWidth()`: `set_zoom(fit_width_zoom)` with the Fit-width axis remembering `MaxImageWidth`; no-op when unknown.
    pub fn zoom_to_fit_width(&mut self) -> ViewerUpdate {
        todo!()
    }

    /// C# `ZoomToFitHeight()`: the height twin of `zoom_to_fit_width`.
    pub fn zoom_to_fit_height(&mut self) -> ViewerUpdate {
        todo!()
    }

    /// C# `ZoomIn()` = `StepZoom(ZoomStep)`.
    pub fn zoom_in(&mut self) -> ViewerUpdate {
        todo!()
    }

    /// C# `ZoomOut()` = `StepZoom(-ZoomStep)`.
    pub fn zoom_out(&mut self) -> ViewerUpdate {
        todo!()
    }

    /// C# `WheelZoom(delta)`: `delta > 0` steps up by `ZoomStep`, any other delta (including 0) steps down.
    pub fn wheel_zoom(&mut self, delta: i32) -> ViewerUpdate {
        todo!()
    }

    /// C# `ResetFit(viewportWidth, viewportHeight)`: Fit-axis memory cleared, `Zoom = 1.0`, `Stretch = Uniform`,
    /// `update_viewport(w, h, force: true)`, and `zoom_mode_changed = true` ALWAYS (even when nothing changed).
    pub fn reset_fit(&mut self, viewport_width: f64, viewport_height: f64) -> ViewerUpdate {
        todo!()
    }

    /// C# `UpdateViewport(viewportWidth, viewportHeight, force)`. First, when the Fit-axis memory is set and both readings are
    /// usable (> 1 and finite), the memory takes the dimension it fills. Then, unless `force` or `Stretch == Uniform`, nothing else.
    /// Otherwise a usable reading sets both viewport limits. `zoom_mode_changed` is never set here.
    pub fn update_viewport(&mut self, viewport_width: f64, viewport_height: f64, force: bool) -> ViewerUpdate {
        todo!()
    }

    /// C# `ApplyInitialViewMode(mode, viewportWidth, viewportHeight, clickZoomPercent, keepZoomAcrossImages)`.
    /// Returns `None` (nothing done) when `keep_zoom_across_images`; otherwise `Some(update)`. Mode table in the contract.
    /// `DEFAULT_CLICK_ZOOM_PERCENT` is the value the C# default parameter takes; the caller passes it explicitly.
    pub fn apply_initial_view_mode(
        &mut self,
        mode: InitialViewMode,
        viewport_width: f64,
        viewport_height: f64,
        click_zoom_percent: i32,
        keep_zoom_across_images: bool,
    ) -> Option<ViewerUpdate> {
        todo!()
    }

    /// C# `ToggleFullscreen()`: `IsFullscreen = !IsFullscreen`.
    pub fn toggle_fullscreen(&mut self) -> ViewerUpdate {
        todo!()
    }

    /// C# `ExitFullscreen()`: `IsFullscreen = false` (no change when already false).
    pub fn exit_fullscreen(&mut self) -> ViewerUpdate {
        todo!()
    }

    /// True when a `FitViewer::reset_fit` call raised `ZoomModeChanged` since the last call of this method. The Fit driver (W8-01d)
    /// cannot see the `ViewerUpdate` through the `()` trait method, so the shell calls this after `apply_fit_start`/`apply_fit_resume`.
    pub fn take_pending_zoom_mode_changed(&mut self) -> bool {
        todo!()
    }
}

impl FitViewer for ViewerState {
    /// C# `ViewerState.ResetFit(viewportWidth, viewportHeight)` through the W8-01d seam. Calls the inherent `reset_fit` (inherent
    /// methods take precedence on `ViewerState::reset_fit`) and records its `zoom_mode_changed` in `pending_fit_zoom_mode_changed`.
    fn reset_fit(&mut self, viewport_width: f64, viewport_height: f64) {
        todo!()
    }
}

/// C# `NormalizeDpi`: `dpi_scale` when it is in `[MIN_PLAUSIBLE_DPI_SCALE, MAX_PLAUSIBLE_DPI_SCALE]`, else 1.0 (NaN too).
/// The other private helpers (`set_zoom_core`, `step_zoom`, `step_base`, `clear_fit_axis`, `fit_zoom_for`, the NaN-propagating
/// min/max) are the porter's to add; the contract's "Private helpers" section names them and their C# origin.
#[allow(dead_code)] // stub: the porter removes this allow with the last todo!() (contracts/README.md D11)
fn normalize_dpi(dpi_scale: f64) -> f64 {
    todo!()
}
