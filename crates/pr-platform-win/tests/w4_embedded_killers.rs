//! Platform expansion of the shared CX-IMAGING3 killers; no reverse crate dependency.
#![cfg(windows)]
#[path = "../../pr-imaging/tests/w4_embedded_killers.rs"]
mod shared_killers;
wic_cache_boundary_killers!();
