//! Port target: `PhotoReview.Core/Updates` (W1-10; UpdateChecker in Wave 2).

/// The ONE place that names the GitHub repository the update check follows (`owner/repo`), as a macro so it can be
/// spliced into `concat!` in constants. DIVERGENCE from C# (DV-22, P-60): C# follows `ConanKLOP2/PhotoReview`; this
/// Rust build is released separately from `ConanKLOP2/PhotoReview-rs`, so it follows its own releases.
macro_rules! update_repo {
    () => {
        "ConanKLOP2/PhotoReview-rs"
    };
}

pub mod checker;
pub mod version;

/// `owner/repo` of the releases the update check follows (see the `update_repo!` macro).
pub const UPDATE_REPO: &str = update_repo!();
