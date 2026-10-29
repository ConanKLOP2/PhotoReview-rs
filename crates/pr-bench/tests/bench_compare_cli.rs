//! New W10-B1/B3 process tests: synthetic JSON only, no engine or timing measurements.
use pr_test_support::TempRoot;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
    sync::mpsc,
    time::Duration,
};

fn summary(p95: f64) -> String {
    json!({"Dataset":{"Folder":"synthetic fixture", "Files":["synthetic.jpg"], "UnsupportedExtensions":{}, "FileLimit":64},
        "Reports":[], "ProfileOutcomes":[{"ProfileId":"preview-light", "Workload":1, "Status":0,
        "Samples":[2.0,4.0], "Count":2, "P50":2.0, "P95":p95, "Max":p95}]}).to_string()
}

fn run(root: &TempRoot, args: &[&str]) -> Output {
    let child = Command::new(env!("CARGO_BIN_EXE_pr-bench"))
        .current_dir(root.path())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = match rx.recv_timeout(Duration::from_secs(30)) {
        Ok(output) => output.expect("output"),
        Err(error) => {
            #[cfg(windows)]
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .output();
            #[cfg(not(windows))]
            let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).output();
            panic!("owned CLI process {pid} exceeded timeout: {error}");
        }
    };
    waiter.join().expect("waiter");
    output
}

fn prepare(root: &TempRoot, p95: f64) {
    for (side, timing) in [("baseline", 4.0), ("candidate", p95)] {
        fs::create_dir(root.join(side)).expect("repeat directory");
        for i in 0..3 {
            fs::write(root.join(format!("{side}/{i:04}.json")), summary(timing)).expect("summary");
        }
    }
}

fn report(root: &TempRoot) -> (Value, String) {
    let entries: Vec<_> = fs::read_dir(root.join("out"))
        .expect("reports")
        .map(|e| e.expect("entry").path())
        .collect();
    assert_eq!(entries.len(), 2);
    let json_path = entries
        .iter()
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .expect("JSON");
    let md_path = json_path.with_extension("md");
    assert!(
        md_path
            .file_name()
            .expect("name")
            .to_string_lossy()
            .ends_with("-bench-compare.md")
    );
    (
        serde_json::from_str(&fs::read_to_string(json_path).expect("JSON text")).expect("JSON report"),
        fs::read_to_string(md_path).expect("Markdown"),
    )
}

// new: paired repeats propagate the p95 gate, serialized source and presentation.
#[test]
fn bench_compare_paired_repeats_writes_reports_and_exits_0_or_1() {
    for (timing, code, verdict) in [(4.0, 0, "Pass"), (4.5, 0, "WithinNoise"), (8.0, 1, "Regression")] {
        let root = TempRoot::new("bench compare spaces");
        prepare(&root, timing);
        let output = run(&root, &["--bench-compare", "baseline", "candidate", "--out", "out"]);
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let (json, md) = report(&root);
        assert_eq!(json["exitCode"], code);
        assert_eq!(json["overallVerdict"], verdict);
        assert_eq!(json["profiles"][0]["repeatCount"], 3);
        assert_eq!(String::from_utf8(output.stdout).expect("stdout"), md);
        assert!(md.contains(verdict));
        assert!(output.stderr.is_empty());
    }
}

// new: malformed/missing input, unequal/insufficient repeats and exact mismatch are exit 3 with evidence.
#[test]
fn bench_compare_invalid_inputs_write_incomparable_reports_and_exit_3() {
    for case in ["malformed", "missing", "few", "unequal", "status", "dataset", "null"] {
        let root = TempRoot::new("bench-incomparable");
        prepare(&root, 4.0);
        let target = root.join("candidate/0000.json");
        match case {
            "malformed" => fs::write(target, "{").expect("bad JSON"),
            "missing" => {
                fs::remove_dir_all(root.join("candidate")).expect("remove own fixture");
            }
            "few" | "unequal" => {
                fs::remove_file(root.join("candidate/0002.json")).expect("remove own fixture");
                if case == "few" {
                    fs::remove_file(root.join("baseline/0002.json")).expect("remove own fixture");
                }
            }
            _ => {
                let mut value: Value = serde_json::from_str(&summary(4.0)).expect("fixture");
                match case {
                    "status" => value["ProfileOutcomes"][0]["Status"] = json!(2),
                    "dataset" => value["Dataset"]["Folder"] = json!("different"),
                    _ => value["ProfileOutcomes"][0]["P95"] = Value::Null,
                }
                fs::write(target, value.to_string()).expect("invalid summary");
            }
        }
        let output = run(&root, &["--bench-compare", "baseline", "candidate", "--out", "out"]);
        assert_eq!(output.status.code(), Some(3), "case {case}");
        let (json, md) = report(&root);
        assert_eq!(json["overallVerdict"], "Incomparable");
        assert!(md.contains("Incomparable"));
    }
}

