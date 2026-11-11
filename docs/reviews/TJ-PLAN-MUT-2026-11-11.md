# TJ-PLAN: FinishPlan size computation extracted and tested directly (2026-11-11)

Closes P2-MUT-2026-11-06 row 81 (mutant 1129:34, `scaled_width < orig_width`, RUN-SUPPORTED open).

## What changed

- `crates/pr-turbojpeg/src/decoder.rs`: new pure `plan_geometry(downscale, orig_w, orig_h, target_w, target_h, transposed, factor)
  -> Result<PlanGeometry, OversizedOutput>` and the `PlanGeometry` struct. It holds the `calculate_output_buffer` call and the
  `fine_scale`, `downscaled`, `original_width`, `original_height` expressions that used to sit inline in `decode_bytes`.
  The decoder now calls it and maps `Err` to the same "output dimensions are too large" refusal as before.
- All three disjuncts of `downscaled` are KEPT, expressions unchanged (no behaviour change).
- `crates/pr-turbojpeg/tests/tj_plan_geometry.rs` (4 tests, no DLL, no download, none ignored).

## Evidence

- Differential: the OLD inline expressions are copied into the test as `old_plan`; `new == old` over 21 x 21 source sides
  (boundary lattice: 1, 2, 3, 7, 8, 9, 15, 16, 17, 63, 64, 65, 4000, 32767, 32768, 46340, 46341, 65535, 65536, i32::MAX-1,
  i32::MAX) x the 8 supported factors x target lattice per side (about 26 values each, incl. 0 and side +-1) x downscale x
  transposed: over 1,000,000 combinations, including the `Err` (oversized) class.
- Production pipeline test (box -> `fit_stored` -> `select_scaling_factor` -> `plan_geometry`, also with the native-refusal fallback
  to 1/1): old == new, and the reachable-domain fact is checked: `scaled_width < orig_width` implies
  `target_width < orig_width || target_height < orig_height` (a factor other than 1/1 is only selected when a target side shrinks), so the first
  disjunct is redundant in production. It is nevertheless kept: as a pure function over all 8 factors it is observable
  (factor 1/2, target = source gives `downscaled == true` only through it), the direct tests catch its mutants, and dropping it
  would make the function differ from the old formula off the reachable domain.
- Hand mutation: removing `scaled_width < orig_width ||` from `plan_geometry` makes 2 of the 4 tests fail
  (`..._equals_the_old_inline_expressions...` and `downscaled_has_three_independent_causes...`); reverted.
- Mutation run (`tools/dev/mutants.sh pr-turbojpeg crates/pr-turbojpeg/src/decoder.rs --tests tj_plan_geometry --budget 900 --jobs 1 -F plan_geometry`,
  TEMP=.../mut-tj-plan): 21 tested / 21 found, 20 caught, 0 missed, 1 unviable (`replace plan_geometry -> ... with Ok(Default::default())`,
  `PlanGeometry` has no `Default`), 0 timeouts. Every mutant of the old 1129:34 expression (now line 878 col 30:
  `<`->`==`, `>`, `<=`) is caught.

## Not run

No DLL-gated test was run (they stay `#[ignore]`). Whole-file mutation of `decoder.rs` was not run (only the new function).
