//! Port of `tests/PhotoReview.Integration.Tests/BenchmarkCliHardeningTests.cs` (the T-B-03 / T-B-05 guard tests),
//! `BenchmarkCliJunctionTests.cs` (`Native`), `BenchmarkCliReportWriteTests.cs` (`Native` R13 and R30) and the golden checks
//! over `tests/golden/cli-tool-path-guard.json` (inputs in `tests/golden-inputs/cli-tool-path-guard.json`; oracle `oracle/W7-03d.cs`).
//!
//! Port of `tools/PhotoReview.Benchmark.Cli/ToolPathGuard.cs` (C# baseline d92d6aae). Unit W7-03d, contract `contracts/W7-03d.md`.
//! Red stage: every stub body is `todo!()`, so each test here fails until the porter implements it.
//!
//! Porting notes: the C# tests call `PerfSession.ValidatePaths`; here the same trees are checked against
//! `ensure_cache_directory` with the photo folder, the output directory and both app data folders as the protected set.
//! `BenchmarkCliArguments.ResolveReportDirectory` is `arguments::resolve_report_directory`.
//! Documented skips: `CollectForbiddenKeys_*`, `RawSurvey_*`, `ScanEmbeddedJpegs_*`, `ReportModes_*`, `IoDecodeSplit_*`,
//! `DecoderMarkdown_*`, `DecoderBackends_*`, `DecoderBench_*`, `TotalDecodes_*`, `PerfSession_*`, `CacheDir_ParseArgsKeepsTheFlag`
//! (other tools of the C# project, see the contract's "NOT here" list).
//! Native tests (`#[ignore = "native..."]`) create real junctions, hard links and symlinks only inside a `TempRoot`.

use std::path::{Path, PathBuf};
use std::process::Command;

use pr_bench::cli::SystemToolEnvironment;
use pr_bench::cli::arguments::resolve_report_directory;
use pr_bench::cli::backend::ToolError;
use pr_bench::cli::tool_path_guard::{
    CACHE_MARKER, CACHE_MARKER_TEXT, app_data_folder, claim_cache_directory, csharp_app_data_folder,
    ensure_cache_directory, ensure_output_directory, ensure_output_file, is_same_or_under, overlaps, write_report_file,
};
use pr_core::io::physical::PhysicalFileSystem;
use pr_test_support::TempRoot;
use pr_test_support::golden;
use serde_json::{Value, json};

fn fs() -> PhysicalFileSystem {
    PhysicalFileSystem
}

/// `photos\a.jpg` with three bytes, the photo folder every case needs.
fn photos_folder(root: &TempRoot, name: &str) -> String {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).expect("create photo folder");
    std::fs::write(dir.join("a.jpg"), [1, 2, 3]).expect("write photo");
    dir.to_string_lossy().into_owned()
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The C# `ValidateCache` helper: the same protected set `PerfSession.ValidatePaths` uses (source, out dir, both app data folders).
fn validate_cache(cache: &str, photos: &str, out: &str) -> Result<(), ToolError> {
    let app_data = app_data_folder();
    let csharp_app_data = csharp_app_data_folder();
    ensure_cache_directory(
        &fs(),
        cache,
        &[
            (photos, "photo folder"),
            (out, "output directory"),
            (app_data.as_str(), "app data folder"),
            (csharp_app_data.as_str(), "app data folder"),
        ],
    )
}

/// Creates a directory junction (`mklink /J`), which resolves like a symlink for the guard.
fn make_junction(link: &Path, target: &Path) {
    let status = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .status()
        .expect("run cmd /C mklink /J");
    assert!(
        status.success(),
        "mklink /J {} {} failed",
        link.display(),
        target.display()
    );
}

