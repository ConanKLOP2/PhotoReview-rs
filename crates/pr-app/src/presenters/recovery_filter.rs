//! Port of `PhotoReview.App.Shared/Presenters/RecoveryFilter.cs` (23 lines): the verdict filter of the Recovery window
//! (`RecoveryFilter`, order = ComboBox items) and the colour hint of a verdict or path status (`RecoveryTone`, WP-35 G0).
//! Contract: `contracts/W8-10f.md`. Lead-owned `presenters/mod.rs` declares this module.
#![allow(unused_variables, dead_code)] // D11: todo!() bodies; the porter removes this line with the last todo!()

/// C# `RecoveryFilter`. Discriminants 0..=3 follow the C# declaration order (the ComboBox index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryFilter {
    All = 0,
    CanRetry = 1,
    AlreadyDone = 2,
    Problems = 3,
}

/// C# `RecoveryTone`: a tone instead of a WPF brush, so the mapping has no UI type. A verdict is always shown as text;
/// the tone is only a hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryTone {
    Neutral = 0,
    Good = 1,
    Info = 2,
    Warn = 3,
    Bad = 4,
}
