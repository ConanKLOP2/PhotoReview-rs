# Task on progress

**Updated:** 2026-10-14

## State
- Wave 0 and Wave 1 contracts merged on master (#1-#4). Wave 1 porting done on branch `integration/wave-1`
  (this PR): all 22 units ported, 21 merged and gated; W1-20 helper de-duplication lands last.
- Results: 22 units, 1 port round + 4 repair rounds (mutation + differential verification against real C# builds).
  Gate (`tools/wave.ps1 -Wave 1 -Gate`): fmt, clippy -D warnings, all tests, synced assets: passed on the merged tree.
- Accepted divergences (contracts/README.md D5 and per-unit `// DIVERGENCE:` comments): bare `-0` / `1e400` number
  tokens (serde_json), UTF-16 BOM side files (perf units), offset timestamps converted to UTC (W1-14), invalid UTF-8
  in a journal property name (W1-14), junction-cycle protection (W1-21; .NET never terminates).
- Corrections to contracts found in verification: W1-21 "C# does not follow reparse points" was wrong (.NET 10 follows
  junctions); contract text fixed.
- `tools/wave.ps1` now gives every worktree its own cargo target dir (shared dir caused false gate failures).

## Next
1. Merge the Wave 1 PR (auto-merge armed). Then delete `.claude/worktrees/W1-*`, `integ`, `wu/*` branches
   (content is squashed into master; check by diff, not merge-base).
2. Wave 2 (docs/MULTI-AGENT-PLAN.md §3): Settings (AppSettings 390 lines, normalizer, store, INV-11), catalog
   services, session store/writer, physical file system (INV-8), logs, instance forwarding. Needs contracts first
   (Contract Writer round, as for Wave 1) and the user's "chạy workflow wave 2".
3. Still open in Phase 0: UI spikes (R0001), COM Explorer spike.
4. Known follow-ups: UTF-16 BOM decode in `pr_perf::csv::decode_bytes` (W1-16 owns); hoist shared helpers
   (`get_extension`, `get_path_root`, `fmt_fixed`/`fmt_double`) into a lead-owned module (contracts D8).
