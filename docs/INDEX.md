# Documentation Index

One row per document (or per folder where a folder is one document set). Check the links with
`python -I tools/dev/doc-links.py` (selftest: `bash tools/dev/test/doc-links-selftest.sh`).

## Rules, state and plans

| File | Purpose | Read |
|---|---|---|
| [`../AGENTS.md`](../AGENTS.md) | Rules, priorities, coexistence, tests, git | Every session |
| [`../CLAUDE.md`](../CLAUDE.md) | Claude Code entry point: points to `AGENTS.md`, `task_on_progress.md` and this index | Every session |
| [`../task_on_progress.md`](../task_on_progress.md) | Current state and next steps | Every session |
| [`../README.md`](../README.md) | Project summary, status and links to the plan | First visit to the repo |
| [`PLAN.md`](PLAN.md) | Phases, gates, stack (Vietnamese) | Planning / picking next work |
| [`MULTI-AGENT-PLAN.md`](MULTI-AGENT-PLAN.md) | Waves, work units, model per unit (Haiku/Sonnet/Opus), prompts, gates | Before launching agents |
| [`MULTI-MACHINE.md`](MULTI-MACHINE.md) | Two or more machines porting in parallel: ownership table, claims (`tools/autopilot/claim.sh`), branches, shared files, messages | Before starting work on a second machine |
| [`PROPOSED-DECISIONS.md`](PROPOSED-DECISIONS.md) | Unconfirmed proposals (P-nn) that were acted on; the user may reverse any row | Before changing a rule or repeating a decision |
| [`LESSONS.md`](LESSONS.md) | Rules learned from the parallel Codex port | Before launching Codex lanes or cleanup |

## Port status and parity

| File | Purpose | Read |
|---|---|---|
| [`PARITY.md`](PARITY.md) | Feature/INV checklist C# → Rust | Before porting anything |
| [`PARITY-CLOSEOUT.md`](PARITY-CLOSEOUT.md) | Parity closeout: baseline and status inventory, `Different` rows | Before declaring parity |
| [`SYNC-LEDGER.md`](SYNC-LEDGER.md) | C# baseline and C# PRs to port | When the C# repo moved |
| [`TRIAL-PLAN.md`](TRIAL-PLAN.md) | Two-week real-user trial: purpose and entry gate | Before the trial |
| [`WAVE-8-REPORT.md`](WAVE-8-REPORT.md) | Wave 8 closing report (status at 2026-10-22) | Picking up after Wave 8 |
| [`UNSAFE-REVIEW-W8-11.md`](UNSAFE-REVIEW-W8-11.md) | Row-by-row review of every `unsafe` operation of W8-11b / W8-11c (Win32 API and message-loop code) | Touching that `unsafe` code or auditing `unsafe` |

## Contracts and wave plans

| File | Purpose | Read |
|---|---|---|
| [`../contracts/README.md`](../contracts/README.md) | Wave rosters, lead decisions, one contract per work unit (`contracts/W<wave>-<n>.md`) | Before porting a unit |
| [`WAVE-4-PLAN.md`](WAVE-4-PLAN.md) | Wave 4 plan (imaging): scope, crates, unit table; supersedes the Wave 4 table of `MULTI-AGENT-PLAN.md` | Before Wave 4 units |
| [`WAVE-8-PLAN.md`](WAVE-8-PLAN.md) | Wave 8 plan (App/UI): unit table with dependency column, risks; supersedes the Wave 8 sketch of `MULTI-AGENT-PLAN.md` | Before Wave 8 units |
| [`WAVE-9-PLAN.md`](WAVE-9-PLAN.md), [`WAVE-10-PLAN.md`](WAVE-10-PLAN.md) | Combined Wave 9 and Wave 10 views: dependency graphs, suggested order, pipeline tags, machine split (the lane plans stay the unit tables) | Before picking Wave 9/10 work |
| [`WAVE-9-PLAN-edt.md`](WAVE-9-PLAN-edt.md), [`WAVE-9-PLAN-ren.md`](WAVE-9-PLAN-ren.md) | Wave 9 plans: external editors UI (lane E), FEAT-RENAME (lane R) | Before Wave 9 units |
| [`WAVE-10-PLAN-http.md`](WAVE-10-PLAN-http.md) | Wave 10 manual update HTTP lane (contracts only) | Before W10-H units |

