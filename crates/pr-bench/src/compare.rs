//! Comparison of C# and Rust benchmark batch summaries (unit W10-B1, `contracts/W10-B1.md`, C# baseline d92d6aae).
//!
//! Pure functions only. [`parse_summary`] reads a `BenchmarkBatchSummary` JSON text (BOM and CRLF tolerant, own tiny
//! parser so `pr-bench` gets no new dependency); [`compare`] / [`compare_repeats`] apply the proposal D-W10BENCH-1:
//! median of the repeat-level statistics, `allowed = max(ratio * |B|, floor)`, strict-majority rule for Regression.
//! C# has no such gate: `PerfComparer` (5 percent, 1 ms, 3 samples) only supplies the default vocabulary.
//! Production timings are NOT produced here: they need the attended measurement (W10-B2) and the engine-backed backend.

use std::fmt;

use crate::profiles;

/// A phase row parsed from one `BenchmarkBatchSummary`.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Rows in the summary's `ProfileOutcomes` array.
    pub rows: Vec<PhaseRow>,
    /// Exact dataset manifest identity used for comparability.
    pub dataset: DatasetIdentity,
}

/// Exact dataset identity fields used before timing comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetIdentity {
    /// Fully qualified input folder.
    pub folder: String,
    /// Ordered supported file paths.
    pub files: Vec<String>,
    /// Unsupported extension counts in source key order.
    pub unsupported_extensions: Vec<(String, i64)>,
    /// Supported file limit.
    pub file_limit: i64,
}

/// One benchmark outcome row.
#[derive(Debug, Clone, PartialEq)]
pub struct PhaseRow {
    /// Stable profile identifier.
    pub profile_id: String,
    /// Workload enum value from the summary.
    pub workload: i64,
    /// Result status enum value from the summary (0 Pass, 1 Warn, 2 Fail, 3 InsufficientData).
    pub status: i64,
    /// Raw benchmark samples in milliseconds.
    pub samples_ms: Vec<f64>,
    /// p50 milliseconds (NaN when the summary has null/absent).
    pub p50_ms: f64,
    /// p95 milliseconds (NaN when the summary has null/absent).
    pub p95_ms: f64,
    /// Maximum milliseconds (NaN when the summary has null/absent).
    pub max_ms: f64,
    /// Correctness-only profile flag.
    pub correctness_only: bool,
    /// Diagnostic metrics retained in source order as name/value pairs; never used for the timing gate.
    pub metrics: Vec<DiagnosticMetric>,
}

/// One diagnostic snapshot value; counters and arrays are rendered as normalized scalar/JSON text.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticMetric {
    /// Snapshot field name.
    pub name: String,
    /// Stable normalized value for reporting.
    pub value: String,
}

/// Comparison policy values. Defaults are the W10-B1 proposal.
#[derive(Debug, Clone, PartialEq)]
pub struct Thresholds {
    /// Relative tolerance, default 5 percent.
    pub ratio_tolerance: f64,
    /// Absolute timing noise floor in milliseconds, default 1 ms.
    pub absolute_floor_ms: f64,
    /// Required independent sequential runs, default 3.
    pub minimum_repeats: usize,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            ratio_tolerance: 0.05,
            absolute_floor_ms: 1.0,
            minimum_repeats: 3,
        }
    }
}

/// Timing or exactness outcome for one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Meets the numeric equal-or-better gate.
    Pass,
    /// Difference is below the absolute or relative noise boundary.
    WithinNoise,
    /// Rust exceeds both allowed boundaries.
    Regression,
    /// Rust is faster by more than both boundaries.
    Improvement,
    /// Inputs cannot be compared.
    Incomparable,
    /// Symmetrically unavailable/not implemented profile.
    Skipped,
}

impl Verdict {
    /// The name written into the compare JSON.
    pub fn name(self) -> &'static str {
        match self {
            Self::Pass => "Pass",
            Self::WithinNoise => "WithinNoise",
            Self::Regression => "Regression",
            Self::Improvement => "Improvement",
            Self::Incomparable => "Incomparable",
            Self::Skipped => "Skipped",
        }
    }
}

