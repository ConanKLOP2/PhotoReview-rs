Status: evidence report (2026-11-03), lane mut-tiff-nav. Complete for the 52 mutants that [CL-MUT-TIFF4](CL-MUT-TIFF4-2026-11-01.md) did not run; PARITY status NOT raised.

# TIFF-NAV: the 52 `tiff_nav.rs` mutants that CL-MUT-TIFF4 left unrun

File `crates/pr-imaging-raw/src/tiff_nav.rs` (unchanged since the Wave 5 port, 320 mutants by `cargo mutants --list`). Tool: cargo-mutants 27.1.0.

## Which mutants were the 52

CL-MUT-TIFF4 ran `--shard 3/4` and stopped after 28 of its 80 mutants. cargo-mutants shards round-robin over the `--list` order (mutant index `n` is in shard `n mod 4`), so the 52 unrun are the indices `n` with `n mod 4 == 3` and `n div 4 >= 28` (index 115 to 319). That is a derivation, not a record: the old run's `mutants.out` is gone, and it assumes the old run went in `--list` order. The derived count is exactly 52, matching the review. They are spread over `try_read_single_strip` (4), `try_read_exif_pixel_dimensions` (6), `try_read_default_crop_size` (4), `has_square_default_scale` (8), `choose_active_sensor_size` (6), `read_tag_value` (1), `read_image_size` (2), `try_read_jpeg_interchange` (3), `starts_with_soi` (3), `reconcile_jpeg_size` (1), `clamp_to_int` (2), `inline_bytes` (1), `collect_items` (4), `exif_summary_extent` (7). The `-F` filter was one anchored regex per mutant name (52 alternatives; `--list -F` printed exactly 52).

## Result (real numbers, `tools/dev/mutants.sh`)

| Tested / total | Caught | Missed | Unviable | Timeouts | Not run |
|---|---|---|---|---|---|
| 52 / 52 | 49 | 3 | 0 | 0 | 0 |

Preflight (unmutated): `running 64 tests`, exit 0. Test bins `tiff_nav` and `mut_tiff4_killers` with `-- --skip golden --skip corpus` (same rule as CL-MUT-TIFF4: a golden-only catch does not count). Run took 8 minutes (about 8 s build + 3 s test per mutant), `-j 1`, `--timeout 90`, own target dir and TEMP under `.claude/tmp-mut-tiffnav`. The other 268 mutants of the file were NOT rerun here; their numbers are those of CL-MUT-TIFF4 (268 tested, 220 caught, 3 unviable, 45 missed of which 38 were killed and 7 equivalent).

## The 3 survivors

- `355:19` `<` -> `>` in `if span.len() < 16 { return Ok(false) }` of `has_square_default_scale`: equivalent. `read(offset, 16)` never returns more than 16 bytes; a shorter span makes `read_u32` return 0 for the missing words, and the zero check right after returns `false` as well. Same as the CL-MUT-TIFF4 survivor.
- `362:16` `||` -> `&&` in `hn == 0 || hd == 0 || ...`: equivalent. With `hn == 0, hd != 0` the final product test is `0 == vn*hd`, false when `vn, hd != 0`; with `hd == 0, hn != 0` it is `hn*vd == 0`, false. Same result `false`. Same as the CL-MUT-TIFF4 survivor.
- `513:15` `+` -> `-` in `(i + 1) * size > span_len` of `collect_items`: NOT equivalent, killable by a pure test. Normal sources return exactly the bytes asked for, so the guard never fires; only a SHORT read reaches it. Killer added: `unsigned_array_stops_at_the_first_item_that_does_not_fit_a_short_read` in `crates/pr-imaging-raw/tests/mut_tiff4_killers.rs` (a fake source that returns at most 6 bytes of a 16-byte LONG array; expects `[0x04030201]`). RED first: with the mutant applied by hand the test fails (`panicked at tiff_nav.rs:517:25`, slice past the end, 14 passed, 1 failed); on the real code `mut_tiff4_killers` is 15 passed. Rerun through `mutants.sh` with `-F` on this one mutant: 1 tested, 1 caught.

After this killer the 52 mutants are: 50 caught, 2 equivalent. Test-only diff, no production change.

## Command

```
TEMP=<repo>/.claude/tmp-mut-tiffnav MUT_OUT=$TEMP/out bash tools/dev/mutants.sh pr-imaging-raw \
  crates/pr-imaging-raw/src/tiff_nav.rs --tests tiff_nav,mut_tiff4_killers \
  --cargo-test-arg -- --cargo-test-arg --skip --cargo-test-arg golden --cargo-test-arg --skip --cargo-test-arg corpus \
  --budget 1800 --jobs 1 -F '<52 anchored mutant-name regexes joined by |>'
```

## NOT covered

No CR3, no real RAW corpus, no native code. The golden tests were skipped on purpose. The identity of the 52 rests on the shard-order derivation above.
