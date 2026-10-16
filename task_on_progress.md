# Task on progress

**Updated:** 2026-10-16 (branch `integration/wave-4`, PR #20)

## How to resume (read this first)
1. Read `AGENTS.md`, this file, `docs/PROPOSED-DECISIONS.md` (P-01..P-28: unconfirmed proposals, all acted on),
   `docs/MULTI-AGENT-PLAN.md`, `docs/PARITY.md` ("Wave 4 final status"), `tools/autopilot/README.md`.
2. Standing user instruction: run the whole port end to end, auto-fix + auto-merge (squash) every PR, never ask;
   log open questions as a `P-nn` row and act on the recommended option. Replies in Vietnamese.
3. The 15-minute loop was cancelled; restart it with `/loop 15m` + `tools/autopilot/loop-prompt.md`.
4. Permission classifier blocks some `gh pr merge`, `git branch -D`, `git push --delete`, publish commands in
   workflow agents; the user must add the rules from `tools/autopilot/README.md` to `.claude/settings.local.json`
   (agents cannot grant themselves permissions). The main session pushes and opens PRs when a workflow agent refuses.

## State (verified)
- **master** (`985ba99`): Wave 0, 1, 2, 3 merged (PR #8, #17), Wave 4 contracts (#16), Wave 4 plan, docs (#9-#15, #18),
  autopilot kit (#19). C# baseline for all goldens: `d92d6aae`.
- **PR #20 (open, auto-merge on)**: Wave 4 port (`integration/wave-4`, 36 sub-units). Last gate run on the merged tree:
  `cargo fmt` clean, `cargo clippy --workspace --all-targets --locked -- -D warnings` clean,
  `cargo test --workspace --locked --no-fail-fast`: 3619 passed, 0 failed, 304 ignored. First CI run failed on 7
  `pr-imaging` tests (CI temp dir is an 8.3 short name `RUNNER~1`; .NET-style `GetFullPath` expands it); fixed in
  `d113721` (`TempRoot` uses the long `%TEMP%`); CI re-run pending when this was written.
- Wave 4 units Partial (PARITY lists the open items): W4-01c, 02b, 10b, 11c, 11e, 14b, 14c, 14d, 16a + 2 native-test gaps.
  Open: `cargo test -p pr-imaging -p pr-platform-win -- --include-ignored` has 32 failing native tests (30 are `todo!()`
  native seams: `pr-imaging` cannot dev-depend on `pr-platform-win`, a cycle; 1 needs the WIC JPEG codec; 1 blocked on
  W4-11c OQ1). cargo-mutants was only partly run (W4-16a 10/152, W4-01c ~35/43, W4-10b 40 manual mutations).
  Latent bug noted in W4-14a: `PreviewError::is_busy/is_canceled` do not unwrap `WithSourceBytes` (C# still retries).
- Wave 3 Partial: W3-01 (cargo-mutants not done, 228/231 untested), W3-05 (verifier re-run unfinished); ignored native
  journal tests (journal 19, compaction 15, reconcile 2, startup_recovery 6, INV-6 equivalence 5) not run.
- Wave 2 Partial: W2-14 (4 mutants need a privileged symlink), W2-17, W2-01, W2-03.
- Known flaky tests (timing, fail under load): `diagnostics_file_log::*` (W2-16); bounds were raised, more may appear.

## Not done
- **C# sync patch "external editors"** (P-17): C# `master` is past `d92d6aae` (`ExternalEditor.cs`, `AppSettings`,
  `SettingsNormalizer`, `SettingsStore`, `ShortcutMappings`, `IDialogService`, languages). Needs W1-04, W2-01/02/03,
  W2-20 re-ported and goldens regenerated at the new baseline (`docs/SYNC-LEDGER.md` row exists).
- **Phase 0**: UI spike R0001 (P-06: Win32 + Direct2D via `windows-rs`), COM Explorer spike (P-07).
- **Waves 5+6** (RAW/TurboJPEG/LibRaw; Platform Windows), **7** (benchmarking), **8** (App/UI). Schedule the port workflows
  by dependency graph (P-28), not by tiers. Wave 4 lost 11 units to "blocked by a stub of the same tier".
- Rough progress: Waves 0-3 done (59 units), Wave 4 ported and in PR; about 45% of units, about 35-40% by effort.

## Next steps
1. Let PR #20 reach green and merge (auto-fix handles CI events); then delete the Wave 4 worktrees/branches
   (`integ-w4`, `f-W4-*`, `r-W4-*`, `fin/*`, `rep/*`, `wu/W4-03b`, `W4-15a-verify`) and `target-wave-4`/`target-w4ci`.
2. The main checkout may still hold stray edits an agent left (`crates/pr-imaging/src/service/disk.rs`, `service/mod.rs`,
   `docs/PARITY.md`); compare with `integration/wave-4` / master before discarding.
3. P-17 sync patch, then Phase 0 spikes, then Waves 5+6 (contracts workflow, wire + PR, port workflow, PR), 7, 8.
4. Fix the stale last sentence of AGENTS.md "Disk hygiene" (it still mentions a per-loop-tick disk check; the loop no
   longer does that).