/// One profile's comparison result.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileComparison {
    /// Profile identifier.
    pub profile_id: String,
    /// Workload enum value (C# side).
    pub workload: i64,
    /// Correctness-only profile (latency informational).
    pub correctness_only: bool,
    /// Final row verdict (the p95 verdict for performance profiles).
    pub verdict: Verdict,
    /// Human-readable explanation.
    pub reason: String,
    /// Number of paired repeats used.
    pub repeat_count: usize,
    /// Status value, C# and Rust (first repeat; NaN-free, -1 when missing).
    pub status: [i64; 2],
    /// C# median p50/p95/max in milliseconds (NaN when unavailable).
    pub csharp_ms: [f64; 3],
    /// Rust median p50/p95/max in milliseconds (NaN when unavailable).
    pub rust_ms: [f64; 3],
    /// Exact sample count of the C# side (first repeat).
    pub sample_count: usize,
    /// Exact sample count of the Rust side (first repeat).
    pub sample_count_rust: usize,
    /// `max(ratio * |B|, floor)` for p95, when computed.
    pub allowed_p95_delta_ms: Option<f64>,
}

/// One diagnostic metric reported as C#, Rust and delta; never fails the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticDelta {
    /// Profile the metric belongs to.
    pub profile_id: String,
    /// Snapshot field name.
    pub name: String,
    /// C# value text, if present.
    pub csharp: Option<String>,
    /// Rust value text, if present.
    pub rust: Option<String>,
    /// `rust - csharp` when both are integers.
    pub delta: Option<String>,
}

/// Aggregate report returned by the pure comparison function.
#[derive(Debug, Clone, PartialEq)]
pub struct CompareReport {
    /// Overall verdict.
    pub verdict: Verdict,
    /// Per-profile comparisons in ordinal profile-id order.
    pub profiles: Vec<ProfileComparison>,
    /// Non-fatal comparability notes.
    pub notes: Vec<String>,
    /// Dataset identity equal in every pair.
    pub dataset_equal: bool,
    /// Diagnostic metrics of the first repeat pair.
    pub diagnostics: Vec<DiagnosticDelta>,
}

impl CompareReport {
    /// Harness exit code: 0 no p95 Regression, 1 p95 Regression, 3 not comparable.
    pub fn exit_code(&self) -> i32 {
        match self.verdict {
            Verdict::Regression => 1,
            Verdict::Incomparable => 3,
            _ => 0,
        }
    }
}

/// Invalid summary shape or an unavailable comparison operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompareError {
    /// Explanation suitable for a command-line error.
    pub message: String,
}

impl CompareError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CompareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CompareError {}

/// Compare one pair of already parsed summaries. Valid only when `minimum_repeats == 1`; otherwise use
/// [`compare_repeats`].
pub fn compare(csharp: &Summary, rust: &Summary, thresholds: &Thresholds) -> Result<CompareReport, CompareError> {
    if thresholds.minimum_repeats != 1 {
        return Err(CompareError::new(
            "single-pair compare requires minimum_repeats == 1; use compare_repeats",
        ));
    }
    compare_repeats(std::slice::from_ref(csharp), std::slice::from_ref(rust), thresholds)
}

