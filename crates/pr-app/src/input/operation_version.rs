//! Port of `PhotoReview.App.Shared/Input/ViewportOperationVersion.cs` (AR13): the counter shared by zoom-at-point and Fit.
//!
//! Each operation takes a new version before its first render yield; when it resumes it applies its scroll only while
//! no later operation (of either kind) has started. UI thread only (no `Send`/`Sync` needed). Baseline `d92d6aae`.
//! Not in the plan's row name list: added to `input` by this unit (see the contract, "Public API").

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

/// C# `ViewportOperationVersion` (`internal sealed`, `long Current { get; private set; }`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViewportOperationVersion {
    current: i64,
}

impl ViewportOperationVersion {
    /// C# `new ViewportOperationVersion()`: `Current == 0`.
    pub fn new() -> Self {
        todo!()
    }

    /// C# `Current` (getter only; `private set`).
    pub fn current(&self) -> i64 {
        todo!()
    }

    /// C# `Next() => ++Current`: starts a new operation, superseding any pending one, and returns its version.
    /// Named `next_version` (not `next`): clippy `should_implement_trait` rejects `next(&mut self)` on a non-iterator.
    /// The increment wraps at `i64::MAX` like unchecked C# `long` arithmetic (`wrapping_add`), it never panics.
    pub fn next_version(&mut self) -> i64 {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::ViewportOperationVersion;

    // new: C# `++Current` is unchecked `long` arithmetic. At i64::MAX the next value wraps to i64::MIN, and nothing panics.
    #[test]
    fn next_version_wraps_like_unchecked_long() {
        let mut version = ViewportOperationVersion { current: i64::MAX };
        assert_eq!(version.next_version(), i64::MIN);
        assert_eq!(version.current(), i64::MIN);
    }
}
