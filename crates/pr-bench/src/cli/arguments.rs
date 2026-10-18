//! Argument interpretation of the CLI modes that take free-form lists.
//!
//! Port of `tools/PhotoReview.Benchmark.Cli/BenchmarkCliArguments.cs` (C# baseline d92d6aae, 115 lines). Every
//! failure is a [`ToolError`] with type `ArgumentException` (or `InvalidOperationException` from the path guard)
//! whose message is printed after `PhotoReview.Benchmark.Cli: ` with exit code 2.

use pr_core::abstractions::FileSystem;

use super::ToolEnvironment;
use super::backend::{ProfileCatalog, ProfileInfo, ToolError};

/// `char.IsWhiteSpace`: U+0009..U+000D, U+0020, U+0085, U+00A0, U+1680, U+2000..U+200A, U+2028, U+2029, U+202F, U+205F, U+3000.
/// This is exactly Rust `char::is_whitespace` (verified with .NET 10: U+001C..U+001F, U+180E, U+200B, U+FEFF are NOT white
/// space), so `str::trim` is `string.Trim()`; the function exists so the rule has one named place.
pub fn is_dotnet_whitespace(c: char) -> bool {
    todo!("W7-03d")
}

/// `string.Trim()` (all `char.IsWhiteSpace` characters on both ends).
pub fn dotnet_trim(s: &str) -> &str {
    todo!("W7-03d")
}

/// `string.IsNullOrWhiteSpace` for a non-null string.
pub fn is_blank(s: &str) -> bool {
    todo!("W7-03d")
}

/// `text.Split(',', RemoveEmptyEntries | TrimEntries)`: split on `,`, trim every piece, drop empty pieces, keep order.
pub fn split_list(text: &str) -> Vec<String> {
    todo!("W7-03d")
}

/// `ResolveProfiles(mode, args)` for `--benchmark`, `--benchmark-all`, `--benchmark-actions`.
///
/// * `--benchmark-all` -> `catalog.runnable()`; `--benchmark-actions` -> `catalog.all()` filtered by `is_file_action`.
/// * `--benchmark` with fewer than 3 args -> `[find("recommended-auto")]` (the C# `!` : a missing profile is a bug, panic is fine
///   only in tests; production catalogs always contain it).
/// * `--benchmark` with `args[2]`: [`split_list`]; no ids -> `ArgumentException("No benchmark profile id given (expected a
///   comma-separated list)")`; an id the catalog does not know -> `ArgumentException("Unknown benchmark profile: {id}")` (the
///   trimmed id, original case); duplicates are kept in order.
/// * any other mode -> `ArgumentException("Not a benchmark mode: {mode}")`.
pub fn resolve_profiles(
    mode: &str,
    args: &[String],
    catalog: &dyn ProfileCatalog,
) -> Result<Vec<ProfileInfo>, ToolError> {
    todo!("W7-03d")
}

/// `ResolveOutput`: `args[2]` when there are at least 3 args, `mode != "--benchmark"` and `args[2]` is not blank.
pub fn resolve_output(mode: &str, args: &[String]) -> Option<String> {
    todo!("W7-03d")
}

/// `ParseWidths`: comma list (same split as [`split_list`]) of invariant non-negative integers, `NumberStyles.None` (ASCII
/// digits only, no sign/space/separator, at most `i32::MAX`, leading zeros fine, any number of TRAILING U+0000 accepted:
/// `int.TryParse("5\0")` is true, a NUL before or between digits is not), duplicates dropped keeping the first.
/// Errors: no piece -> `ArgumentException("At least one width is required")`; a bad piece ->
/// `ArgumentException("Invalid width '{piece}' (expected a non-negative whole number)")`.
pub fn parse_widths(text: &str) -> Result<Vec<i32>, ToolError> {
    todo!("W7-03d")
}

/// `ParsePerfAnalyzeRules`: tokens from index 2; only `--rules <file>` is accepted (exact, case-sensitive); the file must not be
/// blank. Errors: other token -> `Unexpected argument for --perf-analyze: {token}`; dangling or blank -> `--rules needs a file path`.
/// A repeated `--rules` keeps the last one.
pub fn parse_perf_analyze_rules(args: &[String]) -> Result<Option<String>, ToolError> {
    todo!("W7-03d")
}

