//! Phase 7 benchmark parity report: Markdown plus JSON from a [`CompareReport`] (unit W10-B3, `contracts/W10-B3.md`).
//!
//! The Markdown is a presentation of the compare result: this module never recalculates a verdict, it only prints the
//! verdicts and numbers that [`crate::compare`] produced. C# has no report generator (the contract says so), so every
//! test is a `new:` test. Nothing here measures anything: a report is only as real as the summaries fed to the compare.

use crate::compare::{CompareReport, Thresholds};

/// One row of the attended production-surface table (B2). Free text cells; the attended session fills them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttendedRow {
    /// Display and DPI.
    pub display_dpi: String,
    /// Adapter.
    pub adapter: String,
    /// Keep-warm setting.
    pub keep_warm: String,
    /// Run label (for example `held-key 40 ms`).
    pub run: String,
    /// p50 ms, p95 ms, max ms (already formatted by the operator).
    pub timings: [String; 3],
    /// Frames over 50 ms.
    pub frames_over_50ms: String,
    /// IME observations.
    pub ime: String,
}

/// Everything the report needs besides the compare result.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportContext {
    /// `YYYY-MM-DD`.
    pub date: String,
    /// Machine name used in the file name.
    pub machine: String,
    /// Scenario name used in the file name.
    pub scenario: String,
    /// Path of the C# summary (or its repeat directory).
    pub csharp_summary: String,
    /// Path of the Rust summary.
    pub rust_summary: String,
    /// Thresholds the compare ran with.
    pub thresholds: Thresholds,
    /// Build/invocation identity lines.
    pub identity: Vec<(String, String)>,
    /// Machine/environment lines (CPU, GPU, RAM, OS, power plan, ...); only real, recorded values.
    pub environment: Vec<(String, String)>,
    /// Attended (B2) rows; empty means "not measured".
    pub attended: Vec<AttendedRow>,
    /// Waiver ADR reference, if a regression was waived.
    pub waiver: Option<String>,
    /// Extra evidence paths and commands.
    pub evidence: Vec<String>,
    /// Mark every numeric token `EXAMPLE` (placeholder reports and docs).
    pub example: bool,
}

/// `YYYY-MM-DD-<machine>-<scenario>` (file stem for the `.md` and `.json` pair under `docs/drafts/bench-parity/`).
pub fn file_stem(ctx: &ReportContext) -> String {
    format!("{}-{}-{}", ctx.date, ctx.machine, ctx.scenario)
}

