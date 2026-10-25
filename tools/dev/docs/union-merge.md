# union-merge: which shared files may use `merge=union`

Decision (replayed on this repo's history): **no file is union-merged yet**. `.gitattributes` only carries the comment that records this.
A union merge keeps both sides of a clash and NEVER reports a conflict, so a wrong result is silent.

Replay evidence (`git merge-file` with and without `--union`, 3-way on blobs; selftest proves the same behaviour in a temp repo):
- Real merge commits touching a file on both sides: only `docs/PARITY.md` (8 of 34 merges): union identical to the resolved result in 7, differs in 1
  (`e0b0d47c`, a real normal conflict). The other four files have 0 such merges (PRs are squash-merged, so no merge commits exist).
- Parallel replay of consecutive squash commits (second PR re-based onto the first PR's base), pairs / identical to the sequential result:
  PROPOSED-DECISIONS 27/21 (pure-append pairs 17/17; pairs with an in-place edit 10, 4 differ, 1 with a duplicated row),
  contracts/README 33/32 (24/24 pure append; 1 of 9 in-place differs), SYNC-LEDGER 4/2 (1 of 3 in-place differs, 1 duplicate),
  task_on_progress 61/39 (30/31 pure append; 21 of 30 in-place differ, 12 with a duplicate), PARITY 114/92 (11/11 pure append; 22 of 103 in-place differ).
- Real failure shapes: an edited row kept next to its old version (P-29 row duplicated; contracts rows `**Opus**` kept beside `Sonnet (P-34)`).

Why none: every file has in-place row edits (3 to 90 of its commits delete lines); where an edit meets another change nearby, union keeps the stale row.
Pure appends are always safe, so if the team stops editing rows in place the policy can be revisited.

To enable a file later: add `<path> merge=union` to `.gitattributes` and the path to the approved list in `tools/dev/test/union-merge-selftest.sh`.
Risk if enabled: always re-read the merged file (look for duplicated or stale rows); `docs/PARITY.md` must never be listed.
Check: `bash tools/dev/test/union-merge-selftest.sh` (needs real git; env `GIT_BIN` overrides the binary).
