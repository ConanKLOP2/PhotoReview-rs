# P7 mutation evidence: `wic/decoder.rs`, all 218 mutants run (2026-11-06)

Status: evidence report, task P7, worktree `mut7-wic-decoder`, branch `test/mut7-wic-decoder`, base `origin/master` 6b3492f4.
PARITY.md, README, BACKLOG, PROPOSED-DECISIONS, task_on_progress and SYNC-LEDGER are NOT edited. Production code unchanged: the only edit to
`crates/pr-platform-win/src/wic/decoder.rs` is ONE new test inside its existing `#[cfg(test)] mod tests` at the end of the file (cargo-mutants does not mutate
it, and every mutant line number is unchanged). Rule B (P-391, strict): a mutant is closed only if caught, unviable, or proven equivalent over the whole
domain on which mutant and original differ; a sampled or bounded run, or a reading of the code, closes nothing; a timeout is never caught; an infrastructure
failure is NOT RUN. Method as in P5: `tools/dev/mutants.sh`, `-j 1`, one cargo-mutants process, own target dir outside the worktree.

## Result in one paragraph

P3 had 80 of 218 mutants tested and 138 NOT RUN. All **218 now have a verdict**: **184 caught, 6 missed (all six equivalent, see below), 28 unviable,
0 timeouts, 0 NOT RUN** (`tested = 184 + 6 + 28 + 0 = 218`). `172:46` (open in P3 because of infrastructure failures) is decided: all three of its mutants
are caught. `232:38` (`&&` -> `||`) is EQUIVALENT: its case (b) cannot occur (E1); it cannot be killed, and one link of the argument is a short code reading,
so a strict verifier may keep it as "run-supported, not killable". `232:53` and `654:15` are proven by runs over the whole divergence set under the mutant.
Every other survivor of the first run was killed by a test or proven equivalent.

## Counts

| step | mutants | caught | missed | unviable | timeout | NOT RUN |
|---|---:|---:|---:|---:|---:|---:|
| Run 1: all 218, 9 integration test binaries | 218 | 176 | 14 | 28 | 0 | 0 |
| Run 2: 977:5, 1050:23, 1068:23, 1085:9, 1160:9 with the inline `wic::decoder::tests` (`--lib`) | 9 | 8 | 1 | 0 | 0 | 0 |
| Run 3: 1160:9 and 1085:9 with the NEW inline Debug test (`--lib`) | 3 | 3 | 0 | 0 | 0 | 0 |
| Run 4: the six equivalents, `MUT3_EXHAUSTIVE=1`, plus the new mut7 proofs (14 mutants matched by the position filters) | 14 | 8 | 6 | 0 | 0 | 0 |
| **Final, one verdict per mutant** | **218** | **184** | **6** | **28** | **0** | **0** |

Final = Run 1 (176 caught + 28 unviable) + 8 of its 14 survivors caught in Runs 2 and 3 (977:5 x2, 1050:23, 1068:23 x2, 1085:9 x2, 1160:9) + the 6 survivors that
stay missed in Run 4 and are equivalent. Runs 2 to 4 re-ran some already decided neighbours (the position filters select them too); the final row counts each mutant once.

Verdict check (a classifier script over every mutant log, kept with the lane notes, not in the repo): a "caught" needs a failing test in its log; an "unviable"
needs a rustc error that points into `crates/*/src`; a "missed" needs `test result: ok`; a log containing `paging file`, `LNK1xxx`, `E0463`, `E0786`,
`os error 1455`, `only metadata stub`, `memory allocation of` or an `0xc0000...` status would be NOT RUN. Result for all four runs: 0 suspect logs, 0 infrastructure
failures. The 28 unviable are `Default::default()` bodies for types without `Default`, `Com::new(..)` shapes that do not exist, and `654:26 / -> *` (rustc `this
arithmetic operation will overflow`).

## How it ran

Environment: `CARGO_TARGET_DIR=C:\Users\PC\Projects\target-mut7`, `TEMP=TMP=C:\Users\PC\Projects\target-mut7-tmp`, `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`,
`CARGO_PROFILE_DEV_DEBUG=0`; the script's own mutation target dir `target-mut7-tmp\mut-target-pr-platform-win`. cargo-mutants 27.1.0, `--baseline skip`,
`--timeout 300` (900 for Run 4), `-j 1`. `cargo mutants --list --file crates/pr-platform-win/src/wic/decoder.rs` gives 218 mutants.

Run 1 (22 minutes; 5 s build + 2 s test per mutant thanks to `--cargo-arg --test=`):

```
bash tools/dev/mutants.sh pr-platform-win crates/pr-platform-win/src/wic/decoder.rs \
 --cargo-arg --test=wic_decoder --cargo-arg --test=wic_decoder_buffers --cargo-arg --test=wic_decoder_native --cargo-arg --test=w4_mutation_killers2 \
 --cargo-arg --test=w4_mutation_killers3 --cargo-arg --test=mut3_wic_decoder_killers --cargo-arg --test=mut3_wic_decoder_proofs --cargo-arg --test=wic_cache_image \
 --cargo-arg --test=wic_interop \
 --cargo-test-arg -- --cargo-test-arg --include-ignored --cargo-test-arg --test-threads=1 \
 --cargo-test-arg --skip --cargo-test-arg encode_jpeg_and_decode_are_pinned_by_golden_hash \
 --cargo-test-arg --skip --cargo-test-arg wic_icc_transform_failure_carries_the_localized_sentence \
 --cargo-test-arg --skip --cargo-test-arg corrupt_files_fail_safely_without_crash \
 --cargo-test-arg --skip --cargo-test-arg golden_wic_direct_decode_matches_csharp --jobs 1 --budget 9000
```

Preflight: 246 tests executed, exit 0. The four skipped tests FAIL on the unmutated tree of this machine (verified by a plain `cargo test --no-fail-fast`: the
`wic_cache_image` golden hash, `wic_decoder::wic_icc_transform_failure_carries_the_localized_sentence` (OQ1), `wic_decoder_native::corrupt_files_fail_safely_without_crash`
and `golden_wic_direct_decode_matches_csharp`; Windows WIC build differences), so they cannot catch anything. Every other test of those binaries runs, `#[ignore]`d
native ones included (WIC only: no desktop, no window station, nothing shown).

