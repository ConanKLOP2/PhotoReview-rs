//! Port of `PhotoReview.App.Shared/ViewModels/SettingsReloadPath.cs` (C# baseline d92d6aae, 32 lines). Chooses the file a
//! settings-triggered folder reload opens at: the presented capture member is kept only when it is still listed after the
//! reload; otherwise a removed RAW member would make the load fall back to the first image. Lives in the `format` module
//! because the plan (W8-02b) groups it with the other view-model text helpers.

use pr_core::catalog::types::CaptureGroup;
use pr_core::model::RawPairMode;
use pr_core::text::eq_ignore_case;

/// C# `SettingsReloadPath.Choose(presentedPath, group, entryPath, rawEnabled, pairMode)`.
///
/// * `presented_path`: the toggled capture member shown now, or `None` when none or stale.
/// * `group`: the capture group of the current entry, if any.
/// * `entry_path`: the current entry's own path (its representative).
/// * `raw_enabled`: RAW support as it will be after the reload.
/// * `pair_mode`: the pair mode as it will be after the reload. It does not change the result (C# `_ = pairMode`).
///
/// Candidate = `presented_path` if given (an empty string counts as given, trap T19), else `entry_path`; `None` gives `None`.
/// When RAW is off, a group exists and the candidate equals the group's RAW path (`eq_ignore_case`), the JPEG path is returned.
/// Otherwise the candidate is returned unchanged.
pub fn choose_reload_path<'a>(
    presented_path: Option<&'a str>,
    group: Option<&'a CaptureGroup>,
    entry_path: Option<&'a str>,
    raw_enabled: bool,
    pair_mode: RawPairMode,
) -> Option<&'a str> {
    // C# `_ = pairMode`: the pair mode never changes the result.
    let _ = pair_mode;
    let candidate = match presented_path {
        Some(path) => path,
        None => entry_path?,
    };
    if !raw_enabled
        && let Some(group) = group
        && eq_ignore_case(candidate, group.raw_path())
    {
        return Some(group.jpeg_path());
    }
    Some(candidate)
}
