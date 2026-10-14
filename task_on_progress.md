# Task on progress

**Updated:** 2026-10-14

## State
- Wave 0 done (two branches, neither merged when written). Repo: https://github.com/ConanKLOP2/PhotoReview-rs
  (public), local `D:\MyProject\PhotoReview-rs`.
- Merged on master: skeleton, toolchain pin Rust 1.99.0 + `Cargo.lock` + `--locked` CI (PR #1).
- PR #2 (docs/MULTI-AGENT-PLAN.md) and PR #3 (`wave0/foundations`) open, CI green. Auto-merge cannot be armed until
  master has required checks (`build-test`, `repo-checks`); the user said they would add them.
- PR #3 `wave0/foundations`: ADR R0003; `pr_core::{abstractions, time, text, sync, catalog::explorer_types}`;
  `pr-test-support`; oracle (`tools/oracle-cs`, `tools/oracle.ps1`); `tools/wave.ps1`.
- Branch `wave1/contracts` (built on `wave0/foundations`; PR opened only after #3 merges, no stacked PRs): W0-06.
  22 Wave 1 contracts in `contracts/W1-*.md` + `contracts/README.md` (roster, porter model per unit, lead decisions
  D1-D11); all stubs in place with `todo!()` bodies, module wiring, deps (serde, serde_json raw_value, indexmap);
  oracle now 31 commands (`tools/oracle-cs/Commands/*.cs`, PerfAnalysis.dll referenced), 26 new goldens (~2.3 MB),
  `tests/fixtures/diag/` synced. Gate (`tools/wave.ps1 -Wave 1 -Gate`) passed. Contracts were written by Sonnet agents
  that also checked their goldens against throw-away prototypes; the oracle outputs match their reference outputs.
- Oracle findings now in goldens: .NET OrdinalIgnoreCase / System.Text.Json DateTime quirks (Wave 0); `char.IsDigit`
  = BMP Nd only; 8 BMP letters .NET 10 does not lower; `Path.GetPathRoot` normalizes separators; `int.TryParse`
  accepts trailing NULs; LINQ sorts put NaN lowest and `MaxBy` keeps the first max; .NET `double.ToString` switches to
  E notation at exponent >= 17 or <= -5.

## Next
1. User: add required checks on master; merge #2 and #3. Then rebase `wave1/contracts` onto master, open its PR.
2. Wave 1 porting (docs/MULTI-AGENT-PLAN.md §5, roster in contracts/README.md): needs the user's explicit go
   ("chạy workflow wave 1") before launching many agents.
3. Still open in Phase 0: UI spikes (R0001), COM Explorer spike.
