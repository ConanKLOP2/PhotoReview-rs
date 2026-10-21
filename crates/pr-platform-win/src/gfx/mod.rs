//! `pr_platform_win::gfx`: thin graphics interop helpers over `windows-rs` (Wave 8 unit 11, batch B3). Lead-owned index.
//!
//! Replaces `PhotoReview.Shell.Interop/Graphics/*` (1 186 lines of hand-written COM vtables plus 280 lines of helpers). The `windows`
//! crate generates every interface, so only the custom parts are ported; the audit of the rest is in `contracts/W8-11g.md` and
//! `contracts/W8-11h.md`. All `unsafe` of the graphics layer lives in these files, each block with `// SAFETY:`.
//!
//! No `pr_app` type is used here (`pr-app` depends on this crate, D-W8B1-1); window handles cross the boundary as `HWND`
//! (callers convert from `isize` with [`com::hwnd`]).

pub mod com;
pub mod d2d;
pub mod d3d11;
pub mod dwrite;
pub mod dxgi;
pub mod extras;
pub mod guids;
