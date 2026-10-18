//! Argument interpretation of the CLI modes that take free-form lists.
//!
//! Port of `tools/PhotoReview.Benchmark.Cli/BenchmarkCliArguments.cs` (C# baseline d92d6aae, 115 lines). Every
//! failure is a [`ToolError`] with type `ArgumentException` (or `InvalidOperationException` from the path guard)
//! whose message is printed after `PhotoReview.Benchmark.Cli: ` with exit code 2.

use pr_core::abstractions::FileSystem;
use pr_core::text::eq_ignore_case;

use super::ToolEnvironment;
use super::backend::{ProfileCatalog, ProfileInfo, ToolError};
use super::tool_path_guard;

/// `char.IsWhiteSpace`: U+0009..U+000D, U+0020, U+0085, U+00A0, U+1680, U+2000..U+200A, U+2028, U+2029, U+202F, U+205F, U+3000.
/// This is exactly Rust `char::is_whitespace` (verified with .NET 10: U+001C..U+001F, U+180E, U+200B, U+FEFF are NOT white
/// space), so `str::trim` is `string.Trim()`; the function exists so the rule has one named place.
pub fn is_dotnet_whitespace(c: char) -> bool {
    c.is_whitespace()
}

/// `string.Trim()` (all `char.IsWhiteSpace` characters on both ends).
pub fn dotnet_trim(s: &str) -> &str {
    s.trim_matches(is_dotnet_whitespace)
}

/// `string.IsNullOrWhiteSpace` for a non-null string.
pub fn is_blank(s: &str) -> bool {
    s.chars().all(is_dotnet_whitespace)
}

/// `text.Split(',', RemoveEmptyEntries | TrimEntries)`: split on `,`, trim every piece, drop empty pieces, keep order.
pub fn split_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(dotnet_trim)
        .filter(|piece| !piece.is_empty())
        .map(str::to_owned)
        .collect()
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
    match mode {
        "--benchmark-all" => Ok(catalog.runnable()),
        "--benchmark-actions" => Ok(catalog.all().into_iter().filter(|p| p.is_file_action).collect()),
        "--benchmark" => {
            let Some(list) = args.get(2) else {
                // C# uses `!`: a catalog without the default profile is a bug; report it instead of crashing.
                return catalog
                    .find("recommended-auto")
                    .map(|profile| vec![profile])
                    .ok_or_else(|| ToolError::argument("Unknown benchmark profile: recommended-auto"));
            };
            let ids = split_list(list);
            // "," or " " would otherwise "succeed" after running nothing and writing an empty summary.
            if ids.is_empty() {
                return Err(ToolError::argument(
                    "No benchmark profile id given (expected a comma-separated list)",
                ));
            }
            ids.iter()
                .map(|id| {
                    catalog
                        .find(id)
                        .ok_or_else(|| ToolError::argument(format!("Unknown benchmark profile: {id}")))
                })
                .collect()
        }
        other => Err(ToolError::argument(format!("Not a benchmark mode: {other}"))),
    }
}

/// `ResolveOutput`: `args[2]` when there are at least 3 args, `mode != "--benchmark"` and `args[2]` is not blank.
pub fn resolve_output(mode: &str, args: &[String]) -> Option<String> {
    match args.get(2) {
        Some(third) if mode != "--benchmark" && !is_blank(third) => Some(third.clone()),
        _ => None,
    }
}

/// `int.TryParse(text, NumberStyles.None, Invariant)`: ASCII digits only (no sign, no space, no separator), value at most
/// `i32::MAX`, leading zeros fine, and any number of trailing U+0000 accepted (.NET quirk).
fn parse_unsigned_i32(text: &str) -> Option<i32> {
    let digits = text.trim_end_matches('\0');
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut value: i64 = 0;
    for b in digits.bytes() {
        value = value * 10 + i64::from(b - b'0');
        if value > i64::from(i32::MAX) {
            return None;
        }
    }
    i32::try_from(value).ok()
}

