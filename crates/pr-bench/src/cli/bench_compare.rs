//! W10-B1/B3 CLI composition; vocabulary from C# PerfCompare/PerfCompareCommand.cs.
//! Reads only explicitly supplied summaries; never invokes the benchmark backend.

use super::ToolEnvironment;
use crate::{
    compare::{self, CompareReport, Summary, Thresholds, Verdict},
    report::{self, ReportContext},
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const USAGE: &str = "--bench-compare <csharp-summary.json|repeat-dir> <rust-summary.json|repeat-dir> [--thresholds <file>] [--out <dir>]";

fn summaries(path: &Path) -> Result<Vec<Summary>, String> {
    let mut paths = if path.is_dir() {
        let entries = fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
            {
                paths.push(entry.path());
            }
        }
        paths
    } else {
        vec![path.to_owned()]
    };
    paths.sort();
    paths
        .iter()
        .map(|p| {
            let text = fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
            compare::parse_summary(&text).map_err(|e| format!("{}: {e}", p.display()))
        })
        .collect()
}

pub(super) fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write, env: &dyn ToolEnvironment) -> i32 {
    match execute(args, out, env) {
        Ok(code) => code,
        Err((code, message)) => {
            let _ = writeln!(err, "--bench-compare: {message}");
            code
        }
    }
}

fn execute(args: &[String], out: &mut dyn Write, env: &dyn ToolEnvironment) -> Result<i32, (i32, String)> {
    let usage = || (2, USAGE.to_owned());
    if args.len() < 3 || args[1..3].iter().any(|a| a.starts_with("--")) {
        return Err(usage());
    }
    let mut thresholds = Thresholds::default();
    let mut directory = PathBuf::from("docs/drafts/bench-parity");
    let mut seen = std::collections::HashSet::new();
    for option in args[3..].chunks(2) {
        if option.len() != 2 || !seen.insert(&option[0]) {
            return Err(usage());
        }
        match option[0].as_str() {
            "--thresholds" => {
                let text = fs::read_to_string(&option[1]).map_err(|e| (2, format!("threshold file: {e}")))?;
                thresholds = compare::parse_thresholds(&text).map_err(|e| (2, e.to_string()))?;
            }
            "--out" => directory = PathBuf::from(&option[1]),
            _ => return Err(usage()),
        }
    }
    let result = (|| {
        let csharp = summaries(Path::new(&args[1]))?;
        let rust = summaries(Path::new(&args[2]))?;
        compare::compare_repeats(&csharp, &rust, &thresholds).map_err(|e| e.to_string())
    })();
    let report = result.unwrap_or_else(|reason| CompareReport {
        verdict: Verdict::Incomparable,
        profiles: Vec::new(),
        notes: vec![reason],
        dataset_equal: false,
        diagnostics: Vec::new(),
    });
    let stamp = env.utc_stamp();
    let date = if stamp.len() >= 8 {
        format!("{}-{}-{}", &stamp[..4], &stamp[4..6], &stamp[6..8])
    } else {
        "unknown-date".to_owned()
    };
    let machine: String = std::env::var("COMPUTERNAME")
        .unwrap_or_else(|_| "unrecorded-machine".to_owned())
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    let ctx = ReportContext {
        date,
        machine,
        scenario: "bench-compare".to_owned(),
        csharp_summary: args[1].clone(),
        rust_summary: args[2].clone(),
        thresholds,
        identity: vec![("Comparison invocation".to_owned(), format!("{args:?}"))],
        environment: Vec::new(),
        attended: Vec::new(),
        waiver: None,
        evidence: vec![args[1].clone(), args[2].clone()],
        example: false,
    };
    let markdown = report::render_markdown(&report, &ctx);
    let write = || -> std::io::Result<()> {
        fs::create_dir_all(&directory)?;
        let stem = report::file_stem(&ctx);
        fs::write(directory.join(format!("{stem}.json")), report::to_json(&report, &ctx))?;
        fs::write(directory.join(format!("{stem}.md")), &markdown)
    };
    write().map_err(|e| (3, format!("report output: {e}")))?;
    out.write_all(markdown.as_bytes())
        .map_err(|e| (3, format!("stdout: {e}")))?;
    Ok(report.exit_code())
}