/// Compare paired repeat summaries by index and use the median of each repeat-level statistic.
///
/// Errors: unequal slice lengths, fewer than `minimum_repeats` pairs, or an invalid threshold.
pub fn compare_repeats(
    csharp: &[Summary],
    rust: &[Summary],
    thresholds: &Thresholds,
) -> Result<CompareReport, CompareError> {
    let t = thresholds;
    if !t.ratio_tolerance.is_finite()
        || t.ratio_tolerance < 0.0
        || !t.absolute_floor_ms.is_finite()
        || t.absolute_floor_ms < 0.0
        || t.minimum_repeats == 0
    {
        return Err(CompareError::new("invalid thresholds"));
    }
    if csharp.len() != rust.len() {
        return Err(CompareError::new(format!(
            "repeat count mismatch: {} C# summaries, {} Rust summaries",
            csharp.len(),
            rust.len()
        )));
    }
    if csharp.len() < t.minimum_repeats {
        return Err(CompareError::new(format!(
            "too few repeats: {} paired, {} required",
            csharp.len(),
            t.minimum_repeats
        )));
    }

    let mut notes = Vec::new();
    let mut hard_incomparable = false;
    let dataset_equal = csharp.iter().zip(rust).all(|(c, r)| c.dataset == r.dataset);
    if !dataset_equal {
        notes.push("dataset manifest differs between C# and Rust".to_owned());
        hard_incomparable = true;
    }
    if csharp.iter().zip(rust).any(|(c, r)| ids(c) != ids(r)) {
        notes.push("profile id set or order differs between C# and Rust".to_owned());
        hard_incomparable = true;
    }

    let mut all_ids: Vec<&str> = csharp
        .iter()
        .chain(rust)
        .flat_map(|s| s.rows.iter().map(|r| r.profile_id.as_str()))
        .collect();
    all_ids.sort_unstable();
    all_ids.dedup();

    let mut profiles_out = Vec::new();
    let mut diagnostics = Vec::new();
    for id in all_ids {
        let c_rows: Vec<Option<&PhaseRow>> = csharp.iter().map(|s| find_row(s, id)).collect();
        let r_rows: Vec<Option<&PhaseRow>> = rust.iter().map(|s| find_row(s, id)).collect();
        let (cmp, diags) = compare_profile(id, &c_rows, &r_rows, t, &mut notes);
        profiles_out.push(cmp);
        diagnostics.extend(diags);
    }

    let verdict = if profiles_out.is_empty() {
        notes.push("no profile rows on either side: empty phases are not comparable".to_owned());
        Verdict::Incomparable
    } else if hard_incomparable || profiles_out.iter().any(|p| p.verdict == Verdict::Incomparable) {
        Verdict::Incomparable
    } else if profiles_out
        .iter()
        .any(|p| p.verdict == Verdict::Regression && !p.correctness_only)
    {
        Verdict::Regression
    } else if profiles_out.iter().any(|p| p.verdict == Verdict::WithinNoise) {
        Verdict::WithinNoise
    } else if profiles_out
        .iter()
        .filter(|p| p.verdict != Verdict::Skipped)
        .all(|p| p.verdict == Verdict::Improvement)
        && profiles_out.iter().any(|p| p.verdict == Verdict::Improvement)
    {
        Verdict::Improvement
    } else {
        Verdict::Pass
    };

    Ok(CompareReport {
        verdict,
        profiles: profiles_out,
        notes,
        dataset_equal,
        diagnostics,
    })
}

fn ids(s: &Summary) -> Vec<&str> {
    s.rows.iter().map(|r| r.profile_id.as_str()).collect()
}

fn find_row<'a>(s: &'a Summary, id: &str) -> Option<&'a PhaseRow> {
    s.rows.iter().find(|r| r.profile_id == id)
}

fn incomparable(id: &str, row: Option<&PhaseRow>, repeats: usize, reason: impl Into<String>) -> ProfileComparison {
    ProfileComparison {
        profile_id: id.to_owned(),
        workload: row.map_or(-1, |r| r.workload),
        correctness_only: row.is_some_and(|r| r.correctness_only),
        verdict: Verdict::Incomparable,
        reason: reason.into(),
        repeat_count: repeats,
        status: [-1, -1],
        csharp_ms: [f64::NAN; 3],
        rust_ms: [f64::NAN; 3],
        sample_count: row.map_or(0, |r| r.samples_ms.len()),
        sample_count_rust: 0,
        allowed_p95_delta_ms: None,
    }
}