// C#: BenchmarkCliHardeningTests.CacheDir_OverlappingThePhotoFolder_IsRefused (T-B-05)
#[test]
fn cache_dir_overlapping_the_photo_folder_is_refused() {
    let root = TempRoot::new("cli-cache-photo");
    let photos = photos_folder(&root, "cache-photos");
    let out = text(&root.join("cache-out"));
    std::fs::create_dir_all(Path::new(&photos).join("cache")).expect("cache subfolder");
    let exported = Path::new(&photos).join("cache").join("export.png");
    std::fs::write(&exported, [1]).expect("write export");

    assert_eq!(
        validate_cache(&photos, &photos, &out).expect_err("equal").type_name,
        "InvalidOperationException"
    );
    let sub = text(&Path::new(&photos).join("sub"));
    assert_eq!(
        validate_cache(&sub, &photos, &out).expect_err("inside").type_name,
        "InvalidOperationException"
    );
    let parent = text(root.path());
    assert_eq!(
        validate_cache(&parent, &photos, &out).expect_err("ancestor").type_name,
        "InvalidOperationException"
    );
    assert!(exported.exists());
}

// C#: BenchmarkCliHardeningTests.CacheDir_OverlappingOutDirOrAppData_IsRefused (T-B-05)
#[test]
fn cache_dir_overlapping_out_dir_or_app_data_is_refused() {
    let root = TempRoot::new("cli-cache-out");
    let photos = photos_folder(&root, "cache-photos");
    let out = text(&root.join("cache-out"));
    let inside_out = text(&Path::new(&out).join("data").join("cache"));
    assert_eq!(
        validate_cache(&out, &photos, &out).expect_err("equal out").type_name,
        "InvalidOperationException"
    );
    assert_eq!(
        validate_cache(&inside_out, &photos, &out)
            .expect_err("inside out")
            .type_name,
        "InvalidOperationException"
    );
    let app_data = app_data_folder();
    assert_eq!(
        validate_cache(&app_data, &photos, &out)
            .expect_err("app data")
            .type_name,
        "InvalidOperationException"
    );
    let inside_app_data = text(&Path::new(&app_data).join("cache"));
    assert_eq!(
        validate_cache(&inside_app_data, &photos, &out)
            .expect_err("inside app data")
            .type_name,
        "InvalidOperationException"
    );
}

// C#: BenchmarkCliHardeningTests.CacheDir_NeedsToBeEmptyOrMarked (T-B-05)
#[test]
fn cache_dir_needs_to_be_empty_or_marked() {
    let root = TempRoot::new("cli-cache-marker");
    let photos = photos_folder(&root, "cache-photos");
    let out = text(&root.join("cache-out"));

    let foreign = root.join("foreign-cache");
    std::fs::create_dir_all(&foreign).expect("foreign cache");
    std::fs::write(foreign.join("keep.png"), [1]).expect("keep file");
    let err = validate_cache(&text(&foreign), &photos, &out).expect_err("foreign, not empty");
    assert_eq!(err.type_name, "InvalidOperationException");
    assert!(err.message.contains("not empty"), "{}", err.message);

    validate_cache(&text(&root.join("missing-cache")), &photos, &out).expect("missing folder is accepted");
    let empty = root.join("empty-cache");
    std::fs::create_dir_all(&empty).expect("empty cache");
    validate_cache(&text(&empty), &photos, &out).expect("empty folder is accepted");

    let owned = text(&root.join("owned-cache"));
    claim_cache_directory(&owned).expect("claim");
    std::fs::write(Path::new(&owned).join("x.png"), [1]).expect("cache file");
    validate_cache(&owned, &photos, &out).expect("second run in the same batch keeps working");
    assert!(Path::new(&owned).join(CACHE_MARKER).exists());
}