/// `int.TryParse(text, AllowLeadingSign | AllowTrailingWhite, Invariant)` as `Enum.TryParse` uses it (after a `TrimStart`): one
/// optional `+`/`-`, ASCII digits, then optional trailing white space (U+0009..U+000D and U+0020 ONLY, not NBSP or U+0085), then any
/// number of U+0000; `"5 \0"` is accepted and `"5\0 "` is not. The value must fit `i32`.
fn parse_signed_i32(text: &str) -> Option<i32> {
    let (negative, rest) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let end = rest.bytes().position(|b| !b.is_ascii_digit()).unwrap_or(rest.len());
    let digits = &rest[..end];
    if digits.is_empty() {
        return None;
    }
    let tail = rest[end..].trim_start_matches(is_number_whitespace);
    if !tail.bytes().all(|b| b == 0) {
        return None;
    }
    let mut value: i64 = 0;
    for b in digits.bytes() {
        value = value * 10 + i64::from(b - b'0');
        if value > i64::from(i32::MAX) + 1 {
            return None;
        }
    }
    let signed = if negative { -value } else { value };
    i32::try_from(signed).ok()
}

/// `ParseWidths`: comma list (same split as [`split_list`]) of invariant non-negative integers, `NumberStyles.None` (ASCII
/// digits only, no sign/space/separator, at most `i32::MAX`, leading zeros fine, any number of TRAILING U+0000 accepted:
/// `int.TryParse("5\0")` is true, a NUL before or between digits is not), duplicates dropped keeping the first.
/// Errors: no piece -> `ArgumentException("At least one width is required")`; a bad piece ->
/// `ArgumentException("Invalid width '{piece}' (expected a non-negative whole number)")`.
pub fn parse_widths(text: &str) -> Result<Vec<i32>, ToolError> {
    let pieces = split_list(text);
    if pieces.is_empty() {
        return Err(ToolError::argument("At least one width is required"));
    }
    let mut widths: Vec<i32> = Vec::with_capacity(pieces.len());
    for piece in &pieces {
        let width = parse_unsigned_i32(piece).ok_or_else(|| {
            ToolError::argument(format!(
                "Invalid width '{piece}' (expected a non-negative whole number)"
            ))
        })?;
        if !widths.contains(&width) {
            widths.push(width);
        }
    }
    Ok(widths)
}

/// `ParsePerfAnalyzeRules`: tokens from index 2; only `--rules <file>` is accepted (exact, case-sensitive); the file must not be
/// blank. Errors: other token -> `Unexpected argument for --perf-analyze: {token}`; dangling or blank -> `--rules needs a file path`.
/// A repeated `--rules` keeps the last one.
pub fn parse_perf_analyze_rules(args: &[String]) -> Result<Option<String>, ToolError> {
    let mut rules_path = None;
    let mut i = 2;
    while i < args.len() {
        if args[i] != "--rules" {
            return Err(ToolError::argument(format!(
                "Unexpected argument for --perf-analyze: {}",
                args[i]
            )));
        }
        match args.get(i + 1) {
            Some(path) if !is_blank(path) => rules_path = Some(path.clone()),
            _ => return Err(ToolError::argument("--rules needs a file path")),
        }
        i += 2;
    }
    Ok(rules_path)
}

/// `ParsePositiveInt`: `NumberStyles.None` invariant integer `> 0` (same digit rules as [`parse_widths`], trailing NULs included). Error: `ArgumentException("Invalid {name} '{text}' (expected a
/// positive whole number)")`.
pub fn parse_positive_int(text: &str, name: &str) -> Result<i32, ToolError> {
    match parse_unsigned_i32(text) {
        Some(value) if value > 0 => Ok(value),
        _ => Err(ToolError::argument(format!(
            "Invalid {name} '{text}' (expected a positive whole number)"
        ))),
    }
}

