//! `FileActionController` (C# `PhotoReview.App.Shared/Coordinators/FileActionController.cs`, 858 lines), split over four files by
//! contract: `recycle` (W8-07c, owns the struct and the constructor), `actions` and `undo` (W8-07d), `move_copy` (W8-07e).

pub mod actions;
pub mod move_copy;
pub mod recycle;
pub mod undo;