fn compare_profile(
    id: &str,
    c_rows: &[Option<&PhaseRow>],
    r_rows: &[Option<&PhaseRow>],
    t: &Thresholds,
    notes: &mut Vec<String>,
) -> (ProfileComparison, Vec<DiagnosticDelta>) {
    let n = c_rows.len();
    let first = c_rows.iter().chain(r_rows).flatten().next().copied();
    let mut c = Vec::with_capacity(n);
    let mut r = Vec::with_capacity(n);
    for (cr, rr) in c_rows.iter().zip(r_rows) {
        match (cr, rr) {
            (Some(cr), Some(rr)) => {
                c.push(*cr);
                r.push(*rr);
            }
            _ => {
                return (
                    incomparable(id, first, n, "profile present on only one side in some repeat"),
                    Vec::new(),
                );
            }
        }
    }
    let (c0, r0) = (c[0], r[0]);

    // Not-implemented profiles: skipped only when both sides carry the identical outcome.
    let not_implemented = profiles::find(id).is_some_and(profiles::is_not_implemented);
    for (cr, rr) in c.iter().zip(&r) {
        if cr.workload != rr.workload {
            return (incomparable(id, Some(c0), n, "workload differs"), Vec::new());
        }
        if cr.status != rr.status {
            return (
                incomparable(
                    id,
                    Some(c0),
                    n,
                    format!("status mismatch: {} vs {}", cr.status, rr.status),
                ),
                Vec::new(),
            );
        }
        if cr.status == 2 || cr.status == 3 || !(0..=3).contains(&cr.status) {
            return (
                incomparable(id, Some(c0), n, format!("status {} is not a speed result", cr.status)),
                Vec::new(),
            );
        }
        if cr.correctness_only != rr.correctness_only {
            return (
                incomparable(id, Some(c0), n, "correctness-only flag differs"),
                Vec::new(),
            );
        }
        if cr.samples_ms.len() != rr.samples_ms.len() {
            let mut cmp = incomparable(
                id,
                Some(c0),
                n,
                format!(
                    "sample count mismatch: {} vs {}",
                    cr.samples_ms.len(),
                    rr.samples_ms.len()
                ),
            );
            cmp.sample_count_rust = rr.samples_ms.len();
            return (cmp, Vec::new());
        }
        let names = |row: &PhaseRow| row.metrics.iter().map(|m| m.name.clone()).collect::<Vec<_>>();
        if names(cr) != names(rr) {
            return (incomparable(id, Some(c0), n, "metrics key set differs"), Vec::new());
        }
    }

    let diags = diagnostics_of(id, c0, r0);
    let med = |rows: &[&PhaseRow], f: fn(&PhaseRow) -> f64| median(&rows.iter().map(|x| f(x)).collect::<Vec<_>>());
    let base = ProfileComparison {
        profile_id: id.to_owned(),
        workload: c0.workload,
        correctness_only: c0.correctness_only,
        verdict: Verdict::Pass,
        reason: String::new(),
        repeat_count: n,
        status: [c0.status, r0.status],
        csharp_ms: [med(&c, |x| x.p50_ms), med(&c, |x| x.p95_ms), med(&c, |x| x.max_ms)],
        rust_ms: [med(&r, |x| x.p50_ms), med(&r, |x| x.p95_ms), med(&r, |x| x.max_ms)],
        sample_count: c0.samples_ms.len(),
        sample_count_rust: r0.samples_ms.len(),
        allowed_p95_delta_ms: None,
    };

    if not_implemented {
        let mut cmp = base;
        cmp.verdict = Verdict::Skipped;
        cmp.reason = "profile is not implemented in the C# registry and both sides report it identically".to_owned();
        return (cmp, diags);
    }
    if c0.correctness_only {
        let mut cmp = base;
        cmp.verdict = Verdict::Pass;
        cmp.reason = "correctness-only: status, count and correctness fields equal; latency informational".to_owned();
        return (cmp, diags);
    }

    // Timing profile: nothing may be null, non-finite or empty (never coerced to zero).
    for (cr, rr) in c.iter().zip(&r) {
        let ok = |row: &PhaseRow| {
            !row.samples_ms.is_empty()
                && row.p50_ms.is_finite()
                && row.p95_ms.is_finite()
                && row.max_ms.is_finite()
                && row.samples_ms.iter().all(|s| s.is_finite())
        };
        if !ok(cr) || !ok(rr) {
            let mut cmp = incomparable(id, Some(c0), n, "null, non-finite or empty timing data");
            cmp.sample_count = c0.samples_ms.len();
            cmp.sample_count_rust = r0.samples_ms.len();
            return (cmp, diags);
        }
    }

    let stat = |f: fn(&PhaseRow) -> f64| {
        classify(
            &c.iter().map(|x| f(x)).collect::<Vec<_>>(),
            &r.iter().map(|x| f(x)).collect::<Vec<_>>(),
            t,
        )
    };
    let (p95, p95_unstable, allowed) = stat(|x| x.p95_ms);
    for (label, f) in [
        ("p50", (|x: &PhaseRow| x.p50_ms) as fn(&PhaseRow) -> f64),
        ("max", |x| x.max_ms),
    ] {
        let (v, unstable, _) = stat(f);
        if v == Verdict::Regression {
            notes.push(format!(
                "warning: {id} {label} Regression (does not change the exit code)"
            ));
        }
        if unstable {
            notes.push(format!(
                "{id} {label} unstable: median worse but not in a strict majority of pairs"
            ));
        }
    }
    if p95_unstable {
        notes.push(format!(
            "{id} p95 unstable: median worse but not in a strict majority of pairs"
        ));
    }
    let mut cmp = base;
    cmp.verdict = p95;
    cmp.allowed_p95_delta_ms = Some(allowed);
    cmp.reason = format!(
        "p95 median C# {:.3} ms, Rust {:.3} ms, allowed {:.3} ms{}",
        cmp.csharp_ms[1],
        cmp.rust_ms[1],
        allowed,
        if p95_unstable { " (unstable)" } else { "" }
    );
    (cmp, diags)
}

