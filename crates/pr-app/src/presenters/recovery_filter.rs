//! Port of `PhotoReview.App.Shared/Presenters/RecoveryFilter.cs`.

/// Recovery verdict filter in ComboBox order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryFilter {
    All = 0,
    CanRetry = 1,
    AlreadyDone = 2,
    Problems = 3,
}

/// Colour hint for recovery states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryTone {
    Neutral = 0,
    Good = 1,
    Info = 2,
    Warn = 3,
    Bad = 4,
}