Runs 2 and 3 (`--lib`, the inline unit tests of `decoder.rs`, which Run 1 did not include): `--cargo-arg --lib --cargo-test-arg -- --cargo-test-arg --include-ignored
--cargo-test-arg --test-threads=1 --cargo-test-arg wic::decoder::tests` with `-F 'decoder\.rs:<line>:<col>:'` for 977:5, 1050:23, 1068:23, 1085:9, 1160:9 (Run 3: 1160:9, 1085:9).
Preflight 8 tests (9 after the new one).

Run 4: Run 1's command plus `--cargo-arg --test=mut7_wic_decoder_proofs --cargo-arg --test=mut7_wic_decoder_orientation`, `MUT3_EXHAUSTIVE=1` in the environment (enables
the two all-`i32` proofs of `mut3_wic_decoder_proofs`, about 40 s per mutant) and `-F` filters for 232:38, 232:53, 654:15, 674:61, 803:75, 805:75. Every log of the
six missed mutants contains `every_i32_outside_two_to_eight_maps_to_rotate_zero ... ok` and `no_pair_of_i32_sides_multiplies_to_i64_max_over_four ... ok` (so the env gate
was open) and the mut7 tests passing. (After Run 4 one assertion in `mut7_wic_decoder_proofs` was turned into a `const { assert!(..) }` for clippy; same condition.)

## New tests (hand written, no golden, no production change, no attended desktop)

| File | Tests | Targets |
|---|---:|---|
| `crates/pr-platform-win/tests/mut7_wic_decoder_proofs.rs` | 3 (pure, no WIC) | E4 (a zero byte buffer is admitted before the negative check; the memory source panics if read), E5 (flip and rotate flag bits are disjoint), E2 (orientation 1 maps to Rotate0) |
| `crates/pr-platform-win/tests/mut7_wic_decoder_orientation.rs` | 1, `#[ignore = "native: needs WIC"]` (real WIC decode of the committed `orient-0..9` JPEG fixtures, bytes from memory) | E1 case (b): with `apply_orientation == false` and ten `source_orientation` classes (none, 0, 1, 2, 6, 8, 9, -1, `i32::MIN`, `i32::MAX`) every fixture keeps its stored size, reports orientation 1, original size unswapped, not downscaled; control: the same files rotate with `apply_orientation == true` (the tags are really in the files) |
| `crates/pr-platform-win/src/wic/decoder.rs`, `mod tests::color_transform_chain_debug_reports_whether_it_is_active` | 1 (inline, `#[cfg(test)]` only) | 1160:9 (`ColorTransformChain` is private; no file under `tests/` can reach its `Debug`) |

Run: `cargo test -p pr-platform-win --test mut7_wic_decoder_proofs --test mut7_wic_decoder_orientation -- --include-ignored --test-threads=1` (4 passed) and
`cargo test -p pr-platform-win --lib -- --include-ignored wic::decoder::tests` (9 passed).

## Decisions asked for

* **172:46** (`target_h < orig_h`): decided, CAUGHT, all three mutants. `<` -> `>` by `a_box_that_shrinks_only_one_axis_still_downscales` (the 1x100 strip with box 0x40 needs
  `target_h < orig_h`), `<` -> `==` by that test and `a_box_that_does_not_shrink_the_image_does_not_downscale`, `<` -> `<=` by the second. Log check clean; killer file
  `mut3_wic_decoder_killers`. P3's open item was an infrastructure failure, not a surviving mutant.
* **232:38**: E1 below. Case (b) was tried with a real WIC image per orientation tag; there is no seam to inject an orientation under `apply_orientation == false` and
  none is needed (the divergent input does not exist).
* **232:53** and **654:15**: E2 and E3 below.

## Equivalent mutants (the 6 survivors)

**E1 `232:38` `request.apply_orientation && orientation > 1` -> `||`.** The conditions differ only in (a) `apply_orientation && orientation <= 1` and
(b) `!apply_orientation && orientation > 1`.
(a) The mutant calls `map_to_transform_options(orientation)`; `Rotate0` comes back for every `i32` outside 2..=8 and the rotator is created only for `options != Rotate0`,
so nothing changes. RUN over all 2^32 - 7 values: `mut3_wic_decoder_proofs::every_i32_outside_two_to_eight_maps_to_rotate_zero` (passes under the mutant in Run 4).
(b) `orientation` is `given` only when `source_orientation` is `Some` AND `apply_orientation` is true; otherwise it is `read_frame_metadata(.., request.apply_orientation, ..).orientation`,
so with `apply_orientation == false` it is `read_metadata_values(.., false, ..).orientation`, which queries no tag and returns 1 (RUN for every frame:
`mut3_wic_decoder_proofs::without_apply_orientation_the_orientation_is_one_and_no_tag_is_queried`, a reader that panics when asked). The divergence set of (b) is empty:
no file and no request can make the mutant observable, so no test can kill it. `mut7_wic_decoder_orientation` runs the whole class (ten fixture tags times ten
`source_orientation` classes) through the real `decode_from_stream` with `apply_orientation == false` and checks the call-site consequence (stored size, `orientation() == 1`);
it passes under the mutant (Run 4) and would fail if the call site handed `true` to the reader. What stays a code reading: that `read_frame_metadata` is the only source of
`orientation` on the `_` arm of that `match` (visible in the 10 lines above line 232). Hence EQUIVALENT, with the caveat in the first paragraph.

**E2 `232:53` `orientation > 1` -> `>= 1`.** Differs only at `orientation == 1` with `apply_orientation`; `map_to_transform_options(1)` is `Rotate0`: no rotator, same output.
Whole divergence domain = {1}: `mut7_wic_decoder_proofs::orientation_one_maps_to_rotate_zero_so_the_rotator_branch_is_a_no_op`; the all-`i32` proof also passes under the mutant (Run 4). PROVEN.

**E3 `654:15` `pixels > i64::MAX / 4` -> `>=`.** Differs only for `width * height == 2^61 - 1`, a Mersenne prime, so only `1 x (2^61 - 1)` could factor it, and `2^61 - 1` does not
fit an `i32`. `mut3_wic_decoder_proofs::no_pair_of_i32_sides_multiplies_to_i64_max_over_four` loops over EVERY `i32` side `1..=i32::MAX` (zero and negative sides cannot give a
positive product) and passed under the mutant in Run 4. (It is a no-op without `MUT3_EXHAUSTIVE=1`, which is why it "survived" Run 1; P3 lacked this mutated run.) PROVEN.

