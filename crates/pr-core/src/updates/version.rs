//! Port of `PhotoReview.Core/Updates/AppVersion.cs`, `UpdateUrlPolicy.cs` and `UpdateCheckResult.cs`
//! (C# baseline e441d67a).
//!
//! * [`AppVersion`]: numeric `major.minor.patch` plus a pre-release flag; parses `"v2.0.93"`, `"2.0.93+hash"`,
//!   `"2.1.0-beta.1"`.
//! * [`validate_update_url`]: only `https://github.com/ConanKLOP2/PhotoReview[/...]` may be opened from the update
//!   check.
//! * [`UpdateCheckResult`]: outcome of a manual update check (constructed only through the three factory functions).
//!
//! `IUpdateChecker` / `UpdateChecker` (HTTP) are NOT part of this unit.

use std::cmp::Ordering;
use std::fmt;

/// `major.minor.patch` with an optional pre-release flag (C# `record struct AppVersion`).
///
/// Derived equality compares all four fields (like the C# record struct); ordering is [`Ord`] below and agrees with
/// equality (two values compare `Equal` only when all four fields are equal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct AppVersion {
    pub major: i32,
    pub minor: i32,
    pub patch: i32,
    pub is_prerelease: bool,
}

impl AppVersion {
    /// C# `AppVersion.TryParse(string?, out AppVersion)`. `None` is the `false` result (C# leaves `default`).
    /// Accepts an optional leading `v`/`V`, optional `+build` and `-prerelease` suffixes; see contract.md.
    pub fn try_parse(text: Option<&str>) -> Option<AppVersion> {
        let _ = text;
        todo!("port AppVersion.TryParse")
    }
}

impl Ord for AppVersion {
    /// Numeric major, minor, patch; at equal numbers a pre-release sorts BELOW the release.
    fn cmp(&self, other: &Self) -> Ordering {
        let _ = other;
        todo!("port AppVersion.CompareTo")
    }
}

impl PartialOrd for AppVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for AppVersion {
    /// `"{major}.{minor}.{patch}"`, invariant digits; the pre-release flag is NOT printed (C# `ToString`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = f;
        todo!("port AppVersion.ToString")
    }
}

/// C# `UpdateUrlPolicy.ReleasesPageUrl`.
pub const RELEASES_PAGE_URL: &str = "https://github.com/ConanKLOP2/PhotoReview/releases";

/// C# `UpdateUrlPolicy.Validate(string?)`: the normalized absolute URL (what `Uri.AbsoluteUri` would return) when
/// `url` is `https://github.com/ConanKLOP2/PhotoReview` or below it, otherwise `None`.
pub fn validate_update_url(url: Option<&str>) -> Option<String> {
    let _ = url;
    todo!("port UpdateUrlPolicy.Validate")
}

/// C# `UpdateCheckStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UpdateCheckStatus {
    #[default]
    UpToDate,
    UpdateAvailable,
    Failed,
}

/// C# `UpdateFailure`: why a manual update check failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UpdateFailure {
    #[default]
    None,
    Offline,
    Timeout,
    RateLimited,
    BadResponse,
    InvalidVersion,
}

/// C# `sealed record UpdateCheckResult`. Fields are private (C# `private init`); build with
/// [`up_to_date`](Self::up_to_date), [`update_available`](Self::update_available) or [`failed`](Self::failed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheckResult {
    status: UpdateCheckStatus,
    latest_version: Option<String>,
    download_url: Option<String>,
    failure: UpdateFailure,
}

impl UpdateCheckResult {
    /// Status `UpToDate`, `latest_version` set, no URL, failure `None`.
    pub fn up_to_date(latest_version: &str) -> Self {
        let _ = latest_version;
        todo!("port UpdateCheckResult.UpToDate")
    }

    /// Status `UpdateAvailable`, `latest_version` and `download_url` set (stored verbatim, not re-validated),
    /// failure `None`.
    pub fn update_available(latest_version: &str, download_url: &str) -> Self {
        let _ = (latest_version, download_url);
        todo!("port UpdateCheckResult.UpdateAvailable")
    }

    /// Status `Failed`, `failure` set, no version and no URL.
    pub fn failed(failure: UpdateFailure) -> Self {
        let _ = failure;
        todo!("port UpdateCheckResult.Failed")
    }

    pub fn status(&self) -> UpdateCheckStatus {
        self.status
    }

    /// Latest published version (numeric `major.minor.patch`) for `UpToDate`/`UpdateAvailable`, else `None`.
    pub fn latest_version(&self) -> Option<&str> {
        self.latest_version.as_deref()
    }

    /// Download page for `UpdateAvailable`, else `None`.
    pub fn download_url(&self) -> Option<&str> {
        self.download_url.as_deref()
    }

    pub fn failure(&self) -> UpdateFailure {
        self.failure
    }
}
