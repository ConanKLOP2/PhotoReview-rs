# Two-week real-user trial

## Purpose and entry gate

Phase 7 requires parity at 100% or an ADR for each intentional difference, a benchmark equal to or better than C#, a two-week real-user trial, and a separate release. The user decides whether to stop the C# app. This plan defines the trial after those gates, without claiming that they have passed. See `docs/PLAN.md` (Phase 7), `docs/PARITY.md` and `docs/PARITY-CLOSEOUT.md`.

| Before day 1 | Evidence required |
|---|---|
| Parity closed | Apply `docs/PARITY-CLOSEOUT.md`: every active `docs/PARITY.md` row has a proving test or an ADR-backed `Different` disposition; historical `Superseded` rows are reconciled. Include native, RAW corpus, and UI evidence where the row requires it. |
| Current C# specification | Reconcile behavior-changing changes against the baseline in `docs/SYNC-LEDGER.md`; sync language assets and fixtures through `tools/sync-from-csharp.ps1` when needed (`AGENTS.md`, `docs/SYNC-LEDGER.md`). |
| Performance result | Run the comparable `pr_bench`/C# Benchmark.Cli workloads and record the results and environment. Phase 2 requires `pr-bench` to emit the same format as Benchmark.Cli; Phase 7 requires the benchmark to be **equal to or better than C#** (`docs/PLAN.md`, Phases 2 and 7). `docs/PARITY.md` `X-benchmark-*` and `X-bench-*` rows track the implementation; they are not themselves a comparative result. |
| Release candidate | Record the exact Rust build and C# baseline used for the trial. Phase 7 calls for a separate release (`docs/PLAN.md`); the release process/version is an open item below. |

The trial covers real startup, folder review, file actions, recovery, settings, shutdown, and observed review speed. The C# app remains available for daily use (`docs/adr/R0002-coexistence-with-csharp-app.md`).

## People and side-by-side operation

| Role | Trial responsibility |
|---|---|
| User, trial driver | Use the Rust app on real review tasks, record outcomes, decide whether to stop using the C# app after the separate release (`docs/PLAN.md`, Phase 7). |
| Lead | Check entry and exit evidence, triage reports, own the parity and benchmark verdict, and decide whether the trial clock is valid. This responsibility is proposed for the trial; `docs/MULTI-MACHINE.md` already assigns the lead unit ownership and integration coordination. |
| Agents | Reproduce and fix assigned defects, update the relevant `docs/PARITY.md` row and tests in the same PR, and report evidence limits (`AGENTS.md`, `docs/MULTI-MACHINE.md`). |

R0002 separates Rust config, journal, sessions, logs, caches, and window placement under `%LOCALAPPDATA%\PhotoReviewRs`. Rust uses the `PhotoReviewRs` mutex/pipe prefix, `PhotoReviewRs.exe`, and `PHOTOREVIEW_RS_*` environment variables. The only C# app file Rust may read is `config.json`, once and read-only for first-run import. Journal, session, and config formats remain compatible, but the roots remain separate (`AGENTS.md`, `docs/adr/R0002-coexistence-with-csharp-app.md`).

Do not point either app at the other's data root or exchange their journal, session, cache, or log files. Finish pending Undo/Recovery in the app that began the action: each app sees only its own journal. Both apps can act on real photos, so record which app performed each action and avoid concurrent actions on the same photo as a proposed trial precaution. R0002 says the C# data root is untouched by Rust; it does not promise that actions on shared photos are isolated.

## Daily and weekly cadence

Proposed checklist: record an observation for each row every trial day; mark unused actions `not exercised` rather than inventing a pass. Use a test-owned folder for deliberate delete/recovery drills; `AGENTS.md` forbids sweeping the user's real Recycle Bin.

| Each day | Observation to record | PARITY reference |
|---|---|---|
| Startup | Start and reopen; note single-instance handoff and any repair/recovery prompt. | `C-18`, `C-19`, `C-27`, `C-28`, `W-01` |
| Open folder | Open a normal folder, navigate, switch image, and note ordering and first display. | `K-01`, `K-02`, `K-25`, `INV-7`, `INV-9` |
| Review and file actions | Perform only needed move/copy/recycle actions; record destination, confirmation, visible result, and journal outcome. | `K-08`, `K-09`, `K-23`, `K-24`, `INV-3`, `INV-4`, `INV-6` |
| Undo | Undo an action from this app; check photo, destination, and recovery state. | `K-07`, `C-14`, `INV-4`, `INV-5` |
| Recovery | Inspect pending/failed items in the same app; retry or dismiss only after recording evidence. | `C-27`, `W-03`, `INV-5`, `INV-6` |
| Settings | Open, change one useful setting, save, reopen, and check the value and repair message. | `W-02`, `S-64`, `S-65`, `INV-11` |
| Shutdown | Close and restart; check session resumption and that the other app still starts. | `W-01`, `F-22`, `F-23`, `INV-11` |
| Performance spot check | Note folder size, startup/first display and image-switch responsiveness, and workload context; attach benchmark report when run. | `P-08`, `X-benchmark-profiles`, `X-bench-engine` |

| Period | Proposed emphasis |
|---|---|
| Week 1 | Establish normal startup, navigation, review, settings, shutdown, and baseline observations; exercise file actions and Undo on a test-owned set. |
| Week 2 | Repeat routine use after fixes, exercise interrupted/restart recovery and representative RAW/large-folder paths, and compare the recorded benchmark with C# (`docs/PLAN.md`, Phases 3, 5 and 7). |

