Status: evidence report (2026-11-13). First complete mutation run of the whole of `crates/pr-turbojpeg/src/decoder.rs`, with the DLL-gated native tests, and the decision on every survivor. Measured on one machine (Windows 11, pinned `turbojpeg.dll` staged by `tools/dev/stage-native.ps1`).

# `pr-turbojpeg` `decoder.rs` complete mutation run

TJ-PLAN-MUT (2026-11-11) covered only the pure `plan_geometry` function and said the whole file was not run. This run covers every identity of the file.

## Result

335 identities (`cargo mutants --list`).

| Outcome | Count | Notes |
|---|---|---|
| Caught | 301 | 295 in run 1, 4 more by the whole-crate rerun (run 2), 2 former timeouts caught after the fixes below (run 3) |
| Unviable | 25 | do not compile |
| Equivalent by code argument | 9 | no test can fail; argument per mutant below, no run proof |
| Timeout | 0 | |
| Missed with a real gap | 0 | |
| NOT RUN | 0 | |

A caught mutant has at least one failing test (none of the failing tests is a `golden_*` test in the new kills; the older kills include golden rows).

## How it ran

1. Run 1 (`tools/dev/mutants.sh pr-turbojpeg crates/pr-turbojpeg/src/decoder.rs --tests decoder_native,decoder_pure,mut2_n4_decode_plan,mut2_n4_decoder_killers,mut3_decoder_proofs,native,p393_load_file_seam,tj_plan_geometry,availability,availability_extra --native --jobs 1`, `MUT_TIMEOUT` 1500, preflight 229 tests): 335 tested in 5 h, 295 caught, 25 unviable, 13 missed, 2 timeout. A first attempt failed its preflight because `native/x64/turbojpeg.dll` was not staged; it is not used.
2. Run 2 (the 15 survivors, whole crate, `--native`, preflight 270 tests, 26 mutants including siblings at the same positions): 15 caught, 9 missed, 2 timeout. The four `reserve_growth` / `reserve_growth_with` survivors (720:5, 730:5, 730:30 twice) are caught by tests outside the first selection.
3. Run 3 (this branch, `-F 'decoder\.rs:492:22:'`, whole crate, `--native`, preflight 275 tests): 2 of 2 caught, 0 timeout. Run 2 on the same branch before the last fix had caught `661:27` and one of the two `492:22` mutants.

## The two timeouts

- `fill_buffer` 661:27, guard `error.kind() == Interrupted` -> `true`: every read error is retried for ever. The unbounded test `decoder_pure::repair::interrupted_reads_are_retried_and_other_errors_surface` hung the binary. Its `Flaky` reader now ends the stream after 1000 failing calls, so the mutated loop stops with a wrong answer and the unchanged assertion fails (8.5 s). New tests in `a_w8_tj_decoder_killers2.rs` also kill it: a non-`Interrupted` error is returned after exactly one call, and `Interrupted` is retried until the data arrives (bounded by a worker thread and a 20 s `recv_timeout`).
- `try_read_segment` 492:22, `2 + length` -> `2 - length`: a segment of length 2 does not advance the walk, so `scan_header` spins. Every real JPEG goes through this walk, so many tests in `decoder_native` reach it; bounding them one by one failed twice (the hang moved to the next test). `cargo test` stops at the first test binary that fails and runs binaries in name order, so the fail-fast killer file is named `a_w8_tj_decoder_killers2.rs` and runs before `decoder_native`. Its single-step test (payload lengths 0, 1, 2, 3, 14, 250) catches the mutant in milliseconds. The one native test that fed a length-2 segment, `decode_jpeg_without_a_frame_header_reports_invalid_dimensions_or_header_failure`, is also bounded to 20 s now.

The kill of 492:22 therefore depends on test-binary ordering. If binaries are renamed or run with `--no-fail-fast`, the mutant may time out again; the header of the killer file says so.

## Equivalent by code argument (9)

Lines at master 3faa31f6.

| Mutant | Argument |
|---|---|
| 172:9 `NativeDecompressors::shared` -> `Default::default()` | Single field `Option` that is `None` in both; proven by the Debug form in `mut2_n4_decoder_killers`. |
| 382:27 `calculate_output_buffer` `width > i64::MAX / 4` -> `>=` | Differs only at width = 2^61 - 1. Width is `scaled(i32, num i32, denom i32)`; with `denom >= 2` it would need `dim * num >= 2^62 - 3`, but the largest product is `2^62 - 2^32 + 1` (`(-2^31)^2 = 2^62` scales to another value); with `denom == 1` it needs a factor above `i32::MAX` because 2^61 - 1 is a Mersenne prime (checked in `mut3_decoder_proofs`). |
| 519:46 `be16` `\|` -> `^` | `hi << 8` and `lo < 256` have disjoint bits; all 65536 pairs and every 16-bit segment length are covered by `mut2_n4`. |
| 580:23, 580:55 `scan_header` early-break `\|\|` -> `&&` | `has_icc` and `found_exif` are only set when wanted, so each mutated clause is always false and the walk never stops early. The ICC and Exif branches are guarded by `!has_icc` / `!found_exif` and test different markers, so segments after the stop point cannot change the result. Only work differs. Checked exhaustively over about 137,000 segment sequences times four `want` pairs (`mut2_n4`). |
| 698:28 `grow_header_area` `len < length` -> `<=` | `limit <= length`, so the conjunct is redundant and `len == length` fails `len < limit` anyway. |
| 698:57 `len < limit` -> `<=` | At `len == limit` the new length equals the old one and the `new_len <= old_len` check breaks before any read or growth; an empty buffer never enters. |
| 1187:47 `apply_orientation && orientation > 1` -> `\|\|`, 1187:62 `>` -> `>=` (`FinishPlan.orient`) | `apply_orientation()` returns the same pixels for any orientation outside 2..=8, so `orient == true` for orientation 0 or 1 changes nothing observable. Pinned end to end by `orientation_outside_two_to_eight_hands_the_decoded_pixels_to_the_codec_untouched` (it passes under both mutants, as an equivalent must). These two rest on the behaviour of `pr_imaging::pixels::apply_orientation`; a change there would make them real. |

## Limits

- The 9 equivalents have no run proof beyond the exhaustive and brute-force tests named above; the two `1187` entries depend on another crate.
- DLL-gated tests are `#[ignore]` and ran only with `--include-ignored`; default `cargo test` does not run them.
- The corpus-gated RAW cases and the attended items of X-raw-native-turbojpeg are unchanged.
- No production code changed in this branch: no test seam was needed.
