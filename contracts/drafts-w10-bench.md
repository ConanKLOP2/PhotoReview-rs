# W10 benchmark contract roster and decisions (draft)

| WU | Unit | Color | Porter | Depends |
|---|---|---|---|---|
| W10-B1 | Benchmark comparison harness | 🟡 | Sonnet 5.5 | W7-01a, W7-01b, W7-03a, W7-03c, W7-03d; engine-backed backend wiring |
| W10-B2 | Attended production re-measure protocol | 🟡 | Manual operator | W8-12d, W8-12e, W8-12h, W8-16a; attended session |
| W10-B3 | Phase 7 parity report format | 🟡 | Sonnet 5.5 | W10-B1, W10-B2 |

## Decisions D-W10BENCH

- **D-W10BENCH-1 — Proposed numeric parity rule.** Rule: compare paired repeats by index and use medians; `allowed=max(5%*|B|,1 ms)`, `d=R-B`; below `-allowed` Improvement, `[-allowed,0]` Pass, `(0,allowed]` WithinNoise, above allowed Regression only when a strict majority of paired differences exceed allowed, otherwise WithinNoise/unstable. Only p95 Regression fails exit code 1; p50/max Regression is a warning. Rationale: explicit Phase 7 gate using C# PerfComparer defaults. Who may overrule: project lead by recorded decision.
- **D-W10BENCH-2 — Sequential same-machine runs.** Rule: C# and Rust never run concurrently and use the same fixture, profile list, display/power conditions, and separate output folders. Rationale: avoid resource contention and input drift. Who may overrule: project lead with an ADR describing the controlled parallel setup.
- **D-W10BENCH-3 — Comparison preconditions.** Rule: no parity claim before Rust engine-backed backend wiring and schema parity. Rationale: `main.rs` currently selects `UnwiredBackend`, which cannot provide valid batch results. Who may overrule: none for a claim; implementation can change the backend but must meet the same evidence gate.
- **D-W10BENCH-4 — Conditional UI experiment gate.** Rule: W8-12j is started or canceled using W8-12j's attended keep-warm-on trigger and repeat evidence; B1 batch numbers do not replace it. Rationale: benchmark workload and production frame stalls are different measures. Who may overrule: project lead by ADR.
- **D-W10BENCH-5 — Metrics are diagnostic.** Rule: every `ReviewMetricsSnapshot` value is reported as C#, Rust, and delta and never fails parity; Metrics presence and key shape remain schema-exact. Rationale: timing and scheduling can legitimately change counters. Who may overrule: project lead by ADR.
- **D-W10BENCH-6 — P0 production measurement driver.** Rule: production p95 requires `PhotoReviewRs.exe` plus frame log and automated 250 ms Right-arrow driver, or an updated production-surface spike; old spike subset is indicative only. Rationale: old spike lacks keep-warm and is not production surface. Who may overrule: project lead only after equivalent production evidence is documented.

## Preconditions

- Engine-backed benchmark backend wired in `main.rs` (not yet done).
- W8-12d, W8-12e, and W8-12h merged before the attended production measurement.
- P0 measurement driver exists; no suitable input driver was found and it is to be added by a later unit.
- W8-16a frame logging available; `PHOTOREVIEW_RS_FRAME_LOG` accepts a file path.
- Attended session with AC power, display setup, IME checks, and fixture generation.

## Open questions

- Confirm the B1 JSON envelope against its public API when implementation begins.
