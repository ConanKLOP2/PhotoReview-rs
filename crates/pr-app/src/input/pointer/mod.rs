//! Pointer input controller (Wave 8 batch B2; this index is owned by the W8-05g/05h contracts, the lead only adds
//! `pub mod pointer;` to `input/mod.rs`).
//!
//! Port of `PhotoReview.App.Shared/Input/PointerInputController.cs` (672 lines), split over four modules by method
//! boundary (contracts `W8-05g.md` and `W8-05h.md`). One struct, [`PointerInputController`], defined in `wheel`; the other
//! files add `impl PointerInputController` blocks.

pub mod click_zoom;
pub mod pan;
pub mod wheel;
pub mod zoom;

pub use pan::PointerRelease;
pub use wheel::{PointerCommands, PointerContext, PointerInputController};
pub use zoom::{PointerContinuation, PointerStep, ZoomAction};