## Scenario map

These IDs are copied from the inventory and status rows in `docs/PARITY.md`; a range is preserved as written. A mapped scenario is a trial prompt, not proof that its row is Done.

| Scenario | PARITY IDs |
|---|---|
| Main-window open, tools, cache and duplicate commands | `C-04`, `C-05`, `C-06`, `C-18`, `C-19`, `C-25`, `C-27`, `C-28` |
| Context menu, zoom and Undo | `C-07..C-09`, `C-11`, `C-13`, `C-14`, `C-15`, `C-16` |
| Keyboard review, file actions and zoom | `K-01`, `K-02`, `K-07`, `K-08`, `K-09`, `K-12`, `K-23`, `K-24`, `K-25`, `K-27` |
| Secondary windows, dialogs and recovery | `W-01`, `W-02`, `W-03`, `W-05`, `W-06` |
| Settings load/save and review controls | `S-09`, `S-20`, `S-30`, `S-64`, `S-65` |
| Data safety: cache, actions, Undo, recovery, order, reads, stale work, config | `INV-1`, `INV-2`, `INV-3`, `INV-4`, `INV-5`, `INV-6`, `INV-7`, `INV-8`, `INV-9`, `INV-11`, `INV-12` |

## Defect and incident record

Proposed record location: a dedicated trial issue label, with one issue per defect or `INV-*` incident. If issues are unavailable, use a future file under `docs/trial/` (proposed, does not exist yet); neither location is established by the cited repo files. Preserve the original observation, then append triage and retest results.

| Field | Record |
|---|---|
| Identity | Report reference, trial day, reporter, Rust build, C# baseline (`docs/SYNC-LEDGER.md`), machine/OS, scenario and `docs/PARITY.md` ID. |
| Reproduction | Exact action sequence, source/destination paths redacted if shared, expected C# behavior, observed Rust behavior, repeatability. |
| Impact | Affected photos and app, S0–S3 proposal below, whether further file actions were paused. |
| Evidence | Screenshots, timestamps, benchmark reports, relevant tests; copies of Rust journal/session/log from `%LOCALAPPDATA%\PhotoReviewRs` only, with personal paths redacted. Never attach or read the C# app's journal/session/cache/log for Rust diagnosis (`docs/adr/R0002-coexistence-with-csharp-app.md`). |
| Resolution | Owner, linked parity row/test/ADR, fix build, retest result, trial-clock decision. |

## Severity, triage and clock (proposal)

| Severity | Proposed rule | Proposed response |
|---|---|---|
| S0 | Data loss/corruption, an `INV-*` safety violation, or either app touching the other's private data. | Stop Rust file actions and the trial clock; preserve Rust evidence; lead investigates with the user before resuming. |
| S1 | Blocked core review, move/copy/delete, Undo/Recovery, or repeated crash without confirmed data loss. | Pause the affected scenario and trial clock; lead assigns a fix and reruns it before resumption. |
| S2 | A reproducible noncritical behavior or performance regression with a usable workaround. | Log, assign and retest within the trial; lead decides whether it blocks exit. |
| S3 | Cosmetic issue with no review or data impact. | Log for release triage. |

Proposed restart rule: after an S0/S1 fix, the lead and user agree on a new uninterrupted two-week observation window for the affected build. Keep earlier evidence, but do not count days on a build that still has the blocking defect. The repo does not define severity levels or trial-clock rules; confirm these before day 1.

## Exit check

| Check | Required evidence |
|---|---|
| [ ] Two weeks of real-user observations | Dated daily records and both weekly reviews (`docs/PLAN.md`, Phase 7). |
| [ ] Parity closure | `docs/PARITY-CLOSEOUT.md` exit check; active rows Done with proving tests or intentional differences with ADRs (`docs/PARITY.md`, `AGENTS.md`). |
| [ ] Comparative benchmark | Recorded, comparable `pr_bench`/C# result **equal to or better than C#**, including workload, environment, report and decision (`docs/PLAN.md`, Phases 2 and 7; `docs/PARITY.md` `X-bench-*`). |
| [ ] Data safety and defects | No open S0/S1; `INV-*` incidents resolved and retested under the proposed severity rules. |
| [ ] Separate release decision | Release artifact and notes address the two app roots and app-specific Undo/Recovery (`docs/PLAN.md`, Phase 7; R0002). The user decides whether to stop the C# app. |

## Rollback to the C# app

Stop Rust file actions, record its pending journal/recovery state, and close Rust. Resume C# in its own data root. R0002 keeps the C# config, journal, sessions, logs and caches untouched by Rust, so C# can continue using its own state. Photos moved/copied/deleted by Rust are real shared files; inspect their outcome and finish Rust pending recovery in Rust before performing conflicting C# actions on those photos. Do not copy one app's journal into the other (`docs/adr/R0002-coexistence-with-csharp-app.md`).

## Open items

| Item | State |
|---|---|
| Trial start date, named lead/agents, machine, Rust build and release version | TBD (not in the repo) |
| Record location/issue label and severity/clock proposal approval | TBD (not in the repo) |
| Comparable benchmark command, workload, C# result and Rust result | TBD (not in the repo) |
| Final native/UI/RAW corpus evidence and parity closure | TBD (not in the repo) |
| Release packaging and rollback instructions beyond R0002 | TBD (not in the repo) |
