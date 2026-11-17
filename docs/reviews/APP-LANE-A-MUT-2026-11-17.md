Status: evidence report (2026-11-17). Mutation run of `pr-app` lane A (format, windowing, services, misc: 11 files) and the decision on every survivor. Measured on one machine (Windows 11), `--jobs 1`, `MUT_TIMEOUT=900`.

# `pr-app` lane A mutation run

## Result

1203 identities.

| Outcome | Count | Notes |
|---|---|---|
| Caught | 1115 | 1062 in run 1, 11 more by the whole-suite rerun, 42 by the new killer tests |
| Unviable | 67 | do not compile |
| Equivalent by code argument | 21 | argument per mutant below, no run proof |
| Timeout | 0 | |
| Missed with a real gap | 0 | |

## How it ran

1. Run 1 (selected test binaries): 1062 caught, 67 unviable, 73 missed, 1 timeout.
2. Whole-suite rerun of the 74 survivors, in two parts. The first attempt (2026-11-13) was cut by the end of the Windows session: its last 57 results were build failures with exit code `0xC0000142` and were discarded; only the results for `app_log.rs`, `file_hash.rs` and `viewport_convergence.rs` were kept. The other files were rerun on 2026-11-16 (56 mutants with siblings: 24 caught, 32 missed). Of the 74, 11 are caught by the whole suite (`app_log.rs` 7, `window_helpers.rs` 2, `preview_state.rs` 1, `viewport_size_source.rs` 1).
3. Killer tests, written and adversarially reviewed with hand-applied mutations.
4. Confirmation run on this branch (75 mutants at the survivor positions of the four files with new tests, whole suite, preflight 3304 tests): 71 caught, 1 unviable, 1 missed (`window_helpers.rs:79:31`, equivalent), 2 timeouts at `file_hash.rs:497` (line numbers after the seam, +14).
5. Rerun of `file_hash.rs:497` after renaming the killer file so that it runs first (whole suite, preflight 3304 tests): 2 of 2 caught, 0 timeout. See "The wait-loop timeouts".

`fullscreen.rs:93:51` `/` -> `*` was caught in run 1 but missed by the whole-suite rerun (not explained; possibly a timing-dependent kill). It now has a deterministic killer.

## New tests and seam

- `a_w8_lane_a_file_hash_killers.rs` (15 tests), `w8_lane_a_viewport_killers.rs` (6), `w8_lane_a_window_helpers_killers.rs` (7), `w8_lane_a_windowing_killers.rs`.
- Seam (P-408): `#[doc(hidden)] pub fn FileHashService::compute_and_cache_for_tests`, a one-line delegate. No other production change.

## Finding: dead duplicate in `file_hash.rs`

`FileHashService::compute_and_cache` and `complete_if_unchanged` have no production caller. `get` runs the free function `compute_and_cache_parts`, which duplicates their logic. 17 survivors were in the two dead methods; they are now killed through the seam, but that says nothing about the live path. Removing the duplicate is proposed as P-408 option B (not done here).

## The wait-loop timeouts

`file_hash.rs:483:42` (now 497:42, `&&` -> `||`) and its sibling 497:45 (delete `!`) make the waiter in `get` loop for ever when the token never fires. The killer tests are bounded (worker thread, 20 s), but the older unbounded tests in the `file_hash` binary hang first. The killer file is therefore named `a_w8_...` so that it runs, and fails, before `file_hash`. The kill depends on test-binary order: with a rename or `--no-fail-fast` these mutants time out again.

## Known limit

`file_hash.rs:476:25` is killed through the cancelled-flight branch only. The other effect of that mutant (removing a newer flight that replaced a finished one) needs a race that cannot be forced without a production hook.

## Equivalent by code argument (21)

Lines at master 47995dc9.

| Mutant | Argument |
|---|---|
| `fullscreen.rs:27:42/56/70/84` `\|` -> `^` (4) | `WS_CHROME` ORs 0x00C0_0000, 0x0004_0000, 0x0008_0000, 0x0002_0000, 0x0001_0000: pairwise disjoint bits |
| `fullscreen.rs:207:66/83`, `239:54/71` `\|` -> `^` (4) | SWP flags 0x0004, 0x0010, 0x0020: disjoint |
| `fullscreen.rs:244:28/41/56/73` `\|` -> `^` (4) | SWP flags 0x0002, 0x0001, 0x0004, 0x0010, 0x0020: disjoint |
| `visibility.rs:17:22` `\|\|` -> `&&` in `is_visible` | the early return only skips work: for an empty rect the intersection is empty or zero-sized, below the 80 px threshold, so the loop returns false as well |
| `window_helpers.rs:79:31` `>` -> `>=` in `normalize_image_point` | differs only at a zero size; then the division gives inf or NaN, the finite guard fails and the same centre is returned |
| `shortcut_key_name.rs:26:95`, `26:107` `\|\|` -> `&&` in `try_parse` | the closure only pre-rejects text starting with a digit, `-` or `+`; all 202 `KEY_ID_NAMES` start with an ASCII letter, so the lookup returns `None` anyway. Valid only while no key name starts with another character |
| `viewport_size_source.rs:102:36` `\|` -> `^` in `pack_box` | high word and zero-extended low word are disjoint |
| `context.rs:119:24` `>` -> `==`, `<`, `>=` in `decimal_digits_15` (3) | the leading digit of a nonzero `{:.14e}` value is 1-9 and zero returns early, so the changed conditions never fire; not trimming trailing zeros gives the same output in the only caller `format_custom_fixed` |
| `exif.rs:149:63` `>` -> `>=` in `shutter_text` | differs only if `abs(r - D) == D * 0.01` exactly in `f64` for an integer D in 2..10; checked with exact fractions, no `f64` satisfies it |

## Update 2026-11-17: dead duplicate removed (P-408 option B)

The two dead methods were deleted and the seam now calls the live `compute_and_cache_parts`; the 15 killer tests are unchanged and pass against live code. A reviewer hand-mutated four live conditions (staleness `||`, `!is_raw_path`, generation `==`, return value) and the killer binary failed for each. The live function has one check the deleted code lacked (cancel after the re-stat, before `cache.set`); C# has none, so no divergence was removed.

The 17 identities inside the deleted methods no longer exist, and line numbers in `file_hash.rs` moved by about -64. The `file_hash.rs` counts above (killed through the seam) therefore describe the code before the removal.

### Whole-file rerun on the new code (2026-11-18)

`cargo-mutants` over `crates/pr-app/src/misc/file_hash.rs` alone, whole `pr-app` suite, `--jobs 1`, `MUT_TIMEOUT=900`, master at `691c9b6b`: 59 identities, 57 caught, 2 unviable, 0 missed, 0 timeout. These counts replace the `file_hash.rs` counts above. The `476:25` limit and the `497` wait-loop note still apply (line numbers have shifted).
