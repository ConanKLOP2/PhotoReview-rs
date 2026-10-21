//! GPU image cache (`GpuImageCache.cs`, 414 lines, baseline `d92d6aae`), split at the class boundary (D-W8B3S-1):
//! [`policy`] (W8-12f) is the pure budget / LRU / stripe-scheduling class `GpuImageCache` (lines 1-300) over the
//! [`policy::GpuImageBackend`] seam; [`upload`] (W8-12g) is `IGpuImageBackend` + `D2DGpuImageBackend` (lines 301-414): tiles, bitmap
//! upload, release.
//!
//! The concrete cache that `pr_app::services::render::GpuImageCache` (and `PrefetchStatus`) is implemented for is
//! [`upload::D2dGpuImageCache`], an adapter around `BudgetedImageCache<D2dGpuImageBackend<..>>`.

pub mod policy;
pub mod upload;
