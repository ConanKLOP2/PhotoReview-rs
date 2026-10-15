# Proposed decisions (not yet confirmed by the user)

The user's standing instruction (2026-10-15): run the whole project end to end without stopping; record every
proposal that has not been confirmed and proceed on it. Each row is acted on as written. The user may reverse any
row; reversing means a follow-up PR that changes the code and flips the status here. Confirmed outcomes move to an
ADR in `docs/adr/`.

| # | Date | Proposal (acted on) | Why / alternative | Status |
|---|---|---|---|---|
| P-01 | 2026-10-15 | W3-07: add a seam so `RecoveryFileCheck` can tell "stat unreadable/denied" from "missing" (option A: a `FileSystem` stat method returning a typed error, existing `get_file_stat` stays) | Without it the Recovery window says Lost where C# says Unknown. B/C changed the trait more | PROPOSED, being implemented |
| P-02 | 2026-10-15 | W3-08: `remove_if_partial` is called only on the single Copy catch path under the proof guard; group rollback keeps `is_same_file`; panicking `FileSystem` is not caught | Matches C#; a panicking file system is a bug, not a recoverable state | PROPOSED |
| P-03 | 2026-10-15 | W2-14: the 4 surviving mutants (MAX_FOLLOWED_LINKS cap, file_exists_full probe) are accepted as equivalent; add an `#[ignore = "native: needs symlink privilege"]` test that CI may run later | Unreachable without symlink privilege | PROPOSED |
| P-04 | 2026-10-15 | W2-04: invalid UTF-8 in a language file is a "cannot read" warning (C# substitutes U+FFFD); recorded as `Different` in PARITY | Matches D20 | PROPOSED |
| P-05 | 2026-10-15 | W2-16: local time comes from an injected `LocalTimeSource`; `pr-core` has no `windows-sys` | D16 | PROPOSED |
| P-06 | 2026-10-15 | Phase 0 UI (R0001): Win32 window + Direct2D/DirectWrite through `windows-rs` in `pr-app`/`pr-platform-win`, mirroring the C# move to Win32 + Direct2D (C# ADR 0010). No XAML/WinUI/egui | Same rendering path as the oracle; least behavioural drift, review speed first | PROPOSED, spike to validate |
| P-07 | 2026-10-15 | Phase 0 COM Explorer spike: `windows-rs` COM interfaces behind a `pr-platform-win` trait, fakes in tests | Keeps unsafe in one crate | PROPOSED |
| P-08 | 2026-10-15 | Waves 4 -> 5 and 6 in parallel -> 7 -> 8, one PR per wave, auto-fix + auto-merge (squash) on every PR | docs/MULTI-AGENT-PLAN.md section 8 | PROPOSED |
| P-09 | 2026-10-15 | Rate limit: when agents fail with a usage limit, stop retrying until the stated reset time, then resume from the workflow journal | Avoids burning tokens | PROPOSED |
| P-10 | 2026-10-15 | Equivalent or privilege-bound mutants are listed in the PR and in PARITY `Partial` notes; after 3 repair rounds the lead adds the small tests itself (MULTI-AGENT-PLAN section 10) | Prevents endless mutation loops | PROPOSED |
| P-11 | 2026-10-15 | D26(a) PowerLossSafe flush seam: option B, trait `AppendStream` with `as_file_mut()`; `FileSystem::open_append` returns `Box<dyn AppendStream>` (touches W2-14 `io::physical` and `pr-test-support`) | Only way to keep the durable-flush tests and F-13 `Done`; option A makes F-13 `Different` | PROPOSED |
| P-12 | 2026-10-15 | D26(b) stat seam: add `FileSystem::try_get_file_stat` (typed error: denied/unreadable vs missing); `get_file_stat` stays as is. Used by W3-01 `verify_destination`, W3-04, W3-06, W3-07 | One place for every stat; fixes the Unknown-vs-Lost verdict (P-01) | PROPOSED |
| P-13 | 2026-10-15 | D26(c) `RecycleBin::try_restore` returns `io::Result<bool>` (W3-05 option A) | A restore failure must be distinguishable from "item not found" (INV-5) | PROPOSED |
| P-14 | 2026-10-15 | D26(d) port `FaultInjectionFileSystem` and the hook fakes into `pr-test-support` before the red-test pass of W3-01/02 | Compaction fault-at-every-step tests need it; until then they are `#[ignore]` | PROPOSED |
| P-15 | 2026-10-15 | D26(e) sync `operations-legacy.jsonl` and `journal-older-build/*.jsonl` via `tools/sync-from-csharp.ps1` in the first Wave 3 port PR (never hand-edit) | AGENTS.md rule | PROPOSED |
| P-16 | 2026-10-15 | D26(f) add W3-09: integration unit for the fault/crash matrices (action -> journal -> undo/recovery), Opus, after W3-01..W3-08 merge | The only place INV-3..INV-6 are exercised together | PROPOSED |
| P-18 | 2026-10-15 | Granularity: split every work unit that has more than one independent concern (separate file, separate C# class, pure vs I/O half) into sub-units that can run in parallel; one agent per sub-unit, and small chores (docs, ignore files, branch/worktree cleanup, PR bookkeeping) go to Haiku sub-agents in the background. Contracts list sub-units as `W<wave>-<n><letter>` (e.g. W4-03a pure maths, W4-03b I/O). Parallelism is capped by dependencies and by CPU: at most 8 concurrent cargo builds, each in its own target dir | User: "tach nho hon nua duoc cu tach" (2026-10-15); machine slowness comes from too few parallel units AND too many cargo builds, so cap the latter | PROPOSED, applied from Wave 4 contracts |
