//! The `--benchmark`, `--benchmark-all` and `--benchmark-actions` modes: `RunCliBenchmarksAsync` and the dispatch blocks around it
//! in `Program.cs` (C# baseline d92d6aae).

use std::cmp::Ordering;
use std::path::Path;

use pr_core::catalog::types::{SUPPORTED_EXTENSIONS, extension_in, get_extension};
use pr_core::text::cmp_ignore_case;
use pr_core::text::dotnet_path::{ends_in_directory_separator, get_file_name};

use super::arguments::{
    is_blank, is_expected_tool_failure, reject_surplus_benchmark_arguments, resolve_output, resolve_profiles,
    resolve_report_directory,
};
use super::backend::{DatasetManifest, ProfileRun};
use super::backend::{PhaseStatus, PhaseSummary, ProfileInfo, ToolError};
use super::tool_path_guard::{get_full_path, write_report_file};
use super::{CliContext, CliMode, TOOL_NAME, UNHANDLED_EXIT_CODE, write_line};

/// `.Take(64)`: at most this many supported files are measured.
pub const FILE_LIMIT: usize = 64;

/// The files of a folder split into the measured set and the unsupported-extension counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetPlan {
    /// First [`FILE_LIMIT`] supported files, `Path.GetFullPath` applied, in listing order.
    pub files: Vec<String>,
    /// The same files as the directory listing returned them (NOT passed through `Path.GetFullPath`): C# hands these to the
    /// engine, so a relative folder yields `rel\a.jpg` and decode errors that echo the path name that text.
    pub listing_files: Vec<String>,
    /// `(key, count)` ordered by key, ordinal. Key = `"<none>"` when the extension is blank, else the extension lower-cased
    /// (`ToLowerInvariant`).
    pub unsupported_extensions: Vec<(String, usize)>,
}

/// `string.CompareOrdinal`: UTF-16 code unit order (differs from UTF-8 byte order for astral characters).
fn cmp_ordinal(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `string.ToLowerInvariant()`: char by char, keeping the char when its lower case is not a single char (U+0130), and
/// without the final-sigma rule.
fn to_lower_invariant(text: &str) -> String {
    text.chars()
        .map(|c| {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(single), None) => single,
                _ => c,
            }
        })
        .collect()
}

/// The pure part of `RunCliBenchmarksAsync` after `Directory.EnumerateFiles`: sort `all_files` by `get_file_name`
/// (OrdinalIgnoreCase) then by whole path (Ordinal), both stable; split by `catalog::types::extension_in(SUPPORTED_EXTENSIONS, ..)` on the extension
/// (`get_extension`); count the unsupported by key; keep the first 64 supported. No supported file -> `InvalidOperationException
/// ("Benchmark folder contains no supported images")`.
pub fn plan_dataset(all_files: &[String]) -> Result<DatasetPlan, ToolError> {
    let mut sorted: Vec<&String> = all_files.iter().collect();
    sorted.sort_by(|a, b| cmp_ignore_case(get_file_name(a), get_file_name(b)).then_with(|| cmp_ordinal(a, b)));

    let mut unsupported_extensions: Vec<(String, usize)> = Vec::new();
    let mut supported: Vec<&String> = Vec::new();
    for path in sorted {
        let extension = get_extension(path);
        if extension_in(SUPPORTED_EXTENSIONS, extension) {
            supported.push(path);
            continue;
        }
        let key = if is_blank(extension) {
            "<none>".to_owned()
        } else {
            to_lower_invariant(extension)
        };
        match unsupported_extensions.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, count)) => *count += 1,
            None => unsupported_extensions.push((key, 1)),
        }
    }
    unsupported_extensions.sort_by(|a, b| cmp_ordinal(&a.0, &b.0));

    let listing_files: Vec<String> = supported.into_iter().take(FILE_LIMIT).cloned().collect();
    let files = listing_files
        .iter()
        .map(|path| get_full_path(path))
        .collect::<Result<Vec<String>, ToolError>>()?;
    if files.is_empty() {
        return Err(ToolError::invalid_operation(
            "Benchmark folder contains no supported images",
        ));
    }
    Ok(DatasetPlan {
        files,
        listing_files,
        unsupported_extensions,
    })
}