**E4 `674:61` `buffer_length < 0` -> `<= 0` in `ensure_output_fits`.** The only differing value is 0, which never reaches the condition: 0 is inside `0..GUARD_THRESHOLD_BYTES`
(a positive constant) and returns `Ok(())` earlier. `mut7_wic_decoder_proofs::a_zero_byte_buffer_is_admitted_before_the_negative_check_and_without_reading_memory` runs
`ensure_output_fits(.., 0, ..)` for five side pairs with a memory source that panics when read; it passes under the mutant. The same test also kills `< -> ==` and `< -> >`
of that position in Run 4 (they were caught in Run 1 too). PROVEN (divergence domain {0}).

**E5 `803:75` and `805:75` `FlipHorizontal.0 | Rotate270.0` and `| Rotate90.0` -> `^`.** `a | b == a ^ b` exactly when `a & b == 0`; both operands are constants (8 with 3, and
8 with 1). `mut7_wic_decoder_proofs::the_combined_flip_and_rotate_flags_have_disjoint_bits` checks both pairs; the whole domain is those two points. PROVEN.

## Fate of every mutant (218)

Columns: line:col, mutation, function, fate, first killing tests (`(+n)` = n more; empty function = a constant of the file). "caught (rerun with lib tests)": missed by Run 1
(integration binaries only), killed by the inline unit test named, in Runs 2 and 3.

