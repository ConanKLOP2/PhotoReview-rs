//! Port of the counter in `PhotoReview.App.Shared/Input/ViewportOperationVersion.cs` (unit W8-05a, contract `contracts/W8-05a.md`).
//!
//! No C# test exists for this counter, so every test here is new (`// new:`). The wrap-around test lives in the
//! owned file `crates/pr-app/src/input/operation_version.rs` because it reads the private field.
//!
//! Code under test: `input::operation_version` (stub). `new`, `current` and `next_version` are `todo!()`, so these fail
//! until the porter implements them.

use pr_app::input::operation_version::ViewportOperationVersion;

// new: C# `Current` starts at 0 and `Next()` (`++Current`) returns 1 first.
#[test]
fn new_version_is_zero_and_next_version_returns_one() {
    let mut version = ViewportOperationVersion::new();
    assert_eq!(version.current(), 0);
    assert_eq!(version.next_version(), 1);
    assert_eq!(version.current(), 1);
}

// new: each new operation takes a strictly larger version, and an older version is no longer the current one.
#[test]
fn next_version_strictly_increases_and_supersedes_older_ids() {
    let mut version = ViewportOperationVersion::new();
    let first = version.next_version();
    let second = version.next_version();
    let third = version.next_version();
    assert_eq!((first, second, third), (1, 2, 3));
    assert_eq!(version.current(), third);
    assert_ne!(first, version.current(), "the first operation is superseded");
}
