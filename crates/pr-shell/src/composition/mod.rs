//! Composition (Wave 8 batch B2; this index is owned by W8-09a). No DI container: explicit builders and plain functions.
//! W8-09b (batch B6, `contracts/W8-09b.md`) added the modules below the first block.

pub mod contracts;
pub mod decoders;
pub mod factories;

// W8-09b: the shared service graph (`registry`, `main_view_model`; the plan row), the production text/format adapters, the OS seams and the glue to the shell start-up (`graph`).
pub mod adapters;
/// The production `GraphBuilder` of 17d (needs the shell host services, so Windows only like `startup`).
#[cfg(windows)]
pub mod graph;
pub mod main_view_model;
pub mod platform;
pub mod registry;
pub mod texts;