## Architecture, build and operations

| File | Purpose | Read |
|---|---|---|
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | Crates, dependency direction, how the pieces fit | Before touching crate boundaries |
| [`DEVELOPING.md`](DEVELOPING.md) | Prerequisites, build, test and gate commands | Setting up a machine |
| [`RELEASING.md`](RELEASING.md) | W10 manual release gates, pinned native inputs, selftests and outstanding maintainer reviews | Before a release |
| [`RUNBOOK-AUTOPILOT.md`](RUNBOOK-AUTOPILOT.md) | Operator runbook for the Codex pipeline (`tools/autopilot/`) | Before running or debugging the pipeline |
| [`../tools/autopilot/README.md`](../tools/autopilot/README.md) | Autopilot kit: pipeline scripts, state files, how the port is run end to end | Before running the pipeline scripts |
| [`../tools/autopilot/CODEX-RULES.md`](../tools/autopilot/CODEX-RULES.md) | The rules every Codex call receives (owned files only, C# is the spec, no real Recycle Bin) | Before writing a Codex prompt or pack |
| [`../tools/autopilot/loop-prompt.md`](../tools/autopilot/loop-prompt.md) | Prompt of the autopilot loop (Codex-first routing, tick steps) | Before changing the loop |
| [`../tools/dev/README.md`](../tools/dev/README.md) | Developer tools table (`status.sh`, `janitor.sh`, `dashboard.sh`, `parity.sh`, ...) with docs in `tools/dev/docs/` and the matching skills; `tools/dev/doc-links.py` checks the links of every document | Before cleaning worktrees or using a dev tool |
| [`../tools/dev/SCHED.md`](../tools/dev/SCHED.md) | Codex scheduler `codex-sched.sh`: lanes, quota limit, environment | Before running several Codex calls |
| [`../tools/dev/hooks/README.md`](../tools/dev/hooks/README.md) | Claude Code hooks (`post-edit-rustfmt.sh`, `pre-bash-guard.sh`) and how to register them | Before enabling or changing hooks |
| [`../tests/golden-inputs/README.md`](../tests/golden-inputs/README.md) | Oracle/golden workflow (`tools/oracle.ps1`) | Writing tests against C# behaviour |
| [`../tests/fixtures/README.md`](../tests/fixtures/README.md) | Image fixtures and decode-quality metrics (Vietnamese, from the C# WC3 work) | Writing image or decoder tests |

## Decisions, reviews and history (folders)

| File | Purpose | Read |
|---|---|---|
| [`adr/`](adr/) | R0001 UI framework (proposed; spike done, Win32 + Direct2D) · R0002 coexistence · R0003 concurrency + C#→Rust mapping (accepted) · D59/D60 and the unit and `Different`-row ADRs; list in [`adr/README.md`](adr/README.md) | Relevant area; R0003 before any port |
| [`reviews/`](reviews/) | Read-only review reports per area (autopilot, dialogs, invariants, startup, tests, W9 contracts, PR reviews); index with counts and status in [`reviews/README.md`](reviews/README.md); [`reviews/BACKLOG.md`](reviews/BACKLOG.md) merges all findings into one backlog with Codex prompts | Before fixing a review finding or picking cleanup work |
| [`drafts/`](drafts/) | Proposed CI/release files not yet in `.github/workflows` (`release.yml`, `ci-deny-job.yml`, `deny.toml`, `raw-corpus-workflow.yml.md`), each with its purpose and the decision needed ([`drafts/README.md`](drafts/README.md)); old draft reports in [`archive/drafts/`](archive/drafts/) | Release / CI work |
| [`archive/`](archive/) | Older `task_on_progress.md` entries moved verbatim (Wave 5 to Wave 9; table in `../task_on_progress.md`), superseded consistency notes and old draft reports | History only |

C# spec docs (read from the C# repo, not copied): `docs/architecture.md`, `docs/APP-MECHANISMS-VI.md`, `docs/adr/0001-0009`.