// new: threshold JSON is optional, strictly validated, and single-pair use must be explicit.
#[test]
fn bench_compare_threshold_override_and_usage_errors() {
    let root = TempRoot::new("bench-thresholds");
    fs::write(root.join("b.json"), summary(4.0)).expect("baseline");
    fs::write(root.join("r.json"), summary(8.0)).expect("candidate");
    fs::write(
        root.join("thresholds.json"),
        "\u{feff}{\"minimumRepeats\":1,\"absoluteFloorMs\":5,\"ratioTolerance\":0}",
    )
    .expect("thresholds");
    let args = [
        "--bench-compare",
        "b.json",
        "r.json",
        "--thresholds",
        "thresholds.json",
        "--out",
        "out",
    ];
    assert_eq!(run(&root, &args).status.code(), Some(0));
    assert_eq!(report(&root).0["thresholds"]["minimumRepeats"], 1);
    for bad in [
        "{",
        "[]",
        "{}x",
        "{\"typo\":1}",
        "{\"minimumRepeats\":0}",
        "{\"minimumRepeats\":1.5}",
        "{\"ratioTolerance\":-1}",
        "{\"absoluteFloorMs\":null}",
        "{\"ratioTolerance\":1e999}",
        "{\"minimumRepeats\":1,\"minimumRepeats\":3}",
    ] {
        fs::write(root.join("thresholds.json"), bad).expect("bad thresholds");
        assert_eq!(run(&root, &args).status.code(), Some(2), "{bad}");
    }
    for args in [
        vec!["--bench-compare"],
        vec!["--bench-compare", "b.json", "r.json", "--bad", "x"],
        vec!["--bench-compare", "b.json", "r.json", "--out"],
        vec!["--bench-compare", "b.json", "r.json", "--out", "x", "--out", "y"],
    ] {
        assert_eq!(run(&root, &args).status.code(), Some(2));
    }
}

// new: output I/O errors are environment failures, never successful comparisons.
#[test]
fn bench_compare_unwritable_output_exits_3() {
    let root = TempRoot::new("bench-output-failure");
    prepare(&root, 4.0);
    fs::write(root.join("out"), "file blocking directory").expect("block output");
    let output = run(&root, &["--bench-compare", "baseline", "candidate", "--out", "out"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("report output"));
    assert_eq!(
        fs::read_to_string(root.join(Path::new("out"))).expect("preserved"),
        "file blocking directory"
    );
}

// new: defaults retain the three-repeat policy and contract report location; BOM/CRLF work through the process.
#[test]
fn bench_compare_default_policy_and_output_location() {
    let root = TempRoot::new("bench-defaults");
    prepare(&root, 4.0);
    let output = run(
        &root,
        &[
            "--bench-compare",
            "baseline/0000.json",
            "candidate/0000.json",
            "--out",
            "out",
        ],
    );
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(report(&root).0["thresholds"]["minimumRepeats"], 3);
    for side in ["baseline", "candidate"] {
        fs::write(
            root.join(format!("{side}/0000.json")),
            format!("\u{feff}\r\n{}\r\n", summary(4.0)),
        )
        .expect("BOM/CRLF");
    }
    let output = run(&root, &["--bench-compare", "baseline", "candidate"]);
    assert_eq!(output.status.code(), Some(0));
    let files: Vec<_> = fs::read_dir(root.join("docs/drafts/bench-parity"))
        .expect("default reports")
        .map(|e| e.expect("report entry").path())
        .collect();
    assert_eq!(files.len(), 2);
    assert!(files.iter().any(|p| p.extension().is_some_and(|ext| ext == "md")));
    assert!(files.iter().any(|p| p.extension().is_some_and(|ext| ext == "json")));
}
