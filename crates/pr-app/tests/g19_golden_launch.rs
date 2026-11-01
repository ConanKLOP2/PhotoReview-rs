//! G18 (docs/reviews/pr-164.md M1/L3): every case of `tests/golden/shell-launch.json` is compared field by field
//! (the older `shell_launch_golden_cases_match_oracle` only parsed `--smoke`). Fakes for the file system; the
//! current directory is moved only for the `cwd` cases and restored by a guard.
#![cfg(windows)]
use pr_app::startup::launch::{INSTANCE_NAME_PREFIX, SMOKE_ARGUMENT, ShellLaunch};
use pr_test_support::golden::{self, str_of};
use pr_test_support::{FakeFileSystem, RecordingLog, TempRoot};
use serde_json::Value;
use std::path::PathBuf;

struct CwdGuard(PathBuf);
impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn shell_launch_matches_every_golden_case() {
    let g = golden::load("shell-launch");
    let temp = TempRoot::new("g19-golden-launch");
    let root = temp.path().to_string_lossy().into_owned();
    let drive = root.get(..2).unwrap().to_owned();
    let drive_root = format!("{drive}\\");
    let expand = |s: &str| {
        s.replace("$DRIVEROOT", &drive_root)
            .replace("$DRIVE", &drive)
            .replace("$ROOT", &root)
    };
    let opt = |v: &Value| v.as_str().map(&expand);

    let mut launches = 0;
    for case in &g.cases {
        if case["kind"] == "constants" {
            assert_eq!(str_of(case, "smoke_argument"), SMOKE_ARGUMENT);
            // The prefix is intentionally different from C# (D-W8B6-2): only the argument is shared.
            assert_ne!(str_of(case, "prefix"), INSTANCE_NAME_PREFIX);
            continue;
        }
        let id = str_of(case, "id");
        // The oracle root holds a.jpg, photos\b.jpg and the folders photos, photos\sub; cwd cases use relative names.
        let fs = FakeFileSystem::new();
        let cwd = opt(&case["cwd"]);
        for base in [root.as_str(), ""] {
            let join = |p: &str| {
                if base.is_empty() {
                    p.to_owned()
                } else {
                    format!("{base}\\{p}")
                }
            };
            fs.add_file(join("a.jpg"), b"a");
            fs.add_file(join("photos\\b.jpg"), b"b");
            fs.add_dir(join("photos\\sub"));
        }
        fs.add_dir(&drive_root);
        // The fake does not resolve `..` the way the OS does: register the spelling the case passes.
        fs.add_dir(format!("{root}\\photos\\sub\\.."));

        let guard = cwd.as_ref().map(|dir| {
            let guard = CwdGuard(std::env::current_dir().unwrap());
            std::env::set_current_dir(dir).unwrap();
            guard
        });
        let args: Vec<String> = case["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| expand(a.as_str().unwrap()))
            .collect();
        let launch = ShellLaunch::parse(&args, &fs, &RecordingLog::new());
        drop(guard);

        let want_args: Vec<String> = strings(&case["arguments"]).iter().map(|s| expand(s)).collect();
        assert_eq!(launch.arguments, want_args, "case {id}: arguments");
        assert_eq!(
            launch.initial_file,
            opt(&case["initial_file"]),
            "case {id}: initial_file"
        );
        assert_eq!(
            launch.initial_folder,
            opt(&case["initial_folder"]),
            "case {id}: initial_folder"
        );
        assert_eq!(
            launch.launch_folder,
            opt(&case["launch_folder"]),
            "case {id}: launch_folder"
        );
        assert_eq!(
            launch.initial_path().map(str::to_owned),
            opt(&case["initial_path"]),
            "case {id}: initial_path"
        );
        assert_eq!(launch.smoke, case["smoke"].as_bool().unwrap(), "case {id}: smoke");
        let want_existing: Vec<String> = strings(&case["existing_paths"]).iter().map(|s| expand(s)).collect();
        assert_eq!(
            launch.existing_paths(),
            want_existing.as_slice(),
            "case {id}: existing_paths"
        );
        launches += 1;
    }
    assert_eq!(launches, 21);
}
