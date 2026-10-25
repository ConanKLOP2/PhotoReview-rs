# Documentation Index

| File | Purpose | Read |
|---|---|---|
| [`../AGENTS.md`](../AGENTS.md) | Rules, priorities, coexistence, tests, git | Every session |
| [`../task_on_progress.md`](../task_on_progress.md) | Current state and next steps | Every session |
| [`archive/`](archive/) | Older `task_on_progress.md` entries moved verbatim (Wave 5, 6, 7, Wave 8 B1/B2) | History only |
| [`PLAN.md`](PLAN.md) | Phases, gates, stack (Vietnamese) | Planning / picking next work |
| [`MULTI-MACHINE.md`](MULTI-MACHINE.md) | Two or more machines porting in parallel: ownership table, claims (`tools/autopilot/claim.sh`), branches, shared files, messages | Before starting work on a second machine |
| [`MULTI-AGENT-PLAN.md`](MULTI-AGENT-PLAN.md) | Waves, work units, model per unit (Haiku/Sonnet/Opus), prompts, gates | Before launching agents |
| [`PARITY.md`](PARITY.md) | Feature/INV checklist C# → Rust | Before porting anything |
| [`../contracts/README.md`](../contracts/README.md) | Wave rosters, lead decisions, one contract per work unit | Before porting a unit |
| [`SYNC-LEDGER.md`](SYNC-LEDGER.md) | C# baseline and C# PRs to port | When the C# repo moved |
| [`adr/`](adr/) | R0001 UI framework (proposed; spike done, Win32 + Direct2D) · R0002 coexistence · R0003 concurrency + C#→Rust mapping (accepted) | Relevant area; R0003 before any port |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | Crates, dependency direction, how the pieces fit | Before touching crate boundaries |
| [`DEVELOPING.md`](DEVELOPING.md) | Prerequisites, build, test and gate commands | Setting up a machine |
| [`LESSONS.md`](LESSONS.md) | Rules learned from the parallel Codex port | Before launching Codex lanes or cleanup |
| [`RUNBOOK-AUTOPILOT.md`](RUNBOOK-AUTOPILOT.md) | Operator runbook for the Codex pipeline (`tools/autopilot/`) | Before running or debugging the pipeline |
| [`WAVE-8-REPORT.md`](WAVE-8-REPORT.md) | Wave 8 closing report (status at 2026-10-22) | Picking up after Wave 8 |
| [`TRIAL-PLAN.md`](TRIAL-PLAN.md) | Two-week real-user trial: purpose and entry gate | Before the trial |
| [`PARITY-CLOSEOUT.md`](PARITY-CLOSEOUT.md) | Parity closeout: baseline and status inventory, `Different` rows | Before declaring parity |
| [`WAVE-9-PLAN.md`](WAVE-9-PLAN.md), [`WAVE-10-PLAN.md`](WAVE-10-PLAN.md) | Combined Wave 9 and Wave 10 views: dependency graphs, suggested order, pipeline tags, machine split (the lane plans stay the unit tables) | Before picking Wave 9/10 work |
| [`WAVE-9-PLAN-edt.md`](WAVE-9-PLAN-edt.md), [`WAVE-9-PLAN-ren.md`](WAVE-9-PLAN-ren.md) | Wave 9 plans: external editors UI (lane E), FEAT-RENAME (lane R) | Before Wave 9 units |
| [`WAVE-10-PLAN-http.md`](WAVE-10-PLAN-http.md) | Wave 10 manual update HTTP lane (contracts only) | Before W10-H units |
| [`drafts/`](drafts/) | Proposed CI/release files not yet in `.github/workflows` (`release.yml`, `ci-deny-job.yml`, `deny.toml`, `raw-corpus-workflow.yml.md`) | Release / CI work |
| [`../tests/golden-inputs/README.md`](../tests/golden-inputs/README.md) | Oracle/golden workflow (`tools/oracle.ps1`) | Writing tests against C# behaviour |

C# spec docs (read from the C# repo, not copied): `docs/architecture.md`, `docs/APP-MECHANISMS-VI.md`, `docs/adr/0001-0009`.