// C#: BenchmarkCliHardeningTests.OutputDirectoryGuard_EqualInsideAncestor (T-B-03)
#[test]
fn output_directory_guard_equal_inside_ancestor() {
    let root = TempRoot::new("cli-guard-equal");
    let photos = photos_folder(&root, "guard-photos");
    let sep = std::path::MAIN_SEPARATOR;
    let reports = text(&Path::new(&photos).join("reports"));
    let parent = text(root.path());
    let upper = photos.to_ascii_uppercase();
    assert_eq!(
        ensure_output_directory(&fs(), &photos, &photos)
            .expect_err("equal")
            .type_name,
        "InvalidOperationException"
    );
    assert_eq!(
        ensure_output_directory(&fs(), &format!("{photos}{sep}"), &photos)
            .expect_err("trailing separator")
            .type_name,
        "InvalidOperationException"
    );
    assert_eq!(
        ensure_output_directory(&fs(), &reports, &photos)
            .expect_err("inside")
            .type_name,
        "InvalidOperationException"
    );
    assert_eq!(
        ensure_output_directory(&fs(), &parent, &photos)
            .expect_err("ancestor")
            .type_name,
        "InvalidOperationException"
    );
    assert_eq!(
        ensure_output_directory(&fs(), &upper, &photos)
            .expect_err("case")
            .type_name,
        "InvalidOperationException"
    );

    let sibling = text(&root.join("guard-photos-reports"));
    ensure_output_directory(&fs(), &sibling, &photos).expect("a sibling whose name merely starts the same is accepted");
    assert!(!overlaps(&fs(), &sibling, &photos).expect("overlaps"));
}

// C#: BenchmarkCliHardeningTests.ResolveReportDirectory_Overlapping_IsRefused (T-B-03; `BenchmarkCliArguments.ResolveReportDirectory`)
#[test]
fn resolve_report_directory_overlapping_is_refused() {
    let root = TempRoot::new("cli-batch-report-dir");
    let photos = photos_folder(&root, "batch-photos");
    let env = SystemToolEnvironment;
    let inside = text(&Path::new(&photos).join("r"));
    assert_eq!(
        resolve_report_directory(&fs(), &env, &photos, Some(&photos))
            .expect_err("equal")
            .type_name,
        "InvalidOperationException"
    );
    assert_eq!(
        resolve_report_directory(&fs(), &env, &photos, Some(&inside))
            .expect_err("inside")
            .type_name,
        "InvalidOperationException"
    );
    let sibling = text(&root.join("batch-reports"));
    assert_eq!(
        resolve_report_directory(&fs(), &env, &photos, Some(&sibling)).expect("sibling"),
        sibling
    );

    let default = resolve_report_directory(&fs(), &env, &photos, None).expect("default");
    let prefix = std::env::temp_dir().join("PhotoReview-Benchmark-Reports");
    assert!(
        default
            .to_ascii_lowercase()
            .starts_with(&text(&prefix).to_ascii_lowercase()),
        "default report folder {default} is not under {}",
        prefix.display()
    );
}

// C#: BenchmarkCliJunctionTests.OutDirJunctionIntoThePhotoFolder_IsRefused (T-B-13, Native; guard half only)
#[test]
#[ignore = "native: junction"]
fn out_dir_junction_into_the_photo_folder_is_refused() {
    let root = TempRoot::new("cli-junction");
    let photos = photos_folder(&root, "photos");
    let link = root.join("link-to-photos");
    make_junction(&link, Path::new(&photos));
    let link = text(&link);
    assert_eq!(
        ensure_output_directory(&fs(), &text(&Path::new(&link).join("reports")), &photos)
            .expect_err("junction reports")
            .type_name,
        "InvalidOperationException"
    );
    assert_eq!(
        ensure_output_file(&fs(), &text(&Path::new(&link).join("report.md")), &photos)
            .expect_err("junction file")
            .type_name,
        "InvalidOperationException"
    );
    assert_eq!(
        ensure_output_directory(&fs(), &link, &photos)
            .expect_err("junction itself")
            .type_name,
        "InvalidOperationException"
    );
}

// C#: BenchmarkCliReportWriteTests.HardLinkedReportPath_DoesNotOverwriteThePhoto (R13, Native)
#[test]
#[ignore = "native: hard link"]
fn hard_linked_report_path_does_not_overwrite_the_photo() {
    let root = TempRoot::new("cli-hardlink");
    let photos = root.join("photos");
    std::fs::create_dir_all(&photos).expect("photos");
    let photo = photos.join("IMG_0001.CR2");
    std::fs::write(&photo, [1, 2, 3, 4]).expect("photo");
    let reports = root.join("reports");
    std::fs::create_dir_all(&reports).expect("reports");
    let report = reports.join("summary.md");
    std::fs::hard_link(&photo, &report).expect("hard link");

    write_report_file(&text(&report), "report text").expect("write report");

    assert_eq!(std::fs::read(&photo).expect("read photo"), vec![1, 2, 3, 4]);
    assert_eq!(std::fs::read_to_string(&report).expect("read report"), "report text");
}

