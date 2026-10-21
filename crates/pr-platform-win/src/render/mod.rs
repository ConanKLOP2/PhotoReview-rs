//! `pr_platform_win::render`: Direct2D / DirectWrite implementations of the `pr_app::services::render` contracts (Wave 8 unit 12,
//! batch B3). Lead-owned index; each agent lists only its own modules, alphabetical.
//!
//! Declared by the render group 12f-12j (this file): `image_cache` (12f policy + 12g upload), `keep_alive` (12h), `text` (12i).
//! The group 12b-12e (`device`, `draw_context`, `surface`) is declared by the other contract writer; the merge adds those lines.
//! W8-12j (swap-chain latency experiment) is contract-only and has no module here.
//!
//! Layering (D-W8B1-1): the stubs of this group compile against the CURRENT crate graph (`pr-app` depends on `pr-platform-win`), so
//! they use `std`, `pr_imaging`, `pr_core` and `crate::gfx` types only and name the `pr_app` types they will implement in their docs.
//! After the layering PR the porters add the trait impls listed in each contract.

pub mod image_cache;
pub mod keep_alive;
pub mod text;
