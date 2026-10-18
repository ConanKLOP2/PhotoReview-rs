//! Port of `PhotoReview.App.Shared/BuildInfo.cs` (C# baseline d92d6aae): the version line of the Settings window,
//! `"Phiên bản 2.0.41 · build 24/09/2026 07:15 · 1a2b3c4d"`.
//!
//! C# reads two assembly attributes written by `Directory.Build.targets`: `InformationalVersion`
//! (`"2.0.41+1a2b3c4d[.dirty]"`) and the `BuildTime` metadata (`"yyyy-MM-dd HH:mm:ss zzz"`). Rust has no assembly
//! attributes: the composition root builds a [`BuildStamp`] from its own build-time values (W8-20a) and passes it in.
//! The text comes from the injected [`Localizer`] (C# `Tr.*`, D-W8-4) and the local clock offset from a
//! [`UtcOffsetSource`] (C# `TimeZoneInfo.Local`, DST aware), so the pure logic is tested with fixed values.

// D11: temporary allow, the stub bodies are todo!(); removed when the porter fills them.
#![allow(unused_variables, dead_code)]

use pr_l10n::localizer::Localizer;

/// C# `BuildInfo.PartSeparator` text: `" · "` (U+0020 U+00B7 U+0020), language-neutral.
pub const PART_SEPARATOR: &str = " · ";

/// The `BuildTime` metadata format (C# `DateTimeOffset.TryParseExact` format string, invariant culture).
pub const BUILD_TIME_FORMAT: &str = "yyyy-MM-dd HH:mm:ss zzz";

/// The attributes that `Directory.Build.targets` writes into the binary (C# `AssemblyInformationalVersionAttribute` and
/// `AssemblyMetadataAttribute("BuildTime")`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildStamp {
    /// `"2.0.41+1a2b3c4d[.dirty]"`, or `None` when the attribute is absent.
    pub informational_version: Option<String>,
    /// `"yyyy-MM-dd HH:mm:ss zzz"`, or `None` when the metadata is absent.
    pub build_time: Option<String>,
}

impl BuildStamp {
    /// C# `GetVersion(assembly)`: the informational version exactly as written (`None` when absent).
    pub fn version(&self) -> Option<&str> {
        todo!()
    }

    /// C# `Describe(assembly)`: [`describe_parts`] over this stamp's two values.
    pub fn describe(&self, localizer: &Localizer, offset: &dyn UtcOffsetSource) -> String {
        todo!()
    }
}

/// The machine's local UTC offset at a given instant (C# `TimeZoneInfo.Local.GetUtcOffset(instant)`). Implemented in
/// `pr-platform-win` (the time-zone API is Windows code); this module only consumes it. Pure tests use [`FixedOffset`].
pub trait UtcOffsetSource: Send + Sync {
    /// Minutes east of UTC at `utc_unix_seconds` (seconds since 1970-01-01T00:00:00Z), DST aware.
    fn offset_minutes_at(&self, utc_unix_seconds: i64) -> i32;
}

/// A constant offset: tests, and a caller that wants UTC (`FixedOffset(0)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedOffset(pub i32);

impl UtcOffsetSource for FixedOffset {
    fn offset_minutes_at(&self, utc_unix_seconds: i64) -> i32 {
        todo!()
    }
}

/// A parsed `BuildTime` (C# `DateTimeOffset`, reduced to the instant: the written offset only locates the instant and
/// is NOT used for display, see [`describe_parts`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildTime {
    /// Seconds since 1970-01-01T00:00:00Z.
    pub utc_unix_seconds: i64,
}

/// C# `DateTimeOffset.TryParseExact(text, "yyyy-MM-dd HH:mm:ss zzz", Invariant, None, out built)`. `None` input (C#
/// `null`) and any text that does not match the format exactly give `None`. Strict: four-digit year, two-digit fields,
/// a signed `hh:mm` offset, real calendar dates only.
pub fn parse_build_time(text: Option<&str>) -> Option<BuildTime> {
    todo!()
}

/// C# `BuildInfo.Describe(string? informationalVersion, string? buildTime)` (internal overload, the one the tests call).
///
/// * `informational_version` empty or whitespace-only (C# `IsNullOrWhiteSpace`, including `None`): returns
///   `loc.get(settings.version.unknown)` and nothing else, even when `build_time` is present.
/// * Otherwise split at the FIRST `'+'`: `version` = text before it (or the whole text), `commit` = text after it
///   (`None` when there is no `'+'`). Parts, joined with [`PART_SEPARATOR`]:
///   1. `loc.format(settings.version.number, version = version)` (an empty version still gives the labelled part);
///   2. when `build_time` parses: `loc.format(settings.version.build, buildTime = format_local_minute(..))`, where the
///      minute comes from the instant shifted by `offset.offset_minutes_at(instant)`;
///   3. the commit, only when it is non-empty (`"2.0.41+"` has no commit part).
pub fn describe_parts(
    localizer: &Localizer,
    informational_version: Option<&str>,
    build_time: Option<&str>,
    offset: &dyn UtcOffsetSource,
) -> String {
    todo!()
}

/// C# `built.ToLocalTime().ToString("dd'/'MM'/'yyyy HH':'mm", CultureInfo.InvariantCulture)`: the wall-clock minute
/// of `utc_unix_seconds` shifted by `offset_minutes` (`"24/09/2026 07:15"`). Proleptic Gregorian calendar, two-digit
/// day, month, hour and minute, four-digit year. Pure arithmetic (no `chrono`, no OS time zone).
pub fn format_local_minute(utc_unix_seconds: i64, offset_minutes: i32) -> String {
    todo!()
}