/// `$"DONE profile={id} status={status} p50={p50:F1}ms p95={p95:F1}ms max={max:F1}ms report={report_path}"`. `F1` uses
/// `pr_perf::rules::fmt_fixed(v, 1)`-equivalent invariant text (see the contract: tie rounding and the Windows-locale question).
pub fn format_done_line(profile_id: &str, phase: &PhaseSummary, report_path: &str) -> String {
    format!(
        "DONE profile={profile_id} status={} p50={}ms p95={}ms max={}ms report={report_path}",
        phase.status.as_str(),
        pr_perf::rules::fmt_fixed(phase.p50, 1),
        pr_perf::rules::fmt_fixed(phase.p95, 1),
        pr_perf::rules::fmt_fixed(phase.max, 1),
    )
}

/// `$"START profile={id} workload={workload} mode={loading_mode} workers={workers} window={next}/{previous}"`.
pub fn format_start_line(profile: &ProfileInfo) -> String {
    format!(
        "START profile={} workload={} mode={} workers={} window={}/{}",
        profile.id,
        profile.workload,
        profile.loading_mode,
        profile.workers,
        profile.next_window,
        profile.previous_window
    )
}

/// `Path.Combine(directory, name)` for a `name` without separators.
fn combine(directory: &str, name: &str) -> String {
    if directory.is_empty() || ends_in_directory_separator(directory) {
        format!("{directory}{name}")
    } else {
        format!("{directory}\\{name}")
    }
}

/// The part of the profile loop that can throw after `run_profile` returned: report file, then the DONE line.
fn finish_profile(
    ctx: &mut CliContext<'_>,
    profile: &ProfileInfo,
    run: &ProfileRun,
    report_directory: &str,
    any_failed: &mut bool,
) -> Result<(), ToolError> {
    let report_path = combine(report_directory, &format!("{}-{}.json", profile.id, run.run_id));
    let json = ctx.backend.report_json(&run.run_id)?;
    write_report_file(&report_path, &json)?;
    if run.phase.status == PhaseStatus::Fail {
        *any_failed = true;
    }
    write_line(ctx.out, &format_done_line(&profile.id, &run.phase, &report_path));
    Ok(())
}

/// `RunCliBenchmarksAsync(folder, profiles, outputOverride)`. Order of effects (pinned by the process golden):
/// 1. `fs.directory_exists(folder)` else `DirectoryNotFoundException(folder)`;
/// 2. `resolve_report_directory` (guard) -> `create_directory` of it (a failure later still leaves it created);
/// 3. list the top-level files (`fs.enumerate_files(folder, "*")`, hidden/system included), [`plan_dataset`];
/// 4. manifest: `DatasetManifest { folder: get_full_path(folder), files, unsupported_extensions, file_limit: 64 }`;
/// 5. per profile: START line; `backend.run_profile` with a progress callback printing `  {profile_id}: {completed}/{total} {message}`;
///    on success write `{report_dir}\{profile.id}-{run_id}.json` (`backend.report_json` + `write_report_file`) and print the DONE line,
///    `any_failed |= status == Fail`; on ANY `Err` from that block (including a failed report write or report_json): `any_failed = true`,
///    message `Profile failed before producing samples: {type_name}: {message}`, `backend.record_failed_profile`, write its report
///    file the same way (an error there propagates), print `FAIL profile={id} error={message}` to stderr;
/// 6. `{report_dir}\summary.json` from `backend.batch_summary_json`, print `REPORT: {summary path}`;
/// 7. returns `Ok(exit_code)` = 1 when `any_failed` else 0.
///
/// `Err` is an exception that escaped to `Program.cs`; the caller decides with `is_expected_tool_failure`.
pub fn run_cli_benchmarks(
    ctx: &mut CliContext<'_>,
    folder: &str,
    profiles: &[ProfileInfo],
    output_override: Option<&str>,
) -> Result<i32, ToolError> {
    if !ctx.fs.directory_exists(Path::new(folder)) {
        return Err(ToolError::directory_not_found(folder));
    }
    let report_directory = resolve_report_directory(ctx.fs, ctx.env, folder, output_override)?;
    ctx.fs
        .create_directory(Path::new(&report_directory))
        .map_err(|error| ToolError::from_io(&error, &report_directory, "DirectoryNotFoundException"))?;
    let listed = ctx
        .fs
        .enumerate_files(Path::new(folder), "*")
        .map_err(|error| ToolError::from_io(&error, folder, "DirectoryNotFoundException"))?;
    let all_files: Vec<String> = listed.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let plan = plan_dataset(&all_files)?;
    let folder_full_path = get_full_path(folder)?;
    let listing_files = plan.listing_files;
    let manifest = DatasetManifest {
        folder: folder_full_path.clone(),
        files: plan.files,
        unsupported_extensions: plan.unsupported_extensions,
        file_limit: i32::try_from(FILE_LIMIT).unwrap_or(i32::MAX),
    };

    let mut any_failed = false;
    for profile in profiles {
        write_line(ctx.out, &format_start_line(profile));
        let attempt = {
            let out: &mut dyn std::io::Write = &mut *ctx.out;
            ctx.backend.run_profile(folder, profile, &listing_files, &mut |p| {
                write_line(
                    out,
                    &format!("  {}: {}/{} {}", p.profile_id, p.completed, p.total, p.message),
                );
            })
        }
        .and_then(|run| finish_profile(ctx, profile, &run, &report_directory, &mut any_failed));
        if let Err(error) = attempt {
            // Report this profile's failure and continue the batch, as the WPF window does.
            any_failed = true;
            let message = format!(
                "Profile failed before producing samples: {}: {}",
                error.type_name, error.message
            );
            let failed = ctx.backend.record_failed_profile(&folder_full_path, profile, &message);
            let failed_path = combine(&report_directory, &format!("{}-{}.json", profile.id, failed.run_id));
            let json = ctx.backend.report_json(&failed.run_id)?;
            write_report_file(&failed_path, &json)?;
            write_line(ctx.err, &format!("FAIL profile={} error={}", profile.id, error.message));
        }
    }

    let summary_path = combine(&report_directory, "summary.json");
    let summary = ctx.backend.batch_summary_json(&manifest)?;
    write_report_file(&summary_path, &summary)?;
    write_line(ctx.out, &format!("REPORT: {summary_path}"));
    Ok(i32::from(any_failed))
}