/// `ParseIntInRange`: digits-only integer in `min..=max`. Error: `ArgumentException("{name} must be a whole number {min}..{max} (got '{text}')")`.
pub fn parse_int_in_range(text: &str, name: &str, min: i32, max: i32) -> Result<i32, ToolError> {
    match parse_unsigned_i32(text) {
        Some(value) if value >= min && value <= max => Ok(value),
        _ => Err(ToolError::argument(format!(
            "{name} must be a whole number {min}..{max} (got '{text}')"
        ))),
    }
}

/// .NET number-parsing white space: U+0009..U+000D and U+0020 only.
fn is_number_whitespace(c: char) -> bool {
    matches!(c, '\u{9}'..='\u{D}' | ' ')
}

/// `double.TryParse(text, NumberStyles.Float, Invariant)` for the finite syntax: leading white space, sign, digits with an
/// optional `.`, optional exponent, trailing white space, then any number of U+0000. `Infinity`/`NaN` are not parsed here (the
/// callers reject them anyway). The result may be infinite (overflow) or zero (underflow).
fn parse_float_text(text: &str) -> Option<f64> {
    let s = text.trim_start_matches(is_number_whitespace);
    let b = s.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let mut digits = 0;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
        digits += 1;
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exponent_start = j;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        // An exponent letter without digits is not part of the number: it is left over and fails the trailing check.
        if j > exponent_start {
            i = j;
        }
    }
    let rest = s[i..].trim_start_matches(is_number_whitespace);
    if !rest.bytes().all(|c| c == 0) {
        return None;
    }
    s[..i].parse::<f64>().ok()
}

/// `ParsePositiveFiniteDouble`: `double.TryParse(text, NumberStyles.Float, Invariant)`; accepted syntax = optional leading/trailing
/// white space (U+0009..U+000D, U+0020 only; NBSP and U+2003 are NOT accepted), optional sign, digits with optional `.` (`.5` and `5.`
/// are fine), optional exponent, then any number of trailing U+0000 (`"2.5 \0"` and `"1e1\0"` are accepted, `" 5\0 "` is not). `Infinity`, `NaN`, values
/// that overflow to infinity (`1e999`) and values `<= 0` are rejected. Error: `ArgumentException("{name} must be a finite number > 0
/// (got '{text}')")`.
pub fn parse_positive_finite_double(text: &str, name: &str) -> Result<f64, ToolError> {
    match parse_float_text(text) {
        Some(value) if value.is_finite() && value > 0.0 => Ok(value),
        _ => Err(ToolError::argument(format!(
            "{name} must be a finite number > 0 (got '{text}')"
        ))),
    }
}

/// `Enum.TryParse(text, ignoreCase: true)` followed by `Enum.IsDefined`, on `(name, value)` members.
fn try_parse_defined_enum(text: &str, members: &[(&str, i32)]) -> Option<i32> {
    let trimmed = dotnet_trim(text);
    let first = trimmed.chars().next()?;
    let value = if first.is_ascii_digit() || first == '+' || first == '-' {
        // The number parser sees the text with only its LEADING white space removed (trailing Unicode white space fails it).
        match parse_signed_i32(text.trim_start_matches(is_dotnet_whitespace)) {
            Some(number) => number,
            // Not a number: .NET falls back to name parsing, which cannot match a name that starts like a number.
            None => parse_enum_names(trimmed, members)?,
        }
    } else {
        parse_enum_names(trimmed, members)?
    };
    members.iter().any(|(_, v)| *v == value).then_some(value)
}

/// A comma separated list of member names (each trimmed, case-insensitive), OR-ed together.
fn parse_enum_names(text: &str, members: &[(&str, i32)]) -> Option<i32> {
    let mut value = 0;
    for item in text.split(',') {
        let name = dotnet_trim(item);
        if name.is_empty() {
            return None;
        }
        let (_, member_value) = members.iter().find(|(member, _)| eq_ignore_case(member, name))?;
        value |= *member_value;
    }
    Some(value)
}