// C#: BenchmarkCliReportWriteTests.SymlinkedReportPath_DoesNotOverwriteThePhoto (R13, Native; skipped without symlink privilege)
#[test]
#[ignore = "native: symlink"]
fn symlinked_report_path_does_not_overwrite_the_photo() {
    let root = TempRoot::new("cli-symlink");
    let photos = root.join("photos");
    std::fs::create_dir_all(&photos).expect("photos");
    let photo = photos.join("IMG_0002.CR2");
    std::fs::write(&photo, [9, 8, 7]).expect("photo");
    let reports = root.join("reports");
    std::fs::create_dir_all(&reports).expect("reports");
    let report = reports.join("summary.md");
    if let Err(err) = std::os::windows::fs::symlink_file(&photo, &report) {
        // C# skips here too: file symlink creation needs a privilege this account may not have.
        eprintln!("skipped: file symlink creation needs privilege ({err})");
        return;
    }

    write_report_file(&text(&report), "report text").expect("write report");

    assert_eq!(std::fs::read(&photo).expect("read photo"), vec![9, 8, 7]);
    assert_eq!(std::fs::read_to_string(&report).expect("read report"), "report text");
    assert!(
        std::fs::symlink_metadata(&report)
            .expect("metadata")
            .file_type()
            .is_file()
    );
}

// C#: BenchmarkCliReportWriteTests.DefaultReportDirectories_AreUnique (R30; not Native: a temp folder and the real environment)
#[test]
fn default_report_directories_are_unique() {
    let root = TempRoot::new("cli-default-unique");
    let photos = photos_folder(&root, "photos2");
    let env = SystemToolEnvironment;
    let first = resolve_report_directory(&fs(), &env, &photos, None).expect("first");
    let second = resolve_report_directory(&fs(), &env, &photos, None).expect("second");
    assert_ne!(first, second);
}

// ---- golden: tests/golden/cli-tool-path-guard.json -------------------------------------------------------------------

fn golden_inputs(name: &str) -> std::collections::HashMap<String, Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .expect("golden input has cases")
        .iter()
        .map(|c| (golden::str_of(c, "name").to_owned(), c.clone()))
        .collect()
}

/// A golden path: `""` is the temp root, `@` the C# app data folder, `@/x` that folder plus `/x` (as written), anything
/// else is joined to the temp root component by component.
fn resolve(root: &Path, rel: &str, app_data: &str) -> String {
    if rel.is_empty() {
        return text(root);
    }
    if rel == "@" {
        return app_data.to_owned();
    }
    if let Some(rest) = rel.strip_prefix("@/") {
        return format!("{app_data}/{rest}");
    }
    rel.split('/')
        .filter(|part| !part.is_empty())
        .fold(root.to_path_buf(), |path, part| path.join(part))
        .to_string_lossy()
        .into_owned()
}

fn has_junction(input: &Value) -> bool {
    input["tree"]
        .as_array()
        .is_some_and(|tree| tree.iter().any(Value::is_object))
}

/// Creates the golden `tree`: `dir/` directories, plain files (the cache marker gets its real text), and junctions.
fn build_tree(root: &Path, tree: &Value) {
    for entry in tree.as_array().into_iter().flatten() {
        if let Some(dir) = entry.as_str().and_then(|s| s.strip_suffix('/')) {
            std::fs::create_dir_all(root.join(dir)).expect("create golden directory");
        } else if let Some(file) = entry.as_str() {
            let path = file.split('/').fold(root.to_path_buf(), |p, part| p.join(part));
            std::fs::create_dir_all(path.parent().expect("file has a parent")).expect("create golden parent");
            let is_marker = path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(CACHE_MARKER));
            let body: &[u8] = if is_marker { CACHE_MARKER_TEXT.as_bytes() } else { b"x" };
            std::fs::write(&path, body).expect("create golden file");
        } else {
            let link = entry["junction"].as_str().expect("junction name");
            let target = resolve(root, entry["target"].as_str().expect("junction target"), "");
            let link_path = root.join(link);
            make_junction(&link_path, Path::new(&target));
        }
    }
}