/// Milliseconds with 2 decimals, ties away from zero, invariant decimal point, no thousands separator;
/// non-finite values print `n/a`.
pub fn format_ms(value: f64) -> String {
    if !value.is_finite() {
        return "n/a".to_owned();
    }
    // Six decimals first (shortest correct rounding of the double), then decimal half-up on the third digit.
    let text = format!("{:.6}", value.abs());
    let (int_part, frac) = text.split_once('.').unwrap_or((&text, "000000"));
    let mut cents = int_part.parse::<u64>().unwrap_or(0) * 100 + frac[..2].parse::<u64>().unwrap_or(0);
    if frac.as_bytes()[2] >= b'5' {
        cents += 1;
    }
    let sign = if value < 0.0 && cents > 0 { "-" } else { "" };
    format!("{sign}{}.{:02}", cents / 100, cents % 100)
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// JSON number token or `null` for NaN/infinity (never `NaN`/`Infinity`).
fn json_num(v: f64) -> String {
    if v.is_finite() {
        format!("{v}")
    } else {
        "null".to_owned()
    }
}

fn json_opt(v: Option<f64>) -> String {
    v.map_or_else(|| "null".to_owned(), json_num)
}

fn json_opt_str(v: &Option<String>) -> String {
    v.as_deref().map_or_else(|| "null".to_owned(), json_string)
}

fn stats(ms: &[f64; 3]) -> String {
    format!(
        "{{\"p50Ms\":{},\"p95Ms\":{},\"p99Ms\":null,\"maxMs\":{}}}",
        json_num(ms[0]),
        json_num(ms[1]),
        json_num(ms[2])
    )
}

/// The compare JSON of the contract (`schemaVersion` 1), compact, with the compare verdicts as they are.
pub fn to_json(report: &CompareReport, ctx: &ReportContext) -> String {
    let profiles: Vec<String> = report
        .profiles
        .iter()
        .map(|p| {
            format!(
                "{{\"profileId\":{},\"workload\":{},\"correctnessOnly\":{},\"verdict\":{},\"reason\":{},\"repeatCount\":{},\"sampleCountCsharp\":{},\"sampleCountRust\":{},\"csharp\":{},\"rust\":{},\"allowedP95DeltaMs\":{}}}",
                json_string(&p.profile_id),
                json_string(&p.workload.to_string()),
                p.correctness_only,
                json_string(p.verdict.name()),
                json_string(&p.reason),
                p.repeat_count,
                p.sample_count,
                p.sample_count_rust,
                stats(&p.csharp_ms),
                stats(&p.rust_ms),
                json_opt(p.allowed_p95_delta_ms)
            )
        })
        .collect();
    let diagnostics: Vec<String> = report
        .diagnostics
        .iter()
        .map(|d| {
            format!(
                "{{\"name\":{},\"csharp\":{},\"rust\":{},\"delta\":{}}}",
                json_string(&format!("{}/{}", d.profile_id, d.name)),
                json_opt_str(&d.csharp),
                json_opt_str(&d.rust),
                json_opt_str(&d.delta)
            )
        })
        .collect();
    let notes: Vec<String> = report.notes.iter().map(|n| json_string(n)).collect();
    format!(
        "{{\"schemaVersion\":1,\"scenario\":{},\"overallVerdict\":{},\"exitCode\":{},\"thresholds\":{{\"ratioTolerance\":{},\"absoluteFloorMs\":{},\"minimumRepeats\":{}}},\"csharpSummary\":{},\"rustSummary\":{},\"datasetEqual\":{},\"notes\":[{}],\"diagnosticMetrics\":[{}],\"profiles\":[{}]}}",
        json_string(&ctx.scenario),
        json_string(report.verdict.name()),
        report.exit_code(),
        json_num(ctx.thresholds.ratio_tolerance),
        json_num(ctx.thresholds.absolute_floor_ms),
        ctx.thresholds.minimum_repeats,
        json_string(&ctx.csharp_summary),
        json_string(&ctx.rust_summary),
        report.dataset_equal,
        notes.join(","),
        diagnostics.join(","),
        profiles.join(",")
    )
}

fn cell(ctx: &ReportContext, text: String) -> String {
    if ctx.example { format!("EXAMPLE {text}") } else { text }
}

fn pair(ctx: &ReportContext, a: String, b: String) -> String {
    cell(ctx, format!("{a}/{b}"))
}

fn kv_block(rows: &[(String, String)], empty: &str) -> String {
    if rows.is_empty() {
        return format!("{empty}\n");
    }
    rows.iter().map(|(k, v)| format!("- {k}: {v}\n")).collect()
}

/// Markdown report with the ten fixed sections of the contract.
pub fn render_markdown(report: &CompareReport, ctx: &ReportContext) -> String {
    let mut out = String::new();
    let t = &ctx.thresholds;
    out.push_str(&format!(
        "# Benchmark parity: {} ({}, {})\n\n- Overall verdict: {} (exit code {})\n\n",
        ctx.scenario,
        ctx.date,
        ctx.machine,
        report.verdict.name(),
        report.exit_code()
    ));
    out.push_str("## 2. Build and invocation identity\n\n");
    out.push_str(&kv_block(&ctx.identity, "Not recorded."));
    out.push_str("\n## 3. Machine and environment\n\n");
    out.push_str(&kv_block(&ctx.environment, "Not recorded."));
    out.push_str("\n## 4. Dataset manifest and profile selection\n\n");
    out.push_str(&format!(
        "- Dataset equal in every pair: {}\n- Profiles compared: {}\n",
        report.dataset_equal,
        report.profiles.len()
    ));
    out.push_str("\n## 5. C#/Rust benchmark comparison\n\n");
    out.push_str("| Profile | Workload | Status C#/Rust | Count C#/Rust | p50 C#/Rust (ms) | p95 C#/Rust (ms) | max C#/Rust (ms) | delta p95 (ms) | allowed delta (ms) | verdict |\n");
    out.push_str("|---|---|---|---:|---:|---:|---:|---:|---:|---|\n");
    for p in &report.profiles {
        let (c, r) = (&p.csharp_ms, &p.rust_ms);
        let delta = if p.verdict == crate::compare::Verdict::Incomparable {
            "n/a".to_owned()
        } else {
            format_ms(r[1] - c[1])
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            p.profile_id,
            p.workload,
            p.status.map(|s| s.to_string()).join("/"),
            pair(ctx, p.sample_count.to_string(), p.sample_count_rust.to_string()),
            pair(ctx, format_ms(c[0]), format_ms(r[0])),
            pair(ctx, format_ms(c[1]), format_ms(r[1])),
            pair(ctx, format_ms(c[2]), format_ms(r[2])),
            cell(ctx, delta),
            cell(ctx, p.allowed_p95_delta_ms.map_or_else(|| "n/a".to_owned(), format_ms)),
            p.verdict.name()
        ));
    }
    for p in &report.profiles {
        if p.verdict == crate::compare::Verdict::Incomparable || p.verdict == crate::compare::Verdict::Skipped {
            out.push_str(&format!("\n- {} {}: {}\n", p.profile_id, p.verdict.name(), p.reason));
        }
    }
    for n in &report.notes {
        out.push_str(&format!("\n- Note: {n}\n"));
    }
    out.push_str("\n## 6. Exact metrics parity (diagnostic, never fails the gate)\n\n");
    if report.diagnostics.is_empty() {
        out.push_str("No diagnostic metrics in the summaries.\n");
    } else {
        out.push_str("| Metric | C# | Rust | Equal | Notes |\n|---|---|---|---|---|\n");
        for d in &report.diagnostics {
            let equal = d.csharp == d.rust;
            out.push_str(&format!(
                "| {}/{} | {} | {} | {} | delta {} |\n",
                d.profile_id,
                d.name,
                d.csharp.as_deref().unwrap_or("absent"),
                d.rust.as_deref().unwrap_or("absent"),
                if equal { "yes" } else { "no" },
                d.delta.as_deref().unwrap_or("n/a")
            ));
        }
    }
    out.push_str("\n## 7. Attended production surface (B2)\n\n");
    if ctx.attended.is_empty() {
        out.push_str("Not measured: attended measurement (W10-B2) not done.\n");
    } else {
        out.push_str("| Display/DPI | Adapter | Keep-warm | Run | p50 ms | p95 ms | max ms | frames >50 ms | IME observations |\n|---|---|---|---|---:|---:|---:|---:|---|\n");
        for a in &ctx.attended {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                a.display_dpi,
                a.adapter,
                a.keep_warm,
                a.run,
                a.timings[0],
                a.timings[1],
                a.timings[2],
                a.frames_over_50ms,
                a.ime
            ));
        }
    }
    out.push_str("\n## 8. Decision and waiver record\n\n");
    out.push_str(&format!(
        "- B1 `D-W10BENCH-1` / `D-W10BENCH-5`: {} paired repeats, median repeat statistics, `allowed = max({} * |B|, {} ms)`, strict majority for Regression; only a p95 Regression fails (exit 1). Metrics are diagnostic.\n",
        t.minimum_repeats,
        format_pct(t.ratio_tolerance),
        format_ms(t.absolute_floor_ms)
    ));
    out.push_str("- B2 `D-W8B3S-11` + `D-W10BENCH-6`: production p95 is actionable only after P0; above 10 ms starts W8-12j, clean below 8.5 ms cancels it.\n");
    match &ctx.waiver {
        Some(w) => out.push_str(&format!(
            "- Waiver: {w} (needs the ADR and a `Different` PARITY row).\n"
        )),
        None => out.push_str("- Waiver: none.\n"),
    }
    out.push_str("\n## 9. Not measured / limitations\n\n");
    out.push_str("- Native decode correctness over the full RAW/HEIC corpus, UI behavior outside B2's attended scenarios, GPU completion time, disk-cold state, long-term soak and unrecorded hardware are not parity passes.\n");
    if ctx.attended.is_empty() {
        out.push_str("- Attended production measurement (W10-B2) was not done.\n");
    }
    out.push_str("\n## 10. Raw evidence paths and commands\n\n");
    out.push_str(&format!(
        "- C# summary: {}\n- Rust summary: {}\n",
        ctx.csharp_summary, ctx.rust_summary
    ));
    for e in &ctx.evidence {
        out.push_str(&format!("- {e}\n"));
    }
    out
}

fn format_pct(ratio: f64) -> String {
    format!(
        "{}%",
        format_ms(ratio * 100.0).trim_end_matches('0').trim_end_matches('.')
    )
}