| Mutant | Mutation | Function | Fate | Killer |
|---|---|---|---|---|
| 64:51 | `* with +` |  | caught | decode_blank_path_rejects_before_open_and_nonblank_path_reaches_source_reader, source_stream_preserves_initial_cursor_across_length_and_seek_and_is_read_only |
| 64:51 | `* with /` |  | caught | decode_blank_path_rejects_before_open_and_nonblank_path_reaches_source_reader, source_stream_preserves_initial_cursor_across_length_and_seek_and_is_read_only |
| 67:52 | `* with +` |  | caught | source_stream_preserves_initial_cursor_across_length_and_seek_and_is_read_only |
| 67:52 | `* with /` |  | caught | source_stream_preserves_initial_cursor_across_length_and_seek_and_is_read_only |
| 140:9 | `-> Ok(Default)` | WicDirectDecoder::decode_from_stream | unviable | rustc: error[E0277]: the trait bound `DecodedImage: std::default::D |
| 153:28 | `match guard request.apply_orientation with false` | WicDirectDecoder::decode_from_stream | caught | all_decoders_source_orientation_override_wins_over_embedded_exif, decode_source_orientation_with_apply_orientation_rotates_and_swaps_the_original |
| 153:28 | `match guard request.apply_orientation with true` | WicDirectDecoder::decode_from_stream | caught | decode_source_orientation_without_apply_orientation_is_ignored |
| 159:52 | `&& with ||` | WicDirectDecoder::decode_from_stream | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 172:25 | `< with <=` | WicDirectDecoder::decode_from_stream | caught | a_box_that_does_not_shrink_the_image_does_not_downscale |
| 172:25 | `< with ==` | WicDirectDecoder::decode_from_stream | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 172:25 | `< with >` | WicDirectDecoder::decode_from_stream | caught | a_box_that_shrinks_only_one_axis_still_downscales |
| 172:34 | `|| with &&` | WicDirectDecoder::decode_from_stream | caught | a_box_that_shrinks_only_one_axis_still_downscales |
| 172:46 | `< with <=` | WicDirectDecoder::decode_from_stream | caught | a_box_that_does_not_shrink_the_image_does_not_downscale |
| 172:46 | `< with ==` | WicDirectDecoder::decode_from_stream | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 172:46 | `< with >` | WicDirectDecoder::decode_from_stream | caught | a_box_that_shrinks_only_one_axis_still_downscales |
| 232:38 | `&& with ||` | WicDirectDecoder::decode_from_stream | **missed, EQUIVALENT** (E1) | survives all tests (see section "Equivalent mutants") |
| 232:53 | `> with <` | WicDirectDecoder::decode_from_stream | caught | all_decoders_source_orientation_override_wins_over_embedded_exif, decode_applies_orientation_corners (+4) |
| 232:53 | `> with ==` | WicDirectDecoder::decode_from_stream | caught | all_decoders_source_orientation_override_wins_over_embedded_exif, decode_applies_orientation_corners (+4) |
| 232:53 | `> with >=` | WicDirectDecoder::decode_from_stream | **missed, EQUIVALENT** (E2) | survives all tests (see section "Equivalent mutants") |
| 234:24 | `!= with ==` | WicDirectDecoder::decode_from_stream | caught | all_decoders_source_orientation_override_wins_over_embedded_exif, decode_applies_orientation_corners (+4) |
| 316:9 | `-> Ok(Default)` | WicDirectDecoder::read_info_core | unviable | rustc: error[E0277]: the trait bound `ImageInfo: std::default::Defa |
| 329:9 | `-> Ok(Default)` | <impl fmt::Debug for WicDirectDecoder>::fmt | caught | decode_blank_path_rejects_before_open_and_nonblank_path_reaches_source_reader |
| 342:9 | `-> Ok(Arc::new(Default::default()))` | <impl ImageDecoder for WicDirectDecoder>::decode | unviable | rustc: error[E0790]: cannot call associated function on trait witho |
| 369:9 | `-> Ok(Default)` | <impl ImageDecoder for WicDirectDecoder>::read_info | unviable | rustc: error[E0277]: the trait bound `ImageInfo: std::default::Defa |
| 390:5 | `-> false` | is_blank_path | caught | decode_blank_path_rejects_before_open_and_nonblank_path_reaches_source_reader |
| 390:5 | `-> true` | is_blank_path | caught | decode_blank_path_rejects_before_open_and_nonblank_path_reaches_source_reader |
| 395:5 | `-> Default::default()` | blank_argument | unviable | rustc: error[E0277]: the trait bound `DecodeError: std::default::De |
| 438:9 | `-> Default::default()` | SourceStreamBackend::with_buffer_size | unviable | rustc: error[E0277]: the trait bound `SourceStreamBackend: std::def |
| 450:9 | `-> Ok(0)` | SourceStreamBackend::position | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 450:9 | `-> Ok(1)` | SourceStreamBackend::position | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 461:9 | `-> Vec::leak(Vec::new())` | SourceStreamBackend::buffered_from | caught | source_stream_backend_fault_after_a_partial_window_copy_reports_the_copied_bytes, source_stream_backend_reads_a_small_source_once_whatever_the_seeks |
| 461:9 | `-> Vec::leak(vec![0])` | SourceStreamBackend::buffered_from | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 461:9 | `-> Vec::leak(vec![1])` | SourceStreamBackend::buffered_from | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 470:9 | `-> Ok(())` | SourceStreamBackend::move_inner_to | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 470:32 | `!= with ==` | SourceStreamBackend::move_inner_to | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 480:9 | `-> Ok(0)` | SourceStreamBackend::read_inner_at | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 480:9 | `-> Ok(1)` | SourceStreamBackend::read_inner_at | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 483:53 | `+ with *` | SourceStreamBackend::read_inner_at | caught | source_stream_backend_direct_read_after_direct_read_returns_the_target_bytes |
| 483:53 | `+ with -` | SourceStreamBackend::read_inner_at | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 497:9 | `-> Ok(0)` | SourceStreamBackend::fetch | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 497:9 | `-> Ok(1)` | SourceStreamBackend::fetch | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 497:22 | `>= with <` | SourceStreamBackend::fetch | caught | source_stream_backend_direct_read_after_direct_read_returns_the_target_bytes, source_stream_backend_fault_after_a_partial_window_copy_reports_the_copied_bytes (+2) |
| 501:22 | `== with !=` | SourceStreamBackend::fetch | caught | source_stream_backend_fault_after_a_partial_window_copy_reports_the_copied_bytes, source_stream_backend_reads_a_small_source_once_whatever_the_seeks |
| 529:9 | `-> Ok(Default)` | <impl fmt::Debug for SourceStreamBackend>::fmt | caught | source_stream_preserves_initial_cursor_across_length_and_seek_and_is_read_only |
| 538:9 | `-> Ok(0)` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 538:9 | `-> Ok(1)` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 545:39 | `+ with *` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 545:39 | `+ with -` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 546:19 | `== with !=` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 551:35 | `+ with *` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 551:35 | `+ with -` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 553:47 | `+ with *` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 553:47 | `+ with -` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 553:57 | `+ with *` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 553:57 | `+ with -` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 554:27 | `+ with *` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 554:27 | `+ with -` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 556:23 | `match guard copied > 0 with false` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_backend_fault_after_a_partial_window_copy_reports_the_copied_bytes |
| 556:23 | `match guard copied > 0 with true` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_backend_fault_after_a_partial_window_copy_reports_the_copied_bytes |
| 556:30 | `> with <` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_backend_fault_after_a_partial_window_copy_reports_the_copied_bytes |
| 556:30 | `> with ==` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_backend_fault_after_a_partial_window_copy_reports_the_copied_bytes |
| 556:30 | `> with >=` | <impl StreamBackend for SourceStreamBackend>::read | caught | source_stream_backend_fault_after_a_partial_window_copy_reports_the_copied_bytes |
| 561:9 | `-> Ok(())` | <impl StreamBackend for SourceStreamBackend>::write_all | caught | source_stream_preserves_initial_cursor_across_length_and_seek_and_is_read_only |
| 566:9 | `-> Ok(0)` | <impl StreamBackend for SourceStreamBackend>::seek | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 566:9 | `-> Ok(1)` | <impl StreamBackend for SourceStreamBackend>::seek | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 568:70 | `+ with *` | <impl StreamBackend for SourceStreamBackend>::seek | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 568:70 | `+ with -` | <impl StreamBackend for SourceStreamBackend>::seek | caught | source_stream_backend_seek_before_begin_fails_and_keeps_the_position, source_stream_backend_seek_before_begin_is_the_filestream_hresult |
| 569:61 | `+ with *` | <impl StreamBackend for SourceStreamBackend>::seek | caught | source_stream_backend_reads_a_small_source_once_whatever_the_seeks |
| 569:61 | `+ with -` | <impl StreamBackend for SourceStreamBackend>::seek | caught | source_stream_backend_reads_a_small_source_once_whatever_the_seeks, source_stream_backend_seek_before_begin_fails_and_keeps_the_position (+1) |
| 572:27 | `match guard i64::try_from(target).is_ok() with false` | <impl StreamBackend for SourceStreamBackend>::seek | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 572:27 | `match guard i64::try_from(target).is_ok() with true` | <impl StreamBackend for SourceStreamBackend>::seek | caught | source_stream_backend_seek_before_begin_is_the_filestream_hresult, source_stream_backend_seek_past_i64_max_is_refused |
| 583:9 | `-> Ok(())` | <impl StreamBackend for SourceStreamBackend>::set_len | caught | source_stream_backend_len_keeps_position_and_is_read_only |
| 589:9 | `-> Ok(0)` | <impl StreamBackend for SourceStreamBackend>::len | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 589:9 | `-> Ok(1)` | <impl StreamBackend for SourceStreamBackend>::len | caught | source_stream_cached_reads_advance_position_and_cross_window_edges |
| 602:9 | `-> Default::default()` | <impl StreamBackend for SourceStreamBackend>::seek_before_begin_fault | unviable | rustc: error[E0277]: the trait bound `StreamFault: std::default::De |
| 605:9 | `-> Box::new(Default::default())` | <impl StreamBackend for SourceStreamBackend>::into_any | unviable | rustc: error[E0790]: cannot call associated function on trait witho |
| 620:5 | `-> Ok((-1, -1))` | to_int_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 620:5 | `-> Ok((-1, 0))` | to_int_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 620:5 | `-> Ok((-1, 1))` | to_int_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 620:5 | `-> Ok((0, -1))` | to_int_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 620:5 | `-> Ok((0, 0))` | to_int_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 620:5 | `-> Ok((0, 1))` | to_int_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 620:5 | `-> Ok((1, -1))` | to_int_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 620:5 | `-> Ok((1, 0))` | to_int_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 620:5 | `-> Ok((1, 1))` | to_int_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 621:9 | `delete match arm (Ok(w), Ok(h))` | to_int_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 632:5 | `-> Default::default()` | as_invalid_data | unviable | rustc: error[E0277]: the trait bound `DecodeError: std::default::De |
| 643:5 | `-> Default::default()` | relabel_invalid_argument | unviable | rustc: error[E0277]: the trait bound `DecodeError: std::default::De |
| 644:9 | `delete match arm DecodeError::InvalidArgument{message}` | relabel_invalid_argument | caught | wic_argument_hresult_preserves_invalid_argument_before_outer_relabel |
| 653:5 | `-> -1` | output_byte_length | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 653:5 | `-> 0` | output_byte_length | caught | golden_wic_direct_internals_matches_csharp, output_byte_length_saturates_instead_of_wrapping |
| 653:5 | `-> 1` | output_byte_length | caught | golden_wic_direct_internals_matches_csharp, output_byte_length_saturates_instead_of_wrapping |
| 653:35 | `* with +` | output_byte_length | caught | golden_wic_direct_internals_matches_csharp, output_byte_length_saturates_instead_of_wrapping |
| 653:35 | `* with /` | output_byte_length | caught | golden_wic_direct_internals_matches_csharp, output_byte_length_saturates_instead_of_wrapping |
| 654:15 | `> with <` | output_byte_length | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 654:15 | `> with ==` | output_byte_length | caught | golden_wic_direct_internals_matches_csharp, output_byte_length_saturates_instead_of_wrapping |
| 654:15 | `> with >=` | output_byte_length | **missed, EQUIVALENT** (E3) | survives all tests (see section "Equivalent mutants") |
| 654:26 | `/ with %` | output_byte_length | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 654:26 | `/ with *` | output_byte_length | unviable | rustc: error: this arithmetic operation will overflow |
| 654:57 | `* with +` | output_byte_length | caught | golden_wic_direct_internals_matches_csharp, output_byte_length_saturates_instead_of_wrapping |
| 654:57 | `* with /` | output_byte_length | caught | golden_wic_direct_internals_matches_csharp, output_byte_length_saturates_instead_of_wrapping |
| 671:5 | `-> Ok(())` | ensure_output_fits | caught | ensure_output_fits_beyond_int_range_is_admission_error, ensure_output_fits_buffer_of_exactly_the_guard_threshold_consults_the_memory_budget (+5) |
| 674:22 | `> with <` | ensure_output_fits | caught | ensure_output_fits_beyond_int_range_is_admission_error, ensure_output_fits_buffer_of_exactly_the_guard_threshold_consults_the_memory_budget (+2) |
| 674:22 | `> with ==` | ensure_output_fits | caught | ensure_output_fits_beyond_int_range_is_admission_error, ensure_output_fits_exactly_int_max_fits_when_there_is_room (+1) |
| 674:22 | `> with >=` | ensure_output_fits | caught | ensure_output_fits_exactly_int_max_fits_when_there_is_room, golden_wic_direct_internals_matches_csharp |
| 674:44 | `|| with &&` | ensure_output_fits | caught | ensure_output_fits_beyond_int_range_is_admission_error, ensure_output_fits_negative_wrapped_length_is_refused (+1) |
| 674:61 | `< with <=` | ensure_output_fits | **missed, EQUIVALENT** (E4) | survives all tests (see section "Equivalent mutants") |
| 674:61 | `< with ==` | ensure_output_fits | caught | ensure_output_fits_beyond_int_range_is_admission_error, ensure_output_fits_negative_wrapped_length_is_refused (+1) |
| 674:61 | `< with >` | ensure_output_fits | caught | ensure_output_fits_beyond_int_range_is_admission_error, ensure_output_fits_buffer_of_exactly_the_guard_threshold_consults_the_memory_budget (+4) |
| 689:5 | `-> Default::default()` | output_too_large | unviable | rustc: error[E0277]: the trait bound `DecodeError: std::default::De |
| 701:5 | `-> Ok(())` | copy_pixels_guarded | caught | copy_pixels_guarded_transform_active_reports_icc_failure_as_fallbackable, copy_pixels_guarded_transform_inactive_leaves_com_failure_untouched (+1) |
| 702:23 | `match guard color_transform_active && matches!(error.kind(), Decode...` | copy_pixels_guarded | caught | copy_pixels_guarded_transform_active_reports_icc_failure_as_fallbackable, golden_wic_direct_internals_matches_csharp |
| 702:23 | `match guard color_transform_active && matches!(error.kind(), Decode...` | copy_pixels_guarded | caught | copy_pixels_guarded_transform_inactive_leaves_com_failure_untouched, golden_wic_direct_internals_matches_csharp |
| 702:46 | `&& with ||` | copy_pixels_guarded | caught | copy_pixels_guarded_transform_inactive_leaves_com_failure_untouched, golden_wic_direct_internals_matches_csharp |
| 704:17 | `delete match arm DecodeError::Com{hresult, ..}` | copy_pixels_guarded | caught | copy_pixels_guarded_transform_active_reports_icc_failure_as_fallbackable, golden_wic_direct_internals_matches_csharp |
| 717:5 | `-> Default::default()` | icc_transform_failed | unviable | rustc: error[E0277]: the trait bound `DecodeError: std::default::De |
| 742:5 | `-> Ok(Default)` | allocate_and_fill | unviable | rustc: error[E0277]: the trait bound `PixelBuffer: std::default::De |
| 754:5 | `-> false` | is_opaque_format | caught | golden_wic_direct_internals_matches_csharp |
| 754:5 | `-> true` | is_opaque_format | caught | golden_wic_direct_internals_matches_csharp |
| 761:5 | `-> false` | native_reduction_is_useful | caught | native_reduction_codec_offer_is_useful_only_when_it_reduces_without_undershooting |
| 761:5 | `-> true` | native_reduction_is_useful | caught | native_reduction_codec_offer_is_useful_only_when_it_reduces_without_undershooting |
| 761:15 | `>= with <` | native_reduction_is_useful | caught | native_reduction_codec_offer_is_useful_only_when_it_reduces_without_undershooting |
| 761:27 | `&& with ||` | native_reduction_is_useful | caught | native_reduction_codec_offer_is_useful_only_when_it_reduces_without_undershooting |
| 761:40 | `>= with <` | native_reduction_is_useful | caught | native_reduction_codec_offer_is_useful_only_when_it_reduces_without_undershooting |
| 761:52 | `&& with ||` | native_reduction_is_useful | caught | native_reduction_codec_offer_is_useful_only_when_it_reduces_without_undershooting |
| 761:66 | `< with <=` | native_reduction_is_useful | caught | native_reduction_codec_offer_is_useful_only_when_it_reduces_without_undershooting |
| 761:66 | `< with ==` | native_reduction_is_useful | caught | native_reduction_codec_offer_is_useful_only_when_it_reduces_without_undershooting |
| 761:66 | `< with >` | native_reduction_is_useful | caught | native_reduction_codec_offer_is_useful_only_when_it_reduces_without_undershooting |
| 761:79 | `|| with &&` | native_reduction_is_useful | caught | native_reduction_codec_offer_is_useful_only_when_it_reduces_without_undershooting |
| 761:92 | `< with <=` | native_reduction_is_useful | caught | native_reduction_codec_offer_is_useful_only_when_it_reduces_without_undershooting |
| 761:92 | `< with ==` | native_reduction_is_useful | caught | native_reduction_codec_offer_is_useful_only_when_it_reduces_without_undershooting |
| 761:92 | `< with >` | native_reduction_is_useful | caught | native_reduction_codec_offer_is_useful_only_when_it_reduces_without_undershooting |
| 779:5 | `-> Ok(None)` | select_source_color_context | caught | select_source_color_context_embedded_profile_beats_any_exif_color_space, select_source_color_context_first_non_srgb_exif_color_space_wins_and_an_srgb_one_does_not_block_a_later_one (+2) |
| 779:5 | `-> Ok(Some(0))` | select_source_color_context | caught | select_source_color_context_embedded_profile_beats_any_exif_color_space, select_source_color_context_first_non_srgb_exif_color_space_wins_and_an_srgb_one_does_not_block_a_later_one (+4) |
| 779:5 | `-> Ok(Some(1))` | select_source_color_context | caught | select_source_color_context_embedded_profile_beats_any_exif_color_space, select_source_color_context_first_non_srgb_exif_color_space_wins_and_an_srgb_one_does_not_block_a_later_one (+4) |
| 783:17 | `== with !=` | select_source_color_context | caught | select_source_color_context_embedded_profile_beats_any_exif_color_space, select_source_color_context_first_non_srgb_exif_color_space_wins_and_an_srgb_one_does_not_block_a_later_one (+2) |
| 786:17 | `== with !=` | select_source_color_context | caught | select_source_color_context_first_non_srgb_exif_color_space_wins_and_an_srgb_one_does_not_block_a_later_one, select_source_color_context_non_srgb_exif_color_space_is_chosen (+2) |
| 787:13 | `&& with ||` | select_source_color_context | caught | select_source_color_context_first_non_srgb_exif_color_space_wins_and_an_srgb_one_does_not_block_a_later_one, select_source_color_context_srgb_exif_color_space_only_needs_no_transform (+1) |
| 788:13 | `&& with ||` | select_source_color_context | caught | select_source_color_context_first_non_srgb_exif_color_space_wins_and_an_srgb_one_does_not_block_a_later_one, select_source_color_context_srgb_exif_color_space_only_needs_no_transform (+1) |
| 788:43 | `!= with ==` | select_source_color_context | caught | select_source_color_context_first_non_srgb_exif_color_space_wins_and_an_srgb_one_does_not_block_a_later_one, select_source_color_context_non_srgb_exif_color_space_is_chosen (+2) |
| 799:5 | `-> Default::default()` | map_to_transform_options | caught | the_values_two_to_eight_are_the_only_rotations |
| 800:9 | `delete match arm 2` | map_to_transform_options | caught | the_values_two_to_eight_are_the_only_rotations |
| 801:9 | `delete match arm 3` | map_to_transform_options | caught | the_values_two_to_eight_are_the_only_rotations |
| 802:9 | `delete match arm 4` | map_to_transform_options | caught | the_values_two_to_eight_are_the_only_rotations |
| 803:9 | `delete match arm 5` | map_to_transform_options | caught | the_values_two_to_eight_are_the_only_rotations |
| 803:75 | `| with &` | map_to_transform_options | caught | the_values_two_to_eight_are_the_only_rotations |
| 803:75 | `| with ^` | map_to_transform_options | **missed, EQUIVALENT** (E5) | survives all tests (see section "Equivalent mutants") |
| 804:9 | `delete match arm 6` | map_to_transform_options | caught | the_values_two_to_eight_are_the_only_rotations |
| 805:9 | `delete match arm 7` | map_to_transform_options | caught | the_values_two_to_eight_are_the_only_rotations |
| 805:75 | `| with &` | map_to_transform_options | caught | the_values_two_to_eight_are_the_only_rotations |
| 805:75 | `| with ^` | map_to_transform_options | **missed, EQUIVALENT** (E5) | survives all tests (see section "Equivalent mutants") |
| 806:9 | `delete match arm 8` | map_to_transform_options | caught | the_values_two_to_eight_are_the_only_rotations |
| 822:5 | `-> Default::default()` | decode_error_from_wic | unviable | rustc: error[E0277]: the trait bound `DecodeError: std::default::De |
| 824:9 | `delete match arm DotnetExceptionKind::Com` | decode_error_from_wic | caught | decode_error_from_wic_not_initialized_factory_is_com_with_that_hresult |
| 828:9 | `delete match arm DotnetExceptionKind::Argument | DotnetExceptionKin...` | decode_error_from_wic | caught | wic_argument_hresult_preserves_invalid_argument_before_outer_relabel |
| 831:9 | `delete match arm DotnetExceptionKind::OutOfMemory` | decode_error_from_wic | caught | decode_error_from_wic_keeps_the_dotnet_kind_and_its_fallback_verdict |
| 832:9 | `delete match arm DotnetExceptionKind::InvalidCast` | decode_error_from_wic | caught | decode_error_from_wic_keeps_the_dotnet_kind_and_its_fallback_verdict |
| 833:9 | `delete match arm DotnetExceptionKind::NotSupported | DotnetExceptio...` | decode_error_from_wic | caught | decode_error_from_wic_keeps_the_dotnet_kind_and_its_fallback_verdict |
| 838:9 | `delete match arm DotnetExceptionKind::FileNotFound` | decode_error_from_wic | caught | decode_error_from_wic_every_arm_matches_the_dotnet_type |
| 839:9 | `delete match arm DotnetExceptionKind::DirectoryNotFound` | decode_error_from_wic | caught | decode_error_from_wic_every_arm_matches_the_dotnet_type |
| 840:9 | `delete match arm DotnetExceptionKind::Io | DotnetExceptionKind::End...` | decode_error_from_wic | caught | decode_error_from_wic_every_arm_matches_the_dotnet_type |
| 845:9 | `delete match arm DotnetExceptionKind::OperationCanceled` | decode_error_from_wic | caught | decode_error_from_wic_every_arm_matches_the_dotnet_type |
| 846:9 | `delete match arm DotnetExceptionKind::Overflow` | decode_error_from_wic | caught | decode_error_from_wic_every_arm_matches_the_dotnet_type |
| 847:9 | `delete match arm DotnetExceptionKind::Format` | decode_error_from_wic | caught | decode_error_from_wic_every_arm_matches_the_dotnet_type |
| 848:9 | `delete match arm DotnetExceptionKind::BadImageFormat` | decode_error_from_wic | caught | decode_error_from_wic_every_arm_matches_the_dotnet_type |
| 849:9 | `delete match arm DotnetExceptionKind::DllNotFound` | decode_error_from_wic | caught | decode_error_from_wic_every_arm_matches_the_dotnet_type |
| 850:9 | `delete match arm DotnetExceptionKind::EntryPointNotFound` | decode_error_from_wic | caught | decode_error_from_wic_every_arm_matches_the_dotnet_type |
| 851:9 | `delete match arm DotnetExceptionKind::InvalidOperation` | decode_error_from_wic | caught | decode_error_from_wic_every_arm_matches_the_dotnet_type |
| 852:9 | `delete match arm DotnetExceptionKind::ObjectDisposed` | decode_error_from_wic | caught | decode_error_from_wic_every_arm_matches_the_dotnet_type |
| 862:5 | `-> Default::default()` | decode_error_from_pixels | unviable | rustc: error[E0277]: the trait bound `DecodeError: std::default::De |
| 876:5 | `-> false` | is_color_metadata_fault | caught | read_color_contexts_count_query_fails_is_untagged_srgb, read_color_contexts_second_query_or_context_creation_fails_is_untagged_srgb |
| 876:5 | `-> true` | is_color_metadata_fault | caught | read_color_contexts_unexpected_fault_propagates |
| 885:5 | `-> Default::default()` | color_chain_failure | unviable | rustc: error[E0277]: the trait bound `DecodeError: std::default::De |
| 886:9 | `delete match arm DecodeError::Com{hresult, ..}` | color_chain_failure | caught | color_transform_chain_wic_fault_is_reported_as_icc_transform_failure |
| 887:9 | `delete match arm DecodeError::InvalidCast{..} | DecodeError::Invali...` | color_chain_failure | caught | color_transform_chain_wic_fault_is_reported_as_icc_transform_failure |
| 900:5 | `-> Default::default()` | com_error | unviable | rustc: error[E0277]: the trait bound `DecodeError: std::default::De |
| 905:5 | `-> Ok((0, 0))` | source_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 905:5 | `-> Ok((0, 1))` | source_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 905:5 | `-> Ok((1, 0))` | source_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 905:5 | `-> Ok((1, 1))` | source_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 914:5 | `-> Ok(Default)` | pixel_format | caught | all_decoders_source_orientation_override_wins_over_embedded_exif, bytes_win_over_the_path_label (+23) |
| 926:5 | `-> Ok(())` | scaler_initialize | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 937:5 | `-> Ok(())` | converter_initialize | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 957:5 | `-> Ok(())` | flip_rotator_initialize | caught | all_decoders_source_orientation_override_wins_over_embedded_exif, decode_and_read_info_read_each_source_byte_from_disk_once (+7) |
| 965:5 | `-> Ok(())` | copy_pixels | caught | all_decoders_source_orientation_override_wins_over_embedded_exif, bytes_win_over_the_path_label (+31) |
| 971:5 | `-> Ok(Default)` | context_type | caught | decode_exif_adobe_rgb_color_space_is_transformed_to_srgb_while_srgb_and_untagged_are_not, direct_wic_decodes_profiled_jpeg_for_path_and_memory (+6) |
| 977:5 | `-> Ok(0)` | exif_color_space | caught (rerun with lib tests) | exif_color_space_reads_the_context_value |
| 977:5 | `-> Ok(1)` | exif_color_space | caught (rerun with lib tests) | exif_color_space_reads_the_context_value |
| 989:5 | `-> Ok(None)` | try_get_native_reduced_size | caught | decode_box_against_stored_size_exact_size_downscaled_flag_and_original, decode_from_pre_read_bytes_same_decisions_as_from_the_file (+4) |
| 989:5 | `-> Ok(Some((0, 0)))` | try_get_native_reduced_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 989:5 | `-> Ok(Some((0, 1)))` | try_get_native_reduced_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 989:5 | `-> Ok(Some((1, 0)))` | try_get_native_reduced_size | caught | a_box_that_does_not_shrink_the_image_does_not_downscale, a_box_that_shrinks_only_one_axis_still_downscales |
| 989:5 | `-> Ok(Some((1, 1)))` | try_get_native_reduced_size | caught | decode_box_against_stored_size_exact_size_downscaled_flag_and_original, decode_exif_rotated_jpeg_box_applied_after_orientation (+6) |
| 1013:5 | `-> Ok(None)` | read_color_contexts | caught | decode_exif_adobe_rgb_color_space_is_transformed_to_srgb_while_srgb_and_untagged_are_not, direct_wic_decodes_profiled_jpeg_for_path_and_memory (+6) |
| 1013:5 | `-> Ok(Some(vec![None]))` | read_color_contexts | caught | decode_exif_adobe_rgb_color_space_is_transformed_to_srgb_while_srgb_and_untagged_are_not, direct_wic_decodes_profiled_jpeg_for_path_and_memory (+6) |
| 1013:5 | `-> Ok(Some(vec![Some(Com::from(Default::default()))]))` | read_color_contexts | unviable | rustc: error[E0277]: the trait bound `interop::Com<IWICColorContext |
| 1013:5 | `-> Ok(Some(vec![Some(Com::from_iter([Default::default()]))]))` | read_color_contexts | unviable | rustc: error[E0599]: no associated function or constant named `from |
| 1013:5 | `-> Ok(Some(vec![Some(Com::new())]))` | read_color_contexts | unviable | rustc: error[E0599]: no associated function or constant named `new` |
| 1013:5 | `-> Ok(Some(vec![Some(Com::new(Default::default()))]))` | read_color_contexts | unviable | rustc: error[E0599]: no associated function or constant named `new` |
| 1013:5 | `-> Ok(Some(vec![]))` | read_color_contexts | caught | decode_exif_adobe_rgb_color_space_is_transformed_to_srgb_while_srgb_and_untagged_are_not, direct_wic_decodes_profiled_jpeg_for_path_and_memory (+6) |
| 1048:5 | `-> Ok(None)` | read_color_contexts_with | caught | decode_exif_adobe_rgb_color_space_is_transformed_to_srgb_while_srgb_and_untagged_are_not, direct_wic_decodes_profiled_jpeg_for_path_and_memory (+6) |
| 1048:5 | `-> Ok(Some(vec![Default::default()]))` | read_color_contexts_with | unviable | rustc: error[E0277]: the trait bound `C: std::default::Default` is  |
| 1048:5 | `-> Ok(Some(vec![]))` | read_color_contexts_with | caught | decode_exif_adobe_rgb_color_space_is_transformed_to_srgb_while_srgb_and_untagged_are_not, direct_wic_decodes_profiled_jpeg_for_path_and_memory (+6) |
| 1050:23 | `match guard is_color_metadata_fault(&fault) with false` | read_color_contexts_with | caught | damaged_exif_tiff_header_still_decodes |
| 1050:23 | `match guard is_color_metadata_fault(&fault) with true` | read_color_contexts_with | caught (rerun with lib tests) | count_query_other_fault_propagates |
| 1053:14 | `== with !=` | read_color_contexts_with | caught | decode_exif_adobe_rgb_color_space_is_transformed_to_srgb_while_srgb_and_untagged_are_not, direct_wic_decodes_profiled_jpeg_for_path_and_memory (+6) |
| 1068:23 | `match guard is_color_metadata_fault(&fault) with false` | read_color_contexts_with | caught (rerun with lib tests) | creation_or_fill_metadata_fault_is_untagged |
| 1068:23 | `match guard is_color_metadata_fault(&fault) with true` | read_color_contexts_with | caught (rerun with lib tests) | creation_or_fill_other_fault_propagates |
| 1085:9 | `-> false` | ColorTransformChain::is_active | caught (rerun with lib tests) | color_chain_over_a_profiled_frame_is_active |
| 1085:9 | `-> true` | ColorTransformChain::is_active | caught (rerun with lib tests) | color_chain_over_a_profiled_frame_is_active |
| 1100:9 | `-> Ok(Default)` | ColorTransformChain::build | unviable | rustc: error[E0277]: the trait bound `windows::Win32::Graphics::Ima |
| 1112:9 | `-> Ok(Default)` | ColorTransformChain::build_inner | unviable | rustc: error[E0277]: the trait bound `windows::Win32::Graphics::Ima |
| 1155:5 | `-> Ok(())` | color_transform_initialize | caught | decode_exif_adobe_rgb_color_space_is_transformed_to_srgb_while_srgb_and_untagged_are_not, direct_wic_decodes_profiled_jpeg_for_path_and_memory (+6) |
| 1160:9 | `-> Ok(Default)` | <impl fmt::Debug for ColorTransformChain>::fmt | caught (rerun with lib tests) | color_transform_chain_debug_reports_whether_it_is_active (NEW) |
| 1183:9 | `-> Ok(Default)` | <impl FrameMetadata for FrameView<'_>>::metadata_query_reader | unviable | rustc: error[E0277]: the trait bound `QueryReaderView: std::default |
| 1191:9 | `-> Default::default()` | <impl MetadataQueryReader for QueryReaderView>::get_metadata_by_name | caught | decode_applies_orientation_corners, decode_exif_rotated_jpeg_box_applied_after_orientation (+13) |
| 1209:9 | `-> Ok(Default)` | <impl DecoderContainer for DecoderView<'_>>::container_format | unviable | rustc: error[E0277]: the trait bound `Guid: std::default::Default`  |
| 1216:5 | `-> Default::default()` | dotnet_guid | unviable | rustc: error[E0277]: the trait bound `Guid: std::default::Default`  |

## NOT RUN

None. All 218 listed mutants have a valid verdict (no infrastructure failure and no timeout in the four runs).

## Remarks

* Run 1 used integration test binaries only; `decoder.rs` also has inline unit tests that need `--lib` (a separate cargo-mutants invocation). A future run of this file should do both.
* The four tests skipped for failing on the unmutated tree stay red on this machine (Windows WIC build; not touched here).
* `232:38` (E1) rests on a short code reading for the call-site link; the other five equivalents are closed by runs over their whole divergence set.

## Addendum 2026-11-10: 232:38 removed by refactor

The condition `request.apply_orientation && orientation > 1` is now `let rotate_by = if request.apply_orientation { orientation } else { 1 }; if rotate_by > 1 { .. map_to_transform_options(rotate_by) .. }`. Identical behaviour: the local `orientation` (still stored in the result) is untouched, and `rotate_by` equals `orientation` when apply is true and 1 (no rotator) when false, exactly the old truth table. The `&&` -> `||` mutant (E1) no longer exists. cargo-mutants on lines 230-239 of `decoder.rs` (`-F 'decoder\.rs:23[0-9]:'`, `-j 1`, `--baseline skip`, tests wic_decoder, mut3/mut7 proofs and mut7 orientation with `--include-ignored`): 4 tested, 4 caught (233:22 `>` -> `==`, `<`, `>=`; 235:24 `!=` -> `==`), 0 missed, 0 unviable, 0 timeouts. NOT RUN: the other mutants of `decode_from_stream` (a first, wrongly scoped run covered 5 of 15 with default tests only and its misses on 153:28 and 159:52 are covered by the Run 1 to 4 evidence above, not rerun here); all other files.