/// Replaces the temp root and the app data folder by their golden placeholders.
fn scrub(message: &str, root: &Path, app_data: &str) -> String {
    message.replace(&text(root), "{root}").replace(app_data, "{appdata}")
}

fn execute(input: &Value, root: &Path, app_data: &str) -> Result<Value, ToolError> {
    let fs = fs();
    let env = SystemToolEnvironment;
    let at = |key: &str| resolve(root, golden::str_of(input, key), app_data);
    match golden::str_of(input, "op") {
        "output_directory" => ensure_output_directory(&fs, &at("out"), &at("photo")).map(|()| Value::Null),
        "output_file" => ensure_output_file(&fs, &at("file"), &at("photo")).map(|()| Value::Null),
        "overlaps" => overlaps(&fs, &at("a"), &at("b")).map(|v| json!(v)),
        "same_or_under" => is_same_or_under(&fs, &at("path"), &at("root")).map(|v| json!(v)),
        "cache_directory" => {
            let must: Vec<(String, String)> = input["must"]
                .as_array()
                .expect("must list")
                .iter()
                .map(|m| {
                    (
                        resolve(root, golden::str_of(m, "path"), app_data),
                        golden::str_of(m, "name").to_owned(),
                    )
                })
                .collect();
            let borrowed: Vec<(&str, &str)> = must.iter().map(|(p, n)| (p.as_str(), n.as_str())).collect();
            ensure_cache_directory(&fs, &at("cache"), &borrowed).map(|()| Value::Null)
        }
        "claim_cache" => claim_cache_directory(&at("cache")).map(|()| Value::Null),
        "report_directory" => {
            let photo = at("photo");
            let override_text = if let Some(raw) = input.get("override_raw") {
                Some(raw.as_str().expect("override_raw is a string").to_owned())
            } else {
                input.get("override").map(|_| at("override"))
            };
            resolve_report_directory(&fs, &env, &photo, override_text.as_deref()).map(Value::from)
        }
        "write_report" => write_report_file(&at("path"), golden::str_of(input, "content")).map(|()| Value::Null),
        other => panic!("unknown cli-tool-path-guard golden op '{other}'"),
    }
}

/// The golden `after` block: file-system facts the case must leave behind (checked whether or not the call threw).
fn check_after(input: &Value, after: &Value, root: &Path, app_data: &str, failures: &mut Vec<String>, name: &str) {
    match golden::str_of(input, "op") {
        "claim_cache" => {
            let marker = Path::new(&resolve(root, golden::str_of(input, "cache"), app_data)).join(CACHE_MARKER);
            if after["marker_exists"].as_bool() != Some(marker.exists()) {
                failures.push(format!("{name}: marker_exists mismatch at {}", marker.display()));
            }
            if marker.exists() {
                if let Some(want) = after.get("marker_text").and_then(Value::as_str) {
                    let got = std::fs::read_to_string(&marker).unwrap_or_default();
                    if got != want {
                        failures.push(format!("{name}: marker text {got:?}, want {want:?}"));
                    }
                }
                if let Some(bytes) = after.get("marker_bytes").and_then(Value::as_u64) {
                    let len = std::fs::metadata(&marker).map(|m| m.len()).unwrap_or(0);
                    if len != bytes {
                        failures.push(format!("{name}: marker has {len} bytes, want {bytes}"));
                    }
                }
            }
        }
        "report_directory" => {
            if let (Some(want), Some(_)) = (after.get("exists").and_then(Value::as_bool), input.get("override")) {
                let dir = resolve(root, golden::str_of(input, "override"), app_data);
                if Path::new(&dir).is_dir() != want {
                    failures.push(format!("{name}: exists={} expected {want}", Path::new(&dir).is_dir()));
                }
            }
        }
        "write_report" => {
            let path = PathBuf::from(resolve(root, golden::str_of(input, "path"), app_data));
            if let Some(bytes) = after.get("report_bytes").and_then(Value::as_array) {
                let want: Vec<u8> = bytes.iter().map(|b| b.as_u64().unwrap_or(0) as u8).collect();
                let got = std::fs::read(&path).unwrap_or_default();
                if got != want {
                    failures.push(format!("{name}: report bytes {got:?}, want {want:?}"));
                }
            }
            if let Some(want) = after.get("is_link").and_then(Value::as_bool) {
                let is_link = std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink());
                if is_link != want {
                    failures.push(format!("{name}: is_link={is_link}, want {want}"));
                }
            }
            if let Some(want) = after.get("leftover_tmp").and_then(Value::as_array) {
                let parent = path.parent().expect("report has a parent");
                let mut leftover: Vec<String> = std::fs::read_dir(parent)
                    .map(|entries| {
                        entries
                            .filter_map(Result::ok)
                            .map(|e| e.file_name().to_string_lossy().into_owned())
                            .filter(|n| n.ends_with(".tmp"))
                            .collect()
                    })
                    .unwrap_or_default();
                leftover.sort();
                let mut want: Vec<String> = want.iter().map(|v| v.as_str().unwrap_or_default().to_owned()).collect();
                want.sort();
                if leftover != want {
                    failures.push(format!("{name}: leftover tmp files {leftover:?}, want {want:?}"));
                }
            }
        }
        _ => {}
    }
}

