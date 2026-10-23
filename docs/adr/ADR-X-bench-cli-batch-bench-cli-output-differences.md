# ADR-X-bench-cli-batch: pr-bench CLI: invariant numbers, one-line failures, UTF-8 output, per-profile setup failures

- **Status:** Proposed (draft written by DOC-ADR-DRAFTS, machine C, 2026-10-23; not accepted by the lead)
- **PARITY row:** `X-bench-cli-batch` in `docs/PARITY.md`, listed in `docs/PARITY-CLOSEOUT.md` ("Different")
- **Decision owner:** project lead

## Context
The Rust `pr-bench` CLI ports `PhotoReview.Benchmark.Cli`. The PARITY row lists four intentional differences: the DONE line uses invariant `F1` (C# uses the current culture, so a vi-VN machine prints `12,3ms`); an unhandled failure prints one line and returns `0xE0434352` without a stack trace; console output is UTF-8 (C# uses the console/OEM code page when redirected); profile setup failures are per-profile failures (C# lets them escape the profile loop `try`).

## Proposed decision
Accept the four differences as in W7-03d open questions 2, 7, 8 and 11: (2) invariant dot, because the output is scanned by scripts and AGENTS.md makes machine-read text invariant; (7) one line and the same exit code, a backend panic is exit 101; (8) UTF-8; (11) the constructors that fail outside the `try` in C# do not fail in practice, so Rust reports a per-profile failure and continues.

## Consequences
Rust and C# DONE lines differ on non-dot cultures, which matters for any text diff in the Phase 7 comparison (W10-B1 compares JSON summaries, not console text). The `cli-process` golden was never generated (D70).

## Alternatives not chosen
(2b) read the Windows user locale decimal separator and print what C# prints (platform helper plus golden cases per culture); (7) reproduce the stack trace (not testable).

## Sources
`docs/PARITY.md` row `X-bench-cli-batch`; `contracts/W7-03d.md` open questions 2, 7, 8, 11; `contracts/README.md` D70.

## Not verified
Which of the open questions the lead resolved: the row only says DIFFERENT. Question 4 (progress ordering) is not part of the row.
