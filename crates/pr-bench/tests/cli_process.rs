//! Process-level checks: the `pr-bench` binary (`src/main.rs`) run as a child process, comparing stdout, stderr and the exit code.
//!
//! The contract's `cli-process` golden (`oracle/W7-03d.cs`, runs the BASELINE `PhotoReview.Benchmark.Cli.exe`) is NOT in
//! `tests/golden/` yet (the oracle has not been run for W7-03d), so its golden-driven test is not written here: the lead adds it
//! when the golden exists. The two tests below run now (usage text) or once the engine is wired (list profiles).
//!
//! Red stage: `main` calls `cli::run`, whose `classify` is `todo!()`, so the process panics (exit 101) until implemented.

use std::process::{Command, Output};

/// The usage text of `Program.cs` as the binary writes it: CRLF line ends, header first, 16 mode lines.
const USAGE: &str = concat!(
    "PhotoReview.Benchmark.Cli: unknown or missing mode. Supported modes:\r\n",
    "  --benchmark-list-profiles\r\n",
    "  --benchmark <folder> <profile,...>\r\n",
    "  --benchmark-all <folder> [output]\r\n",
    "  --benchmark-actions <folder> [output]\r\n",
    "  --ui-next-probe <folder> [--cache-dir DIR]\r\n",
    "  --perf-session ...\r\n",
    "  --preload-bench <folder> [n]\r\n",
    "  --perf-analyze <dir> [--rules <file>]\r\n",
    "  --startup-probe --shell <exe> [--wpf <exe>] --source <folder> [--count n --rounds n --warmup n --seed n --keys n --out dir --perf-json file]\r\n",
    "  compare <baseline.json> <current.json> [--threshold-percent 5 --min-abs-ms 1 --min-abs-mb 16 --busy-cpu 60 --max-cv 25 --min-samples 3 --json f --markdown f]\r\n",
    "  --perf-export <perf-session dir> [--out file.json]\r\n",
    "  --io-decode-split ...\r\n",
    "  --decoder-bench ...\r\n",
    "  --decoder-bench --raw <folder> <outDir> [iterations=3]\r\n",
    "  --explorer-probe <folder>\r\n",
    "  --raw-survey <dir> [--markdown <out.md>]\r\n",
);

fn run_pr_bench(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pr-bench"))
        .args(args)
        .output()
        .expect("start the pr-bench binary")
}

// C#: Program.cs, the no-arguments path (usage on stderr, nothing on stdout, exit 2). Pinned by the `no-args` golden shape.
#[test]
fn no_arguments_prints_usage_and_exits_2() {
    let output = run_pr_bench(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), USAGE);
}

// C#: Program.cs `--benchmark-list-profiles` (needs the real catalog, so it runs with the engine wired: lead step).
#[test]
#[ignore = "wiring: needs EngineBackend (W7-03d lead step); the stub main.rs uses UnwiredBackend"]
fn list_profiles_prints_eight_tab_separated_fields_per_line() {
    let output = run_pr_bench(&["--benchmark-list-profiles"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).expect("UTF-8");
    let lines: Vec<&str> = text.split("\r\n").filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines.len(),
        30,
        "the catalog has 30 profiles (27 runnable + 3 correctness-only)"
    );
    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 8, "{line}");
        assert!(fields[3].starts_with("mode="), "{line}");
        assert!(
            fields[6].starts_with("correctnessOnly=True") || fields[6].starts_with("correctnessOnly=False"),
            "{line}"
        );
    }
}