/// Runs the golden cases whose junction-ness equals `native`; each case gets a fresh temp root.
fn run_guard_golden(native: bool) {
    let golden = golden::load("cli-tool-path-guard");
    let inputs = golden_inputs("cli-tool-path-guard");
    let mut failures = Vec::new();
    let mut ran = 0usize;
    for case in &golden.cases {
        let name = golden::str_of(case, "name");
        let input = inputs
            .get(name)
            .unwrap_or_else(|| panic!("no golden input named {name}"));
        if has_junction(input) != native {
            continue;
        }
        ran += 1;
        let root = TempRoot::new("cli-guard-golden");
        let app_data = csharp_app_data_folder();
        build_tree(root.path(), &input["tree"]);
        let got = execute(input, root.path(), &app_data);
        let scrubbed = match got {
            Ok(Value::String(s)) if s == "default-shape-ok" => Ok(Value::String(s)),
            Ok(Value::String(s)) => Ok(Value::String(scrub(&s, root.path(), &app_data))),
            Ok(v) => Ok(v),
            Err(e) => Err(ToolError::new(&e.type_name, scrub(&e.message, root.path(), &app_data))),
        };
        let expected = &case["result"];
        match (expected.get("throws"), scrubbed) {
            (Some(type_name), Err(err)) => {
                if type_name != err.type_name.as_str() || golden::str_of(expected, "message") != err.message {
                    failures.push(format!(
                        "{name}: want {type_name} {:?}, got {} {:?}",
                        expected["message"], err.type_name, err.message
                    ));
                }
            }
            (Some(type_name), Ok(value)) => failures.push(format!("{name}: want {type_name} error, got ok {value}")),
            (None, Ok(value)) => {
                let want = &expected["ok"];
                let shape_ok = want.as_str() == Some("default-shape-ok")
                    && value
                        .as_str()
                        .is_some_and(|s| s.contains("PhotoReview-Benchmark-Reports\\"));
                if !shape_ok && want != &value {
                    failures.push(format!("{name}: want ok {want}, got {value}"));
                }
            }
            (None, Err(err)) => failures.push(format!("{name}: want ok {}, got error {err}", expected["ok"])),
        }
        if let Some(after) = case.get("after") {
            check_after(input, after, root.path(), &app_data, &mut failures, name);
        }
    }
    assert!(ran > 0, "no golden cases selected (native = {native})");
    assert!(
        failures.is_empty(),
        "{} cli-tool-path-guard golden mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn golden_cli_tool_path_guard() {
    run_guard_golden(false);
}

#[test]
#[ignore = "native: junction"]
fn golden_cli_tool_path_guard_junctions() {
    run_guard_golden(true);
}