/// Verdict for one statistic: returns (verdict, unstable, allowed).
fn classify(b: &[f64], r: &[f64], t: &Thresholds) -> (Verdict, bool, f64) {
    let (bm, rm) = (median(b), median(r));
    let allowed = (t.ratio_tolerance * bm.abs()).max(t.absolute_floor_ms);
    let d = rm - bm;
    if d < -allowed {
        (Verdict::Improvement, false, allowed)
    } else if d <= 0.0 {
        (Verdict::Pass, false, allowed)
    } else if d <= allowed {
        (Verdict::WithinNoise, false, allowed)
    } else {
        let worse = b.iter().zip(r).filter(|(bi, ri)| **ri - **bi > allowed).count();
        if worse * 2 > b.len() {
            (Verdict::Regression, false, allowed)
        } else {
            (Verdict::WithinNoise, true, allowed)
        }
    }
}

fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    match v.len() {
        0 => f64::NAN,
        n if n % 2 == 1 => v[n / 2],
        n => (v[n / 2 - 1] + v[n / 2]) / 2.0,
    }
}

fn diagnostics_of(id: &str, c: &PhaseRow, r: &PhaseRow) -> Vec<DiagnosticDelta> {
    c.metrics
        .iter()
        .zip(&r.metrics)
        .map(|(cm, rm)| {
            let delta = match (cm.value.parse::<i64>(), rm.value.parse::<i64>()) {
                (Ok(a), Ok(b)) => b.checked_sub(a).map(|d| d.to_string()),
                _ => None,
            };
            DiagnosticDelta {
                profile_id: id.to_owned(),
                name: cm.name.clone(),
                csharp: Some(cm.value.clone()),
                rust: Some(rm.value.clone()),
                delta,
            }
        })
        .collect()
}

