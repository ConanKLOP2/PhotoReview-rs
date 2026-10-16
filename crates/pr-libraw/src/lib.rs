//! Port target: `PhotoReview.Imaging.LibRaw`. Stays dynamically linked (LGPL/CDDL), like the C# P/Invoke build.
//!
//! Wave 5 contracts: every module below is a stub (`todo!()` bodies) pinned by `contracts/W5-15*.md`, `W5-16*.md`, `W5-17.md`
//! and the goldens.

pub mod availability;
pub mod decoder;
pub mod full_decode_gate;
pub mod memory_guard;
pub mod native;
pub mod resampler;
