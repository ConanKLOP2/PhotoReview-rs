//! `pr_platform_win::render`: the Direct2D renderer of `PhotoReview.Shell.Rendering` (Wave 8 unit 12, batch B3). Lead-owned index.
//!
//! The render TRAITS (`RenderSurface`, `DrawContext`, `GpuImage`, `RenderError`, ...) live in `pr_app::services::render` (W8-12a, D-W8-3);
//! this module is their Direct2D implementation. While `pr-app` still depends on `pr-platform-win` (D-W8B1-1) the types here use the
//! interim std/`windows`-only types documented in `contracts/W8-12b.md` .. `W8-12i.md` (decisions D-W8B3R-1, D-W8B3S-4).
//!
//! 12j (swap-chain latency experiment) is contract-only and has no module here.

pub mod device;
pub mod draw_context;
pub mod image_cache;
pub mod keep_alive;
pub mod surface;
pub mod text;