/// The `--benchmark*` block of `Program.cs`: `args[1]` is the folder; `reject_surplus_benchmark_arguments` then `resolve_profiles`
/// (an error prints `PhotoReview.Benchmark.Cli: {message}` to `err`, exit 2); `resolve_output`; `run_cli_benchmarks`; an `Err` that
/// `is_expected_tool_failure` prints the same line and exits 2, any other `Err` prints `Unhandled exception. {type}: {message}`
/// and exits `UNHANDLED_EXIT_CODE`.
pub fn run_benchmark_mode(ctx: &mut CliContext<'_>, mode: CliMode, args: &[String]) -> i32 {
    let mode_text = match mode {
        CliMode::Benchmark => "--benchmark",
        CliMode::BenchmarkAll => "--benchmark-all",
        CliMode::BenchmarkActions => "--benchmark-actions",
        _ => {
            write_line(
                ctx.err,
                &format!(
                    "{TOOL_NAME}: Not a benchmark mode: {}",
                    args.first().map_or("", String::as_str)
                ),
            );
            return 2;
        }
    };
    let Some(folder) = args.get(1) else {
        write_line(ctx.err, &format!("{TOOL_NAME}: Not a benchmark mode: {mode_text}"));
        return 2;
    };
    let requested =
        reject_surplus_benchmark_arguments(args).and_then(|()| resolve_profiles(mode_text, args, &*ctx.backend));
    let profiles = match requested {
        Ok(profiles) => profiles,
        Err(error) => {
            write_line(ctx.err, &format!("{TOOL_NAME}: {}", error.message));
            return 2;
        }
    };
    let output = resolve_output(mode_text, args);
    match run_cli_benchmarks(ctx, folder, &profiles, output.as_deref()) {
        Ok(code) => code,
        Err(error) if is_expected_tool_failure(&error.type_name) => {
            write_line(ctx.err, &format!("{TOOL_NAME}: {}", error.message));
            2
        }
        Err(error) => {
            write_line(
                ctx.err,
                &format!("Unhandled exception. {}: {}", error.type_name, error.message),
            );
            UNHANDLED_EXIT_CODE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::combine;

    #[test]
    fn combine_follows_path_combine() {
        assert_eq!(combine("", "x"), "x");
        assert_eq!(combine("C:\\d", "x"), "C:\\d\\x");
        assert_eq!(combine("C:\\d\\", "x"), "C:\\d\\x");
    }
}