/// `ParseDefinedEnum<TEnum>`: `Enum.TryParse(text, ignoreCase: true)` then `Enum.IsDefined`. `members` lists
/// `(name, value)` in `Enum.GetNames` order (ascending value). `Enum.TryParse` rules to reproduce: surrounding white space (`char.IsWhiteSpace`) is trimmed;
/// empty -> fail; a leading digit, `+` or `-` means a number (must fit the underlying `i32`, leading zeros fine, trailing NULs
/// accepted as for integers, an unnamed number is then rejected by `IsDefined`; `-0` and `+0` are 0); otherwise a comma separated list of names (each trimmed, case-insensitive; a number inside the list is invalid; an empty
/// item is invalid; a NUL anywhere in a name is invalid) whose values are OR-ed; the result must equal a member value
/// (`LoadingMode` `Fast,Preview` is 1 = `Preview`; `ImageSortMode` `SizeDescending,SizeAscending` is 3 = `Default`). Error: `ArgumentException("invalid {name} '{text}' ({Name1|Name2|...})")`.
pub fn parse_defined_enum(text: &str, name: &str, members: &[(&str, i32)]) -> Result<i32, ToolError> {
    try_parse_defined_enum(text, members).ok_or_else(|| {
        let names: Vec<&str> = members.iter().map(|(member, _)| *member).collect();
        ToolError::argument(format!("invalid {name} '{text}' ({})", names.join("|")))
    })
}

/// `IsExpectedToolFailure` on the .NET exception type NAME: true for `ArgumentException` and subclasses
/// (`ArgumentNullException`, `ArgumentOutOfRangeException`, `BenchmarkProfileException`), `IOException` and subclasses
/// (`FileNotFoundException`, `DirectoryNotFoundException`, `PathTooLongException`, `EndOfStreamException`, `FileLoadException`,
/// `DriveNotFoundException`), `InvalidOperationException` and subclasses (`ObjectDisposedException`), `NotSupportedException`
/// (+ `PlatformNotSupportedException`), `UnauthorizedAccessException`, `FormatException`, `InvalidDataException`. False for
/// everything else (`OperationCanceledException`, `TaskCanceledException`, `OutOfMemoryException`, `JsonException`, ...).
pub fn is_expected_tool_failure(type_name: &str) -> bool {
    matches!(
        type_name,
        "ArgumentException"
            | "ArgumentNullException"
            | "ArgumentOutOfRangeException"
            | "BenchmarkProfileException"
            | "IOException"
            | "FileNotFoundException"
            | "DirectoryNotFoundException"
            | "PathTooLongException"
            | "EndOfStreamException"
            | "FileLoadException"
            | "DriveNotFoundException"
            | "InvalidOperationException"
            | "ObjectDisposedException"
            | "NotSupportedException"
            | "PlatformNotSupportedException"
            | "UnauthorizedAccessException"
            | "FormatException"
            | "InvalidDataException"
    )
}

/// `RejectSurplusBenchmarkArguments`: more than 3 args -> `ArgumentException("Unexpected argument for {args[0]}: {args[3]}")`.
pub fn reject_surplus_benchmark_arguments(args: &[String]) -> Result<(), ToolError> {
    match args.get(3) {
        Some(surplus) => Err(ToolError::argument(format!(
            "Unexpected argument for {}: {surplus}",
            args[0]
        ))),
        None => Ok(()),
    }
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
    let report_directory = match output_override {
        Some(text) if !text.is_empty() => tool_path_guard::get_full_path(text)?,
        _ => {
            let guid = env.new_guid_n();
            let short: String = guid.chars().take(6).collect();
            let temp = env.temp_path();
            // `Path.Combine`: a separator is added only when the first part does not end in one.
            let separator = if temp.is_empty() || pr_core::text::dotnet_path::ends_in_directory_separator(&temp) {
                ""
            } else {
                "\\"
            };
            format!(
                "{temp}{separator}PhotoReview-Benchmark-Reports\\{}-{short}",
                env.utc_stamp()
            )
        }
    };
    tool_path_guard::ensure_output_directory(fs, &report_directory, photo_folder)?;
    Ok(report_directory)
}
