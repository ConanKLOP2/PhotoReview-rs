Status: evidence report (2026-11-03), lane ar16-mut. Complete for the mutants of `ReviewCatalog::remove_paths`; PARITY status NOT raised.

# AR16 RemovePaths: mutation run of `ReviewCatalog::remove_paths`

Function: `crates/pr-core/src/catalog/review_catalog.rs` `ReviewCatalog::remove_paths` (lines 608-683). Proving tests: `remove_paths_*` (8 tests) plus golden `review-catalog` in `crates/pr-core/tests/catalog_review_catalog.rs`. Tool: cargo-mutants 27.1.0 (installed with `cargo install cargo-mutants --locked`; the repo pins no version).

## Result (real numbers, from `mutants.out/outcomes.json` via `tools/dev/mutants.sh`)

| Tested / total | Caught | Missed | Unviable | Timeouts | Not run |
|---|---|---|---|---|---|
| 24 / 24 | 20 | 3 | 1 | 0 | 0 |

Preflight (unmutated test command): `running 66 tests`, exit 0. The run took 4 minutes (about 10 s build + 0 s test per mutant), budget 900 s, `-j 1`. Mutants of other functions in the file were NOT run (filter `-F 'ReviewCatalog::remove_paths'`); this report says nothing about them.

## The 3 survivors (no test added; all equivalent, none killable)

- `:615:30` `||` -> `&&` in `if doomed.is_empty() || self.entries.is_empty() { return Vec::new() }`: with `&&` the early return is skipped when only one side is empty, but then no entry matches, `removed` stays empty and the later `if removed.is_empty() { return Vec::new() }` returns the same value without touching state. Equivalent (the guard is a shortcut).
- `:673:31` `<` -> `<=` in `else if old_current < 0 { 0 }`: for `old_current == 0` the next branch yields `next_current`, which is always 0 (the first kept entry has `i >= 0`). Same value. Equivalent.
- `:673:31` `<` -> `==`: differs only for `old_current == -1` with a non-empty catalog. No public path creates that state (`reset` sets 0, `set_current(-1)` only on an empty catalog, `remove`/`insert` keep `current >= 0` while entries exist), so a test cannot reach it. Unreachable-state equivalent (the branch is a defensive port of the C# `_currentIndex < 0` case).

## Commands (from the worktree root, `PY_BIN` = Python 3.12, `CARGO_BUILD_JOBS=3`)

```
TEMP=<repo>/.claude/tmp-mut-ar16 MUT_OUT=$TEMP/out bash tools/dev/mutants.sh pr-core \
  crates/pr-core/src/catalog/review_catalog.rs --tests catalog_review_catalog \
  --budget 900 --jobs 1 -F 'ReviewCatalog::remove_paths'
```

which runs `cargo test -p pr-core --test catalog_review_catalog` (preflight) and then `cargo mutants -p pr-core --baseline skip --timeout 90 -j 1 --file crates/pr-core/src/catalog/review_catalog.rs -F 'ReviewCatalog::remove_paths' --cargo-test-arg=--test --cargo-test-arg=catalog_review_catalog`, with its own `CARGO_TARGET_DIR` outside the worktree, `CARGO_INCREMENTAL=0`.

## Verdict for AR16

The `remove_paths` function body is mutation-clean (20 caught, 3 equivalent, 1 unviable). The AR16 row is still Partial: the other AR16 part (`enumerate_files_with_stat` in `physical.rs`) has its own open native mutation gap, and the change in the 29-30/10 window is still unconfirmed against the C# oracle (not on this machine).
