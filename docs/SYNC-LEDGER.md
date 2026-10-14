# Sync ledger (C# → Rust)

**Current spec baseline:** C# `master` at `e441d67a` (2026-10-14), version line 2.0.N.
Assets synced from that commit: see `assets/SYNC-MANIFEST.txt`.

## How to advance the baseline

1. `git -C ../PhotoReview fetch` then `git -C ../PhotoReview log --oneline <baseline>..origin/master`.
2. Add one row per merged C# PR below. "Behaviour" = does it change what the user sees or what is written to disk.
3. Port behaviour-changing rows (or mark them N/A if the feature is not ported yet; the PARITY row then
   points at the newer C# code). Re-run `tools/sync-from-csharp.ps1` if languages/fixtures changed.
4. Move the baseline line above to the new commit.

| C# PR / commit | Summary | Behaviour? | Rust status | Rust PR |
|---|---|---|---|---|
| `e441d67a` (#327) | Baseline: whole-project review docs | No | Baseline | — |