/// `ParsePositiveInt`: `NumberStyles.None` invariant integer `> 0` (same digit rules as [`parse_widths`], trailing NULs included). Error: `ArgumentException("Invalid {name} '{text}' (expected a
/// positive whole number)")`.
pub fn parse_positive_int(text: &str, name: &str) -> Result<i32, ToolError> {
    todo!("W7-03d")
}

/// `ParseIntInRange`: digits-only integer in `min..=max`. Error: `ArgumentException("{name} must be a whole number {min}..{max} (got '{text}')")`.
pub fn parse_int_in_range(text: &str, name: &str, min: i32, max: i32) -> Result<i32, ToolError> {
    todo!("W7-03d")
}

/// `ParsePositiveFiniteDouble`: `double.TryParse(text, NumberStyles.Float, Invariant)`; accepted syntax = optional leading/trailing
/// white space (U+0009..U+000D, U+0020 only; NBSP and U+2003 are NOT accepted), optional sign, digits with optional `.` (`.5` and `5.`
/// are fine), optional exponent, then any number of trailing U+0000 (`"2.5 \0"` and `"1e1\0"` are accepted, `" 5\0 "` is not). `Infinity`, `NaN`, values
/// that overflow to infinity (`1e999`) and values `<= 0` are rejected. Error: `ArgumentException("{name} must be a finite number > 0
/// (got '{text}')")`.
pub fn parse_positive_finite_double(text: &str, name: &str) -> Result<f64, ToolError> {
    todo!("W7-03d")
}

/// `ParseDefinedEnum<TEnum>`: `Enum.TryParse(text, ignoreCase: true)` then `Enum.IsDefined`. `members` lists
/// `(name, value)` in `Enum.GetNames` order (ascending value). `Enum.TryParse` rules to reproduce: surrounding white space (`char.IsWhiteSpace`) is trimmed;
/// empty -> fail; a leading digit, `+` or `-` means a number (must fit the underlying `i32`, leading zeros fine, trailing NULs
/// accepted as for integers, an unnamed number is then rejected by `IsDefined`; `-0` and `+0` are 0); otherwise a comma separated list of names (each trimmed, case-insensitive; a number inside the list is invalid; an empty
/// item is invalid; a NUL anywhere in a name is invalid) whose values are OR-ed; the result must equal a member value
/// (`LoadingMode` `Fast,Preview` is 1 = `Preview`; `ImageSortMode` `SizeDescending,SizeAscending` is 3 = `Default`). Error: `ArgumentException("invalid {name} '{text}' ({Name1|Name2|...})")`.
pub fn parse_defined_enum(text: &str, name: &str, members: &[(&str, i32)]) -> Result<i32, ToolError> {
    todo!("W7-03d")
}

/// `IsExpectedToolFailure` on the .NET exception type NAME: true for `ArgumentException` and subclasses
/// (`ArgumentNullException`, `ArgumentOutOfRangeException`, `BenchmarkProfileException`), `IOException` and subclasses
/// (`FileNotFoundException`, `DirectoryNotFoundException`, `PathTooLongException`, `EndOfStreamException`, `FileLoadException`,
/// `DriveNotFoundException`), `InvalidOperationException` and subclasses (`ObjectDisposedException`), `NotSupportedException`
/// (+ `PlatformNotSupportedException`), `UnauthorizedAccessException`, `FormatException`, `InvalidDataException`. False for
/// everything else (`OperationCanceledException`, `TaskCanceledException`, `OutOfMemoryException`, `JsonException`, ...).
pub fn is_expected_tool_failure(type_name: &str) -> bool {
    todo!("W7-03d")
}

/// `RejectSurplusBenchmarkArguments`: more than 3 args -> `ArgumentException("Unexpected argument for {args[0]}: {args[3]}")`.
pub fn reject_surplus_benchmark_arguments(args: &[String]) -> Result<(), ToolError> {
    todo!("W7-03d")
}

/// `ResolveReportDirectory`: the override (any non-empty text, even blank) through `get_full_path`, otherwise
/// `{temp_path}PhotoReview-Benchmark-Reports\{utc_stamp}-{first 6 of guid N}`; then
/// `tool_path_guard::ensure_output_directory(dir, photo_folder)`. The directory is NOT created here. An unusable override text
/// (`get_full_path` fails) is an `ArgumentException`.
pub fn resolve_report_directory(
    fs: &dyn FileSystem,
    env: &dyn ToolEnvironment,
    photo_folder: &str,
    output_override: Option<&str>,
) -> Result<String, ToolError> {
    todo!("W7-03d")
}
