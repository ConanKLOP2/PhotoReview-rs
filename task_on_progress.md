# Task on progress

**Updated:** 2026-10-14

## State
- Phase 0 / Wave 0 in progress. Repo: https://github.com/ConanKLOP2/PhotoReview-rs (public), local `D:\MyProject\PhotoReview-rs`.
- Merged on master: skeleton (#1 era), toolchain pin Rust 1.99.0 + `Cargo.lock` + `--locked` CI (PR #1).
- PR #2 (docs/MULTI-AGENT-PLAN.md) open when written; auto-merge could not be enabled while the repo has no required
  checks (GitHub only arms auto-merge when a required check is pending).
- Branch `wave0/foundations` (this PR): ADR R0003; `pr_core::{abstractions, time, text, sync, catalog::explorer_types}`;
  crate `pr-test-support`; `tools/oracle-cs` + `tools/oracle.ps1` (golden from the C# build at the baseline, via
  `git archive`, C# repo untouched); goldens `datetime-json`, `ordinal-ignore-case`, `natural-cmp` (for W1-06) and the
  generated `text/ordinal_casing_table.rs`; `tools/wave.ps1`. Local: fmt OK, clippy 0 warnings, all tests pass.
  Mutation checks: naive `to_uppercase`, untrimmed fraction, lowercase `t`, offset limit +15 all fail a golden test.
- Findings from the oracle (now encoded in goldens): .NET OrdinalIgnoreCase does not map `ı`→`I`, `ſ`→`S`, `ß`→`SS`
  and orders by code point; System.Text.Json trims fraction zeros, accepts `+hh` and `.Z`, rejects `+hhmm`, lowercase
  `t`/`z`, offsets beyond ±14:00.

## Next
1. Merge PR #2 and the Wave 0 PR. For auto-merge to work, the user plans to add required checks (`build-test`,
   `repo-checks`) on master.
2. W0-06: Wave 1 contracts (`contracts/W1-*.md`) + stubs + module declarations (one contract PR).
3. Wave 1 porting by Haiku per docs/MULTI-AGENT-PLAN.md (user asked for the plan; launching a Workflow needs the user's
   explicit "chạy workflow").
4. Still open in Phase 0: UI spikes (R0001), COM Explorer spike.