// ---- summary parsing (own tiny JSON reader; BOM and CRLF tolerant) -------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Obj(props) => props.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn compact(&self) -> String {
        match self {
            Self::Null => "null".to_owned(),
            Self::Bool(b) => b.to_string(),
            Self::Num(t) => t.clone(),
            Self::Str(s) => format!("\"{s}\""),
            Self::Arr(a) => format!("[{}]", a.iter().map(Self::compact).collect::<Vec<_>>().join(",")),
            Self::Obj(o) => format!(
                "{{{}}}",
                o.iter()
                    .map(|(k, v)| format!("\"{k}\":{}", v.compact()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn ws(&mut self) {
        while self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\r' | b'\n') {
            self.pos += 1;
        }
    }

    fn err<T>(&self, what: &str) -> Result<T, CompareError> {
        Err(CompareError::new(format!(
            "malformed summary JSON at byte {}: {what}",
            self.pos
        )))
    }

    fn value(&mut self, depth: usize) -> Result<Value, CompareError> {
        if depth > 64 {
            return self.err("nesting too deep");
        }
        self.ws();
        match self.bytes.get(self.pos) {
            Some(b'{') => {
                self.pos += 1;
                let mut props = Vec::new();
                loop {
                    self.ws();
                    if self.bytes.get(self.pos) == Some(&b'}') {
                        self.pos += 1;
                        return Ok(Value::Obj(props));
                    }
                    if !props.is_empty() {
                        if self.bytes.get(self.pos) != Some(&b',') {
                            return self.err("expected ','");
                        }
                        self.pos += 1;
                        self.ws();
                    }
                    let key = self.string()?;
                    self.ws();
                    if self.bytes.get(self.pos) != Some(&b':') {
                        return self.err("expected ':'");
                    }
                    self.pos += 1;
                    props.push((key, self.value(depth + 1)?));
                }
            }
            Some(b'[') => {
                self.pos += 1;
                let mut items = Vec::new();
                loop {
                    self.ws();
                    if self.bytes.get(self.pos) == Some(&b']') {
                        self.pos += 1;
                        return Ok(Value::Arr(items));
                    }
                    if !items.is_empty() {
                        if self.bytes.get(self.pos) != Some(&b',') {
                            return self.err("expected ','");
                        }
                        self.pos += 1;
                    }
                    items.push(self.value(depth + 1)?);
                }
            }
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') if self.bytes[self.pos..].starts_with(b"true") => {
                self.pos += 4;
                Ok(Value::Bool(true))
            }
            Some(b'f') if self.bytes[self.pos..].starts_with(b"false") => {
                self.pos += 5;
                Ok(Value::Bool(false))
            }
            Some(b'n') if self.bytes[self.pos..].starts_with(b"null") => {
                self.pos += 4;
                Ok(Value::Null)
            }
            Some(b'-' | b'0'..=b'9') => {
                let start = self.pos;
                while self.pos < self.bytes.len()
                    && matches!(self.bytes[self.pos], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    self.pos += 1;
                }
                let token = String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned();
                if token.parse::<f64>().is_err() {
                    return self.err("bad number");
                }
                Ok(Value::Num(token))
            }
            _ => self.err("unexpected token"),
        }
    }

    fn string(&mut self) -> Result<String, CompareError> {
        if self.bytes.get(self.pos) != Some(&b'"') {
            return self.err("expected string");
        }
        self.pos += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(&b) = self.bytes.get(self.pos) else {
                return self.err("unterminated string");
            };
            self.pos += 1;
            match b {
                b'"' => return Ok(String::from_utf8_lossy(&out).into_owned()),
                b'\\' => {
                    let Some(&e) = self.bytes.get(self.pos) else {
                        return self.err("bad escape");
                    };
                    self.pos += 1;
                    match e {
                        b'"' | b'\\' | b'/' => out.push(e),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let hex = self
                                .bytes
                                .get(self.pos..self.pos + 4)
                                .and_then(|h| std::str::from_utf8(h).ok());
                            let Some(code) = hex.and_then(|h| u32::from_str_radix(h, 16).ok()) else {
                                return self.err("bad \\u escape");
                            };
                            self.pos += 4;
                            let ch = char::from_u32(code).unwrap_or('\u{FFFD}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return self.err("bad escape"),
                    }
                }
                _ => out.push(b),
            }
        }
    }
}

fn num_f64(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Num(t)) => t.parse::<f64>().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

fn num_i64(v: Option<&Value>, what: &str) -> Result<i64, CompareError> {
    match v {
        Some(Value::Num(t)) => t
            .parse::<i64>()
            .map_err(|_| CompareError::new(format!("{what} is not an integer"))),
        _ => Err(CompareError::new(format!("{what} is missing"))),
    }
}

/// Parse a `BenchmarkBatchSummary` JSON text (UTF-8 BOM and CRLF tolerated) into a [`Summary`].
///
/// `correctness_only` comes from the profile registry; `metrics` come from `Reports[i].Metrics` when `Reports` and
/// `ProfileOutcomes` have the same length. Null or absent percentiles become NaN (the comparison treats them as
/// incomparable, never as zero).
pub fn parse_summary(text: &str) -> Result<Summary, CompareError> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut reader = Reader {
        bytes: text.as_bytes(),
        pos: 0,
    };
    let root = reader.value(0)?;
    reader.ws();
    if reader.pos != text.len() {
        return reader.err("trailing data");
    }
    let dataset = root
        .get("Dataset")
        .ok_or_else(|| CompareError::new("summary has no Dataset"))?;
    let strings = |v: Option<&Value>| -> Vec<String> {
        match v {
            Some(Value::Arr(a)) => a
                .iter()
                .filter_map(|x| if let Value::Str(s) = x { Some(s.clone()) } else { None })
                .collect(),
            _ => Vec::new(),
        }
    };
    let mut unsupported = Vec::new();
    if let Some(Value::Obj(props)) = dataset.get("UnsupportedExtensions") {
        for (k, v) in props {
            unsupported.push((k.clone(), num_i64(Some(v), "UnsupportedExtensions count")?));
        }
    }
    let identity = DatasetIdentity {
        folder: match dataset.get("Folder") {
            Some(Value::Str(s)) => s.clone(),
            _ => String::new(),
        },
        files: strings(dataset.get("Files")),
        unsupported_extensions: unsupported,
        file_limit: num_i64(dataset.get("FileLimit"), "Dataset.FileLimit")?,
    };

    let Some(Value::Arr(outcomes)) = root.get("ProfileOutcomes") else {
        return Err(CompareError::new("summary has no ProfileOutcomes array"));
    };
    let reports: &[Value] = match root.get("Reports") {
        Some(Value::Arr(a)) if a.len() == outcomes.len() => a,
        _ => &[],
    };
    let mut rows = Vec::with_capacity(outcomes.len());
    for (i, o) in outcomes.iter().enumerate() {
        let profile_id = match o.get("ProfileId") {
            Some(Value::Str(s)) => s.clone(),
            _ => return Err(CompareError::new("ProfileOutcomes row has no ProfileId")),
        };
        let samples_ms = match o.get("Samples") {
            Some(Value::Arr(a)) => a.iter().map(|s| num_f64(Some(s))).collect(),
            _ => Vec::new(),
        };
        let metrics = match reports.get(i).and_then(|r| r.get("Metrics")) {
            Some(Value::Obj(props)) => props
                .iter()
                .map(|(k, v)| DiagnosticMetric {
                    name: k.clone(),
                    value: v.compact(),
                })
                .collect(),
            _ => Vec::new(),
        };
        rows.push(PhaseRow {
            correctness_only: profiles::find(&profile_id).is_some_and(|p| p.correctness_only),
            profile_id,
            workload: num_i64(o.get("Workload"), "Workload")?,
            status: num_i64(o.get("Status"), "Status")?,
            samples_ms,
            p50_ms: num_f64(o.get("P50")),
            p95_ms: num_f64(o.get("P95")),
            max_ms: num_f64(o.get("Max")),
            metrics,
        });
    }
    Ok(Summary {
        rows,
        dataset: identity,
    })
}
