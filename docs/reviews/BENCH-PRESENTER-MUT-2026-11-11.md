# bench-presenter mutation evidence (2026-11-11)

Unit: `crates/pr-app/src/presenters/benchmark.rs`. C# reference: `ApplyImageLimit` (first `limit` files when `limit > 0 && files.Count > limit`, else a copy).

## (a) `apply_image_limit`: DONE
- Old: `Ok(limit) if limit > 0 && files.len() > limit => files[..limit].to_vec()`. New: `Ok(limit) if limit > 0 => files.get(..limit).unwrap_or(files).to_vec()`. `get(..limit)` is `None` only when `limit > len`; at `len == limit` it is the whole list, same as before, so the `>` / `>=` comparison no longer exists.
- Differential test `apply_image_limit_matches_the_comparison_form_on_every_boundary` (mut3_benchmark_presenter_proofs.rs): old formula copied as a reference fn, len 0..=70 x limit -3..=75 plus i32::MIN, MIN+1, MAX-1, MAX. Existing N4/MUT3 tests still pass.
- Hand mutation `limit > 0` -> `limit >= 0` made the new test fail; reverted.
- cargo-mutants (`-F apply_image_limit`, tests mut3 + mut2_n4, budget 900, jobs 1): tested 8 / total 8, caught 8, missed 0, unviable 0, timeouts 0.

## (b) enumeration cancel poll (`seen % ENUMERATION_CANCELLATION_CHECK_INTERVAL == 0`): LEFT UNCHANGED
Reason: `CancelToken` is a concrete `Arc<AtomicBool>` struct with no poll hook, so a counting token needs a new production seam (trait or callback on `enumerate_image_files`/`scan`) and a signature change; the poll after the loop decides the result, so cadence is unobservable without it. Existing run-supported proofs stay.

## Proposed PARITY text
`benchmark.rs apply_image_limit`: rewritten to `files.get(..limit)` form (identical for every len/limit, differential test over boundary classes), the 390:51 comparison mutant no longer exists; 8/8 mutants caught. The enumeration poll cadence (832:14, 833:59, 833:17) stays run-supported: no counting-token seam without a signature change.
