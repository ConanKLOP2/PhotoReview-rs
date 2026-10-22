# Documentation Index

| File | Purpose | Read |
|---|---|---|
| [`../AGENTS.md`](../AGENTS.md) | Rules, priorities, coexistence, tests, git | Every session |
| [`../task_on_progress.md`](../task_on_progress.md) | Current state and next steps | Every session |
| [`PLAN.md`](PLAN.md) | Phases, gates, stack (Vietnamese) | Planning / picking next work |
| [`MULTI-MACHINE.md`](MULTI-MACHINE.md) | Two or more machines porting in parallel: ownership table, claims (`tools/autopilot/claim.sh`), branches, shared files, messages | Before starting work on a second machine |
| [`MULTI-AGENT-PLAN.md`](MULTI-AGENT-PLAN.md) | Waves, work units, model per unit (Haiku/Sonnet/Opus), prompts, gates | Before launching agents |
| [`PARITY.md`](PARITY.md) | Feature/INV checklist C# → Rust | Before porting anything |
| [`../contracts/README.md`](../contracts/README.md) | Wave rosters, lead decisions, one contract per work unit | Before porting a unit |
| [`SYNC-LEDGER.md`](SYNC-LEDGER.md) | C# baseline and C# PRs to port | When the C# repo moved |
| [`adr/`](adr/) | R0001 UI framework (proposed; spike done, Win32 + Direct2D) · R0002 coexistence · R0003 concurrency + C#→Rust mapping (accepted) | Relevant area; R0003 before any port |
| [`../tests/golden-inputs/README.md`](../tests/golden-inputs/README.md) | Oracle/golden workflow (`tools/oracle.ps1`) | Writing tests against C# behaviour |

C# spec docs (read from the C# repo, not copied): `docs/architecture.md`, `docs/APP-MECHANISMS-VI.md`, `docs/adr/0001-0009`.
